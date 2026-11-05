//! Three-handed arena: every trio of bot styles plays many tables, seats
//! rotated, and the average result per style is printed.
//! `cargo run -p bot --release --example table_arena [tables_per_trio]`.

use poker_core::table::{SeatKind, Table};
use poker_core::{Opponent, Phase, STARTING_STACK};

fn main() {
    let per_trio: u64 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(60);
    let all = Opponent::ALL;
    let mut chips = [0i64; 5];
    let mut firsts = [0u32; 5];
    let mut tables = [0u32; 5];
    let mut seed = 0u64;
    for a in 0..5 {
        for b in a + 1..5 {
            for c in b + 1..5 {
                let trio = [a, b, c];
                for t in 0..per_trio {
                    seed += 1;
                    // Rotate seats so nobody keeps a position.
                    let order: Vec<usize> = (0..3).map(|i| trio[(i + t as usize) % 3]).collect();
                    let seats = order
                        .iter()
                        .map(|&i| SeatKind::Bot { opponent: all[i] })
                        .collect();
                    let mut table = Table::solo(seed, seats).unwrap();
                    while table.phase != Phase::MatchOver {
                        if table.phase == Phase::HandOver {
                            table.next_hand().unwrap();
                            continue;
                        }
                        let s = table.hand.to_act.unwrap();
                        let a =
                            bot::decide_table(&table.view(s), all[order[s]], table.decision_seed());
                        table.apply(s, a).unwrap();
                    }
                    let r = table.result.unwrap();
                    for (seat, &style) in order.iter().enumerate() {
                        chips[style] += r.final_stacks[seat] as i64 - STARTING_STACK as i64;
                        tables[style] += 1;
                        if r.places[seat] == 1 {
                            firsts[style] += 1;
                        }
                    }
                }
            }
        }
    }
    println!("style       tables  avg chips  first place");
    for i in 0..5 {
        println!(
            "{:<11} {:>6}  {:>+9.1}  {:>10.1}%",
            format!("{:?}", all[i]),
            tables[i],
            chips[i] as f64 / tables[i] as f64,
            100.0 * firsts[i] as f64 / tables[i] as f64
        );
    }
}
