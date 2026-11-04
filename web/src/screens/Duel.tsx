import { useEffect, useRef, useState } from 'react';
import { ConfirmDialog } from '../components/Dialog';
import { ChipIcon, HandshakeIcon, Trophy, CardsIcon } from '../components/icons';
import { PlayingCard } from '../components/PlayingCard';
import { describeHand, plural } from '../game/format';
import { track } from '../game/analytics';
import { devUserId, formatCode, guest, parseCode, parseRoom } from '../pvp/config';
import type { RoomView, SlotView } from '../pvp/types';
import type { Duel as DuelController } from '../pvp/useDuel';
import { platform } from '../platform';
import { Table, formatClock } from './Table';

interface Props {
  duel: DuelController;
  /** Opened from an invite link; otherwise this player creates a room. */
  inviteRoomId: string | null;
  /** Opens a friend's room from a pasted invite link. */
  onOpenInvite: (roomId: string) => void;
  /** "Войти по коду" from the home screen: ask for the code, create nothing. */
  enterCode?: boolean;
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

/** "Друг прислал ссылку или код?": joins without relying on VK passing the link through. */
function PasteInvite({
  ownRoom,
  ownCode,
  onOpen,
  onCode,
  startOpen = false,
}: {
  ownRoom: string;
  ownCode: string;
  startOpen?: boolean;
  onOpen: (roomId: string) => void;
  onCode: (code: string) => void;
}) {
  const [open, setOpen] = useState(startOpen);
  const [text, setText] = useState('');
  const [bad, setBad] = useState(false);
  if (!open) {
    return (
      <button className="btn btn--ghost btn--block" onClick={() => setOpen(true)}>
        Друг прислал ссылку или код?
      </button>
    );
  }
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    const code = parseCode(text);
    if (code) {
      if (code === ownCode) return setBad(true);
      track('duel_code_entered', {});
      return onCode(code);
    }
    const id = parseRoom(text);
    if (!id || id === ownRoom) return setBad(true);
    track('duel_invite_pasted', {});
    onOpen(id);
  };
  return (
    <form className="duel-paste" onSubmit={submit}>
      <label className="duel__hint" htmlFor="duel-paste">
        Введите код комнаты из 6 цифр или вставьте ссылку из сообщения друга
      </label>
      <input
        id="duel-paste"
        className="duel-paste__input"
        value={text}
        onChange={(e) => {
          setText(e.target.value);
          setBad(false);
        }}
        placeholder="Код, например 482 913, или ссылка"
        inputMode={startOpen ? 'numeric' : undefined}
        autoComplete="off"
        autoCapitalize="off"
        spellCheck={false}
        aria-invalid={bad}
        autoFocus
      />
      {bad && <p className="duel-paste__error">Это не код и не ссылка на комнату друга</p>}
      <button type="submit" className="btn btn--primary btn--block" disabled={!text.trim()}>
        Войти в комнату
      </button>
    </form>
  );
}

