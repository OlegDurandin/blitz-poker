// "Третий не лишний" with friends: a table of 2–4 run by the server. The
// host gathers friends by link or code and may seat bots in free seats;
// everyone plays from their own phone. All state comes from snapshots.

import { useEffect, useRef, useState } from 'react';
import { ConfirmDialog } from '../components/Dialog';
import { SeatView, TableArena, relativeSeats } from '../components/SeatView';
import { FeedbackSettings } from '../components/FeedbackSettings';
import { BotAvatar } from '../components/icons';
import type { Opponent } from '../engine/types';
import { track } from '../game/analytics';
import { OPPONENTS, opponentInfo } from '../game/opponents';
import { formatCode } from '../pvp/config';
import type { TableRoomView, TableSeatView } from '../pvp/types';
import type { Duel as DuelController } from '../pvp/useDuel';
import { platform } from '../platform';
import { Avatar, Center, useTicker } from './Duel';
import { Ring, type Person } from './Ring';
import { RingResult } from './RingResult';
import { formatClock } from './Table';

interface Props {
  duel: DuelController;
  /** Opened from a link or code; otherwise this player gathers a table. */
  inviteId: string | null;
  /** Seats for a new table (host). */
  seats: number;
  solo?: Opponent;
  onLocalFallback?: () => void;
  onExit: () => void;
  onRules: () => void;
}

function people(t: TableRoomView): (Person | null)[] {
  return t.seats.map((s) =>
    s.type === 'player' ? { name: s.name, photo: s.photo, connected: s.connected, gone: s.gone } : null,
  );
}

const CLOSED_TEXT: Record<string, { title: string; text: string }> = {
  cancelled_by_host: { title: 'Стол закрыт', text: 'Хозяин стола отменил игру.' },
  expired: { title: 'Стол закрыт', text: 'Долго ничего не происходило, и стол закрылся.' },
  empty: { title: 'Стол закрыт', text: 'Все игроки ушли из-за стола.' },
};

