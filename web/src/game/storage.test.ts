import { describe, expect, it } from 'vitest';
import { plural } from './format';
import { EMPTY_STATS, completedMatches, recordAbandon, recordResult } from './storage';

describe('stats', () => {
  it('counts results and win streaks', () => {
    let s = recordResult(EMPTY_STATS, 'a', 'win');
    s = recordResult(s, 'b', 'win');
    s = recordResult(s, 'c', 'loss');
    s = recordResult(s, 'd', 'win');
    expect(s).toMatchObject({ wins: 3, losses: 1, currentStreak: 1, bestStreak: 2 });
    expect(completedMatches(s)).toBe(4);
  });

  it('is idempotent per match id', () => {
    const once = recordResult(EMPTY_STATS, 'x', 'draw');
    expect(recordResult(once, 'x', 'draw')).toBe(once);
  });

  it('records abandoned matches separately from losses', () => {
    const s = recordAbandon(EMPTY_STATS, 'y');
    expect(s.abandoned).toBe(1);
    expect(s.losses).toBe(0);
    expect(completedMatches(s)).toBe(0);
  });
});

describe('format', () => {
  it('pluralises Russian nouns', () => {
    const f = (n: number) => plural(n, 'раздача', 'раздачи', 'раздач');
    expect([1, 2, 5, 8, 11, 21, 22].map(f)).toEqual([
      'раздача',
      'раздачи',
      'раздач',
      'раздач',
      'раздач',
      'раздача',
      'раздачи',
    ]);
  });
});
