import { useCallback, useEffect, useRef, useState } from 'react';
import { ConfirmDialog } from './components/Dialog';
import { OpponentPicker } from './components/OpponentPicker';
import { PauseMenu } from './components/PauseMenu';
import type { Opponent } from './engine/types';
import { engine, initEngine } from './engine/engine';
import { track } from './game/analytics';
import { botName } from './game/opponents';
import { completedMatches, storage, type ArchiveLoad } from './game/storage';
import { useGame } from './game/useGame';
import { PVP_URL, activeRoom, clearRoomFromLocation, parseRoom, roomFromLocation } from './pvp/config';
import { useDuel } from './pvp/useDuel';
import { Duel } from './screens/Duel';
import { History } from './screens/History';
import { Home } from './screens/Home';
import { Replay } from './screens/Replay';
import { Result } from './screens/Result';
import { Rules } from './screens/Rules';
import { Stats, matchDate, matchLabel } from './screens/Stats';
import { EngineError, SaveError } from './screens/SaveError';
import { Table } from './screens/Table';
import { onVkLocation } from './vk';

type Screen =
  | { name: 'loading' }
  | { name: 'home' }
  | { name: 'table' }
  | { name: 'result' }
  | { name: 'stats' }
  /** `invite`: room id from an invite link; null when this player hosts. */
  | { name: 'duel'; invite: string | null; byCode?: boolean }
  | { name: 'hands'; matchId: string; from: HistorySource }
  | { name: 'replay'; matchId: string; hand: number; from: HistorySource }
  | { name: 'save_error'; code: string }
  | { name: 'engine_error' };

type Confirm = 'quit' | 'restart' | null;

/** Where the hand list was opened from: the just-finished match or the archive. */
type HistorySource = 'result' | 'stats';

