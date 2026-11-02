import { useState } from 'react';
import type { PresetKind, RaiseOption } from '../engine/types';
import { Dialog } from './Dialog';

const PRESET_LABEL: Record<PresetKind, string> = {
  half_pot: '½ банка',
  pot: 'Банк',
  all_in: 'All-in',
};

interface Props {
  option: RaiseOption;
  onConfirm: (amount: number) => void;
  onCancel: () => void;
}

function clamp(n: number, o: RaiseOption): number {
  return Math.min(o.max_to, Math.max(o.min_to, Math.round(n)));
}

/**
 * Bet/raise size picker (spec S04). Bounds and presets come from the engine;
 * the sheet only constrains input to them. Amounts are "raise to" totals.
 */
export function RaiseSheet({ option, onConfirm, onCancel }: Props) {
  const [amount, setAmount] = useState(option.min_to);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState('');
  const [sent, setSent] = useState(false);

  const isBet = option.kind === 'bet';
  const isAllIn = amount === option.max_to;
  const title = isBet ? `Ставка ${amount}` : `Рейз до ${amount}`;
  const confirmLabel = isAllIn
    ? `Подтвердить олл-ин ${amount}`
    : isBet
      ? `Подтвердить ставку ${amount}`
      : `Подтвердить рейз до ${amount}`;
  const fixed = option.min_to === option.max_to;

  const draftValue = Number(draft);
  const draftValid =
    draft.trim() !== '' &&
    Number.isInteger(draftValue) &&
    draftValue >= option.min_to &&
    draftValue <= option.max_to;

  const confirm = () => {
    if (sent) return;
    const value = editing ? (draftValid ? draftValue : null) : amount;
    if (value === null) return;
    setSent(true);
    onConfirm(value);
  };

  return (
    <Dialog title={isBet ? 'Размер ставки' : 'Размер рейза'} onClose={onCancel}>
      <div className="raise">
        {editing ? (
          <label className="raise__manual">
            <span className="raise__hint">
              От {option.min_to} до {option.max_to}
            </span>
            <input
              className="raise__input"
              type="number"
              inputMode="numeric"
              min={option.min_to}
              max={option.max_to}
              value={draft}
              autoFocus
              onChange={(e) => setDraft(e.target.value)}
              onBlur={() => {
                if (draftValid) setAmount(draftValue);
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && draftValid) {
                  setAmount(draftValue);
                  setEditing(false);
                }
              }}
              aria-invalid={!draftValid}
              aria-label={`Сумма, от ${option.min_to} до ${option.max_to}`}
            />
            {!draftValid && draft !== '' && (
              <span className="raise__error" role="alert">
                Допустимо от {option.min_to} до {option.max_to}
              </span>
            )}
          </label>
        ) : (
          <button
            className="raise__title"
            onClick={() => {
              setDraft(String(amount));
              setEditing(true);
            }}
            aria-label={`${title}. Нажмите, чтобы ввести сумму вручную`}
          >
            {title}
          </button>
        )}

        {!fixed && !editing && (
          <div className="raise__slider-row">
            <button
              className="btn btn--round"
              onClick={() => setAmount((a) => clamp(a - option.step, option))}
              disabled={amount <= option.min_to}
              aria-label={`Уменьшить на ${option.step}`}
            >
              −
            </button>
            <input
              className="raise__slider"
              type="range"
              min={option.min_to}
              max={option.max_to}
              step={1}
              value={amount}
              onChange={(e) => {
                const raw = Number(e.target.value);
                // Snap to the step grid, but keep both ends reachable.
                const snapped =
                  raw >= option.max_to || raw <= option.min_to
                    ? raw
                    : option.min_to + Math.round((raw - option.min_to) / option.step) * option.step;
                setAmount(clamp(snapped, option));
              }}
              aria-label="Размер ставки"
              aria-valuetext={String(amount)}
            />
            <button
              className="btn btn--round"
              onClick={() => setAmount((a) => clamp(a + option.step, option))}
              disabled={amount >= option.max_to}
              aria-label={`Увеличить на ${option.step}`}
            >
              +
            </button>
          </div>
        )}
        {!editing && (
          <div className="raise__bounds" aria-hidden="true">
            <span>мин. {option.min_to}</span>
            <span>макс. {option.max_to}</span>
          </div>
        )}

        <div className="raise__presets">
          {option.presets.map((p) => (
            <button
              key={p.kind}
              className={`btn btn--chip${!editing && amount === p.to && p.enabled ? ' is-active' : ''}`}
              disabled={!p.enabled}
              onClick={() => {
                setEditing(false);
                setAmount(p.to);
              }}
              aria-label={p.enabled ? `${PRESET_LABEL[p.kind]}: ${p.to}` : `${PRESET_LABEL[p.kind]} недоступно`}
            >
              {PRESET_LABEL[p.kind]}
            </button>
          ))}
        </div>

        <button className="btn btn--primary btn--block" onClick={confirm} disabled={sent || (editing && !draftValid)}>
          {editing && draftValid
            ? isBet
              ? `Подтвердить ставку ${draftValue}`
              : `Подтвердить рейз до ${draftValue}`
            : confirmLabel}
        </button>
        <button className="btn btn--ghost btn--block" onClick={onCancel}>
          Отмена
        </button>
      </div>
    </Dialog>
  );
}
