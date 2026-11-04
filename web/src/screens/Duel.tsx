import { useEffect, useRef, useState } from 'react';
import { ConfirmDialog } from '../components/Dialog';
import { ChipIcon, HandshakeIcon, Trophy, CardsIcon } from '../components/icons';
import { PlayingCard } from '../components/PlayingCard';
import { describeHand, plural } from '../game/format';
import { track } from '../game/analytics';
import { inviteLink } from '../pvp/config';
import type { RoomView, SlotView } from '../pvp/types';
import type { Duel as DuelController } from '../pvp/useDuel';
import { copyText, shareLink } from '../vk';
import { Table, formatClock } from './Table';

interface Props {
  duel: DuelController;
  /** Opened from an invite link; otherwise this player creates a room. */
  inviteRoomId: string | null;
  onExit: () => void;
  onRules: () => void;
}

/** Re-renders a few times a second while countdowns are on screen. */
function useTicker(on: boolean) {
  const [, setTick] = useState(0);
  useEffect(() => {
    if (!on) return;
    const t = window.setInterval(() => setTick((n) => n + 1), 250);
    return () => window.clearInterval(t);
  }, [on]);
}

function Avatar({ slot, size = 56 }: { slot: { name: string; photo: string | null } | null; size?: number }) {
  if (slot?.photo) return <img className="duel-avatar" src={slot.photo} alt="" width={size} height={size} />;
  return (
    <span className="duel-avatar duel-avatar--empty" style={{ width: size, height: size }} aria-hidden="true">
      {slot ? slot.name.slice(0, 1).toUpperCase() : '?'}
    </span>
  );
}

function Center({ title, text, children }: { title: string; text?: string; children?: React.ReactNode }) {
  return (
    <main className="center-screen duel">
      <h1 className="center-screen__title">{title}</h1>
      {text && <p className="center-screen__text">{text}</p>}
      <div className="duel__actions">{children}</div>
    </main>
  );
}

