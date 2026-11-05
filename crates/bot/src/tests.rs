use super::*;
use crate::reference::{Policy, calling_station, random_legal, tight_aggressive};
use poker_core::card::cards;
use poker_core::{Match, MatchOutcome, Opponent, Phase};

fn c2(s: &str) -> [Card; 2] {
    let v = cards(s);
    [v[0], v[1]]
}

#[test]
fn equity_sanity() {
    let mut rng = Rng::new(3);
    let aa = equity(c2("As Ah"), &[], 1.0, 4000, &mut rng);
    let trash = equity(c2("7d 2c"), &[], 1.0, 4000, &mut rng);
    assert!((0.82..0.88).contains(&aa), "AA vs random ≈ 0.85, got {aa}");
    assert!(
        (0.28..0.38).contains(&trash),
        "72o vs random ≈ 0.32, got {trash}"
    );
    let nuts = equity(c2("As Ks"), &cards("Qs Js Ts 2d 3c"), 1.0, 500, &mut rng);
    assert_eq!(nuts, 1.0);
    // Narrower (stronger) range lowers a medium hand's equity.
    let wide = equity(c2("9d 8d"), &[], 1.0, 4000, &mut rng);
    let tight = equity(c2("9d 8d"), &[], 0.2, 4000, &mut rng);
    assert!(tight < wide - 0.05, "{tight} vs {wide}");
}

#[test]
fn preflop_ranking() {
    assert!(top_share(c2("As Ah")) < 0.01);
    assert!(top_share(c2("3d 2c")) > 0.99, "the weakest heads-up hand");
    assert!(top_share(c2("7d 2c")) > 0.9);
    assert!(top_share(c2("Ks Qs")) < top_share(c2("Kd Qc")));
    assert!(
        top_share(c2("Ad 2c")) < 0.5,
        "any ace is above average heads-up"
    );
}

#[test]
fn draw_detection() {
    assert!(strong_draw(c2("Ah 5h"), &cards("Kh 9h 2c")), "flush draw");
    assert!(strong_draw(c2("9c 8d"), &cards("7s 6h Kd")), "open-ended");
    assert!(
        !strong_draw(c2("9c 8d"), &cards("7s 5h Kd")),
        "gutshot is not strong"
    );
    assert!(
        !strong_draw(c2("Ac Kd"), &cards("Qs Jh 2d")),
        "one-ended broadway"
    );
    assert!(
        !strong_draw(c2("2c 3d"), &cards("9s 8h 7d 6c")),
        "board-only draw"
    );
    assert!(
        !strong_draw(c2("Ah 5h"), &cards("Kh 9h 2h")),
        "made flush is not a draw"
    );
    assert!(!strong_draw(c2("Ah 5h"), &cards("Kh 9h 2c 3d 4s")), "river");
}

fn is_legal(v: &PlayerView, a: PlayerAction) -> bool {
    let l = v.legal.as_ref().unwrap();
    match a {
        PlayerAction::Fold => l.fold,
        PlayerAction::Check => l.check,
        PlayerAction::Call => l.call.is_some(),
        PlayerAction::RaiseTo { amount } => l
            .raise
            .as_ref()
            .is_some_and(|r| (r.min_to..=r.max_to).contains(&amount)),
    }
}

/// Plays bot (in `bot_seat`) against `opponent`, calling `observe` on every
/// bot decision. Returns the match.
fn play_match(
    seed: u64,
    bot_seat: Seat,
    opponent: Policy,
    observe: impl FnMut(&PlayerView, PlayerAction),
) -> Match {
    play_match_as(Opponent::Alex, seed, bot_seat, opponent, observe)
}

