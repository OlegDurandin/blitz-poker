use super::*;
use crate::eval::Category;

const H: Seat = Seat::Human;
const B: Seat = Seat::Bot;

/// A hand with chosen stacks, button and deck (`h0 b0 h1 b1 flop turn river`).
pub(crate) fn setup(stacks: [u32; 2], button: Seat, number: u8, deck: &str) -> Match {
    let mut m = Match::empty("test".into(), MatchKind::Standard, Rng::new(1));
    m.stacks = stacks;
    m.start_hand(number, button, Some(parse_cards(deck)));
    m
}

const DECK: &str = "As Qd Kc 9c Ad 7h 2c 5s Kh";

fn raise_to(amount: u32) -> PlayerAction {
    PlayerAction::RaiseTo { amount }
}

fn assert_chips(m: &Match) {
    assert_eq!(
        m.chips_in_play(),
        TOTAL_CHIPS,
        "chip invariant broken: {m:#?}"
    );
}

#[test]
fn new_match_posts_blinds_and_button_acts_first() {
    let m = Match::new(12345);
    let button = m.hand.button;
    assert_eq!(m.hand.number, 1);
    assert_eq!(m.hand.bets[button.idx()], 10);
    assert_eq!(m.hand.bets[button.other().idx()], 20);
    assert_eq!(m.hand.to_act, Some(button));
    assert_chips(&m);
    m.validate().unwrap();
}

#[test]
fn small_blind_faces_call_of_the_difference() {
    let m = setup([600, 600], H, 1, DECK);
    let legal = m.legal_actions(H).unwrap();
    assert!(legal.fold && !legal.check);
    assert_eq!(legal.call, Some(10));
    let r = legal.raise.unwrap();
    assert_eq!((r.kind, r.min_to, r.max_to), (RaiseKind::Raise, 40, 600));
    assert!(m.legal_actions(B).is_none(), "not the bot's turn");
}

#[test]
fn big_blind_gets_option_after_limp() {
    let mut m = setup([600, 600], H, 1, DECK);
    m.apply(H, PlayerAction::Call).unwrap();
    assert_eq!(m.hand.to_act, Some(B));
    let legal = m.legal_actions(B).unwrap();
    assert!(legal.check && !legal.fold && legal.call.is_none());
    assert_eq!(legal.raise.unwrap().min_to, 40);
    m.apply(B, PlayerAction::Check).unwrap();
    assert_eq!(m.hand.street, Street::Flop);
    assert_eq!(m.hand.pot, 40);
    assert_eq!(m.hand.board().len(), 3);
    // Postflop the non-button acts first and may bet from one big blind.
    assert_eq!(m.hand.to_act, Some(B));
    let r = m.legal_actions(B).unwrap().raise.unwrap();
    assert_eq!((r.kind, r.min_to), (RaiseKind::Bet, 20));
    assert_chips(&m);
}

#[test]
fn min_raise_tracks_last_full_raise() {
    let mut m = setup([600, 600], H, 1, DECK);
    m.apply(H, PlayerAction::Call).unwrap();
    m.apply(B, PlayerAction::Check).unwrap();
    m.apply(B, raise_to(40)).unwrap();
    assert_eq!(m.legal_actions(H).unwrap().raise.unwrap().min_to, 80);
    m.apply(H, raise_to(100)).unwrap();
    // Increment was 60, so the re-raise must reach at least 160.
    assert_eq!(m.legal_actions(B).unwrap().raise.unwrap().min_to, 160);
    assert_eq!(
        m.apply(B, raise_to(150)),
        Err(EngineError::InvalidAmount { min: 160, max: 580 })
    );
    assert_chips(&m);
}

#[test]
fn rejected_actions_do_not_change_state() {
    let mut m = setup([600, 600], H, 1, DECK);
    let before = m.clone();
    assert_eq!(
        m.apply(B, PlayerAction::Check),
        Err(EngineError::NotYourTurn)
    );
    assert_eq!(
        m.apply(H, PlayerAction::Check),
        Err(EngineError::IllegalAction)
    );
    assert_eq!(
        m.apply(H, raise_to(30)),
        Err(EngineError::InvalidAmount { min: 40, max: 600 })
    );
    assert_eq!(
        m.apply(H, raise_to(601)),
        Err(EngineError::InvalidAmount { min: 40, max: 600 })
    );
    assert_eq!(m.next_hand(), Err(EngineError::WrongPhase));
    assert_eq!(m, before);
}

