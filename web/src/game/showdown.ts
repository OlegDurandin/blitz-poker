// Showdown presentation shared by the table, the replay and duels. The
// engine decides the winner and the best five cards; this only maps them to
// marks and words. Colour identifies the player (cyan: you, amber: the
// opponent), never who won.

import type { Card, Category, Seat, ShownHand } from '../engine/types';
import { CATEGORY_NAME, describeHand, kickerNote } from './format';

/** Whose best five cards are marked. */
export type Focus = 'both' | 'mine' | 'theirs';

export interface MarkProps {
  bestMine?: boolean;
  bestTheirs?: boolean;
  dim?: boolean;
}

/** Card props for a showdown (`[mine, theirs]`), or none outside one. */
export function showdownMarks(showdown: [ShownHand, ShownHand] | null, focus: Focus = 'both') {
  const mine = new Set<Card>(showdown && focus !== 'theirs' ? showdown[0].best : []);
  const theirs = new Set<Card>(showdown && focus !== 'mine' ? showdown[1].best : []);
  return (card: Card): MarkProps =>
    showdown
      ? { bestMine: mine.has(card), bestTheirs: theirs.has(card), dim: !mine.has(card) && !theirs.has(card) }
      : {};
}

/** "двух пар", for "Фулл-хаус сильнее двух пар". */
const CATEGORY_GEN: Record<Category, string> = {
  high_card: 'старшей карты',
  pair: 'пары',
  two_pair: 'двух пар',
  three_of_a_kind: 'тройки',
  straight: 'стрита',
  flush: 'флеша',
  full_house: 'фулл-хауса',
  four_of_a_kind: 'каре',
  straight_flush: 'стрит-флеша',
  royal_flush: 'роял-флеша',
};

const CATEGORY_ORDER: Category[] = [
  'high_card',
  'pair',
  'two_pair',
  'three_of_a_kind',
  'straight',
  'flush',
  'full_house',
  'four_of_a_kind',
  'straight_flush',
  'royal_flush',
];

/**
 * Why the pot went where it did, in one short sentence: "Фулл-хаус сильнее
 * двух пар", "Решил кикер: король против девятки", "Ничья — одинаковая лучшая
 * комбинация". `winner` is the engine's (`human` is always "you").
 */
export function showdownReason(showdown: [ShownHand, ShownHand], winner: Seat | null): string {
  const [mine, theirs] = showdown;
  if (winner === null) {
    const boardPlays = mine.best.every((c) => theirs.best.includes(c));
    return boardPlays ? 'Ничья — у обоих лучшая пятёрка на столе' : 'Ничья — одинаковая лучшая комбинация';
  }
  const [w, l] = winner === 'human' ? [mine, theirs] : [theirs, mine];
  if (w.category !== l.category && CATEGORY_ORDER.indexOf(w.category) > CATEGORY_ORDER.indexOf(l.category)) {
    return `${CATEGORY_NAME[w.category]} сильнее ${CATEGORY_GEN[l.category]}`;
  }
  return kickerNote(w, l) ?? `Старше: ${describeHand(w)}`;
}
