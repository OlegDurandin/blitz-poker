use super::*;
use poker_core::card::full_deck;
use poker_core::rng::Rng;

/// Deterministic dealer for tests.
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

const HOST: u64 = 100;
const GUEST: u64 = 200;
const T: Timings = Timings {
    turn_ms: 20_000,
    bank_ms: 30_000,
    grace_ms: 60_000,
    restart_grace_ms: 180_000,
    room_ttl_ms: 900_000,
    hand_pause_ms: 4_000,
    runout_pause_ms: 6_500,
};

fn profile(id: u64) -> Profile {
    Profile {
        vk_id: id,
        name: format!("Игрок {id}"),
        photo: None,
    }
}

fn ready_room(d: &mut TestDealer) -> Room {
    let mut r = Room::new("room".into(), profile(HOST), "req".into(), 0);
    r.join(profile(GUEST), 10).unwrap();
    r.set_ready(HOST, true, d, 20).unwrap();
    r.set_ready(GUEST, true, d, 30).unwrap();
    assert_eq!(r.status, Status::Playing);
    r
}

fn to_act(r: &Room) -> u64 {
    let g = r.game.as_ref().unwrap();
    if g.m.hand.to_act == Some(Seat::Human) {
        HOST
    } else {
        GUEST
    }
}

fn version(r: &Room) -> u64 {
    r.game.as_ref().unwrap().version
}

fn match_id(r: &Room) -> String {
    r.game.as_ref().unwrap().match_id.clone()
}

/// The passive move for whoever acts.
fn passive(r: &Room) -> PlayerAction {
    let g = r.game.as_ref().unwrap();
    let l = g.m.legal_actions(g.m.hand.to_act.unwrap()).unwrap();
    if l.check {
        PlayerAction::Check
    } else {
        PlayerAction::Call
    }
}

#[test]
fn two_seats_only_and_the_host_keeps_the_host_seat() {
    let mut r = Room::new("room".into(), profile(HOST), "req".into(), 0);
    assert_eq!(
        r.join(profile(HOST), 1),
        Ok(Slot::Host),
        "the host's own link"
    );
    assert_eq!(r.status, Status::Waiting);
    assert_eq!(r.join(profile(GUEST), 2), Ok(Slot::Guest));
    assert_eq!(r.status, Status::Ready);
    assert_eq!(r.join(profile(300), 3), Err(RoomError::Full));
    assert_eq!(
        r.join(profile(GUEST), 4),
        Ok(Slot::Guest),
        "a reload keeps the seat"
    );
    assert_eq!(r.join(profile(HOST), 5), Ok(Slot::Host));
    assert_eq!(r.members().count(), 2);
}

#[test]
fn match_starts_once_when_both_are_ready() {
    let mut d = TestDealer::new(1);
    let mut r = Room::new("room".into(), profile(HOST), "req".into(), 0);
    assert_eq!(
        r.set_ready(HOST, true, &mut d, 1),
        Err(RoomError::WrongState),
        "nobody to play"
    );
    r.join(profile(GUEST), 2).unwrap();
    r.set_ready(HOST, true, &mut d, 3).unwrap();
    r.set_ready(HOST, true, &mut d, 4).unwrap();
    assert_eq!(
        r.status,
        Status::Ready,
        "one player twice is not two players"
    );
    r.set_ready(GUEST, true, &mut d, 5).unwrap();
    assert_eq!(r.status, Status::Playing);
    let first = match_id(&r);
    assert_eq!(
        r.set_ready(GUEST, true, &mut d, 6),
        Err(RoomError::WrongState)
    );
    assert_eq!(match_id(&r), first);
}

#[test]
fn each_player_sees_only_their_own_cards() {
    let mut d = TestDealer::new(2);
    let r = ready_room(&mut d);
    let g = r.game.as_ref().unwrap();
    for (me, seat) in [(HOST, Seat::Human), (GUEST, Seat::Bot)] {
        let view = r.view_for(me, &T).unwrap();
        let game = view.game.as_ref().unwrap();
        assert_eq!(game.view.hole, g.m.hand.hole(seat));
        assert_eq!(game.view.me, Seat::Human, "everyone is at the bottom");
        let json = serde_json::to_string(&view).unwrap();
        for c in g.m.hand.hole(seat.other()) {
            assert!(!json.contains(&format!("\"{c}\"")), "{me} sees {c}");
        }
    }
    assert!(r.view_for(300, &T).is_none(), "outsiders get nothing");
}