export function App() {
  const [screen, setScreen] = useState<Screen>({ name: 'loading' });
  const [rulesOpen, setRulesOpen] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [confirm, setConfirm] = useState<Confirm>(null);
  const [justFinished, setJustFinished] = useState(false);
  const [archive, setArchive] = useState<ArchiveLoad | null>(null);
  const [opponent, setOpponent] = useState<Opponent>(() => storage.loadOpponent());
  const [pickerOpen, setPickerOpen] = useState(false);
  const game = useGame({ paused: menuOpen || rulesOpen || confirm !== null });
  const duel = useDuel(screen.name === 'duel');

  const boot = useCallback(() => {
    setScreen({ name: 'loading' });
    initEngine()
      .then(() => {
        const saved = storage.loadMatch();
        track('app_open', {
          has_saved_match: saved.kind === 'ok',
          completed_matches: completedMatches(storage.loadStats()),
        });
        // An invite link opens its room directly; a saved Solo match stays as it is.
        // A reload during a duel returns to it, like an invite to one's own room.
        const invite = roomFromLocation() ?? activeRoom.get();
        if (invite && PVP_URL) {
          track('duel_opened', { source: roomFromLocation() ? 'invite' : 'resume' });
          return setScreen({ name: 'duel', invite });
        }
        if (saved.kind === 'corrupt') return setScreen({ name: 'save_error', code: 'corrupt_save' });
        if (saved.kind === 'ok') {
          const check = engine.load(saved.saved.state);
          if (!check.ok) return setScreen({ name: 'save_error', code: check.error.code });
        }
        setScreen({ name: 'home' });
      })
      .catch((e) => {
        console.error(e);
        setScreen({ name: 'engine_error' });
      });
  }, []);

  useEffect(boot, [boot]);

  // Opens a friend's room: from an invite tapped while the app was already
  // open, or a link pasted on the duel screen. A room of one's own that nobody
  // has joined yet is left for it; a match in progress is never interrupted.
  const duelRef = useRef(duel);
  duelRef.current = duel;
  const openInvite = useCallback((id: string) => {
    if (!PVP_URL) return;
    const room = duelRef.current.state.room;
    if (room && room.room_id === id) return;
    if (room && room.status !== 'waiting' && room.status !== 'closed') return;
    if (room) duelRef.current.leave();
    track('duel_opened', { source: 'invite' });
    setScreen({ name: 'duel', invite: id });
  }, []);

  useEffect(
    () =>
      onVkLocation((location) => {
        const id = parseRoom(location);
        if (id) openInvite(id);
      }),
    [openInvite],
  );

  // Saves already happen after every action; this covers the app going to background.
  useEffect(() => {
    const onHide = () => document.visibilityState === 'hidden' && game.retrySave();
    document.addEventListener('visibilitychange', onHide);
    return () => document.removeEventListener('visibilitychange', onHide);
  }, [game.retrySave]);

  const toTable = (ok: unknown) => ok && setScreen({ name: 'table' });

  const startRealMatch = (against: Opponent = opponent) => toTable(game.startMatch(against));

  const onStart = () => {
    const firstLaunch = !storage.isTutorialDone();
    track('start_click', { first_launch: firstLaunch });
    toTable(firstLaunch ? game.startTutorial() : game.startMatch(opponent));
  };

  const onContinue = () => {
    const out = game.resumeMatch();
    if (out.ok) setScreen({ name: 'table' });
    else setScreen({ name: 'save_error', code: out.code });
  };

  const onContinueTutorial = () => {
    const out = game.resumeTutorial();
    if (out.ok) setScreen({ name: 'table' });
    else {
      storage.clearTutorial();
      toTable(game.startTutorial());
    }
  };

  const skipTutorial = () => {
    game.finishTutorial(true);
    startRealMatch();
  };

  const onNext = () => {
    const view = game.session?.view;
    if (view?.kind === 'tutorial') {
      game.finishTutorial(false);
      startRealMatch();
      return;
    }
    const next = game.nextHand();
    if (next?.view.phase === 'match_over') setScreen({ name: 'result' });
  };

  const goHome = () => {
    setMenuOpen(false);
    setConfirm(null);
    setScreen({ name: 'home' });
  };

  const view = game.session?.view;

  const openStats = () => {
    setArchive(storage.loadArchive());
    track('match_history_opened', { source: 'home' });
    setScreen({ name: 'stats' });
  };

  /** Hands of a match: the live result for 'result', the archive for 'stats'. */
  const handsFor = (matchId: string, from: HistorySource) => {
    if (from === 'result') return view?.match_id === matchId ? view.history : null;
    return archive?.matches.find((m) => m.id === matchId)?.hands ?? null;
  };

  const historyScreen = screen.name === 'hands' || screen.name === 'replay' ? screen : null;
  const historyHands = historyScreen ? handsFor(historyScreen.matchId, historyScreen.from) : null;
  const replayHand =
    screen.name === 'replay' ? (historyHands?.find((h) => h.number === screen.hand) ?? null) : null;

  // Never leave the user on an empty screen without a way back.
  const orphaned =
    (screen.name === 'table' && !view) ||
    (screen.name === 'result' && !view?.result) ||
    (screen.name === 'stats' && !archive) ||
    (screen.name === 'hands' && !historyHands) ||
    (screen.name === 'replay' && !replayHand);
  useEffect(() => {
    if (orphaned) setScreen({ name: 'home' });
  }, [orphaned]);

  let content: React.ReactNode = null;

  switch (screen.name) {
    case 'loading':
      content = (
        <main className="center-screen" aria-busy="true">
          <div className="loader" aria-label="Загрузка" />
        </main>
      );
      break;
    case 'engine_error':
      content = <EngineError onRetry={boot} />;
      break;
    case 'save_error':
      content = (
        <SaveError
          code={screen.code}
          onNewGame={() => {
            storage.clearMatch();
            startRealMatch();
          }}
        />
      );
      break;
    case 'home':
      content = (
        <Home
          stats={game.stats}
          hasSavedMatch={storage.loadMatch().kind === 'ok'}
          tutorialInProgress={!storage.isTutorialDone() && storage.loadTutorial().kind === 'ok'}
          justFinished={justFinished}
          onStart={onStart}
          onContinue={onContinue}
          onRestart={() => setConfirm('restart')}
          onContinueTutorial={onContinueTutorial}
          onSkipTutorial={skipTutorial}
          onRules={() => setRulesOpen(true)}
          onStats={openStats}
          onDuel={
            PVP_URL
              ? () => {
                  track('duel_opened', { source: 'home' });
                  setScreen({ name: 'duel', invite: null });
                }
              : undefined
          }
          onJoinByCode={
            PVP_URL
              ? () => {
                  track('duel_opened', { source: 'code' });
                  setScreen({ name: 'duel', invite: null, byCode: true });
                }
              : undefined
          }
          opponent={opponent}
          onPickOpponent={() => setPickerOpen(true)}
        />
      );
      break;
    case 'table':
      content = view ? (
        <Table
          view={view}
          tutorialStep={game.session?.meta.tutorialStep}
          paused={menuOpen || rulesOpen || confirm !== null}
          notice={game.notice}
          onRetrySave={game.retrySave}
          onAct={game.act}
          onNext={onNext}
          onMenu={() => setMenuOpen(true)}
          onTutorialStep={game.setTutorialStep}
          onSkipTutorial={skipTutorial}
        />
      ) : null;
      break;
    case 'result':
      content = view?.result ? (
        <Result
          result={view.result}
          opponent={view.opponent}
          onRematch={() => {
            track('rematch_clicked', { previous_result: view.result!.outcome, opponent: view.opponent });
            startRealMatch(view.opponent);
          }}
          onHistory={() => {
            track('hand_history_opened', { source: 'result' });
            setScreen({ name: 'hands', matchId: view.match_id, from: 'result' });
          }}
          onHome={() => {
            game.leaveFinished();
            setJustFinished(true);
            goHome();
          }}
        />
      ) : null;
      break;
    case 'duel':
      content = (
        <Duel
          key={screen.invite ?? (screen.byCode ? 'code' : 'host')}
          duel={duel}
          inviteRoomId={screen.invite}
          enterCode={Boolean(screen.byCode)}
          onOpenInvite={openInvite}
          onRules={() => setRulesOpen(true)}
          onExit={() => {
            clearRoomFromLocation();
            duel.reset();
            setScreen({ name: 'home' });
          }}
        />
      );
      break;
    case 'stats':
      content = archive ? (
        <Stats
          stats={game.stats}
          matches={archive.matches}
          skipped={archive.skipped}
          onBack={goHome}
          onOpenMatch={(m) => {
            track('hand_history_opened', { source: 'stats' });
            setScreen({ name: 'hands', matchId: m.id, from: 'stats' });
          }}
        />
      ) : null;
      break;
    case 'hands': {
      const { matchId, from } = screen;
      const archived = archive?.matches.find((m) => m.id === matchId);
      const subtitle =
        from === 'result' && view?.result
          ? `Итог: ${view.result.final_stacks[0]} : ${view.result.final_stacks[1]}`
          : archived
            ? `${matchLabel(archived)} · ${matchDate(archived)}`
            : undefined;
      content = historyHands ? (
        <History
          title="История раздач"
          subtitle={subtitle}
          hands={historyHands}
          botName={botName(from === 'result' ? view?.opponent : archived?.opponent)}
          onBack={() => setScreen(from === 'result' ? { name: 'result' } : { name: 'stats' })}
          onOpen={(h) => {
            track('hand_replay_opened', { hand_no: h.number, source: from, finished: h.outcome !== null });
            setScreen({ name: 'replay', matchId, hand: h.number, from });
          }}
        />
      ) : null;
      break;
    }
    case 'replay': {
      const { matchId, from } = screen;
      content = replayHand ? (
        <Replay
          hand={replayHand}
          opponent={from === 'result' ? view?.opponent : archive?.matches.find((m) => m.id === matchId)?.opponent}
          load={game.replayHand}
          onClose={({ steps, frames, reachedEnd }) => {
            track('hand_replay_stepped', { steps, frames, reached_end: reachedEnd });
            setScreen({ name: 'hands', matchId, from });
          }}
        />
      ) : null;
      break;
    }
  }

  return (
    <div className="app">
      <div className="column">{content}</div>

      {menuOpen && view && (
        <PauseMenu
          tutorial={view.kind === 'tutorial'}
          onResume={() => setMenuOpen(false)}
          onRules={() => {
            setMenuOpen(false);
            setRulesOpen(true);
          }}
          onSaveExit={() => {
            game.saveAndExit();
            goHome();
          }}
          onQuit={() => {
            setMenuOpen(false);
            if (view.kind === 'tutorial') skipTutorial();
            else setConfirm('quit');
          }}
        />
      )}

      {confirm === 'quit' && (
        <ConfirmDialog
          title="Завершить текущую дуэль?"
          text="Прогресс этого матча будет удалён. Матч будет учтён как прерванный, а не как поражение."
          cancelLabel="Остаться"
          confirmLabel="Завершить"
          onCancel={() => setConfirm(null)}
          onConfirm={() => {
            game.abandon();
            goHome();
          }}
        />
      )}

      {confirm === 'restart' && (
        <ConfirmDialog
          title="Начать новый матч?"
          text="Сохранённый матч будет удалён и учтён как прерванный."
          cancelLabel="Отмена"
          confirmLabel="Начать заново"
          onCancel={() => setConfirm(null)}
          onConfirm={() => {
            setConfirm(null);
            game.discardSaved();
            startRealMatch();
          }}
        />
      )}

      {pickerOpen && (
        <OpponentPicker
          selected={opponent}
          stats={game.stats}
          onClose={() => setPickerOpen(false)}
          onSelect={(id) => {
            setOpponent(id);
            storage.saveOpponent(id);
            track('opponent_selected', { opponent: id });
            setPickerOpen(false);
          }}
        />
      )}

      {rulesOpen && (
        <div className="rules-layer">
          <Rules onClose={() => setRulesOpen(false)} />
        </div>
      )}
    </div>
  );
}
