use super::*;
use crate::card::cards as parse_cards;
use crate::engine::{HandOutcome, Match, MatchOutcome, Seat, blinds};

const P: SeatKind = SeatKind::Player;

/// A hand with chosen stacks (summing to 600 per seat), button, hand
/// number, hole cards per seat and board.
fn setup(stacks: &[u32], button: usize, number: u8, holes: &[&str], board: &str) -> Table {
    let n = stacks.len();
    assert_eq!(holes.len(), n);
    assert_eq!(stacks.iter().sum::<u32>(), STARTING_STACK * n as u32);
    let holes: Vec<Vec<Card>> = holes.iter().map(|h| parse_cards(h)).collect();
    let mut deck: Vec<Card> = holes.iter().map(|h| h[0]).collect();
    deck.extend(holes.iter().map(|h| h[1]));
    deck.extend(parse_cards(board));
    let mut t = Table::empty("test".into(), TableKind::Online, vec![P; n], Rng::new(1));
    t.stacks = stacks.to_vec();
    t.start_hand(number, button, Some(deck));
    t
}

fn raise_to(amount: u32) -> PlayerAction {
    PlayerAction::RaiseTo { amount }
}

fn act(t: &mut Table, seat: usize, a: PlayerAction) {
    assert_eq!(t.hand.to_act, Some(seat), "turn order");
    t.apply(seat, a)
        .unwrap_or_else(|e| panic!("seat {seat} {a:?}: {e:?}"));
}

fn random_action(legal: &LegalActions, rng: &mut Rng) -> PlayerAction {
    let mut options = Vec::new();
    if legal.fold && rng.chance(0.3) {
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
        options.push(raise_to(r.max_to));
    }
    options[rng.below(options.len() as u64) as usize]
}

#[test]
fn match_length_and_blinds_follow_orbits() {
    let hands: Vec<u8> = (2..=6).map(hands_for).collect();
    assert_eq!(hands, [8, 9, 8, 10, 12]);
    for number in 1..=8 {
        assert_eq!(table_blinds(number, 2), blinds(number), "duel schedule");
    }
    let three: Vec<(u32, u32)> = (1..=9).map(|h| table_blinds(h, 3)).collect();
    assert_eq!(three[..3], [(10, 20); 3]);
    assert_eq!(three[3..6], [(15, 30); 3]);
    assert_eq!(three[6..], [(20, 40); 3]);
}

#[test]
fn three_handed_positions_and_turn_order() {
    let mut t = setup(
        &[600, 600, 600],
        0,
        1,
        &["As Ad", "Ks Kd", "7c 2h"],
        "Qh 9c 4d 3s 8h",
    );
    assert_eq!((t.hand.sb_seat, t.hand.bb_seat), (1, 2));
    assert_eq!(t.hand.bets, [0, 10, 20]);
    // Preflop the button (after the big blind) opens.
    act(&mut t, 0, PlayerAction::Call);
    act(&mut t, 1, PlayerAction::Call);
    act(&mut t, 2, PlayerAction::Check);
    assert_eq!(t.hand.street, Street::Flop);
    assert_eq!(t.hand.pot, 60);
    // Postflop the small blind opens.
    assert_eq!(t.hand.to_act, Some(1));
    act(&mut t, 1, PlayerAction::Check);
    act(&mut t, 2, raise_to(40));
    act(&mut t, 0, PlayerAction::Fold);
    act(&mut t, 1, PlayerAction::Call);
    assert_eq!(t.hand.street, Street::Turn);
    assert_eq!(t.hand.to_act, Some(1), "folded button is skipped");
}