/// Like [`play_match`], with the bot playing as `who`.
fn play_match_as(
    who: Opponent,
    seed: u64,
    bot_seat: Seat,
    opponent: Policy,
    mut observe: impl FnMut(&PlayerView, PlayerAction),
) -> Match {
    let mut m = Match::against(seed, who);
    while m.phase != Phase::MatchOver {
        if m.phase != Phase::InHand {
            m.next_hand().unwrap();
            continue;
        }
        let seat = m.hand.to_act.unwrap();
        let v = m.view(seat);
        let a = if seat == bot_seat {
            let a = decide(&v, Personality::Standard(who), m.decision_seed());
            observe(&v, a);
            a
        } else {
            opponent(&v, m.decision_seed() ^ 0x0FF0)
        };
        m.apply(seat, a).expect("legal");
    }
    m
}

/// Tens of thousands of decisions in varied states: the strategy itself must
/// already be legal, so the sanitizer never has to step in.
#[test]
fn strategy_is_always_legal() {
    let mut decisions = 0;
    let mut seed = 0;
    let opponents: [Policy; 3] = [random_legal, tight_aggressive, calling_station];
    while decisions < 20_000 {
        let opponent = opponents[seed as usize % 3];
        let who = Opponent::ALL[seed as usize % 5];
        let seat = if seed % 2 == 0 {
            Seat::Bot
        } else {
            Seat::Human
        };
        play_match_as(who, seed, seat, opponent, |v, final_action| {
            let legal = v.legal.as_ref().unwrap();
            let seed = 0; // the raw policy is re-run with fixed randomness
            let raw = policy(v, legal, Personality::Standard(who), seed);
            assert!(is_legal(v, raw), "raw {raw:?} illegal for {legal:?}");
            assert!(is_legal(v, final_action));
            decisions += 1;
        });
        seed += 1;
    }
}

#[test]
fn sanitizer_repairs_anything() {
    let m = Match::new(5);
    let v = m.view(m.hand.to_act.unwrap());
    let legal = v.legal.as_ref().unwrap();
    for a in [
        PlayerAction::Fold,
        PlayerAction::Check,
        PlayerAction::Call,
        PlayerAction::RaiseTo { amount: 0 },
        PlayerAction::RaiseTo { amount: u32::MAX },
    ] {
        assert!(is_legal(&v, sanitize(legal, a)), "{a:?}");
    }
}

#[derive(Default)]
struct Mix {
    decisions: [u32; 4],
    all_in: [u32; 4],
    bets: u32,
    min_bets: u32,
    big_bets_faced_with_trash: u32,
    trash_calls: u32,
    folds_facing_bet: u32,
    facing_bet: u32,
}

fn street_idx(s: Street) -> usize {
    match s {
        Street::Preflop => 0,
        Street::Flop => 1,
        Street::Turn => 2,
        Street::River => 3,
    }
}

fn record(mix: &mut Mix, v: &PlayerView, a: PlayerAction) {
    let l = v.legal.as_ref().unwrap();
    let i = street_idx(v.street);
    mix.decisions[i] += 1;
    let pot = v.pot + v.bets[0] + v.bets[1];
    if v.to_call > 0 && v.street != Street::Preflop {
        mix.facing_bet += 1;
        if a == PlayerAction::Fold {
            mix.folds_facing_bet += 1;
        }
        // A bet of at least half the pot before it, while holding very little.
        if v.to_call * 3 >= pot {
            let mut rng = Rng::new(1);
            if equity(v.hole, &v.board, 1.0, 400, &mut rng) < 0.25 {
                mix.big_bets_faced_with_trash += 1;
                if a == PlayerAction::Call {
                    mix.trash_calls += 1;
                }
            }
        }
    }
    // Only shoves the bot starts; calling someone else's all-in is a call.
    if let PlayerAction::RaiseTo { amount } = a {
        let r = l.raise.as_ref().unwrap();
        if amount == r.max_to {
            mix.all_in[i] += 1;
        } else if v.street != Street::Preflop {
            mix.bets += 1;
            if amount == r.min_to {
                mix.min_bets += 1;
            }
        }
    }
}