export function Duel({ duel, inviteRoomId, onExit, onRules }: Props) {
  const { state } = duel;
  const room = state.room;
  const asked = useRef(false);
  const [confirm, setConfirm] = useState<'surrender' | 'cancel' | null>(null);
  const [menu, setMenu] = useState(false);
  const [flash, setFlash] = useState<string | null>(null);
  useTicker(Boolean(room && room.status !== 'closed'));

  // Once online: create a room (host) or look at the invited one (guest).
  useEffect(() => {
    if (state.conn !== 'online' || asked.current || room) return;
    asked.current = true;
    if (inviteRoomId) duel.peek(inviteRoomId);
    else duel.createRoom();
  }, [state.conn, room, inviteRoomId, duel]);

  // The host's own link, or a reload: take the seat back without asking.
  const preview = state.preview;
  useEffect(() => {
    if (preview?.already_member && !room) duel.join(preview.room_id);
  }, [preview, room, duel]);

  const exit = () => {
    duel.leave();
    onExit();
  };

  const say = (text: string) => {
    setFlash(text);
    window.setTimeout(() => setFlash(null), 2500);
  };

  if (state.conn === 'failed') {
    return (
      <Center title="Не удалось войти" text={state.connError ?? 'Попробуйте открыть игру заново из VK.'}>
        <button className="btn btn--secondary btn--block" onClick={onExit}>
          На главную
        </button>
      </Center>
    );
  }
  if (state.conn === 'replaced') {
    return (
      <Center title="Игра открыта на другом устройстве" text="Здесь матч остановлен, чтобы не было двух игроков с одного аккаунта.">
        <button className="btn btn--primary btn--block" onClick={duel.reconnect}>
          Играть здесь
        </button>
        <button className="btn btn--ghost btn--block" onClick={onExit}>
          На главную
        </button>
      </Center>
    );
  }

  const offline = state.conn !== 'online';

  if (!room) {
    if (inviteRoomId && state.error && ['room_not_found', 'room_closed'].includes(state.error.code)) {
      return (
        <Center title="Приглашение больше не действует" text="Попросите друга прислать новую ссылку или сыграйте против бота.">
          <button className="btn btn--primary btn--block" onClick={onExit}>
            На главную
          </button>
        </Center>
      );
    }
    if (inviteRoomId && state.error?.code === 'room_full') {
      return (
        <Center title="Комната уже занята" text="В этой дуэли уже двое игроков.">
          <button className="btn btn--primary btn--block" onClick={onExit}>
            На главную
          </button>
        </Center>
      );
    }
    if (inviteRoomId && preview && !preview.already_member) {
      if (!preview.joinable) {
        return (
          <Center
            title={preview.status === 'closed' ? 'Приглашение больше не действует' : 'Комната уже занята'}
            text="Попросите друга прислать новую ссылку."
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
          <h1 className="duel__title">{preview.host_name} приглашает сыграть Blitz Poker</h1>
          <p className="duel__text">
            Дуэль один на один: 8 раздач, у каждого по 600 условных фишек, блайнды растут. Фишки
            виртуальные, на деньги не играем.
          </p>
          <div className="duel__actions">
            <button className="btn btn--primary btn--hero" disabled={offline} onClick={() => duel.join(preview.room_id)}>
              Присоединиться
            </button>
            <button className="btn btn--ghost btn--block" onClick={onExit}>
              Отказаться
            </button>
          </div>
        </main>
      );
    }
    if (state.error && state.error.code !== 'offline') {
      return (
        <Center title="Не получилось" text={state.error.message}>
          <button className="btn btn--primary btn--block" onClick={onExit}>
            На главную
          </button>
        </Center>
      );
    }
    return (
      <main className="center-screen" aria-busy="true">
        <div className="loader" aria-label="Подключение" />
        <p className="center-screen__text">{offline ? 'Подключаемся к серверу…' : 'Готовим комнату…'}</p>
        <button className="btn btn--ghost btn--block" onClick={onExit}>
          Отмена
        </button>
      </main>
    );
  }

  const now = duel.serverNow();
  const opponent = room.opponent;

  if (room.status === 'closed') {
    const text = closedText(room);
    return (
      <Center title={text.title} text={text.text}>
        <button className="btn btn--primary btn--block" onClick={onExit}>
          На главную
        </button>
      </Center>
    );
  }

  if (room.status === 'playing' && room.game) {
    const g = room.game;
    const clock = g.clock;
    return (
      <>
        {offline && <div className="notice notice--floating">Нет соединения, переподключаемся…</div>}
        <Table
          view={g.view}
          paused={false}
          notice={state.error && state.error.code !== 'offline' ? state.error.message : null}
          onRetrySave={duel.clearError}
          onAct={duel.act}
          onNext={() => {}}
          onMenu={() => setMenu(true)}
          onTutorialStep={() => {}}
          onSkipTutorial={() => {}}
          duel={{
            name: opponent?.name ?? 'Соперник',
            photo: opponent?.photo ?? null,
            connected: opponent?.connected ?? false,
            reconnectInMs: opponent?.reconnect_deadline_ms ? opponent.reconnect_deadline_ms - now : null,
            clock: clock ? { mine: clock.my_turn, leftMs: clock.deadline_ms - now } : null,
            nextHandInMs: g.next_hand_at_ms ? g.next_hand_at_ms - now : null,
            pending: state.pending || offline,
          }}
        />
        {menu && (
          <div className="overlay" onClick={() => setMenu(false)}>
            <div className="surface surface--sheet" role="dialog" aria-modal="true" aria-label="Меню" onClick={(e) => e.stopPropagation()}>
              <h2 className="surface__title">Дуэль</h2>
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
                    setConfirm('surrender');
                  }}
                >
                  Сдаться и выйти
                </button>
              </div>
            </div>
          </div>
        )}
        {confirm === 'surrender' && (
          <ConfirmDialog
            title="Сдаться?"
            text="Матч закончится техническим поражением. Если просто закрыть приложение, у вас будет минута, чтобы вернуться."
            cancelLabel="Играть дальше"
            confirmLabel="Сдаться"
            onCancel={() => setConfirm(null)}
            onConfirm={() => {
              setConfirm(null);
              duel.surrender();
            }}
          />
        )}
      </>
    );
  }

  if (room.status === 'finished' && room.game?.view.result) {
    return <DuelResult room={room} duel={duel} offline={offline} onExit={exit} />;
  }

  // Waiting for a friend / both deciding they are ready.
  const link = inviteLink(room.room_id);
  const expiresIn = room.expires_at_ms ? room.expires_at_ms - now : null;
  return (
    <main className="duel duel--lobby">
      <header className="screen-header">
        <button
          className="btn btn--ghost btn--small"
          onClick={() => (room.you_are_host && opponent ? setConfirm('cancel') : exit())}
        >
          ← {room.you_are_host ? 'Отменить' : 'Выйти'}
        </button>
        <h1 className="screen-header__title">Дуэль с другом</h1>
      </header>

      {offline && <div className="notice">Нет соединения, переподключаемся…</div>}

      <div className="duel-seats">
        <SeatCard slot={room.me} label="Вы" />
        <span className="duel-seats__vs" aria-hidden="true">
          VS
        </span>
        {opponent ? <SeatCard slot={opponent} label={opponent.name} /> : <EmptySeat />}
      </div>

      {room.status === 'waiting' && room.you_are_host && (
        <section className="duel__invite">
          <p className="duel__text">Отправьте другу ссылку: он откроет игру прямо в этой комнате.</p>
          <button
            className="btn btn--primary btn--hero"
            disabled={offline}
            onClick={async () => {
              const shared = await shareLink(link);
              track('duel_invite_shared', { native: shared });
              if (!shared && (await copyText(link))) say('Ссылка скопирована');
            }}
          >
            Пригласить
          </button>
          <button
            className="btn btn--secondary btn--block"
            onClick={async () => {
              track('duel_invite_copied', {});
              say((await copyText(link)) ? 'Ссылка скопирована' : link);
            }}
          >
            Скопировать ссылку
          </button>
        </section>
      )}

      {room.status === 'waiting' && !room.you_are_host && (
        <p className="duel__text">Ждём, пока хозяин комнаты вернётся…</p>
      )}

      {room.status === 'ready' && (
        <section className="duel__ready">
          <p className="duel__text">
            8 раздач, по 600 условных фишек. Матч начнётся, когда оба нажмут «Готов».
          </p>
          <button
            className={`btn btn--hero ${room.me.ready ? 'btn--secondary' : 'btn--primary'}`}
            disabled={offline}
            onClick={() => duel.ready(!room.me.ready)}
          >
            {room.me.ready ? 'Не готов' : 'Готов'}
          </button>
        </section>
      )}

      {expiresIn !== null && expiresIn < 10 * 60_000 && (
        <p className="duel__hint">Комната закроется через {formatClock(expiresIn)}, если ничего не происходит</p>
      )}
      {flash && (
        <p className="duel__flash" role="status">
          {flash}
        </p>
      )}
      {state.error && state.error.code !== 'offline' && <p className="duel__error">{state.error.message}</p>}

      {confirm === 'cancel' && (
        <ConfirmDialog
          title="Отменить комнату?"
          text="Друг увидит, что комната закрыта."
          cancelLabel="Остаться"
          confirmLabel="Отменить комнату"
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

function SeatCard({ slot, label }: { slot: SlotView; label: string }) {
  return (
    <div className={`duel-seat${slot.ready ? ' is-ready' : ''}`}>
      <Avatar slot={slot} />
      <span className="duel-seat__name">{label}</span>
      <span className="duel-seat__state">{!slot.connected ? 'Не в сети' : slot.ready ? 'Готов' : 'Не готов'}</span>
    </div>
  );
}

function EmptySeat() {
  return (
    <div className="duel-seat duel-seat--empty">
      <Avatar slot={null} />
      <span className="duel-seat__name">Ждём друга…</span>
    </div>
  );
}

function closedText(room: RoomView): { title: string; text: string } {
  switch (room.close_reason) {
    case 'cancelled_by_host':
      return room.you_are_host
        ? { title: 'Комната отменена', text: 'Создайте новую, чтобы пригласить друга.' }
        : { title: 'Друг отменил комнату', text: 'Попросите прислать новое приглашение.' };
    case 'left':
      return { title: 'Друг вышел из комнаты', text: 'Чтобы сыграть ещё, отправьте новое приглашение.' };
    case 'aborted':
      return { title: 'Матч прерван', text: 'Оба игрока отключились надолго. Результат не засчитан.' };
    default:
      return { title: 'Приглашение больше не действует', text: 'Комната закрылась за давностью.' };
  }
}

function DuelResult({
  room,
  duel,
  offline,
  onExit,
}: {
  room: RoomView;
  duel: DuelController;
  offline: boolean;
  onExit: () => void;
}) {
  const result = room.game!.view.result!;
  const name = room.opponent?.name ?? 'Соперник';
  const opponentHere = room.opponent?.connected ?? false;
  const forfeit = result.forfeit;
  const title = result.outcome === 'win' ? 'Победа!' : result.outcome === 'loss' ? `Победил ${name}` : 'Ничья!';
  const subtitle =
    forfeit === 'bot'
      ? `Техническая победа: ${name} сдался или не вернулся`
      : forfeit === 'human'
        ? 'Техническое поражение'
        : result.outcome === 'draw'
          ? 'Стеки сравнялись'
          : `${result.hands_played} ${plural(result.hands_played, 'раздача', 'раздачи', 'раздач')}`;
  const n = result.hands_played;
  const { rematch, score } = room;
  return (
    <main className={`result result--${result.outcome}`}>
      <div className="result__hero">
        {result.outcome === 'win' ? <Trophy /> : result.outcome === 'draw' ? <HandshakeIcon /> : <CardsIcon />}
        <h1 className="result__title">{title}</h1>
        <p className="result__subtitle">{subtitle}</p>
      </div>
      <section className="scoreboard" aria-label="Итоговые стеки">
        <div className={result.outcome === 'win' ? 'is-leader' : ''}>
          <span className="scoreboard__who">Вы</span>
          <span className="scoreboard__chips">
            <ChipIcon /> {result.final_stacks[0]}
          </span>
        </div>
        <div className={result.outcome === 'loss' ? 'is-leader' : ''}>
          <span className="scoreboard__who">{name}</span>
          <span className="scoreboard__chips">
            <ChipIcon /> {result.final_stacks[1]}
          </span>
        </div>
      </section>
      <ul className="result__facts">
        <li>
          {n} {plural(n, 'раздача сыграна', 'раздачи сыграно', 'раздач сыграно')}
          {forfeit && ' · матч завершён досрочно'}
        </li>
        <li>
          Счёт встреч: {score.me} : {score.opponent}
          {score.draws > 0 && `, ничьих ${score.draws}`}
        </li>
        {result.best_hand && (
          <li className="result__best">
            <span>Ваша лучшая комбинация: {describeHand({ category: result.best_hand.category, best: result.best_hand.cards })}</span>
            <span className="result__best-cards">
              {result.best_hand.cards.map((c) => (
                <PlayingCard key={c} card={c} size="xs" />
              ))}
            </span>
          </li>
        )}
      </ul>
      <div className="result__actions">
        {!opponentHere ? (
          <p className="duel__text">{name} не в сети. Можно отправить новое приглашение.</p>
        ) : rematch.requested_by_opponent ? (
          <>
            <p className="duel__text">{name} предлагает реванш</p>
            <button className="btn btn--primary btn--hero" disabled={offline} onClick={duel.acceptRematch}>
              Принять
            </button>
            <button className="btn btn--secondary btn--block" disabled={offline} onClick={duel.declineRematch}>
              Отказаться
            </button>
          </>
        ) : rematch.requested_by_me ? (
          <button className="btn btn--secondary btn--hero" disabled>
            Ждём ответа…
          </button>
        ) : (
          <>
            {rematch.declined_by_opponent && <p className="duel__text">{name} отказался от реванша</p>}
            <button className="btn btn--primary btn--hero" disabled={offline} onClick={duel.requestRematch}>
              Предложить реванш
            </button>
          </>
        )}
        {!opponentHere && (
          <button className="btn btn--primary btn--hero" onClick={duel.createRoom}>
            Новое приглашение
          </button>
        )}
        <button className="btn btn--ghost btn--block" onClick={onExit}>
          На главную
        </button>
      </div>
    </main>
  );
}