#[test]
fn all_ins_for_different_amounts_make_side_pots() {
    let mut t = setup(
        &[100, 300, 1400],
        0,
        1,
        &["As Ad", "Ks Kd", "7c 2h"],
        "Qh 9c 4d 3s 8h",
    );
    act(&mut t, 0, raise_to(100));
    act(&mut t, 1, raise_to(300));
    let legal = t.legal_actions(2).unwrap();
    assert_eq!(legal.raise, None, "nobody left to raise against");
    act(&mut t, 2, PlayerAction::Call);
    assert_eq!(t.phase, Phase::HandOver);
    let o = t.hand.outcome.clone().unwrap();
    assert_eq!(o.runout_from, Some(0));
    assert_eq!(
        o.pots,
        [
            PotShare {
                amount: 300,
                eligible: vec![0, 1, 2],
                winners: vec![0]
            },
            PotShare {
                amount: 400,
                eligible: vec![1, 2],
                winners: vec![1]
            },
        ]
    );
    assert_eq!(t.stacks, [300, 400, 1100]);
    assert_eq!(o.net, [200, 100, -300]);
    assert_eq!(o.winner(), Some(0));
}

#[test]
fn bigger_stack_gets_its_unmatched_raise_back() {
    let mut t = setup(
        &[100, 300, 1400],
        0,
        1,
        &["As Ad", "Ks Kd", "7c 2h"],
        "Qh 9c 4d 3s 8h",
    );
    act(&mut t, 0, raise_to(100));
    act(&mut t, 1, PlayerAction::Fold);
    // Seat 2 can raise no further than the 100 seat 0 has.
    assert_eq!(t.legal_actions(2).unwrap().raise, None);
    act(&mut t, 2, PlayerAction::Call);
    let o = t.hand.outcome.clone().unwrap();
    assert_eq!(o.pot, 210);
    assert_eq!(t.stacks, [210, 290, 1300]);
}

#[test]
fn short_all_in_does_not_reopen_raising() {
    let mut t = setup(
        &[600, 130, 1070],
        0,
        1,
        &["As Ad", "Ks Kd", "7c 2h"],
        "Qh 9c 4d 3s 8h",
    );
    act(&mut t, 0, raise_to(100));
    act(&mut t, 1, raise_to(130)); // all-in, 30 more: not a full raise
    assert!(
        t.legal_actions(2).unwrap().raise.is_some(),
        "seat 2 has not acted yet"
    );
    act(&mut t, 2, PlayerAction::Call);
    let legal = t.legal_actions(0).unwrap();
    assert_eq!(legal.call, Some(30));
    assert_eq!(legal.raise, None, "seat 0 already acted on a full raise");
}

#[test]
fn full_raise_reopens_raising() {
    let mut t = setup(
        &[600, 600, 600],
        0,
        1,
        &["As Ad", "Ks Kd", "7c 2h"],
        "Qh 9c 4d 3s 8h",
    );
    act(&mut t, 0, raise_to(60));
    act(&mut t, 1, PlayerAction::Call);
    act(&mut t, 2, raise_to(200));
    assert!(t.legal_actions(0).unwrap().raise.is_some());
}

#[test]
fn split_pot_odd_chip_goes_left_of_the_button() {
    // Hand 4 of a three-seat match: blinds 15/30, so the pot is odd.
    let mut t = setup(
        &[600, 600, 600],
        0,
        4,
        &["2c 3d", "4c 5d", "2d 3c"],
        "Ts Js Qs Ks As",
    );
    assert_eq!((t.hand.sb, t.hand.bb), (15, 30));
    act(&mut t, 0, raise_to(60));
    act(&mut t, 1, PlayerAction::Fold);
    act(&mut t, 2, PlayerAction::Call);
    for _ in 0..3 {
        act(&mut t, 2, PlayerAction::Check);
        act(&mut t, 0, PlayerAction::Check);
    }
    let o = t.hand.outcome.clone().unwrap();
    assert_eq!(o.pot, 135);
    assert_eq!(o.pots[0].winners, [0, 2]);
    assert_eq!(o.awarded, [67, 0, 68], "seat 2 is first after the button");
    assert_eq!(o.winner(), None);
}

