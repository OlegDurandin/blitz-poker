use super::*;
use poker_core::card::{Card, full_deck};
use poker_core::rng::Rng;

struct TestDealer {
    rng: Rng,
    n: u32,
}

impl TestDealer {
    fn new(seed: u64) -> TestDealer {
        TestDealer {
            rng: Rng::new(seed),
            n: 0,
        }
    }
}

impl Dealer for TestDealer {
    fn deck(&mut self) -> Vec<Card> {
        let mut d = full_deck();
        self.rng.shuffle(&mut d);
        d
    }
    fn coin(&mut self) -> bool {
        self.rng.chance(0.5)
    }
    fn match_id(&mut self) -> String {
        self.n += 1;
        format!("m{}", self.n)
    }
}

const HOST: PlayerId = PlayerId::from_raw(100);
const ANNA: PlayerId = PlayerId::from_raw(200);
const BORIS: PlayerId = PlayerId::from_raw(300);
const T: Timings = Timings {
    turn_ms: 20_000,
    bank_ms: 30_000,
    grace_ms: 60_000,
    restart_grace_ms: 180_000,
    room_ttl_ms: 900_000,
    hand_pause_ms: 4_000,
    runout_pause_ms: 6_500,
    bot_think_ms: 900,
};

fn profile(id: PlayerId) -> Profile {
    Profile {
        id,
        name: format!("Игрок {}", id.raw()),
        photo: None,
    }
}

fn lobby(capacity: usize) -> TableRoom {
    TableRoom::new("t".into(), profile(HOST), capacity, "req".into(), 0).unwrap()
}

/// Host, Anna and a bot, dealt.
fn playing(d: &mut TestDealer) -> TableRoom {
    let mut t = lobby(3);
    t.join(profile(ANNA), 1).unwrap();
    t.add_bot(HOST, Opponent::Tight, 2).unwrap();
    t.start(HOST, d, 3).unwrap();
    t
}

fn game(t: &TableRoom) -> &TableGame {
    t.game.as_ref().unwrap()
}

/// People check or call; time runs on so bots and the pauses advance.
fn play_out(t: &mut TableRoom, d: &mut TestDealer, mut now: u64) -> u64 {
    for _ in 0..5000 {
        if t.status != TableStatus::Playing {
            return now;
        }
        let g = game(t);
        if g.t.phase == Phase::InHand {
            let seat = g.t.hand.to_act.unwrap();
            if let TableSeat::Player { member, .. } = &t.seats[seat] {
                let legal = g.t.legal_actions(seat).unwrap();
                let a = if legal.check {
                    PlayerAction::Check
                } else {
                    PlayerAction::Call
                };
                let (id, v, m) = (member.profile.id, g.version, g.match_id.clone());
                t.action(id, &m, &format!("a{now}"), v, a, &T, now).unwrap();
                now += 100;
                continue;
            }
        }
        now += 1_000;
        t.tick(d, &T, now);
    }
    panic!("table did not finish");
}

#[test]
fn lobby_seats_friends_and_bots() {
    let mut t = lobby(4);
    assert_eq!(t.join(profile(ANNA), 1), Ok(1));
    assert_eq!(t.join(profile(ANNA), 2), Ok(1), "a reload keeps the seat");
    assert_eq!(
        t.add_bot(ANNA, Opponent::Alex, 3),
        Err(RoomError::NotMember),
        "host only"
    );
    t.add_bot(HOST, Opponent::Alex, 3).unwrap();
    assert_eq!(
        t.add_bot(HOST, Opponent::Alex, 3),
        Err(RoomError::Illegal),
        "each bot once"
    );
    t.add_bot(HOST, Opponent::Station, 4).unwrap();
    assert!(!t.joinable());
    assert_eq!(t.join(profile(BORIS), 5), Err(RoomError::Full));
    t.remove_bot(HOST, 3, 6).unwrap();
    assert_eq!(
        t.remove_bot(HOST, 1, 6),
        Err(RoomError::Illegal),
        "people are not removed"
    );
    assert_eq!(t.join(profile(BORIS), 7), Ok(3));
    // A friend leaving frees the seat; the host leaving closes the table.
    t.leave(BORIS, 8).unwrap();
    assert_eq!(t.seats[3], TableSeat::Empty);
    assert_eq!(
        t.start(ANNA, &mut TestDealer::new(1), 9),
        Err(RoomError::NotMember)
    );
    t.leave(HOST, 9).unwrap();
    assert_eq!(t.status, TableStatus::Closed);
    assert_eq!(t.close_reason, Some(TableCloseReason::CancelledByHost));
    assert_eq!(t.join(profile(BORIS), 10), Err(RoomError::Closed));
}

