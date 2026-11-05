//! Authoritative server for private Blitz Poker duels.
//!
//! The poker rules are `poker-core`, the same crate the Solo client runs as
//! WASM. Here they run natively, and only this server ever holds a duel's
//! full state; clients receive per-player views and send intents.

pub mod auth;
pub mod config;
pub mod hub;
pub mod ids;
pub mod player;
pub mod protocol;
pub mod room;
pub mod server;
pub mod store;
pub mod table_room;
pub mod telegram;
pub mod vk;
