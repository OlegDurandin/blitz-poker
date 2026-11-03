// Where the duel server lives and how invite links look.

/** Base URL of the PvP server, e.g. https://pvp.example.ru. Unset: duels are hidden. */
export const PVP_URL: string | undefined = import.meta.env.VITE_PVP_URL || undefined;

/** Launch parameters VK signed for this session (verified by the server). */
export function launchParams(): string {
  return window.location.search.slice(1);
}

/**
 * Local two-window testing without VK (server with PVP_DEV_AUTH=1):
 * open http://localhost:5173/?dev_user=1 and ?dev_user=2. Dev builds only.
 */
export function devUserId(): number | null {
  if (!import.meta.env.DEV) return null;
  const id = Number(new URLSearchParams(window.location.search).get('dev_user'));
  return Number.isInteger(id) && id > 0 ? id : null;
}

function appId(): string | null {
  return new URLSearchParams(window.location.search).get('vk_app_id') ?? import.meta.env.VITE_VK_APP_ID ?? null;
}

/**
 * The link a friend taps. VK passes the part after `#` to the Mini App as
 * `location.hash`. Needs checking on iOS, Android and desktop VK; "Скопировать
 * ссылку" gives the same link as a fallback.
 */
export function inviteLink(roomId: string): string {
  const id = appId();
  const hash = `room=${roomId}`;
  if (id) return `https://vk.com/app${id}#${hash}`;
  return `${window.location.origin}${window.location.pathname}${window.location.search}#${hash}`;
}

/** The room id from an invite link opened in the app, if any. */
export function roomFromLocation(): string | null {
  const hash = window.location.hash.replace(/^#/, '');
  const id = new URLSearchParams(hash).get('room');
  return id && /^[A-Za-z0-9_-]{16,64}$/.test(id) ? id : null;
}

const ACTIVE_ROOM_KEY = 'blitz.v1.duel_room';

/** The duel room this player is in, so a WebView reload returns to it. */
export const activeRoom = {
  get(): string | null {
    try {
      return localStorage.getItem(ACTIVE_ROOM_KEY);
    } catch {
      return null;
    }
  },
  set(id: string | null): void {
    try {
      if (id) localStorage.setItem(ACTIVE_ROOM_KEY, id);
      else localStorage.removeItem(ACTIVE_ROOM_KEY);
    } catch {
      // Private mode: a reload then lands on the home screen.
    }
  },
};

/** Forget the invite in the address bar once it has been used. */
export function clearRoomFromLocation(): void {
  if (window.location.hash) history.replaceState(null, '', window.location.pathname + window.location.search);
}