#[test]
fn start_needs_two_and_drops_empty_seats() {
    let mut d = TestDealer::new(2);
    let mut t = lobby(4);
    assert_eq!(t.start(HOST, &mut d, 1), Err(RoomError::WrongState));
    t.add_bot(HOST, Opponent::Novice, 2).unwrap();
    t.start(HOST, &mut d, 3).unwrap();
    assert_eq!(t.status, TableStatus::Playing);
    assert_eq!(t.seats.len(), 2);
    assert_eq!(game(&t).t.seats.len(), 2);
    assert_eq!(
        t.join(profile(ANNA), 4),
        Err(RoomError::Full),
        "no joining mid-game"
    );
}

#[test]
fn bots_act_after_a_pause_and_people_on_their_turn() {
    let mut d = TestDealer::new(3);
    let mut t = playing(&mut d);
    let mut now = 3;
    // Run until a bot is to act, then check it waits for its pause.
    for _ in 0..200 {
        let g = game(&t);
        let seat = g.t.hand.to_act.unwrap();
        if matches!(t.seats[seat], TableSeat::Bot { .. }) {
            break;
        }
        let id = t.seats[seat].player().unwrap();
        let legal = g.t.legal_actions(seat).unwrap();
        let a = if legal.check {
            PlayerAction::Check
        } else {
            PlayerAction::Call
        };
        let (v, m) = (g.version, g.match_id.clone());
        let other = if id == HOST { ANNA } else { HOST };
        assert_eq!(
            t.action(other, &m, "x", v, a, &T, now),
            Err(RoomError::NotYourTurn)
        );
        t.action(id, &m, &format!("a{now}"), v, a, &T, now).unwrap();
        now += 10;
        if game(&t).t.phase != Phase::InHand {
            now += 10_000;
            t.tick(&mut d, &T, now);
        }
    }
    let version = game(&t).version;
    let start = game(&t).turn_started_ms;
    assert!(
        !t.tick(&mut d, &T, start + T.bot_think_ms - 1),
        "the bot is thinking"
    );
    assert!(t.tick(&mut d, &T, start + T.bot_think_ms));
    assert_eq!(game(&t).version, version + 1);
}

#[test]
fn a_full_match_then_everyone_votes_again() {
    let mut d = TestDealer::new(4);
    let mut t = playing(&mut d);
    let now = play_out(&mut t, &mut d, 3);
    assert_eq!(t.status, TableStatus::Finished);
    assert_eq!(
        t.take_events(),
        [TableEvent::Finished { people: 2, bots: 1 }]
    );
    let view = t.view_for(HOST, &T).unwrap();
    assert!(view.game.as_ref().unwrap().view.result.is_some());
    t.vote_again(HOST, &mut d, now).unwrap();
    assert_eq!(t.status, TableStatus::Finished, "Anna has not agreed yet");
    assert_eq!(
        t.view_for(ANNA, &T).unwrap().again,
        AgainView {
            votes: 1,
            needed: 2,
            mine: false
        }
    );
    t.vote_again(ANNA, &mut d, now + 1).unwrap();
    assert_eq!(t.status, TableStatus::Playing);
    assert_eq!(game(&t).match_id, "m2");
}

#[test]
fn leaving_mid_game_folds_and_the_rest_play_on() {
    let mut d = TestDealer::new(5);
    let mut t = playing(&mut d);
    t.leave(ANNA, 10).unwrap();
    assert_eq!(t.status, TableStatus::Playing);
    let seat = t.seat_of(ANNA).unwrap();
    assert!(game(&t).t.left.contains(&seat));
    assert_eq!(t.members().count(), 1);
    assert_eq!(
        t.action(
            ANNA,
            "m1",
            "z",
            game(&t).version,
            PlayerAction::Fold,
            &T,
            11
        ),
        Err(RoomError::NotMember)
    );
    // The match goes on between the host and the bot, then Anna's seat is
    // gone from the next line-up.
    let now = play_out(&mut t, &mut d, 20);
    assert_eq!(t.status, TableStatus::Finished);
    let result = game(&t).t.result.clone().unwrap();
    assert_eq!(result.places[seat], 3, "who left ranks last");
    t.vote_again(HOST, &mut d, now).unwrap();
    assert_eq!(t.status, TableStatus::Playing);
    assert_eq!(t.seats.len(), 2);
}

