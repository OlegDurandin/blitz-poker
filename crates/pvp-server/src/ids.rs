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

/// Six digits a person can dictate or type. Derived from the room id (FNV-1a),
/// so it survives a restart with the room and needs no storage. Not a secret on
/// its own: it only finds a room still waiting for a guest, and lookups are
/// rate-limited per user (`hub`).
pub fn room_code(room_id: &str) -> String {
    let hash = room_id.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    format!("{:06}", hash % 1_000_000)
}

/// "482 913", "482-913", " 482913 " → "482913"; anything else → None.
pub fn parse_code(text: &str) -> Option<String> {
    let digits: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    (digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_digit())).then_some(digits)
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
    fn room_codes_are_six_digits_and_stable() {
        let id = room_id();
        let code = room_code(&id);
        assert_eq!(code.len(), 6);
        assert!(code.bytes().all(|b| b.is_ascii_digit()));
        assert_eq!(room_code(&id), code, "same room, same code");
        assert_eq!(parse_code(" 482 913 ").as_deref(), Some("482913"));
        assert_eq!(parse_code("482-913").as_deref(), Some("482913"));
        assert_eq!(parse_code("012345").as_deref(), Some("012345"));
        for bad in ["", "12345", "1234567", "12a456", "１２３４５６"] {
            assert_eq!(parse_code(bad), None, "{bad}");
        }
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
