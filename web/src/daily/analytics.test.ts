import { afterEach, beforeEach, expect, it, vi } from 'vitest';
const h = vi.hoisted(() => ({ transport: null as any, owner: 'vk:1', personal: vi.fn(async (_path: string, _body: unknown) => ({})) }));
vi.mock('../game/analytics', () => ({ setTransport: (t: any) => { h.transport = t; } }));
vi.mock('../profile/api', () => ({ accountKey: () => h.owner, personal: h.personal }));
import { connectDailyAnalytics } from './analytics';
beforeEach(() => { h.owner = 'vk:1'; h.personal.mockClear(); connectDailyAnalytics(); });
it('sends only allowed events to the existing authenticated outbox and strips all params', () => {
  h.transport('daily_challenges_viewed', { token: 'secret', cards: 'As' });
  expect(h.personal).toHaveBeenCalledWith('daily-analytics', { event: 'daily_challenges_viewed', event_id: expect.any(String) });
  expect(h.personal.mock.calls[0][1]).not.toHaveProperty('token');
  h.transport('app_open', { cards: 'As' }); expect(h.personal).toHaveBeenCalledTimes(1);
  h.owner = ''; h.transport('daily_challenges_play_clicked'); expect(h.personal).toHaveBeenCalledTimes(1);
});

afterEach(() => vi.unstubAllGlobals());
it('keeps the play action usable if analytics cannot create an event id', () => {
  vi.stubGlobal('crypto', { getRandomValues: () => { throw new Error('unavailable'); } });
  expect(() => h.transport('daily_challenges_play_clicked')).not.toThrow();
  expect(h.personal).not.toHaveBeenCalled();
});
