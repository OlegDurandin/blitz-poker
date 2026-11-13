use super::*;
use crate::{player::PlayerId, room::Profile, store::Store};
fn player(s: &Store, tg: bool) -> String {
    progress::identity(
        &s.conn,
        &Profile {
            id: if tg {
                PlayerId::telegram(42).unwrap()
            } else {
                PlayerId::vk(42).unwrap()
            },
            name: "Игрок".into(),
            photo: None,
        },
        0,
    )
    .unwrap()
    .unwrap()
}
fn send(s: &Store, id: &str, kind: &str, key: &str, won: bool, bot: bool, now: u64) {
    let tx = s.conn.unchecked_transaction().unwrap();
    event(
        &tx,
        id,
        kind,
        key,
        Participation { won, bot },
        now,
        &s.rewards,
    )
    .unwrap();
    tx.commit().unwrap();
}
#[test]
fn fixed_sets_rewards_duplicates_and_provider_isolation() {
    let s = Store::open(":memory:").unwrap();
    let id = player(&s, false);
    let tg = player(&s, true);
    let first = get(&s.conn, &id, 1, &s.rewards).unwrap();
    assert_eq!(first["items"].as_array().unwrap().len(), 3);
    assert_eq!(first, get(&s.conn, &id, 1, &s.rewards).unwrap());
    for n in 0..6 {
        send(&s, &id, "hand", &n.to_string(), n < 2, false, 1000);
    }
    for n in 0..2 {
        send(&s, &id, "match", &n.to_string(), false, false, 1000);
    }
    let done = get(&s.conn, &id, 1000, &s.rewards).unwrap();
    assert_eq!(done["completed_count"], 3);
    assert_eq!(done["completion_bonus"]["awarded_xp"], 30);
    assert_eq!(
        s.conn
            .query_row("SELECT SUM(xp) FROM xp_ledger", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        100
    );
    send(&s, &id, "hand", "0", true, false, 86_400_001);
    send(&s, &id, "match", "0", false, false, 86_400_001);
    assert_eq!(done, get(&s.conn, &id, 1000, &s.rewards).unwrap());
    assert_eq!(
        get(&s.conn, &tg, 1000, &s.rewards).unwrap()["completed_count"],
        0
    );
    assert_eq!(
        get(&s.conn, &id, 86_400_001, &s.rewards).unwrap()["completed_count"],
        0
    );
}
#[test]
fn utc_event_day_and_config_snapshot() {
    let mut s = Store::open(":memory:").unwrap();
    let id = player(&s, false);
    send(&s, &id, "hand", "before", false, false, 86_399_000);
    s.rewards.task_hands = 999;
    s.rewards.task_bonus = 999;
    let old = get(&s.conn, &id, 86_399_000, &s.rewards).unwrap();
    assert_eq!(old["items"][1]["progress"], 1);
    assert_eq!(old["items"][1]["reward_xp"], 20);
    assert_eq!(old["completion_bonus"]["reward_xp"], 30);
    send(&s, &id, "hand", "after", true, false, 86_401_000);
    let next = get(&s.conn, &id, 86_401_000, &s.rewards).unwrap();
    assert_eq!(next["day_utc"], "1970-01-02");
    assert_eq!(next["next_reset_at"], "1970-01-03T00:00:00Z");
    assert_eq!(next["items"][1]["progress"], 1);
    assert_eq!(next["items"][1]["reward_xp"], 999);
    assert_eq!(old["items"][1]["progress"], 1);
}
#[test]
fn common_caps_bot_caps_disabled_and_transaction_retry() {
    let mut s = Store::open(":memory:").unwrap();
    let id = player(&s, false);
    award(&s.conn, &id, "match:prior", 0, true, 95, &s.rewards).unwrap();
    send(&s, &id, "hand", "win1", true, true, 1);
    send(&s, &id, "hand", "win2", true, true, 1);
    let v = get(&s.conn, &id, 1, &s.rewards).unwrap();
    assert_eq!(v["items"][2]["awarded_xp"], 5);
    assert_eq!(v["items"][2]["reward_reason"], "xp_limit");
    assert_eq!(
        award(&s.conn, &id, "match:after", 1, true, 15, &s.rewards).unwrap(),
        0
    );
    s.rewards.daily = 105;
    assert_eq!(
        award(&s.conn, &id, "match:human", 1, false, 30, &s.rewards).unwrap(),
        5
    );
    {
        let tx = s.conn.unchecked_transaction().unwrap();
        event(
            &tx,
            &id,
            "match",
            "rollback",
            Participation {
                won: false,
                bot: false,
            },
            1,
            &s.rewards,
        )
        .unwrap();
        // Rollback also removes event receipt and progress.
    }
    assert_eq!(
        get(&s.conn, &id, 1, &s.rewards).unwrap()["items"][0]["progress"],
        0
    );
    send(&s, &id, "match", "rollback", false, false, 1);
    assert_eq!(
        get(&s.conn, &id, 1, &s.rewards).unwrap()["items"][0]["progress"],
        1
    );
    s.rewards.tasks_enabled = 0;
    send(&s, &id, "match", "disabled", false, false, 86_400_001);
    send(&s, &id, "match", "disabled2", false, false, 86_400_001);
    assert_eq!(
        get(&s.conn, &id, 86_400_001, &s.rewards).unwrap()["items"][0]["awarded_xp"],
        0
    );
}

fn profile(n: u64) -> Profile {
    Profile {
        id: PlayerId::vk(n).unwrap(),
        name: format!("Игрок {n}"),
        photo: None,
    }
}
fn duel() -> Room {
    use crate::room::OsDealer;
    let mut r = Room::new(crate::ids::room_id(), profile(1), "q".into(), 1);
    r.join(profile(2), 1).unwrap();
    r.set_ready(profile(1).id, true, &mut OsDealer, 1).unwrap();
    r.set_ready(profile(2).id, true, &mut OsDealer, 1).unwrap();
    r
}
#[test]
fn authoritative_fold_split_and_retry_across_midnight() {
    use poker_core::{Match, PlayerAction};
    let s = Store::open(":memory:").unwrap();
    let mut r = duel();
    let id = progress::identity(&s.conn, &profile(1), 0)
        .unwrap()
        .unwrap();
    let other = progress::identity(&s.conn, &profile(2), 0)
        .unwrap()
        .unwrap();
    s.save_room(&r, 1).unwrap();
    let g = r.game.as_ref().unwrap();
    let actor = g.m.hand.to_act.unwrap();
    let match_id = g.match_id.clone();
    let version = g.version;
    r.action(
        profile(if actor == Seat::Human { 1 } else { 2 }).id,
        &match_id,
        "utc-fold",
        version,
        PlayerAction::Fold,
        &crate::room::Timings::default(),
        86_399_000,
    )
    .unwrap();
    assert_eq!(r.game.as_ref().unwrap().hand_finished_ms, vec![86_399_000]);
    s.conn.execute_batch("CREATE TRIGGER fail_daily BEFORE INSERT ON daily_challenge_events BEGIN SELECT RAISE(ABORT,'retry'); END;").unwrap();
    assert!(s.save_room(&r, 86_399_000).is_err());
    s.conn.execute_batch("DROP TRIGGER fail_daily").unwrap();
    s.save_room(&r, 86_401_000).unwrap();
    s.save_room(&r, 86_402_000).unwrap();
    for p in [&id, &other] {
        let v = get(&s.conn, p, 86_399_000, &s.rewards).unwrap();
        assert_eq!(v["items"][1]["progress"], 1, "fold participates once");
        assert_eq!(
            v["items"][2]["progress"],
            if (p == &id) == (actor != Seat::Human) {
                1
            } else {
                0
            }
        );
        assert_eq!(
            get(&s.conn, p, 86_401_000, &s.rewards).unwrap()["items"][1]["progress"],
            0
        );
    }
    // A real engine showdown where the royal flush on the board splits the pot.
    let g = r.game.as_mut().unwrap();
    let mut deck: Vec<poker_core::Card> = "2s 3d 2c 3h As Ks Qs Js Ts"
        .split_whitespace()
        .map(|v| poker_core::Card::try_from(v.to_string()).unwrap())
        .collect();
    for card in poker_core::card::full_deck() {
        if !deck.contains(&card) {
            deck.push(card);
        }
    }
    g.match_id = crate::ids::match_id();
    g.m = Match::duel(g.match_id.clone(), Seat::Bot, &deck).unwrap();
    while g.m.phase == Phase::InHand {
        let seat = g.m.hand.to_act.unwrap();
        let legal = g.m.view(seat).legal.unwrap();
        g.m.apply(
            seat,
            if legal.check {
                PlayerAction::Check
            } else {
                PlayerAction::Call
            },
        )
        .unwrap();
    }
    assert_eq!(g.m.history[0].outcome.winner, None);
    g.hand_finished_ms = vec![86_401_000];
    s.save_room(&r, 86_401_000).unwrap();
    for p in [&id, &other] {
        let v = get(&s.conn, p, 86_401_000, &s.rewards).unwrap();
        assert_eq!(v["items"][1]["progress"], 1);
        assert_eq!(
            v["items"][2]["progress"], 0,
            "split is not sole-main-pot win"
        );
    }
}

#[test]
fn concurrent_gets_share_one_persisted_set() {
    let path = std::env::temp_dir().join(format!("blitz-daily-{}.sqlite", crate::ids::room_id()));
    let path = path.to_str().unwrap().to_string();
    let s = Store::open(&path).unwrap();
    let id = player(&s, false);
    drop(s);
    let a = Store::open(&path).unwrap();
    let b = Store::open(&path).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads = [a, b]
        .into_iter()
        .map(|s| {
            let barrier = barrier.clone();
            let id = id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let tx = s.conn.unchecked_transaction().unwrap();
                let v = get(&tx, &id, 1, &s.rewards).unwrap();
                tx.commit().unwrap();
                v
            })
        })
        .collect::<Vec<_>>();
    let results = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results[0], results[1]);
    let s = Store::open(&path).unwrap();
    assert_eq!(
        s.conn
            .query_row("SELECT COUNT(*) FROM daily_challenge_items", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        3
    );
    drop(s);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn local_reports_do_not_progress_and_migration_preserves_xp_and_skips_old_hands() {
    let path = std::env::temp_dir().join(format!(
        "blitz-daily-migration-{}.sqlite",
        crate::ids::room_id()
    ));
    let path = path.to_str().unwrap();
    let mut r = duel();
    let g = r.game.as_mut().unwrap();
    g.m.apply(g.m.hand.to_act.unwrap(), poker_core::PlayerAction::Fold)
        .unwrap();
    let id;
    {
        let c = Connection::open(path).unwrap();
        c.execute_batch("CREATE TABLE rooms(id TEXT PRIMARY KEY,status TEXT,data TEXT,updated_ms INTEGER); CREATE TABLE tables(id TEXT PRIMARY KEY,status TEXT,data TEXT,updated_ms INTEGER); CREATE TABLE duels(a INTEGER,b INTEGER,a_wins INTEGER,b_wins INTEGER,draws INTEGER,PRIMARY KEY(a,b));").unwrap();
        c.execute_batch(progress::SCHEMA).unwrap();
        id = progress::identity(&c, &profile(1), 0).unwrap().unwrap();
        c.execute(
            "INSERT INTO player_results VALUES ('old',?1,'old','server','duel',0,0,'win',30,'{}')",
            [&id],
        )
        .unwrap();
        c.execute(
            "INSERT INTO rooms VALUES (?1,'playing',?2,1)",
            params![r.id, serde_json::to_string(&r).unwrap()],
        )
        .unwrap();
        c.execute_batch("PRAGMA user_version=3").unwrap();
    }
    let s = Store::open(path).unwrap();
    s.save_room(&r, 2).unwrap();
    assert_eq!(
        get(&s.conn, &id, 2, &s.rewards).unwrap()["items"][1]["progress"],
        0
    );
    assert_eq!(
        s.conn
            .query_row(
                "SELECT SUM(xp) FROM xp_ledger WHERE player_id=?1",
                [&id],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        30
    );
    let report = progress::LocalReport {
        match_id: "local-proof-test".into(),
        mode: "solo".into(),
        outcome: "win".into(),
        final_stack: 700,
        hands_played: 8,
        hands_won: 8,
        opponents: vec!["Бот".into()],
    };
    // Old result JSON is deliberately not used as a profile/statistics fixture.
    s.conn
        .execute("DELETE FROM player_results WHERE event_id='old'", [])
        .unwrap();
    progress::local(&s.conn, &profile(1), &report, 2, &s.rewards).unwrap();
    assert_eq!(
        get(&s.conn, &id, 2, &s.rewards).unwrap()["completed_count"],
        0
    );
    assert_eq!(
        get(&s.conn, &id, 2, &s.rewards).unwrap()["items"][0]["progress"],
        0
    );
    drop(s);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn economy_models_one_two_three_five_ten_matches_and_all_tasks() {
    // Eight closed hands, two sole-main-pot wins per match; match outcome win.
    // This is an XP-policy simulation, not a claim that every real match lasts eight hands.
    for bot in [true, false] {
        for n in [1usize, 2, 3, 5, 10] {
            for tasks in [false, true] {
                let s = Store::open(":memory:").unwrap();
                let id = player(&s, false);
                for m in 0..n {
                    if tasks {
                        for h in 0..8 {
                            send(&s, &id, "hand", &format!("{m}:{h}"), h < 2, bot, 1);
                        }
                        send(&s, &id, "match", &m.to_string(), false, bot, 1);
                    }
                    award(
                        &s.conn,
                        &id,
                        &format!("match:{m}"),
                        1,
                        bot,
                        if bot { 15 } else { 30 },
                        &s.rewards,
                    )
                    .unwrap();
                }
                let total = s
                    .conn
                    .query_row("SELECT SUM(xp) FROM xp_ledger", [], |r| r.get::<_, i64>(0))
                    .unwrap();
                let expected = if bot {
                    if tasks {
                        if n == 1 { 65 } else { 100 }
                    } else {
                        (n as i64 * 15).min(100)
                    }
                } else if tasks {
                    if n == 1 {
                        80
                    } else {
                        (n as i64 * 30 + 100).min(300)
                    }
                } else {
                    (n as i64 * 30).min(300)
                };
                assert_eq!(total, expected, "bot={bot}, matches={n}, tasks={tasks}");
                if tasks {
                    let v = get(&s.conn, &id, 1, &s.rewards).unwrap();
                    assert_eq!(v["completed_count"], if n == 1 { 2 } else { 3 });
                    let daily=s.conn.query_row("SELECT COALESCE(SUM(xp),0) FROM xp_ledger WHERE reward_key LIKE 'daily_%'",[],|r|r.get::<_,i64>(0)).unwrap();
                    println!("economy bot={bot} matches={n} total={total} actual_daily={daily}");
                }
            }
        }
    }
}

#[test]
fn simultaneous_final_hand_match_and_all_three_rewards_survive_restart_retry() {
    let path = std::env::temp_dir().join(format!("daily-final-{}.sqlite", crate::ids::room_id()));
    let path = path.to_str().unwrap();
    let mut room = duel();
    let id;
    {
        let s = Store::open(path).unwrap();
        id = progress::identity(&s.conn, &profile(1), 1)
            .unwrap()
            .unwrap();
        for h in 0..5 {
            send(&s, &id, "hand", &format!("prior:{h}"), h == 0, false, 1);
        }
        send(&s, &id, "match", "prior-match", false, false, 1);
        // A real engine hand won by the host, then a confirmed forfeit result,
        // persisted in the same snapshot (the final-hand + match event boundary).
        let g = room.game.as_mut().unwrap();
        if g.m.hand.to_act == Some(Seat::Human) {
            g.m.apply(Seat::Human, poker_core::PlayerAction::Call)
                .unwrap();
        }
        g.m.apply(Seat::Bot, poker_core::PlayerAction::Fold)
            .unwrap();
        g.hand_finished_ms = vec![86_399_000];
        room.surrender(profile(2).id, 86_399_000).unwrap();
        s.conn.execute_batch("CREATE TRIGGER fail_xp BEFORE INSERT ON player_results BEGIN SELECT RAISE(ABORT,'test rollback'); END;").unwrap();
        assert!(s.save_room(&room, 86_399_000).is_err());
        assert_eq!(
            get(&s.conn, &id, 1, &s.rewards).unwrap()["completed_count"],
            0
        );
        s.conn.execute_batch("DROP TRIGGER fail_xp").unwrap();
        s.save_room(&room, 86_401_000).unwrap();
    }
    {
        let s = Store::open(path).unwrap();
        let mut restored = s.room(&room.id).unwrap().unwrap();
        restored.restore_after_restart(&crate::room::Timings::default(), 86_402_000);
        s.save_room(&restored, 86_402_000).unwrap();
        s.save_room(&restored, 86_403_000).unwrap();
        let old = get(&s.conn, &id, 86_399_000, &s.rewards).unwrap();
        assert_eq!(old["completed_count"], 3);
        assert_eq!(old["completion_bonus"]["awarded_xp"], 30);
        assert_eq!(
            old["xp_budget"]["awarded_today"], 130,
            "100 daily + 30 match, only once"
        );
        let today = get(&s.conn, &id, 86_401_000, &s.rewards).unwrap();
        assert_eq!(today["completed_count"], 0);
        assert_eq!(today["xp_budget"]["awarded_today"], 0);
        assert_eq!(
            s.conn
                .query_row(
                    "SELECT COUNT(*) FROM player_results WHERE player_id=?1",
                    [&id],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        assert_eq!(
            s.conn
                .query_row(
                    "SELECT COUNT(*) FROM xp_ledger WHERE player_id=?1",
                    [&id],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            5
        );
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn two_rooms_for_one_player_serialize_rewards_under_the_same_daily_cap() {
    let path = std::env::temp_dir().join(format!("daily-race-{}.sqlite", crate::ids::room_id()));
    let path = path.to_str().unwrap().to_string();
    let id;
    {
        let s = Store::open(&path).unwrap();
        id = progress::identity(&s.conn, &profile(1), 0)
            .unwrap()
            .unwrap();
        award(&s.conn, &id, "prior-xp", 1, false, 295, &s.rewards).unwrap();
    }
    let a = Store::open(&path).unwrap();
    let b = Store::open(&path).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads = [a, b]
        .into_iter()
        .map(|s| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut room = duel();
                room.surrender(profile(2).id, 2).unwrap();
                barrier.wait();
                s.save_room(&room, 2).unwrap();
                s.save_room(&room, 3).unwrap();
            })
        })
        .collect::<Vec<_>>();
    for t in threads {
        t.join().unwrap();
    }
    let s = Store::open(&path).unwrap();
    let v = get(&s.conn, &id, 4, &s.rewards).unwrap();
    assert_eq!(v["items"][0]["progress"], 2);
    assert_eq!(v["xp_budget"]["awarded_today"], 300);
    assert_eq!(
        s.conn
            .query_row(
                "SELECT COUNT(*) FROM player_results WHERE player_id=?1",
                [&id],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        2
    );
    drop(s);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn mixed_bot_progress_and_disabled_rewards_cannot_be_reissued_after_cap_changes() {
    let mut s = Store::open(":memory:").unwrap();
    let id = player(&s, false);
    award(&s.conn, &id, "prior-bots", 1, true, 100, &s.rewards).unwrap();
    send(&s, &id, "match", "bot", false, true, 1);
    send(&s, &id, "match", "human", false, false, 1);
    let v = get(&s.conn, &id, 1, &s.rewards).unwrap();
    assert_eq!(v["items"][0]["awarded_xp"], 0);
    assert_eq!(v["items"][0]["reward_reason"], "xp_limit");
    s.rewards.bot_daily = 1000;
    send(&s, &id, "match", "human-again", false, false, 1);
    assert_eq!(
        get(&s.conn, &id, 1, &s.rewards).unwrap()["items"][0]["awarded_xp"],
        0
    );
    s.rewards.tasks_enabled = 0;
    for h in 0..6 {
        send(&s, &id, "hand", &h.to_string(), h < 2, false, 1);
    }
    let v = get(&s.conn, &id, 1, &s.rewards).unwrap();
    assert_eq!(v["completion_bonus"]["reward_reason"], "disabled");
    s.rewards.tasks_enabled = 1;
    send(&s, &id, "hand", "later", true, false, 1);
    assert_eq!(
        get(&s.conn, &id, 1, &s.rewards).unwrap()["completion_bonus"]["awarded_xp"],
        0
    );
}

#[test]
fn v3_migration_preserves_real_result_data_local_dedup_caps_and_repeated_open() {
    let path = std::env::temp_dir().join(format!("migration331-{}.sqlite", crate::ids::room_id()));
    let path = path.to_str().unwrap();
    let id;
    {
        let c = Connection::open(path).unwrap();
        c.execute_batch("CREATE TABLE rooms(id TEXT PRIMARY KEY,status TEXT,data TEXT,updated_ms INTEGER); CREATE TABLE tables(id TEXT PRIMARY KEY,status TEXT,data TEXT,updated_ms INTEGER); CREATE TABLE duels(a INTEGER,b INTEGER,a_wins INTEGER,b_wins INTEGER,draws INTEGER,PRIMARY KEY(a,b));").unwrap();
        c.execute_batch(progress::SCHEMA).unwrap();
        id = progress::identity(&c, &profile(1), 0).unwrap().unwrap();
        for (source, mode, event, xp, bots) in [
            ("local", "solo", format!("local:{id}:legacy-local"), 80, 1),
            ("server", "duel", "server:legacy-server".into(), 180, 0),
        ] {
            let row = progress::ResultRow {
                event_id: event.clone(),
                match_id: if source == "local" {
                    "legacy-local"
                } else {
                    "legacy-server"
                }
                .into(),
                source: source.into(),
                mode: mode.into(),
                finished_ms: 1,
                human_count: if source == "local" { 1 } else { 2 },
                bot_count: bots,
                outcome: "win".into(),
                final_stack: 700,
                hands_played: 8,
                hands_won: 2,
                participant_hands: None,
                best_hand: None,
                opponents: vec!["Соперник".into()],
                xp: xp as u64,
                xp_limited: false,
            };
            c.execute(
                "INSERT INTO player_results VALUES (?1,?2,?3,?4,?5,1,?6,'win',?7,?8)",
                params![
                    event,
                    id,
                    row.match_id,
                    source,
                    mode,
                    bots as i64,
                    xp,
                    serde_json::to_string(&row).unwrap()
                ],
            )
            .unwrap();
        }
        c.execute_batch("PRAGMA user_version=3").unwrap();
    }
    for _ in 0..2 {
        let s = Store::open(path).unwrap();
        assert_eq!(
            progress::profile(&s.conn, &profile(1), "all", 2).unwrap()["progress"]["xp_total"],
            260
        );
        assert_eq!(
            get(&s.conn, &id, 2, &s.rewards).unwrap()["xp_budget"]["bot_awarded_today"],
            80
        );
        let report = progress::LocalReport {
            match_id: "legacy-local".into(),
            mode: "solo".into(),
            outcome: "loss".into(),
            final_stack: 0,
            hands_played: 8,
            hands_won: 0,
            opponents: vec!["Бот".into()],
        };
        let old = progress::local(&s.conn, &profile(1), &report, 86_400_001, &s.rewards).unwrap();
        assert_eq!(old["xp"], 80);
        assert_eq!(old["outcome"], "win");
        assert_eq!(old["finished_ms"], 1);
        assert_eq!(
            s.conn
                .query_row("SELECT COUNT(*) FROM xp_ledger", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
    }
    let s = Store::open(path).unwrap();
    assert_eq!(
        award(&s.conn, &id, "new-bot", 2, true, 30, &s.rewards).unwrap(),
        20
    );
    assert_eq!(
        award(&s.conn, &id, "new-human", 2, false, 30, &s.rewards).unwrap(),
        20
    );
    assert_eq!(
        get(&s.conn, &id, 2, &s.rewards).unwrap()["xp_budget"]["remaining_total"],
        0
    );
    drop(s);
    std::fs::remove_file(path).unwrap();
}