export function TableOnline({ duel, inviteId, seats, solo, onLocalFallback, onExit, onRules }: Props) {
  const { state } = duel;
  const table = state.table;
  const preview = state.tablePreview;
  const asked = useRef(false);
  const rejoined = useRef<string | null>(null);
  const [menu, setMenu] = useState(false);
  const [confirm, setConfirm] = useState<'leave' | 'cancel' | null>(null);
  const [flash, setFlash] = useState<string | null>(null);
  const [botsOpen, setBotsOpen] = useState(false);
  const [slow, setSlow] = useState(false);
  useEffect(() => {
    if (!solo || table) return;
    const timer = window.setTimeout(() => setSlow(true), 8000);
    return () => window.clearTimeout(timer);
  }, [solo, table]);
  const fallback = solo && !table && onLocalFallback ? <div className="solo-fallback"><p>Локальная игра доступна без сервера. Её результаты не засчитываются в задачи дня. Она не продолжает уже начатую сетевую партию.</p><button className="btn btn--secondary btn--block" onClick={onLocalFallback}>Играть локально</button></div> : null;
  useTicker(Boolean(table && table.status !== 'closed'));
  const offline = state.conn !== 'online';

  // Once online: gather a table (host) or look at the one we were sent to.
  useEffect(() => {
    if (state.conn !== 'online' || asked.current || table) return;
    asked.current = true;
    if (solo) duel.createSolo(solo);
    else if (!inviteId) duel.createTable(seats);
    else if (preview?.table_id !== inviteId) duel.peek(inviteId);
  }, [state.conn, table, inviteId, seats, solo, preview, duel]);

  // Our own seat (a reload, the host's own link): take it back, once.
  useEffect(() => {
    if (!preview?.already_member || table || rejoined.current === preview.table_id) return;
    rejoined.current = preview.table_id;
    if (preview.status === 'closed') onExit();
    else duel.joinTable(preview.table_id);
  }, [preview, table, duel, onExit]);

  const say = (text: string) => {
    setFlash(text);
    window.setTimeout(() => setFlash(null), 2500);
  };
  const exit = () => {
    duel.leaveTable();
    onExit();
  };

  if (state.conn === 'failed') {
    return (
      <Center title="Не удалось подключиться" text={state.connError ?? 'Попробуйте открыть игру заново.'}>
        <button className="btn btn--primary btn--block" onClick={duel.reconnect}>
          Повторить подключение
        </button>
        {fallback}
        <button className="btn btn--secondary btn--block" onClick={onExit}>
          На главную
        </button>
      </Center>
    );
  }
  if (state.conn === 'replaced') {
    return (
      <Center title="Игра открыта на другом устройстве" text="Здесь стол остановлен, чтобы не было двух игроков с одного аккаунта.">
        <button className="btn btn--primary btn--block" onClick={duel.reconnect}>
          Играть здесь
        </button>
        <button className="btn btn--ghost btn--block" onClick={onExit}>
          На главную
        </button>
      </Center>
    );
  }

  if (!table) {
    const err = state.error && state.error.code !== 'offline' ? state.error : null;
    if (inviteId && err && ['room_not_found', 'room_closed', 'room_full'].includes(err.code)) {
      return (
        <Center
          title={err.code === 'room_full' ? 'За столом нет мест' : 'Приглашение больше не действует'}
          text={err.code === 'room_full' ? 'Все места уже заняты или игра началась.' : 'Попросите друга прислать новую ссылку.'}
        >
          <button className="btn btn--primary btn--block" onClick={onExit}>
            На главную
          </button>
        </Center>
      );
    }
    if (inviteId && preview && !preview.already_member) {
      if (!preview.joinable) {
        return (
          <Center
            title={preview.status === 'closed' ? 'Приглашение больше не действует' : 'За столом нет мест'}
            text={preview.status === 'playing' ? 'Игра уже идёт. Попросите позвать вас в следующий раз.' : 'Попросите друга прислать новую ссылку.'}
          >
            <button className="btn btn--primary btn--block" onClick={onExit}>
              На главную
            </button>
          </Center>
        );
      }
      return (
        <main className="duel duel--invite">
          <Avatar slot={{ name: preview.host_name, photo: preview.host_photo }} size={88} />
          <h1 className="duel__title">{preview.host_name} зовёт за стол</h1>
          <p className="duel__text">
            «Третий не лишний»: стол на {preview.seats}, занято {preview.taken}. У каждого по 600 условных фишек,
            побеждает тот, у кого к концу больше всех. Фишки виртуальные, на деньги не играем.
          </p>
          <div className="duel__actions">
            <button className="btn btn--primary btn--hero" disabled={offline} onClick={() => duel.joinTable(preview.table_id)}>
              Сесть за стол
            </button>
            <button className="btn btn--ghost btn--block" onClick={onExit}>
              Отказаться
            </button>
          </div>
        </main>
      );
    }
    if (err) {
      return (
        <Center title="Не получилось" text={err.message}>
          {solo && err.code === 'persistence_pending' && <button className="btn btn--primary btn--block" onClick={duel.reconnect}>Повторить подключение</button>}
          {fallback}
          <button className="btn btn--primary btn--block" onClick={onExit}>
            На главную
          </button>
        </Center>
      );
    }
    return (
      <main className="center-screen" aria-busy="true">
        <div className="loader" aria-label="Подключение" />
        {slow && fallback}
        <p className="center-screen__text">{offline ? 'Подключаемся к серверу…' : 'Готовим стол…'}</p>
        <button className="btn btn--ghost btn--block" onClick={onExit}>
          Отмена
        </button>
      </main>
    );
  }

  const now = duel.serverNow();

  if (table.status === 'closed') {
    const text = CLOSED_TEXT[table.close_reason ?? 'expired'] ?? CLOSED_TEXT.expired;
    return (
      <Center title={text.title} text={text.text}>
        <button className="btn btn--primary btn--block" onClick={exit}>
          На главную
        </button>
      </Center>
    );
  }

  const game = table.game;
  if (table.status === 'finished' && game?.view.result) {
    return (
      <RingResult
        view={game.view}
        online={{ people: people(table), again: table.again }}
        onAgain={() => {
          track('duel_rematch_requested', { table: true });
          duel.tableAgain();
        }}
        onHome={exit}
      />
    );
  }

  if (table.status === 'playing' && game) {
    const clock = game.clock;
    const gone = table.seats[table.my_seat]?.type === 'player' && (table.seats[table.my_seat] as { gone: boolean }).gone;
    return (
      <>
        {offline && <div className="notice notice--floating">Нет соединения, переподключаемся…</div>}
        <Ring
          view={game.view}
          paused={false}
          notice={state.error && state.error.code !== 'offline' ? state.error.message : null}
          onRetrySave={duel.clearError}
          onAct={duel.tableAct}
          onMenu={() => setMenu(true)}
          online={{
            people: people(table),
            clock: clock ? { seat: clock.seat, leftMs: clock.deadline_ms !== null ? clock.deadline_ms - now : null } : null,
            nextHandInMs: game.next_hand_at_ms ? game.next_hand_at_ms - now : null,
            pending: state.pending || offline || gone,
            offline,
          }}
        />
        {menu && (
          <div className="overlay" onClick={() => setMenu(false)}>
            <div className="surface surface--sheet" role="dialog" aria-modal="true" aria-label="Меню" onClick={(e) => e.stopPropagation()}>
              <h2 className="surface__title">Стол</h2>
              <FeedbackSettings />
              <div className="menu">
                <button className="btn btn--primary btn--block" onClick={() => setMenu(false)}>
                  Вернуться к игре
                </button>
                <button
                  className="btn btn--secondary btn--block"
                  onClick={() => {
                    setMenu(false);
                    onRules();
                  }}
                >
                  Правила
                </button>
                <button
                  className="btn btn--danger-text btn--block"
                  onClick={() => {
                    setMenu(false);
                    setConfirm('leave');
                  }}
                >
                  Уйти из-за стола
                </button>
              </div>
            </div>
          </div>
        )}
        {confirm === 'leave' && (
          <ConfirmDialog
            title="Уйти из-за стола?"
            text="Ваши карты сбрасываются, и до конца матча вы не играете. Если просто закрыть приложение, у вас будет минута, чтобы вернуться."
            cancelLabel="Играть дальше"
            confirmLabel="Уйти"
            onCancel={() => setConfirm(null)}
            onConfirm={() => {
              setConfirm(null);
              exit();
            }}
          />
        )}
      </>
    );
  }

  // Lobby: gather friends, seat bots, start.
  const link = platform.inviteLink(table.table_id);
  const taken = table.seats.filter((s) => s.type !== 'empty').length;
  const free = table.seats.length - taken;
  const seatedBots = new Set(table.seats.flatMap((s) => (s.type === 'bot' ? [s.opponent] : [])));
  const expiresIn = table.expires_at_ms ? table.expires_at_ms - now : null;
  const invite = async () => {
    if (!link) { say(`Код: ${formatCode(table.code)}`); return; }
    const shared = await platform.share(link);
    track('duel_invite_shared', { native: shared, table: true });
    if (!shared) say((await platform.copy(link)) ? 'Ссылка скопирована' : link);
  };
  const renderLobbySeat = (index: number) => {
    const seat = table.seats[index];
    return <LobbySeat key={index} seat={seat} offline={offline} canRemove={table.you_are_host && seat.type === 'bot'} canAdd={table.you_are_host} onRemove={() => duel.removeBot(index)} onAdd={() => setBotsOpen(true)} onInvite={() => void invite()} />;
  };
  return (
    <main className="duel duel--lobby">
      <header className="screen-header">
        <button
          className="btn btn--ghost btn--small"
          onClick={() => {
            // Nobody else here yet: keep the table so a link already sent works.
            if (table.you_are_host && taken === 1) onExit();
            else if (table.you_are_host) setConfirm('cancel');
            else exit();
          }}
        >
          ← {table.you_are_host && taken > 1 ? 'Отменить' : table.you_are_host ? 'Назад' : 'Выйти'}
        </button>
        <h1 className="screen-header__title">Ваш стол</h1>
      </header>

      {offline && <div className="notice">Нет соединения, переподключаемся…</div>}

      <div className="lobby-scroll">
      <TableArena lobby opponents={relativeSeats(table.seats.length, table.my_seat).map(renderLobbySeat)} hero={renderLobbySeat(table.my_seat)}>
        <p className="lobby-state">{free > 0 ? 'Собираем компанию' : 'Все за столом'}</p>
        <strong className="lobby-count">{taken} из {table.seats.length}</strong>
        <span className="felt__street">{table.you_are_host ? 'Вы решаете, когда начать' : 'Ждём начала игры'}</span>
      </TableArena>
      {botsOpen && table.you_are_host && free > 0 && (
        <section className="lobby-bots" aria-label="Добавить бота">
          <div className="lobby-bots__heading"><h2>Кого посадим?</h2><button className="btn btn--ghost btn--small" onClick={() => setBotsOpen(false)}>Закрыть</button></div>
          <div className="lobby-bots__list">
            {OPPONENTS.filter(o => !seatedBots.has(o.id)).map(o => <button key={o.id} className="lobby-bots__add" disabled={offline} onClick={() => { duel.addBot(o.id as Opponent); setBotsOpen(false); }} aria-label={`Добавить бота ${o.name}, ${o.style}`}><BotAvatar color={o.color} /><span>{o.name.replace(/^Бот /, '')}</span></button>)}
          </div>
        </section>
      )}

      <section className="duel__invite">
          <button
            className="duel-code"
            onClick={async () => {
              track('duel_code_copied', { table: true });
              say((await platform.copy(table.code)) ? 'Код скопирован' : `Код: ${formatCode(table.code)}`);
            }}
            aria-label={`Код стола ${formatCode(table.code)}, нажмите, чтобы скопировать`}
          >
            <span className="duel-code__label">Код приглашения</span>
            <span className="duel-code__digits">{formatCode(table.code)}</span>
            <span className="duel-code__copy">Скопировать</span>
          </button>
        </section>

      </div>
      <div className="lobby-controls">
          {link && free > 0 && (
            <button
              className="btn btn--primary btn--block"
              disabled={offline}
              onClick={async () => {
                const shared = await platform.share(link);
                track('duel_invite_shared', { native: shared, table: true });
                if (!shared) say((await platform.copy(link)) ? 'Ссылка скопирована' : link);
              }}
            >
              Пригласить друга
            </button>
          )}

      {table.you_are_host ? (
        <div className="lobby-start">
          <button className="btn btn--secondary btn--hero btn--block" disabled={offline || taken < 2} onClick={duel.startTable}>Начать игру{taken >= 2 ? ` · ${taken} игрока` : ''}</button>
          {taken < 2 && <p className="duel__hint">Добавьте ещё одного игрока или бота</p>}
        </div>
      ) : (
        <p className="duel__text">Ждём, когда хозяин стола начнёт игру…</p>
      )}

      </div>

      {expiresIn !== null && expiresIn < 10 * 60_000 && (
        <p className="duel__hint">Стол закроется через {formatClock(expiresIn)}, если ничего не происходит</p>
      )}
      {flash && (
        <p className="duel__flash" role="status">
          {flash}
        </p>
      )}
      {state.error && state.error.code !== 'offline' && <p className="duel__error">{state.error.message}</p>}

      {confirm === 'cancel' && (
        <ConfirmDialog
          title="Отменить стол?"
          text="Друзья увидят, что стол закрыт."
          cancelLabel="Остаться"
          confirmLabel="Отменить стол"
          onCancel={() => setConfirm(null)}
          onConfirm={() => {
            setConfirm(null);
            exit();
          }}
        />
      )}
    </main>
  );
}

export function LobbySeat({ seat, canRemove, canAdd, offline, onRemove, onAdd, onInvite }: { seat: TableSeatView; canRemove: boolean; canAdd: boolean; offline: boolean; onRemove: () => void; onAdd: () => void; onInvite: () => void }) {
  if (seat.type === 'empty') return <section className="empty-seat" aria-label="Свободное место">
    <span className="empty-seat__icon" aria-hidden="true">+</span><strong>Свободно</strong>
    {canAdd && <button disabled={offline} onClick={onAdd}>Добавить бота</button>}
    <button disabled={offline} onClick={onInvite}>Пригласить</button>
  </section>;
  if (seat.type === 'bot') {
    const bot = opponentInfo(seat.opponent);
    return <SeatView name={bot.name} botColor={bot.color} action="Бот · готов">
      {canRemove && <button className="seat-remove" disabled={offline} onClick={onRemove} aria-label={`Убрать бота ${bot.name}`}>Убрать</button>}
    </SeatView>;
  }
  return <SeatView name={seat.is_me ? 'Вы' : seat.name} mine={seat.is_me} photo={seat.photo} host={seat.is_host} disconnected={!seat.connected} gone={seat.gone} action="За столом" />;
}
