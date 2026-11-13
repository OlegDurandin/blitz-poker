import { describe, expect, it, vi } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { DailyCard, DailyScreen } from './DailyChallenges';
import { completionNotice, countdown, resetDelay, type DailyChallenges, type DailyState } from './useDailyChallenges';
vi.mock('../profile/api', () => ({ accountKey: () => null, personal: vi.fn() }));
vi.mock('../pvp/config', () => ({ PVP_URL: undefined }));
function data(): DailyChallenges {
  return { day_utc: '2026-11-13', server_now_utc: '2026-11-13T23:59:00Z', next_reset_at: '2026-11-14T00:00:00Z', completed_count: 0, total_count: 3, rewards_enabled: true, items: [
    { id: '1', template_id: 'play_matches_2', title: 'Сыграй 2 партии', progress: 0, target: 2, reward_xp: 20, awarded_xp: 0, status: 'in_progress', reward_reason: 'none' },
    { id: '2', template_id: 'play_hands_6', title: 'Сыграй 6 раздач', progress: 0, target: 6, reward_xp: 20, awarded_xp: 0, status: 'in_progress', reward_reason: 'none' },
    { id: '3', template_id: 'win_hands_2', title: 'Выиграй 2 раздачи', progress: 0, target: 2, reward_xp: 30, awarded_xp: 0, status: 'in_progress', reward_reason: 'none' },
  ], completion_bonus: { reward_xp: 30, awarded_xp: 0, status: 'locked', reward_reason: 'none' } };
}
function state(d: DailyChallenges | null): DailyState { return { data: d, loading: false, error: '', available: true, remaining: 60_000, refresh: async () => {}, notice: null }; }
describe('daily challenges', () => {
  it('uses server reset time and rounds the visible countdown up', () => {
    expect(resetDelay(data())).toBe(60_000);
    expect(countdown(60_000)).toBe('00:01'); expect(countdown(-1)).toBe('00:00'); expect(countdown(86_400_000)).toBe('24:00');
  });
  it('announces actual XP once, groups completions, and suppresses reload/day rollover celebrations', () => {
    const before = data(); const after = data();
    after.items[0] = { ...after.items[0], progress: 2, status: 'rewarded', awarded_xp: 5, reward_reason: 'xp_limit' };
    expect(completionNotice(null, after)).toBeNull();
    expect(completionNotice(before, after)).toContain('+5 XP');
    expect(completionNotice(before, after)).toContain('Лимит XP');
    expect(completionNotice(after, after)).toBeNull();
    after.items[1] = { ...after.items[1], progress: 6, status: 'rewarded', awarded_xp: 20 };
    after.completion_bonus = { ...after.completion_bonus, status: 'rewarded', awarded_xp: 0 };
    expect(completionNotice(before, after)).toContain('2 задачи выполнены · +25 XP. Бонус: +0 XP');
    after.day_utc = '2026-11-14'; expect(completionNotice(before, after)).toBeNull();
  });
  it('renders all tasks with accessible progress, the ordinary play action and local-game caveat', () => {
    const html = renderToStaticMarkup(<DailyScreen state={state(data())} onClose={() => {}} onPlay={() => {}} />);
    expect(html.match(/<article/g)).toHaveLength(3); expect(html.match(/<progress/g)).toHaveLength(3);
    expect(html).toContain('Локальная'); expect(html).toContain('Играть'); expect(html).not.toContain('Забрать');
  });
  it('keeps guest, error and capped states informative', () => {
    const guest = { ...state(null), available: false };
    expect(renderToStaticMarkup(<DailyCard state={guest} onOpen={() => {}} />)).toContain('VK или Telegram');
    expect(renderToStaticMarkup(<DailyScreen state={{ ...guest, error: 'Нет сети' }} onClose={() => {}} onPlay={() => {}} />)).toContain('Повторить');
    const d = data(); d.items[0] = { ...d.items[0], progress: 2, status: 'rewarded', awarded_xp: 0, reward_reason: 'xp_limit' };
    const html = renderToStaticMarkup(<DailyScreen state={state(d)} onClose={() => {}} onPlay={() => {}} />);
    expect(html).toContain('Лимит XP на сегодня'); expect(html).toContain('+0 XP');
  });
});

it('shows the shared XP budget and explains disabled or capped bonus notifications', () => {
  const d = data(); d.xp_budget = { total_cap: 300, bot_cap: 100, awarded_today: 100, bot_awarded_today: 100, remaining_total: 200, remaining_bot: 0 };
  const html = renderToStaticMarkup(<DailyScreen state={state(d)} onClose={() => {}} onPlay={() => {}} />);
  expect(html).toContain('XP сегодня: 100/300'); expect(html).toContain('с ботами: 100/100'); expect(html).toContain('Доступно с ботами: 0 XP');
  const next = data(); next.completion_bonus = { reward_xp: 30, awarded_xp: 0, status: 'rewarded', reward_reason: 'disabled' };
  expect(completionNotice(data(), next)).toContain('Начисление XP отключено');
  next.completion_bonus.reward_reason = 'xp_limit'; expect(completionNotice(data(), next)).toContain('Лимит XP на сегодня');
});
