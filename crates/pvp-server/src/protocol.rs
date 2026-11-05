//! WebSocket protocol, version 1: JSON text frames.
//!
//! Every client message carries `v` and `type`. State is always sent whole
//! (`state_snapshot`, personalised per player), so a reconnecting client
//! never replays events: it renders the latest snapshot. `server_seq` grows
//! across all server messages; `state_version` grows with every accepted
//! game transition and must be echoed back with `player_action`.

use crate::room::{RoomPreview, RoomView};
use crate::table_room::{TablePreview, TableRoomView};
use poker_core::{Opponent, PlayerAction};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

/// Longest id a client may send (room ids are 22 characters).
pub const MAX_ID_LEN: usize = 64;

#[derive(Debug, Deserialize)]
pub struct Envelope {
    pub v: u32,
    #[serde(flatten)]
    pub msg: ClientMsg,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// Retrying with the same `request_id` returns the same room.
    CreateRoom {
        request_id: String,
    },
    /// A friend dictated the room code: answers like `peek_room` (a preview
    /// with the room id), only for a room still waiting for a guest.
    FindRoom {
        code: String,
    },
    /// Who invited me, and can I still join? Does not join.
    PeekRoom {
        room_id: String,
    },
    JoinRoom {
        room_id: String,
    },
    /// Host: cancel the room. Guest: give the seat back. After a match: leave.
    LeaveRoom {
        room_id: String,
    },
    SetReady {
        room_id: String,
        ready: bool,
    },
    PlayerAction {
        room_id: String,
        match_id: String,
        /// Unique per attempt; a retry with the same id is applied once.
        action_id: String,
        expected_state_version: u64,
        action: PlayerAction,
    },
    RequestSnapshot {
        room_id: String,
    },
    RequestRematch {
        room_id: String,
    },
    AcceptRematch {
        room_id: String,
    },
    DeclineRematch {
        room_id: String,
    },
    /// "Сдаться и выйти": technical defeat now.
    Surrender {
        room_id: String,
    },
    Heartbeat,

    // ---------- tables of 2–4 (people and bots) ----------
    /// Retrying with the same `request_id` returns the same table.
    CreateTable {
        request_id: String,
        seats: usize,
    },
    JoinTable {
        table_id: String,
    },
    /// Lobby: free the seat (the host closes the table). In a match: fold
    /// and sit out. After it: leave the line-up.
    LeaveTable {
        table_id: String,
    },
    /// Host, lobby: seat a bot in the first free seat.
    TableAddBot {
        table_id: String,
        opponent: Opponent,
    },
    TableRemoveBot {
        table_id: String,
        seat: usize,
    },
    StartTable {
        table_id: String,
    },
    TableAction {
        table_id: String,
        match_id: String,
        action_id: String,
        expected_state_version: u64,
        action: PlayerAction,
    },
    /// "Ещё раз": the same line-up again once every person agrees.
    TableAgain {
        table_id: String,
    },
    RequestTableSnapshot {
        table_id: String,
    },
}

impl ClientMsg {
    /// Ids the client sent, for length checks.
    pub fn ids(&self) -> Vec<&str> {
        match self {
            ClientMsg::CreateRoom { request_id } => vec![request_id],
            ClientMsg::FindRoom { code } => vec![code],
            ClientMsg::PlayerAction {
                room_id,
                match_id,
                action_id,
                ..
            } => vec![room_id, match_id, action_id],
            ClientMsg::PeekRoom { room_id }
            | ClientMsg::JoinRoom { room_id }
            | ClientMsg::LeaveRoom { room_id }
            | ClientMsg::SetReady { room_id, .. }
            | ClientMsg::RequestSnapshot { room_id }
            | ClientMsg::RequestRematch { room_id }
            | ClientMsg::AcceptRematch { room_id }
            | ClientMsg::DeclineRematch { room_id }
            | ClientMsg::Surrender { room_id } => vec![room_id],
            ClientMsg::Heartbeat => vec![],
            ClientMsg::CreateTable { request_id, .. } => vec![request_id],
            ClientMsg::TableAction {
                table_id,
                match_id,
                action_id,
                ..
            } => vec![table_id, match_id, action_id],
            ClientMsg::JoinTable { table_id }
            | ClientMsg::LeaveTable { table_id }
            | ClientMsg::TableAddBot { table_id, .. }
            | ClientMsg::TableRemoveBot { table_id, .. }
            | ClientMsg::StartTable { table_id }
            | ClientMsg::TableAgain { table_id }
            | ClientMsg::RequestTableSnapshot { table_id } => vec![table_id],
        }
    }

    /// Messages about a table rather than a duel room.
    pub fn is_table(&self) -> bool {
        matches!(
            self,
            ClientMsg::CreateTable { .. }
                | ClientMsg::JoinTable { .. }
                | ClientMsg::LeaveTable { .. }
                | ClientMsg::TableAddBot { .. }
                | ClientMsg::TableRemoveBot { .. }
                | ClientMsg::StartTable { .. }
                | ClientMsg::TableAction { .. }
                | ClientMsg::TableAgain { .. }
                | ClientMsg::RequestTableSnapshot { .. }
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicUser {
    pub name: String,
    pub photo: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Welcome {
        user: PublicUser,
        server_time_ms: u64,
    },
    RoomCreated {
        room_id: String,
        request_id: String,
        /// The host's room that was still open ("Твоя комната ещё открыта"),
        /// not a new one.
        reused: bool,
    },
    RoomPreview {
        preview: RoomPreview,
    },
    StateSnapshot {
        server_time_ms: u64,
        room: Box<RoomView>,
    },
    ActionAccepted {
        action_id: String,
        state_version: u64,
        duplicate: bool,
    },
    ActionRejected {
        action_id: String,
        code: &'static str,
        message: &'static str,
    },
    ServerError {
        code: &'static str,
        message: &'static str,
        room_id: Option<String>,
    },
    Pong {
        server_time_ms: u64,
    },
    TableCreated {
        table_id: String,
        request_id: String,
        /// The host's lobby that was still open, not a new one.
        reused: bool,
    },
    /// Answers `find_room` / `peek_room` when the code or link is a table.
    TablePreview {
        preview: TablePreview,
    },
    TableSnapshot {
        server_time_ms: u64,
        table: Box<TableRoomView>,
    },
}

/// What actually goes on the wire: the message plus `v` and `server_seq`.
#[derive(Serialize)]
pub struct Outbound<'a> {
    pub v: u32,
    pub server_seq: u64,
    #[serde(flatten)]
    pub msg: &'a ServerMsg,
}
