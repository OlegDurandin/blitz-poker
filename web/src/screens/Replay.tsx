import { useEffect, useMemo, useState } from 'react';
import { BotAvatar } from '../components/icons';
import { PlayingCard } from '../components/PlayingCard';
import { ActionTag, BetChip, DealerButton, Stack } from '../components/TableParts';
import type { ActionRecord, HandRecord, Opponent, ReplayFrame, ReplayResult, Seat } from '../engine/types';
import { STREET_NAME, describeHand } from '../game/format';
import { showdownMarks, showdownReason } from '../game/showdown';
import { botName, opponentInfo } from '../game/opponents';

interface Props {
  hand: HandRecord;
  /** The bot of that match; missing in archives from before 1.2 (Alex). */
  opponent?: Opponent;
  /** Pure: builds frames from the record, never touches the live match. */
  load: (hand: HandRecord) => ReplayResult;
  onClose: (stats: { steps: number; frames: number; reachedEnd: boolean }) => void;
}

function actionText(a: ActionRecord): string {
  const allIn = a.all_in ? ' · олл-ин' : '';
  switch (a.kind) {
    case 'small_blind':
      return `малый блайнд ${a.amount}${allIn}`;
    case 'big_blind':
      return `большой блайнд ${a.amount}${allIn}`;
    case 'fold':
      return 'сброс карт';
    case 'check':
      return 'чек';
    case 'call':
      return `колл ${a.amount}${allIn}`;
    case 'bet':
      return `ставка ${a.to}${allIn}`;
    case 'raise':
      return `рейз до ${a.to}${allIn}`;
    case 'return':
      return `возврат неуравненной ставки ${a.amount}`;
  }
}

/** One factual sentence per step; no advice or judgement. */
function caption(hand: HandRecord, f: ReplayFrame, bot: string): string {
  const bank = f.pot + f.bets[0] + f.bets[1];
  const e = f.event;
  switch (e.type) {
    case 'start':
      return `Раздача ${hand.number} · блайнды ${hand.sb}/${hand.bb} · баттон: ${hand.button === 'human' ? 'вы' : bot}`;
    case 'action': {
      const verb = e.action.seat === 'human' ? 'Вы' : bot;
      return `${verb}: ${actionText(e.action)} · банк ${bank}`;
    }
    case 'street':
      return `${STREET_NAME[e.street]}${e.runout ? ' · олл-ин, карты открываются без торговли' : ''} · банк ${bank}`;
    case 'showdown': {
      const winner = hand.outcome?.winner ?? null;
      return `Вскрытие: вы — ${describeHand(e.hands[0])}, ${bot} — ${describeHand(e.hands[1])}. ${showdownReason(e.hands, winner)}`;
    }
    case 'award': {
      const stacks = `стеки ${f.stacks[0]} : ${f.stacks[1]}`;
      if (e.winner === null) return `Банк ${e.pot} разделён: по ${e.awarded[0]} и ${e.awarded[1]} · ${stacks}`;
      const who = e.winner === 'human' ? 'Вы выиграли' : `${bot} выиграл`;
      const why = e.by_fold ? ` (${e.winner === 'human' ? `${bot} сбросил карты` : 'вы сбросили карты'})` : '';
      return `${who} банк ${e.pot}${why} · ${stacks}`;
    }
    case 'unfinished':
      return 'Матч был прерван на этом месте';
  }
}

/** The seat's latest action on the frame's street, up to and including `index`. */
function lastAction(frames: ReplayFrame[], index: number, seat: Seat): ActionRecord | null {
  const street = frames[index].street;
  for (let i = index; i >= 0; i--) {
    const e = frames[i].event;
    if (e.type === 'street') return null;
    if (e.type === 'action' && e.action.seat === seat && e.action.kind !== 'return' && e.action.street === street) {
      return e.action;
    }
  }
  return null;
}

