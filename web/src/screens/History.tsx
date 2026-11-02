import { useState } from 'react';
import { PlayingCard } from '../components/PlayingCard';
import type { HandSummary, Street } from '../engine/types';
import { BOT_NAME, CATEGORY_NAME, STREET_NAME, actionLabel, signed } from '../game/format';

interface Props {
  hands: HandSummary[];
  onBack: () => void;
}

/** Factual hand history (spec S06): no evaluative advice. */
export function History({ hands, onBack }: Props) {
  const [open, setOpen] = useState<number | null>(null);
  return (
    <main className="history">
      <header className="screen-header">
        <button className="btn btn--ghost btn--small" onClick={onBack} aria-label="Назад">
          ← Назад
        </button>
        <h1 className="screen-header__title">История раздач</h1>
      </header>
      <ol className="history__list">
        {hands.map((h) => {
          const o = h.outcome;
          const isOpen = open === h.number;
          const net = o.net[0];
          const ended = o.by_fold ? STREET_NAME[o.ended_on] : 'Вскрытие';
          return (
            <li key={h.number} className="history__item">
              <button
                className="history__row"
                onClick={() => setOpen(isOpen ? null : h.number)}
                aria-expanded={isOpen}
              >
                <span className="history__no">#{h.number}</span>
                <span className="history__main">
                  <span>
                    Банк {o.pot} · {ended}
                  </span>
                  <span className="history__sub">
                    {o.winner === 'human' ? 'Вы выиграли' : o.winner === 'bot' ? `Выиграл ${BOT_NAME}` : 'Ничья'}
                    {o.by_fold ? ' (сброс)' : ''}
                  </span>
                </span>
                <span className={`history__net ${net > 0 ? 'tone-win' : net < 0 ? 'tone-loss' : ''}`}>
                  {signed(net)}
                </span>
              </button>
              {isOpen && (
                <div className="history__detail">
                  <p className="history__meta">
                    Блайнды {h.sb}/{h.bb} · баттон: {h.button === 'human' ? 'вы' : BOT_NAME}
                  </p>
                  {h.board.length > 0 && (
                    <div className="history__cards" aria-label="Общие карты">
                      {h.board.map((c) => (
                        <PlayingCard key={c} card={c} size="xs" />
                      ))}
                    </div>
                  )}
                  {o.showdown && (
                    <div className="history__showdown">
                      {(['Вы', BOT_NAME] as const).map((who, i) => (
                        <div key={who} className="history__shown">
                          <span>{who}:</span>
                          {o.showdown![i].hole.map((c) => (
                            <PlayingCard key={c} card={c} size="xs" />
                          ))}
                          <span className="history__sub">{CATEGORY_NAME[o.showdown![i].category]}</span>
                        </div>
                      ))}
                    </div>
                  )}
                  <ActionLog hand={h} />
                </div>
              )}
            </li>
          );
        })}
      </ol>
    </main>
  );
}

function ActionLog({ hand }: { hand: HandSummary }) {
  const streets: Street[] = ['preflop', 'flop', 'turn', 'river'];
  return (
    <div className="history__log">
      {streets.map((street) => {
        const actions = hand.actions.filter((a) => a.street === street);
        if (actions.length === 0) return null;
        return (
          <div key={street}>
            <h3 className="history__street">{STREET_NAME[street]}</h3>
            <ul>
              {actions.map((a, i) => (
                <li key={i}>
                  <span className="history__who">{a.seat === 'human' ? 'Вы' : BOT_NAME}</span> {actionLabel(a)}
                </li>
              ))}
            </ul>
          </div>
        );
      })}
    </div>
  );
}
