//! Bot evaluation harness: `cargo run -p bot --release --example evaluate [matches]`.
//!
//! Each deal is played twice with the seats swapped ("duplicate" format), so
//! card luck mostly cancels out. The bot under test is measured against
//! reference policies and against the frozen 1.0 bot. Besides results it
//! reports the action mix per street and a few pathology counters.

mod baseline_v1;

use bot::Personality;
use bot::reference::{calling_station, random_legal, tight_aggressive};
use poker_core::card::full_deck;
use poker_core::rng::Rng;
use poker_core::{
    Card, Match, Opponent, Phase, PlayerAction, PlayerView, Seat, Street, hand_value,
};
use std::time::{Duration, Instant};

type Policy = fn(&PlayerView, u64) -> PlayerAction;

fn current(v: &PlayerView, seed: u64) -> PlayerAction {
    bot::decide(v, Personality::Standard(Opponent::Alex), seed)
}

fn novice(v: &PlayerView, seed: u64) -> PlayerAction {
    bot::decide(v, Personality::Standard(Opponent::Novice), seed)
}

fn tight(v: &PlayerView, seed: u64) -> PlayerAction {
    bot::decide(v, Personality::Standard(Opponent::Tight), seed)
}

fn station(v: &PlayerView, seed: u64) -> PlayerAction {
    bot::decide(v, Personality::Standard(Opponent::Station), seed)
}

fn aggressor(v: &PlayerView, seed: u64) -> PlayerAction {
    bot::decide(v, Personality::Standard(Opponent::Aggressor), seed)
}

fn baseline(v: &PlayerView, seed: u64) -> PlayerAction {
    baseline_v1::decide(v, baseline_v1::Personality::Standard, seed)
}

/// Equity against a uniformly random hand: a seat-independent strength gauge.
fn raw_strength(hole: [Card; 2], board: &[Card], rng: &mut Rng) -> f64 {
    let known: Vec<Card> = hole.iter().chain(board).copied().collect();
    let deck: Vec<Card> = full_deck()
        .into_iter()
        .filter(|c| !known.contains(c))
        .collect();
    let mut score = 0.0;
    const N: usize = 300;
    for _ in 0..N {
        let mut d = deck.clone();
        let need = 2 + 5 - board.len();
        for k in 0..need {
            let j = k + rng.below((d.len() - k) as u64) as usize;
            d.swap(k, j);
        }
        let full: Vec<Card> = board
            .iter()
            .copied()
            .chain(d[2..need].iter().copied())
            .collect();
        let mine = hand_value(&[&hole[..], &full[..]].concat());
        let theirs = hand_value(&[&d[..2], &full[..]].concat());
        score += match mine.cmp(&theirs) {
            std::cmp::Ordering::Greater => 1.0,
            std::cmp::Ordering::Equal => 0.5,
            std::cmp::Ordering::Less => 0.0,
        };
    }
    score / N as f64
}

#[derive(Default)]
struct Mix {
    fold: u32,
    check: u32,
    call: u32,
    bet_raise: u32,
    /// Shoves the player started.
    all_in: u32,
    /// Calls of the opponent's all-in.
    call_all_in: u32,
    /// Shoves made with a stack no bigger than the pot (any real bet is all-in).
    shove_short: u32,
    min_bets: u32,
}

impl Mix {
    fn total(&self) -> u32 {
        self.fold + self.check + self.call + self.bet_raise + self.all_in + self.call_all_in
    }
}

#[derive(Default)]
struct Report {
    wins: u32,
    losses: u32,
    draws: u32,
    chips: i64,
    matches: u32,
    mix: [Mix; 4],
    /// Calls of at least a pot-sized bet holding < 30% raw equity.
    weak_big_calls: u32,
    big_calls: u32,
    /// Bets/raises when holding < 25% raw equity (bluffs), and their share.
    bluffs: u32,
    /// Folds when holding > 80% raw equity.
    strong_folds: u32,
    /// Postflop decisions facing a bet, and folds among them.
    facing_bet: u32,
    folds_to_bet: u32,
    decisions: u32,
    illegal: u32,
    think: Duration,
    slowest: Duration,
}

fn street_idx(s: Street) -> usize {
    match s {
        Street::Preflop => 0,
        Street::Flop => 1,
        Street::Turn => 2,
        Street::River => 3,
    }
}

