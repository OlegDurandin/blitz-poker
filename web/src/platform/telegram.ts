// Telegram Mini Apps. Telegram opens the app with `#tgWebAppData=<initData>`
// in the URL; the server verifies initData with the bot token. Invites are
// https://t.me/<bot>/<app>?startapp=room_<id>, which Telegram hands back as
// `start_param`. The SDK script adds ready(), haptics and the share sheet.

import { isRoomId } from '../pvp/config';
import { clipboardCopy } from './browser';
import type { Platform, PlatformUser } from './types';

interface TgWebApp {
  initData: string;
  ready(): void;
  expand(): void;
  openTelegramLink(url: string): void;
  HapticFeedback?: { selectionChanged(): void };
}

declare global {
  interface Window {
    Telegram?: { WebApp?: TgWebApp };
  }
}

const SDK = 'https://telegram.org/js/telegram-web-app.js';
const START_PREFIX = 'room_';
const USED_START = 'blitz.v1.tg_start_used';

/** initData as Telegram launched us, read before anything touches the URL. */
const INIT_DATA: string = (() => {
  if (typeof window === 'undefined') return '';
  const fromHash = new URLSearchParams(window.location.hash.slice(1)).get('tgWebAppData');
  return fromHash ?? window.Telegram?.WebApp?.initData ?? '';
})();

export function isTelegram(): boolean {
  return INIT_DATA !== '';
}

function field(name: string): string | null {
  return new URLSearchParams(INIT_DATA).get(name);
}

function webApp(): TgWebApp | undefined {
  return window.Telegram?.WebApp;
}

/** `startapp` of the link that opened the app, if it is an invite. */
export function roomFromStartParam(start: string | null): string | null {
  if (!start?.startsWith(START_PREFIX)) return null;
  const id = start.slice(START_PREFIX.length);
  return isRoomId(id) ? id : null;
}

export const telegram: Platform = {
  name: 'telegram',
  canHost: true,

  init() {
    if (webApp()) {
      webApp()!.ready();
      webApp()!.expand();
      return;
    }
    const s = document.createElement('script');
    s.src = SDK;
    s.async = true;
    s.onload = () => {
      webApp()?.ready();
      webApp()?.expand();
    };
    document.head.appendChild(s);
  },

  haptic() {
    webApp()?.HapticFeedback?.selectionChanged();
  },

  async getUser(): Promise<PlatformUser | null> {
    try {
      const u = JSON.parse(field('user') ?? 'null') as {
        first_name?: string;
        last_name?: string;
        photo_url?: string;
      } | null;
      if (!u?.first_name) return null;
      return { name: u.first_name, photo: u.photo_url ?? null };
    } catch {
      return null;
    }
  },

  async share(link) {
    const app = webApp();
    if (!app) return false;
    const url = `https://t.me/share/url?url=${encodeURIComponent(link)}&text=${encodeURIComponent('Сыграем дуэль в Blitz Poker?')}`;
    app.openTelegramLink(url);
    return true;
  },

  copy: clipboardCopy,

  authProof: () => ({ tg_init_data: INIT_DATA }),

  inviteLink(roomId) {
    const bot = import.meta.env.VITE_TG_BOT;
    const app = import.meta.env.VITE_TG_APP;
    return bot && app ? `https://t.me/${bot}/${app}?startapp=${START_PREFIX}${roomId}` : null;
  },

  /** start_param stays in initData for the whole session: hand it out once. */
  launchRoom() {
    const id = roomFromStartParam(field('start_param'));
    try {
      if (!id || sessionStorage.getItem(USED_START) === id) return null;
    } catch {
      // No session storage: an exit may reopen the invite on reload; harmless.
    }
    return id;
  },

  clearLaunchRoom() {
    const id = roomFromStartParam(field('start_param'));
    try {
      if (id) sessionStorage.setItem(USED_START, id);
    } catch {
      // See launchRoom.
    }
  },

  // Telegram reopens the app for a new startapp link: nothing arrives live.
  onInvite: () => () => {},
};