#[test]
fn fold_awards_pot_and_hides_cards() {
    let mut m = setup([600, 600], H, 1, DECK);
    m.apply(H, raise_to(60)).unwrap();
    m.apply(B, PlayerAction::Fold).unwrap();
    assert_eq!(m.phase, Phase::HandOver);
    let o = m.hand.outcome.clone().unwrap();
    assert!(o.by_fold && o.showdown.is_none());
    assert_eq!(o.winner, Some(H));
    // Uncalled 40 returned; human wins the 40 that was contested.
    assert_eq!(o.pot, 40);
    assert_eq!(m.stacks, [620, 580]);
    assert_eq!(o.net, [20, -20]);
    assert!(m.view(H).opponent_hole.is_none());
    assert!(m.history[0].outcome.showdown.is_none());
    assert_chips(&m);
}

#[test]
fn fold_not_offered_when_check_is_free() {
    let mut m = setup([600, 600], H, 1, DECK);
    m.apply(H, PlayerAction::Call).unwrap();
    assert_eq!(
        m.apply(B, PlayerAction::Fold),
        Err(EngineError::IllegalAction)
    );
}

#[test]
fn all_in_and_call_runs_out_board() {
    let mut m = setup([600, 600], H, 1, DECK);
    m.apply(H, raise_to(600)).unwrap();
    let legal = m.legal_actions(B).unwrap();
    assert!(legal.raise.is_none(), "cannot raise an all-in player");
    assert!(legal.call_is_all_in);
    m.apply(B, PlayerAction::Call).unwrap();
    let o = m.hand.outcome.clone().unwrap();
    assert_eq!(o.runout_from, Some(0));
    assert_eq!(m.hand.board().len(), 5);
    let sd = o.showdown.unwrap();
    // Human: As Kc on Ad 7h 2c 5s Kh = two pair, aces and kings.
    assert_eq!(sd[0].category, Category::TwoPair);
    assert_eq!(o.winner, Some(H));
    assert_eq!(m.stacks, [1200, 0]);
    assert!(m.is_final_hand());
    m.next_hand().unwrap();
    let r = m.result.clone().unwrap();
    assert_eq!(r.outcome, MatchOutcome::Win);
    assert!(r.ended_early);
    assert_eq!(r.best_hand.unwrap().category, Category::TwoPair);
}

#[test]
fn short_call_returns_uncalled_excess() {
    let mut m = setup([900, 300], H, 1, DECK);
    let r = m.legal_actions(H).unwrap().raise.unwrap();
    assert_eq!(r.max_to, 300, "effective max is the covered stack");
    m.apply(H, raise_to(300)).unwrap();
    m.apply(B, PlayerAction::Call).unwrap();
    assert_eq!(m.hand.outcome.as_ref().unwrap().pot, 600);
    assert_chips(&m);
}

#[test]
fn short_blind_all_in_resolves_without_action() {
    // Bot in the big blind with only 5 chips: all-in from the blind.
    let m = setup([1195, 5], H, 1, DECK);
    assert_eq!(m.phase, Phase::HandOver, "nobody can act");
    let o = m.hand.outcome.clone().unwrap();
    assert_eq!(o.pot, 10, "5 of the small blind returned");
    assert!(
        m.hand
            .actions
            .iter()
            .any(|a| a.kind == ActionKind::Return && a.amount == 5)
    );
    assert_chips(&m);

    // Big blind all-in for more than the small blind: button must still decide.
    let mut m = setup([1185, 15], H, 1, DECK);
    let legal = m.legal_actions(H).unwrap();
    assert_eq!(legal.call, Some(5));
    assert!(legal.raise.is_none());
    m.apply(H, PlayerAction::Call).unwrap();
    assert_eq!(m.phase, Phase::HandOver);
    assert_chips(&m);
}

#[test]
fn split_pot_when_board_plays() {
    let mut m = setup([600, 600], B, 1, "2s 3d 2c 3h As Ks Qs Js Ts");
    m.apply(B, PlayerAction::Call).unwrap();
    m.apply(H, PlayerAction::Check).unwrap();
    for _ in 0..3 {
        m.apply(H, PlayerAction::Check).unwrap();
        m.apply(B, PlayerAction::Check).unwrap();
    }
    let o = m.hand.outcome.clone().unwrap();
    assert_eq!(o.winner, None);
    assert_eq!(o.awarded, [20, 20]);
    assert_eq!(o.showdown.unwrap()[0].category, Category::RoyalFlush);
    assert_eq!(m.stacks, [600, 600]);
}

