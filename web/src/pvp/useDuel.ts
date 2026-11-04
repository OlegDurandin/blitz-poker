// Duel controller for React: owns the connection and the latest snapshot.
// Commands are intents; the screen re-renders only from server snapshots.

import { useCallback, useEffect, useRef, useState } from 'react';
import type { PlayerAction } from '../engine/types';
import { track } from '../game/analytics';
import { getUserInfo } from '../vk';
import { PVP_URL, activeRoom, devUserId, guest } from './config';
import { DuelConnection, type ConnState } from './connection';
import type { RoomPreview, RoomView, ServerMsg } from './types';

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

  const onMessage = useCallback((msg: ServerMsg) => {
    switch (msg.type) {
      case 'room_created':
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
      case 'action_rejected':
        if (msg.action_id === pendingAction.current) pendingAction.current = null;
        setState((s) => ({ ...s, pending: false, error: { code: msg.code, message: msg.message } }));
        break;
      case 'server_error':
        setState((s) => ({ ...s, error: { code: msg.code, message: msg.message } }));
        break;
      default:
        break;
    }
  }, []);

  useEffect(() => {
    if (!active || !PVP_URL) return;
    let stopped = false;
    void getUserInfo().then((user) => {
      if (stopped) return;
      const dev = devUserId();
      const me = user ?? { name: dev ? `Игрок ${dev}` : guest.name() || 'Игрок', photo: null };
      const c = new DuelConnection(PVP_URL!, me, {
        onMessage,
        onState: (s, error) => {
          setState((st) => ({ ...st, conn: s, connError: error ?? null }));
          if (s === 'online') {
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

  const reset = useCallback(() => {
    roomId.current = null;
    activeRoom.set(null);
    pendingAction.current = null;
    setState((s) => ({ ...s, room: null, preview: null, error: null, pending: false, reused: false }));
  }, []);

  return {
    state,
    serverNow: () => conn.current?.serverNow() ?? Date.now(),
    createRoom: () => {
      // A new invite replaces whatever room this player was in.
      if (roomId.current) send({ type: 'leave_room', room_id: roomId.current });
      reset();
      send({ type: 'create_room', request_id: randomId() });
    },
    /** A friend's room by its six-digit code: answers with a preview, like `peek`. */
    findByCode: (code: string) => {
      setState((s) => ({ ...s, preview: null, error: null }));
      if (restarting.current || !conn.current?.send({ type: 'find_room', code })) pendingFind.current = code;
    },
    /** Outside VK: the name the host will see; reconnects if it changed. */
    setGuestName: (name: string) => {
      if (name === guest.name()) return;
      guest.setName(name);
      restarting.current = true;
      setProfileVersion((v) => v + 1);
    },
    peek: (id: string) => {
      setState((s) => ({ ...s, preview: null, error: null }));
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
      if (!roomId.current || !game || state.pending) return;
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
      if (!ok) setState((s) => ({ ...s, pending: false }));
    },
    surrender: () => roomId.current && send({ type: 'surrender', room_id: roomId.current }),
    requestRematch: () => {
      track('duel_rematch_requested', {});
      return roomId.current && send({ type: 'request_rematch', room_id: roomId.current });
    },
    acceptRematch: () => roomId.current && send({ type: 'accept_rematch', room_id: roomId.current }),
    declineRematch: () => roomId.current && send({ type: 'decline_rematch', room_id: roomId.current }),
    reconnect: () => conn.current?.reconnect(),
    clearError: () => setState((s) => ({ ...s, error: null })),
    reset,
  };
}

export type Duel = ReturnType<typeof useDuel>;
