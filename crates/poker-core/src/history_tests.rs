use super::*;
use crate::engine::tests::setup;
use crate::engine::{PlayerAction, TOTAL_CHIPS};
use crate::rng::Rng;
use serde_json::Value;

const H: Seat = Seat::Human;
const B: Seat = Seat::Bot;
const DECK: &str = "As Qd Kc 9c Ad 7h 2c 5s Kh";

fn raise_to(amount: u32) -> PlayerAction {
    PlayerAction::RaiseTo { amount }
}

/// Replays the last finished hand and checks the per-frame invariants.
fn frames_of(m: &Match) -> Vec<ReplayFrame> {
    let record = m.hand_records(H, false).pop().expect("a finished hand");
    let frames = replay(&record).expect("replays");
    for f in &frames {
        assert_eq!(
            f.stacks[0] + f.stacks[1] + f.bets[0] + f.bets[1] + f.pot,
            record.start_stacks[0] + record.start_stacks[1],
            "chips conserved at {:?}",
            f.event
        );
    }
    let last = frames.last().unwrap();
    assert_eq!(last.stacks, m.stacks, "replay ends where the hand ended");
    assert!(matches!(last.event, ReplayEvent::Award { .. }));
    frames
}

fn kinds(frames: &[ReplayFrame]) -> Vec<String> {
    frames
        .iter()
        .map(|f| match &f.event {
            ReplayEvent::Start => "start".into(),
            ReplayEvent::Action { action } => format!("{:?}", action.kind).to_lowercase(),
            ReplayEvent::Street { street, runout } => {
                format!("{street:?}{}", if *runout { "*" } else { "" }).to_lowercase()
            }
            ReplayEvent::Showdown { .. } => "showdown".into(),
            ReplayEvent::Award { .. } => "award".into(),
            ReplayEvent::Unfinished => "unfinished".into(),
        })
        .collect()
}

#[test]
fn preflop_fold_returns_uncalled_raise_and_hides_cards() {
    let mut m = setup([600, 600], H, 1, DECK);
    m.apply(H, raise_to(60)).unwrap();
    m.apply(B, PlayerAction::Fold).unwrap();
    let frames = frames_of(&m);
    assert_eq!(
        kinds(&frames),
        [
            "start",
            "smallblind",
            "bigblind",
            "raise",
            "fold",
            "return",
            "award"
        ]
    );
    assert!(
        frames
            .iter()
            .all(|f| f.showdown.is_none() && f.board.is_empty())
    );
    let record = &m.hand_records(H, false)[0];
    assert_eq!(record.hole, Some(m.history[0].holes.unwrap()[0]));
}

#[test]
fn postflop_fold_reveals_only_dealt_streets() {
    let mut m = setup([600, 600], H, 1, DECK);
    m.apply(H, PlayerAction::Call).unwrap();
    m.apply(B, PlayerAction::Check).unwrap();
    m.apply(B, raise_to(40)).unwrap();
    m.apply(H, PlayerAction::Fold).unwrap();
    let frames = frames_of(&m);
    assert_eq!(
        kinds(&frames),
        [
            "start",
            "smallblind",
            "bigblind",
            "call",
            "check",
            "flop",
            "bet",
            "fold",
            "return",
            "award"
        ]
    );
    assert_eq!(frames.last().unwrap().board.len(), 3);
    assert_eq!(
        m.hand_records(H, false)[0].board.len(),
        3,
        "turn and river stay hidden"
    );
}

#[test]
fn checked_down_hand_reaches_showdown() {
    let mut m = setup([600, 600], B, 1, "2s 3d 2c 3h As Ks Qs Js Ts");
    m.apply(B, PlayerAction::Call).unwrap();
    m.apply(H, PlayerAction::Check).unwrap();
    for _ in 0..3 {
        m.apply(H, PlayerAction::Check).unwrap();
        m.apply(B, PlayerAction::Check).unwrap();
    }
    let frames = frames_of(&m);
    let k = kinds(&frames);
    assert_eq!(&k[k.len() - 2..], ["showdown", "award"]);
    assert!(k.contains(&"turn".to_string()) && k.contains(&"river".to_string()));
    let ReplayEvent::Award {
        winner, awarded, ..
    } = &frames.last().unwrap().event
    else {
        unreachable!()
    };
    assert_eq!((*winner, *awarded), (None, [20, 20]), "split pot");
}

