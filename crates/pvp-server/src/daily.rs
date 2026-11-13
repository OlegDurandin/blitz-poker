//! Daily tasks consume only persisted authoritative engine outcomes, never local reports.
//! All functions run in the caller's SQLite transaction (including snapshot and match XP).
use crate::{
    progress::{self, Rewards},
    room::Room,
    table_room::{TableRoom, TableSeat},
};
use poker_core::{Phase, Seat};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

pub const SCHEMA: &str = "
CREATE TABLE xp_ledger (
 player_id TEXT NOT NULL REFERENCES players(id), reward_key TEXT NOT NULL,
 finished_ms INTEGER NOT NULL, bot INTEGER NOT NULL, requested_xp INTEGER NOT NULL,
 xp INTEGER NOT NULL, PRIMARY KEY(player_id,reward_key));
CREATE INDEX xp_day ON xp_ledger(player_id,finished_ms);
INSERT INTO xp_ledger SELECT player_id,'match:'||event_id,finished_ms,
 (bot_count>0 OR source='local'),xp,xp FROM player_results;
CREATE TABLE daily_challenge_sets (
 id TEXT PRIMARY KEY, player_id TEXT NOT NULL REFERENCES players(id), day_utc INTEGER NOT NULL,
 created_ms INTEGER NOT NULL, bonus_xp INTEGER NOT NULL, rewarded_ms INTEGER,
 UNIQUE(player_id,day_utc));
CREATE TABLE daily_challenge_items (
 id TEXT PRIMARY KEY, set_id TEXT NOT NULL REFERENCES daily_challenge_sets(id), slot INTEGER NOT NULL,
 template_id TEXT NOT NULL, template_version INTEGER NOT NULL, title TEXT NOT NULL,
 target INTEGER NOT NULL, progress INTEGER NOT NULL DEFAULT 0 CHECK(progress>=0 AND progress<=target),
 reward_xp INTEGER NOT NULL, bot_progress INTEGER NOT NULL DEFAULT 0, completed_ms INTEGER, rewarded_ms INTEGER,
 UNIQUE(set_id,slot), UNIQUE(set_id,template_id));
CREATE TABLE daily_challenge_events (
 player_id TEXT NOT NULL REFERENCES players(id), event_type TEXT NOT NULL,
 event_id TEXT NOT NULL, day_utc INTEGER NOT NULL, processed_ms INTEGER NOT NULL,
 PRIMARY KEY(player_id,event_type,event_id));
CREATE TABLE daily_baselines (match_id TEXT PRIMARY KEY, hand_number INTEGER NOT NULL);
INSERT OR IGNORE INTO daily_baselines
 SELECT json_extract(data,'$.game.match_id'),COALESCE((SELECT MAX(json_extract(value,'$.number')) FROM json_each(rooms.data,'$.game.m.history')),0)
 FROM rooms WHERE json_valid(data) AND json_extract(data,'$.game.match_id') IS NOT NULL;
INSERT OR IGNORE INTO daily_baselines
 SELECT json_extract(data,'$.game.match_id'),COALESCE((SELECT MAX(json_extract(value,'$.number')) FROM json_each(tables.data,'$.game.t.history')),0)
 FROM tables WHERE json_valid(data) AND json_extract(data,'$.game.match_id') IS NOT NULL;
PRAGMA user_version=4;
";

