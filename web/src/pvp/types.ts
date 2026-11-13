// Mirrors crates/pvp-server/src/room.rs and protocol.rs (protocol v1).

import type { Opponent, PlayerAction, PlayerView, TableView } from '../engine/types';

export type RoomStatus = 'waiting' | 'ready' | 'playing' | 'finished' | 'closed';
export type CloseReason = 'cancelled_by_host' | 'expired' | 'left' | 'aborted';

export interface SlotView {
  name: string;
  photo: string | null;
  ready: boolean;
  connected: boolean;
  /** While a dropped player may still come back. */
  reconnect_deadline_ms: number | null;
}

export interface Clock {
  my_turn: boolean;
  deadline_ms: number;
  turn_ms: number;
  bank_ms_me: number;
  bank_ms_opponent: number;
}

export interface GameView {
  match_id: string;
  state_version: number;
  /** The engine's view for me; I am always seat "human". */
  view: PlayerView;
  clock: Clock | null;
  next_hand_at_ms: number | null;
}

export interface RoomView {
  room_id: string;
  /** Six digits to dictate instead of the link. */
  code: string;
  status: RoomStatus;
  close_reason: CloseReason | null;
  left_by_me: boolean | null;
  you_are_host: boolean;
  expires_at_ms: number | null;
  me: SlotView;
  opponent: SlotView | null;
  game: GameView | null;
  rematch: { requested_by_me: boolean; requested_by_opponent: boolean; declined_by_opponent: boolean };
  score: { me: number; opponent: number; draws: number };
}

export interface RoomPreview {
  room_id: string;
  host_name: string;
  host_photo: string | null;
  joinable: boolean;
  already_member: boolean;
  status: RoomStatus;
}

// ---------- tables of 2–4: friends and bots (table_room.rs) ----------

export type TableStatus = 'lobby' | 'playing' | 'finished' | 'closed';
export type TableCloseReason = 'cancelled_by_host' | 'expired' | 'empty';

export type TableSeatView =
  | { type: 'empty' }
  | { type: 'bot'; opponent: Opponent }
  | {
      type: 'player';
      name: string;
      photo: string | null;
      connected: boolean;
      reconnect_deadline_ms: number | null;
      is_host: boolean;
      is_me: boolean;
      /** Left the table: folded out of this match. */
      gone: boolean;
      /** Voted for another match. */
      again: boolean;
    };

export interface TableClock {
  seat: number;
  my_turn: boolean;
  /** A person's deadline; null while a bot thinks. */
  deadline_ms: number | null;
  turn_ms: number;
  bank_ms_me: number;
}

export interface TableRoomView {
  table_id: string;
  code: string;
  status: TableStatus;
  close_reason: TableCloseReason | null;
  you_are_host: boolean;
  my_seat: number;
  expires_at_ms: number | null;
  seats: TableSeatView[];
  game: {
    match_id: string;
    state_version: number;
    /** The engine's view for my seat (`view.me`). */
    view: TableView;
    clock: TableClock | null;
    next_hand_at_ms: number | null;
  } | null;
  again: { votes: number; needed: number; mine: boolean };
}

export interface TablePreview {
  table_id: string;
  host_name: string;
  host_photo: string | null;
  joinable: boolean;
  already_member: boolean;
  status: TableStatus;
  seats: number;
  taken: number;
}

export type ClientMsg =
  | { type: 'create_room'; request_id: string }
  | { type: 'find_room'; code: string }
  | { type: 'peek_room'; room_id: string }
  | { type: 'join_room'; room_id: string }
  | { type: 'leave_room'; room_id: string }
  | { type: 'set_ready'; room_id: string; ready: boolean }
  | {
      type: 'player_action';
      room_id: string;
      match_id: string;
      action_id: string;
      expected_state_version: number;
      action: PlayerAction;
    }
  | { type: 'request_snapshot'; room_id: string }
  | { type: 'request_rematch'; room_id: string }
  | { type: 'accept_rematch'; room_id: string }
  | { type: 'decline_rematch'; room_id: string }
  | { type: 'surrender'; room_id: string }
  | { type: 'heartbeat' }
  | { type: 'create_solo'; request_id: string; opponent: Opponent }
  | { type: 'create_table'; request_id: string; seats: number }
  | { type: 'join_table'; table_id: string }
  | { type: 'leave_table'; table_id: string }
  | { type: 'table_add_bot'; table_id: string; opponent: Opponent }
  | { type: 'table_remove_bot'; table_id: string; seat: number }
  | { type: 'start_table'; table_id: string }
  | {
      type: 'table_action';
      table_id: string;
      match_id: string;
      action_id: string;
      expected_state_version: number;
      action: PlayerAction;
    }
  | { type: 'table_again'; table_id: string }
  | { type: 'request_table_snapshot'; table_id: string };

export type ServerMsg = { v: number; server_seq: number } & (
  | { type: 'welcome'; user: { name: string; photo: string | null }; server_time_ms: number }
  | { type: 'room_created'; room_id: string; request_id: string; reused: boolean }
  | { type: 'room_preview'; preview: RoomPreview }
  | { type: 'state_snapshot'; server_time_ms: number; room: RoomView }
  | { type: 'action_accepted'; action_id: string; state_version: number; duplicate: boolean }
  | { type: 'action_rejected'; action_id: string; code: string; message: string }
  | { type: 'server_error'; code: string; message: string; room_id: string | null }
  | { type: 'pong'; server_time_ms: number }
  | { type: 'table_created'; table_id: string; request_id: string; reused: boolean }
  | { type: 'table_preview'; preview: TablePreview }
  | { type: 'table_snapshot'; server_time_ms: number; table: TableRoomView }
);