#[test]
fn reraise_and_call_keeps_exact_bet_totals() {
    let mut m = setup([600, 600], H, 1, DECK);
    m.apply(H, raise_to(60)).unwrap();
    m.apply(B, raise_to(180)).unwrap();
    m.apply(H, PlayerAction::Call).unwrap();
    let flop = m.view(H);
    assert_eq!((flop.pot, flop.board.len()), (360, 3));
    m.apply(B, PlayerAction::Check).unwrap();
    m.apply(H, PlayerAction::Check).unwrap();
    m.apply(B, PlayerAction::Check).unwrap();
    m.apply(H, PlayerAction::Check).unwrap();
    m.apply(B, PlayerAction::Check).unwrap();
    m.apply(H, PlayerAction::Check).unwrap();
    let frames = frames_of(&m);
    let flop_frame = frames
        .iter()
        .find(|f| {
            f.event
                == ReplayEvent::Street {
                    street: Street::Flop,
                    runout: false,
                }
        })
        .unwrap();
    assert_eq!((flop_frame.pot, flop_frame.bets), (360, [0, 0]));
}

#[test]
fn all_in_runout_is_marked() {
    let mut m = setup([600, 600], H, 1, DECK);
    m.apply(H, raise_to(600)).unwrap();
    m.apply(B, PlayerAction::Call).unwrap();
    let frames = frames_of(&m);
    assert_eq!(
        kinds(&frames)[5..],
        ["flop*", "turn*", "river*", "showdown", "award"]
    );
}

#[test]
fn blind_all_in_without_any_decision_replays() {
    let m = setup([1195, 5], H, 1, DECK);
    let frames = frames_of(&m);
    assert_eq!(
        kinds(&frames),
        [
            "start",
            "smallblind",
            "bigblind",
            "return",
            "flop*",
            "turn*",
            "river*",
            "showdown",
            "award"
        ]
    );
}

#[test]
fn unfinished_hand_stops_at_the_live_state() {
    let mut m = setup([600, 600], H, 1, DECK);
    m.apply(H, PlayerAction::Call).unwrap();
    m.apply(B, PlayerAction::Check).unwrap();
    m.apply(B, raise_to(40)).unwrap();
    let records = m.hand_records(H, true);
    assert_eq!(records.len(), 1);
    assert!(records[0].outcome.is_none());
    let frames = replay(&records[0]).unwrap();
    let last = frames.last().unwrap();
    assert_eq!(last.event, ReplayEvent::Unfinished);
    let v = m.view(H);
    assert_eq!(
        (last.stacks, last.bets, last.pot, &last.board),
        (v.stacks, v.bets, v.pot, &v.board)
    );
    assert!(m.hand_records(H, false).is_empty());
}

#[test]
fn corrupt_records_are_rejected_not_panicking() {
    let mut m = setup([600, 600], H, 1, DECK);
    m.apply(H, raise_to(600)).unwrap();
    m.apply(B, PlayerAction::Call).unwrap();
    let good = m.hand_records(H, false).remove(0);

    let mut short_board = good.clone();
    short_board.board.truncate(3);
    assert!(replay(&short_board).is_err());

    let mut wrong_amount = good.clone();
    wrong_amount.actions[2].amount += 5;
    assert!(replay(&wrong_amount).is_err());

    let mut wrong_stacks = good;
    wrong_stacks.start_stacks = [10, 10];
    assert!(replay(&wrong_stacks).is_err());
}

/// Public table state as the live engine shows it to the human.
fn live(m: &Match) -> (Street, Vec<Card>, [u32; 2], [u32; 2], u32) {
    let v = m.view(H);
    (v.street, v.board, v.stacks, v.bets, v.pot)
}

fn frame_state(f: &ReplayFrame) -> (Street, Vec<Card>, [u32; 2], [u32; 2], u32) {
    (f.street, f.board.clone(), f.stacks, f.bets, f.pot)
}

fn card_strings(cards: &[Card]) -> Vec<String> {
    cards.iter().map(|c| format!("\"{c}\"")).collect()
}

