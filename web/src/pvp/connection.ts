// The duel socket: signs in with VK launch parameters, opens the WebSocket
// with a one-time ticket, keeps it alive and reconnects with backoff. It only
// sends intents; every state shown comes from the server's snapshots.

import { BUILD, ENTRY_HASH, devUserId, guest, inVk, launchParams } from './config';
import type { ClientMsg, ServerMsg } from './types';

export type ConnState = 'connecting' | 'online' | 'offline' | 'replaced' | 'failed';

export interface Handlers {
  onMessage: (msg: ServerMsg) => void;
  onState: (state: ConnState, error?: string) => void;
}

interface Profile {
  name: string;
  photo: string | null;
}

const HEARTBEAT_MS = 15_000;
const MAX_BACKOFF_MS = 10_000;

export class DuelConnection {
  private ws: WebSocket | null = null;
  private session: string | null = null;
  private heartbeat: number | undefined;
  private retryTimer: number | undefined;
  private attempt = 0;
  private closed = false;
  /** serverNow ≈ Date.now() + offset, for countdowns. */
  offsetMs = 0;

  constructor(
    private readonly base: string,
    private readonly profile: Profile,
    private readonly handlers: Handlers,
  ) {}

  start(): void {
    this.closed = false;
    void this.connect();
  }

  stop(): void {
    this.closed = true;
    window.clearTimeout(this.retryTimer);
    window.clearInterval(this.heartbeat);
    this.ws?.close();
    this.ws = null;
  }

  serverNow(): number {
    return Date.now() + this.offsetMs;
  }

  /** False if the socket is not open (the UI shows "переподключаемся"). */
  send(msg: ClientMsg): boolean {
    if (this.ws?.readyState !== WebSocket.OPEN) return false;
    this.ws.send(JSON.stringify({ v: 1, ...msg }));
    return true;
  }

  private async post(path: string, body: object, bearer?: string): Promise<Response> {
    return fetch(`${this.base}${path}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', ...(bearer ? { Authorization: `Bearer ${bearer}` } : {}) },
      body: JSON.stringify(body),
    });
  }

  private async signIn(): Promise<string> {
    const dev = devUserId();
    const res = await this.post('/api/auth', {
      ...(dev ? { dev_user_id: dev } : inVk() ? { launch_params: launchParams() } : { guest_key: guest.key() }),
      name: this.profile.name,
      photo: this.profile.photo,
      // Diagnostics: how VK opened the app (does the invite fragment arrive?).
      entry: { build: BUILD, hash: ENTRY_HASH.slice(0, 200), hash_now: window.location.hash.slice(0, 200) },
    });
    const body = await res.json().catch(() => null);
    if (!res.ok || !body?.ok) throw new AuthError(body?.error?.message ?? 'Не удалось войти');
    this.session = body.session;
    return body.ticket as string;
  }

  /** A fresh one-time ticket; signs in again if the session is gone. */
  private async ticket(): Promise<string> {
    if (this.session) {
      const res = await this.post('/api/ticket', {}, this.session);
      if (res.ok) return (await res.json()).ticket as string;
      this.session = null;
    }
    return this.signIn();
  }

  private async connect(): Promise<void> {
    if (this.closed) return;
    this.handlers.onState('connecting');
    let ticket: string;
    try {
      ticket = await this.ticket();
    } catch (e) {
      if (e instanceof AuthError) {
        this.handlers.onState('failed', e.message);
        return;
      }
      return this.retry();
    }
    if (this.closed) return;
    const url = `${this.base.replace(/^http/, 'ws')}/ws?ticket=${encodeURIComponent(ticket)}`;
    const ws = new WebSocket(url);
    this.ws = ws;
    ws.onopen = () => {
      this.attempt = 0;
      this.handlers.onState('online');
      window.clearInterval(this.heartbeat);
      this.heartbeat = window.setInterval(() => this.send({ type: 'heartbeat' }), HEARTBEAT_MS);
    };
    ws.onmessage = (e) => {
      let msg: ServerMsg;
      try {
        msg = JSON.parse(e.data as string) as ServerMsg;
      } catch {
        return;
      }
      if ('server_time_ms' in msg) this.offsetMs = msg.server_time_ms - Date.now();
      this.handlers.onMessage(msg);
    };
    ws.onclose = (e) => {
      if (this.ws !== ws) return;
      window.clearInterval(this.heartbeat);
      this.ws = null;
      if (e.code === 4001) {
        // The same account connected elsewhere: don't fight over the seat.
        this.handlers.onState('replaced');
        return;
      }
      this.retry();
    };
  }

  private retry(): void {
    if (this.closed) return;
    this.handlers.onState('offline');
    const delay = Math.min(MAX_BACKOFF_MS, 500 * 2 ** this.attempt);
    this.attempt += 1;
    this.retryTimer = window.setTimeout(() => void this.connect(), delay);
  }

  /** Reconnect now (e.g. "Переподключиться" after the session was replaced). */
  reconnect(): void {
    window.clearTimeout(this.retryTimer);
    this.attempt = 0;
    this.closed = false;
    void this.connect();
  }
}

class AuthError extends Error {}
