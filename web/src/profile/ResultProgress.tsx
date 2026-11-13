import { useEffect, useState } from 'react';
import { accountKey, personal, syncLocal, type MatchRow, type Profile } from './api';
import { XpProgress } from './Profile';
import { PVP_URL } from '../pvp/config';

export function openProfile() { window.dispatchEvent(new Event('blitz:profile')); }
export function ResultProgress({ matchId, local = false }: { matchId: string; local?: boolean }) {
  const [saved, setSaved] = useState<{ row: MatchRow; profile: Profile } | null>(null);
  const [error, setError] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const enabled = Boolean(PVP_URL && accountKey());
  useEffect(() => {
    if (!enabled) return;
    let live = true;
    setSaved(null); setError(false);
    void (async () => {
      try {
        if (local) await syncLocal();
        const profile = await personal<Profile>('profile');
        const event = local ? `local:${profile.player_id}:${matchId}` : `server:${matchId}`;
        const row = await personal<MatchRow>(`matches/${encodeURIComponent(event)}`);
        if (live) setSaved({ row, profile });
      } catch { if (live) setError(true); }
    })();
    return () => { live = false; };
  }, [enabled, matchId, local, attempt]);
  if (!enabled) return null;
  return <section className="result-progress" aria-label="Прогресс матча">
    {saved ? <><strong>Результат сохранён · +{saved.row.xp} XP</strong><span>Уровень {saved.profile.progress.level}</span><XpProgress progress={saved.profile.progress} />{saved.row.xp_limited && <p>Достигнут дневной лимит XP. Можно продолжать играть.</p>}</> : <p role="status">{error ? 'Результат ещё не подтверждён сервером.' : 'Сохраняем результат…'}</p>}
    {error && <button className="btn btn--secondary btn--block" onClick={() => setAttempt(n => n + 1)}>Проверить сохранение</button>}
    <button className="btn btn--ghost btn--block" onClick={openProfile}>Мой профиль</button>
  </section>;
}
