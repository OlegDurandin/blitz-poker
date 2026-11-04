//! Who a player is, across the platforms that share one duel space.
//!
//! VK and Telegram players, and guests who came in by room code, meet in the
//! same rooms. Each gets a `PlayerId` in its own range of one `u64`, so ids
//! never collide, the database keeps plain integers, and VK ids stored before
//! other platforms existed stay valid as they are.
//!
//! | Platform | Range                       | Source                          |
//! |----------|-----------------------------|---------------------------------|
//! | VK       | `1 .. 2^61`                 | `vk_user_id`, signed by VK      |
//! | Telegram | `2^61 | user.id`            | `initData`, signed for the bot  |
//! | Guest    | `2^62 | 48-bit digest`      | browser key (`server::guest_id`)|

use serde::{Deserialize, Serialize};

const TELEGRAM_BIT: u64 = 1 << 61;
const GUEST_BIT: u64 = 1 << 62;
/// Ids on either platform must stay below this to keep the ranges apart.
const PLATFORM_ID_LIMIT: u64 = TELEGRAM_BIT;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Platform {
    Vk,
    Telegram,
    /// No messenger sign-in: joined by room code from a plain browser.
    Guest,
}

impl Platform {
    pub fn as_str(self) -> &'static str {
        match self {
            Platform::Vk => "vk",
            Platform::Telegram => "tg",
            Platform::Guest => "guest",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlayerId(u64);

impl PlayerId {
    /// A VK user; None for ids outside the VK range.
    pub fn vk(user_id: u64) -> Option<PlayerId> {
        (user_id > 0 && user_id < PLATFORM_ID_LIMIT).then_some(PlayerId(user_id))
    }

    /// A Telegram user; None for ids outside the Telegram range.
    pub fn telegram(user_id: u64) -> Option<PlayerId> {
        (user_id > 0 && user_id < PLATFORM_ID_LIMIT).then_some(PlayerId(TELEGRAM_BIT | user_id))
    }

    /// A guest from 48 bits of a digest of their browser key.
    pub fn guest(digest: [u8; 6]) -> PlayerId {
        let mut bytes = [0u8; 8];
        bytes[2..].copy_from_slice(&digest);
        PlayerId(GUEST_BIT | u64::from_be_bytes(bytes))
    }

    pub fn platform(self) -> Platform {
        if self.0 & GUEST_BIT != 0 {
            Platform::Guest
        } else if self.0 & TELEGRAM_BIT != 0 {
            Platform::Telegram
        } else {
            Platform::Vk
        }
    }

    /// Guests may join rooms but never host them.
    pub fn is_guest(self) -> bool {
        self.platform() == Platform::Guest
    }

    /// The stored integer (database columns, persisted rooms).
    pub const fn raw(self) -> u64 {
        self.0
    }

    pub const fn from_raw(raw: u64) -> PlayerId {
        PlayerId(raw)
    }
}

impl std::fmt::Display for PlayerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.platform().as_str(), self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platforms_have_disjoint_ranges() {
        let vk = PlayerId::vk(12345).unwrap();
        let tg = PlayerId::telegram(12345).unwrap();
        let guest = PlayerId::guest([1, 2, 3, 4, 5, 6]);
        assert_ne!(vk, tg);
        assert_eq!(vk.platform(), Platform::Vk);
        assert_eq!(tg.platform(), Platform::Telegram);
        assert_eq!(guest.platform(), Platform::Guest);
        assert!(guest.is_guest() && !vk.is_guest() && !tg.is_guest());
        assert_eq!(vk.raw(), 12345, "VK ids are stored unchanged");
        assert_eq!(PlayerId::from_raw(tg.raw()), tg);
        assert_eq!(PlayerId::vk(0), None);
        assert_eq!(PlayerId::vk(1 << 61), None);
        assert_eq!(PlayerId::telegram(1 << 61), None);
    }

    #[test]
    fn serializes_as_a_plain_integer() {
        let id = PlayerId::vk(42).unwrap();
        assert_eq!(serde_json::to_string(&id).unwrap(), "42");
        assert_eq!(serde_json::from_str::<PlayerId>("42").unwrap(), id);
    }
}
