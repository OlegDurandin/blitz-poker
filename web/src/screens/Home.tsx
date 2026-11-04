import { BotAvatar, HeroSpade, SpadeLogo } from '../components/icons';
import { Difficulty } from '../components/OpponentPicker';
import type { Opponent } from '../engine/types';
import { opponentInfo } from '../game/opponents';
import { completedMatches, type Stats } from '../game/storage';
import { BUILD } from '../pvp/config';

interface Props {
  stats: Stats;
  hasSavedMatch: boolean;
  tutorialInProgress: boolean;
  justFinished: boolean;
  onStart: () => void;
  onContinue: () => void;
  onRestart: () => void;
  onContinueTutorial: () => void;
  onSkipTutorial: () => void;
  onRules: () => void;
  onStats: () => void;
  opponent: Opponent;
  onPickOpponent: () => void;
  /** "Играть с другом"; absent when no duel server is configured. */
  onDuel?: () => void;
  /** "Войти по коду": a friend's room by its six digits. */
  onJoinByCode?: () => void;
}

export function Home(p: Props) {
  const played = completedMatches(p.stats) + p.stats.abandoned;
  return (
    <main className="home">
      <header className="brand">
        <SpadeLogo />
        <span className="brand__name">BLITZ POKER</span>
        <span className="brand__divider" aria-hidden="true" />
        <span className="brand__chip">SOLO</span>
      </header>

      <div className="home__hero">
        <HeroSpade />
        <h1 className="home__title">Покер за 5 минут</h1>
        <p className="home__subtitle">8 раздач против ИИ</p>
        <p className="home__build">Версия {BUILD}</p>
      </div>

      <div className="home__actions">
        {!p.hasSavedMatch && !p.tutorialInProgress && <OpponentRow id={p.opponent} onClick={p.onPickOpponent} />}
        {p.hasSavedMatch ? (
          <>
            <button className="btn btn--primary btn--hero" onClick={p.onContinue}>
              Продолжить матч
            </button>
            <button className="btn btn--secondary btn--block" onClick={p.onRestart}>
              Начать заново
            </button>
          </>
        ) : p.tutorialInProgress ? (
          <>
            <button className="btn btn--primary btn--hero" onClick={p.onContinueTutorial}>
              Продолжить обучение
            </button>
            <button className="btn btn--secondary btn--block" onClick={p.onSkipTutorial}>
              Пропустить обучение
            </button>
          </>
        ) : (
          <button className="btn btn--primary btn--hero" onClick={p.onStart}>
            {p.justFinished ? 'Играть снова' : 'Начать игру'}
          </button>
        )}
        {p.onDuel && (
          <button className="btn btn--secondary btn--hero home__duel" onClick={p.onDuel}>
            Играть с другом
          </button>
        )}
        {p.onJoinByCode && (
          <button className="btn btn--secondary btn--block" onClick={p.onJoinByCode}>
            Войти по коду
          </button>
        )}
        <button className="btn btn--ghost btn--block" onClick={p.onRules}>
          Как играть
        </button>
      </div>

      {played > 0 ? (
        <section className="stats-card" aria-label="Моя статистика">
          <h2 className="stats-card__title">Моя статистика</h2>
          <dl className="stats-card__grid">
            <div>
              <dt>Победы</dt>
              <dd className="tone-win">{p.stats.wins}</dd>
            </div>
            <div>
              <dt>Поражения</dt>
              <dd>{p.stats.losses}</dd>
            </div>
            <div>
              <dt>Ничьи</dt>
              <dd>{p.stats.draws}</dd>
            </div>
            <div>
              <dt>Лучшая серия</dt>
              <dd>{p.stats.bestStreak}</dd>
            </div>
          </dl>
          <button className="btn btn--ghost btn--block stats-card__more" onClick={p.onStats}>
            Статистика и история
          </button>
        </section>
      ) : (
        <p className="home__tagline">Играй в своём темпе. Соперник уже готов.</p>
      )}

      <p className="home__legal">
        Только для развлечения: фишки виртуальные, не покупаются и не выводятся.
      </p>
    </main>
  );
}

function OpponentRow({ id, onClick }: { id: Opponent; onClick: () => void }) {
  const o = opponentInfo(id);
  return (
    <button className="opponent-row" onClick={onClick} aria-label={`Соперник: ${o.name}, ${o.style}. Сменить соперника`}>
      <BotAvatar color={o.color} />
      <span className="opponent-row__body">
        <span className="opponent-row__label">Соперник</span>
        <span className="opponent-row__name">{o.name}</span>
        <span className="opponent-row__meta">{o.style}</span>
        <Difficulty level={o.difficulty} />
      </span>
      <span className="opponent-row__change">Сменить</span>
    </button>
  );
}
