import { useCallback, useEffect, useRef, useState } from 'react';
import { accountKey, personal } from '../profile/api';
import { PVP_URL } from '../pvp/config';

export interface DailyItem { id: string; template_id: string; title: string; progress: number; target: number; reward_xp: number; awarded_xp: number; status: 'in_progress' | 'completed' | 'rewarded'; reward_reason: 'none' | 'xp_limit' | 'disabled' }
export interface DailyChallenges { day_utc: string; server_now_utc: string; next_reset_at: string; completed_count: number; total_count: number; items: DailyItem[]; rewards_enabled: boolean; xp_budget?: { total_cap: number; bot_cap: number; awarded_today: number; bot_awarded_today: number; remaining_total: number; remaining_bot: number }; completion_bonus: { reward_xp: number; awarded_xp: number; status: 'locked' | 'rewarded'; reward_reason: DailyItem['reward_reason'] } }
export function resetDelay(data: DailyChallenges): number { return Math.max(0, Date.parse(data.next_reset_at) - Date.parse(data.server_now_utc)); }
export function countdown(ms: number): string {
  const minutes = Math.max(0, Math.ceil(ms / 60_000));
  return `${Math.floor(minutes / 60).toString().padStart(2, '0')}:${(minutes % 60).toString().padStart(2, '0')}`;
}
export function completionNotice(previous: DailyChallenges | null, next: DailyChallenges): string | null {
  if (!previous || previous.day_utc !== next.day_utc) return null;
  const done = next.items.filter(i => i.status === 'rewarded' && previous.items.some(p => p.id === i.id && p.status !== 'rewarded'));
  const bonus = previous.completion_bonus.status !== 'rewarded' && next.completion_bonus.status === 'rewarded';
  if (!done.length && !bonus) return null;
  const xp = done.reduce((sum, i) => sum + i.awarded_xp, 0);
  const title = done.length === 1 ? `Задача выполнена: ${done[0].title}` : `${done.length} задачи выполнены`;
  return `${done.length ? `${title} · +${xp} XP` : ''}${bonus ? `${done.length ? '. ' : ''}Бонус: +${next.completion_bonus.awarded_xp} XP` : ''}${done.some(i => i.reward_reason === 'disabled') || (bonus && next.completion_bonus.reward_reason === 'disabled') ? ' · Начисление XP отключено' : done.some(i => i.reward_reason === 'xp_limit') || (bonus && next.completion_bonus.reward_reason === 'xp_limit') ? ' · Лимит XP на сегодня' : ''}`;
}

export function useDailyChallenges() {
  const [data, setData] = useState<DailyChallenges | null>(null);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const [remaining, setRemaining] = useState(0);
  const [notice, setNotice] = useState<string | null>(null);
  const current = useRef<DailyChallenges | null>(null);
  const deadline = useRef(0);
  const busy = useRef(false);
  const retryAt = useRef(0);
  const pending = useRef(false);
  const alive = useRef(false);
  const available = Boolean(PVP_URL && accountKey());
  const refresh = useCallback(async () => {
    if (!available || !alive.current) return;
    if (busy.current) { pending.current = true; return; }
    busy.current = true; retryAt.current = performance.now() + 30_000; setLoading(true);
    try {
      const next = await personal<DailyChallenges>('daily-challenges');
      if (!alive.current) return;
      const message = completionNotice(current.current, next);
      if (message) setNotice(message);
      deadline.current = performance.now() + resetDelay(next);
      setRemaining(resetDelay(next));
      current.current = next; setData(next); setError('');
    } catch (e) { if (alive.current) setError((e as Error).message); }
    finally {
      busy.current = false;
      if (alive.current) {
        setLoading(false);
        if (pending.current) { pending.current = false; void refresh(); }
      }
    }
  }, [available]);
  useEffect(() => {
    alive.current = true;
    void refresh();
    const focus = () => { if (document.visibilityState !== 'hidden') void refresh(); };
    window.addEventListener('focus', focus); window.addEventListener('online', focus);
    document.addEventListener('visibilitychange', focus);
    const timer = window.setInterval(() => {
      if (document.visibilityState === 'hidden') return;
      if (deadline.current) {
        const left = Math.max(0, deadline.current - performance.now());
        setRemaining(left);
        if (left === 0 && performance.now() >= retryAt.current) void refresh(); // Keep old progress until the server returns the new set.
      }
    }, 1000);
    const poll = window.setInterval(focus, 30_000);
    return () => { alive.current = false; window.clearInterval(timer); window.clearInterval(poll); window.removeEventListener('focus', focus); window.removeEventListener('online', focus); document.removeEventListener('visibilitychange', focus); };
  }, [refresh]);
  useEffect(() => { if (!notice) return; const timer = window.setTimeout(() => setNotice(null), 7000); return () => window.clearTimeout(timer); }, [notice]);
  return { data, error, loading, remaining, refresh, notice, available };
}
export type DailyState = ReturnType<typeof useDailyChallenges>;