#[test]
fn action_mix_is_not_pathological() {
    let mut mix = Mix::default();
    let opponents: [Policy; 3] = [random_legal, tight_aggressive, calling_station];
    for seed in 0..240u64 {
        let seat = if seed % 2 == 0 {
            Seat::Bot
        } else {
            Seat::Human
        };
        play_match(seed * 31 + 7, seat, opponents[seed as usize % 3], |v, a| {
            record(&mut mix, v, a)
        });
    }
    for (street, (all_in, n)) in ["preflop", "flop", "turn", "river"]
        .iter()
        .zip(mix.all_in.iter().zip(mix.decisions))
    {
        let share = *all_in as f64 / n.max(1) as f64;
        assert!(
            share < 0.15,
            "{street}: shoves in {:.0}% of decisions",
            share * 100.0
        );
    }
    let min_share = mix.min_bets as f64 / mix.bets.max(1) as f64;
    assert!(
        min_share < 0.15,
        "min-size in {:.0}% of postflop bets",
        min_share * 100.0
    );
    let fold_share = mix.folds_facing_bet as f64 / mix.facing_bet.max(1) as f64;
    assert!(
        (0.1..0.7).contains(&fold_share),
        "folds {:.0}% when facing a bet",
        fold_share * 100.0
    );
    let trash = mix.trash_calls as f64 / mix.big_bets_faced_with_trash.max(1) as f64;
    assert!(
        mix.big_bets_faced_with_trash >= 10 && trash < 0.1,
        "called {}/{} big bets with trash",
        mix.trash_calls,
        mix.big_bets_faced_with_trash
    );
}

/// Returns (bot wins, bot losses, average chips won) over `matches` matches.
fn results(opponent: Policy, matches: u64) -> (u32, u32, f64) {
    let (mut wins, mut losses) = (0, 0);
    let mut chips = 0i64;
    for seed in 0..matches {
        // Same deal from both seats, to cancel out card luck.
        for bot_seat in Seat::BOTH {
            let m = play_match(seed * 7919 + 1, bot_seat, opponent, |_, _| {});
            chips += m.stacks[bot_seat.idx()] as i64 - 600;
            let won = match m.result.unwrap().outcome {
                MatchOutcome::Win => Seat::Human,
                MatchOutcome::Loss => Seat::Bot,
                MatchOutcome::Draw => continue,
            };
            if won == bot_seat {
                wins += 1;
            } else {
                losses += 1;
            }
        }
    }
    (wins, losses, chips as f64 / (2 * matches) as f64)
}

#[test]
fn beats_reference_opponents() {
    // Credible, not unbeatable: clearly ahead of naive play, ahead of a
    // simple tight-aggressive player. Eight-hand matches are noisy, so the
    // margins are loose.
    for (name, opponent, min_chips) in [
        ("calling station", calling_station as Policy, 100.0),
        ("random", random_legal, 100.0),
        ("tight-aggressive", tight_aggressive, 0.0),
    ] {
        let (w, l, chips) = results(opponent, 100);
        assert!(
            w > l && chips > min_chips,
            "vs {name}: {w}/{l}, {chips:+.0} chips"
        );
    }
}

#[derive(Default, Debug)]
struct StyleStats {
    preflop: u32,
    preflop_folds: u32,
    postflop: u32,
    aggressive: u32,
    facing_bet: u32,
    folds_to_bet: u32,
}

impl StyleStats {
    fn preflop_fold(&self) -> f64 {
        self.preflop_folds as f64 / self.preflop.max(1) as f64
    }
    /// Share of postflop decisions that are bets or raises.
    fn aggression(&self) -> f64 {
        self.aggressive as f64 / self.postflop.max(1) as f64
    }
    fn fold_to_bet(&self) -> f64 {
        self.folds_to_bet as f64 / self.facing_bet.max(1) as f64
    }
}

