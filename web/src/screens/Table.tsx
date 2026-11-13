import { useState } from 'react';
import { HandHistory } from '../components/HandHistory';
import { SeatView, TableArena } from '../components/SeatView';
import { LegalActions } from '../components/LegalActions';
import { actionFeedback } from '../game/feedback';
import { PlayingCard } from '../components/PlayingCard';
import { BetChip } from '../components/TableParts';
import type { ActionRecord, PlayerAction, PlayerView, Seat } from '../engine/types';
import { STREET_NAME, describeHand, actionLabel } from '../game/format';
import { showdownMarks, showdownReason, type Focus } from '../game/showdown';
import { botName, opponentInfo } from '../game/opponents';
import { streetFor, useRunoutReveal } from '../game/runout';
import type { TutorialStep } from '../game/storage';

interface Props {
  view: PlayerView;
  tutorialStep?: TutorialStep;
  paused: boolean;
  notice: string | null;
  onRetrySave: () => void;
  onAct: (action: PlayerAction) => void;
  onNext: () => void;
  onMenu: () => void;
  onTutorialStep: (step: TutorialStep) => void;
  onSkipTutorial: () => void;
  /** Set for a duel with a person: the server runs clocks and hands. */
  duel?: DuelTable;
}

export interface DuelTable {
  name: string;
  photo: string | null;
  connected: boolean;
  /** While the opponent may still reconnect. */
  reconnectInMs: number | null;
  /** Time left for whoever acts (turn plus bank), and whose turn it is. */
  clock: { mine: boolean; leftMs: number } | null;
  /** During the result pause, until the next hand is dealt. */
  nextHandInMs: number | null;
  /** An action is on its way to the server. */
  pending: boolean;
  offline?: boolean;
}