#[test]
fn actions_are_idempotent_and_versioned() {
    let mut d = TestDealer::new(3);
    let mut r = ready_room(&mut d);
    let id = match_id(&r);
    let actor = to_act(&r);
    let other = if actor == HOST { GUEST } else { HOST };
    let v = version(&r);
    let a = passive(&r);

    assert_eq!(
        r.action(other, &id, "x", v, a, &T, 40),
        Err(RoomError::NotYourTurn)
    );
    assert_eq!(
        r.action(actor, &id, "x", v + 5, a, &T, 40),
        Err(RoomError::Stale)
    );
    assert_eq!(
        r.action(actor, "old", "x", v, a, &T, 40),
        Err(RoomError::WrongMatch)
    );
    assert_eq!(
        r.action(
            actor,
            &id,
            "x",
            v,
            PlayerAction::RaiseTo { amount: 1 },
            &T,
            40
        ),
        Err(RoomError::InvalidAmount)
    );
    assert_eq!(
        r.action(300, &id, "x", v, a, &T, 40),
        Err(RoomError::NotMember)
    );

    let ack = r.action(actor, &id, "a1", v, a, &T, 50).unwrap();
    assert_eq!(
        ack,
        ActionAck {
            state_version: v + 1,
            duplicate: false
        }
    );
    let chips = r.game.as_ref().unwrap().m.stacks;
    let again = r.action(actor, &id, "a1", v, a, &T, 60).unwrap();
    assert_eq!(
        again,
        ActionAck {
            state_version: v + 1,
            duplicate: true
        }
    );
    assert_eq!(
        r.game.as_ref().unwrap().m.stacks,
        chips,
        "a retry moves no chips"
    );
}

/// Plays whole matches by the passive move, letting the pause between hands
/// run out, and checks the result is recorded once.
#[test]
fn full_match_through_ticks() {
    for seed in 0..30 {
        let mut d = TestDealer::new(seed);
        let mut r = ready_room(&mut d);
        let mut now = 100;
        let mut steps = 0;
        while r.status == Status::Playing {
            steps += 1;
            assert!(steps < 2000);
            let g = r.game.as_ref().unwrap();
            assert_eq!(g.m.chips_in_play(), poker_core::TOTAL_CHIPS);
            if g.m.phase == Phase::InHand {
                let (who, v, id) = (to_act(&r), version(&r), match_id(&r));
                let a = passive(&r);
                r.action(who, &id, &format!("a{steps}"), v, a, &T, now)
                    .unwrap();
            } else {
                now += 7_000;
                assert!(r.tick(&mut d, &T, now));
            }
        }
        assert_eq!(r.status, Status::Finished);
        let events = r.take_events();
        assert_eq!(events.len(), 1, "seed {seed}");
        assert_eq!(r.score.host + r.score.guest + r.score.draws, 1);
        let host = r
            .view_for(HOST, &T)
            .unwrap()
            .game
            .unwrap()
            .view
            .result
            .unwrap();
        let guest = r
            .view_for(GUEST, &T)
            .unwrap()
            .game
            .unwrap()
            .view
            .result
            .unwrap();
        assert_eq!(
            host.final_stacks,
            [guest.final_stacks[1], guest.final_stacks[0]]
        );
        assert!(r.take_events().is_empty());
    }
}

