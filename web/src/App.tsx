import { useCallback, useEffect, useState } from 'react';
import { ConfirmDialog } from './components/Dialog';
import { PauseMenu } from './components/PauseMenu';
import { engine, initEngine } from './engine/engine';
import { track } from './game/analytics';
import { completedMatches, storage } from './game/storage';
import { useGame } from './game/useGame';
import { History } from './screens/History';
import { Home } from './screens/Home';
import { Result } from './screens/Result';
import { Rules } from './screens/Rules';
import { EngineError, SaveError } from './screens/SaveError';
import { Table } from './screens/Table';

type Screen =
  | { name: 'loading' }
  | { name: 'home' }
  | { name: 'table' }
  | { name: 'result' }
  | { name: 'history' }
  | { name: 'save_error'; code: string }
  | { name: 'engine_error' };

type Confirm = 'quit' | 'restart' | null;

export function App() {
  const [screen, setScreen] = useState<Screen>({ name: 'loading' });
  const [rulesOpen, setRulesOpen] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [confirm, setConfirm] = useState<Confirm>(null);
  const [justFinished, setJustFinished] = useState(false);
  const game = useGame({ paused: menuOpen || rulesOpen || confirm !== null });

  const boot = useCallback(() => {
    setScreen({ name: 'loading' });
    initEngine()
      .then(() => {
        const saved = storage.loadMatch();
        track('app_open', {
          has_saved_match: saved.kind === 'ok',
          completed_matches: completedMatches(storage.loadStats()),
        });
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

  // Saves already happen after every action; this covers the app going to background.
  useEffect(() => {
    const onHide = () => document.visibilityState === 'hidden' && game.retrySave();
    document.addEventListener('visibilitychange', onHide);
    return () => document.removeEventListener('visibilitychange', onHide);
  }, [game.retrySave]);

  const toTable = (ok: unknown) => ok && setScreen({ name: 'table' });

  const startRealMatch = () => toTable(game.startMatch());

  const onStart = () => {
    const firstLaunch = !storage.isTutorialDone();
    track('start_click', { first_launch: firstLaunch });
    toTable(firstLaunch ? game.startTutorial() : game.startMatch());
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

  // Never leave the user on an empty screen without a way back.
  const orphaned =
    (screen.name === 'table' && !view) ||
    (screen.name === 'result' && !view?.result) ||
    (screen.name === 'history' && !view);
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
          onRematch={() => {
            track('rematch_clicked', { previous_result: view.result!.outcome });
            startRealMatch();
          }}
          onHistory={() => {
            track('hand_history_opened', { source: 'result' });
            setScreen({ name: 'history' });
          }}
          onHome={() => {
            game.leaveFinished();
            setJustFinished(true);
            goHome();
          }}
        />
      ) : null;
      break;
    case 'history':
      content = view ? <History hands={view.history} onBack={() => setScreen({ name: 'result' })} /> : null;
      break;
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

      {rulesOpen && (
        <div className="rules-layer">
          <Rules onClose={() => setRulesOpen(false)} />
        </div>
      )}
    </div>
  );
}
