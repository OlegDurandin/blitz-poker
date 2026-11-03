import { HeroSpade, SpadeLogo } from '../components/icons';
import { completedMatches, type Stats } from '../game/storage';

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
      </div>

      <div className="home__actions">
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
