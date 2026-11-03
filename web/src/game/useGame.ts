// Game controller: owns the current engine snapshot, persists it after every
// confirmed transition and drives the bot. Rendering code only reads
// `session.view` and calls these commands.

import { useCallback, useEffect, useRef, useState } from 'react';
import { engine } from '../engine/engine';
import type { EngineResult, MatchState, Opponent, PlayerAction, PlayerView } from '../engine/types';
import { track } from './analytics';
import {
  ARCHIVE_VERSION,
  matchFacts,
  recordAbandon,
  recordResult,
  storage,
  type MatchFacts,
  type LoadResult,
  type SessionMeta,
  type Stats,
  type TutorialStep,
} from './storage';
import { prefersReducedMotion } from './motion';

export interface Session {
  state: unknown;
  view: PlayerView;
  meta: SessionMeta;
}

export type ResumeOutcome = { ok: true; session: Session } | { ok: false; code: string };

const BOT_DELAY_MS = 800;
const BOT_DELAY_REDUCED_MS = 350;

function persist(s: Session): boolean {
  const saved = { state: s.state, meta: s.meta };
  if (s.view.kind === 'tutorial') return storage.saveTutorial(saved);
  if (s.view.phase === 'match_over') return storage.clearMatch();
  return storage.saveMatch(saved);
}

/**
 * Stores the match in the history archive and returns facts for the stats.
 * A failed write never blocks the game: stats still count the match.
 */
function archiveMatch(state: MatchState, view: PlayerView, meta: SessionMeta): MatchFacts {
  const res = engine.history(state);
  if (!res.ok) {
    track('persistence_error', { what: 'history', code: res.error.code });
    return { hands: view.history.length, biggestPot: 0 };
  }
  const finished = view.phase === 'match_over';
  const ok = storage.saveArchived({
    v: ARCHIVE_VERSION,
    id: view.match_id,
    status: finished ? 'completed' : 'abandoned',
    outcome: finished ? view.result!.outcome : null,
    startedAt: meta.startedAt,
    endedAt: Date.now(),
    opponent: view.opponent,
    finalStacks: finished ? view.result!.final_stacks : view.stacks,
    hands: res.hands,
  });
  if (!ok) track('persistence_error', { what: 'archive' });
  return matchFacts(res.hands);
}

function handResult(view: PlayerView): string {
  const w = view.outcome?.winner;
  return w === 'human' ? 'win' : w === 'bot' ? 'loss' : 'split';
}

