//! Bot Alex.
//!
//! The bot only ever sees a [`PlayerView`] for its own seat: its hole cards,
//! the board, stacks and the public betting history. It never touches the
//! match state, so it cannot peek at the human's cards, the undealt deck or
//! the shuffle RNG.
//!
//! Strategy (approximate, not a solved chart):
//! - Preflop: hands are ranked by heads-up equity vs a random hand
//!   (`preflop_table.rs`). Opening, defending and 3-betting ranges depend on
//!   position and effective stack; at 10 big blinds or less it plays
//!   push/fold.
//! - Postflop: Monte Carlo equity against an opponent range narrowed by their
//!   aggression, plus draw detection. Value bets, semi-bluffs, continuation
//!   bets, occasional traps and bluffs, each with a bounded frequency.
//! - Price: calls are compared to pot odds, with a little implied odds for
//!   draws before the river.
//! - Legality: [`decide`] passes every choice through a sanitizer that only
//!   returns actions the engine offered.

mod preflop_table;
#[doc(hidden)]
pub mod reference;

use poker_core::card::full_deck;
use poker_core::rng::Rng;
use poker_core::{
    ActionKind, ActionRecord, Card, LegalActions, Opponent, PlayerAction, PlayerView, Seat, Street,
    hand_value,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Personality {
    /// One of the five opponents the player can pick.
    Standard(Opponent),
    /// Passive check/call for the training hand.
    Tutorial,
}

/// Knobs that turn the one strategy below into five distinct opponents.
/// Probabilities are multiplied and then capped, so `1.0` means Alex's play.
#[derive(Clone, Copy, Debug)]
struct Style {
    /// Small-blind opening share above 16 big blinds, and at 10–16.
    open_deep: f64,
    open_mid: f64,
    /// Chance to limp instead of raising a hand it opens.
    limp: f64,
    /// Top share of hands that 3-bet for value.
    three_bet: f64,
    /// Added to equity when deciding whether to call (looseness).
    call_bonus: f64,
    /// Multipliers for value bets, bluffs/semi-bluffs and raises.
    value: f64,
    bluff: f64,
    raise: f64,
    /// Multiplier for bet sizes.
    size: f64,
    /// Chance to slow-play a monster on the flop.
    trap: f64,
    /// Random misjudgement of equity, ± this much.
    noise: f64,
    /// Multiplier for the short-stack shoving range.
    push: f64,
}

fn style(opponent: Opponent) -> Style {
    let alex = Style {
        open_deep: 0.66,
        open_mid: 0.58,
        limp: 0.0,
        three_bet: 0.09,
        call_bonus: 0.0,
        value: 1.0,
        bluff: 1.0,
        raise: 1.0,
        size: 1.0,
        trap: 0.15,
        noise: 0.0,
        push: 1.0,
    };
    match opponent {
        Opponent::Alex => alex,
        Opponent::Novice => Style {
            open_deep: 0.85,
            open_mid: 0.8,
            limp: 0.6,
            three_bet: 0.02,
            call_bonus: 0.06,
            value: 0.7,
            bluff: 0.6,
            raise: 0.5,
            size: 0.55,
            trap: 0.2,
            noise: 0.15,
            push: 1.0,
        },
        Opponent::Tight => Style {
            open_deep: 0.42,
            open_mid: 0.38,
            three_bet: 0.06,
            call_bonus: -0.06,
            value: 0.85,
            bluff: 0.3,
            raise: 0.8,
            trap: 0.1,
            push: 0.75,
            ..alex
        },
        Opponent::Station => Style {
            open_deep: 0.9,
            open_mid: 0.85,
            limp: 0.8,
            three_bet: 0.03,
            call_bonus: 0.15,
            value: 0.6,
            bluff: 0.2,
            raise: 0.3,
            size: 0.8,
            trap: 0.3,
            noise: 0.03,
            push: 0.9,
        },
        Opponent::Aggressor => Style {
            open_deep: 0.88,
            open_mid: 0.8,
            three_bet: 0.22,
            call_bonus: 0.02,
            value: 1.25,
            bluff: 2.3,
            raise: 2.0,
            size: 1.3,
            trap: 0.05,
            push: 1.2,
            ..alex
        },
    }
}

/// A probability scaled by a style multiplier, kept below certainty.
fn p(base: f64, mult: f64) -> f64 {
    (base * mult).clamp(0.0, 0.95)
}

const ITERATIONS: usize = 600;

/// Picks a legal action for `view.me`. Deterministic for a given view and seed.
pub fn decide(view: &PlayerView, personality: Personality, seed: u64) -> PlayerAction {
    let Some(legal) = view.legal.as_ref() else {
        return PlayerAction::Check;
    };
    sanitize(legal, policy(view, legal, personality, seed))
}

fn policy(
    view: &PlayerView,
    legal: &LegalActions,
    personality: Personality,
    seed: u64,
) -> PlayerAction {
    let opponent = match personality {
        Personality::Tutorial => return passive(legal),
        Personality::Standard(o) => o,
    };
    let st = style(opponent);
    let mut rng = Rng::new(seed);
    let s = Situation::new(view);
    if view.street == Street::Preflop {
        preflop(view, &s, &st, legal, &mut rng)
    } else {
        postflop(view, &s, &st, legal, &mut rng)
    }
}

/// The last line of defence: whatever the strategy chose, return something
/// the engine accepts.
fn sanitize(legal: &LegalActions, action: PlayerAction) -> PlayerAction {
    match action {
        PlayerAction::Fold if legal.fold => action,
        PlayerAction::Fold => PlayerAction::Check,
        PlayerAction::Check if legal.check => action,
        PlayerAction::Check => PlayerAction::Fold,
        PlayerAction::Call if legal.call.is_some() => action,
        PlayerAction::Call => passive(legal),
        PlayerAction::RaiseTo { amount } => match &legal.raise {
            Some(r) => PlayerAction::RaiseTo {
                amount: amount.clamp(r.min_to, r.max_to),
            },
            None => passive(legal),
        },
    }
}

fn passive(legal: &LegalActions) -> PlayerAction {
    if legal.check {
        PlayerAction::Check
    } else {
        PlayerAction::Call
    }
}

struct Situation {
    me: Seat,
    pot_total: u32,
    max_bet: u32,
    to_call: u32,
    pot_odds: f64,
    effective_bb: f64,
    bb: u32,
    /// Bets and raises on the current street (blinds excluded).
    raises: usize,
    /// The opponent already checked on this street.
    opponent_checked: bool,
    /// We made the last bet or raise of the previous street.
    was_aggressor: bool,
}

impl Situation {
    fn new(v: &PlayerView) -> Situation {
        let (me, opp) = (v.me.idx(), v.me.other().idx());
        let pot_total = v.pot + v.bets[0] + v.bets[1];
        let to_call = v.to_call.min(v.stacks[me]);
        let effective = (v.stacks[me] + v.bets[me]).min(v.stacks[opp] + v.bets[opp]);
        let aggressive = |a: &&ActionRecord| matches!(a.kind, ActionKind::Bet | ActionKind::Raise);
        let last_aggressor_before = v
            .actions
            .iter()
            .rev()
            .filter(|a| a.street < v.street)
            .find(aggressive)
            .map(|a| a.seat);
        Situation {
            me: v.me,
            pot_total,
            max_bet: v.bets[0].max(v.bets[1]),
            to_call,
            pot_odds: if to_call == 0 {
                0.0
            } else {
                to_call as f64 / (pot_total + to_call) as f64
            },
            effective_bb: effective as f64 / v.bb.max(1) as f64,
            bb: v.bb,
            raises: v
                .actions
                .iter()
                .filter(|a| a.street == v.street)
                .filter(aggressive)
                .count(),
            opponent_checked: v.actions.iter().any(|a| {
                a.street == v.street && a.seat == v.me.other() && a.kind == ActionKind::Check
            }),
            was_aggressor: last_aggressor_before == Some(v.me),
        }
    }
}

fn all_in(legal: &LegalActions) -> Option<PlayerAction> {
    legal
        .raise
        .as_ref()
        .map(|r| PlayerAction::RaiseTo { amount: r.max_to })
}

/// Share of the small blind's hands that may be shoved at `eff` big blinds
/// (rounded from heads-up push/fold charts).
fn push_share(eff: f64) -> f64 {
    if eff <= 5.0 {
        0.85
    } else if eff <= 8.0 {
        0.68
    } else {
        0.56
    }
}

/// Monte Carlo equity, blurred by the style's misjudgement.
fn felt_equity(v: &PlayerView, st: &Style, rng: &mut Rng) -> f64 {
    let e = equity(v.hole, &v.board, opponent_range(v), ITERATIONS, rng);
    if st.noise > 0.0 {
        (e + (rng.unit() * 2.0 - 1.0) * st.noise).clamp(0.0, 1.0)
    } else {
        e
    }
}

fn preflop(
    v: &PlayerView,
    s: &Situation,
    st: &Style,
    legal: &LegalActions,
    rng: &mut Rng,
) -> PlayerAction {
    let top = top_share(v.hole);
    let eff = s.effective_bb;
    let shove = all_in(legal);

    // Small blind (button), first decision.
    if s.raises == 0 && s.to_call > 0 {
        if eff <= 10.0 {
            if top <= 0.12 && eff > 7.0 && rng.chance(0.25) {
                return raise_to(legal, s.bb * 2).unwrap_or(PlayerAction::Call);
            }
            let push = (push_share(eff) * st.push).min(0.95);
            if top <= push && st.limp > 0.0 && rng.chance(st.limp * 0.5) {
                return PlayerAction::Call;
            }
            return match shove {
                Some(a) if top <= push => a,
                _ if top <= push => PlayerAction::Call,
                _ => PlayerAction::Fold,
            };
        }
        let open = if eff > 16.0 {
            st.open_deep
        } else {
            st.open_mid
        };
        if top <= 0.05 && rng.chance(st.trap * 0.8) {
            return PlayerAction::Call; // occasional trap limp
        }
        if top <= open {
            if st.limp > 0.0 && rng.chance(st.limp) {
                return PlayerAction::Call;
            }
            let bb = s.bb as f64;
            let size = if eff > 16.0 { 2.5 * bb } else { 2.25 * bb } * st.size.max(0.8);
            return raise_to(legal, size as u32).unwrap_or(PlayerAction::Call);
        }
        if top <= open + 0.15 && rng.chance(0.5) {
            return PlayerAction::Call;
        }
        return PlayerAction::Fold;
    }

    // Big blind after a limp.
    if s.raises == 0 {
        if eff <= 10.0 {
            return match shove {
                Some(a) if top <= 0.25 => a,
                _ => PlayerAction::Check,
            };
        }
        if top <= (0.22 * st.raise).min(0.6) || rng.chance(p(0.06, st.bluff)) {
            return raise_to(legal, s.bb * 4).unwrap_or(PlayerAction::Check);
        }
        return PlayerAction::Check;
    }

    // Facing a raise.
    let equity = felt_equity(v, st, rng);
    let stack = v.stacks[s.me.idx()];
    let committing = legal.call_is_all_in || s.to_call as f64 >= 0.35 * stack as f64;
    if committing {
        // Looser styles call off their stack lighter, but only by half as much.
        if equity + st.call_bonus / 2.0 >= s.pot_odds + 0.02 {
            return match shove {
                Some(a) if top <= 0.04 => a,
                _ => PlayerAction::Call,
            };
        }
        return PlayerAction::Fold;
    }
    if s.raises == 1 {
        let bluff_3bet = (0.3..0.5).contains(&top) && rng.chance(p(0.06, st.bluff));
        if top <= st.three_bet || bluff_3bet {
            let three_bet = if eff <= 20.0 {
                shove
            } else {
                raise_to(legal, s.max_bet * 3)
            };
            if let Some(a) = three_bet {
                return a;
            }
        }
        // Closing the action in the big blind is cheap: defend wide.
        return if equity + 0.04 + st.call_bonus >= s.pot_odds {
            PlayerAction::Call
        } else {
            PlayerAction::Fold
        };
    }
    if top <= (st.three_bet * 0.45).max(0.02) {
        return shove.unwrap_or(PlayerAction::Call);
    }
    if top <= 0.2 + st.call_bonus * 2.0 && equity + st.call_bonus >= s.pot_odds + 0.02 {
        return PlayerAction::Call;
    }
    PlayerAction::Fold
}

fn postflop(
    v: &PlayerView,
    s: &Situation,
    st: &Style,
    legal: &LegalActions,
    rng: &mut Rng,
) -> PlayerAction {
    let equity = felt_equity(v, st, rng);
    let draw = v.street != Street::River && strong_draw(v.hole, &v.board);
    let street = v.street;

    if legal.check {
        if equity >= 0.85 {
            if street == Street::Flop && equity >= 0.9 && rng.chance(st.trap) {
                return PlayerAction::Check; // trap
            }
            let frac = if street == Street::River {
                rng_frac(rng, 0.6, 0.9)
            } else {
                rng_frac(rng, 0.6, 0.85)
            };
            return sized(legal, s, frac * st.size, PlayerAction::Check);
        }
        if equity >= 0.66 {
            if rng.chance(p(0.9, st.value)) {
                let frac = rng_frac(rng, 0.5, 0.75) * st.size;
                return sized(legal, s, frac, PlayerAction::Check);
            }
            return PlayerAction::Check;
        }
        if draw {
            let base = if street == Street::Flop { 0.5 } else { 0.35 };
            if rng.chance(p(base, st.bluff)) {
                let frac = rng_frac(rng, 0.5, 0.7) * st.size;
                return sized(legal, s, frac, PlayerAction::Check);
            }
            return PlayerAction::Check;
        }
        if equity >= 0.5 {
            let base = if s.was_aggressor && street == Street::Flop {
                0.65
            } else {
                0.45
            };
            if rng.chance(p(base, st.value)) {
                let frac = rng_frac(rng, 0.33, 0.5) * st.size;
                return sized(legal, s, frac, PlayerAction::Check);
            }
            return PlayerAction::Check;
        }
        let bluff = match (street, s.was_aggressor) {
            (Street::Flop, true) => 0.3,
            (Street::Flop, false) => 0.08,
            (Street::Turn, true) => 0.15,
            (Street::Turn, false) => 0.06,
            _ if s.opponent_checked => 0.12,
            _ => 0.05,
        };
        if rng.chance(p(bluff, st.bluff)) {
            let frac = if street == Street::River {
                rng_frac(rng, 0.66, 1.0)
            } else {
                rng_frac(rng, 0.5, 0.75)
            };
            return sized(legal, s, frac * st.size, PlayerAction::Check);
        }
        return PlayerAction::Check;
    }

    // Facing a bet.
    if equity >= 0.8 && s.raises < 3 && rng.chance(p(0.65, st.raise)) {
        let frac = rng_frac(rng, 0.7, 0.9) * st.size;
        return sized(legal, s, frac, PlayerAction::Call);
    }
    if equity >= 0.7 && s.raises < 2 && rng.chance(p(0.35, st.raise)) {
        let frac = rng_frac(rng, 0.6, 0.8) * st.size;
        return sized(legal, s, frac, PlayerAction::Call);
    }
    if draw && street == Street::Flop && s.raises < 2 && rng.chance(p(0.12, st.bluff)) {
        return sized(legal, s, 0.7 * st.size, PlayerAction::Call);
    }
    let implied = if draw { 0.06 } else { 0.0 };
    let margin = if legal.call_is_all_in { 0.03 } else { 0.01 };
    let bonus = if legal.call_is_all_in {
        st.call_bonus / 2.0
    } else {
        st.call_bonus
    };
    if equity + implied + bonus >= s.pot_odds + margin {
        return PlayerAction::Call;
    }
    if s.raises == 1 && street != Street::River && rng.chance(p(0.025, st.bluff)) {
        return sized(legal, s, 0.75 * st.size, PlayerAction::Fold);
    }
    PlayerAction::Fold
}

fn rng_frac(rng: &mut Rng, lo: f64, hi: f64) -> f64 {
    lo + (hi - lo) * rng.unit()
}

/// Bet or raise roughly `frac` of the pot (after calling); `fallback` if no
/// raise is legal.
fn sized(legal: &LegalActions, s: &Situation, frac: f64, fallback: PlayerAction) -> PlayerAction {
    let pot_after_call = (s.pot_total + s.to_call) as f64;
    // In small pots a third of the pot is below the minimum bet; go bigger
    // rather than always betting exactly the minimum.
    let frac = if pot_after_call <= 3.0 * s.bb as f64 {
        frac.max(0.6)
    } else {
        frac
    };
    let to = s.max_bet + (pot_after_call * frac).round() as u32;
    raise_to(legal, to).unwrap_or(fallback)
}

fn raise_to(legal: &LegalActions, to: u32) -> Option<PlayerAction> {
    let r = legal.raise.as_ref()?;
    // Round to a multiple of 5 so sizes look like a person chose them.
    let mut amount = (to.saturating_add(2) / 5 * 5).clamp(r.min_to, r.max_to);
    // Don't leave a pointless sliver behind: commit when most of the stack goes in.
    if amount * 5 >= r.max_to * 4 {
        amount = r.max_to;
    }
    Some(PlayerAction::RaiseTo { amount })
}

/// A flush draw or an open-ended straight draw that uses a hole card.
fn strong_draw(hole: [Card; 2], board: &[Card]) -> bool {
    if board.len() >= 5 {
        return false;
    }
    let all: Vec<Card> = hole.iter().chain(board).copied().collect();
    let flush_draw = hole
        .iter()
        .any(|h| all.iter().filter(|c| c.suit() == h.suit()).count() == 4);
    // Ranks as a bitmask with the ace also low (bit 0); deuce is bit 1.
    let bits = |cards: &[Card]| {
        cards.iter().fold(0u16, |m, c| {
            let m = m | 1 << (c.rank() + 1);
            if c.rank() == 12 { m | 1 } else { m }
        })
    };
    let ranks = bits(&all);
    let board_ranks = bits(board);
    let made_straight = (0..=9).any(|lo| ranks >> lo & 0x1F == 0x1F);
    let open_ended = (1..=9).any(|lo| {
        let four = 0xFu16 << lo;
        ranks & four == four && board_ranks & four != four
    });
    flush_draw || open_ended && !made_straight
}

/// Fraction of the opponent's starting hands still assumed possible, given
/// how aggressively they have played this hand.
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
        // A preflop open-shove at short stacks is wide; later shoves less so.
        let preflop_open_shove = aggression == 1 && v.board.is_empty();
        range = range.min(if preflop_open_shove { 0.55 } else { 0.4 });
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

/// Heads-up preflop equity of a starting hand vs a random hand.
fn preflop_equity(a: Card, b: Card) -> f64 {
    let (hi, lo) = (a.rank().max(b.rank()), a.rank().min(b.rank()));
    let e = if a.suit() == b.suit() && hi != lo {
        preflop_table::EQUITY[hi as usize][lo as usize]
    } else {
        preflop_table::EQUITY[lo as usize][hi as usize]
    };
    e as f64
}

/// Share of all 1326 starting hands at least as strong as `hole`:
/// about 0.005 for aces, 1.0 for the weakest hand.
fn top_share(hole: [Card; 2]) -> f64 {
    let mine = preflop_equity(hole[0], hole[1]);
    let mut at_least = 0u32;
    for hi in 0..13usize {
        for lo in 0..13usize {
            let (e, combos) = match hi.cmp(&lo) {
                std::cmp::Ordering::Equal => (preflop_table::EQUITY[hi][hi], 6),
                std::cmp::Ordering::Greater => (preflop_table::EQUITY[hi][lo], 4),
                std::cmp::Ordering::Less => (preflop_table::EQUITY[hi][lo], 12),
            };
            if e as f64 >= mine {
                at_least += combos;
            }
        }
    }
    at_least as f64 / 1326.0
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
            combos.push((preflop_equity(deck[i], deck[j]), deck[i], deck[j]));
        }
    }
    combos.sort_by(|x, y| y.0.total_cmp(&x.0));
    let keep = ((combos.len() as f64 * range).ceil() as usize).clamp(1, combos.len());
    combos.truncate(keep);

    let mut total = 0.0;
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
        cards.clear();
        cards.extend_from_slice(&hole);
        cards.extend_from_slice(board);
        cards.extend_from_slice(&rest[..missing]);
        let mine = hand_value(&cards);
        cards[0] = o1;
        cards[1] = o2;
        let theirs = hand_value(&cards);
        total += match mine.cmp(&theirs) {
            std::cmp::Ordering::Greater => 1.0,
            std::cmp::Ordering::Equal => 0.5,
            std::cmp::Ordering::Less => 0.0,
        };
    }
    total / iterations as f64
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
