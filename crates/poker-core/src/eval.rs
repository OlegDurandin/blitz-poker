//! Hand evaluation: best five-card hand out of five to seven cards.

use crate::card::{Card, Rank};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    HighCard,
    Pair,
    TwoPair,
    ThreeOfAKind,
    Straight,
    Flush,
    FullHouse,
    FourOfAKind,
    StraightFlush,
    /// Display-only refinement of an ace-high straight flush; ranks like one.
    RoyalFlush,
}

/// Totally ordered hand strength: higher is better, equal means a split.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct HandValue(u32);

impl HandValue {
    pub fn category(self) -> Category {
        match self.0 >> 20 {
            0 => Category::HighCard,
            1 => Category::Pair,
            2 => Category::TwoPair,
            3 => Category::ThreeOfAKind,
            4 => Category::Straight,
            5 => Category::Flush,
            6 => Category::FullHouse,
            7 => Category::FourOfAKind,
            _ if self.0 & 0xF_FFFF == 12 << 16 => Category::RoyalFlush,
            _ => Category::StraightFlush,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BestHand {
    pub value: HandValue,
    /// The five cards making the hand, most significant first.
    pub cards: [Card; 5],
}

impl BestHand {
    pub fn category(&self) -> Category {
        self.value.category()
    }
}

fn encode(class: u32, kickers: &[Rank]) -> HandValue {
    let mut v = class << 20;
    for (i, &r) in kickers.iter().take(5).enumerate() {
        v |= (r as u32) << (16 - 4 * i);
    }
    HandValue(v)
}

/// Evaluates exactly five cards and returns the value plus the cards ordered by
/// significance (e.g. the pair first, then kickers high to low).
pub fn eval5(hand: [Card; 5]) -> (HandValue, [Card; 5]) {
    let mut cards = hand;
    cards.sort_by_key(|c| std::cmp::Reverse(c.rank()));

    let mut counts = [0u8; 13];
    for c in &cards {
        counts[c.rank() as usize] += 1;
    }
    let flush = cards.iter().all(|c| c.suit() == cards[0].suit());

    let distinct = counts.iter().filter(|&&n| n > 0).count();
    let straight_high: Option<Rank> = if distinct == 5 {
        let hi = cards[0].rank();
        let lo = cards[4].rank();
        if hi - lo == 4 {
            Some(hi)
        } else if hi == 12 && cards[1].rank() == 3 {
            // Wheel: A-5-4-3-2, the ace plays low.
            cards.rotate_left(1);
            Some(3)
        } else {
            None
        }
    } else {
        None
    };

    if let Some(high) = straight_high {
        let class = if flush { 8 } else { 4 };
        return (encode(class, &[high]), cards);
    }
    if flush {
        let ranks: Vec<Rank> = cards.iter().map(|c| c.rank()).collect();
        return (encode(5, &ranks), cards);
    }

    // Group by (count desc, rank desc).
    cards.sort_by(|a, b| {
        let (ca, cb) = (counts[a.rank() as usize], counts[b.rank() as usize]);
        cb.cmp(&ca).then(b.rank().cmp(&a.rank()))
    });
    let mut groups: Vec<(u8, Rank)> = Vec::with_capacity(5);
    for c in &cards {
        if groups.last().map(|g| g.1) != Some(c.rank()) {
            groups.push((counts[c.rank() as usize], c.rank()));
        }
    }
    let ranks: Vec<Rank> = groups.iter().map(|g| g.1).collect();
    let class = match (groups[0].0, groups.get(1).map(|g| g.0)) {
        (4, _) => 7,
        (3, Some(2)) => 6,
        (3, _) => 3,
        (2, Some(2)) => 2,
        (2, _) => 1,
        _ => 0,
    };
    (encode(class, &ranks), cards)
}

/// Best five-card hand from 5..=7 cards.
pub fn best_hand(cards: &[Card]) -> BestHand {
    let n = cards.len();
    assert!((5..=7).contains(&n), "need 5 to 7 cards, got {n}");
    let mut best: Option<BestHand> = None;
    for mask in 0u32..(1 << n) {
        if mask.count_ones() != 5 {
            continue;
        }
        let mut five = [cards[0]; 5];
        let mut k = 0;
        for (i, &c) in cards.iter().enumerate() {
            if mask & (1 << i) != 0 {
                five[k] = c;
                k += 1;
            }
        }
        let (value, ordered) = eval5(five);
        if best.is_none_or(|b| value > b.value) {
            best = Some(BestHand {
                value,
                cards: ordered,
            });
        }
    }
    best.expect("at least one combination")
}

/// Strength only, for hot loops such as Monte Carlo equity.
pub fn hand_value(cards: &[Card]) -> HandValue {
    best_hand(cards).value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::cards;

    fn cat(s: &str) -> Category {
        best_hand(&cards(s)).category()
    }

    fn val(s: &str) -> HandValue {
        hand_value(&cards(s))
    }

    #[test]
    fn categories() {
        assert_eq!(cat("As Ks Qs Js Ts 2d 3c"), Category::RoyalFlush);
        assert_eq!(cat("9h 8h 7h 6h 5h Ad Ac"), Category::StraightFlush);
        assert_eq!(cat("5d 4d 3d 2d Ad Kc Qh"), Category::StraightFlush);
        assert_eq!(cat("7s 7h 7d 7c 2s 3h 9d"), Category::FourOfAKind);
        assert_eq!(cat("Ks Kh Kd 4c 4s 2h 9d"), Category::FullHouse);
        assert_eq!(cat("Ks Kh Kd 4c 4s 4h 9d"), Category::FullHouse);
        assert_eq!(cat("2h 7h 9h Jh Kh As Ad"), Category::Flush);
        assert_eq!(cat("As 2d 3c 4h 5s Kd Kc"), Category::Straight);
        assert_eq!(cat("Ts Jd Qc Kh As 2d 2c"), Category::Straight);
        assert_eq!(cat("Qs Qh Qd 2c 5s 8h 9d"), Category::ThreeOfAKind);
        assert_eq!(cat("Qs Qh 5d 5c 2s 8h 9d"), Category::TwoPair);
        assert_eq!(cat("Qs Qh 5d 7c 2s 8h 9d"), Category::Pair);
        assert_eq!(cat("Qs Jh 5d 7c 2s 8h 9d"), Category::HighCard);
    }

    #[test]
    fn ordering_and_kickers() {
        assert!(val("As Ad Kc Qh 9s 3d 2c") > val("As Ad Kc Jh 9s 3d 2c"));
        assert!(
            val("6s 5d 4c 3h 2s Kd Kc") > val("As 2d 3c 4h 5s Kd 9c"),
            "6-high beats wheel"
        );
        assert!(
            val("Ks Kh 2d 2c 3s 3h Ad") > val("Ks Kh 2d 2c 3s 4h Qd"),
            "best two pair + kicker"
        );
        assert!(val("2s 2h 2d 3c 3s 4h 5d") < val("2s 2h 2d 4c 4s 9h 5d"));
        // Board plays: split.
        assert_eq!(val("As Ks Qs Js Ts 2d 3c"), val("As Ks Qs Js Ts 4h 5h"));
    }

    #[test]
    fn best_five_cards_reported() {
        let best = best_hand(&cards("Qs Qh 5d 5c 2s 8h 9d"));
        assert_eq!(best.cards.to_vec(), cards("Qs Qh 5d 5c 9d"));
        let wheel = best_hand(&cards("As 2d 3c 4h 5s Kd 9c"));
        assert_eq!(wheel.cards.to_vec(), cards("5s 4h 3c 2d As"));
    }

    /// Two showdowns reported from a real device (1.1): both were scored
    /// correctly; the UI now names the ranks and the deciding kicker.
    #[test]
    fn reported_showdowns() {
        // Board K K J J 4: J6 makes jacks full of kings, A2 only the board's two pair.
        let board = "Kd Ks Jc Jh 4c";
        let human = best_hand(&cards(&format!("Js 6d {board}")));
        let bot = best_hand(&cards(&format!("Ac 2c {board}")));
        assert_eq!(human.category(), Category::FullHouse);
        assert_eq!(human.cards[..3], cards("Js Jc Jh")[..], "trips first");
        assert_eq!(bot.category(), Category::TwoPair);
        assert_eq!(bot.cards.to_vec(), cards("Kd Ks Jc Jh Ac"));
        assert!(human.value > bot.value);

        // Board 7 6 5 J J: both play the board's jacks; the king kicker beats the nine.
        let board = "7h 6s 5c Jd Jh";
        let human = best_hand(&cards(&format!("9h 3c {board}")));
        let bot = best_hand(&cards(&format!("Kc 3h {board}")));
        assert_eq!(
            (human.category(), bot.category()),
            (Category::Pair, Category::Pair)
        );
        assert_eq!(human.cards.to_vec(), cards("Jd Jh 9h 7h 6s"));
        assert_eq!(bot.cards.to_vec(), cards("Jd Jh Kc 7h 6s"));
        assert!(bot.value > human.value);
    }

    #[test]
    fn exhaustive_five_card_category_counts() {
        // Known distribution over all C(52,5) = 2,598,960 hands.
        let deck = crate::card::full_deck();
        let mut counts = [0u32; 10];
        for a in 0..52 {
            for b in a + 1..52 {
                for c in b + 1..52 {
                    for d in c + 1..52 {
                        for e in d + 1..52 {
                            let (v, _) = eval5([deck[a], deck[b], deck[c], deck[d], deck[e]]);
                            counts[v.category() as usize] += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(
            counts,
            [
                1_302_540, 1_098_240, 123_552, 54_912, 10_200, 5_108, 3_744, 624, 36, 4
            ]
        );
    }
}