/** "0:07" */
export function formatClock(ms: number): string {
  const s = Math.max(0, Math.ceil(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

const IDX: Record<Seat, 0 | 1> = { human: 0, bot: 1 };

function lastAction(view: PlayerView, seat: Seat): ActionRecord | null {
  for (let i = view.actions.length - 1; i >= 0; i--) {
    const a = view.actions[i];
    if (a.seat !== seat || a.kind === 'return') continue;
    if (view.phase === 'in_hand' && a.street !== view.street) return null;
    return a;
  }
  return null;
}

export function Table(props: Props) {
  const { view, tutorialStep, paused } = props;
  const tutorial = view.kind === 'tutorial';
  const duel = props.duel;
  const bot = duel?.name ?? botName(view.opponent);
  const reveal = useRunoutReveal(`${view.match_id}:${view.hand_number}`, duel ? null : view.outcome?.runout_from ?? null, view.board.length);


  const handOver = view.phase === 'hand_over';
  const settled = handOver && !reveal.revealing;
  const outcome = view.outcome;
  const showdown = settled ? outcome?.showdown ?? null : null;

  // While the runout is still being revealed, show pre-award stacks.
  const stacks = view.stacks.map((s, i) => (reveal.revealing && outcome ? s - outcome.awarded[i] : s));
  const myTurn = view.phase === 'in_hand' && view.to_act === 'human' && view.legal !== null;

  const act = (action: PlayerAction) => {
    if (paused || duel?.pending) return;
    actionFeedback();
    if (tutorial && tutorialStep === 'action') props.onTutorialStep('result');
    props.onAct(action);
  };

  // Which best five is marked; every new hand starts with both.
  const [focus, setFocus] = useState<{ key: string; focus: Focus }>({ key: '', focus: 'both' });
  const handKey = `${view.match_id}:${view.hand_number}`;
  const currentFocus = focus.key === handKey ? focus.focus : 'both';
  const cardProps = showdownMarks(showdown, currentFocus);

  const potShown = reveal.revealing && outcome ? outcome.pot : view.pot;
  const board = view.board.slice(0, reveal.visible);
  const opponentCards = view.opponent_hole;

  const hint = tutorial ? currentHint(tutorialStep, view, settled) : null;

  const renderSeat = (seat: Seat) => {
    const mine = seat === 'human';
    const index = IDX[seat];
    const record = lastAction(view, seat);
    const folded = view.actions.some(a => a.seat === seat && a.kind === 'fold');
    const active = view.phase === 'in_hand' && view.to_act === seat;
    const cards = mine ? view.hole : opponentCards;
    return <SeatView name={mine ? 'Вы' : bot} mine={mine} photo={!mine ? duel?.photo : undefined}
      botColor={!mine && !duel ? opponentInfo(view.opponent).color : undefined}
      stack={stacks[index]} gain={settled ? outcome!.net[index] : 0} active={active} folded={folded}
      allIn={view.phase === 'in_hand' && !folded && stacks[index] === 0}
      disconnected={duel?.offline || (!mine && duel ? !duel.connected : false)}
      badges={view.button === seat ? ['D', 'SB'] : ['BB']} winner={settled && (outcome?.awarded[index] ?? 0) > 0}
      leftMs={active && duel?.clock?.mine === mine ? duel.clock.leftMs : null}
      action={record && !['small_blind', 'big_blind'].includes(record.kind) ? actionLabel(record) : null}
      cards={cards ? cards.map(c => <PlayingCard key={c} card={c} size={mine ? 'lg' : 'sm'} {...cardProps(c)} />) : !folded ? <><PlayingCard faceDown size="sm" /><PlayingCard faceDown size="sm" /></> : null}>
      {view.bets[index] > 0 && <BetChip amount={view.bets[index]} who={mine ? 'Вы' : bot} />}
    </SeatView>;
  };

  return (
    <div className={`table-screen${settled ? ' table-screen--result' : ''}`}>
      <header className="hud">
        <span className={`hud__hand${tutorial ? ' hud__hand--tutorial' : ''}`}>
          {tutorial ? 'Обучение' : `Раздача ${view.hand_number}/${view.hands_total}`}
        </span>
        <span className="hud__blinds" aria-label={`Блайнды ${view.sb} и ${view.bb}`}>
          <span className="hud__blinds-word">Блайнды </span>
          {view.sb}/{view.bb}
        </span>
        <button className="hud__menu" onClick={props.onMenu} aria-label={duel ? 'Меню' : 'Меню и пауза'}>
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

      <div className="table-scroll" role="region" aria-label="Стол и история действий" tabIndex={0}>
      <TableArena opponents={[renderSeat('bot')]} hero={renderSeat('human')}>
        <div className="pot" aria-live="polite">Банк {potShown}</div>
        <div className="board" aria-label="Общие карты">
          {Array.from({ length: 5 }, (_, i) => <PlayingCard key={board[i] ?? `slot-${i}`} card={board[i]} size="md" {...(board[i] ? cardProps(board[i]) : {})} />)}
        </div>
        <div className="felt__street">{settled ? (outcome?.by_fold ? 'Раздача завершена' : 'Вскрытие') : STREET_NAME[reveal.revealing ? streetFor(reveal.visible) : view.street]}</div>
      </TableArena>
      {settled && outcome && <HandHistory actions={view.actions.map(a => ({ ...a, name: a.seat === 'human' ? 'Вы' : bot }))} />}

      {hint && (
        <TutorialHint
          step={hint}
          onOk={() => props.onTutorialStep(hint === 'cards' ? 'action' : 'result')}
          onSkip={props.onSkipTutorial}
        />
      )}

      </div>

      {settled && outcome && <div className="hand-result">
        <ResultBanner view={view} bot={bot} focus={currentFocus} onFocus={f => setFocus({ key: handKey, focus: f })} />
      </div>}

      <div className="dock">
        <div className="dock__context" aria-live="polite">
          {dockContext(view, bot, reveal.revealing, paused, duel)}
        </div>
        <div className="dock__body">
          {myTurn && view.legal ? (
            <LegalActions legal={view.legal} disabled={paused || !!duel?.pending} turnKey={`${view.hand_number}:${view.actions.length}:${view.to_act}`} onAct={act} />
          ) : reveal.revealing ? (
            <button className="btn btn--secondary btn--block" onClick={reveal.skip}>
              Пропустить анимацию
            </button>
          ) : settled && duel ? (
            // Duels move on by themselves after a short pause.
            <div className="actions actions--waiting duel-next" role="status">
              {view.is_final_hand ? 'Подводим итоги…' : 'Следующая раздача скоро начнётся'}
            </div>
          ) : settled ? (
            <button className="btn btn--primary btn--block" onClick={props.onNext}>
              {tutorial ? 'Начать матч' : view.is_final_hand ? 'Итоги матча' : 'Следующая раздача'}
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

function dockContext(view: PlayerView, bot: string, revealing: boolean, paused: boolean, duel?: DuelTable): string {
  if (duel?.offline) return 'Восстанавливаем соединение…';
  if (revealing) return 'Олл-ин: открываем карты';
  if (duel && !duel.connected && duel.reconnectInMs !== null) {
    return `${bot} переподключается… ${formatClock(duel.reconnectInMs)}`;
  }
  if (view.phase === 'hand_over') {
    if (view.kind === 'tutorial') return 'Обучение завершено';
    if (view.is_final_hand) return 'Матч завершён';
    if (duel?.nextHandInMs != null) {
      return `Раздача ${view.hand_number + 1}/${view.hands_total} через ${Math.max(1, Math.ceil(duel.nextHandInMs / 1000))} с`;
    }
    const [sb, bb] = view.next_blinds ?? [view.sb, view.bb];
    return `Раздача ${view.hand_number + 1}/${view.hands_total} · блайнды ${sb}/${bb}`;
  }
  if (view.to_act === 'human') {
    if (duel?.pending) return 'Отправляем ход…';
    return view.to_call > 0 ? `К ответу: ${Math.min(view.to_call, view.stacks[0])}` : 'Можно сделать чек';
  }
  if (view.to_act === 'bot') {
    if (duel) return `Ход: ${bot}`;
    return paused ? 'Пауза' : `${bot} думает…`;
  }
  return '';
}

function currentHint(step: TutorialStep | undefined, view: PlayerView, settled: boolean): TutorialStep | null {
  if (step === 'cards' && view.phase === 'in_hand') return 'cards';
  if (step === 'action' && view.phase === 'in_hand' && view.to_act === 'human') return 'action';
  if ((step === 'action' || step === 'result') && settled) return 'result';
  return null;
}

const HINT_TEXT: Record<Exclude<TutorialStep, 'done'>, string> = {
  cards: 'У тебя две закрытые карты. На стол постепенно откроются пять общих',
  action: 'Когда нужно отвечать на ставку, можно сбросить карты, уравнять её или повысить',
  result: 'Побеждает лучшая комбинация из пяти карт: твоих и общих',
};

function TutorialHint({ step, onOk, onSkip }: { step: TutorialStep; onOk: () => void; onSkip: () => void }) {
  if (step === 'done') return null;
  return (
    <div className={`hint hint--${step}`} role="note">
      <p className="hint__text">{HINT_TEXT[step]}</p>
      <div className="hint__buttons">
        <button className="btn btn--ghost btn--small" onClick={onSkip}>
          Пропустить обучение
        </button>
        {step !== 'result' && (
          <button className="btn btn--primary btn--small" onClick={onOk}>
            Понятно
          </button>
        )}
      </div>
    </div>
  );
}

function ResultBanner({
  view,
  bot,
  focus,
  onFocus,
}: {
  view: PlayerView;
  bot: string;
  focus: Focus;
  onFocus: (f: Focus) => void;
}) {
  const o = view.outcome!;
  const headline =
    o.winner === 'human'
      ? `Вы выиграли банк ${o.pot}`
      : o.winner === 'bot'
        ? `${bot} выиграл банк ${o.pot}`
        : `Ничья · банк ${o.pot} разделён`;
  const tone = o.winner === 'human' ? 'win' : o.winner === 'bot' ? 'loss' : 'draw';
  const icon = tone === 'win' ? '▲' : tone === 'loss' ? '▼' : '=';
  return (
    <div className={`banner banner--${tone}`} role="status">
      <div className="banner__headline">
        <span aria-hidden="true">{icon}</span> {headline}
      </div>
      <div className="awards">{o.awarded.map((amount, i) => amount > 0 && <span key={i}>{i === 0 ? 'Вы' : bot} <strong>+{amount}</strong></span>)}</div>
      {o.showdown ? (
        <div className="banner__detail">
          {/* Tapping a player shows only their five; tapping again shows both. */}
          <button
            className="banner__who"
            aria-pressed={focus === 'mine'}
            onClick={() => onFocus(focus === 'mine' ? 'both' : 'mine')}
          >
            <i className="legend legend--mine" aria-hidden="true" />
            Вы — {describeHand(o.showdown[0])}
          </button>
          <button
            className="banner__who"
            aria-pressed={focus === 'theirs'}
            onClick={() => onFocus(focus === 'theirs' ? 'both' : 'theirs')}
          >
            <i className="legend legend--theirs" aria-hidden="true" />
            {bot} — {describeHand(o.showdown[1])}
          </button>
          <span className="banner__reason">{showdownReason(o.showdown, o.winner)}</span>
        </div>
      ) : (
        <div className="banner__detail">
          <span>
            {o.winner === 'human' ? `Победа после сброса карт: ${bot} сбросил` : 'Вы сбросили карты'}
          </span>
        </div>
      )}
    </div>
  );
}