/// Random legal play: every live state appears, in order, in the replay of
/// the hand; the bot's projection never contains unrevealed human cards; a
/// mid-hand record replays to exactly the live state.
#[test]
fn replay_agrees_with_live_play_fuzz() {
    for seed in 0..1500u64 {
        let mut m = Match::new(seed);
        let mut rng = Rng::new(seed ^ 0x5EED);
        let mut seen = vec![live(&m)];
        while m.phase != Phase::MatchOver {
            match m.phase {
                Phase::InHand => {
                    let seat = m.hand.to_act.unwrap();
                    let legal = m.legal_actions(seat).unwrap();
                    let mut options = vec![];
                    if legal.fold && rng.chance(0.25) {
                        options.push(PlayerAction::Fold);
                    }
                    if legal.check {
                        options.push(PlayerAction::Check);
                    }
                    if legal.call.is_some() {
                        options.push(PlayerAction::Call);
                    }
                    if let Some(r) = &legal.raise {
                        let span = (r.max_to - r.min_to) as u64 + 1;
                        options.push(raise_to(r.min_to + rng.below(span) as u32));
                    }
                    m.apply(seat, options[rng.below(options.len() as u64) as usize])
                        .unwrap();
                    if m.phase == Phase::InHand {
                        seen.push(live(&m));
                        let partial = m.hand_records(H, true).pop().unwrap();
                        let last = replay(&partial).unwrap().pop().unwrap();
                        assert_eq!(frame_state(&last), live(&m), "seed {seed}: partial replay");
                    }
                }
                Phase::HandOver => {
                    let frames = frames_of(&m);
                    let mut it = frames.iter().map(frame_state);
                    for state in &seen {
                        assert!(
                            it.any(|f| &f == state),
                            "seed {seed} hand {}: live state {state:?} missing from replay",
                            m.hand.number
                        );
                    }

                    // Cards repeat across hands, so check each hand on its own.
                    let bot_view = m.view(B);
                    for (record, h) in bot_view.history.iter().zip(&m.history) {
                        if h.outcome.showdown.is_none() {
                            let json = serde_json::to_string(record).unwrap();
                            for c in card_strings(&h.holes.unwrap()[0]) {
                                assert!(!json.contains(&c), "seed {seed}: bot sees {c}");
                            }
                        }
                    }
                    m.next_hand().unwrap();
                    if m.phase == Phase::InHand {
                        let mut current = m.view(B);
                        current.history.clear();
                        let bot_json = serde_json::to_string(&current).unwrap();
                        for c in card_strings(&m.hand.hole(H)) {
                            assert!(!bot_json.contains(&c), "seed {seed}: bot sees live {c}");
                        }
                    }
                    seen = vec![live(&m)];
                }
                Phase::MatchOver => unreachable!(),
            }
        }
        let total: u32 = m.stacks.iter().sum();
        assert_eq!(total, TOTAL_CHIPS);
    }
}

/// A 1.0 snapshot has no `start_stacks` or `holes` in its history. It must
/// still load, and its hands must still replay (without hole cards).
#[test]
fn old_saves_without_new_history_fields_still_load() {
    let mut m = Match::new(7);
    let mut rng = Rng::new(1);
    while m.history.len() < 3 && m.phase != Phase::MatchOver {
        match m.phase {
            Phase::InHand => {
                let seat = m.hand.to_act.unwrap();
                let l = m.legal_actions(seat).unwrap();
                let a = if l.check {
                    PlayerAction::Check
                } else if rng.chance(0.5) {
                    PlayerAction::Call
                } else {
                    PlayerAction::Fold
                };
                m.apply(seat, a).unwrap();
            }
            _ => m.next_hand().unwrap(),
        }
    }
    let mut json: Value = serde_json::to_value(&m).unwrap();
    for h in json["history"].as_array_mut().unwrap() {
        let h = h.as_object_mut().unwrap();
        h.remove("start_stacks");
        h.remove("holes");
    }
    let mut old: Match = serde_json::from_value(json).unwrap();
    old.upgrade();
    old.validate().unwrap();
    for (restored, original) in old.history.iter().zip(&m.history) {
        assert_eq!(restored.start_stacks, original.start_stacks);
    }
    for record in old.hand_records(H, false) {
        assert_eq!(record.hole, None);
        replay(&record).unwrap();
    }
}
