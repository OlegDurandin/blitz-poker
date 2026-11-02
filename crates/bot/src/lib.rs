//! Bot Alex.
//!
//! The bot only ever sees a [`PlayerView`] for its own seat: its hole cards,
//! the board, stacks and the betting history. It never touches the match
//! state, so it cannot peek at the human's cards or the undealt deck.
//!
//! Strategy: Monte Carlo equity against an opponent range that narrows with
//! the opponent's aggression, compared to pot odds, plus a little randomised
//! value betting and bluffing so it is not trivially exploitable.

use poker_core::card::full_deck;
use poker_core::rng::Rng;
use poker_core::{ActionKind, Card, LegalActions, PlayerAction, PlayerView, Street, hand_value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Personality {
    Standard,
    /// Passive check/call for the training hand.
    Tutorial,
}

const ITERATIONS: usize = 600;

pub fn decide(view: &PlayerView, personality: Personality, seed: u64) -> PlayerAction {
    let Some(legal) = view.legal.as_ref() else {
        return PlayerAction::Check;
    };
    let passive = if legal.check {
        PlayerAction::Check
    } else {
        PlayerAction::Call
    };
    if personality == Personality::Tutorial {
        return passive;
    }
    let mut rng = Rng::new(seed);
    let s = Situation::new(view);
    let range = opponent_range(view);
    let equity = equity(view.hole, &view.board, range, ITERATIONS, &mut rng);

    if view.street == Street::Preflop && s.effective_bb <= 12.0 {
        return push_or_fold(&s, legal, equity, &mut rng);
    }
    if view.street == Street::Preflop && s.is_unopened_small_blind() {
        return open_small_blind(&s, legal, equity, &mut rng);
    }

    let raises = s.aggressive_actions_this_street;
    if legal.check {
        if equity >= 0.68 && rng.chance(0.85) {
            return sized(legal, &s, rng_frac(&mut rng, 0.55, 0.8), passive);
        }
        if equity >= 0.52 && rng.chance(0.45) {
            return sized(legal, &s, 0.5, passive);
        }
        let bluff = match view.street {
            Street::Preflop => 0.08,
            Street::Flop | Street::Turn => 0.2,
            Street::River => 0.12,
        };
        if rng.chance(bluff) {
            return sized(legal, &s, 0.6, passive);
        }
        return PlayerAction::Check;
    }

    let margin = 0.02;
    if equity >= 0.75 && raises < 3 && rng.chance(0.7) {
        return sized(legal, &s, rng_frac(&mut rng, 0.6, 0.9), PlayerAction::Call);
    }
    if equity >= s.pot_odds + margin {
        let semi_bluff = matches!(view.street, Street::Flop | Street::Turn) && equity > 0.35;
        if semi_bluff && raises < 2 && rng.chance(0.06) {
            return sized(legal, &s, 0.75, PlayerAction::Call);
        }
        return PlayerAction::Call;
    }
    if raises < 2 && rng.chance(0.03) {
        return sized(legal, &s, 0.8, PlayerAction::Fold);
    }
    PlayerAction::Fold
}

struct Situation {
    pot_total: u32,
    max_bet: u32,
    to_call: u32,
    pot_odds: f64,
    effective_bb: f64,
    aggressive_actions_this_street: usize,
    street: Street,
}

impl Situation {
    fn new(v: &PlayerView) -> Situation {
        let (me, opp) = (v.me.idx(), v.me.other().idx());
        let pot_total = v.pot + v.bets[0] + v.bets[1];
        let to_call = v.to_call.min(v.stacks[me]);
        let effective = (v.stacks[me] + v.bets[me]).min(v.stacks[opp] + v.bets[opp]);
        Situation {
            pot_total,
            max_bet: v.bets[0].max(v.bets[1]),
            to_call,
            pot_odds: if to_call == 0 {
                0.0
            } else {
                to_call as f64 / (pot_total + to_call) as f64
            },
            effective_bb: effective as f64 / v.bb.max(1) as f64,
            aggressive_actions_this_street: v
                .actions
                .iter()
                .filter(|a| {
                    a.street == v.street && matches!(a.kind, ActionKind::Bet | ActionKind::Raise)
                })
                .count(),
            street: v.street,
        }
    }

    fn is_unopened_small_blind(&self) -> bool {
        self.street == Street::Preflop
            && self.aggressive_actions_this_street == 0
            && self.to_call > 0
    }
}

/// First decision on the button preflop: raise, limp or fold.
fn open_small_blind(
    s: &Situation,
    legal: &LegalActions,
    equity: f64,
    rng: &mut Rng,
) -> PlayerAction {
    if equity >= 0.50 && rng.chance(0.85) {
        let to = s.max_bet * 5 / 2;
        return raise_to(legal, to).unwrap_or(PlayerAction::Call);
    }
    if equity >= 0.38 || rng.chance(0.15) {
        return PlayerAction::Call;
    }
    PlayerAction::Fold
}

/// Short-stacked preflop play.
fn push_or_fold(s: &Situation, legal: &LegalActions, equity: f64, rng: &mut Rng) -> PlayerAction {
    let shove_at = if s.effective_bb <= 6.0 {
        0.42
    } else if s.effective_bb <= 9.0 {
        0.47
    } else {
        0.52
    };
    let all_in = legal
        .raise
        .as_ref()
        .map(|r| PlayerAction::RaiseTo { amount: r.max_to });
    if legal.check {
        return match all_in {
            Some(a) if equity >= shove_at + 0.06 || rng.chance(0.05) => a,
            _ => PlayerAction::Check,
        };
    }
    if s.aggressive_actions_this_street == 0 {
        // Unopened small blind.
        return match all_in {
            Some(a) if equity >= shove_at => a,
            _ if equity >= shove_at => PlayerAction::Call,
            _ => PlayerAction::Fold,
        };
    }
    if equity >= 0.6 {
        return all_in.unwrap_or(PlayerAction::Call);
    }
    if equity >= s.pot_odds + 0.03 {
        PlayerAction::Call
    } else {
        PlayerAction::Fold
    }
}

fn rng_frac(rng: &mut Rng, lo: f64, hi: f64) -> f64 {
    lo + (hi - lo) * rng.unit()
}

/// Bet or raise roughly `frac` of the pot (after calling); `fallback` if no
/// raise is legal.
fn sized(legal: &LegalActions, s: &Situation, frac: f64, fallback: PlayerAction) -> PlayerAction {
    let pot_after_call = (s.pot_total + s.to_call) as f64;
    let to = s.max_bet + (pot_after_call * frac).round() as u32;
    raise_to(legal, to).unwrap_or(fallback)
}

fn raise_to(legal: &LegalActions, to: u32) -> Option<PlayerAction> {
    let r = legal.raise.as_ref()?;
    // Round to a multiple of 5 so sizes look like a person chose them.
    let mut amount = (to.saturating_add(2) / 5 * 5).clamp(r.min_to, r.max_to);
    // Don't leave a pointless sliver behind: commit when most of the stack goes in.
    if amount * 10 >= r.max_to * 7 {
        amount = r.max_to;
    }
    Some(PlayerAction::RaiseTo { amount })
}

/// Fraction of starting hands the opponent is assumed to hold, given how
/// aggressively they have played this hand.
fn opponent_range(v: &PlayerView) -> f64 {
    let opp = v.me.other();
    let aggression = v
        .actions
        .iter()
        .filter(|a| a.seat == opp && matches!(a.kind, ActionKind::Bet | ActionKind::Raise))
        .count();
    let opp_all_in = v
        .actions
        .iter()
        .any(|a| a.seat == opp && a.all_in && a.kind != ActionKind::BigBlind);
    let base: f64 = match aggression {
        0 => 1.0,
        1 => 0.6,
        2 => 0.4,
        _ => 0.25,
    };
    // Against a habitually aggressive opponent each bet means less.
    let looseness = (aggression_frequency(v) / 0.3).clamp(1.0, 3.0);
    let mut range = base.powf(1.0 / looseness);
    let called_preflop_raise = v.actions.iter().any(|a| {
        a.seat == opp && a.street == Street::Preflop && a.kind == ActionKind::Call && a.to > v.bb
    });
    if called_preflop_raise {
        range = range.min(0.75);
    }
    if opp_all_in {
        range = range.min(0.4);
    }
    range
}

/// Share of the opponent's voluntary actions this match that were bets or
/// raises, shrunk towards a typical 0.3 while the sample is small.
fn aggression_frequency(v: &PlayerView) -> f64 {
    let opp = v.me.other();
    let past = v.history.iter().flat_map(|h| h.actions.iter());
    let (mut aggressive, mut voluntary) = (0.0, 0.0);
    for a in past.chain(v.actions.iter()).filter(|a| a.seat == opp) {
        match a.kind {
            ActionKind::Bet | ActionKind::Raise => {
                aggressive += 1.0;
                voluntary += 1.0;
            }
            ActionKind::Check | ActionKind::Call | ActionKind::Fold => voluntary += 1.0,
            _ => {}
        }
    }
    const PRIOR_WEIGHT: f64 = 8.0;
    (aggressive + 0.3 * PRIOR_WEIGHT) / (voluntary + PRIOR_WEIGHT)
}

/// Rough preflop strength in `[0, 1]`, used only to rank opponent combos.
fn preflop_score(a: Card, b: Card) -> f64 {
    let (hi, lo) = if a.rank() >= b.rank() {
        (a.rank(), b.rank())
    } else {
        (b.rank(), a.rank())
    };
    if hi == lo {
        return 0.5 + hi as f64 / 24.0;
    }
    let gap = (hi - lo - 1) as f64;
    let mut score = (hi as f64 + lo as f64) / 24.0 * 0.6;
    if a.suit() == b.suit() {
        score += 0.06;
    }
    score += (0.06 - gap * 0.02).max(0.0);
    if hi == 12 {
        score += 0.08;
    }
    score
}

/// Equity of `hole` against an opponent holding a random hand from the top
/// `range` fraction of possible holdings, by Monte Carlo.
fn equity(hole: [Card; 2], board: &[Card], range: f64, iterations: usize, rng: &mut Rng) -> f64 {
    let known: Vec<Card> = hole.iter().chain(board).copied().collect();
    let deck: Vec<Card> = full_deck()
        .into_iter()
        .filter(|c| !known.contains(c))
        .collect();

    let mut combos: Vec<(f64, Card, Card)> = Vec::with_capacity(deck.len() * deck.len() / 2);
    for i in 0..deck.len() {
        for j in i + 1..deck.len() {
            combos.push((preflop_score(deck[i], deck[j]), deck[i], deck[j]));
        }
    }
    combos.sort_by(|x, y| y.0.total_cmp(&x.0));
    let keep = ((combos.len() as f64 * range).ceil() as usize).clamp(1, combos.len());
    combos.truncate(keep);

    let mut total = 0.0;
    let mut runs = 0usize;
    let mut cards = Vec::with_capacity(7);
    let mut rest = Vec::with_capacity(deck.len());
    for _ in 0..iterations {
        let (_, o1, o2) = combos[rng.below(combos.len() as u64) as usize];
        rest.clear();
        rest.extend(deck.iter().copied().filter(|&c| c != o1 && c != o2));
        let missing = 5 - board.len();
        // Partial Fisher–Yates for the cards still to come.
        for k in 0..missing {
            let j = k + rng.below((rest.len() - k) as u64) as usize;
            rest.swap(k, j);
        }
        let full_board: Vec<Card> = board
            .iter()
            .copied()
            .chain(rest[..missing].iter().copied())
            .collect();

        cards.clear();
        cards.extend_from_slice(&hole);
        cards.extend_from_slice(&full_board);
        let mine = hand_value(&cards);
        cards.clear();
        cards.extend_from_slice(&[o1, o2]);
        cards.extend_from_slice(&full_board);
        let theirs = hand_value(&cards);
        total += match mine.cmp(&theirs) {
            std::cmp::Ordering::Greater => 1.0,
            std::cmp::Ordering::Equal => 0.5,
            std::cmp::Ordering::Less => 0.0,
        };
        runs += 1;
    }
    total / runs as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use poker_core::card::cards;
    use poker_core::{Match, MatchOutcome, Phase, Seat};

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

    type Policy = fn(&PlayerView, &mut Rng) -> PlayerAction;

    fn calling_station(v: &PlayerView, _: &mut Rng) -> PlayerAction {
        if v.legal.as_ref().unwrap().check {
            PlayerAction::Check
        } else {
            PlayerAction::Call
        }
    }

    fn random_player(v: &PlayerView, rng: &mut Rng) -> PlayerAction {
        let l = v.legal.as_ref().unwrap();
        let roll = rng.unit();
        if let Some(r) = &l.raise
            && roll < 0.25
        {
            let span = (r.max_to - r.min_to) as u64 + 1;
            return PlayerAction::RaiseTo {
                amount: r.min_to + rng.below(span) as u32,
            };
        }
        if l.fold && roll > 0.8 {
            return PlayerAction::Fold;
        }
        if l.check {
            PlayerAction::Check
        } else {
            PlayerAction::Call
        }
    }

    /// Returns (bot wins, bot losses, average chips won) over `matches` matches.
    fn play(opponent: Policy, matches: u64) -> (u32, u32, f64) {
        let (mut wins, mut losses) = (0, 0);
        let mut chips = 0i64;
        for seed in 0..matches {
            let mut m = Match::new(seed * 7919 + 1);
            let mut rng = Rng::new(seed);
            while m.phase != Phase::MatchOver {
                match m.phase {
                    Phase::InHand => {
                        let seat = m.hand.to_act.unwrap();
                        let view = m.view(seat);
                        let action = match seat {
                            Seat::Bot => decide(&view, Personality::Standard, m.decision_seed()),
                            Seat::Human => opponent(&view, &mut rng),
                        };
                        m.apply(seat, action)
                            .expect("bot and test policies only play legal actions");
                    }
                    _ => m.next_hand().unwrap(),
                }
            }
            chips += m.stacks[Seat::Bot.idx()] as i64 - 600;
            match m.result.unwrap().outcome {
                MatchOutcome::Loss => wins += 1,
                MatchOutcome::Win => losses += 1,
                MatchOutcome::Draw => {}
            }
        }
        (wins, losses, chips as f64 / matches as f64)
    }

    #[test]
    fn beats_simple_opponents() {
        // Eight-hand matches are noisy; these margins hold with a wide berth.
        let (w, l, chips) = play(calling_station, 200);
        assert!(
            w * 2 > l * 3 && chips > 100.0,
            "vs calling station: {w}/{l}, {chips:+.0} chips"
        );
        let (w, l, chips) = play(random_player, 200);
        assert!(
            w * 2 > l * 3 && chips > 100.0,
            "vs random player: {w}/{l}, {chips:+.0} chips"
        );
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
        let m = Match::new(42);
        let seat = m.hand.to_act.unwrap();
        let v = m.view(seat);
        assert_eq!(
            decide(&v, Personality::Standard, m.decision_seed()),
            decide(&v, Personality::Standard, m.decision_seed())
        );
    }
}
