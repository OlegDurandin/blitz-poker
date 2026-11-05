import { beforeEach, describe, expect, it } from 'vitest';
import { EMPTY_TABLE_STATS, recordTable, recordTableAbandon, tableStorage, validBots } from './tableStorage';

/** The node test environment has no localStorage: a map stands in. */
function memoryStorage() {
  const data = new Map<string, string>();
  return {
    getItem: (k: string) => data.get(k) ?? null,
    setItem: (k: string, v: string) => void data.set(k, v),
    removeItem: (k: string) => void data.delete(k),
  };
}

describe('table storage', () => {
  beforeEach(() => {
    (globalThis as { localStorage?: unknown }).localStorage = memoryStorage();
  });

  it('counts a finished table once, by place', () => {
    let s = recordTable(EMPTY_TABLE_STATS, 't1', 2);
    s = recordTable(s, 't1', 2);
    s = recordTable(s, 't2', 1);
    expect(s.played).toBe(2);
    expect(s.places).toEqual([1, 1]);
    expect(recordTableAbandon(s, 't2')).toBe(s);
    expect(recordTableAbandon(s, 't3').abandoned).toBe(1);
  });

  it('accepts two or three different bots only', () => {
    expect(validBots(['alex', 'tight'])).toBe(true);
    expect(validBots(['alex', 'tight', 'novice'])).toBe(true);
    expect(validBots(['alex'])).toBe(false);
    expect(validBots(['alex', 'alex'])).toBe(false);
    expect(validBots(['alex', 'robot'])).toBe(false);
    expect(validBots(['alex', 'tight', 'novice', 'station'])).toBe(false);
  });

  it('remembers the line-up and falls back to a friendly one', () => {
    expect(tableStorage.loadBots()).toEqual(['novice', 'tight']);
    tableStorage.saveBots(['aggressor', 'alex', 'station']);
    expect(tableStorage.loadBots()).toEqual(['aggressor', 'alex', 'station']);
    localStorage.setItem('blitz.v1.table_bots', '"junk"');
    expect(tableStorage.loadBots()).toEqual(['novice', 'tight']);
  });

  it('reports a damaged save as corrupt', () => {
    expect(tableStorage.load().kind).toBe('none');
    localStorage.setItem('blitz.v1.table', '{not json');
    expect(tableStorage.load().kind).toBe('corrupt');
    tableStorage.save({ state: { x: 1 }, meta: { startedAt: 5 } });
    expect(tableStorage.load()).toEqual({ kind: 'ok', saved: { state: { x: 1 }, meta: { startedAt: 5 } } });
  });
});