fn style_stats(who: Opponent) -> StyleStats {
    let mut st = StyleStats::default();
    let opponents: [Policy; 3] = [random_legal, tight_aggressive, calling_station];
    for seed in 0..90u64 {
        let seat = if seed % 2 == 0 {
            Seat::Bot
        } else {
            Seat::Human
        };
        play_match_as(
            who,
            seed * 13 + 5,
            seat,
            opponents[seed as usize % 3],
            |v, a| {
                if v.street == Street::Preflop {
                    st.preflop += 1;
                    st.preflop_folds += (a == PlayerAction::Fold) as u32;
                    return;
                }
                st.postflop += 1;
                if v.to_call > 0 {
                    st.facing_bet += 1;
                    st.folds_to_bet += (a == PlayerAction::Fold) as u32;
                }
                if let PlayerAction::RaiseTo { .. } = a {
                    st.aggressive += 1;
                }
            },
        );
    }
    st
}

/// The five opponents must feel different: the cautious one folds most, the
/// aggressor bets most, the calling station almost never lets go.
#[test]
fn opponents_have_distinct_styles() {
    let [novice, station, tight, alex, aggressor] = Opponent::ALL.map(style_stats);
    assert!(
        tight.preflop_fold() > alex.preflop_fold() + 0.05
            && alex.preflop_fold() > station.preflop_fold() + 0.05,
        "preflop folds: tight {tight:?} alex {alex:?} station {station:?}"
    );
    assert!(
        aggressor.aggression() > alex.aggression() * 1.15
            && alex.aggression() > station.aggression() * 1.5,
        "aggression: aggressor {:.2} alex {:.2} station {:.2}",
        aggressor.aggression(),
        alex.aggression(),
        station.aggression()
    );
    assert!(
        station.fold_to_bet() < tight.fold_to_bet() - 0.1,
        "fold to bet: station {:.2} tight {:.2}",
        station.fold_to_bet(),
        tight.fold_to_bet()
    );
    assert!(
        novice.preflop_fold() < alex.preflop_fold(),
        "the novice plays loose"
    );
}

fn alex(v: &PlayerView, seed: u64) -> PlayerAction {
    decide(v, Personality::Standard(Opponent::Alex), seed)
}

/// The novice is the easy pick: over duplicate deals it loses to Alex.
#[test]
fn novice_is_easier_than_alex() {
    let mut chips = 0i64;
    for seed in 0..120u64 {
        for seat in Seat::BOTH {
            let m = play_match_as(Opponent::Novice, seed * 7919 + 3, seat, alex, |_, _| {});
            chips += m.stacks[seat.idx()] as i64 - 600;
        }
    }
    let avg = chips as f64 / 240.0;
    assert!(avg < 0.0, "novice vs alex: {avg:+.0} chips per match");
}

#[test]
fn tutorial_bot_is_passive() {
    let mut m = Match::new_tutorial();
    m.apply(Seat::Human, PlayerAction::RaiseTo { amount: 60 })
        .unwrap();
    let a = decide(&m.view(Seat::Bot), Personality::Tutorial, 0);
    assert_eq!(a, PlayerAction::Call);
}

#[test]
fn decisions_are_reproducible() {
    for seed in 0..50 {
        let m = Match::new(seed);
        let seat = m.hand.to_act.unwrap();
        let v = m.view(seat);
        assert_eq!(
            decide(&v, Personality::Standard(Opponent::Alex), m.decision_seed()),
            decide(&v, Personality::Standard(Opponent::Alex), m.decision_seed())
        );
    }
}

// ---------- concrete spots: sensible choices in hand-built situations ----------

/// Deck with the given hole cards and board: human, bot, human, bot, board…
fn spot_deck(human: &str, bot: &str, board: &str) -> Vec<Card> {
    let parse = |s: &str| -> Vec<Card> {
        s.split_whitespace()
            .map(|c| Card::parse(c).unwrap())
            .collect()
    };
    let (h, b, board) = (parse(human), parse(bot), parse(board));
    let mut d = vec![h[0], b[0], h[1], b[1]];
    d.extend(&board);
    for c in full_deck() {
        if !d.contains(&c) {
            d.push(c);
        }
    }
    d
}

