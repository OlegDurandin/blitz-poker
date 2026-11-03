// The five bots the player can choose from. Their play is defined in
// crates/bot (`style`); this file only names and describes them.

import type { Opponent } from '../engine/types';

export interface OpponentInfo {
  id: Opponent;
  name: string;
  /** Short style label shown under the name. */
  style: string;
  description: string;
  /** 1 easy, 2 medium, 3 hard — measured against the other bots, see README. */
  difficulty: 1 | 2 | 3;
  /** Avatar accent. */
  color: string;
}

export const OPPONENTS: readonly OpponentInfo[] = [
  {
    id: 'novice',
    name: 'Бот Пикси',
    style: 'Новичок',
    description: 'Играет почти любые карты, ошибается в оценке руки и ставит понемногу.',
    difficulty: 1,
    color: '#35D69A',
  },
  {
    id: 'station',
    name: 'Бот Сэм',
    style: 'Любит уравнивать',
    description: 'Почти не сбрасывает и редко повышает. Блефовать бесполезно, ставьте с сильной рукой.',
    difficulty: 1,
    color: '#F5B83D',
  },
  {
    id: 'tight',
    name: 'Бот Тихон',
    style: 'Осторожный',
    description: 'Играет мало рук и почти не блефует. Если он ставит, скорее всего, у него что-то есть.',
    difficulty: 2,
    color: '#8FB0FF',
  },
  {
    id: 'alex',
    name: 'Бот Алекс',
    style: 'Сбалансированный',
    description: 'Считает шансы, ставит на добор и иногда блефует.',
    difficulty: 3,
    color: '#6A95FF',
  },
  {
    id: 'aggressor',
    name: 'Бот Рекс',
    style: 'Агрессор',
    description: 'Часто ставит и повышает, много блефует. Давит на каждой улице.',
    difficulty: 3,
    color: '#F48F8F',
  },
];

export const DIFFICULTY_LABEL = { 1: 'Лёгкий', 2: 'Средний', 3: 'Сложный' } as const;

const ALEX = OPPONENTS.find((o) => o.id === 'alex')!;

/** Unknown or missing ids (saves from before 1.2) mean Alex. */
export function opponentInfo(id: Opponent | null | undefined): OpponentInfo {
  return OPPONENTS.find((o) => o.id === id) ?? ALEX;
}

export function botName(id: Opponent | null | undefined): string {
  return opponentInfo(id).name;
}
