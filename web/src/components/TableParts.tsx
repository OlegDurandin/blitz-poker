// Small pieces of the table shared by the live table and the hand replay.

import type { ActionRecord } from '../engine/types';
import { actionLabel, signed } from '../game/format';
import { ChipIcon } from './icons';

export function DealerButton() {
  return (
    <span className="dealer" title="Баттон (малый блайнд)" aria-label="Баттон">
      D
    </span>
  );
}

export function Stack({ amount, gain }: { amount: number; gain: number }) {
  return (
    <div className="seat__stack" aria-label={`Стек ${amount}`}>
      <ChipIcon />
      <span>{amount}</span>
      {gain > 0 && (
        <span className="gain" aria-label={`выигрыш ${gain}`}>
          {signed(gain)}
        </span>
      )}
    </div>
  );
}

export function BetChip({ amount, who }: { amount: number; who: string }) {
  return (
    <div className={`bet${amount > 0 ? '' : ' is-empty'}`} aria-label={amount > 0 ? `Ставка: ${who}, ${amount}` : undefined}>
      <ChipIcon />
      {amount}
    </div>
  );
}

export function ActionTag({ action, thinking }: { action: ActionRecord | null; thinking?: boolean }) {
  if (thinking) return <div className="tag tag--thinking">думает…</div>;
  if (!action) return <div className="tag is-empty" />;
  return <div className={`tag tag--${action.kind}`}>{actionLabel(action)}</div>;
}
