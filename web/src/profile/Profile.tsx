import { useEffect, useState } from 'react';
import { PlayingCard } from '../components/PlayingCard';
import { describeHand } from '../game/format';
import { Dialog } from '../components/Dialog';
import { track } from '../game/analytics';
import { personal, syncLocal, type MatchPage, type MatchRow, type Mode, type Profile, type Source } from './api';

const MODES: Record<Mode, string> = { all: 'Все', solo: 'Соло', duel: 'Дуэли', table: 'Столы' };
const ACHIEVEMENTS = { first_match: 'Первый матч', first_win: 'Первая победа', ten_matches: '10 матчей', three_win_streak: 'Три победы подряд' };
export function ProfileScreen({ onClose, onStart, onDaily, dailyCount }: { onClose: () => void; onStart: () => void; onDaily?: () => void; dailyCount?: string }) {
  const [profile, setProfile] = useState<Profile | null>(null);
  const [mode, setMode] = useState<Mode>('all');
  const [source, setSource] = useState<Source>('all');
  const [page, setPage] = useState<MatchPage | null>(null);
  const [error, setError] = useState('');
  const [syncError, setSyncError] = useState('');
  const [busy, setBusy] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [selected, setSelected] = useState<MatchRow | null>(null);
  useEffect(() => { track('profile_opened'); }, []);
  useEffect(() => {
    let live = true;
    setBusy(true); setError(''); setPage(null); setSelected(null);
    void (async () => {
      try {
        try { await syncLocal(); if (live) setSyncError(''); }
        catch { if (live) setSyncError('Локальные результаты ещё не отправлены. Повторим при подключении.'); }
        const [p, matches] = await Promise.all([
          personal<Profile>(`profile?source=${source}`), personal<MatchPage>(`matches?source=${source}&mode=${mode}`),
        ]);
        if (live) { setProfile(p); setPage(matches); }
      } catch (e) { if (live) setError((e as Error).message); }
      finally { if (live) setBusy(false); }
    })();
    return () => { live = false; };
  }, [source, mode, attempt]);
  const stats = profile?.statistics[mode];
  const loadMore = async () => {
    if (!page?.next_cursor || busy) return;
    setBusy(true); setError('');
    try {
      const next = await personal<MatchPage>(`matches?source=${source}&mode=${mode}&cursor=${encodeURIComponent(page.next_cursor)}`);
      setPage({ items: [...page.items, ...next.items], next_cursor: next.next_cursor });
    } catch (e) { setError((e as Error).message); }
    finally { setBusy(false); }
  };
  return <Dialog title="Мой профиль" onClose={onClose}>
    <div className="player-profile">
      <div className="profile-top"><h1>Мой профиль</h1><button className="btn btn--ghost" onClick={onClose} aria-label="Закрыть профиль">✕</button></div>
      {profile && <>
        <header className="profile-identity">{profile.avatar_url && <img src={profile.avatar_url} alt="" />}<div><strong>{profile.display_name}</strong><span>{profile.platform_badge} · Уровень {profile.progress.level}</span></div></header>
        <XpProgress progress={profile.progress} />
        {onDaily && <button className="btn btn--secondary btn--block" onClick={onDaily}>Задачи дня {dailyCount ?? ""} ›</button>}
        <label className="profile-source">Результаты<select value={source} disabled={busy} onChange={e => setSource(e.target.value as Source)}><option value="all">Все игры</option><option value="server">Серверные игры</option><option value="local">Локальные игры</option></select></label>
        <p className="profile-note">Локальные итоги передаёт приложение. Сетевые результаты подтверждает сервер. XP — личный прогресс.</p>
        <div className="profile-tabs" role="group" aria-label="Режим статистики">{(Object.keys(MODES) as Mode[]).map(m => <button key={m} disabled={busy} aria-pressed={mode === m} onClick={() => { setMode(m); track('profile_mode_selected', { mode: m }); }}>{MODES[m]}</button>)}</div>
        {stats && <div className="profile-metrics"><div><strong>{stats.matches_played}</strong><span>Матчи</span></div><div><strong>{stats.win_rate === null ? '—' : `${Math.round(stats.win_rate)}%`}</strong><span>Победы, %</span></div><div><strong>{stats.best_win_streak}</strong><span>Лучшая серия</span></div></div>}
        {stats && <p className="profile-note">Победы: {stats.wins} · Поражения: {stats.losses} · Ничьи: {stats.draws}<br />Раздачи: {stats.hands_played} · Выиграно: {stats.hands_won}</p>}
      </>}
      {stats?.best_hand && <div className="profile-detail"><span>Лучшая комбинация: {describeHand({ category: stats.best_hand.category, best: stats.best_hand.cards })}</span><div className="result__best-cards">{stats.best_hand.cards.map(c => <PlayingCard key={c} card={c} size="xs" />)}</div></div>}
      {busy && <p role="status">Загружаем профиль…</p>}
      {error && <p role="alert">{error}</p>}
      {(error || syncError) && <button className="btn btn--secondary btn--block" onClick={() => setAttempt(n => n + 1)}>Повторить</button>}
      {syncError && <p role="status">{syncError}</p>}
      {page && <section aria-label="История матчей"><h2>Последние матчи</h2>
        {page.items.length === 0 && <><p>Пока нет сыгранных матчей</p><button className="btn btn--primary btn--block" onClick={onStart}>Начать игру</button></>}
        {page.items.map(m => <div key={m.event_id}><button className="profile-match" onClick={() => { setSelected(m); track('match_history_opened', { source: 'profile', mode: m.mode }); }}><span><strong>{outcomeLabel(m)}</strong><small>{MODES[m.mode]} · {m.human_count} чел. + {m.bot_count} бот.</small><small>{m.source === 'server' ? 'Серверная' : 'Локальная'} · {new Date(m.finished_ms).toLocaleString('ru-RU')}</small></span><span>+{m.xp} XP ›</span></button>{selected?.event_id === m.event_id && <MatchDetails match={selected} onClose={() => setSelected(null)} />}</div>)}
        {page.next_cursor && <button className="btn btn--secondary btn--block" disabled={busy} onClick={() => void loadMore()}>Показать ещё</button>}
      </section>}

      {profile && <section><h2>Достижения</h2><ul className="profile-achievements">{Object.entries(ACHIEVEMENTS).map(([key, name]) => { const a = profile.achievements.find(a => a.key === key); return <li key={key} className={a ? 'is-unlocked' : ''}><span aria-hidden="true">{a ? '✓' : '○'}</span> {name}<small>{a ? new Date(a.unlocked_at).toLocaleDateString('ru-RU') : 'Пока не открыто'}</small></li>; })}</ul></section>}
      <p className="profile-note">Статистика ведётся с обновления 3.2</p>
    </div>
  </Dialog>;
}
export function XpProgress({ progress: p }: { progress: Profile['progress'] }) {
  return <div className="profile-xp"><progress value={p.xp_current_level} max={p.xp_to_next_level} aria-label={`Прогресс уровня ${p.level}`} /><small>{p.xp_total} XP · До уровня {p.level + 1}: {p.xp_to_next_level - p.xp_current_level} XP</small></div>;
}
function outcomeLabel(m: MatchRow) { return { win: 'Победа', loss: 'Поражение', draw: 'Ничья' }[m.outcome]; }

function MatchDetails({ match: m, onClose }: { match: MatchRow; onClose: () => void }) {
  return <section className="profile-detail" aria-label="Карточка матча"><h2>{outcomeLabel(m)}</h2><p>{m.opponents.join(', ')}</p><p>Итоговый стек: {m.final_stack} · Раздачи: {m.hands_played}</p><p>+{m.xp} XP{m.xp_limited ? ' · Достигнут дневной лимит XP, можно продолжать играть' : ''}</p><button className="btn btn--ghost" onClick={onClose}>Скрыть</button></section>;
}
