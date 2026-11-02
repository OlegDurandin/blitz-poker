interface Props {
  code: string;
  onNewGame: () => void;
}

/** Honest recovery screen (spec S07) instead of a blank or looping table. */
export function SaveError({ code, onNewGame }: Props) {
  const incompatible = code === 'incompatible_save';
  return (
    <main className="center-screen">
      <h1 className="center-screen__title">Не удалось продолжить матч</h1>
      <p className="center-screen__text">
        {incompatible
          ? 'Приложение обновилось, и сохранённый матч из прошлой версии больше не открывается.'
          : 'Сохранение матча повреждено и не может быть восстановлено.'}{' '}
        Статистика завершённых матчей не пострадала.
      </p>
      <button className="btn btn--primary btn--hero" onClick={onNewGame}>
        Начать новую игру
      </button>
    </main>
  );
}

export function EngineError({ onRetry }: { onRetry: () => void }) {
  return (
    <main className="center-screen">
      <h1 className="center-screen__title">Не удалось загрузить игру</h1>
      <p className="center-screen__text">Проверьте соединение и попробуйте ещё раз.</p>
      <button className="btn btn--primary btn--hero" onClick={onRetry}>
        Повторить
      </button>
    </main>
  );
}
