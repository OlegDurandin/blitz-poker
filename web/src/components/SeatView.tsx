import type { ReactNode } from 'react';
import { BotAvatar } from './icons';
import { Stack } from './TableParts';

export interface SeatViewProps {
  name: string;
  photo?: string | null;
  botColor?: string;
  stack?: number;
  gain?: number;
  active?: boolean;
  folded?: boolean;
  allIn?: boolean;
  disconnected?: boolean;
  eliminated?: boolean;
  gone?: boolean;
  mine?: boolean;
  winner?: boolean;
  host?: boolean;
  badges?: string[];
  /** Time belongs to this seat only when active. Never infer whose turn it is. */
  leftMs?: number | null;
  action?: string | null;
  cards?: ReactNode;
  children?: ReactNode;
}

export function seatStatus(p: SeatViewProps): string {
  if (p.gone) return 'Ушёл';
  if (p.disconnected) return 'Нет связи';
  if (p.eliminated) return 'Выбыл';
  if (p.folded) return 'Сбросил';
  if (p.allIn) return 'All-in';
  if (p.active) return p.mine ? 'Ваш ход' : 'Ходит';
  return p.action ?? '';
}

export function SeatView(p: SeatViewProps) {
  const status = seatStatus(p);
  // Live seats keep their geometry while actions, bets and visibility change.
  const playing = p.stack !== undefined;
  const active = p.active && !p.disconnected && !p.gone && !p.folded && !p.eliminated && !p.allIn;
  const classes = ['seat-view', p.mine && 'seat-view--mine', active && 'is-active', p.folded && 'is-folded', p.eliminated && 'is-eliminated', p.disconnected && 'is-disconnected', p.winner && 'is-winner'].filter(Boolean).join(' ');
  return (
    <section className={classes} aria-label={`${p.name}${p.stack !== undefined ? `, стек ${p.stack}` : ''}${status ? `, ${status}` : ''}`}>
      {p.mine && (playing || p.cards) && <div className="seat-view__cards seat-view__cards--mine">{p.cards}</div>}
      <div className="seat-view__plate">
        <div className="seat-view__avatar" aria-hidden="true">
          {p.photo ? <img src={p.photo} alt="" /> : p.botColor ? <BotAvatar color={p.botColor} /> : <span>{p.mine ? 'Я' : p.name.slice(0, 1).toUpperCase()}</span>}
          {p.winner && <span className="seat-view__crown">★</span>}
        </div>
        <div className="seat-view__info">
          <div className="seat-view__name" title={p.name}>{p.name.replace(/^Бот /, '')}</div>
          {p.stack !== undefined && <Stack amount={p.stack} gain={p.gain ?? 0} />}
          {p.host && <span className="seat-view__host">Хозяин</span>}
        </div>
        {!!p.badges?.length && <span className="seat-view__badges">{p.badges.map(b => <abbr key={b} title={b === 'D' ? 'Дилер' : b === 'SB' ? 'Малый блайнд' : 'Большой блайнд'}>{b}</abbr>)}</span>}
        {active && p.leftMs != null && <span className={`seat-view__clock${p.leftMs <= 5000 ? ' is-low' : ''}`} aria-label={`Осталось ${Math.max(0, Math.ceil(p.leftMs / 1000))} секунд`}>{Math.max(0, Math.ceil(p.leftMs / 1000))}с</span>}
      </div>
      {(playing || status) && <span className="seat-view__status" title={status || undefined} aria-hidden={!status || undefined}>{p.disconnected && <span aria-hidden="true">↯ </span>}{status}</span>}
      {!p.mine && (playing || p.cards) && <div className="seat-view__cards">{p.cards}</div>}
      {playing ? <div className="seat-view__bet">{p.children}</div> : p.children}
    </section>
  );
}

/** Rotate presentation only; all callbacks and engine indices stay absolute. */
export function relativeSeats(count: number, me: number): number[] {
  return Array.from({ length: count - 1 }, (_, i) => (me + i + 1) % count);
}

export function TableArena({ opponents, hero, children, lobby = false }: { opponents: ReactNode[]; hero: ReactNode; children: ReactNode; lobby?: boolean }) {
  return <div className={`table-arena table-arena--${opponents.length + 1}${lobby ? ' table-arena--lobby' : ''}`}>
    {opponents.map((seat, i) => <div className={`arena-seat arena-seat--${i}`} key={i}>{seat}</div>)}
    <div className="arena-center">{children}</div>
    <div className="arena-hero">{hero}</div>
  </div>;
}
