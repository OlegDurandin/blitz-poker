//! Unpredictable identifiers and the CSPRNG shuffle. Everything random on the
//! server comes from the operating system's CSPRNG (`getrandom`).

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use poker_core::card::{Card, full_deck};

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).expect("OS random number generator");
    buf
}

/// 128-bit room id; it is also the invite secret, so it is never logged in full.
pub fn room_id() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes::<16>())
}

/// 256-bit bearer token for sessions and one-time WebSocket tickets.
pub fn token() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes::<32>())
}

pub fn match_id() -> String {
    random_bytes::<8>()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Uniform in `0..n` without modulo bias.
fn below(n: u64) -> u64 {
    let zone = u64::MAX - u64::MAX % n;
    loop {
        let x = u64::from_le_bytes(random_bytes::<8>());
        if x < zone {
            return x % n;
        }
    }
}

/// A full deck shuffled with Fisher–Yates over the OS CSPRNG.
pub fn shuffled_deck() -> Vec<Card> {
    let mut deck = full_deck();
    for i in (1..deck.len()).rev() {
        deck.swap(i, below(i as u64 + 1) as usize);
    }
    deck
}

pub fn coin() -> bool {
    random_bytes::<1>()[0] & 1 == 1
}

/// A short, non-secret tag for logs: never log room ids or tokens in full.
pub fn log_tag(secret: &str) -> String {
    secret.chars().take(4).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_long_and_distinct() {
        let a = room_id();
        assert_eq!(a.len(), 22, "128 bits in base64url");
        assert_ne!(a, room_id());
        assert_eq!(token().len(), 43);
        assert_eq!(log_tag(&a).chars().count(), 5);
    }

    #[test]
    fn shuffle_is_a_permutation_and_varies() {
        let d = shuffled_deck();
        let mut sorted = d.clone();
        sorted.sort();
        assert_eq!(sorted, full_deck());
        assert_ne!(shuffled_deck(), d);
        for n in [1, 2, 3, 52] {
            assert!((0..200).all(|_| below(n) < n));
        }
    }
}