#[test]
fn turn_timer_uses_the_bank_then_checks_or_folds() {
    let mut d = TestDealer::new(4);
    let mut r = ready_room(&mut d);
    let actor = to_act(&r);
    let start = r.game.as_ref().unwrap().turn_started_ms;
    // 20 s turn + 30 s bank: nothing happens at 49.9 s.
    assert!(!r.tick(&mut d, &T, start + 49_900));
    assert!(r.tick(&mut d, &T, start + 50_000));
    let g = r.game.as_ref().unwrap();
    let last =
        g.m.hand
            .actions
            .iter()
            .rev()
            .find(|a| a.kind != poker_core::ActionKind::Return);
    // Preflop the small blind faces a bet, so the timeout folds.
    assert_eq!(last.map(|a| a.kind), Some(poker_core::ActionKind::Fold));
    let slot = if actor == HOST {
        Slot::Host
    } else {
        Slot::Guest
    };
    assert_eq!(g.bank(slot, &T), 0, "the bank is spent");
    let view = r.view_for(actor, &T).unwrap();
    assert_eq!(
        view.game.unwrap().clock,
        None,
        "no clock during the hand result"
    );
}

#[test]
fn slow_actions_draw_from_the_bank() {
    let mut d = TestDealer::new(5);
    let mut r = ready_room(&mut d);
    let actor = to_act(&r);
    let start = r.game.as_ref().unwrap().turn_started_ms;
    let (v, id, a) = (version(&r), match_id(&r), passive(&r));
    r.action(actor, &id, "slow", v, a, &T, start + 26_000)
        .unwrap();
    let slot = if actor == HOST {
        Slot::Host
    } else {
        Slot::Guest
    };
    assert_eq!(r.game.as_ref().unwrap().bank(slot, &T), 24_000);
}

#[test]
fn a_dropped_player_has_a_minute_to_return() {
    let mut d = TestDealer::new(6);
    let mut r = ready_room(&mut d);
    r.set_connected(GUEST, false, &T, 1_000);
    let view = r.view_for(HOST, &T).unwrap();
    assert_eq!(view.opponent.unwrap().reconnect_deadline_ms, Some(61_000));
    // Coming back in time cancels the countdown.
    r.set_connected(GUEST, true, &T, 30_000);
    assert!(r.guest.as_ref().unwrap().grace_until_ms.is_none());

    r.set_connected(GUEST, false, &T, 40_000);
    // Keep the host from timing out while we wait.
    r.game.as_mut().unwrap().turn_started_ms = 99_000;
    assert!(!r.tick(&mut d, &T, 99_999));
    assert!(r.tick(&mut d, &T, 100_000));
    assert_eq!(r.status, Status::Finished);
    let result = r.game.as_ref().unwrap().m.result.clone().unwrap();
    assert_eq!(result.forfeit, Some(Seat::Bot));
    assert_eq!(r.score.host, 1);
    match &r.take_events()[..] {
        [Event::MatchFinished { forfeit, .. }] => assert_eq!(*forfeit, Some(Slot::Guest)),
        e => panic!("{e:?}"),
    }
}

#[test]
fn both_gone_aborts_without_a_result() {
    let mut d = TestDealer::new(7);
    let mut r = ready_room(&mut d);
    r.set_connected(GUEST, false, &T, 1_000);
    r.set_connected(HOST, false, &T, 2_000);
    r.game.as_mut().unwrap().turn_started_ms = 100_000;
    assert!(r.tick(&mut d, &T, 61_000));
    assert_eq!(
        (r.status, r.close_reason),
        (Status::Closed, Some(CloseReason::Aborted))
    );
    assert!(r.take_events().is_empty());
    assert_eq!(r.score, Score::default());
}

#[test]
fn surrender_is_an_immediate_technical_defeat() {
    let mut d = TestDealer::new(8);
    let mut r = ready_room(&mut d);
    r.surrender(HOST, 500).unwrap();
    assert_eq!(r.status, Status::Finished);
    let view = r.view_for(HOST, &T).unwrap().game.unwrap().view;
    let result = view.result.unwrap();
    assert_eq!(
        (result.outcome, result.forfeit),
        (MatchOutcome::Loss, Some(Seat::Human))
    );
    assert!(view.opponent_hole.is_none());
    assert_eq!(r.surrender(HOST, 600), Err(RoomError::WrongState));
}

