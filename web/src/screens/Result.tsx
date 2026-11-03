import { CardsIcon, ChipIcon, HandshakeIcon, Trophy } from '../components/icons';
import { PlayingCard } from '../components/PlayingCard';
import type { MatchResult, Opponent } from '../engine/types';
import { describeHand, plural } from '../game/format';
import { botName } from '../game/opponents';

interface Props {
  result: MatchResult;
  opponent: Opponent;
  onRematch: () => void;
  onHistory: () => void;
  onHome: () => void;
}

const COPY = {
  win: { title: 'Победа!', subtitle: 'Вы выиграли дуэль' },
  loss: { title: 'В этот раз сильнее бот', subtitle: 'Следующая дуэль может сложиться иначе' },
  draw: { title: 'Ничья!', subtitle: 'Стеки сравнялись после восьми раздач' },
} as const;

export function Result({ result, opponent, onRematch, onHistory, onHome }: Props) {
  const copy = COPY[result.outcome];
  const n = result.hands_played;
  return (
    <main className={`result result--${result.outcome}`}>
      <div className="result__hero">
        {result.outcome === 'win' ? <Trophy /> : result.outcome === 'draw' ? <HandshakeIcon /> : <CardsIcon />}
        <h1 className="result__title">{copy.title}</h1>
        <p className="result__subtitle">{copy.subtitle}</p>
      </div>

      <section className="scoreboard" aria-label="Итоговые стеки">
        <div className={result.outcome === 'win' ? 'is-leader' : ''}>
          <span className="scoreboard__who">Вы</span>
          <span className="scoreboard__chips">
            <ChipIcon /> {result.final_stacks[0]}
          </span>
        </div>
        <div className={result.outcome === 'loss' ? 'is-leader' : ''}>
          <span className="scoreboard__who">{botName(opponent)}</span>
          <span className="scoreboard__chips">
            <ChipIcon /> {result.final_stacks[1]}
          </span>
        </div>
      </section>

      <ul className="result__facts">
        <li>
          {n} {plural(n, 'раздача сыграна', 'раздачи сыграно', 'раздач сыграно')}
          {result.ended_early && ' · досрочно, у одного из игроков закончились фишки'}
        </li>
        <li>
          Выиграно раздач: {result.hands_won[0]} из {n}
        </li>
        {result.best_hand && (
          <li className="result__best">
            <span>Лучшая комбинация: {describeHand({ category: result.best_hand.category, best: result.best_hand.cards })}</span>
            <span className="result__best-cards">
              {result.best_hand.cards.map((c) => (
                <PlayingCard key={c} card={c} size="xs" />
              ))}
            </span>
          </li>
        )}
      </ul>

      <div className="result__actions">
        <button className="btn btn--primary btn--hero" onClick={onRematch}>
          Играть снова
        </button>
        <button className="btn btn--secondary btn--block" onClick={onHistory}>
          История раздач
        </button>
        <button className="btn btn--ghost btn--block" onClick={onHome}>
          На главную
        </button>
      </div>
    </main>
  );
}
