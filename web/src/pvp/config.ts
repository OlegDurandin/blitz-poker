// Where the duel server lives and how invite links look.

/** Base URL of the PvP server, e.g. https://pvp.example.ru. Unset: duels are hidden. */
export const PVP_URL: string | undefined = import.meta.env.VITE_PVP_URL || undefined;

/** Commit of this build. */
export const BUILD: string = typeof __BUILD__ === 'string' ? __BUILD__ : 'dev';

/** The fragment the app was opened with, before anything could change it. */
export const ENTRY_HASH: string = typeof window === 'undefined' ? '' : window.location.hash;

/** Launch parameters VK signed for this session (verified by the server). */
export function launchParams(): string {
  return window.location.search.slice(1);
}

/** Opened inside VK (launch parameters carry VK's signature). */
export function inVk(): boolean {
  return new URLSearchParams(window.location.search).has('sign');
}

const GUEST_KEY = 'blitz.v1.guest_key';
const GUEST_NAME = 'blitz.v1.guest_name';
let memoryKey: string | null = null;

/**
 * Outside VK: a random key this browser keeps, so a guest who reloads gets
 * the same seat back, and the name they typed. Guests only join by code.
 */
export const guest = {
  key(): string {
    try {
      const saved = localStorage.getItem(GUEST_KEY);
      if (saved && /^[A-Za-z0-9_-]{32,128}$/.test(saved)) return saved;
    } catch {
      // Private mode: fall back to a key for this page only.
    }
    if (!memoryKey) {
      const b = crypto.getRandomValues(new Uint8Array(32));
      memoryKey = btoa(String.fromCharCode(...b)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
    }
    try {
      localStorage.setItem(GUEST_KEY, memoryKey);
    } catch {
      // Not persisted; fine for this visit.
    }
    return memoryKey;
  },
  name(): string {
    try {
      return localStorage.getItem(GUEST_NAME) ?? '';
    } catch {
      return '';
    }
  },
  setName(name: string): void {
    try {
      localStorage.setItem(GUEST_NAME, name);
    } catch {
      // Private mode: the name lasts for this connection only.
    }
  },
};

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

const ROOM_ID = /^[A-Za-z0-9_-]{16,64}$/;

/**
 * The room id in an invite link, a fragment or a bare id. Lenient on purpose:
 * VK clients may hand the fragment over percent-encoded ("room%3D…") or with
 * a leading slash, and a player may paste the whole link.
 */
export function parseRoom(text: string | null | undefined): string | null {
  let s = (text ?? '').trim();
  if (ROOM_ID.test(s)) return s;
  for (let i = 0; i < 3; i++) {
    const m = /(?:^|[#?&/\s])room=([A-Za-z0-9_-]{16,64})(?![A-Za-z0-9_-])/.exec(s);
    if (m) return m[1];
    try {
      const decoded = decodeURIComponent(s);
      if (decoded === s) break;
      s = decoded;
    } catch {
      break;
    }
  }
  return null;
}

/** A room code typed by hand: "482 913", "482-913" → "482913"; otherwise null. */
export function parseCode(text: string | null | undefined): string | null {
  const digits = (text ?? '').replace(/[\s-]/g, '');
  return /^\d{6}$/.test(digits) ? digits : null;
}

/** "482913" → "482 913": easier to read aloud. */
export function formatCode(code: string): string {
  return `${code.slice(0, 3)} ${code.slice(3)}`;
}

/** The room id from an invite link opened in the app, if any. */
export function roomFromLocation(): string | null {
  return parseRoom(window.location.hash) ?? parseRoom(window.location.search);
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