#[test]
fn busted_seat_sits_out_and_heads_up_rules_return() {
    let mut t = setup(
        &[890, 890, 20],
        0,
        1,
        &["As Ad", "Ks Kd", "7c 2h"],
        "Qh 9c 4d 3s 8h",
    );
    act(&mut t, 0, PlayerAction::Call);
    act(&mut t, 1, PlayerAction::Call);
    // The all-in big blind has nothing to decide.
    for _ in 0..3 {
        act(&mut t, 1, PlayerAction::Check);
        act(&mut t, 0, PlayerAction::Check);
    }
    assert_eq!(t.phase, Phase::HandOver);
    assert_eq!(t.stacks, [930, 870, 0]);
    assert!(!t.is_final_hand());
    t.next_hand_dealt(&full_deck()).unwrap();
    assert_eq!(t.hand.dealt, [true, true, false]);
    assert_eq!(t.hand.button, 1);
    assert_eq!((t.hand.sb_seat, t.hand.bb_seat), (1, 0), "button posts SB");
    assert_eq!(t.hand.to_act, Some(1), "and acts first preflop");
    let v = t.view(2);
    assert_eq!(v.hole, None);
    assert_eq!(v.legal, None);
}

#[test]
fn every_seat_posts_every_blind_level_equally() {
    let mut t = Table::solo(5, vec![P; 3]).unwrap();
    // Everyone folds to the big blind: nobody busts.
    while t.phase != Phase::MatchOver {
        if t.phase == Phase::HandOver {
            t.next_hand().unwrap();
            continue;
        }
        let s = t.hand.to_act.unwrap();
        let a = if t.legal_actions(s).unwrap().check {
            PlayerAction::Check
        } else {
            PlayerAction::Fold
        };
        t.apply(s, a).unwrap();
    }
    assert_eq!(t.history.len(), 9);
    for seat in 0..3 {
        let posted: Vec<(u32, u32)> = t
            .history
            .iter()
            .filter(|h| h.bb_seat == seat)
            .map(|h| (h.sb, h.bb))
            .collect();
        assert_eq!(posted, [(10, 20), (15, 30), (20, 40)], "seat {seat}");
        assert_eq!(t.history.iter().filter(|h| h.button == seat).count(), 3);
        assert_eq!(t.history.iter().filter(|h| h.sb_seat == seat).count(), 3);
    }
}

#[test]
fn leaving_folds_and_ranks_below_everyone_who_stayed() {
    let mut t = setup(
        &[600, 600, 600],
        0,
        1,
        &["As Ad", "Ks Kd", "7c 2h"],
        "Qh 9c 4d 3s 8h",
    );
    // Seat 1 leaves while seat 0 is to act.
    t.leave(1).unwrap();
    assert!(t.hand.folded[1]);
    assert_eq!(t.hand.to_act, Some(0));
    assert_eq!(t.leave(1), Err(EngineError::IllegalAction));
    act(&mut t, 0, PlayerAction::Fold);
    assert_eq!(t.phase, Phase::HandOver);
    assert_eq!(t.stacks, [600, 590, 610]);
    t.next_hand_dealt(&full_deck()).unwrap();
    assert_eq!(t.hand.dealt, [true, false, true]);
    // The last opponent leaves too: the hand and the match end at once.
    t.leave(0).unwrap();
    assert_eq!(t.phase, Phase::MatchOver);
    let r = t.result.unwrap();
    assert_eq!(r.places, [2, 3, 1]);
    assert!(r.ended_early);
}

#[test]
fn equal_stacks_share_a_place() {
    let mut t = Table::solo(1, vec![P; 3]).unwrap();
    t.stacks = vec![700, 400, 700];
    assert_eq!(t.places(), [1, 3, 1]);
}

#[test]
fn view_shows_only_own_cards_until_showdown() {
    let mut t = setup(
        &[600, 600, 600],
        0,
        1,
        &["As Ad", "Ks Kd", "7c 2h"],
        "Qh 9c 4d 3s 8h",
    );
    let json = serde_json::to_string(&t.view(1)).unwrap();
    assert!(json.contains("\"Ks\""));
    for hidden in ["As", "Ad", "7c", "2h", "Qh"] {
        assert!(!json.contains(&format!("\"{hidden}\"")), "{hidden} leaked");
    }
    act(&mut t, 0, PlayerAction::Call);
    act(&mut t, 1, PlayerAction::Fold);
    act(&mut t, 2, PlayerAction::Check);
    for _ in 0..3 {
        act(&mut t, 2, PlayerAction::Check);
        act(&mut t, 0, PlayerAction::Check);
    }
    let v = t.view(1);
    assert_eq!(v.shown[0], Some(t.hand.hole(0)));
    assert_eq!(v.shown[1], None, "a folded hand is never shown");
    assert_eq!(v.history[0].hole, Some(t.hand.hole(1)));
}

