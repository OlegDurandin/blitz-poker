import type { ActionKind, Street } from '../engine/types';
import { actionLabel, STREET_NAME } from '../game/format';

export function HandHistory({ actions, title = 'История раздачи' }: { actions: { name: string; kind: ActionKind; street: Street; amount: number; to: number; all_in: boolean }[]; title?: string }) {
  return <details className="hand-history"><summary>{title}</summary>
    <ol>{actions.map((a, i) => <li key={i}><span>{STREET_NAME[a.street]} · {a.name}</span><strong>{actionLabel({ ...a, seat: 'human' })}</strong></li>)}</ol>
  </details>;
}
