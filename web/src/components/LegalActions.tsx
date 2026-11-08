import { useEffect, useState } from 'react';
import type { LegalActions as Legal, PlayerAction } from '../engine/types';
import { RaiseSheet } from './RaiseSheet';

/** Only displays actions supplied by the engine/server. */
export function LegalActions({ legal, disabled, turnKey, onAct }: { legal: Legal; disabled: boolean; turnKey: string; onAct: (a: PlayerAction) => void }) {
  const [raise, setRaise] = useState(false);
  useEffect(() => { setRaise(false); }, [turnKey, disabled]);
  const act = (a: PlayerAction) => { if (!disabled) { setRaise(false); onAct(a); } };
  const any = legal.fold || legal.check || legal.call !== null || legal.raise;
  return <>
    <div className="actions" aria-label="Ваши действия" aria-busy={disabled}>
      {legal.fold && <button className="btn btn--secondary btn--action" disabled={disabled} onClick={() => act({ type: 'fold' })}>Сброс</button>}
      {legal.check && <button className="btn btn--primary btn--action" disabled={disabled} onClick={() => act({ type: 'check' })}>Чек</button>}
      {legal.call !== null && <button className="btn btn--primary btn--action" disabled={disabled} onClick={() => act({ type: 'call' })}>{legal.call_is_all_in ? 'All-in' : 'Колл'} {legal.call}</button>}
      {legal.raise && <button className="btn btn--primary btn--action" disabled={disabled} aria-haspopup="dialog" onClick={() => setRaise(true)}>{legal.raise.kind === 'bet' ? 'Ставка…' : 'Рейз…'}</button>}
      {!any && <span className="actions--waiting">Нет доступных действий</span>}
    </div>
    {raise && !disabled && legal.raise && <RaiseSheet key={turnKey} option={legal.raise} onCancel={() => setRaise(false)} onConfirm={amount => act({ type: 'raise_to', amount })} />}
  </>;
}
