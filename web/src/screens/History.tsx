import { PlayingCard } from '../components/PlayingCard';
import type { HandRecord } from '../engine/types';
import { BOT_NAME, STREET_NAME, signed } from '../game/format';
import { useScrollMemory } from '../game/scroll';

interface Props {
  title: string;
  /** One line about the match, e.g. "Победа · 720 : 480". */
  subtitle?: string;
  hands: HandRecord[];
  onBack: () => void;
  onOpen: (hand: HandRecord) => void;
}

function winnerLine(h: HandRecord): string {
  const o = h.outcome;
  if (!o) return 'Не доиграна: матч прерван';
  const how = o.by_fold ? ' (сброс)' : '';
  if (o.winner === 'human') return `Вы выиграли${how}`;
  if (o.winner === 'bot') return `Выиграл ${BOT_NAME}${how}`;
  return 'Банк разделён';
}

/** Factual list of a match's hands (spec S06): no evaluative advice. */
export function History({ title, subtitle, hands, onBack, onOpen }: Props) {
  useScrollMemory(`hands:${title}:${subtitle ?? ''}`);
  return (
    <main className="history">
      <header className="screen-header">
        <button className="btn btn--ghost btn--small" onClick={onBack} aria-label="Назад">
          ← Назад
        </button>
        <div className="screen-header__titles">
          <h1 className="screen-header__title">{title}</h1>
          {subtitle && <p className="screen-header__subtitle">{subtitle}</p>}
        </div>
      </header>
      {hands.length === 0 ? (
        <p className="history__empty">В этом матче не сыграно ни одной раздачи.</p>
      ) : (
        <ol className="history__list">
          {hands.map((h) => {
            const o = h.outcome;
            const net = o?.net[0] ?? 0;
            const ended = !o ? STREET_NAME[h.actions.at(-1)?.street ?? 'preflop'] : o.by_fold ? STREET_NAME[o.ended_on] : 'Вскрытие';
            return (
              <li key={h.number} className={`history__item${o ? '' : ' history__item--unfinished'}`}>
                <button
                  className="history__row"
                  onClick={() => onOpen(h)}
                  aria-label={`Раздача ${h.number}: ${winnerLine(h)}. Открыть повтор`}
                >
                  <span className="history__no">#{h.number}</span>
                  <span className="history__main">
                    <span>
                      {o ? `Банк ${o.pot}` : 'Не доиграна'} · {ended}
                    </span>
                    <span className="history__sub">{winnerLine(h)}</span>
                    {h.board.length > 0 && (
                      <span className="history__cards" aria-label="Общие карты">
                        {h.board.map((c) => (
                          <PlayingCard key={c} card={c} size="xs" />
                        ))}
                      </span>
                    )}
                  </span>
                  <span className={`history__net ${net > 0 ? 'tone-win' : net < 0 ? 'tone-loss' : ''}`}>
                    {o ? signed(net) : '—'}
                  </span>
                  <span className="history__chevron" aria-hidden="true">
                    ›
                  </span>
                </button>
              </li>
            );
          })}
        </ol>
      )}
    </main>
  );
}