#[test]
fn rejects_bad_tables() {
    assert!(Table::solo(1, vec![P]).is_err());
    assert!(Table::solo(1, vec![P; MAX_SEATS + 1]).is_err());
    assert!(Table::online("x".into(), vec![P; 3], 3, &full_deck()).is_err());
    assert!(Table::online("x".into(), vec![P; 6], 0, &full_deck()[..16]).is_err());
}

/// Random tables of every size with random actions and the odd seat
/// leaving: invariants hold on every transition, snapshots round-trip.
#[test]
fn random_play_fuzz() {
    for seed in 0..1500u64 {
        let n = MIN_SEATS + (seed as usize % (MAX_SEATS - 1));
        let seats = (0..n)
            .map(|s| {
                if s == 0 {
                    P
                } else {
                    SeatKind::Bot {
                        opponent: Opponent::ALL[s % Opponent::ALL.len()],
                    }
                }
            })
            .collect();
        let mut t = Table::solo(seed, seats).unwrap();
        let mut rng = Rng::new(seed ^ 0x5EA7);
        let mut steps = 0;
        while t.phase != Phase::MatchOver {
            steps += 1;
            assert!(steps < 2000, "match did not terminate");
            assert_eq!(t.chips_in_play(), t.total_chips());
            t.validate().unwrap();
            if steps % 7 == 0 {
                let json = serde_json::to_string(&t).unwrap();
                let back: Table = serde_json::from_str(&json).unwrap();
                assert_eq!(back, t, "snapshot round trip");
            }
            if rng.chance(0.003) {
                let seat = rng.below(n as u64) as usize;
                let _ = t.leave(seat);
                continue;
            }
            match t.phase {
                Phase::InHand => {
                    let seat = t.hand.to_act.unwrap();
                    assert!(t.hand.dealt[seat] && !t.hand.folded[seat]);
                    assert!(t.stacks[seat] > 0);
                    for other in (0..n).filter(|&s| s != seat) {
                        assert_eq!(t.legal_actions(other), None);
                    }
                    let a = random_action(&t.legal_actions(seat).unwrap(), &mut rng);
                    t.apply(seat, a).unwrap();
                }
                Phase::HandOver => {
                    let o = t.hand.outcome.as_ref().unwrap();
                    assert_eq!(o.net.iter().sum::<i32>(), 0);
                    assert_eq!(o.awarded.iter().sum::<u32>(), o.pot);
                    assert_eq!(o.pots.iter().map(|p| p.amount).sum::<u32>(), o.pot);
                    assert_eq!(o.by_fold, o.showdown.is_none());
                    t.next_hand().unwrap();
                }
                Phase::MatchOver => unreachable!(),
            }
        }
        let r = t.result.as_ref().unwrap();
        assert!(r.hands_played <= hands_for(n));
        assert_eq!(r.final_stacks.iter().sum::<u32>(), t.total_chips());
        let mut places = r.places.clone();
        places.sort_unstable();
        assert_eq!(places[0], 1);
        assert!(places.iter().all(|&p| (1..=n as u8).contains(&p)));
    }
}

fn seat(i: usize) -> Seat {
    if i == 0 { Seat::Human } else { Seat::Bot }
}

fn same_outcome(o: &HandOutcome, t: &TableOutcome) {
    assert_eq!(o.by_fold, t.by_fold);
    assert_eq!(o.ended_on, t.ended_on);
    assert_eq!(o.pot, t.pot);
    assert_eq!(o.awarded.to_vec(), t.awarded);
    assert_eq!(o.net.to_vec(), t.net);
    assert_eq!(o.runout_from, t.runout_from);
    assert_eq!(o.winner.map(Seat::idx), t.winner());
    assert_eq!(
        o.showdown
            .map(|s| s.into_iter().map(Some).collect::<Vec<_>>()),
        t.showdown
    );
}

