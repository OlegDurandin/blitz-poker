//! Reference opponents for tests and the `evaluate` harness. Not used in play.

use poker_core::rng::Rng;
use poker_core::{Category, PlayerAction, PlayerView, Street, best_hand};

pub type Policy = fn(&PlayerView, u64) -> PlayerAction;

fn passive(v: &PlayerView) -> PlayerAction {
    if v.legal.as_ref().is_some_and(|l| l.check) {
        PlayerAction::Check
    } else {
        PlayerAction::Call
    }
}

/// Never folds, never raises.
pub fn calling_station(v: &PlayerView, _: u64) -> PlayerAction {
    passive(v)
}

/// Uniform over the legal action types; raises to a uniform legal size.
pub fn random_legal(v: &PlayerView, seed: u64) -> PlayerAction {
    let l = v.legal.as_ref().expect("called on own turn");
    let mut rng = Rng::new(seed ^ 0xA11CE);
    let mut options = vec![];
    // Folding is always legal, but a free check is never given up: keeps this
    // baseline comparable with measurements from before that rule.
    if l.fold && !l.check {
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

/// Simple tight-aggressive rules a careful beginner might follow.
pub fn tight_aggressive(v: &PlayerView, _: u64) -> PlayerAction {
    let l = v.legal.as_ref().expect("called on own turn");
    let pot = v.pot + v.bets[0] + v.bets[1];
    let raise = |frac: f64| {
        l.raise.as_ref().map(|r| {
            let to = v.bets[0].max(v.bets[1]) + (pot as f64 * frac) as u32;
            PlayerAction::RaiseTo {
                amount: to.clamp(r.min_to, r.max_to),
            }
        })
    };
    let [a, b] = v.hole;
    let (hi, lo) = (a.rank().max(b.rank()), a.rank().min(b.rank()));
    if v.street == Street::Preflop {
        let premium = hi == lo && hi >= 7 || hi == 12 && lo >= 9;
        let playable =
            hi == lo || hi >= 10 && lo >= 7 || hi == 12 || a.suit() == b.suit() && hi - lo <= 2;
        return if premium {
            raise(1.5).unwrap_or(passive(v))
        } else if playable && v.to_call <= v.bb * 3 {
            passive(v)
        } else if l.check {
            PlayerAction::Check
        } else {
            PlayerAction::Fold
        };
    }
    let mut seven = v.hole.to_vec();
    seven.extend_from_slice(&v.board);
    let made = best_hand(&seven).category();
    let top_board = v.board.iter().map(|c| c.rank()).max().unwrap_or(0);
    let strong = made >= Category::TwoPair;
    let top_pair = made == Category::Pair && v.hole.iter().any(|c| c.rank() >= top_board);
    if l.check {
        return if strong || top_pair {
            raise(0.66).unwrap_or(PlayerAction::Check)
        } else {
            PlayerAction::Check
        };
    }
    if strong {
        return raise(0.8).unwrap_or(PlayerAction::Call);
    }
    if top_pair || made == Category::Pair && v.to_call * 3 <= pot {
        return PlayerAction::Call;
    }
    PlayerAction::Fold
}