export function useGame({ paused }: { paused: boolean }) {
  const [session, setSession] = useState<Session | null>(null);
  const [stats, setStats] = useState<Stats>(() => storage.loadStats());
  const [notice, setNotice] = useState<string | null>(null);
  const sessionRef = useRef<Session | null>(null);
  sessionRef.current = session;

  /** Applies an engine response; returns the new session or null on error. */
  const commit = useCallback((res: EngineResult, meta?: SessionMeta): Session | null => {
    const prev = sessionRef.current;
    if (!res.ok) {
      setNotice('Действие не принято. Попробуйте ещё раз');
      console.error('engine rejected command', res.error);
      return null;
    }
    const nextMeta: SessionMeta = { ...(meta ?? prev!.meta) };
    const view = res.view;
    if (prev && prev.view.phase === 'in_hand' && view.phase === 'hand_over' && view.kind === 'standard') {
      track('hand_completed', {
        hand_no: view.hand_number,
        duration_ms: Date.now() - nextMeta.handStartedAt,
        result: handResult(view),
      });
    }
    if (prev && view.hand_number !== prev.view.hand_number) nextMeta.handStartedAt = Date.now();

    if (res.bot_action) {
      // Action mix for tuning the bot; no cards are sent.
      const a = res.bot_action;
      track('bot_action_selected', { street: prev?.view.street ?? 'preflop', action: a.type });
    }

    const next: Session = { state: res.state, view, meta: nextMeta };
    sessionRef.current = next;
    setSession(next);
    const saved = persist(next);
    if (!saved) track('persistence_error', { what: 'match' });
    setNotice(saved ? null : 'Не удалось сохранить. Повторим');
    return next;
  }, []);

  const startMatch = useCallback((opponent: Opponent) => {
    const firstMatch = storage.loadStats().lastMatchId === null;
    const now = Date.now();
    const next = commit(engine.newMatch(opponent), { startedAt: now, handStartedAt: now, firstMatch });
    if (next) track('match_started', { match_id: next.view.match_id, is_first_match: firstMatch, opponent });
    return next;
  }, [commit]);

  const startTutorial = useCallback(() => {
    const now = Date.now();
    const next = commit(engine.newTutorial(), {
      startedAt: now,
      handStartedAt: now,
      firstMatch: true,
      tutorialStep: 'cards',
    });
    if (next) track('tutorial_started', { step: 'cards' });
    return next;
  }, [commit]);

  const resumeFrom = useCallback((loaded: LoadResult): ResumeOutcome => {
    if (loaded.kind === 'none') return { ok: false, code: 'none' };
    if (loaded.kind === 'corrupt') return { ok: false, code: 'corrupt_save' };
    const res = engine.load(loaded.saved.state);
    if (!res.ok) return { ok: false, code: res.error.code };
    const next: Session = { state: res.state, view: res.view, meta: loaded.saved.meta };
    sessionRef.current = next;
    setSession(next);
    return { ok: true, session: next };
  }, []);

  const resumeMatch = useCallback(() => {
    const out = resumeFrom(storage.loadMatch());
    if (out.ok) track('match_resumed', { hand_no: out.session.view.hand_number });
    return out;
  }, [resumeFrom]);

  const resumeTutorial = useCallback(() => resumeFrom(storage.loadTutorial()), [resumeFrom]);

  const act = useCallback(
    (action: PlayerAction) => {
      const s = sessionRef.current;
      // Ignore double taps: only one action per human turn reaches the engine.
      if (!s || s.view.phase !== 'in_hand' || s.view.to_act !== 'human') return;
      commit(engine.act(s.state, action));
    },
    [commit],
  );

  /** Leaves the hand summary. Returns the session (match_over when finished). */
  const nextHand = useCallback(() => {
    const s = sessionRef.current;
    if (!s || s.view.phase !== 'hand_over') return null;
    const next = commit(engine.next(s.state));
    if (next && next.view.phase === 'match_over' && next.view.kind === 'standard') {
      const result = next.view.result!;
      const facts = archiveMatch(next.state, next.view, next.meta);
      const updated = recordResult(storage.loadStats(), next.view.match_id, result.outcome, facts, next.view.opponent);
      storage.saveStats(updated);
      setStats(updated);
      track('match_completed', {
        duration_ms: Date.now() - next.meta.startedAt,
        result: result.outcome,
        hands_played: result.hands_played,
        opponent: next.view.opponent,
      });
    }
    return next;
  }, [commit]);

  const setTutorialStep = useCallback((step: TutorialStep) => {
    const s = sessionRef.current;
    if (!s || s.view.kind !== 'tutorial' || s.meta.tutorialStep === step) return;
    const next = { ...s, meta: { ...s.meta, tutorialStep: step } };
    sessionRef.current = next;
    setSession(next);
    persist(next);
  }, []);

  const finishTutorial = useCallback((skipped: boolean) => {
    const step = sessionRef.current?.meta.tutorialStep ?? 'cards';
    storage.markTutorialDone();
    storage.clearTutorial();
    track(skipped ? 'tutorial_skipped' : 'tutorial_completed', { step });
    sessionRef.current = null;
    setSession(null);
  }, []);

  /** "Сохранить и выйти": the snapshot is already on disk. */
  const saveAndExit = useCallback(() => {
    const s = sessionRef.current;
    if (s?.view.kind === 'standard') track('match_paused', { hand_no: s.view.hand_number });
    if (s) persist(s);
    sessionRef.current = null;
    setSession(null);
  }, []);

  const abandonView = useCallback((state: MatchState, view: PlayerView, meta: SessionMeta, reason: string) => {
    const facts = archiveMatch(state, view, meta);
    const updated = recordAbandon(storage.loadStats(), view.match_id, facts);
    storage.saveStats(updated);
    setStats(updated);
    storage.clearMatch();
    track('match_abandoned', { hand_no: view.hand_number, reason });
  }, []);

  /** "Завершить матч" from the pause menu. */
  const abandon = useCallback(() => {
    const s = sessionRef.current;
    if (s && s.view.kind === 'standard') abandonView(s.state, s.view, s.meta, 'user_quit');
    sessionRef.current = null;
    setSession(null);
  }, [abandonView]);

  /** "Начать заново" on the home screen: drop the saved match. */
  const discardSaved = useCallback(() => {
    const loaded = storage.loadMatch();
    if (loaded.kind === 'ok') {
      const res = engine.load(loaded.saved.state);
      if (res.ok) abandonView(res.state, res.view, loaded.saved.meta, 'restart');
    }
    storage.clearMatch();
  }, [abandonView]);

  const leaveFinished = useCallback(() => {
    sessionRef.current = null;
    setSession(null);
  }, []);

  /** Replay only reads an archived record; the live match is untouched. */
  const replayHand = useCallback((hand: Parameters<typeof engine.replay>[0]) => engine.replay(hand), []);

  // Bot turn: short visible "thinking" pause, then one engine call.
  useEffect(() => {
    if (!session || paused) return;
    const v = session.view;
    if (v.phase !== 'in_hand' || v.to_act !== 'bot') return;
    const delay = prefersReducedMotion() ? BOT_DELAY_REDUCED_MS : BOT_DELAY_MS;
    const timer = window.setTimeout(() => {
      const s = sessionRef.current;
      if (s && s.view.phase === 'in_hand' && s.view.to_act === 'bot') commit(engine.botAct(s.state));
    }, delay);
    return () => window.clearTimeout(timer);
  }, [session, paused, commit]);

  const retrySave = useCallback(() => {
    const s = sessionRef.current;
    if (s) setNotice(persist(s) ? null : 'Не удалось сохранить. Повторим');
  }, []);

  return {
    session,
    stats,
    notice,
    startMatch,
    startTutorial,
    resumeMatch,
    resumeTutorial,
    act,
    nextHand,
    setTutorialStep,
    finishTutorial,
    saveAndExit,
    abandon,
    discardSaved,
    leaveFinished,
    retrySave,
    replayHand,
  };
}

export type Game = ReturnType<typeof useGame>;
