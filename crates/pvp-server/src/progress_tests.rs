use super::*;
use crate::{
    room::{OsDealer, Room, Timings},
    store::Store,
    table_room::TableRoom,
};
use poker_core::{Opponent, PlayerAction};

fn who(id: u64) -> Profile {
    Profile {
        id: PlayerId::vk(id).unwrap(),
        name: format!("Игрок {id}"),
        photo: None,
    }
}
fn row(n: u64, o: &str, now: u64) -> ResultRow {
    ResultRow {
        event_id: format!("server:{n}"),
        match_id: n.to_string(),
        source: "server".into(),
        mode: "duel".into(),
        finished_ms: now,
        human_count: 2,
        bot_count: 0,
        outcome: o.into(),
        final_stack: 600,
        hands_played: 8,
        hands_won: 2,
        participant_hands: None,
        best_hand: None,
        opponents: vec!["Соперник".into()],
        xp: 0,
        xp_limited: false,
    }
}
fn report(id: &str) -> LocalReport {
    LocalReport {
        match_id: id.into(),
        mode: "solo".into(),
        outcome: "win".into(),
        final_stack: 700,
        hands_played: 8,
        hands_won: 4,
        opponents: vec!["Бот Алекс".into()],
    }
}

#[test]
fn xp_boundaries_and_large_values() {
    for (xp, n, current) in [
        (0, 1, 0),
        (99, 1, 99),
        (100, 2, 0),
        (299, 2, 199),
        (300, 3, 0),
        (599, 3, 299),
        (600, 4, 0),
    ] {
        let p = level(xp);
        assert_eq!(p["level"], n);
        assert_eq!(p["xp_current_level"], current);
    }
    assert!(level(u64::MAX)["level"].as_u64().unwrap() > 1);
}
#[test]
fn statistics_draw_streak_and_achievement_idempotency() {
    let s = Store::open(":memory:").unwrap();
    let p = who(1);
    let id = identity(&s.conn, &p, 0).unwrap().unwrap();
    assert!(stats(&s.conn, &id, "all", "all").unwrap()["all"]["win_rate"].is_null());
    for (i, o) in ["win", "win", "draw", "win", "loss"].iter().enumerate() {
        assert!(
            insert(
                &s.conn,
                &id,
                row(i as u64, o, i as u64),
                Value::Null,
                &s.rewards
            )
            .unwrap()
        );
    }
    assert!(!insert(&s.conn, &id, row(0, "win", 999), Value::Null, &s.rewards).unwrap());
    let v = profile(&s.conn, &p, "all", 1000).unwrap();
    assert_eq!(v["statistics"]["all"]["matches_played"], 5);
    assert_eq!(v["statistics"]["all"]["win_rate"], 60.);
    assert_eq!(v["statistics"]["all"]["current_win_streak"], 0);
    assert_eq!(v["statistics"]["all"]["best_win_streak"], 3);
    assert_eq!(v["progress"]["xp_total"], 130);
    assert_eq!(v["achievements"].as_array().unwrap().len(), 3);
}
#[test]
fn local_duplicates_caps_utc_day_and_platform_isolation() {
    let s = Store::open(":memory:").unwrap();
    let p = who(42);
    let mut tg = who(42);
    tg.id = PlayerId::telegram(42).unwrap();
    let vk_id = identity(&s.conn, &p, 0).unwrap().unwrap();
    let tg_id = identity(&s.conn, &tg, 0).unwrap().unwrap();
    assert_ne!(vk_id, tg_id);
    let mut sum = 0;
    for n in 0..10 {
        let r = local(&s.conn, &p, &report(&format!("local{n:04}")), 0, &s.rewards).unwrap();
        sum += r["xp"].as_u64().unwrap();
    }
    assert_eq!(sum, 100);
    let same = local(&s.conn, &p, &report("local0000"), 86_400_000, &s.rewards).unwrap();
    assert_eq!(same["finished_ms"], 0);
    assert_eq!(same["xp"], 15);
    let next = local(&s.conn, &p, &report("next-day"), 86_400_000, &s.rewards).unwrap();
    assert_eq!(next["xp"], 15);
    assert!(
        detail(&s.conn, &tg_id, same["event_id"].as_str().unwrap())
            .unwrap()
            .is_none()
    );
    assert_eq!(
        profile(&s.conn, &tg, "all", 0).unwrap()["statistics"]["all"]["matches_played"],
        0
    );
    for n in 0..20 {
        insert(
            &s.conn,
            &vk_id,
            row(n, "win", 86_400_000),
            Value::Null,
            &s.rewards,
        )
        .unwrap();
    }
    assert_eq!(
        profile(&s.conn, &p, "all", 0).unwrap()["progress"]["xp_total"],
        400
    );
    assert_eq!(
        stats(&s.conn, &vk_id, "all", "local").unwrap()["all"]["matches_played"],
        11
    );
}
#[test]
fn cursor_ties_do_not_duplicate_or_skip_and_details_are_private() {
    let s = Store::open(":memory:").unwrap();
    let id = identity(&s.conn, &who(1), 0).unwrap().unwrap();
    for n in 0..45 {
        insert(
            &s.conn,
            &id,
            row(n, "draw", 10),
            json!([{"hole":["As","Ks"]}]),
            &s.rewards,
        )
        .unwrap();
    }
    let mut q = HistoryQuery::default();
    let mut ids = std::collections::HashSet::new();
    let mut sizes = vec![];
    loop {
        let p = matches(&s.conn, &id, &q).unwrap();
        sizes.push(p["items"].as_array().unwrap().len());
        for r in p["items"].as_array().unwrap() {
            assert!(r.get("hands").is_none());
            assert!(ids.insert(r["event_id"].as_str().unwrap().to_string()));
        }
        q.cursor = p["next_cursor"].as_str().map(str::to_string);
        if q.cursor.is_none() {
            break;
        }
    }
    assert_eq!(sizes, vec![20, 20, 5]);
    assert_eq!(ids.len(), 45);
    assert!(
        detail(&s.conn, "another-account", "server:1")
            .unwrap()
            .is_none()
    );
}
fn duel() -> Room {
    let mut d = OsDealer;
    let a = who(1);
    let b = who(2);
    let mut r = Room::new(ids::room_id(), a.clone(), "q".into(), 0);
    r.join(b.clone(), 0).unwrap();
    r.set_ready(a.id, true, &mut d, 0).unwrap();
    r.set_ready(b.id, true, &mut d, 0).unwrap();
    r
}
#[test]
fn result_snapshot_pair_score_and_xp_commit_or_rollback_together() {
    let s = Store::open(":memory:").unwrap();
    let mut r = duel();
    s.save_room(&r, 0).unwrap();
    r.surrender(who(2).id, 1).unwrap();
    s.conn.execute_batch("CREATE TRIGGER fail_result BEFORE INSERT ON player_results BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
    assert!(s.save_room(&r, 1).is_err());
    assert_eq!(
        s.room(&r.id).unwrap().unwrap().status,
        crate::room::Status::Playing
    );
    assert_eq!(s.head_to_head(who(1).id, who(2).id).unwrap().host, 0);
    s.conn.execute_batch("DROP TRIGGER fail_result").unwrap();
    s.save_room(&r, 2).unwrap();
    s.save_room(&r, 3).unwrap();
    assert_eq!(s.head_to_head(who(1).id, who(2).id).unwrap().host, 1);
    let v = profile(&s.conn, &who(1), "all", 4).unwrap();
    assert_eq!(v["progress"]["xp_total"], 30);
    assert_eq!(v["statistics"]["all"]["wins"], 1);
}
#[test]
fn aborted_duel_does_not_award_a_win() {
    let s = Store::open(":memory:").unwrap();
    let mut r = duel();
    let t = Timings::default();
    r.set_connected(who(1).id, false, &t, 1);
    r.set_connected(who(2).id, false, &t, 1);
    r.tick(&mut OsDealer, &t, 1 + t.grace_ms + 1);
    s.save_room(&r, 1 + t.grace_ms + 1).unwrap();
    assert_eq!(
        profile(&s.conn, &who(1), "all", 0).unwrap()["progress"]["xp_total"],
        0
    );
}
#[test]
fn tables_keep_starting_composition_and_project_only_viewer_hole_cards() {
    for humans in [1, 2] {
        let s = Store::open(":memory:").unwrap();
        let a = who(1);
        let mut t = TableRoom::new(ids::room_id(), a.clone(), humans + 2, "q".into(), 0).unwrap();
        if humans == 2 {
            t.join(who(2), 0).unwrap();
        }
        t.add_bot(a.id, Opponent::Alex, 0).unwrap();
        t.add_bot(a.id, Opponent::Tight, 0).unwrap();
        t.start(a.id, &mut OsDealer, 0).unwrap();
        let g = t.game.as_mut().unwrap();
        while g.t.phase == Phase::InHand {
            let seat = g.t.hand.to_act.unwrap();
            let legal = g.t.view(seat).legal.unwrap();
            g.t.apply(
                seat,
                if legal.check {
                    PlayerAction::Check
                } else {
                    PlayerAction::Call
                },
            )
            .unwrap();
        }
        g.t.end_now().unwrap();
        s.save_table(&t, 1).unwrap();
        s.save_table(&t, 2).unwrap();
        let v = profile(&s.conn, &a, "all", 2).unwrap();
        let m = &v["recent_matches"]["items"][0];
        assert_eq!(m["mode"], "table");
        assert_eq!(m["human_count"], humans);
        assert_eq!(m["bot_count"], 2);
        let id = v["player_id"].as_str().unwrap();
        let full = detail(&s.conn, id, m["event_id"].as_str().unwrap())
            .unwrap()
            .unwrap();
        assert!(full["hands"][0].get("holes").is_none());
        assert!(full["hands"][0].get("hole").is_some());
    }
}
#[test]
fn migration_preserves_v2_rooms_and_does_not_reward_old_results() {
    let path = std::env::temp_dir().join(format!("blitz-progress-{}.sqlite", ids::room_id()));
    let path = path.to_str().unwrap();
    let mut old = duel();
    old.surrender(who(2).id, 1).unwrap();
    old.take_events();
    {
        let c = Connection::open(path).unwrap();
        c.execute_batch("CREATE TABLE rooms(id TEXT PRIMARY KEY,status TEXT,data TEXT,updated_ms INTEGER); PRAGMA user_version=2;").unwrap();
        c.execute(
            "INSERT INTO rooms VALUES (?1,'finished',?2,1)",
            params![old.id, serde_json::to_string(&old).unwrap()],
        )
        .unwrap();
    }
    let id;
    {
        let s = Store::open(path).unwrap();
        assert_eq!(s.room(&old.id).unwrap(), Some(old.clone()));
        s.save_room(&old, 2).unwrap();
        let v = profile(&s.conn, &who(1), "all", 3).unwrap();
        assert_eq!(v["progress"]["xp_total"], 0);
        id = v["player_id"].clone();
        let tx = s.conn.unchecked_transaction().unwrap();
        local(&tx, &who(1), &report("fresh001"), 3, &s.rewards).unwrap();
        tx.commit().unwrap();
    }
    {
        let s = Store::open(path).unwrap();
        let v = profile(&s.conn, &who(1), "all", 4).unwrap();
        assert_eq!(v["player_id"], id);
        assert_eq!(v["progress"]["xp_total"], 15);
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn reward_configuration_validates_without_mutating_process_environment() {
    let parse = |name: &str, value: &str| {
        Rewards::from_lookup(|key| Ok((key == name).then(|| value.to_string())))
    };
    for name in [
        "DAILY_XP_MATCHES",
        "DAILY_XP_HANDS",
        "DAILY_XP_WINS",
        "DAILY_XP_BONUS",
    ] {
        for invalid in ["", "-1", "1.5", "NaN", "1000001", "18446744073709551616"] {
            assert!(parse(name, invalid).is_err(), "{name}={invalid}");
        }
        assert!(parse(name, "0").is_ok());
        assert!(parse(name, " 20 ").is_ok());
        assert!(parse(name, "1000000").is_ok());
    }
    for invalid in ["", "true", "false", "-1", "2"] {
        assert!(parse("DAILY_REWARDS_ENABLED", invalid).is_err());
    }
    assert_eq!(
        parse("DAILY_REWARDS_ENABLED", "0").unwrap().tasks_enabled,
        0
    );
    let mut r = Rewards::default();
    assert_eq!(
        r.warnings().len(),
        1,
        "default bot-budget overlap is explicit"
    );
    r.bot_daily = 500;
    assert!(r.warnings().iter().any(|s| s.contains("exceeds PROFILE")));
    r.task_bonus = 1_000_000;
    assert!(r.warnings().iter().any(|s| s.contains("overall daily cap")));
    r.tasks_enabled = 0;
    assert_eq!(
        r.warnings().len(),
        1,
        "disabled rewards do not emit task-budget warnings"
    );
    assert!(Rewards::from_lookup(|_| Err("invalid UTF-8".into())).is_err());
}
