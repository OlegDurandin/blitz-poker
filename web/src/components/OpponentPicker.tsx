import type { Opponent } from '../engine/types';
import { DIFFICULTY_LABEL, OPPONENTS, type OpponentInfo } from '../game/opponents';
import type { Stats } from '../game/storage';
import { Dialog } from './Dialog';
import { BotAvatar } from './icons';

interface Props {
  selected: Opponent;
  stats: Stats;
  onSelect: (id: Opponent) => void;
  onClose: () => void;
}

export function Difficulty({ level }: { level: OpponentInfo['difficulty'] }) {
  return (
    <span className="difficulty" aria-label={`Сложность: ${DIFFICULTY_LABEL[level].toLowerCase()}`}>
      {[1, 2, 3].map((i) => (
        <i key={i} className={i <= level ? 'is-on' : ''} aria-hidden="true" />
      ))}
      <span aria-hidden="true">{DIFFICULTY_LABEL[level]}</span>
    </span>
  );
}

/** Bottom sheet with the five bots; picking one closes it. */
export function OpponentPicker({ selected, stats, onSelect, onClose }: Props) {
  return (
    <Dialog title="Выбор соперника" onClose={onClose}>
      <h2 className="surface__title">Выберите соперника</h2>
      <div className="picker" role="radiogroup" aria-label="Соперник">
        {OPPONENTS.map((o) => {
          const record = stats.byOpponent[o.id];
          const played = record ? record.win + record.loss + record.draw : 0;
          return (
            <button
              key={o.id}
              className={`picker__item${o.id === selected ? ' is-selected' : ''}`}
              role="radio"
              aria-checked={o.id === selected}
              onClick={() => onSelect(o.id)}
            >
              <BotAvatar color={o.color} />
              <span className="picker__body">
                <span className="picker__head">
                  <span className="picker__name">{o.name}</span>
                  <Difficulty level={o.difficulty} />
                </span>
                <span className="picker__style">{o.style}</span>
                <span className="picker__text">{o.description}</span>
                {played > 0 && (
                  <span className="picker__record">
                    Ваш счёт: {record!.win} – {record!.loss}
                    {record!.draw > 0 ? `, ничьих ${record!.draw}` : ''}
                  </span>
                )}
              </span>
            </button>
          );
        })}
      </div>
    </Dialog>
  );
}
