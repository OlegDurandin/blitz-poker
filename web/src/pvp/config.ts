// Where the duel server lives, room ids and codes. Platform specifics (sign-in,
// invite links) live in ../platform.

/** Base URL of the PvP server, e.g. https://pvp.example.ru. Unset: duels are hidden. */
const proxyOrigin = import.meta.env.VITE_PVP_PROXY_ORIGIN;
export const PVP_URL: string | undefined =
  proxyOrigin && typeof window !== 'undefined' && window.location.origin === proxyOrigin
    ? proxyOrigin
    : import.meta.env.VITE_PVP_URL || undefined;

/** Commit of this build. */
export const BUILD: string = typeof __BUILD__ === 'string' ? __BUILD__ : 'dev';

/** The fragment the app was opened with, before anything could change it. */
export const ENTRY_HASH: string = typeof window === 'undefined' ? '' : window.location.hash;

const GUEST_KEY = 'blitz.v1.guest_key';
const GUEST_NAME = 'blitz.v1.guest_name';
let memoryKey: string | null = null;

/**
 * Outside a messenger: a random key this browser keeps, so a guest who reloads gets
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

const ROOM_ID = /^[A-Za-z0-9_-]{16,64}$/;

/** A bare room id (base64url, as the server makes them). */
export function isRoomId(text: string): boolean {
  return ROOM_ID.test(text);
}

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