fn observe(r: &mut Report, v: &PlayerView, a: PlayerAction, rng: &mut Rng) {
    let l = v.legal.as_ref().unwrap();
    let mix = &mut r.mix[street_idx(v.street)];
    let pot = v.pot + v.bets[0] + v.bets[1];
    let strength = raw_strength(v.hole, &v.board, rng);
    if v.street != Street::Preflop && v.to_call > 0 {
        r.facing_bet += 1;
        if a == PlayerAction::Fold {
            r.folds_to_bet += 1;
        }
    }
    match a {
        PlayerAction::Fold => {
            mix.fold += 1;
            if strength > 0.8 {
                r.strong_folds += 1;
            }
        }
        PlayerAction::Check => mix.check += 1,
        PlayerAction::Call => {
            if l.call_is_all_in {
                mix.call_all_in += 1;
            } else {
                mix.call += 1;
            }
            if v.to_call * 2 >= pot {
                r.big_calls += 1;
                if strength < 0.3 {
                    r.weak_big_calls += 1;
                }
            }
        }
        PlayerAction::RaiseTo { amount } => {
            let rr = l.raise.as_ref().unwrap();
            if amount == rr.max_to {
                mix.all_in += 1;
                if v.stacks[v.me.idx()] <= pot {
                    mix.shove_short += 1;
                }
            } else {
                mix.bet_raise += 1;
                if amount == rr.min_to {
                    mix.min_bets += 1;
                }
            }
            if strength < 0.25 {
                r.bluffs += 1;
            }
        }
    }
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

/// Plays `deals` deals twice each (seats swapped). `tested` is measured.
fn duel(tested: Policy, opponent: Policy, deals: u64) -> Report {
    let mut r = Report::default();
    let mut rng = Rng::new(99);
    for deal in 0..deals {
        for tested_seat in Seat::BOTH {
            let mut m = Match::new(deal * 2_654_435_761 + 17);
            while m.phase != Phase::MatchOver {
                if m.phase != Phase::InHand {
                    m.next_hand().unwrap();
                    continue;
                }
                let seat = m.hand.to_act.unwrap();
                let v = m.view(seat);
                let seed = m.decision_seed();
                let a = if seat == tested_seat {
                    let t = Instant::now();
                    let a = tested(&v, seed);
                    let dt = t.elapsed();
                    r.think += dt;
                    r.slowest = r.slowest.max(dt);
                    r.decisions += 1;
                    if !is_legal(&v, a) {
                        r.illegal += 1;
                    }
                    observe(&mut r, &v, a, &mut rng);
                    a
                } else {
                    // Opponents get their own seed stream so mirrored deals differ only in seats.
                    opponent(&v, seed ^ 0x0FF0)
                };
                m.apply(seat, a).expect("all policies play legal actions");
            }
            let res = m.result.unwrap();
            let mine = res.final_stacks[tested_seat.idx()] as i64;
            let theirs = res.final_stacks[tested_seat.other().idx()] as i64;
            r.chips += mine - 600;
            r.matches += 1;
            match mine.cmp(&theirs) {
                std::cmp::Ordering::Greater => r.wins += 1,
                std::cmp::Ordering::Less => r.losses += 1,
                std::cmp::Ordering::Equal => r.draws += 1,
            }
        }
    }
    r
}

fn pct(n: u32, d: u32) -> String {
    if d == 0 {
        "—".into()
    } else {
        format!("{:.0}%", 100.0 * n as f64 / d as f64)
    }
}

fn print(name: &str, opp: &str, r: &Report) {
    println!(
        "{name:>8} vs {opp:<16} W/L/D {:>3}/{:>3}/{:>2}  win {:>4}  avg {:+6.0} chips  \
         big calls {:>4} (weak {:>4})  bluffs {:>4}  strong folds {}  illegal {}  \
         avg {:.1} ms, max {:.1} ms",
        r.wins,
        r.losses,
        r.draws,
        pct(r.wins, r.wins + r.losses),
        r.chips as f64 / r.matches as f64,
        r.big_calls,
        pct(r.weak_big_calls, r.big_calls),
        pct(r.bluffs, r.decisions),
        r.strong_folds,
        r.illegal,
        r.think.as_secs_f64() * 1000.0 / r.decisions.max(1) as f64,
        r.slowest.as_secs_f64() * 1000.0,
    );
}

fn print_mix(r: &Report) {
    for (s, m) in ["preflop", "flop", "turn", "river"].iter().zip(&r.mix) {
        let t = m.total();
        println!(
            "           {s:<8} n={t:<5} fold {:>4} check {:>4} call {:>4} bet/raise {:>4} shove {:>4} ({} with stack <= pot) call-all-in {:>4}  (min-size {} of bets)",
            pct(m.fold, t),
            pct(m.check, t),
            pct(m.call, t),
            pct(m.bet_raise, t),
            pct(m.all_in, t),
            pct(m.shove_short, m.all_in),
            pct(m.call_all_in, t),
            pct(m.min_bets, m.bet_raise),
        );
    }
}

/// One line per opponent style: results and the numbers that make the styles differ.
fn personas(deals: u64) {
    let styles: [(&str, Policy); 5] = [
        ("novice", novice),
        ("station", station),
        ("tight", tight),
        ("alex", current),
        ("aggressor", aggressor),
    ];
    let opponents: [(&str, Policy); 4] = [
        ("random", random_legal),
        ("station", calling_station),
        ("tight-aggr", tight_aggressive),
        ("alex", current),
    ];
    println!("{deals} deals x 2 seats; chips = average result per match\n");
    println!(
        "{:<10} {:>8} {:>8} {:>11} {:>8}   {:>9} {:>8} {:>12} {:>6}",
        "style",
        "random",
        "station",
        "tight-aggr",
        "alex",
        "pre-fold",
        "post AF",
        "fold-to-bet",
        "ms"
    );
    for (name, tested) in styles {
        let mut chips = vec![];
        let mut all = Report::default();
        for (_, opp) in opponents {
            let r = duel(tested, opp, deals);
            chips.push(r.chips as f64 / r.matches as f64);
            for (a, b) in all.mix.iter_mut().zip(&r.mix) {
                a.fold += b.fold;
                a.check += b.check;
                a.call += b.call;
                a.bet_raise += b.bet_raise;
                a.all_in += b.all_in;
                a.call_all_in += b.call_all_in;
            }
            all.facing_bet += r.facing_bet;
            all.folds_to_bet += r.folds_to_bet;
            all.decisions += r.decisions;
            all.think += r.think;
        }
        let post = &all.mix[1..];
        let aggressive: u32 = post.iter().map(|m| m.bet_raise + m.all_in).sum();
        let calls: u32 = post.iter().map(|m| m.call + m.call_all_in).sum();
        println!(
            "{name:<10} {:>+8.0} {:>+8.0} {:>+11.0} {:>+8.0}   {:>9} {:>8.2} {:>12} {:>6.1}",
            chips[0],
            chips[1],
            chips[2],
            chips[3],
            pct(all.mix[0].fold, all.mix[0].total()),
            aggressive as f64 / calls.max(1) as f64,
            pct(all.folds_to_bet, all.facing_bet),
            all.think.as_secs_f64() * 1000.0 / all.decisions.max(1) as f64,
        );
    }
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("personas") {
        let deals = std::env::args()
            .nth(2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(200);
        return personas(deals);
    }
    let deals: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);
    println!("{deals} deals x 2 seats per pairing\n");
    let opponents: [(&str, Policy); 3] = [
        ("random-legal", random_legal),
        ("calling-station", calling_station),
        ("tight-aggr", tight_aggressive),
    ];
    for (name, tested) in [("1.0", baseline as Policy), ("1.1", current as Policy)] {
        let mut all = Report::default();
        for (opp, policy) in opponents {
            let r = duel(tested, policy, deals);
            print(name, opp, &r);
            for (a, b) in all.mix.iter_mut().zip(&r.mix) {
                a.fold += b.fold;
                a.check += b.check;
                a.call += b.call;
                a.bet_raise += b.bet_raise;
                a.all_in += b.all_in;
                a.call_all_in += b.call_all_in;
                a.shove_short += b.shove_short;
                a.min_bets += b.min_bets;
            }
        }
        println!("         action mix over all three:");
        print_mix(&all);
        println!();
    }
    let r = duel(current, baseline, deals);
    print("1.1", "bot 1.0", &r);
}
