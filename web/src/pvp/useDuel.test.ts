import { beforeEach, describe, expect, it, vi } from 'vitest';

// Minimal hook lifecycle runner: preserves state/refs between renders and lets
// the test run child actions before the parent's connection effect.
const h = vi.hoisted(() => ({ slots: [] as any[], cursor: 0, effects: [] as (() => void | (() => void))[], sockets: [] as any[] }));
vi.mock('react', () => ({
  useRef: (value: unknown) => {
    const i = h.cursor++;
    return h.slots[i] ??= { current: value };
  },
  useState: (value: unknown) => {
    const i = h.cursor++;
    if (!(i in h.slots)) h.slots[i] = value;
    return [h.slots[i], (next: any) => { h.slots[i] = typeof next === 'function' ? next(h.slots[i]) : next; }];
  },
  useCallback: (fn: unknown) => fn,
  useEffect: (fn: () => void | (() => void)) => { h.effects.push(fn); },
}));
vi.mock('../platform', () => ({ platform: { getUser: async () => ({ name: 'Test', photo: null }) } }));
vi.mock('../game/analytics', () => ({ track: vi.fn() }));
vi.mock('./config', () => ({ PVP_URL: 'https://example.test', activeRoom: { set: vi.fn() }, devUserId: () => null, guest: { name: () => '' } }));
vi.mock('./connection', () => ({ DuelConnection: class {
  online = false;
  send = vi.fn(() => this.online);
  constructor(_base: string, _profile: unknown, public handlers: any) { h.sockets.push(this); }
  start() { this.handlers.onState('connecting'); }
  stop() { this.online = false; }
  serverNow() { return 0; }
  connect() { this.online = true; this.handlers.onState('online'); }
} }));
import { useDuel } from './useDuel';
function render(active = true) { h.cursor = 0; h.effects = []; return useDuel(active); }
async function mount() { const cleanup = h.effects[0](); await Promise.resolve(); return cleanup; }
beforeEach(() => { h.slots = []; h.cursor = 0; h.effects = []; h.sockets = []; });

describe('creation across connection lifecycles', () => {
  it.each(['table', 'room'] as const)('queues a %s after returning from a previous online screen', async (kind) => {
    render();
    const cleanup = await mount();
    h.sockets[0].connect();
    cleanup?.();
    const duel = render();
    expect(duel.state.conn).toBe('online'); // stale state seen by the child
    if (kind === 'table') duel.createTable(3); else duel.createRoom();
    await mount();
    const socket = h.sockets[1];
    expect(socket.send).not.toHaveBeenCalled();
    socket.connect();
    expect(socket.send).toHaveBeenCalledWith(expect.objectContaining({ type: `create_${kind}` }));
    const request = socket.send.mock.calls[0][0];
    socket.connect(); // reconnect before acknowledgement: same id, no duplicate room
    expect(socket.send.mock.calls[1][0]).toEqual(request);
    socket.handlers.onMessage({ type: `${kind}_created`, request_id: request.request_id, [`${kind}_id`]: 'new-id', reused: false });
    socket.send.mockClear();
    socket.connect();
    expect(socket.send).not.toHaveBeenCalledWith(expect.objectContaining({ type: `create_${kind}` }));
    expect(socket.send).toHaveBeenCalledWith(expect.objectContaining({ type: kind === 'table' ? 'request_table_snapshot' : 'request_snapshot' }));
  });
  it('does not create a cancelled table when connection becomes available', async () => {
    const duel = render();
    duel.createTable(4);
    duel.reset();
    await mount();
    h.sockets[0].connect();
    expect(h.sockets[0].send).not.toHaveBeenCalled();
  });
});

it.each(['table', 'room'] as const)('blocks same-tick duplicate actions for %s and unlocks on a newer snapshot', async kind => {
  render(); await mount();
  const socket = h.sockets[0]; socket.connect();
  const snapshot = (version: number) => socket.handlers.onMessage({
    type: kind === 'table' ? 'table_snapshot' : 'state_snapshot',
    [kind]: { [`${kind}_id`]: 'qa-id', status: 'playing', game: { hand_id: 'hand-1', state_version: version } },
  });
  socket.handlers.onMessage({ type: `${kind}_created`, request_id: 'qa', [`${kind}_id`]: 'qa-id' });
  snapshot(1);
  let duel = render();
  const act = kind === 'table' ? duel.tableAct : duel.act;
  socket.send.mockClear();
  act({ type: 'check' }); act({ type: 'check' });
  expect(socket.send).toHaveBeenCalledTimes(1);
  snapshot(2); duel = render();
  (kind === 'table' ? duel.tableAct : duel.act)({ type: 'check' });
  expect(socket.send).toHaveBeenCalledTimes(2);
});

it('preserves an open lobby on home navigation, but clears it on explicit leave', async () => {
  const { activeRoom } = await import('./config');
  const duel = render(); await mount();
  h.sockets[0].connect();
  h.sockets[0].handlers.onMessage({ type: 'table_created', request_id: 'qa', table_id: 'lobby-id' });
  vi.mocked(activeRoom.set).mockClear();
  duel.reset(true);
  expect(activeRoom.set).not.toHaveBeenCalled();
  duel.leaveTable();
  expect(activeRoom.set).toHaveBeenCalledWith(null);
});
