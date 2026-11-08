// "Третий не лишний": a table of 3–4, me at the bottom and everyone else
// along the top in turn order. Solo against bots, or online with friends
// (`online`: names, turn clock, the server deals the next hand by itself).

import { HandHistory } from '../components/HandHistory';
import { showdownReason } from '../game/showdown';
import { SeatView, TableArena, relativeSeats } from '../components/SeatView';
import { LegalActions } from '../components/LegalActions';
import { actionFeedback } from '../game/feedback';
import { PlayingCard } from '../components/PlayingCard';
import { BetChip } from '../components/TableParts';
import type { ActionRecord, Card, PlayerAction, TableAction, TableOutcome, TableView } from '../engine/types';
import { STREET_NAME, describeHand, actionLabel } from '../game/format';
import { opponentInfo } from '../game/opponents';
import { streetFor, useRunoutReveal } from '../game/runout';

/** A person at an online table, as the server shows them. */
export interface Person {
  name: string;
  photo: string | null;
  connected: boolean;
  gone: boolean;
}

export interface OnlineTable {
  /** Per seat; null for bots. */
  people: (Person | null)[];
  /** Who acts and how long they have left (null while a bot thinks). */
  clock: { seat: number; leftMs: number | null } | null;
  nextHandInMs: number | null;
  /** An action is on its way to the server. */
  pending: boolean;
  offline?: boolean;
}

interface Props {
  view: TableView;
  paused: boolean;
  notice: string | null;
  onRetrySave: () => void;
  onAct: (action: PlayerAction) => void;
  onNext?: () => void;
  /** Solo: the human is out of chips, go to the standings. */
  onEnd?: () => void;
  onMenu: () => void;
  online?: OnlineTable;
}

export function seatName(view: Pick<TableView, 'seats' | 'me'>, seat: number, people?: (Person | null)[]): string {
  if (seat === view.me) return 'Вы';
  const s = view.seats[seat];
  if (s.type === 'bot') return opponentInfo(s.opponent).name;
  return people?.[seat]?.name ?? `Игрок ${seat + 1}`;
}

function seatColor(view: TableView, seat: number): string {
  const s = view.seats[seat];
  return s.type === 'bot' ? opponentInfo(s.opponent).color : '#8fb0ff';
}

/** ActionTag renders duel records; the seat field is not shown. */
function asRecord(a: TableAction): ActionRecord {
  return { ...a, seat: 'bot' };
}

function lastAction(view: TableView, seat: number): ActionRecord | null {
  for (let i = view.actions.length - 1; i >= 0; i--) {
    const a = view.actions[i];
    if (a.seat !== seat || a.kind === 'return') continue;
    // A fold stays visible for the rest of the hand.
    if (view.phase === 'in_hand' && a.street !== view.street && a.kind !== 'fold') return null;
    return asRecord(a);
  }
  return null;
}

const HAND_COLORS = ['#38BDF8', '#F59E0B', '#C4B5FD', '#86EFAC'];
export function tableMarks(outcome: TableOutcome | null, me: number, names: string[]) {
  return (card: Card) => ({ marks: (outcome?.showdown ?? []).flatMap((hand, seat) => hand?.best.includes(card) ? [{ label: String(seat + 1), name: names[seat], color: HAND_COLORS[(seat - me + names.length) % names.length] }] : []) });
}