fn same_state(m: &Match, t: &Table) {
    assert_eq!(m.phase, t.phase);
    assert_eq!(m.stacks.to_vec(), t.stacks);
    assert_eq!(m.hand.bets.to_vec(), t.hand.bets);
    assert_eq!(m.hand.pot, t.hand.pot);
    assert_eq!(m.hand.street, t.hand.street);
    assert_eq!(m.hand.board(), t.hand.board());
    assert_eq!(m.hand.to_act.map(Seat::idx), t.hand.to_act);
    assert_eq!(m.hand.button.idx(), t.hand.button);
    assert_eq!((m.hand.sb, m.hand.bb), (t.hand.sb, t.hand.bb));
    for i in 0..2 {
        assert_eq!(m.legal_actions(seat(i)), t.legal_actions(i));
        assert_eq!(m.hand.hole(seat(i)), t.hand.hole(i));
    }
    assert_eq!(m.hand.actions.len(), t.hand.actions.len());
    for (a, b) in m.hand.actions.iter().zip(&t.hand.actions) {
        assert_eq!(
            (a.seat.idx(), a.street, a.kind, a.amount, a.to, a.all_in),
            (b.seat, b.street, b.kind, b.amount, b.to, b.all_in)
        );
    }
    match (&m.hand.outcome, &t.hand.outcome) {
        (Some(o), Some(to)) => same_outcome(o, to),
        (None, None) => {}
        other => panic!("outcome mismatch: {other:?}"),
    }
    assert_eq!(m.is_final_hand(), t.is_final_hand());
    match (&m.result, &t.result) {
        (Some(r), Some(tr)) => {
            assert_eq!(r.final_stacks.to_vec(), tr.final_stacks);
            assert_eq!(r.hands_played, tr.hands_played);
            assert_eq!(r.hands_won.to_vec(), tr.hands_won);
            assert_eq!(r.ended_early, tr.ended_early);
            assert_eq!(r.best_hand, tr.best_hands[0]);
            assert_eq!(r.opponent_best_hand, tr.best_hands[1]);
            let places = match r.outcome {
                MatchOutcome::Win => [1, 2],
                MatchOutcome::Loss => [2, 1],
                MatchOutcome::Draw => [1, 1],
            };
            assert_eq!(tr.places, places);
        }
        (None, None) => {}
        other => panic!("result mismatch: {other:?}"),
    }
}

/// With two seats the table must play exactly like the duel engine: same
/// decks, same actions, identical state after every step.
#[test]
fn heads_up_matches_the_duel_engine() {
    for game in 0..3000u64 {
        let mut rng = Rng::new(game);
        let deal = |rng: &mut Rng| {
            let mut d = full_deck();
            rng.shuffle(&mut d);
            d
        };
        let button = rng.below(2) as usize;
        let deck = deal(&mut rng);
        let mut m = Match::duel("x".into(), seat(button), &deck).unwrap();
        let mut t = Table::online("x".into(), vec![P; 2], button, &deck).unwrap();
        let mut steps = 0;
        loop {
            steps += 1;
            assert!(steps < 500);
            same_state(&m, &t);
            match m.phase {
                Phase::InHand => {
                    let s = m.hand.to_act.unwrap();
                    let a = random_action(&m.legal_actions(s).unwrap(), &mut rng);
                    m.apply(s, a).unwrap();
                    t.apply(s.idx(), a).unwrap();
                }
                Phase::HandOver if m.is_final_hand() => {
                    m.next_hand().unwrap();
                    t.next_hand().unwrap();
                }
                Phase::HandOver => {
                    let deck = deal(&mut rng);
                    m.next_hand_dealt(&deck).unwrap();
                    t.next_hand_dealt(&deck).unwrap();
                }
                Phase::MatchOver => break,
            }
        }
    }
}
