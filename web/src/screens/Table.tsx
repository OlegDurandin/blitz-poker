import { useEffect, useState } from 'react';
import { BotAvatar } from '../components/icons';
import { PlayingCard } from '../components/PlayingCard';
import { RaiseSheet } from '../components/RaiseSheet';
import { ActionTag, BetChip, DealerButton, Stack } from '../components/TableParts';
import type { ActionRecord, Card, PlayerAction, PlayerView, Seat } from '../engine/types';
import { STREET_NAME, describeHand, kickerNote } from '../game/format';
import { botName, opponentInfo } from '../game/opponents';
import { prefersReducedMotion } from '../game/motion';
import type { TutorialStep } from '../game/storage';
import { haptic } from '../vk';

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
}

const IDX: Record<Seat, 0 | 1> = { human: 0, bot: 1 };

/**
 * Board cards revealed so far during an all-in runout. The engine resolves the
 * hand at once; this only paces the reveal and can be skipped.
 */
function useRunoutReveal(view: PlayerView) {
  const key = `${view.match_id}:${view.hand_number}`;
  const from = view.outcome?.runout_from ?? null;
  const [progress, setProgress] = useState<{ key: string; count: number }>({ key: '', count: 5 });

  useEffect(() => {
    if (from === null) return;
    let count = from;
    setProgress({ key, count });
    const step = prefersReducedMotion() ? 350 : 750;
    const timer = window.setInterval(() => {
      count = count < 3 ? 3 : count + 1;
      setProgress({ key, count });
      if (count >= 5) window.clearInterval(timer);
    }, step);
    return () => window.clearInterval(timer);
  }, [key, from]);

  const count = from === null ? view.board.length : progress.key === key ? progress.count : from;
  return {
    visible: Math.min(count, view.board.length),
    revealing: from !== null && count < 5,
    skip: () => setProgress({ key, count: 5 }),
  };
}

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
  const bot = botName(view.opponent);
  const reveal = useRunoutReveal(view);
  const [raiseOpen, setRaiseOpen] = useState(false);

  const handOver = view.phase === 'hand_over';
  const settled = handOver && !reveal.revealing;
  const outcome = view.outcome;
  const showdown = settled ? outcome?.showdown ?? null : null;

  // While the runout is still being revealed, show pre-award stacks.
  const stacks = view.stacks.map((s, i) => (reveal.revealing && outcome ? s - outcome.awarded[i] : s));
  const myTurn = view.phase === 'in_hand' && view.to_act === 'human' && view.legal !== null;

  // A hand ending or a new hand starting closes a stale raise sheet.
  useEffect(() => {
    if (!myTurn) setRaiseOpen(false);
  }, [myTurn]);

  const act = (action: PlayerAction) => {
    haptic();
    setRaiseOpen(false);
    if (tutorial && tutorialStep === 'action') props.onTutorialStep('result');
    props.onAct(action);
  };

  const mineBest = new Set<Card>(showdown?.[0].best ?? []);
  const theirsBest = new Set<Card>(showdown?.[1].best ?? []);
  const cardProps = (card: Card) =>
    showdown
      ? {
          bestMine: mineBest.has(card),
          bestTheirs: theirsBest.has(card),
          dim: !mineBest.has(card) && !theirsBest.has(card),
        }
      : {};

  const potShown = reveal.revealing && outcome ? outcome.pot : view.pot;
  const board = view.board.slice(0, reveal.visible);
  const opponentCards = view.opponent_hole;

  const hint = tutorial && !raiseOpen ? currentHint(tutorialStep, view, settled) : null;

  return (
    <div className="table-screen">
      <header className="hud">
        <span className={`hud__hand${tutorial ? ' hud__hand--tutorial' : ''}`}>
          {tutorial ? 'Обучение' : `Раздача ${view.hand_number}/${view.hands_total}`}
        </span>
        <span className="hud__blinds" aria-label={`Блайнды ${view.sb} и ${view.bb}`}>
          Блайнды {view.sb}/{view.bb}
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

      <section className="seat seat--bot" aria-label={bot}>
        <div className="seat__cards">
          {opponentCards ? (
            opponentCards.map((c) => <PlayingCard key={c} card={c} size="sm" {...cardProps(c)} />)
          ) : (
            <>
              <PlayingCard faceDown size="sm" />
              <PlayingCard faceDown size="sm" />
            </>
          )}
        </div>
        <div className="seat__plate">
          <BotAvatar color={opponentInfo(view.opponent).color} />
          <div>
            <div className="seat__name">
              {bot}
              {view.button === 'bot' && <DealerButton />}
            </div>
            <Stack amount={stacks[1]} gain={settled ? outcome!.net[1] : 0} />
          </div>
        </div>
        <ActionTag action={lastAction(view, 'bot')} thinking={view.phase === 'in_hand' && view.to_act === 'bot'} />
      </section>

      <section className="felt" aria-label="Стол">
        <BetChip amount={view.bets[IDX.bot]} who={bot} />
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
          <ResultBanner view={view} />
        ) : (
          <div className="felt__street">{STREET_NAME[reveal.revealing ? streetFor(reveal.visible) : view.street]}</div>
        )}
        <BetChip amount={view.bets[IDX.human]} who="Вы" />
      </section>

      <section className="seat seat--human" aria-label="Вы">
        <div className="seat__cards seat__cards--hero">
          {view.hole.map((c) => (
            <PlayingCard key={c} card={c} size="lg" {...cardProps(c)} />
          ))}
        </div>
        <div className="seat__plate">
          <div>
            <div className="seat__name">
              Вы
              {view.button === 'human' && <DealerButton />}
            </div>
            <Stack amount={stacks[0]} gain={settled ? outcome!.net[0] : 0} />
          </div>
        </div>
        <ActionTag action={lastAction(view, 'human')} />
      </section>

      {hint && (
        <TutorialHint
          step={hint}
          onOk={() => props.onTutorialStep(hint === 'cards' ? 'action' : 'result')}
          onSkip={props.onSkipTutorial}
        />
      )}

      <div className="dock">
        <div className="dock__context" aria-live="polite">
          {dockContext(view, reveal.revealing, paused)}
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

