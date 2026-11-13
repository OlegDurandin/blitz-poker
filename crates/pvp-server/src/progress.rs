//! Personal progress. Immutable participant results are the statistics and XP ledger.
//! Local reports are explicitly distinguished from authoritative server results.
use crate::{
    ids,
    player::{Platform, PlayerId},
    room::{Profile, Room},
    table_room::{TableRoom, TableSeat},
};
use poker_core::{MatchOutcome, Phase};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const SCHEMA: &str = "
CREATE TABLE players (id TEXT PRIMARY KEY, created_ms INTEGER NOT NULL, name TEXT NOT NULL, photo TEXT);
CREATE TABLE platform_identities (
 provider TEXT NOT NULL, provider_user_id TEXT NOT NULL, player_id TEXT NOT NULL REFERENCES players(id),
 game_id INTEGER NOT NULL UNIQUE, PRIMARY KEY(provider, provider_user_id));
CREATE TABLE player_results (
 event_id TEXT NOT NULL, player_id TEXT NOT NULL REFERENCES players(id), match_id TEXT NOT NULL,
 source TEXT NOT NULL CHECK(source IN ('server','local')), mode TEXT NOT NULL,
 finished_ms INTEGER NOT NULL, bot_count INTEGER NOT NULL, outcome TEXT NOT NULL,
 xp INTEGER NOT NULL, data TEXT NOT NULL, PRIMARY KEY(event_id, player_id));
CREATE INDEX player_history ON player_results(player_id, finished_ms DESC, event_id DESC);
CREATE TABLE player_achievements (
 player_id TEXT NOT NULL REFERENCES players(id), key TEXT NOT NULL, unlocked_ms INTEGER NOT NULL,
 PRIMARY KEY(player_id,key));
CREATE TABLE progress_commits (match_id TEXT PRIMARY KEY);
CREATE TABLE progress_events (event_key TEXT PRIMARY KEY, kind TEXT NOT NULL, payload TEXT NOT NULL);
";

#[derive(Clone, Debug)]
pub struct Rewards {
    pub solo: u64,
    pub solo_win: u64,
    pub multiplayer: u64,
    pub multiplayer_win: u64,
    pub bot_daily: u64,
    pub daily: u64,
    pub task_matches: u64,
    pub task_hands: u64,
    pub task_wins: u64,
    pub task_bonus: u64,
    pub tasks_enabled: u64,
}
impl Default for Rewards {
    fn default() -> Self {
        Self {
            solo: 10,
            solo_win: 5,
            multiplayer: 20,
            multiplayer_win: 10,
            bot_daily: 100,
            daily: 300,
            task_matches: 20,
            task_hands: 20,
            task_wins: 30,
            task_bonus: 30,
            tasks_enabled: 1,
        }
    }
}
impl Rewards {
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(|name| match std::env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(_) => Err(format!("{name} must be valid UTF-8")),
        })
    }
    fn from_lookup(
        mut lookup: impl FnMut(&str) -> Result<Option<String>, String>,
    ) -> Result<Self, String> {
        let mut r = Self::default();
        for (name, value) in [
            ("PROFILE_XP_SOLO", &mut r.solo),
            ("PROFILE_XP_SOLO_WIN", &mut r.solo_win),
            ("PROFILE_XP_MULTIPLAYER", &mut r.multiplayer),
            ("PROFILE_XP_MULTIPLAYER_WIN", &mut r.multiplayer_win),
            ("PROFILE_XP_BOT_DAILY", &mut r.bot_daily),
            ("PROFILE_XP_DAILY", &mut r.daily),
            ("DAILY_XP_MATCHES", &mut r.task_matches),
            ("DAILY_XP_HANDS", &mut r.task_hands),
            ("DAILY_XP_WINS", &mut r.task_wins),
            ("DAILY_XP_BONUS", &mut r.task_bonus),
            ("DAILY_REWARDS_ENABLED", &mut r.tasks_enabled),
        ] {
            if let Some(s) = lookup(name)? {
                *value = s
                    .trim()
                    .parse()
                    .map_err(|_| format!("{name} must be a nonnegative integer"))?;
            }
            if *value > 1_000_000 {
                return Err(format!("{name} exceeds 1000000"));
            }
        }
        if r.tasks_enabled > 1 {
            return Err("DAILY_REWARDS_ENABLED must be 0 or 1".into());
        }
        Ok(r)
    }
    pub fn warnings(&self) -> Vec<&'static str> {
        let mut warnings = Vec::new();
        if self.bot_daily > self.daily {
            warnings.push(
                "PROFILE_XP_BOT_DAILY exceeds PROFILE_XP_DAILY; the overall cap still applies",
            );
        }
        let tasks = self.task_matches + self.task_hands + self.task_wins + self.task_bonus;
        if self.tasks_enabled > 0 && tasks > self.daily {
            warnings.push(
                "Nominal daily task XP exceeds the overall daily cap; rewards will be clipped",
            );
        }
        if self.tasks_enabled > 0 && tasks >= self.bot_daily && tasks > 0 {
            warnings.push("Daily tasks and match XP share the bot cap; the full nominal task reward is not guaranteed in Solo");
        }
        warnings
    }
}