/// One XP policy and ledger for local/server matches and tasks. Zero awards also
/// consume their unique reward key: raising a cap never reissues an old reward.
pub fn award(
    c: &Connection,
    id: &str,
    key: &str,
    now: u64,
    bot: bool,
    desired: u64,
    r: &Rewards,
) -> rusqlite::Result<u64> {
    if let Some(xp) = c
        .query_row(
            "SELECT xp FROM xp_ledger WHERE player_id=?1 AND reward_key=?2",
            params![id, key],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
    {
        return Ok(xp as u64);
    }
    let day = now / 86_400_000;
    let (total,bots):(u64,u64)=c.query_row("SELECT COALESCE(SUM(xp),0),COALESCE(SUM(CASE WHEN bot=1 THEN xp ELSE 0 END),0) FROM xp_ledger WHERE player_id=?1 AND finished_ms>=?2 AND finished_ms<?3",params![id,(day*86_400_000) as i64,((day+1)*86_400_000) as i64],|r|Ok((r.get::<_,i64>(0)? as u64,r.get::<_,i64>(1)? as u64)))?;
    let mut xp = desired.min(r.daily.saturating_sub(total));
    if bot {
        xp = xp.min(r.bot_daily.saturating_sub(bots));
    }
    c.execute(
        "INSERT INTO xp_ledger VALUES (?1,?2,?3,?4,?5,?6)",
        params![id, key, now as i64, bot, desired as i64, xp as i64],
    )?;
    Ok(xp)
}

fn ensure(c: &Connection, id: &str, now: u64, r: &Rewards) -> rusqlite::Result<String> {
    let day = now / 86_400_000;
    let candidate = crate::ids::room_id();
    let created = c.execute("INSERT OR IGNORE INTO daily_challenge_sets(id,player_id,day_utc,created_ms,bonus_xp) VALUES (?1,?2,?3,?4,?5)",params![candidate,id,day as i64,now as i64,r.task_bonus as i64])?;
    let set: String = c.query_row(
        "SELECT id FROM daily_challenge_sets WHERE player_id=?1 AND day_utc=?2",
        params![id, day as i64],
        |r| r.get(0),
    )?;
    if created > 0 {
        for (slot, template, title, target, xp) in [
            (1, "play_matches_2", "Сыграй 2 партии", 2, r.task_matches),
            (2, "play_hands_6", "Сыграй 6 раздач", 6, r.task_hands),
            (3, "win_hands_2", "Выиграй 2 раздачи", 2, r.task_wins),
        ] {
            c.execute("INSERT INTO daily_challenge_items(id,set_id,slot,template_id,template_version,title,target,reward_xp) VALUES (?1,?2,?3,?4,1,?5,?6,?7)",params![crate::ids::room_id(),set,slot,template,title,target,xp as i64])?;
        }
    }
    Ok(set)
}

fn emit(c: &Connection, key: &str, kind: &str, payload: Value) -> rusqlite::Result<()> {
    progress::enqueue_event(c, key, kind, payload)
}

struct Participation {
    won: bool,
    bot: bool,
}

fn event(
    c: &Connection,
    id: &str,
    kind: &str,
    event_id: &str,
    participation: Participation,
    now: u64,
    r: &Rewards,
) -> rusqlite::Result<()> {
    let Participation { won, bot } = participation;
    if c.execute(
        "INSERT OR IGNORE INTO daily_challenge_events VALUES (?1,?2,?3,?4,?5)",
        params![id, kind, event_id, (now / 86_400_000) as i64, now as i64],
    )? == 0
    {
        return Ok(());
    }
    let set = ensure(c, id, now, r)?;
    let mut stmt = c.prepare("SELECT id,slot,progress,target,reward_xp,bot_progress FROM daily_challenge_items WHERE set_id=?1 ORDER BY slot")?;
    let items = stmt
        .query_map([&set], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, u8>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, bool>(5)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (item, slot, current, target, reward, prior_bot) in items {
        let applies = match slot {
            1 => kind == "match",
            2 => kind == "hand",
            _ => kind == "hand" && won,
        };
        if !applies || current >= target {
            continue;
        }
        c.execute(
            "UPDATE daily_challenge_items SET progress=progress+1,bot_progress=(bot_progress OR ?2) WHERE id=?1",
            params![item,bot],
        )?;
        emit(
            c,
            &format!("daily:progress:{id}:{kind}:{event_id}:{slot}"),
            "daily_challenge_progressed",
            json!({"player_id":id,"item_id":item,"progress":current+1}),
        )?;
        if current + 1 == target {
            let key = format!("daily_task:{item}");
            let xp = award(
                c,
                id,
                &key,
                now,
                bot || prior_bot,
                if r.tasks_enabled > 0 {
                    reward as u64
                } else {
                    0
                },
                r,
            )?;
            c.execute(
                "UPDATE daily_challenge_items SET completed_ms=?2,rewarded_ms=?2 WHERE id=?1",
                params![item, now as i64],
            )?;
            emit(
                c,
                &format!("{key}:completed"),
                "daily_challenge_completed",
                json!({"player_id":id,"item_id":item}),
            )?;
            emit(
                c,
                &key,
                "daily_challenge_rewarded",
                json!({"player_id":id,"item_id":item,"awarded_xp":xp}),
            )?;
        }
    }
    let (remaining,bonus,awarded,bonus_bot):(i64,u64,Option<i64>,bool)=c.query_row("SELECT (SELECT COUNT(*) FROM daily_challenge_items WHERE set_id=?1 AND completed_ms IS NULL),bonus_xp,rewarded_ms,(SELECT MAX(bot_progress) FROM daily_challenge_items WHERE set_id=?1) FROM daily_challenge_sets WHERE id=?1",[&set],|r|Ok((r.get(0)?,r.get::<_,i64>(1)? as u64,r.get(2)?,r.get(3)?)))?;
    if remaining == 0 && awarded.is_none() {
        let key = format!("daily_bonus:{set}");
        let xp = award(
            c,
            id,
            &key,
            now,
            bonus_bot,
            if r.tasks_enabled > 0 { bonus } else { 0 },
            r,
        )?;
        c.execute(
            "UPDATE daily_challenge_sets SET rewarded_ms=?2 WHERE id=?1",
            params![set, now as i64],
        )?;
        emit(
            c,
            &key,
            "daily_challenges_all_completed",
            json!({"player_id":id,"set_id":set,"awarded_xp":xp}),
        )?;
    }
    Ok(())
}

pub fn get(c: &Connection, id: &str, now: u64, r: &Rewards) -> rusqlite::Result<Value> {
    let set = ensure(c, id, now, r)?;
    let mut stmt=c.prepare("SELECT i.id,i.template_id,i.title,i.progress,i.target,i.reward_xp,i.rewarded_ms,COALESCE(x.xp,0),i.template_version,COALESCE(x.requested_xp,0) FROM daily_challenge_items i LEFT JOIN xp_ledger x ON x.player_id=?2 AND x.reward_key='daily_task:'||i.id WHERE i.set_id=?1 ORDER BY i.slot")?;
    let items=stmt.query_map(params![set,id],|r| {
        let reward:i64=r.get(5)?; let xp:i64=r.get(7)?; let done=r.get::<_,Option<i64>>(6)?.is_some();
        Ok(json!({"id":r.get::<_,String>(0)?,"template_id":r.get::<_,String>(1)?,"title":r.get::<_,String>(2)?,"progress":r.get::<_,i64>(3)?,"target":r.get::<_,i64>(4)?,
                    "reward_xp":reward,"awarded_xp":xp,"template_version":r.get::<_,i64>(8)?,"status":if done {"rewarded"} else {"in_progress"},"reward_reason":if done && xp<reward {if r.get::<_,i64>(9)?==0 {"disabled"} else {"xp_limit"}} else {"none"}}))
    })?.collect::<rusqlite::Result<Vec<_>>>()?;
    let completed = items.iter().filter(|v| v["status"] == "rewarded").count();
    let (reward,done,xp,requested):(i64,Option<i64>,i64,i64)=c.query_row("SELECT s.bonus_xp,s.rewarded_ms,COALESCE(x.xp,0),COALESCE(x.requested_xp,0) FROM daily_challenge_sets s LEFT JOIN xp_ledger x ON x.player_id=s.player_id AND x.reward_key='daily_bonus:'||s.id WHERE s.id=?1",[&set],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
    let (day,server,reset):(String,String,String)=c.query_row("SELECT date(?1/1000,'unixepoch'),strftime('%Y-%m-%dT%H:%M:%fZ',?1/1000.0,'unixepoch'),strftime('%Y-%m-%dT%H:%M:%SZ',(?1/86400000+1)*86400,'unixepoch')",[now as i64],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    let day_start = (now / 86_400_000 * 86_400_000) as i64;
    let (total,bots):(i64,i64)=c.query_row("SELECT COALESCE(SUM(xp),0),COALESCE(SUM(CASE WHEN bot=1 THEN xp ELSE 0 END),0) FROM xp_ledger WHERE player_id=?1 AND finished_ms>=?2 AND finished_ms<?3",params![id,day_start,day_start+86_400_000],|r|Ok((r.get(0)?,r.get(1)?)))?;
    let budget = json!({"total_cap":r.daily,"bot_cap":r.bot_daily,"awarded_today":total,"bot_awarded_today":bots,"remaining_total":r.daily.saturating_sub(total as u64),"remaining_bot":r.bot_daily.saturating_sub(bots as u64).min(r.daily.saturating_sub(total as u64))});
    Ok(
        json!({"day_utc":day,"server_now_utc":server,"next_reset_at":reset,"completed_count":completed,"total_count":items.len(),"items":items,"rewards_enabled":r.tasks_enabled>0,"xp_budget":budget,"completion_bonus":{"reward_xp":reward,"awarded_xp":xp,"status":if done.is_some() {"rewarded"} else {"locked"},"reward_reason":if done.is_some() && xp<reward {if requested==0 {"disabled"} else {"xp_limit"}} else {"none"}}}),
    )
}

fn baseline(c: &Connection, game: &str) -> rusqlite::Result<u8> {
    Ok(c.query_row(
        "SELECT hand_number FROM daily_baselines WHERE match_id=?1",
        [game],
        |r| r.get(0),
    )
    .optional()?
    .unwrap_or(0))
}
fn match_unprocessed(c: &Connection, game: &str) -> rusqlite::Result<bool> {
    Ok(c.query_row(
        "SELECT 1 FROM progress_commits WHERE match_id=?1",
        [game],
        |_| Ok(()),
    )
    .optional()?
    .is_none())
}

pub fn room(c: &Connection, room: &Room, now: u64, r: &Rewards) -> rusqlite::Result<()> {
    let Some(g) = &room.game else { return Ok(()) };
    let Some(guest) = &room.guest else {
        return Ok(());
    };
    let baseline = baseline(c, &g.match_id)?;
    for (seat, p) in [
        (Seat::Human, &room.host.profile),
        (Seat::Bot, &guest.profile),
    ] {
        let Some(id) = progress::identity(c, p, now)? else {
            continue;
        };
        for (index, h) in g.m.history.iter().enumerate() {
            if h.number <= baseline || h.holes.is_none() {
                continue;
            }
            event(
                c,
                &id,
                "hand",
                &format!("{}:{}", g.match_id, h.number),
                Participation {
                    won: h.outcome.winner == Some(seat),
                    bot: false,
                },
                g.hand_finished_ms.get(index).copied().unwrap_or(now),
                r,
            )?;
        }
        if g.m.phase == Phase::MatchOver
            && g.m.result.is_some()
            && match_unprocessed(c, &g.match_id)?
        {
            event(
                c,
                &id,
                "match",
                &g.match_id,
                Participation {
                    won: false,
                    bot: false,
                },
                g.finished_ms.unwrap_or(now),
                r,
            )?;
        }
    }
    Ok(())
}

pub fn table(c: &Connection, table: &TableRoom, now: u64, r: &Rewards) -> rusqlite::Result<()> {
    let Some(g) = &table.game else { return Ok(()) };
    let baseline = baseline(c, &g.match_id)?;
    let bot = table
        .seats
        .iter()
        .any(|s| matches!(s, TableSeat::Bot { .. }));
    for (seat, s) in table.seats.iter().enumerate() {
        let TableSeat::Player { member, .. } = s else {
            continue;
        };
        let Some(id) = progress::identity(c, &member.profile, now)? else {
            continue;
        };
        for (index, h) in g.t.history.iter().enumerate() {
            if h.number <= baseline || h.record_for(seat).hole.is_none() {
                continue;
            }
            // The engine's hands_won means sole winner of the main pot, including folds.
            event(
                c,
                &id,
                "hand",
                &format!("{}:{}", g.match_id, h.number),
                Participation {
                    won: h.outcome.winner() == Some(seat),
                    bot,
                },
                g.hand_finished_ms.get(index).copied().unwrap_or(now),
                r,
            )?;
        }
        if g.t.phase == Phase::MatchOver
            && g.t.result.is_some()
            && match_unprocessed(c, &g.match_id)?
        {
            event(
                c,
                &id,
                "match",
                &g.match_id,
                Participation { won: false, bot },
                g.finished_ms.unwrap_or(now),
                r,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "daily_tests.rs"]
mod tests;
