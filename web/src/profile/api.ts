import type { MatchResult } from '../engine/types';
import { platform } from '../platform';
import { devUserId, PVP_URL } from '../pvp/config';

export type Mode = 'all' | 'solo' | 'duel' | 'table';
export type Source = 'all' | 'server' | 'local';
export interface LocalReport { match_id: string; mode: 'solo' | 'table'; outcome: 'win' | 'loss' | 'draw'; final_stack: number; hands_played: number; hands_won: number; opponents: string[] }
export interface MatchRow extends Omit<LocalReport, 'mode'> { event_id: string; mode: Exclude<Mode, 'all'>; source: 'server' | 'local'; finished_ms: number; human_count: number; bot_count: number; xp: number; xp_limited: boolean }
export interface Stats { matches_played: number; wins: number; losses: number; draws: number; win_rate: number | null; hands_played: number; hands_won: number; current_win_streak: number; best_win_streak: number; best_hand: MatchResult['best_hand'] }
export interface MatchPage { items: MatchRow[]; next_cursor: string | null }
export interface Progress { level: number; xp_total: number; xp_current_level: number; xp_to_next_level: number }
export interface Profile { player_id: string; display_name: string; avatar_url: string | null; platform_badge: string; progress: Progress; statistics: Record<Mode, Stats>; achievements: { key: string; unlocked_at: number }[]; recent_matches: MatchPage }

/** Queue ownership follows the launch account, never a name or browser/device id. */
export function accountKey(): string | null {
  const dev = devUserId();
  if (dev !== null) return `vk:${dev}`;
  const proof = platform.authProof?.();
  if (!proof) return null;
  try {
    if ('launch_params' in proof) {
      const id = new URLSearchParams(proof.launch_params.replace(/^\?/, '')).get('vk_user_id');
      return id && /^\d+$/.test(id) ? `vk:${BigInt(id)}` : null;
    }
    const id = JSON.parse(new URLSearchParams(proof.tg_init_data).get('user') ?? '{}').id;
    return typeof id === 'number' && Number.isSafeInteger(id) ? `tg:${(1n << 61n) | BigInt(id)}` : null;
  } catch { return null; }
}

let session: string | null = null;
let sessionOwner: string | null = null;
let signing: Promise<void> | null = null;
async function signIn(): Promise<void> {
  if (!signing) signing = (async () => {
    if (!PVP_URL || !accountKey()) throw new Error('Откройте профиль через VK или Telegram');
    const owner = accountKey();
    const user = await platform.getUser();
    const proof = devUserId() !== null ? { dev_user_id: devUserId() } : platform.authProof();
    const res = await fetch(`${PVP_URL}/api/auth`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ ...proof, name: user?.name, photo: user?.photo }), signal: AbortSignal.timeout(12000) });
    const data = await res.json();
    if (!res.ok || data.account_key !== owner || owner !== accountKey()) throw new Error(data.error?.message ?? 'Не удалось подтвердить аккаунт');
    session = data.session;
    sessionOwner = owner;
  })().finally(() => { signing = null; });
  return signing;
}
export interface DailyAnalyticsReport { event: 'daily_challenges_viewed' | 'daily_challenges_play_clicked'; event_id: string }
export async function personal<T>(path: string, report?: LocalReport | DailyAnalyticsReport): Promise<T> {
  const owner = accountKey();
  if (sessionOwner !== owner) session = null;
  for (let attempt = 0; attempt < 2; attempt++) {
    if (!session) await signIn();
    if (owner !== accountKey() || sessionOwner !== owner) throw new Error('Аккаунт изменился. Откройте приложение заново');
    const res = await fetch(`${PVP_URL}/api/me/${path}`, { method: report ? 'POST' : 'GET', headers: { Authorization: `Bearer ${session}`, ...(report ? { 'Content-Type': 'application/json' } : {}) }, ...(report ? { body: JSON.stringify(report) } : {}), signal: AbortSignal.timeout(12000) });
    if (res.status === 401 && attempt === 0) { session = null; continue; }
    const data = await res.json();
    if (owner !== accountKey()) throw new Error('Аккаунт изменился. Откройте приложение заново');
    if (!res.ok) throw new Error(data.error?.message ?? 'Не удалось загрузить профиль');
    return data as T;
  }
  throw new Error('Не удалось подтвердить аккаунт');
}

interface Pending { owner: string; report: LocalReport }
const QUEUE = 'blitz.v3.progress_pending.';
function pending(): Array<Pending & { key: string }> {
  const rows: Array<Pending & { key: string }> = [];
  for (let i = 0; i < localStorage.length; i++) {
    const key = localStorage.key(i);
    if (key?.startsWith(QUEUE)) {
      const row = JSON.parse(localStorage.getItem(key) ?? 'null') as Pending | null;
      if (row?.owner && row.report) rows.push({ ...row, key });
    }
  }
  return rows;
}
/** Returns false on storage failure: controllers retain the finished snapshot for retry. */
export function queueLocal(report: LocalReport, owner: string | null | undefined): boolean {
  if (!PVP_URL || !owner) return true;
  try {
    const key = `${QUEUE}${encodeURIComponent(owner)}.${report.match_id}`;
    if (!localStorage.getItem(key)) localStorage.setItem(key, JSON.stringify({ owner, report }));
    void syncLocal().catch(() => {});
    return true;
  } catch { return false; }
}
let syncing: Promise<void> | null = null;
export function syncLocal(): Promise<void> {
  if (syncing) return syncing;
  syncing = (async () => {
    const owner = accountKey();
    if (!PVP_URL || !owner) return;
    let failed = false;
    for (const next of pending().filter(r => r.owner === owner)) {
      if (accountKey() !== owner) throw new Error('Аккаунт изменился');
      try {
        await personal<MatchRow>('local-results', next.report);
        localStorage.removeItem(next.key);
      } catch { failed = true; }
    }
    if (failed) throw new Error('Не все локальные результаты отправлены');
  })().finally(() => { syncing = null; });
  return syncing;
}
