import { beforeEach, describe, expect, it } from 'vitest';
import type { HandRecord } from '../engine/types';
import { plural } from './format';
import {
  ARCHIVE_LIMIT,
  ARCHIVE_VERSION,
  EMPTY_STATS,
  completedMatches,
  matchFacts,
  parseStats,
  recordAbandon,
  recordResult,
  storage,
  winRate,
  type ArchivedMatch,
} from './storage';

/** Minimal in-memory localStorage for the node test environment. */
class MemoryStorage {
  data = new Map<string, string>();
  failWrites = false;
  get length() {
    return this.data.size;
  }
  key(i: number) {
    return [...this.data.keys()][i] ?? null;
  }
  getItem(k: string) {
    return this.data.get(k) ?? null;
  }
  setItem(k: string, v: string) {
    if (this.failWrites) throw new Error('QuotaExceededError');
    this.data.set(k, v);
  }
  removeItem(k: string) {
    this.data.delete(k);
  }
}

let mem: MemoryStorage;
beforeEach(() => {
  mem = new MemoryStorage();
  (globalThis as { localStorage?: unknown }).localStorage = mem;
});

function hand(n: number, winner: 'human' | 'bot' | null, pot: number, finished = true): HandRecord {
  return {
    number: n,
    button: 'human',
    sb: 10,
    bb: 20,
    start_stacks: [600, 600],
    hole: ['As', 'Kd'],
    board: [],
    actions: [],
    outcome: finished
      ? {
          winner,
          by_fold: true,
          ended_on: 'preflop',
          pot,
          awarded: [0, 0],
          net: [0, 0],
          showdown: null,
          runout_from: null,
        }
      : null,
  };
}

function archived(id: string, endedAt: number): ArchivedMatch {
  return {
    v: ARCHIVE_VERSION,
    id,
    status: 'completed',
    outcome: 'win',
    startedAt: endedAt - 1000,
    endedAt,
    finalStacks: [700, 500],
    hands: [hand(1, 'human', 40)],
  };
}

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

  it('never counts a match twice, even after other matches', () => {
    let s = recordResult(EMPTY_STATS, 'a', 'win', { hands: 8, biggestPot: 300 });
    s = recordAbandon(s, 'b', { hands: 2, biggestPot: 0 });
    const again = recordResult(s, 'a', 'win', { hands: 8, biggestPot: 300 });
    expect(again).toBe(s);
    expect(s).toMatchObject({ wins: 1, abandoned: 1, handsPlayed: 10, biggestPot: 300 });
  });

  it('takes hands and the biggest won pot from finished hands only', () => {
    const facts = matchFacts([hand(1, 'human', 80), hand(2, 'bot', 500), hand(3, 'human', 120), hand(4, null, 0, false)]);
    expect(facts).toEqual({ hands: 3, biggestPot: 120 });
  });

  it('shows no win rate until a match is won or lost', () => {
    expect(winRate(EMPTY_STATS)).toBeNull();
    expect(winRate(recordResult(EMPTY_STATS, 'd', 'draw'))).toBeNull();
    let s = recordResult(EMPTY_STATS, 'w', 'win');
    s = recordResult(s, 'l', 'loss');
    s = recordResult(s, 'd', 'draw');
    expect(winRate(s)).toBe(0.5);
  });

  it('reads 1.0 stats and survives garbage', () => {
    const old = JSON.stringify({ wins: 3, losses: 2, draws: 0, abandoned: 1, currentStreak: 1, bestStreak: 2, lastMatchId: 'm' });
    expect(parseStats(old)).toEqual({ ...EMPTY_STATS, wins: 3, losses: 2, abandoned: 1, currentStreak: 1, bestStreak: 2, lastMatchId: 'm' });
    expect(parseStats('{not json')).toEqual(EMPTY_STATS);
    expect(parseStats(JSON.stringify({ wins: -4, losses: 'x' }))).toEqual(EMPTY_STATS);
  });
});

describe('match archive', () => {
  it('upserts by id, newest first, and keeps a bounded number of matches', () => {
    for (let i = 0; i < ARCHIVE_LIMIT + 5; i++) expect(storage.saveArchived(archived(`m${i}`, i))).toBe(true);
    expect(storage.saveArchived(archived('m3', 1000))).toBe(true);
    const { matches, skipped } = storage.loadArchive();
    expect(skipped).toBe(0);
    expect(matches).toHaveLength(ARCHIVE_LIMIT);
    expect(matches[0].id).toBe('m3');
    expect(matches.filter((m) => m.id === 'm3')).toHaveLength(1);
    expect(mem.getItem('blitz.v1.archive.m0')).toBeNull();
  });

  it('keeps valid matches when one record is corrupt', () => {
    storage.saveArchived(archived('a', 1));
    storage.saveArchived(archived('b', 2));
    storage.saveArchived(archived('c', 3));
    mem.setItem('blitz.v1.archive.b', '{"v":1,');
    mem.setItem('blitz.v1.archive.c', JSON.stringify({ ...archived('c', 3), v: 99 }));
    const { matches, skipped } = storage.loadArchive();
    expect(matches.map((m) => m.id)).toEqual(['a']);
    expect(skipped).toBe(2);
  });

  it('rebuilds the index from records when the index itself is broken', () => {
    storage.saveArchived(archived('old', 1));
    storage.saveArchived(archived('new', 2));
    mem.setItem('blitz.v1.archive', 'garbage');
    expect(storage.loadArchive().matches.map((m) => m.id)).toEqual(['new', 'old']);
  });

  it('reports a failed write instead of throwing', () => {
    mem.failWrites = true;
    expect(storage.saveArchived(archived('x', 1))).toBe(false);
    expect(storage.loadArchive()).toEqual({ matches: [], skipped: 0 });
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