export function Ring(props: Props) {
  const { view, paused, online } = props;
  const people = online?.people;
  const pending = online?.pending ?? false;
  const me = view.me;
  const reveal = useRunoutReveal(`${view.table_id}:${view.hand_number}`, online ? null : view.outcome?.runout_from ?? null, view.board.length);


  const handOver = view.phase === 'hand_over';
  const settled = handOver && !reveal.revealing;
  const outcome = view.outcome;
  const cardProps = tableMarks(settled ? outcome : null, me, view.seats.map((_, i) => seatName(view, i, people)));
  const stacks = view.stacks.map((s, i) => (reveal.revealing && outcome ? s - outcome.awarded[i] : s));
  const myTurn = view.phase === 'in_hand' && view.to_act === me && view.legal !== null;
  const busted = view.stacks[me] === 0 && settled;


  const act = (action: PlayerAction) => {
    if (pending || paused) return;
    actionFeedback();
    props.onAct(action);
  };

  const opponents = relativeSeats(view.seats.length, view.me);
  const potShown = reveal.revealing && outcome ? outcome.pot : view.pot;
  const board = view.board.slice(0, reveal.visible);

  const renderSeat = (seat: number) => {
    const mine = seat === me;
    const person = people?.[seat];
    const folded = view.folded[seat];
    const active = view.phase === 'in_hand' && view.to_act === seat;
    const cards = mine ? view.hole : view.shown[seat];
    const record = lastAction(view, seat);
    const badges = [view.button === seat && 'D', view.sb_seat === seat && 'SB', view.bb_seat === seat && 'BB'].filter(Boolean) as string[];
    return <SeatView key={seat} name={seatName(view, seat, people)} mine={mine}
      photo={person?.photo} botColor={view.seats[seat].type === 'bot' ? seatColor(view, seat) : undefined}
      stack={stacks[seat]} gain={settled && outcome ? outcome.net[seat] : 0}
      active={active} folded={folded} eliminated={!view.dealt[seat]} gone={person?.gone || view.left[seat]}
      disconnected={online?.offline || (person != null && !person.connected)}
      allIn={view.phase === 'in_hand' && view.dealt[seat] && !folded && stacks[seat] === 0}
      winner={settled && (outcome?.awarded[seat] ?? 0) > 0} badges={badges}
      leftMs={active && online?.clock?.seat === seat ? online.clock.leftMs : null}
      action={record && !['small_blind', 'big_blind'].includes(record.kind) ? actionLabel(record) : null}
      cards={cards ? cards.map(c => <PlayingCard key={c} card={c} size={mine ? 'lg' : 'sm'} {...cardProps(c)} />) : !mine && view.dealt[seat] && !folded ? <><PlayingCard faceDown size="sm" /><PlayingCard faceDown size="sm" /></> : null}>
      {view.bets[seat] > 0 && <BetChip amount={view.bets[seat]} who={seatName(view, seat, people)} />}
    </SeatView>;
  };

  return (
    <div className={`table-screen${settled ? ' table-screen--result' : ''}`}>
      <header className="hud">
        <span className="hud__hand">
          Раздача {view.hand_number}/{view.hands_total}
        </span>
        <span className="hud__blinds" aria-label={`Блайнды ${view.sb} и ${view.bb}`}>
          <span className="hud__blinds-word">Блайнды </span>
          {view.sb}/{view.bb}
        </span>
        <button className="hud__menu" onClick={props.onMenu} aria-label="Меню и пауза">
          <span aria-hidden="true">⋯</span>
        </button>
      </header>

      {props.notice && (
        <div className="notice" role="status">
          <span>{props.notice}</span>
          <button className="notice__retry" onClick={props.onRetrySave}>
            Повторить
          </button>
        </div>
      )}

      <div className="table-scroll" role="region" aria-label="Стол и результат раздачи" tabIndex={0}>
      <TableArena opponents={opponents.map(seat => renderSeat(seat))} hero={renderSeat(me)}>
        <div className="pot" aria-live="polite">Банк {potShown}</div>
        <div className="board" aria-label="Общие карты">
          {Array.from({ length: 5 }, (_, i) => <PlayingCard key={board[i] ?? `slot-${i}`} card={board[i]} size="md" {...(board[i] ? cardProps(board[i]) : {})} />)}
        </div>
        <div className="felt__street">{settled ? (outcome?.by_fold ? 'Раздача завершена' : 'Вскрытие') : STREET_NAME[reveal.revealing ? streetFor(reveal.visible) : view.street]}</div>
      </TableArena>
      {settled && outcome && <><RingBanner view={view} outcome={outcome} people={people} />
        <HandHistory actions={view.actions.map(a => ({ ...a, name: seatName(view, a.seat, people) }))} />
      </>}

      </div>

      <div className="dock">
        <div className="dock__context" aria-live="polite">
          {dockContext(view, reveal.revealing, paused, busted, online)}
        </div>
        <div className="dock__body">
          {myTurn && view.legal ? (
            <LegalActions legal={view.legal} disabled={pending || paused} turnKey={`${view.hand_number}:${view.actions.length}:${view.to_act}`} onAct={act} />
          ) : reveal.revealing ? (
            <button className="btn btn--secondary btn--block" onClick={reveal.skip}>
              Пропустить анимацию
            </button>
          ) : settled && online ? (
            // Online the server deals the next hand by itself.
            <div className="actions actions--waiting duel-next" role="status">
              {view.is_final_hand ? 'Подводим итоги…' : 'Следующая раздача скоро начнётся'}
            </div>
          ) : busted && !view.is_final_hand ? (
            <button className="btn btn--primary btn--block" onClick={props.onEnd}>
              Итоги стола
            </button>
          ) : settled ? (
            <button className="btn btn--primary btn--block" onClick={props.onNext}>
              {view.is_final_hand ? 'Итоги стола' : 'Следующая раздача'}
            </button>
          ) : (
            <div className="actions actions--waiting" aria-hidden="true">
              <span className="thinking-dots">
                <i />
                <i />
                <i />
              </span>
            </div>
          )}
        </div>
      </div>

    </div>
  );
}