#[test]
fn full_match_alternates_button_and_blinds() {
    let mut m = Match::new(99);
    let mut buttons = [0; 2];
    let mut levels = Vec::new();
    while m.phase != Phase::MatchOver {
        if m.phase == Phase::InHand && m.hand.actions.len() == 2 {
            buttons[m.hand.button.idx()] += 1;
            levels.push(m.hand.bb);
        }
        match m.phase {
            Phase::InHand => {
                let seat = m.hand.to_act.unwrap();
                let legal = m.legal_actions(seat).unwrap();
                let action = if legal.check {
                    PlayerAction::Check
                } else {
                    PlayerAction::Call
                };
                m.apply(seat, action).unwrap();
            }
            Phase::HandOver => m.next_hand().unwrap(),
            Phase::MatchOver => unreachable!(),
        }
        assert_chips(&m);
    }
    let r = m.result.unwrap();
    assert_eq!(r.hands_played, 8);
    assert!(!r.ended_early);
    assert_eq!(buttons, [4, 4]);
    assert_eq!(levels, [20, 20, 30, 30, 40, 40, 60, 60]);
    assert_eq!(r.final_stacks[0] + r.final_stacks[1], TOTAL_CHIPS);
}

#[test]
fn tutorial_is_single_deterministic_hand() {
    let m = Match::new_tutorial();
    assert_eq!(m.kind, MatchKind::Tutorial);
    assert_eq!(
        m.hand.hole(H),
        [Card::parse("As").unwrap(), Card::parse("Kc").unwrap()]
    );
    assert_eq!(m.hand.to_act, Some(H));
    assert_eq!(m.hands_total(), 1);
    assert!(m.is_final_hand());
}

#[test]
fn snapshot_validation() {
    let m = Match::new(5);
    let json = serde_json::to_string(&m).unwrap();
    let back: Match = serde_json::from_str(&json).unwrap();
    assert_eq!(back, m);
    back.validate().unwrap();

    let mut tampered = m.clone();
    tampered.stacks[0] += 100;
    assert_eq!(
        tampered.validate(),
        Err(EngineError::CorruptSave("chip count"))
    );
    let mut old = m.clone();
    old.schema = 0;
    assert_eq!(
        old.validate(),
        Err(EngineError::IncompatibleSave { found: 0 })
    );
}

#[test]
fn view_never_leaks_opponent_cards_mid_hand() {
    let m = setup([600, 600], H, 1, DECK);
    let json = serde_json::to_string(&m.view(H)).unwrap();
    assert!(json.contains("\"As\"") && json.contains("\"Kc\""));
    assert!(
        !json.contains("\"Qd\"") && !json.contains("\"9c\""),
        "{json}"
    );
    assert!(!json.contains("\"Ad\""), "undealt flop must stay hidden");
}

/// Plays thousands of random matches with random legal actions and checks the
/// invariants on every transition, including resuming from a JSON snapshot.
#[test]
fn saves_before_opponents_load_as_alex() {
    let m = Match::against(3, Opponent::Station);
    let mut json: serde_json::Value = serde_json::to_value(&m).unwrap();
    assert_eq!(json["opponent"], "station");
    json.as_object_mut().unwrap().remove("opponent");
    let old: Match = serde_json::from_value(json).unwrap();
    assert_eq!(old.opponent, Opponent::Alex);
    old.validate().unwrap();
}

#[test]
fn random_play_fuzz() {
    for seed in 0..3000u64 {
        let mut m = Match::new(seed);
        let mut rng = Rng::new(seed ^ 0xABCD);
        let mut steps = 0;
        while m.phase != Phase::MatchOver {
            steps += 1;
            assert!(steps < 500, "match did not terminate");
            assert_chips(&m);
            m.validate().unwrap();
            if steps % 7 == 0 {
                let json = serde_json::to_string(&m).unwrap();
                let back: Match = serde_json::from_str(&json).unwrap();
                assert_eq!(back, m, "snapshot round trip");
            }
            match m.phase {
                Phase::InHand => {
                    let seat = m.hand.to_act.unwrap();
                    let legal = m.legal_actions(seat).unwrap();
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
                    let pick = options[rng.below(options.len() as u64) as usize];
                    let to_call = m.hand.max_bet() - m.hand.bets[seat.idx()];
                    m.apply(seat, pick).unwrap();
                    if let PlayerAction::Call = pick {
                        assert!(to_call > 0);
                    }
                }
                Phase::HandOver => {
                    let o = m.hand.outcome.as_ref().unwrap();
                    assert_eq!(o.net[0] + o.net[1], 0);
                    assert_eq!(o.by_fold, o.showdown.is_none());
                    m.next_hand().unwrap();
                }
                Phase::MatchOver => unreachable!(),
            }
        }
        let r = m.result.as_ref().unwrap();
        assert!(r.hands_played <= HANDS_PER_MATCH);
        assert_eq!(
            r.ended_early,
            r.final_stacks.contains(&0) && r.hands_played < 8
        );
        assert_eq!(m.history.len(), r.hands_played as usize);
        for pair in m.history.windows(2) {
            assert_ne!(pair[0].button, pair[1].button, "button alternates");
            assert_eq!(pair[0].number + 1, pair[1].number);
        }
    }
}
