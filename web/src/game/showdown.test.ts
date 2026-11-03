import { describe, expect, it } from 'vitest';
import type { Category, ShownHand } from '../engine/types';
import { showdownMarks, showdownReason } from './showdown';

const hand = (category: Category, best: string, hole = 'As Ad'): ShownHand => ({
  hole: hole.split(' ') as [string, string],
  category,
  best: best.split(' '),
});

describe('showdown reason', () => {
  it('compares categories in words', () => {
    const fh = hand('full_house', 'Js Jc Jh Kd Ks');
    const tp = hand('two_pair', 'Kd Ks Jc Jh Ac');
    expect(showdownReason([fh, tp], 'human')).toBe('Фулл-хаус сильнее двух пар');
    expect(showdownReason([tp, fh], 'bot')).toBe('Фулл-хаус сильнее двух пар');
  });

  it('names the kicker or the higher combination', () => {
    const nine = hand('pair', 'Jd Jh 9h 7h 6s');
    const king = hand('pair', 'Jd Jh Kc 7h 6s');
    expect(showdownReason([nine, king], 'bot')).toBe('Решил кикер: король против девятки');
    const aces = hand('pair', 'Ad Ah 4h 3c 2s');
    const kings = hand('pair', 'Kd Kh Qc Jh 9s');
    expect(showdownReason([kings, aces], 'bot')).toBe('Старше: пара тузов');
  });

  it('explains ties, including when the board plays', () => {
    const board = 'Ah Kh Qh Jh Th';
    expect(showdownReason([hand('royal_flush', board), hand('royal_flush', board)], null)).toBe(
      'Ничья — у обоих лучшая пятёрка на столе',
    );
    expect(showdownReason([hand('straight', '9c 8d 7s 6h 5c'), hand('straight', '9d 8d 7s 6h 5c')], null)).toBe(
      'Ничья — одинаковая лучшая комбинация',
    );
  });
});

describe('showdown marks', () => {
  const sd: [ShownHand, ShownHand] = [hand('pair', 'Jd Jh 9h 7h 6s'), hand('pair', 'Jd Jh Kc 7h 6s')];

  it('marks shared board cards for both players and dims the rest', () => {
    const marks = showdownMarks(sd);
    expect(marks('Jd')).toEqual({ bestMine: true, bestTheirs: true, dim: false });
    expect(marks('9h')).toEqual({ bestMine: true, bestTheirs: false, dim: false });
    expect(marks('Kc')).toEqual({ bestMine: false, bestTheirs: true, dim: false });
    expect(marks('5c')).toEqual({ bestMine: false, bestTheirs: false, dim: true });
  });

  it('can focus on one player', () => {
    expect(showdownMarks(sd, 'mine')('Kc')).toEqual({ bestMine: false, bestTheirs: false, dim: true });
    expect(showdownMarks(sd, 'theirs')('Jd')).toEqual({ bestMine: false, bestTheirs: true, dim: false });
    expect(showdownMarks(null)('Jd')).toEqual({});
  });
});
