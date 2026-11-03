// Local persistence. Saves are plain JSON written after every confirmed engine
// transition, so a closed WebView resumes exactly where it was — with the same
// deck, because the engine snapshot carries the RNG state.

import type { HandRecord, MatchState, Opponent } from '../engine/types';

const KEYS = {
  match: 'blitz.v1.match',
  tutorial: 'blitz.v1.tutorial',
  tutorialDone: 'blitz.v1.tutorial_done',
  stats: 'blitz.v1.stats',
  opponent: 'blitz.v1.opponent',
  archiveIndex: 'blitz.v1.archive',
  archivePrefix: 'blitz.v1.archive.',
} as const;

/** Finished or abandoned matches kept for the history screens. */
export const ARCHIVE_LIMIT = 30;
export const ARCHIVE_VERSION = 1;
export const STATS_VERSION = 3;

const OPPONENT_IDS: readonly Opponent[] = ['novice', 'station', 'tight', 'alex', 'aggressor'];
const isOpponent = (x: unknown): x is Opponent => OPPONENT_IDS.includes(x as Opponent);

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
  v: number;
  wins: number;
  losses: number;
  draws: number;
  abandoned: number;
  currentStreak: number;
  bestStreak: number;
  /** Hands that reached an outcome, in completed and abandoned matches. */
  handsPlayed: number;
  /** Largest pot the human won in a single hand. */
  biggestPot: number;
  /** Most recent match counted; also tells whether any match was played. */
  lastMatchId: string | null;
  /** Guards against counting the same match twice. */
  countedIds: string[];
  /** Finished-match results per opponent; matches from before 1.2 count for Alex. */
  byOpponent: Partial<Record<Opponent, Record<'win' | 'loss' | 'draw', number>>>;
}

export const EMPTY_STATS: Stats = {
  v: STATS_VERSION,
  wins: 0,
  losses: 0,
  draws: 0,
  abandoned: 0,
  currentStreak: 0,
  bestStreak: 0,
  handsPlayed: 0,
  biggestPot: 0,
  lastMatchId: null,
  countedIds: [],
  byOpponent: {},
};

const COUNTED_IDS_LIMIT = 50;

export function completedMatches(s: Stats): number {
  return s.wins + s.losses + s.draws;
}

/** Wins / (wins + losses); null when there is nothing to divide by. */
export function winRate(s: Stats): number | null {
  const decided = s.wins + s.losses;
  return decided === 0 ? null : s.wins / decided;
}

/** Facts about one match taken from its finished hands. */
export interface MatchFacts {
  hands: number;
  biggestPot: number;
}

export function matchFacts(hands: HandRecord[]): MatchFacts {
  let finished = 0;
  let biggestPot = 0;
  for (const h of hands) {
    if (!h.outcome) continue;
    finished += 1;
    if (h.outcome.winner === 'human') biggestPot = Math.max(biggestPot, h.outcome.pot);
  }
  return { hands: finished, biggestPot };
}

function alreadyCounted(s: Stats, matchId: string): boolean {
  return s.lastMatchId === matchId || s.countedIds.includes(matchId);
}

function counted(s: Stats, matchId: string, facts: MatchFacts): Stats {
  return {
    ...s,
    handsPlayed: s.handsPlayed + facts.hands,
    biggestPot: Math.max(s.biggestPot, facts.biggestPot),
    lastMatchId: matchId,
    countedIds: [matchId, ...s.countedIds].slice(0, COUNTED_IDS_LIMIT),
  };
}

/** Idempotent: recording the same match id twice changes nothing. */
export function recordResult(
  s: Stats,
  matchId: string,
  outcome: 'win' | 'loss' | 'draw',
  facts: MatchFacts = { hands: 0, biggestPot: 0 },
  opponent: Opponent = 'alex',
): Stats {
  if (alreadyCounted(s, matchId)) return s;
  const currentStreak = outcome === 'win' ? s.currentStreak + 1 : 0;
  const vs = s.byOpponent[opponent] ?? { win: 0, loss: 0, draw: 0 };
  return counted(
    {
      ...s,
      byOpponent: { ...s.byOpponent, [opponent]: { ...vs, [outcome]: vs[outcome] + 1 } },
      wins: s.wins + (outcome === 'win' ? 1 : 0),
      losses: s.losses + (outcome === 'loss' ? 1 : 0),
      draws: s.draws + (outcome === 'draw' ? 1 : 0),
      currentStreak,
      bestStreak: Math.max(s.bestStreak, currentStreak),
    },
    matchId,
    facts,
  );
}

/** An abandoned match is counted separately and is not a loss. */
export function recordAbandon(
  s: Stats,
  matchId: string,
  facts: MatchFacts = { hands: 0, biggestPot: 0 },
): Stats {
  if (alreadyCounted(s, matchId)) return s;
  return counted({ ...s, abandoned: s.abandoned + 1 }, matchId, facts);
}

