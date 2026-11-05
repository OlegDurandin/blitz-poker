// A plain browser: the hosting URL opened outside any messenger. Players here
// are guests: they join a friend's room by its code (or a pasted link).

import { parseRoom } from '../pvp/config';
import type { Platform } from './types';

export async function clipboardCopy(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

/** Invites in the address bar: `#room=<id>` (also what VK hands over). */
export function roomFromLocation(): string | null {
  return parseRoom(window.location.hash) ?? parseRoom(window.location.search);
}

export function clearRoomFromLocation(): void {
  if (window.location.hash) history.replaceState(null, '', window.location.pathname + window.location.search);
}

export function onHashInvite(cb: (roomId: string) => void): () => void {
  const onHash = () => {
    const id = parseRoom(window.location.hash);
    if (id) cb(id);
  };
  window.addEventListener('hashchange', onHash);
  return () => window.removeEventListener('hashchange', onHash);
}

export const browser: Platform = {
  name: 'browser',
  label: '',
  canHost: false,
  init() {},
  haptic() {},
  getUser: async () => null,
  share: async () => false,
  copy: clipboardCopy,
  authProof: () => null,
  inviteLink: (roomId) =>
    `${window.location.origin}${window.location.pathname}${window.location.search}#room=${roomId}`,
  launchRoom: roomFromLocation,
  clearLaunchRoom: clearRoomFromLocation,
  onInvite: onHashInvite,
};
