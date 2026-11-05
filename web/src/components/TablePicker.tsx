import { useState } from 'react';
import type { Opponent } from '../engine/types';
import { OPPONENTS } from '../game/opponents';
import { MAX_BOTS, MIN_BOTS } from '../game/tableStorage';
import { Dialog } from './Dialog';
import { BotAvatar } from './icons';
import { Difficulty } from './OpponentPicker';

interface Props {
  initial: Opponent[];
  onStart: (bots: Opponent[]) => void;
  onClose: () => void;
}

/** "Третий не лишний": pick two or three bots to share the table with. */
export function TablePicker({ initial, onStart, onClose }: Props) {
  const [picked, setPicked] = useState<Opponent[]>(initial);
  const toggle = (id: Opponent) =>
    setPicked((p) => (p.includes(id) ? p.filter((x) => x !== id) : p.length < MAX_BOTS ? [...p, id] : p));
  const ready = picked.length >= MIN_BOTS;
  // Whole orbits: 3 seats play 9 hands, 4 seats 8 (engine `hands_for`).
  const hands = picked.length === MAX_BOTS ? 8 : 9;
  return (
    <Dialog title="Третий не лишний" onClose={onClose}>
      <h2 className="surface__title">Третий не лишний</h2>
      <p className="surface__text">
        Вы и ещё {MIN_BOTS}–{MAX_BOTS} бота за одним столом, {hands} раздач. Побеждает тот, у кого к концу больше всех
        фишек.
      </p>
      <div className="picker" role="group" aria-label="Соперники за столом">
        {OPPONENTS.map((o) => {
          const on = picked.includes(o.id);
          const full = !on && picked.length >= MAX_BOTS;
          return (
            <button
              key={o.id}
              className={`picker__item${on ? ' is-selected' : ''}`}
              role="checkbox"
              aria-checked={on}
              aria-disabled={full}
              disabled={full}
              onClick={() => toggle(o.id)}
            >
              <BotAvatar color={o.color} />
              <span className="picker__body">
                <span className="picker__head">
                  <span className="picker__name">{o.name}</span>
                  <Difficulty level={o.difficulty} />
                </span>
                <span className="picker__style">{o.style}</span>
              </span>
              <span className={`picker__check${on ? ' is-on' : ''}`} aria-hidden="true">
                {on ? '✓' : ''}
              </span>
            </button>
          );
        })}
      </div>
      <button className="btn btn--primary btn--block picker__go" disabled={!ready} onClick={() => onStart(picked)}>
        {ready ? `Сесть за стол (${picked.length + 1} игрока)` : `Выберите ещё ${MIN_BOTS - picked.length}`}
      </button>
    </Dialog>
  );
}
