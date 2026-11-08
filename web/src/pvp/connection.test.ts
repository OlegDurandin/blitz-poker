import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
vi.mock('../platform', () => ({ platform: { authProof: () => ({ guest_key: 'test' }) } }));
vi.mock('./config', () => ({ BUILD: 'test', ENTRY_HASH: '', devUserId: () => null, guest: { key: () => 'test' } }));
import { DuelConnection } from './connection';

class Socket {
  static OPEN = 1;
  static instances: Socket[] = [];
  readyState = 0;
  onopen?: () => void;
  onclose?: (event: { code: number }) => void;
  onmessage?: (event: { data: string }) => void;
  constructor() { Socket.instances.push(this); }
  close() { this.readyState = 3; this.onclose?.({ code: 1000 }); }
  send() {}
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal('window', { location: { hash: '' }, setTimeout, clearTimeout, setInterval, clearInterval });
  vi.stubGlobal('WebSocket', Socket);
  Socket.instances = [];
});
afterEach(() => { vi.clearAllTimers(); vi.useRealTimers(); vi.unstubAllGlobals(); });
function setup() {
  const state = vi.fn();
  const connection = new DuelConnection('https://example.test', { name: 'Test', photo: null }, { onState: state, onMessage: vi.fn() });
  return { connection, state };
}
const auth = () => Promise.resolve({ ok: true, json: async () => ({ ok: true, session: 'session', ticket: 'ticket' }) });

describe('connection recovery', () => {
  it('bounds hanging HTTP requests and allows a manual retry', async () => {
    const fetchMock = vi.fn((_url, init) => new Promise((_resolve, reject) => {
      init.signal.addEventListener('abort', () => reject(new Error('aborted')));
    }));
    vi.stubGlobal('fetch', fetchMock);
    const { connection, state } = setup();
    connection.start();
    await vi.advanceTimersByTimeAsync(50_000);
    expect(fetchMock).toHaveBeenCalledTimes(3);
    expect(state).toHaveBeenLastCalledWith('failed', expect.any(String));
    connection.reconnect();
    expect(fetchMock).toHaveBeenCalledTimes(4);
    connection.stop();
  });
  it('bounds a stalled WebSocket handshake', async () => {
    vi.stubGlobal('fetch', vi.fn(auth));
    const { connection, state } = setup();
    connection.start();
    await vi.advanceTimersByTimeAsync(50_000);
    expect(Socket.instances).toHaveLength(3);
    expect(state).toHaveBeenLastCalledWith('failed', expect.any(String));
    connection.stop();
  });
  it('clears the connection deadline once online', async () => {
    vi.stubGlobal('fetch', vi.fn(auth));
    const { connection, state } = setup();
    connection.start();
    await vi.advanceTimersByTimeAsync(0);
    Socket.instances[0].readyState = 1;
    Socket.instances[0].onopen?.();
    await vi.advanceTimersByTimeAsync(50_000);
    expect(state).toHaveBeenLastCalledWith('online');
    expect(Socket.instances).toHaveLength(1);
    connection.stop();
  });
  it('ignores an auth response arriving after stop', async () => {
    let resolve!: (value: Awaited<ReturnType<typeof auth>>) => void;
    vi.stubGlobal('fetch', vi.fn(() => new Promise((r) => { resolve = r; })));
    const { connection, state } = setup();
    connection.start();
    connection.stop();
    resolve(await auth());
    await vi.advanceTimersByTimeAsync(50_000);
    expect(Socket.instances).toHaveLength(0);
    expect(state).toHaveBeenCalledTimes(1);
  });
});
