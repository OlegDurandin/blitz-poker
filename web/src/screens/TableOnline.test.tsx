import { afterEach, expect, it, vi } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
const h = vi.hoisted(() => ({ effects: [] as Array<() => void | (() => void)> }));
vi.mock('react', async (original) => ({ ...await original<typeof import('react')>(), useEffect: (f: () => void) => { h.effects.push(f); } }));
vi.mock('../platform', () => ({ platform: { getUser: async () => null } }));
vi.mock('../game/analytics', () => ({ track: vi.fn() }));
vi.mock('../pvp/config', () => ({ formatCode: (v: string) => v, PVP_URL: undefined, devUserId: () => null }));
vi.mock('./Duel', () => ({ Avatar: () => null, Center: ({ title, text, children }: any) => <main><h1>{title}</h1><p>{text}</p>{children}</main>, useTicker: () => {} }));
import { TableOnline } from './TableOnline';
import type { Duel } from '../pvp/useDuel';
function controller(conn = 'online', table: unknown = null) {
  return { state: { conn, table, tablePreview: null, error: null }, createSolo: vi.fn(), createTable: vi.fn(), reconnect: vi.fn() } as unknown as Duel;
}
afterEach(() => { h.effects = []; vi.unstubAllGlobals(); });
it('normal Solo creation sends one authoritative intent and never seats a bot through separate client commands', () => {
  vi.stubGlobal('window', { setTimeout: () => 1, clearTimeout: () => {} });
  const duel = controller();
  renderToStaticMarkup(<TableOnline duel={duel} solo="tight" seats={2} inviteId={null} onExit={() => {}} onRules={() => {}} />);
  for (const effect of h.effects) effect();
  expect(duel.createSolo).toHaveBeenCalledOnce(); expect(duel.createSolo).toHaveBeenCalledWith('tight'); expect(duel.createTable).not.toHaveBeenCalled();
});
it('offers explained local fallback on initial connection failure but never converts an existing match', () => {
  const render = (table: unknown) => renderToStaticMarkup(<TableOnline duel={controller('failed', table)} solo="alex" seats={2} inviteId={null} onLocalFallback={() => {}} onExit={() => {}} onRules={() => {}} />);
  expect(render(null)).toContain('Играть локально'); expect(render(null)).toContain('не засчитываются в задачи дня');
  expect(render({ status: 'playing' })).not.toContain('Играть локально');
});