#[test]
fn rematch_needs_both_players() {
    let mut d = TestDealer::new(9);
    let mut r = ready_room(&mut d);
    r.surrender(GUEST, 100).unwrap();
    let first = match_id(&r);
    assert_eq!(
        r.accept_rematch(GUEST, &mut d, 110),
        Err(RoomError::WrongState),
        "nothing to accept"
    );
    r.request_rematch(HOST, &mut d, 120).unwrap();
    assert_eq!(r.status, Status::Finished, "one request is not enough");
    assert!(r.view_for(GUEST, &T).unwrap().rematch.requested_by_opponent);
    r.decline_rematch(GUEST, 130).unwrap();
    assert!(r.view_for(HOST, &T).unwrap().rematch.declined_by_opponent);
    r.request_rematch(HOST, &mut d, 140).unwrap();
    r.accept_rematch(GUEST, &mut d, 150).unwrap();
    assert_eq!(r.status, Status::Playing);
    assert_ne!(match_id(&r), first, "a new, independent match");
    assert_eq!(
        r.game.as_ref().unwrap().m.stacks.iter().sum::<u32>(),
        1200 - 30
    );

    r.surrender(HOST, 200).unwrap();
    r.set_connected(GUEST, false, &T, 210);
    assert_eq!(
        r.request_rematch(HOST, &mut d, 220),
        Err(RoomError::OpponentAway)
    );
}

#[test]
fn rooms_expire_and_can_be_cancelled_or_left() {
    let mut d = TestDealer::new(10);
    let mut r = Room::new("room".into(), profile(HOST), "req".into(), 0);
    assert!(!r.tick(&mut d, &T, 899_999));
    assert!(r.tick(&mut d, &T, 900_000));
    assert_eq!(r.close_reason, Some(CloseReason::Expired));
    assert_eq!(r.join(profile(GUEST), 900_001), Err(RoomError::Closed));

    let mut r = Room::new("room".into(), profile(HOST), "req".into(), 0);
    r.join(profile(GUEST), 1).unwrap();
    r.set_ready(HOST, true, &mut d, 2).unwrap();
    r.leave(GUEST, 3).unwrap();
    assert_eq!(r.status, Status::Waiting, "the seat opens again");
    assert!(!r.host.ready);
    r.join(profile(GUEST), 4).unwrap();
    r.leave(HOST, 5).unwrap();
    assert_eq!(r.close_reason, Some(CloseReason::CancelledByHost));
    assert!(r.view_for(GUEST, &T).unwrap().close_reason == Some(CloseReason::CancelledByHost));

    let mut r = ready_room(&mut d);
    assert_eq!(
        r.leave(HOST, 100),
        Err(RoomError::WrongState),
        "use surrender mid-match"
    );
    r.surrender(HOST, 110).unwrap();
    r.leave(GUEST, 120).unwrap();
    let host_view = r.view_for(HOST, &T).unwrap();
    assert_eq!(
        (host_view.close_reason, host_view.left_by_me),
        (Some(CloseReason::Left), Some(false))
    );
}

/// A room saved mid-match and loaded after a restart: everyone is offline
/// with a long grace window, and no one loses because of the downtime.
#[test]
fn survives_a_restart_without_false_defeats() {
    let mut d = TestDealer::new(11);
    let mut r = ready_room(&mut d);
    let (who, v, id, a) = (to_act(&r), version(&r), match_id(&r), passive(&r));
    r.action(who, &id, "a", v, a, &T, 1_000).unwrap();
    let json = serde_json::to_string(&r).unwrap();

    // The server comes back an hour later.
    let mut back: Room = serde_json::from_str(&json).unwrap();
    let now = 3_600_000;
    back.restore_after_restart(&T, now);
    assert!(back.members().all(|m| !m.connected));
    assert!(
        !back.tick(&mut d, &T, now + 50_000),
        "the clock waits for the player to return"
    );
    back.set_connected(HOST, true, &T, now + 60_000);
    back.set_connected(GUEST, true, &T, now + 60_000);
    assert!(
        !back.tick(&mut d, &T, now + 70_000),
        "the turn starts over on return"
    );
    assert_eq!(back.game.as_ref().unwrap().turn_started_ms, now + 70_000);
    assert!(!back.tick(&mut d, &T, now + 119_999));
    assert_eq!(back.status, Status::Playing);
    assert_eq!(version(&back), v + 1);
    // Only now does the normal turn clock apply again.
    assert!(back.tick(&mut d, &T, now + 120_000));
}
