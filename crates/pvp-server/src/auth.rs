//! Sessions and one-time WebSocket tickets, issued after VK verification.
//!
//! A session (12 h) lets the client get fresh tickets for reconnects; a
//! ticket (60 s, single use) opens one WebSocket. Both live in memory: after
//! a server restart the client simply signs in again with its launch
//! parameters, which it still has in the page URL.

use crate::room::Profile;
use std::collections::HashMap;

const SESSION_MS: u64 = 12 * 3_600_000;
const TICKET_MS: u64 = 60_000;

#[derive(Default)]
pub struct Sessions {
    sessions: HashMap<String, (Profile, u64)>,
    tickets: HashMap<String, (Profile, u64)>,
}

impl Sessions {
    /// A new session and a first ticket.
    pub fn issue(&mut self, profile: Profile, now: u64) -> (String, String) {
        let session = crate::ids::token();
        self.sessions
            .insert(session.clone(), (profile.clone(), now + SESSION_MS));
        let ticket = self.new_ticket(profile, now);
        (session, ticket)
    }

    fn new_ticket(&mut self, profile: Profile, now: u64) -> String {
        let ticket = crate::ids::token();
        self.tickets
            .insert(ticket.clone(), (profile, now + TICKET_MS));
        ticket
    }

    pub fn ticket_for(&mut self, session: &str, now: u64) -> Option<String> {
        let (profile, expires) = self.sessions.get(session)?.clone();
        (now < expires).then(|| self.new_ticket(profile, now))
    }

    pub fn profile(&self, session: &str, now: u64) -> Option<Profile> {
        let (profile, expires) = self.sessions.get(session)?;
        (now < *expires).then(|| profile.clone())
    }

    /// Consumes a ticket.
    pub fn redeem(&mut self, ticket: &str, now: u64) -> Option<Profile> {
        let (profile, expires) = self.tickets.remove(ticket)?;
        (now < expires).then_some(profile)
    }

    pub fn prune(&mut self, now: u64) {
        self.sessions.retain(|_, (_, e)| now < *e);
        self.tickets.retain(|_, (_, e)| now < *e);
    }
}

/// Display name from the client: trimmed, no control characters, bounded.
pub fn clean_name(name: &str) -> String {
    let cleaned: String = name.chars().filter(|c| !c.is_control()).take(40).collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        "Игрок".to_string()
    } else {
        cleaned.to_string()
    }
}

/// Avatar URL from the client: only https, bounded.
pub fn clean_photo(photo: Option<&str>) -> Option<String> {
    photo
        .filter(|p| {
            p.starts_with("https://") && p.len() <= 400 && !p.contains(['"', '<', '>', ' '])
        })
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tickets_are_single_use_and_expire() {
        let mut s = Sessions::default();
        let p = Profile {
            id: crate::player::PlayerId::from_raw(1),
            name: "A".into(),
            photo: None,
        };
        let (session, ticket) = s.issue(p.clone(), 0);
        assert_eq!(s.redeem(&ticket, 10), Some(p.clone()));
        assert_eq!(s.redeem(&ticket, 10), None, "used up");
        let t2 = s.ticket_for(&session, 100).unwrap();
        assert_eq!(s.redeem(&t2, 100 + TICKET_MS), None, "expired");
        assert!(s.ticket_for(&session, SESSION_MS + 1).is_none());
        assert!(s.ticket_for("nope", 0).is_none());
    }

    #[test]
    fn profile_fields_are_sanitised() {
        assert_eq!(clean_name("  Олег\u{0007}  "), "Олег");
        assert_eq!(clean_name(""), "Игрок");
        assert_eq!(clean_name(&"я".repeat(100)).chars().count(), 40);
        assert_eq!(
            clean_photo(Some("https://sun.userapi.com/a.jpg")).as_deref(),
            Some("https://sun.userapi.com/a.jpg")
        );
        assert_eq!(clean_photo(Some("javascript:alert(1)")), None);
        assert_eq!(clean_photo(Some("https://x/\"><script>")), None);
    }
}