/// Share of `n` decisions by `who` in the spot that satisfy `ok`.
fn share(
    who: Opponent,
    button: Seat,
    cards: (&str, &str, &str),
    script: &[(Seat, PlayerAction)],
    ok: impl Fn(PlayerAction) -> bool,
) -> f64 {
    let mut m = Match::duel("spot".into(), button, &spot_deck(cards.0, cards.1, cards.2)).unwrap();
    for &(seat, a) in script {
        m.apply(seat, a).unwrap();
    }
    let v = m.view(Seat::Bot);
    let n = 200u64;
    let hits = (0..n)
        .filter(|s| ok(decide(&v, Personality::Standard(who), s * 7919 + 1)))
        .count();
    hits as f64 / n as f64
}

const HU: Seat = Seat::Human;
const BOT: Seat = Seat::Bot;

#[test]
fn worst_hands_fold_preflop() {
    for who in Opponent::ALL {
        let sb = share(who, BOT, ("As Ah", "7c 2d", "Kd 9s 4c 3h 2s"), &[], |a| {
            a == PlayerAction::Fold
        });
        assert!(
            sb > 0.99,
            "{who:?} plays 7-2 from the small blind: fold {sb:.2}"
        );
    }
    for who in [
        Opponent::Alex,
        Opponent::Tight,
        Opponent::Aggressor,
        Opponent::Novice,
    ] {
        let bb = share(
            who,
            HU,
            ("As Kh", "7c 2d", "Kd 9s 4c 3h 2s"),
            &[(HU, PlayerAction::RaiseTo { amount: 60 })],
            |a| a == PlayerAction::Fold,
        );
        assert!(
            bb > 0.99,
            "{who:?} defends 7-2 against a raise: fold {bb:.2}"
        );
    }
}

#[test]
fn premium_hands_are_played_for_value() {
    for who in [Opponent::Alex, Opponent::Tight, Opponent::Aggressor] {
        let aa = share(who, BOT, ("7c 2d", "As Ah", "Kd 9s 4c 3h 2s"), &[], |a| {
            matches!(a, PlayerAction::RaiseTo { .. })
        });
        assert!(aa > 0.7, "{who:?} raises aces only {aa:.2}");
    }
    for who in Opponent::ALL {
        let aks = share(
            who,
            HU,
            ("9c 9d", "Ah Kh", "Qd 9s 4c 3h 2s"),
            &[(HU, PlayerAction::RaiseTo { amount: 60 })],
            |a| a != PlayerAction::Fold,
        );
        assert_eq!(aks, 1.0, "{who:?} folds A-K suited to a raise");
    }
}

#[test]
fn never_folds_the_nuts_and_folds_air_to_big_bets() {
    use PlayerAction::{Call, Check, RaiseTo};
    let river_nuts = [
        (HU, RaiseTo { amount: 60 }),
        (BOT, Call),
        (BOT, Check),
        (HU, RaiseTo { amount: 60 }),
        (BOT, Call),
        (BOT, Check),
        (HU, RaiseTo { amount: 120 }),
        (BOT, Call),
        (BOT, Check),
        (HU, RaiseTo { amount: 200 }),
    ];
    let air = [
        (HU, RaiseTo { amount: 60 }),
        (BOT, Call),
        (BOT, Check),
        (HU, RaiseTo { amount: 120 }),
    ];
    for who in Opponent::ALL {
        let nuts = share(
            who,
            HU,
            ("Kc Kd", "Ah 5h", "Kh 8h 2h 7c 3s"),
            &river_nuts,
            |a| a != PlayerAction::Fold,
        );
        assert_eq!(nuts, 1.0, "{who:?} folds the nut flush");
        let fold_air = share(who, HU, ("Ah Ad", "8c 3d", "Ks Qh 2c 5d 9h"), &air, |a| {
            a == PlayerAction::Fold
        });
        assert!(
            fold_air > 0.85,
            "{who:?} continues with air: fold {fold_air:.2}"
        );
    }
}
