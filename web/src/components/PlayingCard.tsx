import type { Card } from '../engine/types';
import { SUIT_SYMBOL, cardName, isRed, rankLabel, suitOf } from '../game/format';

export type CardSize = 'xs' | 'sm' | 'md' | 'lg';

interface Props {
  card?: Card | null;
  /** Face down (opponent) or an empty board slot when `card` is absent. */
  faceDown?: boolean;
  size?: CardSize;
  /** Part of my / the opponent's best five at showdown (cyan / amber marks). */
  bestMine?: boolean;
  bestTheirs?: boolean;
  dim?: boolean;
  marks?: { label: string; name: string; color: string }[];
}

export function PlayingCard({ card, faceDown, size = 'md', bestMine, bestTheirs, dim, marks }: Props) {
  if (faceDown) {
    return <div className={`card card--${size} card--back`} role="img" aria-label="Закрытая карта" />;
  }
  if (!card) {
    return <div className={`card card--${size} card--slot`} aria-hidden="true" />;
  }
  const classes = [
    'card',
    `card--${size}`,
    isRed(card) ? 'card--red' : 'card--black',
    bestMine && 'card--best-mine',
    bestTheirs && 'card--best-theirs',
    dim && 'card--dim',
  ]
    .filter(Boolean)
    .join(' ');
  let label = cardName(card);
  if (bestMine) label += ', в вашей комбинации';
  if (bestTheirs) label += ', в комбинации соперника';
  if (marks?.length) label += ', в комбинации: ' + marks.map(m => m.name).join(', ');
  const suit = SUIT_SYMBOL[suitOf(card)];
  return (
    <div className={classes} role="img" aria-label={label}>
      <span className="card__rank" aria-hidden="true">
        {rankLabel(card)}
      </span>
      <span className="card__suit-small" aria-hidden="true">
        {suit}
      </span>
      <span className="card__suit" aria-hidden="true">
        {suit}
      </span>
      {marks && <span className="card__participants" aria-hidden="true">{marks.map(m => <i key={m.label} style={{ backgroundColor: m.color }}>{m.label}</i>)}</span>}
      {bestMine && <span className="card__mark card__mark--mine" aria-hidden="true" />}
      {bestTheirs && <span className="card__mark card__mark--theirs" aria-hidden="true" />}
    </div>
  );
}
