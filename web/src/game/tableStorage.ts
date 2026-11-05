// Local persistence for "Третий не лишний" tables, kept apart from duel
// saves: a table in progress and a saved duel can exist side by side.

import type { Opponent, TableState } from '../engine/types';
import { OPPONENTS } from './opponents';

const KEYS = {
  table: 'blitz.v1.table',
  stats: 'blitz.v1.table_stats',
  bots: 'blitz.v1.table_bots',
} as const;

export const TABLE_STATS_VERSION = 1;
const COUNTED_IDS_LIMIT = 30;
/** How many bots sit with the player. */
export const MIN_BOTS = 2;
export const MAX_BOTS = 3;

export interface SavedTable {
  state: TableState;
  meta: { startedAt: number };
}

export interface TableStats {
  v: number;
  /** Finished tables; `places[i]` counts how often the player finished i+1-th. */
  played: number;
  places: number[];
  abandoned: number;
  countedIds: string[];
}

export const EMPTY_TABLE_STATS: TableStats = { v: TABLE_STATS_VERSION, played: 0, places: [], abandoned: 0, countedIds: [] };

export type TableLoad = { kind: 'none' } | { kind: 'corrupt' } | { kind: 'ok'; saved: SavedTable };

const IDS = OPPONENTS.map((o) => o.id);
const isOpponent = (x: unknown): x is Opponent => IDS.includes(x as Opponent);

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function write(key: string, value: string): boolean {
  try {
    localStorage.setItem(key, value);
    return true;
  } catch {
    return false;
  }
}

function remove(key: string): boolean {
  try {
    localStorage.removeItem(key);
    return true;
  } catch {
    return false;
  }
}

/** A valid bot line-up: 2–3 different bots. */
export function validBots(bots: unknown): bots is Opponent[] {
  return (
    Array.isArray(bots) &&
    bots.length >= MIN_BOTS &&
    bots.length <= MAX_BOTS &&
    bots.every(isOpponent) &&
    new Set(bots).size === bots.length
  );
}

/** Idempotent: a table id is counted once. */
export function recordTable(s: TableStats, tableId: string, place: number): TableStats {
  if (s.countedIds.includes(tableId)) return s;
  const places = [...s.places];
  while (places.length < place) places.push(0);
  places[place - 1] += 1;
  return {
    ...s,
    played: s.played + 1,
    places,
    countedIds: [tableId, ...s.countedIds].slice(0, COUNTED_IDS_LIMIT),
  };
}

export function recordTableAbandon(s: TableStats, tableId: string): TableStats {
  if (s.countedIds.includes(tableId)) return s;
  return { ...s, abandoned: s.abandoned + 1, countedIds: [tableId, ...s.countedIds].slice(0, COUNTED_IDS_LIMIT) };
}

export const tableStorage = {
  load(): TableLoad {
    const raw = read(KEYS.table);
    if (raw === null) return { kind: 'none' };
    try {
      const saved = JSON.parse(raw) as SavedTable;
      if (!saved || typeof saved !== 'object' || !('state' in saved) || typeof saved.meta?.startedAt !== 'number') {
        return { kind: 'corrupt' };
      }
      return { kind: 'ok', saved };
    } catch {
      return { kind: 'corrupt' };
    }
  },
  save: (saved: SavedTable) => write(KEYS.table, JSON.stringify(saved)),
  clear: () => remove(KEYS.table),

  loadStats(): TableStats {
    try {
      const s = JSON.parse(read(KEYS.stats) ?? 'null') as TableStats | null;
      if (s && s.v === TABLE_STATS_VERSION && Array.isArray(s.places) && Array.isArray(s.countedIds)) return s;
    } catch {
      // Unreadable stats start over; they are a convenience.
    }
    return EMPTY_TABLE_STATS;
  },
  saveStats: (s: TableStats) => write(KEYS.stats, JSON.stringify(s)),

  /** The last line-up, or two bots a newcomer can beat and learn from. */
  loadBots(): Opponent[] {
    try {
      const bots: unknown = JSON.parse(read(KEYS.bots) ?? 'null');
      if (validBots(bots)) return bots;
    } catch {
      // Fall through to the default.
    }
    return ['novice', 'tight'];
  },
  saveBots: (bots: Opponent[]) => write(KEYS.bots, JSON.stringify(bots)),
};
