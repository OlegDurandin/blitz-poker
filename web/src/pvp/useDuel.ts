// Duel controller for React: owns the connection and the latest snapshot.
// Commands are intents; the screen re-renders only from server snapshots.

import { useCallback, useEffect, useRef, useState } from 'react';
import type { PlayerAction } from '../engine/types';
import { track } from '../game/analytics';
import { platform } from '../platform';
import { PVP_URL, activeRoom, devUserId, guest } from './config';
import { DuelConnection, type ConnState } from './connection';
import type { Opponent } from '../engine/types';
import type { ClientMsg, RoomPreview, RoomView, ServerMsg, TablePreview, TableRoomView } from './types';

export interface DuelState {
  conn: ConnState;
  connError: string | null;
  room: RoomView | null;
  preview: RoomPreview | null;
  /** Last error for the current screen (code + Russian text). */
  error: { code: string; message: string } | null;
  /** An action sent and not yet confirmed: buttons stay locked. */
  pending: boolean;
  /** "Играть с другом" brought the host back to a room that was still open. */
  reused: boolean;
  /** A table of 2–4 this screen created or joined. */
  table: TableRoomView | null;
  /** A link or code led to a table rather than a duel room. */
  tablePreview: TablePreview | null;
}

function randomId(): string {
  const b = crypto.getRandomValues(new Uint8Array(12));
  return Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('');
}

