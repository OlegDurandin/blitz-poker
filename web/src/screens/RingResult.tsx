import { BotAvatar, CardsIcon, ChipIcon, Trophy } from '../components/icons';
import { PlayingCard } from '../components/PlayingCard';
import type { TableView } from '../engine/types';
import { describeHand, plural } from '../game/format';
import { opponentInfo } from '../game/opponents';
import type { TableStats } from '../game/tableStorage';
import { seatName } from './Ring';

interface Props {
  view: TableView;
  stats: TableStats;
  onAgain: () => void;
  onHome: () => void;
}

const PLACE_WORD = ['Первое', 'Второе', 'Третье', 'Четвёртое'];

export function RingResult({ view, stats, onAgain, onHome }: Props) {
  const r = view.result!;
  const me = view.me;
  const place = r.places[me];
  const shared = r.places.filter((p) => p === place).length > 1;
  const title = place === 1 ? (shared ? 'Делите первое место!' : 'Победа за столом!') : `${PLACE_WORD[place - 1] ?? `${place}-е`} место`;
  const behind = view.seats.map((_, i) => i).filter((i) => r.places[i] > place);
  const subtitle =
    place === 1
      ? 'Больше всех фишек к концу стола'
      : behind.length === 0
        ? 'В этот раз стол оказался сильнее'
        : `Позади: ${behind.map((i) => seatName(view, i)).join(', ')}`;
  const order = view.seats.map((_, i) => i).sort((a, b) => r.places[a] - r.places[b] || a - b);
  const n = r.hands_played;
  const best = r.best_hands[me];
  const firsts = stats.places[0] ?? 0;

  return (
    <main className={`result result--${place === 1 ? 'win' : 'loss'}`}>
      <div className="result__hero">
        {place === 1 ? <Trophy /> : <CardsIcon />}
        <h1 className="result__title">{title}</h1>
        <p className="result__subtitle">{subtitle}</p>
      </div>

      <ol className="standings" aria-label="Итоговые места">
        {order.map((seat) => {
          const s = view.seats[seat];
          return (
            <li key={seat} className={`standings__row${seat === me ? ' is-me' : ''}`}>
              <span className="standings__place">{r.places[seat]}</span>
              {s.type === 'bot' ? <BotAvatar color={opponentInfo(s.opponent).color} /> : <span className="standings__you">Вы</span>}
              <span className="standings__name">{seatName(view, seat)}</span>
              <span className="scoreboard__chips">
                <ChipIcon /> {r.final_stacks[seat]}
              </span>
            </li>
          );
        })}
      </ol>

      <ul className="result__facts">
        <li>
          {n} {plural(n, 'раздача сыграна', 'раздачи сыграно', 'раздач сыграно')}
          {r.ended_early && ' · досрочно'}
        </li>
        <li>
          Выиграно раздач: {r.hands_won[me]} из {n}
        </li>
        {best && (
          <li className="result__best">
            <span>Лучшая комбинация: {describeHand({ category: best.category, best: best.cards })}</span>
            <span className="result__best-cards">
              {best.cards.map((c) => (
                <PlayingCard key={c} card={c} size="xs" />
              ))}
            </span>
          </li>
        )}
        {stats.played > 1 && (
          <li>
            Первых мест за столами: {firsts} из {stats.played}
          </li>
        )}
      </ul>

      <div className="result__actions">
        <button className="btn btn--primary btn--hero" onClick={onAgain}>
          Сыграть ещё стол
        </button>
        <button className="btn btn--ghost btn--block" onClick={onHome}>
          На главную
        </button>
      </div>
    </main>
  );
}
