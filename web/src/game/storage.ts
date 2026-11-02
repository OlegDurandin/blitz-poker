// Local persistence. Saves are plain JSON written after every confirmed engine
// transition, so a closed WebView resumes exactly where it was — with the same
// deck, because the engine snapshot carries the RNG state.

import type { MatchState } from '../engine/types';

const KEYS = {
  match: 'blitz.v1.match',
  tutorial: 'blitz.v1.tutorial',
  tutorialDone: 'blitz.v1.tutorial_done',
  stats: 'blitz.v1.stats',
} as const;

export type TutorialStep = 'cards' | 'action' | 'result' | 'done';

export interface SessionMeta {
  startedAt: number;
  handStartedAt: number;
  firstMatch: boolean;
  tutorialStep?: TutorialStep;
}

export interface Saved {
  state: MatchState;
  meta: SessionMeta;
}

export interface Stats {
  wins: number;
  losses: number;
  draws: number;
  abandoned: number;
  currentStreak: number;
  bestStreak: number;
  /** Guards against counting the same match twice. */
  lastMatchId: string | null;
}

export const EMPTY_STATS: Stats = {
  wins: 0,
  losses: 0,
  draws: 0,
  abandoned: 0,
  currentStreak: 0,
  bestStreak: 0,
  lastMatchId: null,
};

export function completedMatches(s: Stats): number {
  return s.wins + s.losses + s.draws;
}

/** Idempotent: recording the same match id twice changes nothing. */
export function recordResult(s: Stats, matchId: string, outcome: 'win' | 'loss' | 'draw'): Stats {
  if (s.lastMatchId === matchId) return s;
  const currentStreak = outcome === 'win' ? s.currentStreak + 1 : 0;
  return {
    ...s,
    wins: s.wins + (outcome === 'win' ? 1 : 0),
    losses: s.losses + (outcome === 'loss' ? 1 : 0),
    draws: s.draws + (outcome === 'draw' ? 1 : 0),
    currentStreak,
    bestStreak: Math.max(s.bestStreak, currentStreak),
    lastMatchId: matchId,
  };
}

/** An abandoned match is counted separately and is not a loss. */
export function recordAbandon(s: Stats, matchId: string): Stats {
  if (s.lastMatchId === matchId) return s;
  return { ...s, abandoned: s.abandoned + 1, lastMatchId: matchId };
}

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function write(key: string, value: string | null): boolean {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
    return true;
  } catch {
    return false;
  }
}

export type LoadResult = { kind: 'none' } | { kind: 'ok'; saved: Saved } | { kind: 'corrupt' };

function load(key: string): LoadResult {
  const raw = read(key);
  if (raw === null) return { kind: 'none' };
  try {
    const parsed = JSON.parse(raw) as Partial<Saved>;
    if (!parsed || typeof parsed !== 'object' || !parsed.state || !parsed.meta) {
      return { kind: 'corrupt' };
    }
    return { kind: 'ok', saved: parsed as Saved };
  } catch {
    return { kind: 'corrupt' };
  }
}

export const storage = {
  loadMatch: () => load(KEYS.match),
  saveMatch: (s: Saved) => write(KEYS.match, JSON.stringify(s)),
  clearMatch: () => write(KEYS.match, null),

  loadTutorial: () => load(KEYS.tutorial),
  saveTutorial: (s: Saved) => write(KEYS.tutorial, JSON.stringify(s)),
  clearTutorial: () => write(KEYS.tutorial, null),

  isTutorialDone: () => read(KEYS.tutorialDone) === '1',
  markTutorialDone: () => write(KEYS.tutorialDone, '1'),

  loadStats(): Stats {
    try {
      const raw = read(KEYS.stats);
      return raw ? { ...EMPTY_STATS, ...(JSON.parse(raw) as Partial<Stats>) } : EMPTY_STATS;
    } catch {
      return EMPTY_STATS;
    }
  },
  saveStats: (s: Stats) => write(KEYS.stats, JSON.stringify(s)),
};