export function useDuel(active: boolean) {
  const [state, setState] = useState<DuelState>({
    conn: 'connecting',
    connError: null,
    room: null,
    preview: null,
    error: null,
    pending: false,
    reused: false,
    table: null,
    tablePreview: null,
  });
  const conn = useRef<DuelConnection | null>(null);
  const roomId = useRef<string | null>(null);
  /** Join automatically once online (invite link or "Присоединиться"). */
  const pendingJoin = useRef<string | null>(null);
  const pendingAction = useRef<string | null>(null);
  /** A code to look up once (re)connected, e.g. after a guest renamed. */
  const pendingFind = useRef<string | null>(null);
  /** Bumped when a guest changes their name: reconnects with it. */
  const [profileVersion, setProfileVersion] = useState(0);
  const restarting = useRef(false);
  const tableId = useRef<string | null>(null);
  const pendingJoinTable = useRef<string | null>(null);
  // Keep the same request id until acknowledged, including across reconnects.
  const pendingCreate = useRef<Extract<ClientMsg, { type: 'create_room' | 'create_table' }> | null>(null);

  const onMessage = useCallback((msg: ServerMsg) => {
    switch (msg.type) {
      case 'room_created':
        if (pendingCreate.current?.request_id === msg.request_id) pendingCreate.current = null;
        roomId.current = msg.room_id;
        activeRoom.set(msg.room_id);
        setState((s) => ({ ...s, reused: Boolean(msg.reused) }));
        track('duel_room_created', { reused: Boolean(msg.reused) });
        break;
      case 'room_preview':
        setState((s) => ({ ...s, preview: msg.preview, error: null }));
        break;
      case 'state_snapshot':
        // The server also pushes other rooms this user is still in (e.g. an
        // old finished one); only the room this screen joined matters.
        if (msg.room.room_id !== roomId.current) break;
        if (msg.room.status === 'closed') activeRoom.set(null);
        setState((s) => {
          // A newer state settles any action in flight.
          const settled =
            s.pending && (msg.room.game?.state_version ?? 0) > (s.room?.game?.state_version ?? 0);
          if (settled) pendingAction.current = null;
          return { ...s, room: msg.room, pending: settled ? false : s.pending, error: null };
        });
        break;
      case 'table_created':
        if (pendingCreate.current?.request_id === msg.request_id) pendingCreate.current = null;
        tableId.current = msg.table_id;
        activeRoom.set(msg.table_id);
        setState((s) => ({ ...s, reused: Boolean(msg.reused) }));
        break;
      case 'table_preview':
        setState((s) => ({ ...s, tablePreview: msg.preview, error: null }));
        break;
      case 'table_snapshot':
        if (msg.table.table_id !== tableId.current) break;
        if (msg.table.status === 'closed') activeRoom.set(null);
        setState((s) => {
          const settled =
            s.pending && (msg.table.game?.state_version ?? 0) > (s.table?.game?.state_version ?? 0);
          if (settled) pendingAction.current = null;
          return { ...s, table: msg.table, pending: settled ? false : s.pending, error: null };
        });
        break;
      case 'action_rejected':
        if (msg.action_id === pendingAction.current) pendingAction.current = null;
        setState((s) => ({ ...s, pending: false, error: { code: msg.code, message: msg.message } }));
        break;
      case 'server_error':
        pendingCreate.current = null;
        setState((s) => ({ ...s, error: { code: msg.code, message: msg.message } }));
        break;
      default:
        break;
    }
  }, []);

  useEffect(() => {
    if (!active || !PVP_URL) return;
    setState((s) => ({ ...s, conn: 'connecting', connError: null }));
    let stopped = false;
    void platform.getUser().then((user) => {
      if (stopped) return;
      const dev = devUserId();
      const me = user ?? { name: dev ? `Игрок ${dev}` : guest.name() || 'Игрок', photo: null };
      const c = new DuelConnection(PVP_URL!, me, {
        onMessage,
        onState: (s, error) => {
          setState((st) => ({ ...st, conn: s, connError: error ?? null }));
          if (s === 'online') {
            if (pendingCreate.current) c.send(pendingCreate.current);
            if (pendingFind.current) {
              c.send({ type: 'find_room', code: pendingFind.current });
              pendingFind.current = null;
            }
            if (pendingJoin.current) {
              c.send({ type: 'join_room', room_id: pendingJoin.current });
              pendingJoin.current = null;
            } else if (roomId.current) {
              c.send({ type: 'request_snapshot', room_id: roomId.current });
            }
            if (pendingJoinTable.current) {
              c.send({ type: 'join_table', table_id: pendingJoinTable.current });
              pendingJoinTable.current = null;
            } else if (tableId.current) {
              c.send({ type: 'request_table_snapshot', table_id: tableId.current });
            }
          }
        },
      });
      conn.current = c;
      restarting.current = false;
      c.start();
    });
    return () => {
      stopped = true;
      conn.current?.stop();
      conn.current = null;
    };
  }, [active, onMessage, profileVersion]);

  const send = (msg: Parameters<DuelConnection['send']>[0]) => {
    const ok = conn.current?.send(msg) ?? false;
    if (!ok) setState((s) => ({ ...s, error: { code: 'offline', message: 'Нет соединения, переподключаемся…' } }));
    return ok;
  };

  const reset = useCallback((keepActiveRoom = false) => {
    pendingCreate.current = null;
    pendingJoin.current = null;
    pendingJoinTable.current = null;
    pendingFind.current = null;
    roomId.current = null;
    tableId.current = null;
    if (!keepActiveRoom) activeRoom.set(null);
    pendingAction.current = null;
    setState((s) => ({
      ...s,
      room: null,
      preview: null,
      error: null,
      pending: false,
      reused: false,
      table: null,
      tablePreview: null,
    }));
  }, []);

  const tableSend = (make: (id: string) => Parameters<DuelConnection['send']>[0]) =>
    tableId.current ? send(make(tableId.current)) : false;

  return {
    state,
    serverNow: () => conn.current?.serverNow() ?? Date.now(),
    createRoom: () => {
      // A new invite replaces whatever room this player was in.
      if (roomId.current) send({ type: 'leave_room', room_id: roomId.current });
      reset();
      pendingCreate.current = { type: 'create_room', request_id: randomId() };
      conn.current?.send(pendingCreate.current);
    },
    /** A friend's room by its six-digit code: answers with a preview, like `peek`. */
    findByCode: (code: string) => {
      setState((s) => ({ ...s, preview: null, tablePreview: null, error: null }));
      if (restarting.current || !conn.current?.send({ type: 'find_room', code })) pendingFind.current = code;
    },
    /** Outside VK: the name the host will see; reconnects if it changed. */
    setGuestName: (name: string) => {
      if (name === guest.name()) return;
      guest.setName(name);
      restarting.current = true;
      // The old socket is torn down without a state callback: say so here, so
      // nothing is sent until the new one is online.
      setState((s) => ({ ...s, conn: 'connecting' }));
      setProfileVersion((v) => v + 1);
    },
    peek: (id: string) => {
      setState((s) => ({ ...s, preview: null, tablePreview: null, error: null }));
      send({ type: 'peek_room', room_id: id });
    },
    join: (id: string) => {
      roomId.current = id;
      activeRoom.set(id);
      if (!send({ type: 'join_room', room_id: id })) pendingJoin.current = id;
      track('duel_joined', {});
    },
    leave: () => {
      if (roomId.current) send({ type: 'leave_room', room_id: roomId.current });
      reset();
    },
    ready: (ready: boolean) => roomId.current && send({ type: 'set_ready', room_id: roomId.current, ready }),
    act: (action: PlayerAction) => {
      const game = state.room?.game;
      if (!roomId.current || !game || state.pending || pendingAction.current) return;
      const actionId = randomId();
      pendingAction.current = actionId;
      setState((s) => ({ ...s, pending: true }));
      const ok = send({
        type: 'player_action',
        room_id: roomId.current,
        match_id: game.match_id,
        action_id: actionId,
        expected_state_version: game.state_version,
        action,
      });
      if (!ok) { pendingAction.current = null; setState((s) => ({ ...s, pending: false })); }
    },
    surrender: () => roomId.current && send({ type: 'surrender', room_id: roomId.current }),
    requestRematch: () => {
      track('duel_rematch_requested', {});
      return roomId.current && send({ type: 'request_rematch', room_id: roomId.current });
    },
    acceptRematch: () => roomId.current && send({ type: 'accept_rematch', room_id: roomId.current }),
    declineRematch: () => roomId.current && send({ type: 'decline_rematch', room_id: roomId.current }),
    reconnect: () => conn.current?.reconnect(),

    // ---------- tables ----------
    createTable: (seats: number) => {
      if (tableId.current) send({ type: 'leave_table', table_id: tableId.current });
      reset();
      pendingCreate.current = { type: 'create_table', request_id: randomId(), seats };
      conn.current?.send(pendingCreate.current);
    },
    joinTable: (id: string) => {
      tableId.current = id;
      activeRoom.set(id);
      if (!send({ type: 'join_table', table_id: id })) pendingJoinTable.current = id;
      track('duel_joined', { table: true });
    },
    leaveTable: () => {
      tableSend((table_id) => ({ type: 'leave_table', table_id }));
      reset();
    },
    /** Mid-match: fold out but keep watching the standings. */
    quitTable: () => tableSend((table_id) => ({ type: 'leave_table', table_id })),
    addBot: (opponent: Opponent) => tableSend((table_id) => ({ type: 'table_add_bot', table_id, opponent })),
    removeBot: (seat: number) => tableSend((table_id) => ({ type: 'table_remove_bot', table_id, seat })),
    startTable: () => tableSend((table_id) => ({ type: 'start_table', table_id })),
    tableAgain: () => tableSend((table_id) => ({ type: 'table_again', table_id })),
    tableAct: (action: PlayerAction) => {
      const game = state.table?.game;
      if (!tableId.current || !game || state.pending || pendingAction.current) return;
      const actionId = randomId();
      pendingAction.current = actionId;
      setState((s) => ({ ...s, pending: true }));
      const ok = send({
        type: 'table_action',
        table_id: tableId.current,
        match_id: game.match_id,
        action_id: actionId,
        expected_state_version: game.state_version,
        action,
      });
      if (!ok) { pendingAction.current = null; setState((s) => ({ ...s, pending: false })); }
    },
    clearError: () => setState((s) => ({ ...s, error: null })),
    reset,
  };
}

export type Duel = ReturnType<typeof useDuel>;