function parseByOpponent(x: unknown): Stats['byOpponent'] {
  const out: Stats['byOpponent'] = {};
  if (!x || typeof x !== 'object') return out;
  const count = (n: unknown) => (typeof n === 'number' && Number.isFinite(n) && n >= 0 ? Math.floor(n) : 0);
  for (const [id, r] of Object.entries(x as Record<string, Record<string, unknown>>)) {
    if (isOpponent(id) && r && typeof r === 'object') out[id] = { win: count(r.win), loss: count(r.loss), draw: count(r.draw) };
  }
  return out;
}

/** Reads stats of any earlier version; unknown or broken fields fall back to defaults. */
export function parseStats(raw: string | null): Stats {
  if (!raw) return EMPTY_STATS;
  try {
    const p = JSON.parse(raw) as Partial<Record<keyof Stats, unknown>>;
    if (!p || typeof p !== 'object') return EMPTY_STATS;
    const count = (x: unknown) => (typeof x === 'number' && Number.isFinite(x) && x >= 0 ? Math.floor(x) : 0);
    return {
      v: STATS_VERSION,
      wins: count(p.wins),
      losses: count(p.losses),
      draws: count(p.draws),
      abandoned: count(p.abandoned),
      currentStreak: count(p.currentStreak),
      bestStreak: count(p.bestStreak),
      handsPlayed: count(p.handsPlayed),
      biggestPot: count(p.biggestPot),
      lastMatchId: typeof p.lastMatchId === 'string' ? p.lastMatchId : null,
      countedIds: Array.isArray(p.countedIds) ? p.countedIds.filter((id): id is string => typeof id === 'string') : [],
      byOpponent: parseByOpponent(p.byOpponent),
    };
  } catch {
    return EMPTY_STATS;
  }
}

/** A finished or abandoned match, as kept for the history screens. */
export interface ArchivedMatch {
  v: number;
  id: string;
  status: 'completed' | 'abandoned';
  /** null for abandoned matches. */
  outcome: 'win' | 'loss' | 'draw' | null;
  /** Missing in records from 1.1, which were all against Alex. */
  opponent?: Opponent;
  startedAt: number;
  endedAt: number;
  /** `[human, bot]` when the match ended or was abandoned. */
  finalStacks: [number, number];
  hands: HandRecord[];
}

function isArchivedMatch(x: unknown): x is ArchivedMatch {
  if (!x || typeof x !== 'object') return false;
  const m = x as Partial<ArchivedMatch>;
  return (
    m.v === ARCHIVE_VERSION &&
    typeof m.id === 'string' &&
    (m.status === 'completed' || m.status === 'abandoned') &&
    typeof m.endedAt === 'number' &&
    Array.isArray(m.finalStacks) &&
    Array.isArray(m.hands) &&
    m.hands.every((h) => h && typeof h === 'object' && Array.isArray(h.actions) && Array.isArray(h.board))
  );
}

export interface ArchiveLoad {
  matches: ArchivedMatch[];
  /** Records that could not be read; the rest are still shown. */
  skipped: number;
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

/** Archive ids, newest first. Rebuilt from the stored records if the index is unreadable. */
function archiveIds(): string[] {
  const raw = read(KEYS.archiveIndex);
  if (raw !== null) {
    try {
      const ids = JSON.parse(raw) as unknown;
      if (Array.isArray(ids) && ids.every((id) => typeof id === 'string')) return ids;
    } catch {
      // fall through to the scan
    }
  }
  const found: Array<[string, number]> = [];
  try {
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i);
      if (!key?.startsWith(KEYS.archivePrefix)) continue;
      const id = key.slice(KEYS.archivePrefix.length);
      try {
        found.push([id, (JSON.parse(localStorage.getItem(key) ?? '') as ArchivedMatch).endedAt ?? 0]);
      } catch {
        found.push([id, 0]);
      }
    }
  } catch {
    return [];
  }
  return found.sort((a, b) => b[1] - a[1]).map(([id]) => id);
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

  loadStats: (): Stats => parseStats(read(KEYS.stats)),

  /** The last opponent picked on the home screen; Alex by default. */
  loadOpponent(): Opponent {
    const id = read(KEYS.opponent);
    return isOpponent(id) ? id : 'alex';
  },
  saveOpponent: (id: Opponent) => write(KEYS.opponent, id),
  saveStats: (s: Stats) => write(KEYS.stats, JSON.stringify(s)),

  loadArchive(): ArchiveLoad {
    const matches: ArchivedMatch[] = [];
    let skipped = 0;
    for (const id of archiveIds()) {
      const raw = read(KEYS.archivePrefix + id);
      try {
        const parsed = raw === null ? null : (JSON.parse(raw) as unknown);
        if (isArchivedMatch(parsed)) matches.push(parsed);
        else skipped += 1;
      } catch {
        skipped += 1;
      }
    }
    return { matches, skipped };
  },

  /** Upserts by match id, newest first, keeping at most ARCHIVE_LIMIT matches. */
  saveArchived(m: ArchivedMatch): boolean {
    const ids = [m.id, ...archiveIds().filter((id) => id !== m.id)];
    if (!write(KEYS.archivePrefix + m.id, JSON.stringify(m))) return false;
    for (const old of ids.slice(ARCHIVE_LIMIT)) write(KEYS.archivePrefix + old, null);
    return write(KEYS.archiveIndex, JSON.stringify(ids.slice(0, ARCHIVE_LIMIT)));
  },
};
