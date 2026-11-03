import { describe, expect, it } from 'vitest';
import type { Category, ShownHand } from '../engine/types';
import { describeHand, kickerNote } from './format';

/** `best` in the engine's order: the combination first, then kickers. */
const hand = (category: Category, best: string): ShownHand => ({
  hole: ['As', 'Ad'],
  category,
  best: best.split(' '),
});

describe('hand descriptions', () => {
  it('names the ranks of the combination', () => {
    expect(describeHand(hand('pair', 'Jd Jh 9h 7h 6s'))).toBe('пара валетов');
    expect(describeHand(hand('two_pair', 'Kd Ks Jc Jh Ac'))).toBe('две пары: короли и валеты');
    expect(describeHand(hand('full_house', 'Js Jc Jh Kd Ks'))).toBe('фулл-хаус: валеты и короли');
    expect(describeHand(hand('straight', '9c 8d 7s 6h 5c'))).toBe('стрит до девятки');
    expect(describeHand(hand('straight', '5s 4h 3c 2d As'))).toBe('стрит до пятёрки');
    expect(describeHand(hand('flush', 'Ah Th 8h 4h 2h'))).toBe('флеш до туза');
    expect(describeHand(hand('high_card', 'Kc Jd 9s 5h 3c'))).toBe('старшая карта: король');
    expect(describeHand(hand('four_of_a_kind', 'Qs Qh Qd Qc 2s'))).toBe('каре дам');
    expect(describeHand(hand('three_of_a_kind', '3c 3d 3h Ks Qd'))).toBe('три тройки');
    expect(describeHand(hand('three_of_a_kind', 'Jc Jd Jh Ks Qd'))).toBe('три валета');
    expect(describeHand(hand('royal_flush', 'As Ks Qs Js Ts'))).toBe('роял-флеш');
  });

  it('explains a pot decided by the kicker', () => {
    // Board 7 6 5 J J: pair of jacks each, king beats nine.
    const human = hand('pair', 'Jd Jh 9h 7h 6s');
    const bot = hand('pair', 'Jd Jh Kc 7h 6s');
    expect(kickerNote(human, bot)).toBe('Решил кикер: король против девятки');
  });

  it('says nothing when the combinations themselves differ', () => {
    expect(kickerNote(hand('full_house', 'Js Jc Jh Kd Ks'), hand('two_pair', 'Kd Ks Jc Jh Ac'))).toBeNull();
    expect(kickerNote(hand('pair', 'Ad Ah 4h 3c 2s'), hand('pair', 'Kd Kh Qc Jh 9s'))).toBeNull();
    expect(kickerNote(hand('straight', '9c 8d 7s 6h 5c'), hand('straight', 'Tc 9d 8s 7h 6c'))).toBeNull();
  });
});