function streetFor(boardLen: number) {
  return boardLen >= 5 ? 'river' : boardLen === 4 ? 'turn' : boardLen === 3 ? 'flop' : 'preflop';
}

function dockContext(view: PlayerView, revealing: boolean, paused: boolean): string {
  const bot = botName(view.opponent);
  if (revealing) return 'Олл-ин: открываем карты';
  if (view.phase === 'hand_over') {
    if (view.kind === 'tutorial') return 'Обучение завершено';
    if (view.is_final_hand) return 'Матч завершён';
    const [sb, bb] = view.next_blinds ?? [view.sb, view.bb];
    return `Раздача ${view.hand_number + 1}/${view.hands_total} · блайнды ${sb}/${bb}`;
  }
  if (view.to_act === 'human') {
    return view.to_call > 0 ? `Ваш ход · к ответу: ${Math.min(view.to_call, view.stacks[0])}` : 'Ваш ход';
  }
  if (view.to_act === 'bot') return paused ? 'Пауза' : `${bot} думает…`;
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

function ResultBanner({ view }: { view: PlayerView }) {
  const o = view.outcome!;
  const bot = botName(view.opponent);
  const headline =
    o.winner === 'human'
      ? `Вы выиграли банк ${o.pot}`
      : o.winner === 'bot'
        ? `${bot} выиграл банк ${o.pot}`
        : `Ничья · банк ${o.pot} разделён`;
  const tone = o.winner === 'human' ? 'win' : o.winner === 'bot' ? 'loss' : 'draw';
  const kicker = o.showdown && o.winner ? kickerNote(o.showdown[0], o.showdown[1]) : null;
  const icon = tone === 'win' ? '▲' : tone === 'loss' ? '▼' : '=';
  return (
    <div className={`banner banner--${tone}`} role="status">
      <div className="banner__headline">
        <span aria-hidden="true">{icon}</span> {headline}
      </div>
      {o.showdown ? (
        <div className="banner__detail">
          <span>
            <i className="legend legend--mine" aria-hidden="true" />
            Вы — {describeHand(o.showdown[0])}
          </span>
          <span>
            <i className="legend legend--theirs" aria-hidden="true" />
            {bot} — {describeHand(o.showdown[1])}
          </span>
          {kicker && <span className="banner__kicker">{kicker}</span>}
        </div>
      ) : (
        <div className="banner__detail">
          <span>{o.winner === 'human' ? `${bot} сбросил карты` : 'Вы сбросили карты'}</span>
        </div>
      )}
    </div>
  );
}
