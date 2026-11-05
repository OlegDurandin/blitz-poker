// "Третий не лишний" controller: owns the table snapshot, saves it after
// every confirmed transition and drives the bots, one at a time with a
// short visible pause. Rendering reads `session.view` only.

import { useCallback, useEffect, useRef, useState } from 'react';
import { tableEngine } from '../engine/engine';
import type { Opponent, PlayerAction, TableEngineResult, TableView } from '../engine/types';
import { prefersReducedMotion } from './motion';
import { recordTable, recordTableAbandon, tableStorage, type SavedTable, type TableStats } from './tableStorage';

export interface TableSession {
  state: unknown;
  view: TableView;
  meta: SavedTable['meta'];
}

const BOT_DELAY_MS = 750;
const BOT_DELAY_REDUCED_MS = 300;

function persist(s: TableSession): boolean {
  if (s.view.phase === 'match_over') return tableStorage.clear();
  return tableStorage.save({ state: s.state, meta: s.meta });
}

export function useTableGame({ paused }: { paused: boolean }) {
  const [session, setSession] = useState<TableSession | null>(null);
  const [stats, setStats] = useState<TableStats>(() => tableStorage.loadStats());
  const [notice, setNotice] = useState<string | null>(null);
  const ref = useRef<TableSession | null>(null);
  ref.current = session;

  const commit = useCallback((res: TableEngineResult, meta?: SavedTable['meta']): TableSession | null => {
    if (!res.ok) {
      setNotice('Действие не принято. Попробуйте ещё раз');
      console.error('engine rejected table command', res.error);
      return null;
    }
    const next: TableSession = { state: res.state, view: res.view, meta: meta ?? ref.current!.meta };
    ref.current = next;
    setSession(next);
    const saved = persist(next);
    setNotice(saved ? null : 'Не удалось сохранить. Повторим');
    if (next.view.phase === 'match_over' && next.view.result) {
      const updated = recordTable(tableStorage.loadStats(), next.view.table_id, next.view.result.places[next.view.me]);
      tableStorage.saveStats(updated);
      setStats(updated);
    }
    return next;
  }, []);

  const start = useCallback(
    (bots: Opponent[]) => {
      tableStorage.saveBots(bots);
      return commit(tableEngine.start(bots), { startedAt: Date.now() });
    },
    [commit],
  );

  /** Loads the saved table; false when it is missing or unreadable. */
  const resume = useCallback((): boolean => {
    const loaded = tableStorage.load();
    if (loaded.kind !== 'ok') return false;
    const res = tableEngine.load(loaded.saved.state);
    if (!res.ok) return false;
    const next: TableSession = { state: res.state, view: res.view, meta: loaded.saved.meta };
    ref.current = next;
    setSession(next);
    return true;
  }, []);

  const act = useCallback(
    (action: PlayerAction) => {
      const s = ref.current;
      // Ignore double taps: one action per turn reaches the engine.
      if (!s || s.view.phase !== 'in_hand' || s.view.to_act !== s.view.me) return;
      commit(tableEngine.act(s.state, action));
    },
    [commit],
  );

  const nextHand = useCallback(() => {
    const s = ref.current;
    if (!s || s.view.phase !== 'hand_over') return null;
    return commit(tableEngine.next(s.state));
  }, [commit]);

  /** Out of chips: straight to the standings instead of watching the bots. */
  const endNow = useCallback(() => {
    const s = ref.current;
    if (!s || s.view.phase !== 'hand_over') return null;
    return commit(tableEngine.end(s.state));
  }, [commit]);

  const saveAndExit = useCallback(() => {
    const s = ref.current;
    if (s) persist(s);
    ref.current = null;
    setSession(null);
  }, []);

  /** "Завершить": the table is dropped and counted as abandoned. */
  const abandon = useCallback(() => {
    let id = ref.current?.view.table_id ?? null;
    if (!id) {
      // From the home screen: the table is only on disk.
      const loaded = tableStorage.load();
      const res = loaded.kind === 'ok' ? tableEngine.load(loaded.saved.state) : null;
      if (res?.ok) id = res.view.table_id;
    }
    if (id) {
      const updated = recordTableAbandon(tableStorage.loadStats(), id);
      tableStorage.saveStats(updated);
      setStats(updated);
    }
    tableStorage.clear();
    ref.current = null;
    setSession(null);
  }, []);

  const leaveFinished = useCallback(() => {
    ref.current = null;
    setSession(null);
  }, []);

  const retrySave = useCallback(() => {
    const s = ref.current;
    if (s) setNotice(persist(s) ? null : 'Не удалось сохранить. Повторим');
  }, []);

  // Bots' turns: a short "thinking" pause, then one engine call each.
  useEffect(() => {
    if (!session || paused) return;
    const v = session.view;
    if (v.phase !== 'in_hand' || v.to_act === null || v.to_act === v.me) return;
    const delay = prefersReducedMotion() ? BOT_DELAY_REDUCED_MS : BOT_DELAY_MS;
    const timer = window.setTimeout(() => {
      const s = ref.current;
      if (s && s.view.phase === 'in_hand' && s.view.to_act !== null && s.view.to_act !== s.view.me) {
        commit(tableEngine.botAct(s.state));
      }
    }, delay);
    return () => window.clearTimeout(timer);
  }, [session, paused, commit]);

  return { session, stats, notice, start, resume, act, nextHand, endNow, saveAndExit, abandon, leaveFinished, retrySave };
}

export type TableGame = ReturnType<typeof useTableGame>;