export function Replay({ hand, opponent, load, onClose }: Props) {
  const bot = botName(opponent);
  const result = useMemo(() => load(hand), [hand, load]);
  const frames = result.ok ? result.frames : [];
  const [index, setIndex] = useState(0);
  const [steps, setSteps] = useState(0);
  const last = frames.length - 1;

  const go = (to: number) => {
    const next = Math.max(0, Math.min(last, to));
    if (next !== index) setSteps((n) => n + 1);
    setIndex(next);
  };
  const close = () => onClose({ steps, frames: frames.length, reachedEnd: index === last });

  // Arrow keys on desktop VK.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'ArrowRight') go(index + 1);
      else if (e.key === 'ArrowLeft') go(index - 1);
      else if (e.key === 'Home') go(0);
      else if (e.key === 'End') go(last);
      else if (e.key === 'Escape') close();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  const header = (
    <header className="screen-header replay__header">
      <button className="btn btn--ghost btn--small" onClick={close}>
        ← Закрыть
      </button>
      <div className="screen-header__titles">
        <h1 className="screen-header__title">Раздача {hand.number}</h1>
        <p className="screen-header__subtitle">
          Блайнды {hand.sb}/{hand.bb}
        </p>
      </div>
    </header>
  );

  if (!result.ok || frames.length === 0) {
    return (
      <main className="replay">
        {header}
        <div className="center-screen">
          <p className="center-screen__text">Запись этой раздачи повреждена, её нельзя показать. Остальная история не пострадала.</p>
        </div>
      </main>
    );
  }

  const f = frames[index];
  const showdown = f.showdown;
  const cardProps = showdownMarks(showdown);
  const award = f.event.type === 'award' ? f.event : null;

  return (
    <main className="replay">
      {header}

      <section className="seat seat--bot" aria-label={bot}>
        <div className="seat__cards">
          {showdown ? (
            showdown[1].hole.map((c) => <PlayingCard key={c} card={c} size="sm" {...cardProps(c)} />)
          ) : (
            <>
              <PlayingCard faceDown size="sm" />
              <PlayingCard faceDown size="sm" />
            </>
          )}
        </div>
        <div className="seat__plate">
          <BotAvatar color={opponentInfo(opponent).color} />
          <div>
            <div className="seat__name">
              {bot}
              {hand.button === 'bot' && <DealerButton />}
            </div>
            <Stack amount={f.stacks[1]} gain={award ? award.net[1] : 0} />
          </div>
        </div>
        <ActionTag action={lastAction(frames, index, 'bot')} />
      </section>

      <section className="felt replay__felt" aria-label="Стол">
        <BetChip amount={f.bets[1]} who={bot} />
        <div className={`pot${f.pot > 0 ? '' : ' is-empty'}`}>Банк {f.pot}</div>
        <div className="board" aria-label="Общие карты">
          {Array.from({ length: 5 }, (_, i) =>
            f.board[i] ? (
              <PlayingCard key={f.board[i]} card={f.board[i]} size="md" {...cardProps(f.board[i])} />
            ) : (
              <PlayingCard key={`slot-${i}`} size="md" />
            ),
          )}
        </div>
        <div className="felt__street">{STREET_NAME[f.street]}</div>
        <BetChip amount={f.bets[0]} who="Вы" />
      </section>

      <section className="seat seat--human" aria-label="Вы">
        <div className="seat__cards seat__cards--hero">
          {hand.hole ? (
            hand.hole.map((c) => <PlayingCard key={c} card={c} size="lg" {...cardProps(c)} />)
          ) : (
            <span className="replay__no-cards">Карты не сохранились (запись версии 1.0)</span>
          )}
        </div>
        <div className="seat__plate">
          <div>
            <div className="seat__name">
              Вы
              {hand.button === 'human' && <DealerButton />}
            </div>
            <Stack amount={f.stacks[0]} gain={award ? award.net[0] : 0} />
          </div>
        </div>
        <ActionTag action={lastAction(frames, index, 'human')} />
      </section>

      <div className="dock replay__dock">
        <div className="dock__context replay__caption" aria-live="polite">
          {caption(hand, f, bot)}
        </div>
        <div className="replay__controls">
          <button className="btn btn--secondary replay__btn" onClick={() => go(0)} disabled={index === 0} aria-label="В начало">
            ⏮
          </button>
          <button className="btn btn--secondary replay__btn replay__btn--wide" onClick={() => go(index - 1)} disabled={index === 0}>
            Назад
          </button>
          <span className="replay__counter" aria-label={`Шаг ${index + 1} из ${frames.length}`}>
            {index + 1} / {frames.length}
          </span>
          <button className="btn btn--primary replay__btn replay__btn--wide" onClick={() => go(index + 1)} disabled={index === last}>
            Вперёд
          </button>
          <button className="btn btn--secondary replay__btn" onClick={() => go(last)} disabled={index === last} aria-label="В конец">
            ⏭
          </button>
        </div>
      </div>
    </main>
  );
}
