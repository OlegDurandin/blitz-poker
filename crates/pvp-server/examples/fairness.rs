//! Fairness check for duels: both seats play the same simple strategy over
//! many matches with the production dealer; wins should split evenly between
//! host (Human seat) and guest (Bot seat), and between who had the first button.
//!
//! cargo run --release -p pvp-server --example fairness -- 20000

use poker_core::{Match, MatchOutcome, Phase, PlayerAction, Seat};
use pvp_server::ids::{coin, shuffled_deck};

/// Same for both seats: call or check, raise a little now and then.
fn act(m: &Match, seat: Seat, k: u64) -> PlayerAction {
    let legal = m.legal_actions(seat).expect("to act");
    if let Some(r) = &legal.raise
        && k.is_multiple_of(7)
    {
        return PlayerAction::RaiseTo { amount: r.min_to };
    }
    if legal.check {
        PlayerAction::Check
    } else if legal.fold && k.is_multiple_of(11) {
        PlayerAction::Fold
    } else {
        PlayerAction::Call
    }
}

fn main() {
    let n: u64 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(10_000);
    let (mut host, mut guest, mut draws) = (0u64, 0u64, 0u64);
    let (mut first_button_won, mut decided) = (0u64, 0u64);
    let mut chips = [0u64; 2];
    let mut k = 0u64;
    for i in 0..n {
        let button = if coin() { Seat::Human } else { Seat::Bot };
        let mut m = Match::duel(format!("f{i}"), button, &shuffled_deck()).unwrap();
        loop {
            match m.phase {
                Phase::InHand => {
                    let seat = m.hand.to_act.expect("someone acts");
                    k = k
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    m.apply(seat, act(&m, seat, (k >> 33) % 1000)).unwrap();
                }
                Phase::HandOver => m.next_hand_dealt(&shuffled_deck()).unwrap(),
                Phase::MatchOver => break,
            }
        }
        let r = m.result.as_ref().unwrap();
        chips[0] += u64::from(r.final_stacks[0]);
        chips[1] += u64::from(r.final_stacks[1]);
        let winner = match r.outcome {
            MatchOutcome::Win => Some(Seat::Human),
            MatchOutcome::Loss => Some(Seat::Bot),
            MatchOutcome::Draw => None,
        };
        match winner {
            Some(Seat::Human) => host += 1,
            Some(Seat::Bot) => guest += 1,
            None => draws += 1,
        }
        if let Some(w) = winner {
            decided += 1;
            if w == button {
                first_button_won += 1;
            }
        }
    }
    let pct = |a: u64, b: u64| 100.0 * a as f64 / b.max(1) as f64;
    let sigma = 100.0 * 0.5 / (decided as f64).sqrt();
    println!("matches: {n}, decided: {decided}, draws: {draws}");
    println!(
        "host (Human seat) won: {:.2}%   guest (Bot seat) won: {:.2}%",
        pct(host, decided),
        pct(guest, decided)
    );
    println!(
        "first-button player won: {:.2}%",
        pct(first_button_won, decided)
    );
    println!(
        "avg final stacks: host {:.1}  guest {:.1}",
        chips[0] as f64 / n as f64,
        chips[1] as f64 / n as f64
    );
    println!("(one standard error ≈ {sigma:.2} percentage points)");
}
