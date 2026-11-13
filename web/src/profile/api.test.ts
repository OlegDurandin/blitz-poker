import { beforeEach, expect, it, vi } from 'vitest';
const state = vi.hoisted(() => ({ id: 1, offline: false, delivered: [] as string[] }));
vi.mock('../pvp/config', () => ({ PVP_URL: 'http://test', devUserId: () => state.id }));
vi.mock('../platform', () => ({ platform: { getUser: async () => ({ name: 'Player' }), authProof: () => null } }));
let data: Map<string, string>;
beforeEach(() => {
  vi.resetModules(); state.id = 1; state.offline = false; state.delivered = []; data = new Map();
  vi.stubGlobal('localStorage', { get length() { return data.size; }, key: (i: number) => [...data.keys()][i] ?? null, getItem: (k: string) => data.get(k) ?? null, setItem: (k: string, v: string) => { data.set(k, v); }, removeItem: (k: string) => { data.delete(k); } });
  vi.stubGlobal('fetch', vi.fn(async (url: string, options: RequestInit) => {
    if (state.offline) throw new Error('offline');
    if (url.endsWith('/api/auth')) return { ok: true, json: async () => ({ session: 'session', account_key: `vk:${state.id}` }) };
    const report = JSON.parse(options.body as string); state.delivered.push(report.match_id);
    return { ok: true, json: async () => ({ xp: 15 }) };
  }));
});
const report = { match_id: 'match001', mode: 'solo' as const, outcome: 'win' as const, final_stack: 700, hands_played: 8, hands_won: 3, opponents: ['Бот Алекс'] };
it('retains failed delivery across reload and removes it only after acknowledgement', async () => {
  state.offline = true;
  let api = await import('./api');
  expect(api.queueLocal(report, 'vk:1')).toBe(true);
  await api.syncLocal().catch(() => {}); expect(data.size).toBe(1);
  vi.resetModules(); api = await import('./api'); state.offline = false;
  await api.syncLocal(); expect(data.size).toBe(0); expect(state.delivered).toEqual(['match001']);
});
it('does not attribute another account’s pending match to the current account', async () => {
  state.offline = true; const api = await import('./api');
  api.queueLocal(report, 'vk:2'); await api.syncLocal();
  expect(data.size).toBe(1); expect(state.delivered).toEqual([]);
  state.id = 2; state.offline = false; await api.syncLocal(); expect(data.size).toBe(0);
});
it('keeps independent match keys and deduplicates enqueues during a retry', async () => {
  state.offline = true; const api = await import('./api');
  api.queueLocal(report, 'vk:1'); api.queueLocal(report, 'vk:1'); api.queueLocal({ ...report, match_id: 'match002' }, 'vk:1');
  await api.syncLocal().catch(() => {}); expect(data.size).toBe(2);
  state.offline = false; await api.syncLocal(); expect(new Set(state.delivered)).toEqual(new Set(['match001', 'match002'])); expect(data.size).toBe(0);
});
it('reports storage failure so the game controller can retain the finished snapshot', async () => {
  const api = await import('./api'); vi.stubGlobal('localStorage', { getItem: () => null, setItem: () => { throw new Error('quota'); } });
  expect(api.queueLocal(report, 'vk:1')).toBe(false);
});

it('rejects an old account response that arrives after switching accounts', async () => {
  const api = await import('./api');
  vi.stubGlobal('fetch', vi.fn(async (url: string) => {
    if (url.endsWith('/api/auth')) return { ok: true, json: async () => ({ session: 'session', account_key: 'vk:1' }) };
    return { ok: true, json: async () => { state.id = 2; return { secret: 'previous-player' }; } };
  }));
  await expect(api.personal('daily-challenges')).rejects.toThrow('Аккаунт изменился');
});
