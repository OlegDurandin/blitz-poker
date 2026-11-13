import { ResultProgress } from '../profile/ResultProgress';
import { BotAvatar, CardsIcon, ChipIcon, Trophy } from '../components/icons';
import { PlayingCard } from '../components/PlayingCard';
import type { TableView } from '../engine/types';
import { describeHand, plural } from '../game/format';
import { opponentInfo } from '../game/opponents';
import type { TableStats } from '../game/tableStorage';
import { seatName, type Person } from './Ring';

interface Props {
  view: TableView;
  /** Solo tables: the player's record across tables. */
  stats?: TableStats;
  onAgain: () => void;
  onHome: () => void;
  /** Online: who sat where, and the vote for another match. */
  online?: { people: (Person | null)[]; again: { votes: number; needed: number; mine: boolean } };
}

const PLACE_WORD = ['Первое', 'Второе', 'Третье', 'Четвёртое'];

export function RingResult({ view, stats, onAgain, onHome, online }: Props) {
  const people = online?.people;
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
        : `Позади: ${behind.map((i) => seatName(view, i, people)).join(', ')}`;
  const order = view.seats.map((_, i) => i).sort((a, b) => r.places[a] - r.places[b] || a - b);
  const n = r.hands_played;
  const best = r.best_hands[me];
  const firsts = stats?.places[0] ?? 0;

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
          const photo = people?.[seat]?.photo;
          return (
            <li key={seat} className={`standings__row${seat === me ? ' is-me' : ''}`}>
              <span className="standings__place">{r.places[seat]}</span>
              {s.type === 'bot' ? (
                <BotAvatar color={opponentInfo(s.opponent).color} />
              ) : photo ? (
                <img className="bot-avatar avatar-photo" src={photo} alt="" />
              ) : (
                <span className="standings__you">{seat === me ? 'Вы' : seatName(view, seat, people).slice(0, 1)}</span>
              )}
              <span className="standings__name">
                {seatName(view, seat, people)}
                {people?.[seat]?.gone && <span className="standings__note"> · ушёл</span>}
              </span>
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
        {stats && stats.played > 1 && (
          <li>
            Первых мест за столами: {firsts} из {stats.played}
          </li>
        )}
      </ul>

      <div className="result__actions">
        {online ? (
          <button className="btn btn--primary btn--hero" disabled={online.again.mine || online.again.needed < 1} onClick={onAgain}>
            {online.again.mine
              ? `Ждём остальных · ${online.again.votes} из ${online.again.needed}`
              : online.again.votes > 0
                ? `Ещё раз · ${online.again.votes} из ${online.again.needed} готовы`
                : 'Сыграть ещё раз'}
          </button>
        ) : (
          <button className="btn btn--primary btn--hero" onClick={onAgain}>
            Сыграть ещё стол
          </button>
        )}
        <ResultProgress matchId={view.table_id} local={!online} />
        <button className="btn btn--ghost btn--block" onClick={onHome}>
          {online ? 'Выйти из-за стола' : 'На главную'}
        </button>
      </div>
    </main>
  );
}