#[test]
fn the_last_person_leaving_closes_the_table() {
    let mut d = TestDealer::new(6);
    let mut t = playing(&mut d);
    t.leave(ANNA, 10).unwrap();
    t.leave(HOST, 11).unwrap();
    assert_eq!(t.status, TableStatus::Closed);
    assert_eq!(t.close_reason, Some(TableCloseReason::Empty));
}

#[test]
fn a_dropped_player_is_out_when_the_grace_runs_out() {
    let mut d = TestDealer::new(7);
    let mut t = playing(&mut d);
    t.set_connected(ANNA, false, &T, 100);
    let view = t.view_for(HOST, &T).unwrap();
    assert!(view.seats.iter().any(|s| matches!(
        s,
        SeatView::Player {
            connected: false,
            reconnect_deadline_ms: Some(60_100),
            ..
        }
    )));
    t.set_connected(ANNA, true, &T, 200);
    t.set_connected(ANNA, false, &T, 300);
    let mut now = 300;
    while now < 60_300 + 1 {
        now += 500;
        t.tick(&mut d, &T, now);
        if t.status != TableStatus::Playing {
            break;
        }
    }
    if t.status == TableStatus::Playing {
        assert!(game(&t).t.left.contains(&t.seat_of(ANNA).unwrap()));
    }
    assert!(t.members().all(|m| m.profile.id != ANNA));
}

#[test]
fn a_person_out_of_time_checks_or_folds() {
    let mut d = TestDealer::new(8);
    let mut t = playing(&mut d);
    let mut now = 3;
    // Wait for a person's turn.
    while matches!(
        t.seats[game(&t).t.hand.to_act.unwrap()],
        TableSeat::Bot { .. }
    ) {
        now += 1_000;
        t.tick(&mut d, &T, now);
    }
    let seat = game(&t).t.hand.to_act.unwrap();
    let deadline = game(&t).deadline(seat, &T);
    assert!(!t.tick(&mut d, &T, deadline - 1));
    assert!(t.tick(&mut d, &T, deadline));
    let last = game(&t).t.hand.actions.last().unwrap();
    assert_eq!(last.seat, seat);
    assert!(matches!(
        last.kind,
        poker_core::ActionKind::Check | poker_core::ActionKind::Fold
    ));
    assert_eq!(game(&t).bank_ms[seat], 0);
}

#[test]
fn each_player_sees_only_their_cards() {
    let mut d = TestDealer::new(9);
    let t = playing(&mut d);
    let host = t.view_for(HOST, &T).unwrap();
    let anna = t.view_for(ANNA, &T).unwrap();
    let (hv, av) = (
        &host.game.as_ref().unwrap().view,
        &anna.game.as_ref().unwrap().view,
    );
    assert_eq!(hv.me, 0);
    assert_eq!(av.me, 1);
    assert_ne!(hv.hole, av.hole);
    let json = serde_json::to_string(&host).unwrap();
    for c in av.hole.unwrap() {
        assert!(
            !json.contains(&format!("\"{c}\"")),
            "Anna's {c} leaked to the host"
        );
    }
    assert!(t.view_for(BORIS, &T).is_none());
    assert!(json.contains("\"type\":\"bot\""));
}

#[test]
fn survives_a_restart() {
    let mut d = TestDealer::new(10);
    let t = playing(&mut d);
    let json = serde_json::to_string(&t).unwrap();
    let mut back: TableRoom = serde_json::from_str(&json).unwrap();
    assert_eq!(back, t);
    back.restore_after_restart(&T, 1_000);
    assert!(
        back.members()
            .all(|m| !m.connected && m.grace_until_ms == Some(181_000))
    );
}

#[test]
fn idle_lobby_expires() {
    let mut d = TestDealer::new(11);
    let mut t = lobby(3);
    assert!(!t.tick(&mut d, &T, T.room_ttl_ms - 1));
    assert!(t.tick(&mut d, &T, T.room_ttl_ms));
    assert_eq!(t.close_reason, Some(TableCloseReason::Expired));
}

#[test]
fn capacity_is_two_to_four() {
    for (cap, ok) in [(1, false), (2, true), (4, true), (5, false)] {
        assert_eq!(
            TableRoom::new("t".into(), profile(HOST), cap, "r".into(), 0).is_ok(),
            ok
        );
    }
}
