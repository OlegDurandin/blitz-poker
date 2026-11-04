// Mirrors crates/pvp-server/src/room.rs and protocol.rs (protocol v1).

import type { PlayerAction, PlayerView } from '../engine/types';

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
  | { type: 'heartbeat' };

export type ServerMsg = { v: number; server_seq: number } & (
  | { type: 'welcome'; user: { name: string; photo: string | null }; server_time_ms: number }
  | { type: 'room_created'; room_id: string; request_id: string; reused: boolean }
  | { type: 'room_preview'; preview: RoomPreview }
  | { type: 'state_snapshot'; server_time_ms: number; room: RoomView }
  | { type: 'action_accepted'; action_id: string; state_version: number; duplicate: boolean }
  | { type: 'action_rejected'; action_id: string; code: string; message: string }
  | { type: 'server_error'; code: string; message: string; room_id: string | null }
  | { type: 'pong'; server_time_ms: number }
);