pub fn level(xp: u64) -> Value {
    let (mut lo, mut hi) = (1u64, 1_000_000_000u64);
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let threshold = 100u128 * (mid - 1) as u128 * mid as u128 / 2;
        if threshold <= xp as u128 {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let n = lo;
    let floor = 100u128 * (n - 1) as u128 * n as u128 / 2;
    json!({"level":n,"xp_total":xp,"xp_current_level":xp as u128-floor,"xp_to_next_level":100*n})
}

pub fn identity(c: &Connection, p: &Profile, now: u64) -> rusqlite::Result<Option<String>> {
    if p.id.is_guest() {
        return Ok(None);
    }
    let provider = if p.id.platform() == Platform::Vk {
        "vk"
    } else {
        "telegram"
    };
    let external = if provider == "vk" {
        p.id.raw()
    } else {
        p.id.raw() & ((1 << 61) - 1)
    }
    .to_string();
    let existing: Option<String> = c
        .query_row(
            "SELECT player_id FROM platform_identities WHERE provider=?1 AND provider_user_id=?2",
            params![provider, external],
            |r| r.get(0),
        )
        .optional()?;
    let id = if let Some(id) = existing {
        id
    } else {
        let id = ids::room_id();
        c.execute(
            "INSERT INTO players VALUES (?1,?2,?3,?4)",
            params![id, now as i64, p.name, p.photo],
        )?;
        c.execute(
            "INSERT INTO platform_identities VALUES (?1,?2,?3,?4)",
            params![provider, external, id, p.id.raw() as i64],
        )?;
        id
    };
    c.execute(
        "UPDATE players SET name=?2,photo=?3 WHERE id=?1",
        params![id, p.name, p.photo],
    )?;
    Ok(Some(id))
}

pub fn player_id(c: &Connection, game: PlayerId) -> rusqlite::Result<Option<String>> {
    c.query_row(
        "SELECT player_id FROM platform_identities WHERE game_id=?1",
        [game.raw() as i64],
        |r| r.get(0),
    )
    .optional()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResultRow {
    pub event_id: String,
    pub match_id: String,
    pub source: String,
    pub mode: String,
    pub finished_ms: u64,
    pub human_count: usize,
    pub bot_count: usize,
    pub outcome: String,
    pub final_stack: u32,
    pub hands_played: u8,
    pub hands_won: u8,
    pub participant_hands: Option<u8>,
    pub best_hand: Option<Value>,
    pub opponents: Vec<String>,
    pub xp: u64,
    pub xp_limited: bool,
}

#[derive(Default, Serialize)]
pub struct Stats {
    matches_played: u64,
    wins: u64,
    losses: u64,
    draws: u64,
    hands_played: u64,
    hands_won: u64,
    best_hand: Option<Value>,
    current_win_streak: u64,
    best_win_streak: u64,
    win_rate: Option<f64>,
}
impl Stats {
    fn add(&mut self, r: &ResultRow) {
        self.matches_played += 1;
        self.hands_played += r.participant_hands.unwrap_or(r.hands_played) as u64;
        self.hands_won += r.hands_won as u64;
        if let Some(best) = &r.best_hand {
            let value = best["value"].as_u64().unwrap_or(0);
            if self
                .best_hand
                .as_ref()
                .is_none_or(|old| old["value"].as_u64().unwrap_or(0) < value)
            {
                self.best_hand = Some(best.clone());
            }
        }
        match r.outcome.as_str() {
            "win" => {
                self.wins += 1;
                self.current_win_streak += 1;
            }
            "loss" => {
                self.losses += 1;
                self.current_win_streak = 0;
            }
            _ => self.draws += 1,
        }
        self.best_win_streak = self.best_win_streak.max(self.current_win_streak);
        self.win_rate = Some(self.wins as f64 * 100. / self.matches_played as f64);
    }
}

pub fn insert(
    c: &Connection,
    id: &str,
    mut row: ResultRow,
    history: Value,
    rewards: &Rewards,
) -> rusqlite::Result<bool> {
    if c.query_row(
        "SELECT 1 FROM player_results WHERE event_id=?1 AND player_id=?2",
        params![row.event_id, id],
        |_| Ok(()),
    )
    .optional()?
    .is_some()
    {
        return Ok(false);
    }
    let desired = if row.mode == "solo" {
        rewards.solo
            + if row.outcome == "win" {
                rewards.solo_win
            } else {
                0
            }
    } else {
        rewards.multiplayer
            + if row.outcome == "win" {
                rewards.multiplayer_win
            } else {
                0
            }
    };
    row.xp = crate::daily::award(
        c,
        id,
        &format!("match:{}", row.event_id),
        row.finished_ms,
        row.bot_count > 0 || row.source == "local",
        desired,
        rewards,
    )?;
    row.xp_limited = row.xp < desired;
    let mut data = serde_json::to_value(&row).expect("result serializes");
    data["hands"] = history;
    c.execute(
        "INSERT INTO player_results VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            row.event_id,
            id,
            row.match_id,
            row.source,
            row.mode,
            row.finished_ms as i64,
            row.bot_count as i64,
            row.outcome,
            row.xp as i64,
            data.to_string()
        ],
    )?;
    let stats = stats(c, id, "all", "all")?;
    let all = &stats["all"];
    for (key, yes) in [
        ("first_match", true),
        ("first_win", all["wins"].as_u64().unwrap_or(0) > 0),
        (
            "ten_matches",
            all["matches_played"].as_u64().unwrap_or(0) >= 10,
        ),
        (
            "three_win_streak",
            all["best_win_streak"].as_u64().unwrap_or(0) >= 3,
        ),
    ] {
        if yes
            && c.execute(
                "INSERT OR IGNORE INTO player_achievements VALUES (?1,?2,?3)",
                params![id, key, row.finished_ms as i64],
            )? > 0
        {
            enqueue_event(
                c,
                &format!("achievement:{id}:{key}"),
                "achievement_unlocked",
                json!({"player_id":id,"achievement":key}),
            )?;
        }
    }
    let provider: String = c.query_row(
        "SELECT provider FROM platform_identities WHERE player_id=?1 LIMIT 1",
        [id],
        |r| r.get(0),
    )?;
    enqueue_event(
        c,
        &format!("completed:{}:{id}", row.event_id),
        "match_completed",
        json!({"player_id":id,"match_id":row.match_id,"source":row.source,"mode":row.mode,"human_count":row.human_count,"bot_count":row.bot_count,"outcome":row.outcome,"provider":provider}),
    )?;
    enqueue_event(
        c,
        &format!("xp:{}:{id}", row.event_id),
        "xp_awarded",
        json!({"player_id":id,"match_id":row.match_id,"delta":row.xp}),
    )?;
    Ok(true)
}

pub(crate) fn enqueue_event(
    c: &Connection,
    key: &str,
    kind: &str,
    payload: Value,
) -> rusqlite::Result<()> {
    c.execute(
        "INSERT OR IGNORE INTO progress_events VALUES (?1,?2,?3)",
        params![key, kind, payload.to_string()],
    )?;
    Ok(())
}
/// Called only outside the result transaction. A crash may redeliver a log with the
/// same event_key; a future analytics transport must deduplicate that key.
pub fn publish_events(c: &Connection) {
    let result = (|| -> rusqlite::Result<()> {
        let mut stmt = c.prepare("SELECT event_key,kind,payload FROM progress_events LIMIT 100")?;
        let events = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (key, kind, payload) in events {
            tracing::info!(
                event_key = key,
                event = kind,
                data = payload,
                "profile_event"
            );
            c.execute("DELETE FROM progress_events WHERE event_key=?1", [key])?;
        }
        Ok(())
    })();
    if let Err(e) = result {
        tracing::error!("profile event delivery deferred: {e}");
    }
}

pub fn stats(c: &Connection, id: &str, mode: &str, source: &str) -> rusqlite::Result<Value> {
    let mut stmt=c.prepare("SELECT data FROM player_results WHERE player_id=?1 AND (?2='all' OR mode=?2) AND (?3='all' OR source=?3) ORDER BY finished_ms,event_id")?;
    let rows = stmt.query_map(params![id, mode, source], |r| r.get::<_, String>(0))?;
    let mut buckets = [
        Stats::default(),
        Stats::default(),
        Stats::default(),
        Stats::default(),
    ];
    for r in rows {
        let r: ResultRow = serde_json::from_str(&r?).expect("stored result");
        buckets[0].add(&r);
        let i = match r.mode.as_str() {
            "solo" => 1,
            "duel" => 2,
            _ => 3,
        };
        buckets[i].add(&r);
    }
    Ok(json!({"all":buckets[0],"solo":buckets[1],"duel":buckets[2],"table":buckets[3]}))
}

#[derive(Default, Deserialize)]
pub struct HistoryQuery {
    pub mode: Option<String>,
    pub source: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}
impl HistoryQuery {
    pub fn valid(&self) -> bool {
        self.mode
            .as_deref()
            .is_none_or(|s| ["all", "solo", "duel", "table"].contains(&s))
            && self
                .source
                .as_deref()
                .is_none_or(|s| ["all", "server", "local"].contains(&s))
            && self.limit.is_none_or(|n| (1..=20).contains(&n))
            && self.cursor.as_ref().is_none_or(|s| {
                s.len() <= 200
                    && s.split_once(':')
                        .is_some_and(|(a, b)| a.parse::<u64>().is_ok() && !b.is_empty())
            })
    }
}
pub fn matches(c: &Connection, id: &str, q: &HistoryQuery) -> rusqlite::Result<Value> {
    let (time, event) = q
        .cursor
        .as_deref()
        .and_then(|s| s.split_once(':'))
        .map(|(a, b)| (a.parse::<i64>().unwrap_or(i64::MAX), b))
        .unwrap_or((i64::MAX, ""));
    let limit = q.limit.unwrap_or(20);
    let mut stmt=c.prepare("SELECT data FROM player_results WHERE player_id=?1 AND (?2='all' OR mode=?2) AND (?3='all' OR source=?3) AND (finished_ms<?4 OR (finished_ms=?4 AND event_id<?5)) ORDER BY finished_ms DESC,event_id DESC LIMIT ?6")?;
    let mut items = stmt
        .query_map(
            params![
                id,
                q.mode.as_deref().unwrap_or("all"),
                q.source.as_deref().unwrap_or("all"),
                time,
                event,
                (limit + 1) as i64
            ],
            |r| r.get::<_, String>(0),
        )?
        .map(|r| r.map(|s| serde_json::from_str::<Value>(&s).expect("stored result")))
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let more = items.len() > limit;
    items.truncate(limit);
    // Cards and full hand lists are only fetched through the authorized detail route.
    for item in &mut items {
        item.as_object_mut().unwrap().remove("hands");
    }
    let cursor = if more {
        items.last().map(|v| {
            format!(
                "{}:{}",
                v["finished_ms"].as_u64().unwrap(),
                v["event_id"].as_str().unwrap()
            )
        })
    } else {
        None
    };
    Ok(json!({"items":items,"next_cursor":cursor}))
}
pub fn detail(c: &Connection, id: &str, event: &str) -> rusqlite::Result<Option<Value>> {
    c.query_row(
        "SELECT data FROM player_results WHERE player_id=?1 AND event_id=?2",
        params![id, event],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .map(|s| s.map(|s| serde_json::from_str(&s).expect("stored result")))
}
pub fn profile(c: &Connection, p: &Profile, source: &str, now: u64) -> rusqlite::Result<Value> {
    let id = identity(c, p, now)?.expect("authenticated platform player");
    let (created, name, photo): (i64, String, Option<String>) = c.query_row(
        "SELECT created_ms,name,photo FROM players WHERE id=?1",
        [&id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let xp: i64 = c.query_row(
        "SELECT COALESCE(SUM(xp),0) FROM xp_ledger WHERE player_id=?1",
        [&id],
        |r| r.get(0),
    )?;
    let mut stmt=c.prepare("SELECT key,unlocked_ms FROM player_achievements WHERE player_id=?1 ORDER BY unlocked_ms,key")?;
    let achievements = stmt
        .query_map([&id], |r| {
            Ok(json!({"key":r.get::<_,String>(0)?,"unlocked_at":r.get::<_,i64>(1)?}))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(
        json!({"player_id":id,"display_name":name,"avatar_url":photo,"platform_badge":if p.id.platform()==Platform::Vk {"VK"} else {"TG"},"created_at":created,"progress":level(xp as u64),"statistics":stats(c,&id,"all",source)?,"achievements":achievements,"recent_matches":matches(c,&id,&HistoryQuery{source:Some(source.into()),..Default::default()})?}),
    )
}

fn outcome(o: MatchOutcome) -> String {
    match o {
        MatchOutcome::Win => "win",
        MatchOutcome::Loss => "loss",
        MatchOutcome::Draw => "draw",
    }
    .into()
}
pub fn room(c: &Connection, r: &Room, now: u64, rewards: &Rewards) -> rusqlite::Result<()> {
    let Some(g) = &r.game else { return Ok(()) };
    let now = g.finished_ms.unwrap_or(now);
    if g.m.phase != Phase::MatchOver {
        return Ok(());
    }
    let Some(result) = &g.m.result else {
        return Ok(());
    };
    let Some(guest) = &r.guest else { return Ok(()) };
    if c.execute(
        "INSERT OR IGNORE INTO progress_commits VALUES (?1)",
        [&g.match_id],
    )? == 0
    {
        return Ok(());
    }
    // Replace the old nontransactional pair-score event with the same transaction as the snapshot.
    let (host, other) = (r.host.profile.id.raw(), guest.profile.id.raw());
    let (a, b) = (host.min(other), host.max(other));
    let draw = result.outcome == MatchOutcome::Draw;
    let awin = if host == a {
        result.outcome == MatchOutcome::Win
    } else {
        result.outcome == MatchOutcome::Loss
    };
    c.execute("INSERT INTO duels VALUES (?1,?2,?3,?4,?5) ON CONFLICT(a,b) DO UPDATE SET a_wins=a_wins+?3,b_wins=b_wins+?4,draws=draws+?5",params![a as i64,b as i64,(awin&&!draw) as i64,(!awin&&!draw) as i64,draw as i64])?;
    for (seat, p, opponent) in [
        (poker_core::Seat::Human, &r.host.profile, &guest.profile),
        (poker_core::Seat::Bot, &guest.profile, &r.host.profile),
    ] {
        let Some(id) = identity(c, p, now)? else {
            continue;
        };
        let i = seat.idx();
        let o = if i == 0 {
            result.outcome
        } else {
            match result.outcome {
                MatchOutcome::Win => MatchOutcome::Loss,
                MatchOutcome::Loss => MatchOutcome::Win,
                MatchOutcome::Draw => MatchOutcome::Draw,
            }
        };
        insert(
            c,
            &id,
            ResultRow {
                event_id: format!("server:{}", g.match_id),
                match_id: g.match_id.clone(),
                source: "server".into(),
                mode: "duel".into(),
                finished_ms: now,
                human_count: 2,
                bot_count: 0,
                outcome: outcome(o),
                final_stack: result.final_stacks[i],
                hands_played: result.hands_played,
                hands_won: result.hands_won[i],
                participant_hands: None,
                best_hand: if i == 0 {
                    result.best_hand
                } else {
                    result.opponent_best_hand
                }
                .map(|v| json!(v)),
                opponents: vec![opponent.name.clone()],
                xp: 0,
                xp_limited: false,
            },
            json!(g.m.hand_records(seat, false)),
            rewards,
        )?;
    }
    Ok(())
}

pub fn table(c: &Connection, t: &TableRoom, now: u64, rewards: &Rewards) -> rusqlite::Result<()> {
    let Some(g) = &t.game else { return Ok(()) };
    let now = g.finished_ms.unwrap_or(now);
    if g.t.phase != Phase::MatchOver {
        return Ok(());
    }
    let Some(r) = &g.t.result else { return Ok(()) };
    if c.execute(
        "INSERT OR IGNORE INTO progress_commits VALUES (?1)",
        [&g.match_id],
    )? == 0
    {
        return Ok(());
    }
    let humans = t
        .seats
        .iter()
        .filter(|s| matches!(s, TableSeat::Player { .. }))
        .count();
    let bots = t.seats.len() - humans;
    let mode = if t.seats.len() >= 3 {
        "table"
    } else if bots > 0 {
        "solo"
    } else {
        "duel"
    };
    let names: Vec<String> = t
        .seats
        .iter()
        .map(|s| match s {
            TableSeat::Player { member, .. } => member.profile.name.clone(),
            TableSeat::Bot { opponent } => format!("Бот {opponent:?}"),
            _ => String::new(),
        })
        .collect();
    for (i, s) in t.seats.iter().enumerate() {
        if let TableSeat::Player { member, .. } = s {
            let Some(id) = identity(c, &member.profile, now)? else {
                continue;
            };
            let o = if r.places[i] == 1 {
                if r.places.iter().filter(|&&p| p == 1).count() > 1 {
                    "draw"
                } else {
                    "win"
                }
            } else {
                "loss"
            };
            insert(
                c,
                &id,
                ResultRow {
                    event_id: format!("server:{}", g.match_id),
                    match_id: g.match_id.clone(),
                    source: "server".into(),
                    mode: mode.into(),
                    finished_ms: now,
                    human_count: humans,
                    bot_count: bots,
                    outcome: o.into(),
                    final_stack: r.final_stacks[i],
                    hands_played: r.hands_played,
                    hands_won: r.hands_won[i],
                    participant_hands: Some(
                        g.t.history
                            .iter()
                            .filter(|h| h.record_for(i).hole.is_some())
                            .count() as u8,
                    ),
                    best_hand: r.best_hands[i].map(|v| json!(v)),
                    opponents: names
                        .iter()
                        .enumerate()
                        .filter(|(j, _)| *j != i)
                        .map(|(_, s)| s.clone())
                        .collect(),
                    xp: 0,
                    xp_limited: false,
                },
                json!(
                    g.t.history
                        .iter()
                        .map(|h| h.record_for(i))
                        .collect::<Vec<_>>()
                ),
                rewards,
            )?;
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalReport {
    pub match_id: String,
    pub mode: String,
    pub outcome: String,
    pub final_stack: u32,
    pub hands_played: u8,
    pub hands_won: u8,
    pub opponents: Vec<String>,
}
impl LocalReport {
    pub fn valid(&self) -> bool {
        (8..=64).contains(&self.match_id.len())
            && self
                .match_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
            && ["solo", "table"].contains(&self.mode.as_str())
            && ["win", "loss", "draw"].contains(&self.outcome.as_str())
            && (1..=48).contains(&self.hands_played)
            && self.hands_won <= self.hands_played
            && self.final_stack <= 2400
            && (if self.mode == "solo" {
                self.opponents.len() == 1
            } else {
                (2..=3).contains(&self.opponents.len())
            })
            && self.opponents.iter().all(|s| {
                !s.trim().is_empty() && s.chars().count() <= 40 && !s.chars().any(char::is_control)
            })
    }
}
pub fn local(
    c: &Connection,
    p: &Profile,
    r: &LocalReport,
    now: u64,
    rewards: &Rewards,
) -> rusqlite::Result<Value> {
    let id = identity(c, p, now)?.expect("platform identity");
    let event = format!("local:{}:{}", id, r.match_id);
    insert(
        c,
        &id,
        ResultRow {
            event_id: event.clone(),
            match_id: r.match_id.clone(),
            source: "local".into(),
            mode: r.mode.clone(),
            finished_ms: now,
            human_count: 1,
            bot_count: r.opponents.len(),
            outcome: r.outcome.clone(),
            final_stack: r.final_stack,
            hands_played: r.hands_played,
            hands_won: r.hands_won,
            participant_hands: None,
            best_hand: None,
            opponents: r.opponents.clone(),
            xp: 0,
            xp_limited: false,
        },
        Value::Null,
        rewards,
    )?;
    Ok(detail(c, &id, &event)?.expect("just inserted"))
}

#[cfg(test)]
#[path = "progress_tests.rs"]
mod tests;
