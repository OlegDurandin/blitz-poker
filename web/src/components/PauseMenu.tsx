import { Dialog } from './Dialog';

interface Props {
  tutorial: boolean;
  onResume: () => void;
  onRules: () => void;
  onSaveExit: () => void;
  onQuit: () => void;
  /** Overrides "Завершить матч" (e.g. for a table). */
  quitLabel?: string;
}

export function PauseMenu({ tutorial, onResume, onRules, onSaveExit, onQuit, quitLabel }: Props) {
  return (
    <Dialog title="Пауза" onClose={onResume}>
      <h2 className="surface__title">Пауза</h2>
      <div className="menu">
        <button className="btn btn--primary btn--block" onClick={onResume}>
          Продолжить
        </button>
        <button className="btn btn--secondary btn--block" onClick={onRules}>
          Правила и комбинации
        </button>
        <button className="btn btn--secondary btn--block" onClick={onSaveExit}>
          Сохранить и выйти
        </button>
        <button className="btn btn--ghost btn--block btn--danger-text" onClick={onQuit}>
          {tutorial ? 'Пропустить обучение' : (quitLabel ?? 'Завершить матч')}
        </button>
      </div>
    </Dialog>
  );
}