export function RingBanner({ view, outcome, people }: { view: TableView; outcome: TableOutcome; people?: (Person | null)[] }) {
  const me = view.me;
  const name = (seat: number) => seatName(view, seat, people);
  const main = outcome.pots[0];
  if (!main) return null;
  const tone = outcome.net[me] > 0 ? 'win' : outcome.net[me] < 0 ? 'loss' : 'draw';
  const shown = outcome.showdown;
  const headline = main.winners.length > 1 ? 'Банк разделён' : `${name(main.winners[0])} ${main.winners[0] === me ? 'выиграли' : 'выиграл'} ${main.amount}`;
  const shownSeats = shown?.flatMap((h, i) => h ? [i] : []) ?? [];
  const comparison = shown && shownSeats.length === 2 ? showdownReason([shown[shownSeats[0]]!, shown[shownSeats[1]]!], main.winners.length > 1 ? null : main.winners[0] === shownSeats[0] ? 'human' : 'bot') : null;
  return <section className={`banner banner--${tone}`} aria-label="Результат раздачи" role="status">
    <div className="banner__headline">{headline}</div>
    <div className="banner__detail">
      {shown?.map((hand, seat) => hand && <span key={seat} className="ring-banner__hand"><i className="hand-number" style={{ background: HAND_COLORS[(seat - me + view.seats.length) % view.seats.length] }}>{seat + 1}</i>{name(seat)} — {describeHand(hand)}</span>)}
      {comparison && <span className="banner__reason">{comparison}</span>}
      {!shown && <span>Остальные игроки сбросили карты</span>}
      {outcome.pots.length > 1 && <details className="pot-details"><summary>Основной {main.amount} · Побочные: {outcome.pots.length - 1}</summary>
        {outcome.pots.map((pot, i) => <div key={i}><strong>{i === 0 ? 'Основной банк' : `Побочный банк ${i}`} · {pot.amount}</strong><span>Победители: {pot.winners.map(name).join(', ')}</span><small>Претенденты: {pot.eligible.map(name).join(', ')}</small></div>)}
      </details>}
      <div className="awards" aria-label="Фактические выплаты">{outcome.awarded.map((amount, seat) => amount > 0 && <span key={seat}>{name(seat)} <strong>+{amount}</strong></span>)}</div>
    </div>
  </section>;
}

function dockContext(
  view: TableView,
  revealing: boolean,
  paused: boolean,
  busted: boolean,
  online?: OnlineTable,
): string {
  if (online?.offline) return 'Восстанавливаем соединение…';
  if (revealing) return 'Олл-ин: открываем карты';
  if (view.phase === 'hand_over') {
    if (view.is_final_hand) return 'Стол завершён';
    if (busted) return online ? 'У вас закончились фишки — можно досмотреть' : 'У вас закончились фишки';
    if (online?.nextHandInMs != null) {
      return `Раздача ${view.hand_number + 1}/${view.hands_total} через ${Math.max(1, Math.ceil(online.nextHandInMs / 1000))} с`;
    }
    const [sb, bb] = view.next_blinds ?? [view.sb, view.bb];
    return `Раздача ${view.hand_number + 1}/${view.hands_total} · блайнды ${sb}/${bb}`;
  }
  if (view.to_act === view.me) {
    if (online?.pending) return 'Отправляем ход…';
    return view.to_call > 0 ? `К ответу: ${Math.min(view.to_call, view.stacks[view.me])}` : 'Можно сделать чек';
  }
  if (view.to_act !== null) {
    const who = seatName(view, view.to_act, online?.people);
    if (online) return online.people[view.to_act] ? `Ход: ${who}` : `${who} думает…`;
    return paused ? 'Пауза' : `${who} думает…`;
  }
  return '';
}