export function Duel({ duel, inviteRoomId, onOpenInvite, enterCode = false, onExit, onRules }: Props) {
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
    else if (!enterCode) duel.createRoom();
  }, [state.conn, room, inviteRoomId, enterCode, duel]);

  // The host's own link, or a reload: take the seat back without asking, once.
  // A room of one's own that has closed since is just left behind.
  const preview = state.preview;
  const rejoined = useRef<string | null>(null);
  useEffect(() => {
    if (!preview?.already_member || room || rejoined.current === preview.room_id) return;
    rejoined.current = preview.room_id;
    if (preview.status === 'closed') onExit();
    else duel.join(preview.room_id);
  }, [preview, room, duel, onExit]);

  // A code typed by hand: once the server finds the friend's room, open it
  // like an invite link.
  const byCode = useRef(false);
  useEffect(() => {
    if (!byCode.current || !preview || preview.room_id === room?.room_id) return;
    byCode.current = false;
    onOpenInvite(preview.room_id);
  }, [preview, room, onOpenInvite]);

  // A code that found nothing: later previews are not the answer to it.
  useEffect(() => {
    if (state.error && ['code_not_found', 'too_many_attempts', 'bad_request'].includes(state.error.code)) {
      byCode.current = false;
    }
  }, [state.error]);

  const [keepRoom, setKeepRoom] = useState(false);
  const isGuest = !platform.canHost && devUserId() === null;
  const [guestName, setGuestName] = useState(() => guest.name());

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

  if (!room && enterCode && !inviteRoomId) {
    const codeError =
      state.error && state.error.code !== 'offline' && state.error.message ? state.error.message : null;
    return (
      <main className="center-screen duel">
        <h1 className="center-screen__title">Войти по коду</h1>
        <p className="center-screen__text">
          Попросите друга нажать «Играть с другом» и продиктовать код комнаты из 6 цифр.
        </p>
        {offline && <div className="notice">Подключаемся к серверу…</div>}
        {isGuest && (
          <div className="duel-paste">
            <label className="duel__hint" htmlFor="guest-name">
              Вы не в VK: друг увидит это имя
            </label>
            <input
              id="guest-name"
              className="duel-paste__input"
              value={guestName}
              onChange={(e) => setGuestName(e.target.value.slice(0, 30))}
              placeholder="Ваше имя"
              autoComplete="nickname"
            />
          </div>
        )}
        <PasteInvite
          ownRoom=""
          ownCode=""
          startOpen
          onOpen={(id) => {
            if (isGuest) duel.setGuestName(guestName.trim());
            onOpenInvite(id);
          }}
          onCode={(code) => {
            if (isGuest) duel.setGuestName(guestName.trim());
            byCode.current = true;
            duel.findByCode(code);
          }}
        />
        {codeError && <p className="duel__error">{codeError}</p>}
        <button className="btn btn--ghost btn--block" onClick={onExit}>
          На главную
        </button>
      </main>
    );
  }

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
  // Links open the host's own messenger; a friend elsewhere uses the code.
  const link = platform.inviteLink(room.room_id);
  const expiresIn = room.expires_at_ms ? room.expires_at_ms - now : null;
  return (
    <main className="duel duel--lobby">
      <header className="screen-header">
        <button
          className="btn btn--ghost btn--small"
          onClick={() => {
            // Nobody has joined yet: keep the room so the link already sent
            // still works; "Играть с другом" brings the host back to it.
            if (room.you_are_host && !opponent) onExit();
            else if (room.you_are_host) setConfirm('cancel');
            else exit();
          }}
        >
          ← {room.you_are_host && opponent ? 'Отменить' : room.you_are_host ? 'Назад' : 'Выйти'}
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

      {room.status === 'waiting' && room.you_are_host && state.reused && !keepRoom && (
        <div className="duel-reused" role="status">
          <b>Твоя комната ещё открыта</b>
          <span>Ссылка и код прежние — друг, может быть, уже ждёт.</span>
          <div className="duel-reused__actions">
            <button className="btn btn--secondary btn--small" onClick={() => setKeepRoom(true)}>
              Продолжить
            </button>
            <button className="btn btn--ghost btn--small" disabled={offline} onClick={() => duel.createRoom()}>
              Создать новую
            </button>
          </div>
        </div>
      )}

      {room.status === 'waiting' && room.you_are_host && (
        <section className="duel__invite">
          <p className="duel__text">
            Отправьте другу ссылку в VK или продиктуйте код: он откроет игру прямо в этой комнате.
            Комната ждёт 15 минут, «Играть с другом» на главной вернёт вас сюда.
          </p>
          <button
            className="duel-code"
            onClick={async () => {
              track('duel_code_copied', {});
              say((await platform.copy(room.code)) ? 'Код скопирован' : `Код: ${formatCode(room.code)}`);
            }}
            aria-label={`Код комнаты ${formatCode(room.code)}, нажмите, чтобы скопировать`}
          >
            <span className="duel-code__label">Код комнаты</span>
            <span className="duel-code__digits">{formatCode(room.code)}</span>
          </button>
          {link && (
            <>
              <button
                className="btn btn--primary btn--hero"
                disabled={offline}
                onClick={async () => {
                  const shared = await platform.share(link);
                  track('duel_invite_shared', { native: shared });
                  if (!shared && (await platform.copy(link))) say('Ссылка скопирована');
                }}
              >
                Пригласить
              </button>
              <button
                className="btn btn--secondary btn--block"
                onClick={async () => {
                  track('duel_invite_copied', {});
                  say((await platform.copy(link)) ? 'Ссылка скопирована' : link);
                }}
              >
                Скопировать ссылку
              </button>
            </>
          )}
          <PasteInvite
            ownRoom={room.room_id}
            ownCode={room.code}
            onOpen={onOpenInvite}
            onCode={(code) => {
              byCode.current = true;
              duel.findByCode(code);
            }}
          />
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
