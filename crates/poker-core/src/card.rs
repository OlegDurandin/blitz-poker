//! Cards and the 52-card deck.
//!
//! A card serializes as a two-character string such as `"As"` or `"Td"` so
//! that the JSON snapshot stays compact and readable from the UI side.

use serde::{Deserialize, Serialize};
use std::fmt;

const RANK_CHARS: &[u8; 13] = b"23456789TJQKA";
const SUIT_CHARS: &[u8; 4] = b"shdc";

/// Rank index: 0 = deuce … 12 = ace.
pub type Rank = u8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Suit {
    Spades = 0,
    Hearts = 1,
    Diamonds = 2,
    Clubs = 3,
}

impl Suit {
    pub const ALL: [Suit; 4] = [Suit::Spades, Suit::Hearts, Suit::Diamonds, Suit::Clubs];
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct Card(u8);

impl Card {
    pub fn new(rank: Rank, suit: Suit) -> Card {
        assert!(rank < 13, "rank out of range");
        Card(rank * 4 + suit as u8)
    }

    /// Index in `0..52`.
    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub fn from_index(index: usize) -> Card {
        assert!(index < 52, "card index out of range");
        Card(index as u8)
    }

    pub fn rank(self) -> Rank {
        self.0 / 4
    }

    pub fn suit(self) -> Suit {
        Suit::ALL[(self.0 % 4) as usize]
    }

    pub fn parse(s: &str) -> Option<Card> {
        let bytes = s.as_bytes();
        if bytes.len() != 2 {
            return None;
        }
        let rank = RANK_CHARS
            .iter()
            .position(|&c| c == bytes[0].to_ascii_uppercase())?;
        let suit = SUIT_CHARS
            .iter()
            .position(|&c| c == bytes[1].to_ascii_lowercase())?;
        Some(Card::new(rank as u8, Suit::ALL[suit]))
    }
}

impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let r = RANK_CHARS[self.rank() as usize] as char;
        let s = SUIT_CHARS[self.suit() as usize] as char;
        write!(f, "{r}{s}")
    }
}

impl fmt::Debug for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl From<Card> for String {
    fn from(card: Card) -> String {
        card.to_string()
    }
}

impl TryFrom<String> for Card {
    type Error = String;

    fn try_from(s: String) -> Result<Card, String> {
        Card::parse(&s).ok_or_else(|| format!("invalid card: {s:?}"))
    }
}

/// Parses a whitespace-separated list such as `"As Kd 7c"`. Panics on bad input;
/// intended for tests and fixed tutorial decks.
pub fn cards(s: &str) -> Vec<Card> {
    s.split_whitespace()
        .map(|c| Card::parse(c).unwrap_or_else(|| panic!("bad card {c:?}")))
        .collect()
}

pub fn full_deck() -> Vec<Card> {
    (0..52).map(Card::from_index).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_all_cards() {
        for card in full_deck() {
            assert_eq!(Card::parse(&card.to_string()), Some(card));
        }
    }

    #[test]
    fn serializes_as_string() {
        let card = Card::new(12, Suit::Spades);
        assert_eq!(serde_json::to_string(&card).unwrap(), "\"As\"");
        let back: Card = serde_json::from_str("\"Td\"").unwrap();
        assert_eq!(back, Card::new(8, Suit::Diamonds));
        assert!(serde_json::from_str::<Card>("\"Xx\"").is_err());
    }
}
