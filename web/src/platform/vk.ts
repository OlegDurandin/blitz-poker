// VK Mini Apps: vk-bridge, launch parameters signed by VK, invites as
// https://vk.com/app<id>#room=<id>.

import bridge from '@vkontakte/vk-bridge';
import { parseRoom } from '../pvp/config';
import { clearRoomFromLocation, clipboardCopy, onHashInvite, roomFromLocation } from './browser';
import type { Platform, PlatformUser } from './types';

let hapticsSupported = false;

/** Opened inside VK: the launch parameters carry VK's signature. */
export function isVk(): boolean {
  const q = new URLSearchParams(window.location.search);
  return q.has('sign') && q.has('vk_user_id');
}

/** Outside VK some bridge calls never answer; don't wait for them forever. */
function withTimeout<T>(p: Promise<T>, ms = 1500): Promise<T> {
  return Promise.race([p, new Promise<T>((_, reject) => window.setTimeout(() => reject(new Error('timeout')), ms))]);
}

function appId(): string | null {
  return new URLSearchParams(window.location.search).get('vk_app_id') ?? import.meta.env.VITE_VK_APP_ID ?? null;
}

/**
 * An invite link tapped while the app is already open: VK does not reload the
 * page, it reports the new fragment (and some clients change the hash).
 */
function onVkInvite(cb: (roomId: string) => void): () => void {
  const listener = (e: { detail?: { type?: string; data?: unknown } }) => {
    if (e.detail?.type !== 'VKWebAppLocationChanged') return;
    const loc = (e.detail.data as { location?: unknown } | undefined)?.location;
    const id = typeof loc === 'string' ? parseRoom(loc) : null;
    if (id) cb(id);
  };
  bridge.subscribe(listener as Parameters<typeof bridge.subscribe>[0]);
  const offHash = onHashInvite(cb);
  return () => {
    bridge.unsubscribe(listener as Parameters<typeof bridge.unsubscribe>[0]);
    offHash();
  };
}

export const vk: Platform = {
  name: 'vk',
  label: 'VK',
  canHost: true,

  init() {
    bridge.send('VKWebAppInit').catch(() => {});
    bridge
      .supportsAsync('VKWebAppTapticSelectionChanged')
      .then((ok) => (hapticsSupported = ok))
      .catch(() => {});
  },

  haptic() {
    if (hapticsSupported) bridge.send('VKWebAppTapticSelectionChanged').catch(() => {});
  },

  async getUser(): Promise<PlatformUser | null> {
    try {
      const u = await withTimeout(bridge.send('VKWebAppGetUserInfo'));
      return { name: u.first_name, photo: u.photo_100 ?? null };
    } catch {
      return null;
    }
  },

  async share(link) {
    try {
      // The share sheet stays open while the user picks a chat: allow time.
      await withTimeout(bridge.send('VKWebAppShare', { link }), 120_000);
      return true;
    } catch {
      return false;
    }
  },

  async copy(text) {
    try {
      await withTimeout(bridge.send('VKWebAppCopyText', { text }));
      return true;
    } catch {
      return clipboardCopy(text);
    }
  },

  authProof: () => ({ launch_params: window.location.search.slice(1) }),

  /**
   * VK passes the part after `#` to the Mini App as `location.hash` (not on
   * every client: room codes are the fallback that always works).
   */
  inviteLink(roomId) {
    const id = appId();
    return id ? `https://vk.com/app${id}#room=${roomId}` : null;
  },

  launchRoom: roomFromLocation,
  clearLaunchRoom: clearRoomFromLocation,
  onInvite: onVkInvite,
};
