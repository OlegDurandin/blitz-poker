//! Sanity probe: hand-built spots, every opponent decides many times, and the
//! spread of choices is printed. `cargo run -p bot --release --example probe`.
//!
//! The human sits in `Seat::Human`, the bot in `Seat::Bot`. Deck order is
//! human, bot, human, bot (hole cards), then the board.

use bot::Personality;
use poker_core::card::{Card, full_deck};
use poker_core::{Match, Opponent, PlayerAction, Seat};

const N: u64 = 300;

fn deck(human: &str, bot: &str, board: &str) -> Vec<Card> {
    let parse = |s: &str| -> Vec<Card> {
        s.split_whitespace()
            .map(|c| Card::parse(c).unwrap_or_else(|| panic!("card {c}")))
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

struct Spot {
    name: &'static str,
    /// Who has the button (small blind, acts first preflop).
    button: Seat,
    human: &'static str,
    bot: &'static str,
    board: &'static str,
    /// Actions played before the bot decides, in order.
    script: &'static [(Seat, PlayerAction)],
    /// What a sensible player mostly does here.
    expect: &'static str,
}

const H: Seat = Seat::Human;
const B: Seat = Seat::Bot;
use PlayerAction::{Call, Check, RaiseTo};

fn spots() -> Vec<Spot> {
    vec![
        Spot {
            name: "SB, AA preflop",
            button: B,
            human: "7c 2d",
            bot: "As Ah",
            board: "Kd 9s 4c 3h 2s",
            script: &[],
            expect: "raise",
        },
        Spot {
            name: "SB, 7-2 off preflop",
            button: B,
            human: "As Ah",
            bot: "7c 2d",
            board: "Kd 9s 4c 3h 2s",
            script: &[],
            expect: "fold (loose styles may limp)",
        },
        Spot {
            name: "BB vs 3x open, 7-2 off",
            button: H,
            human: "As Kh",
            bot: "7c 2d",
            board: "Kd 9s 4c 3h 2s",
            script: &[(H, RaiseTo { amount: 60 })],
            expect: "fold",
        },
        Spot {
            name: "BB vs 3x open, A-K suited",
            button: H,
            human: "9c 9d",
            bot: "Ah Kh",
            board: "Qd 9s 4c 3h 2s",
            script: &[(H, RaiseTo { amount: 60 })],
            expect: "3-bet or call",
        },
        Spot {
            name: "Flop nut flush, checked to",
            button: B,
            human: "Kd Qd",
            bot: "Ah Kh",
            board: "Qh 7h 2h 5c 9s",
            script: &[(B, Call), (H, Check), (H, Check)],
            expect: "bet (sometimes slow-play)",
        },
        Spot {
            name: "Flop air, opp bets pot",
            button: H,
            human: "Ah Ad",
            bot: "8c 3d",
            board: "Ks Qh 2c 5d 9h",
            script: &[
                (H, RaiseTo { amount: 60 }),
                (B, Call),
                (B, Check),
                (H, RaiseTo { amount: 120 }),
            ],
            expect: "fold (rare bluff-raise)",
        },
        Spot {
            name: "River nuts, opp bets",
            button: H,
            human: "Kc Kd",
            bot: "Ah 5h",
            board: "Kh 8h 2h 7c 3s",
            script: &[
                (H, RaiseTo { amount: 60 }),
                (B, Call),
                (B, Check),
                (H, RaiseTo { amount: 60 }),
                (B, Call),
                (B, Check),
                (H, RaiseTo { amount: 120 }),
                (B, Call),
                (B, Check),
                (H, RaiseTo { amount: 200 }),
            ],
            expect: "raise / all-in, never fold",
        },
        Spot {
            name: "River bottom pair vs all-in",
            button: H,
            human: "Ac Ad",
            bot: "3c 4d",
            board: "Ks Qh 3h 8c 9d",
            script: &[
                (H, RaiseTo { amount: 60 }),
                (B, Call),
                (B, Check),
                (H, RaiseTo { amount: 60 }),
                (B, Call),
                (B, Check),
                (H, Check),
                (B, Check),
                (H, RaiseTo { amount: 480 }),
            ],
            expect: "fold (station may call)",
        },
        Spot {
            name: "River missed draw, checked to",
            button: B,
            human: "Kc 9c",
            bot: "Jh Th",
            board: "Kh 8h 2c 3s 4d",
            script: &[
                (B, Call),
                (H, Check),
                (H, Check),
                (B, RaiseTo { amount: 20 }),
                (H, Call),
                (H, Check),
                (B, Check),
                (H, Check),
            ],
            expect: "mostly check, some bluffs",
        },
        Spot {
            name: "Top pair, opp bets half pot",
            button: H,
            human: "Qc Jc",
            bot: "Ah Kd",
            board: "Kh 8s 2c 5d 9h",
            script: &[
                (H, RaiseTo { amount: 60 }),
                (B, Call),
                (B, Check),
                (H, RaiseTo { amount: 60 }),
            ],
            expect: "call or raise",
        },
    ]
}

fn label(a: PlayerAction, stack_after_call: u32, max_to: Option<u32>) -> &'static str {
    match a {
        PlayerAction::Fold => "fold",
        PlayerAction::Check => "check",
        PlayerAction::Call => "call",
        PlayerAction::RaiseTo { amount } if Some(amount) == max_to && stack_after_call > 0 => {
            "all-in"
        }
        PlayerAction::RaiseTo { .. } => "raise",
    }
}

fn main() {
    for spot in spots() {
        let mut m = Match::duel(
            "probe".into(),
            spot.button,
            &deck(spot.human, spot.bot, spot.board),
        )
        .unwrap();
        for &(seat, a) in spot.script {
            m.apply(seat, a)
                .unwrap_or_else(|e| panic!("{}: {seat:?} {a:?}: {e:?}", spot.name));
        }
        assert_eq!(m.hand.to_act, Some(B), "{}: bot should act", spot.name);
        let view = m.view(B);
        let max_to = view
            .legal
            .as_ref()
            .and_then(|l| l.raise.as_ref())
            .map(|r| r.max_to);
        println!("{}  [{}]  expect: {}", spot.name, spot.bot, spot.expect);
        for o in Opponent::ALL {
            let mut counts: Vec<(&str, u32)> = vec![];
            for seed in 0..N {
                let a = bot::decide(&view, Personality::Standard(o), seed * 7919 + 1);
                let l = label(a, view.stacks[1], max_to);
                match counts.iter_mut().find(|(k, _)| *k == l) {
                    Some((_, n)) => *n += 1,
                    None => counts.push((l, 1)),
                }
            }
            counts.sort_by_key(|c| std::cmp::Reverse(c.1));
            let shown: Vec<String> = counts
                .iter()
                .map(|(k, n)| format!("{k} {:.0}%", 100.0 * *n as f64 / N as f64))
                .collect();
            println!("    {:<10} {}", format!("{o:?}"), shown.join(", "));
        }
        println!();
    }
}
