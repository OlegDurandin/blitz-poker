use super::*;
use crate::card::{Card, full_deck};
use crate::engine::{EngineError, MatchKind, Phase, PlayerAction};
use crate::rng::Rng;

fn deck(rng: &mut Rng) -> Vec<Card> {
    let mut d = full_deck();
    rng.shuffle(&mut d);
    d
}

fn random_action(m: &Match, seat: Seat, rng: &mut Rng) -> PlayerAction {
    let l = m.legal_actions(seat).unwrap();
    let mut options = vec![];
    if l.fold && rng.chance(0.2) {
        options.push(PlayerAction::Fold);
    }
    if l.check {
        options.push(PlayerAction::Check);
    }
    if l.call.is_some() {
        options.push(PlayerAction::Call);
    }
    if let Some(r) = &l.raise {
        let span = (r.max_to - r.min_to) as u64 + 1;
        options.push(PlayerAction::RaiseTo {
            amount: r.min_to + rng.below(span) as u32,
        });
    }
    options[rng.below(options.len() as u64) as usize]
}

fn card_strings(cards: &[Card]) -> Vec<String> {
    cards.iter().map(|c| format!("\"{c}\"")).collect()
}

/// Random duels: the guest's view is the host's layout with seats swapped,
/// never shows the host's hidden cards, and mirroring twice is the identity.
#[test]
fn duel_views_fuzz() {
    for seed in 0..800u64 {
        let mut rng = Rng::new(seed ^ 0xD0E1);
        let button = if rng.chance(0.5) {
            Seat::Human
        } else {
            Seat::Bot
        };
        let mut m = Match::duel(format!("d{seed}"), button, &deck(&mut rng)).unwrap();
        assert_eq!(m.kind, MatchKind::Duel);
        while m.phase != Phase::MatchOver {
            for seat in Seat::BOTH {
                let v = m.view_from(seat);
                assert_eq!(v.me, Seat::Human, "everyone sees themselves at the bottom");
                assert_eq!(v.hole, m.hand.hole(seat));
                assert_eq!(v.stacks[0], m.stacks[seat.idx()]);
                assert_eq!(v.bets[0], m.hand.bets[seat.idx()]);
                assert_eq!(v.to_act == Some(Seat::Human), m.hand.to_act == Some(seat));
                assert_eq!(
                    v.legal.is_some(),
                    m.hand.to_act == Some(seat) && m.phase == Phase::InHand
                );
                assert_eq!(v.clone().mirrored().mirrored(), v);
                if m.phase == Phase::InHand {
                    let mut current = v.clone();
                    current.history.clear();
                    let json = serde_json::to_string(&current).unwrap();
                    for c in card_strings(&m.hand.hole(seat.other())) {
                        assert!(!json.contains(&c), "seed {seed}: {seat:?} sees {c}");
                    }
                }
            }
            match m.phase {
                Phase::InHand => {
                    let seat = m.hand.to_act.unwrap();
                    let a = random_action(&m, seat, &mut rng);
                    m.apply(seat, a).unwrap();
                }
                Phase::HandOver => {
                    if m.is_final_hand() {
                        m.next_hand().unwrap();
                    } else {
                        // Duels never deal from the engine's own RNG.
                        assert_eq!(m.next_hand(), Err(EngineError::IllegalAction));
                        m.next_hand_dealt(&deck(&mut rng)).unwrap();
                    }
                }
                Phase::MatchOver => unreachable!(),
            }
            assert_eq!(m.chips_in_play(), crate::TOTAL_CHIPS);
        }
        let host = m.view_from(Seat::Human).result.unwrap();
        let guest = m.view_from(Seat::Bot).result.unwrap();
        assert_eq!(
            host.final_stacks,
            [guest.final_stacks[1], guest.final_stacks[0]]
        );
        assert_eq!(host.best_hand, guest.opponent_best_hand);
        match host.outcome {
            MatchOutcome::Draw => assert_eq!(guest.outcome, MatchOutcome::Draw),
            o => assert_ne!(guest.outcome, o),
        }
    }
}

#[test]
fn duel_decks_are_validated() {
    let d = full_deck();
    assert!(Match::duel("x".into(), Seat::Human, &d[..8]).is_err());
    let mut dup = d.clone();
    dup[3] = dup[0];
    assert!(Match::duel("x".into(), Seat::Human, &dup).is_err());
    let m = Match::duel("x".into(), Seat::Bot, &d).unwrap();
    assert_eq!(m.hand.hole(Seat::Human), [d[0], d[2]]);
    m.validate().unwrap();
}

#[test]
fn forfeit_ends_the_match_against_the_loser() {
    let mut rng = Rng::new(9);
    let mut m = Match::duel("f".into(), Seat::Human, &deck(&mut rng)).unwrap();
    m.apply(Seat::Human, PlayerAction::Call).unwrap();
    // The bot seat (guest) is ahead in nothing; it simply leaves mid-hand.
    m.forfeit(Seat::Bot).unwrap();
    assert_eq!(m.phase, Phase::MatchOver);
    m.validate().unwrap();
    let host = m.view_from(Seat::Human);
    let r = host.result.clone().unwrap();
    assert_eq!((r.outcome, r.forfeit), (MatchOutcome::Win, Some(Seat::Bot)));
    assert!(r.ended_early);
    assert_eq!(r.hands_played, 0, "the abandoned hand is not counted");
    assert!(
        host.opponent_hole.is_none(),
        "no cards revealed by a forfeit"
    );
    let guest = m.view_from(Seat::Bot).result.unwrap();
    assert_eq!(
        (guest.outcome, guest.forfeit),
        (MatchOutcome::Loss, Some(Seat::Human))
    );
    assert_eq!(m.forfeit(Seat::Human), Err(EngineError::WrongPhase));
}

#[test]
fn timeout_checks_when_free_and_folds_otherwise() {
    let mut rng = Rng::new(4);
    let mut m = Match::duel("t".into(), Seat::Human, &deck(&mut rng)).unwrap();
    assert_eq!(
        m.timeout_action(Seat::Human),
        Some(PlayerAction::Fold),
        "small blind faces the big blind"
    );
    assert_eq!(m.timeout_action(Seat::Bot), None, "not their turn");
    m.apply(Seat::Human, PlayerAction::Call).unwrap();
    assert_eq!(m.timeout_action(Seat::Bot), Some(PlayerAction::Check));
}
