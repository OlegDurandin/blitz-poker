import { plural } from '../game/format';
import { useScrollMemory } from '../game/scroll';
import { winRate, type ArchivedMatch, type Stats as StatsData } from '../game/storage';

interface Props {
  stats: StatsData;
  matches: ArchivedMatch[];
  /** Archive records that could not be read. */
  skipped: number;
  onBack: () => void;
  onOpenMatch: (m: ArchivedMatch) => void;
}

const RESULT_LABEL = { win: 'Победа', loss: 'Поражение', draw: 'Ничья' } as const;

export function matchLabel(m: ArchivedMatch): string {
  return m.status === 'abandoned' || !m.outcome ? 'Прерван' : RESULT_LABEL[m.outcome];
}

export function matchDate(m: ArchivedMatch): string {
  return new Date(m.endedAt).toLocaleString('ru-RU', {
    day: 'numeric',
    month: 'short',
    hour: '2-digit',
    minute: '2-digit',
  });
}

export function finishedHands(m: ArchivedMatch): number {
  return m.hands.filter((h) => h.outcome).length;
}

export function Stats({ stats, matches, skipped, onBack, onOpenMatch }: Props) {
  useScrollMemory('stats');
  const rate = winRate(stats);
  return (
    <main className="history stats">
      <header className="screen-header">
        <button className="btn btn--ghost btn--small" onClick={onBack} aria-label="Назад">
          ← Назад
        </button>
        <h1 className="screen-header__title">Статистика</h1>
      </header>

      <section className="stats-card" aria-label="Итоги матчей">
        <dl className="stats-card__grid">
          <div>
            <dt>Победы</dt>
            <dd className="tone-win">{stats.wins}</dd>
          </div>
          <div>
            <dt>Поражения</dt>
            <dd>{stats.losses}</dd>
          </div>
          <div>
            <dt>Ничьи</dt>
            <dd>{stats.draws}</dd>
          </div>
          <div>
            <dt>Прервано</dt>
            <dd>{stats.abandoned}</dd>
          </div>
        </dl>
        <dl className="stats-list">
          <div>
            <dt>Доля побед</dt>
            <dd>{rate === null ? '—' : `${Math.round(rate * 100)}%`}</dd>
          </div>
          <div>
            <dt>Текущая серия побед</dt>
            <dd>{stats.currentStreak}</dd>
          </div>
          <div>
            <dt>Лучшая серия побед</dt>
            <dd>{stats.bestStreak}</dd>
          </div>
          <div>
            <dt>Раздач сыграно</dt>
            <dd>{stats.handsPlayed}</dd>
          </div>
          <div>
            <dt>Самый крупный выигранный банк</dt>
            <dd>{stats.biggestPot || '—'}</dd>
          </div>
        </dl>
        <p className="stats__note">
          Доля побед — победы среди матчей, закончившихся победой или поражением. Ничьи и прерванные матчи в неё не
          входят. Обучение не учитывается.
        </p>
      </section>

      <h2 className="stats__heading">Последние матчи</h2>
      {skipped > 0 && (
        <p className="stats__warning" role="status">
          {skipped} {plural(skipped, 'запись не удалось', 'записи не удалось', 'записей не удалось')} прочитать. Остальная
          история на месте.
        </p>
      )}
      {matches.length === 0 ? (
        <p className="history__empty">Здесь появятся сыгранные матчи.</p>
      ) : (
        <ol className="history__list">
          {matches.map((m) => {
            const n = finishedHands(m);
            const label = matchLabel(m);
            const tone = m.outcome === 'win' && m.status === 'completed' ? 'tone-win' : m.outcome === 'loss' && m.status === 'completed' ? 'tone-loss' : '';
            return (
              <li key={m.id} className="history__item">
                <button
                  className="history__row"
                  onClick={() => onOpenMatch(m)}
                  aria-label={`${label}, ${matchDate(m)}, ${n} ${plural(n, 'раздача', 'раздачи', 'раздач')}. Открыть раздачи`}
                >
                  <span className="history__main">
                    <span className={tone}>{label}</span>
                    <span className="history__sub">
                      {matchDate(m)} · {n} {plural(n, 'раздача', 'раздачи', 'раздач')}
                    </span>
                  </span>
                  <span className="history__score" aria-label={`Стеки: вы ${m.finalStacks[0]}, бот ${m.finalStacks[1]}`}>
                    {m.finalStacks[0]} : {m.finalStacks[1]}
                  </span>
                  <span className="history__chevron" aria-hidden="true">
                    ›
                  </span>
                </button>
              </li>
            );
          })}
        </ol>
      )}
    </main>
  );
}
