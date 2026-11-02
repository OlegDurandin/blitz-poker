import { useEffect, useRef } from 'react';
import { PlayingCard } from '../components/PlayingCard';
import type { Category } from '../engine/types';
import { CATEGORY_NAME } from '../game/format';

const EXAMPLES: Array<[Category, string[], string]> = [
  ['royal_flush', ['As', 'Ks', 'Qs', 'Js', 'Ts'], 'Туз, король, дама, валет, десятка одной масти'],
  ['straight_flush', ['9h', '8h', '7h', '6h', '5h'], 'Пять карт подряд одной масти'],
  ['four_of_a_kind', ['7s', '7h', '7d', '7c', 'Kd'], 'Четыре карты одного достоинства'],
  ['full_house', ['Ks', 'Kh', 'Kd', '4c', '4s'], 'Тройка и пара'],
  ['flush', ['Ah', 'Jh', '9h', '6h', '2h'], 'Пять карт одной масти'],
  ['straight', ['Ts', '9d', '8c', '7h', '6s'], 'Пять карт подряд; туз может быть и младшей картой'],
  ['three_of_a_kind', ['Qs', 'Qh', 'Qd', '8c', '3s'], 'Три карты одного достоинства'],
  ['two_pair', ['Js', 'Jd', '5c', '5h', 'Ad'], 'Две разные пары'],
  ['pair', ['As', 'Ah', 'Kd', '8c', '4s'], 'Две карты одного достоинства'],
  ['high_card', ['Ad', 'Js', '8h', '6c', '3d'], 'Ничего из перечисленного: решает старшая карта'],
];

/** Compact reference (spec S08). Closes back into the same context. */
export function Rules({ onClose }: { onClose: () => void }) {
  const closeRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    closeRef.current?.focus();
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  return (
    <div className="rules" role="dialog" aria-modal="true" aria-label="Правила и комбинации">
      <header className="screen-header">
        <button ref={closeRef} className="btn btn--ghost btn--small" onClick={onClose} aria-label="Закрыть правила">
          ← Назад
        </button>
        <h1 className="screen-header__title">Правила</h1>
      </header>

      <section>
        <h2>Как идёт раздача</h2>
        <p>
          У каждого две закрытые карты. На стол по очереди открываются пять общих: <b>флоп</b> (три карты),{' '}
          <b>тёрн</b> (четвёртая) и <b>ривер</b> (пятая). Торговля идёт до флопа (<b>префлоп</b>) и после каждой
          улицы.
        </p>
        <p>Побеждает лучшая комбинация из пяти карт — любые пять из ваших двух и пяти общих.</p>
      </section>

      <section>
        <h2>Комбинации — от старшей к младшей</h2>
        <ol className="rules__combos">
          {EXAMPLES.map(([cat, cards, text]) => (
            <li key={cat}>
              <div className="rules__combo-head">
                <b>{CATEGORY_NAME[cat]}</b>
                <span>{text}</span>
              </div>
              <div className="rules__cards">
                {cards.map((c) => (
                  <PlayingCard key={c} card={c} size="xs" />
                ))}
              </div>
            </li>
          ))}
        </ol>
      </section>

      <section>
        <h2>Термины</h2>
        <dl className="rules__terms">
          <dt>Блайнды</dt>
          <dd>Обязательные ставки до раздачи: малый и большой. Задают размер игры.</dd>
          <dt>Баттон (D)</dt>
          <dd>Один на один баттон ставит малый блайнд, ходит первым до флопа и последним после.</dd>
          <dt>Банк</dt>
          <dd>Фишки, собранные со всех завершённых кругов торговли.</dd>
          <dt>Чек</dt>
          <dd>Пропустить ход, когда ставить ничего не нужно.</dd>
          <dt>Колл</dt>
          <dd>Уравнять ставку соперника.</dd>
          <dt>Ставка / рейз</dt>
          <dd>Поставить первым на улице / повысить ставку соперника.</dd>
          <dt>Сброс</dt>
          <dd>Отказаться от раздачи; банк забирает соперник.</dd>
          <dt>Олл-ин</dt>
          <dd>Поставить все свои фишки. Если ставки закончились, оставшиеся карты откроются сами.</dd>
        </dl>
      </section>

      <section>
        <h2>Матч Blitz8</h2>
        <ul className="rules__list">
          <li>У вас и у бота по 600 фишек. Максимум 8 раздач.</li>
          <li>Блайнды растут каждые две раздачи: 10/20, 15/30, 20/40, 30/60.</li>
          <li>Баттон переходит каждую раздачу, так что у обоих поровну баттонов.</li>
          <li>После 8-й раздачи побеждает больший стек; при равенстве — ничья. Если фишки закончились, матч завершается досрочно.</li>
          <li>Стеки обнуляются с каждым матчем: фишки виртуальные, не покупаются и не выводятся.</li>
        </ul>
      </section>
    </div>
  );
}
