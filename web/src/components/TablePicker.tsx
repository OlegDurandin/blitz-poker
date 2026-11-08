import { useState } from 'react';
import type { Opponent } from '../engine/types';
import { OPPONENTS } from '../game/opponents';
import { MAX_BOTS } from '../game/tableStorage';
import { Dialog } from './Dialog';
import { BotAvatar } from './icons';
import { Difficulty } from './OpponentPicker';

interface Props {
  initial: Opponent[];
  onStart: (bots: Opponent[]) => void;
  onClose: () => void;
  /** Gather friends at a server table of `seats`; absent without a server. */
  onCreateOnline?: (seats: number) => void;
}

/** Pick bots locally, or assemble a mixed group in an online lobby. */
export function TablePicker({ initial, onStart, onClose, onCreateOnline }: Props) {
  const [picked, setPicked] = useState<Opponent[]>(initial.slice(0, 1));
  const [mode, setMode] = useState<'bots' | 'friends'>(onCreateOnline ? 'friends' : 'bots');
  const toggle = (id: Opponent) =>
    setPicked((p) => p.includes(id) ? p.filter(x => x !== id) : p.length < MAX_BOTS ? [...p, id] : p);
  const ready = picked.length > 0;
  // Heads-up has 8 hands; larger tables use whole orbits.
  const hands = picked.length === 2 ? 9 : 8;
  return (
    <Dialog title="Создать стол" onClose={onClose}>
      <h2 className="surface__title">Создать стол</h2>
      {onCreateOnline && (
        <div className="segmented" role="tablist" aria-label="С кем играть">
          <button
            role="tab"
            aria-selected={mode === 'bots'}
            className={mode === 'bots' ? 'is-on' : ''}
            onClick={() => setMode('bots')}
          >
            С ботами
          </button>
          <button
            role="tab"
            aria-selected={mode === 'friends'}
            className={mode === 'friends' ? 'is-on' : ''}
            onClick={() => setMode('friends')}
          >
            С друзьями
          </button>
        </div>
      )}
      {mode === 'friends' && onCreateOnline ? (
        <>
          <p className="surface__text">
            Приглашайте друзей и добавляйте ботов прямо за столом. Можно начать вдвоём или собрать компанию до 4 игроков.
          </p>
          <button className="btn btn--primary btn--block picker__go" onClick={() => onCreateOnline(4)}>
            Создать стол
          </button>
        </>
      ) : (
        <>
          <p className="surface__text">
            {ready
              ? `Вы и ${picked.length} ${picked.length === 1 ? 'бот' : 'бота'} · ${hands} раздач`
              : 'Выберите от 1 до 3 ботов'}
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
            {ready ? `Сесть за стол (${picked.length + 1} игрока)` : 'Выберите хотя бы одного бота'}
          </button>
        </>
      )}
    </Dialog>
  );
}
