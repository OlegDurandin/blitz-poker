// "Третий не лишний": the human (seat 0, bottom) against two or three bots
// along the top. Same felt, cards and controls as the duel table.

import { useEffect, useState } from 'react';
import { BotAvatar } from '../components/icons';
import { PlayingCard } from '../components/PlayingCard';
import { RaiseSheet } from '../components/RaiseSheet';
import { ActionTag, BetChip, DealerButton, Stack } from '../components/TableParts';
import type { ActionRecord, Card, PlayerAction, TableAction, TableOutcome, TableView } from '../engine/types';
import { STREET_NAME, describeHand } from '../game/format';
import { opponentInfo } from '../game/opponents';
import { streetFor, useRunoutReveal } from '../game/runout';
import { platform } from '../platform';

interface Props {
  view: TableView;
  paused: boolean;
  notice: string | null;
  onRetrySave: () => void;
  onAct: (action: PlayerAction) => void;
  onNext: () => void;
  /** The human is out of chips: go to the standings. */
  onEnd: () => void;
  onMenu: () => void;
}

export function seatName(view: Pick<TableView, 'seats' | 'me'>, seat: number): string {
  if (seat === view.me) return 'Вы';
  const s = view.seats[seat];
  return s.type === 'bot' ? opponentInfo(s.opponent).name : `Игрок ${seat + 1}`;
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

/**
 * Cyan marks on my best five, amber on the best five of whoever took the
 * main pot (if not me).
 */
function marks(outcome: TableOutcome | null, me: number) {
  const shown = outcome?.showdown;
  if (!shown) return () => ({});
  const mine = shown[me]?.best ?? [];
  const other = outcome.pots[0]?.winners.find((w) => w !== me);
  const theirs = other !== undefined ? (shown[other]?.best ?? []) : [];
  return (c: Card) => ({ bestMine: mine.includes(c), bestTheirs: theirs.includes(c) });
}

export function Ring(props: Props) {
  const { view, paused } = props;
  const me = view.me;
  const reveal = useRunoutReveal(`${view.table_id}:${view.hand_number}`, view.outcome?.runout_from ?? null, view.board.length);
  const [raiseOpen, setRaiseOpen] = useState(false);

  const handOver = view.phase === 'hand_over';
  const settled = handOver && !reveal.revealing;
  const outcome = view.outcome;
  const cardProps = marks(settled ? outcome : null, me);
  const stacks = view.stacks.map((s, i) => (reveal.revealing && outcome ? s - outcome.awarded[i] : s));
  const myTurn = view.phase === 'in_hand' && view.to_act === me && view.legal !== null;
  const busted = view.stacks[me] === 0 && settled;

  useEffect(() => {
    if (!myTurn) setRaiseOpen(false);
  }, [myTurn]);

  const act = (action: PlayerAction) => {
    platform.haptic();
    setRaiseOpen(false);
    props.onAct(action);
  };

  const opponents = view.seats.map((_, i) => i).filter((i) => i !== me);
  const potShown = reveal.revealing && outcome ? outcome.pot : view.pot;
  const board = view.board.slice(0, reveal.visible);

  return (
    <div className="table-screen">
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

      <section className={`ring-row ring-row--${opponents.length}`} aria-label="Соперники">
        {opponents.map((seat) => (
          <OpponentSeat
            key={seat}
            view={view}
            seat={seat}
            stack={stacks[seat]}
            gain={settled && outcome ? outcome.net[seat] : 0}
            cardProps={cardProps}
            thinking={view.phase === 'in_hand' && view.to_act === seat}
          />
        ))}
      </section>

      <section className={`felt${settled && outcome ? ' felt--result' : ''}`} aria-label="Стол">
        <div className={`pot${potShown > 0 ? '' : ' is-empty'}`} aria-live="polite">
          Банк {potShown}
        </div>
        <div className="board" aria-label="Общие карты">
          {Array.from({ length: 5 }, (_, i) =>
            board[i] ? (
              <PlayingCard key={board[i]} card={board[i]} size="md" {...cardProps(board[i])} />
            ) : (
              <PlayingCard key={`slot-${i}`} size="md" />
            ),
          )}
        </div>
        {settled && outcome ? (
          <RingBanner view={view} outcome={outcome} />
        ) : (
          <div className="felt__street">{STREET_NAME[reveal.revealing ? streetFor(reveal.visible) : view.street]}</div>
        )}
        <BetChip amount={view.bets[me]} who="Вы" />
      </section>

      <section className={`seat seat--human${view.folded[me] ? ' is-folded' : ''}`} aria-label="Вы">
        <div className="seat__cards seat__cards--hero">
          {view.hole ? (
            view.hole.map((c) => <PlayingCard key={c} card={c} size="lg" {...cardProps(c)} />)
          ) : (
            <span className="ring-out">Фишки закончились</span>
          )}
        </div>
        <div className="seat__plate">
          <div>
            <div className="seat__name">
              Вы
              {view.button === me && <DealerButton />}
            </div>
            <Stack amount={stacks[me]} gain={settled && outcome ? outcome.net[me] : 0} />
          </div>
        </div>
        <ActionTag action={lastAction(view, me)} />
      </section>

      <div className="dock">
        <div className="dock__context" aria-live="polite">
          {dockContext(view, reveal.revealing, paused, busted)}
        </div>
        <div className="dock__body">
          {myTurn && view.legal ? (
            <div className="actions">
              {view.legal.fold && (
                <button className="btn btn--secondary btn--action" onClick={() => act({ type: 'fold' })} aria-label="Сбросить карты">
                  Сбросить
                </button>
              )}
              {view.legal.check && (
                <button className="btn btn--secondary btn--action" onClick={() => act({ type: 'check' })} aria-label="Чек: пропустить ход без ставки">
                  Чек
                </button>
              )}
              {view.legal.call !== null && (
                <button
                  className="btn btn--secondary btn--action"
                  onClick={() => act({ type: 'call' })}
                  aria-label={view.legal.call_is_all_in ? `Уравнять олл-ин ${view.legal.call}` : `Уравнять ${view.legal.call}`}
                >
                  {view.legal.call_is_all_in ? `Олл-ин ${view.legal.call}` : `Колл ${view.legal.call}`}
                </button>
              )}
              {view.legal.raise && (
                <button
                  className="btn btn--primary btn--action"
                  onClick={() => setRaiseOpen(true)}
                  aria-label={view.legal.raise.kind === 'bet' ? 'Сделать ставку' : 'Повысить ставку'}
                  aria-haspopup="dialog"
                >
                  {view.legal.raise.kind === 'bet' ? 'Ставка' : 'Рейз'}
                </button>
              )}
            </div>
          ) : reveal.revealing ? (
            <button className="btn btn--secondary btn--block" onClick={reveal.skip}>
              Пропустить анимацию
            </button>
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

      {raiseOpen && myTurn && view.legal?.raise && (
        <RaiseSheet
          option={view.legal.raise}
          onCancel={() => setRaiseOpen(false)}
          onConfirm={(amount) => act({ type: 'raise_to', amount })}
        />
      )}
    </div>
  );
}

function OpponentSeat({
  view,
  seat,
  stack,
  gain,
  cardProps,
  thinking,
}: {
  view: TableView;
  seat: number;
  stack: number;
  gain: number;
  cardProps: (c: Card) => { bestMine?: boolean; bestTheirs?: boolean };
  thinking: boolean;
}) {
  const name = seatName(view, seat);
  const shown = view.shown[seat];
  const inHand = view.dealt[seat] && !view.folded[seat];
  const out = !view.dealt[seat];
  return (
    <div
      className={`ring-seat${thinking ? ' is-turn' : ''}${view.folded[seat] ? ' is-folded' : ''}${out ? ' is-out' : ''}`}
      aria-label={`${name}, стек ${stack}`}
    >
      <div className="ring-seat__cards">
        {shown ? (
          shown.map((c) => <PlayingCard key={c} card={c} size="sm" {...cardProps(c)} />)
        ) : inHand ? (
          <>
            <PlayingCard faceDown size="sm" />
            <PlayingCard faceDown size="sm" />
          </>
        ) : (
          <span className="ring-seat__status">{out ? 'Выбыл' : 'Сбросил'}</span>
        )}
      </div>
      <div className="ring-seat__plate">
        <BotAvatar color={seatColor(view, seat)} />
        <div className="ring-seat__info">
          <div className="seat__name">
            <span className="seat__name-text">{name.replace(/^Бот /, '')}</span>
            {view.button === seat && <DealerButton />}
          </div>
          <Stack amount={stack} gain={gain} />
        </div>
      </div>
      <BetChip amount={view.bets[seat]} who={name} />
      <ActionTag action={lastAction(view, seat)} thinking={thinking} />
    </div>
  );
}

function RingBanner({ view, outcome }: { view: TableView; outcome: TableOutcome }) {
  const me = view.me;
  const names = (seats: number[]) => seats.map((s) => seatName(view, s)).join(', ');
  const main = outcome.pots[0];
  const won = outcome.awarded[me];
  const tone = outcome.net[me] > 0 ? 'win' : outcome.net[me] < 0 ? 'loss' : 'draw';
  const icon = tone === 'win' ? '▲' : tone === 'loss' ? '▼' : '=';
  let headline: string;
  if (main.winners.length > 1) headline = `Банк ${main.amount} разделён: ${names(main.winners)}`;
  else if (main.winners[0] === me) headline = `Вы выиграли банк ${won}`;
  else headline = `${seatName(view, main.winners[0])} выигрывает банк ${main.amount}`;

  const shown = outcome.showdown;
  return (
    <div className={`banner banner--${tone}`} role="status">
      <div className="banner__headline">
        <span aria-hidden="true">{icon}</span> {headline}
      </div>
      <div className="banner__detail">
        {outcome.pots.slice(1).map((p, i) => (
          <span key={i} className="banner__kicker">
            Побочный банк {p.amount}: {names(p.winners)}
          </span>
        ))}
        {shown ? (
          shown.map((h, seat) =>
            h ? (
              <span key={seat} className="ring-banner__hand">
                {seat === me && <i className="legend legend--mine" aria-hidden="true" />}
                {seat !== me && main.winners.includes(seat) && <i className="legend legend--theirs" aria-hidden="true" />}
                {seatName(view, seat)} — {describeHand(h)}
              </span>
            ) : null,
          )
        ) : (
          <span>{main.winners[0] === me ? 'Соперники сбросили карты' : 'Остальные сбросили карты'}</span>
        )}
        {won > 0 && main.winners[0] !== me && <span className="banner__kicker">Вам досталось {won}</span>}
      </div>
    </div>
  );
}

function dockContext(view: TableView, revealing: boolean, paused: boolean, busted: boolean): string {
  if (revealing) return 'Олл-ин: открываем карты';
  if (view.phase === 'hand_over') {
    if (view.is_final_hand) return 'Стол завершён';
    if (busted) return 'У вас закончились фишки';
    const [sb, bb] = view.next_blinds ?? [view.sb, view.bb];
    return `Раздача ${view.hand_number + 1}/${view.hands_total} · блайнды ${sb}/${bb}`;
  }
  if (view.to_act === view.me) {
    return view.to_call > 0 ? `Ваш ход · к ответу: ${Math.min(view.to_call, view.stacks[view.me])}` : 'Ваш ход';
  }
  if (view.to_act !== null) return paused ? 'Пауза' : `${seatName(view, view.to_act)} думает…`;
  return '';
}
