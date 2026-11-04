import bridge from '@vkontakte/vk-bridge';

let hapticsSupported = false;

/** Tells the VK client the app is ready. Harmless outside VK (plain browser). */
export function initVk(): void {
  bridge.send('VKWebAppInit').catch(() => {
    // Not running inside VK, e.g. local development.
  });
  bridge
    .supportsAsync('VKWebAppTapticSelectionChanged')
    .then((ok) => (hapticsSupported = ok))
    .catch(() => {});
}

/** Light haptic tick, only where the platform supports it. */
export function haptic(): void {
  if (hapticsSupported) bridge.send('VKWebAppTapticSelectionChanged').catch(() => {});
}

/** Outside VK some bridge calls never answer; don't wait for them forever. */
function withTimeout<T>(p: Promise<T>, ms = 1500): Promise<T> {
  return Promise.race([p, new Promise<T>((_, reject) => window.setTimeout(() => reject(new Error('timeout')), ms))]);
}

export interface VkUser {
  name: string;
  photo: string | null;
}

/** The player's first name and avatar; null outside VK. Display only. */
export async function getUserInfo(): Promise<VkUser | null> {
  if (bridge.isStandalone()) return null;
  try {
    const u = await withTimeout(bridge.send('VKWebAppGetUserInfo'));
    return { name: u.first_name, photo: u.photo_100 ?? null };
  } catch {
    return null;
  }
}

/** Opens the VK share dialog for a link. Resolves false if unavailable. */
export async function shareLink(link: string): Promise<boolean> {
  if (bridge.isStandalone()) return false;
  try {
    // The share sheet stays open while the user picks a chat: allow time.
    await withTimeout(bridge.send('VKWebAppShare', { link }), 120_000);
    return true;
  } catch {
    return false;
  }
}

/** Copies text via VK, falling back to the browser clipboard. */
export async function copyText(text: string): Promise<boolean> {
  try {
    if (bridge.isStandalone()) throw new Error('outside VK');
    await withTimeout(bridge.send('VKWebAppCopyText', { text }));
    return true;
  } catch {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      return false;
    }
  }
}

/**
 * An invite link tapped while the app is already open: VK does not reload the
 * page, it reports the new fragment. Calls back with the raw location.
 */
export function onVkLocation(cb: (location: string) => void): () => void {
  const listener = (e: { detail?: { type?: string; data?: unknown } }) => {
    if (e.detail?.type !== 'VKWebAppLocationChanged') return;
    const loc = (e.detail.data as { location?: unknown } | undefined)?.location;
    if (typeof loc === 'string') cb(loc);
  };
  const onHash = () => cb(window.location.hash);
  bridge.subscribe(listener as Parameters<typeof bridge.subscribe>[0]);
  window.addEventListener('hashchange', onHash);
  return () => {
    bridge.unsubscribe(listener as Parameters<typeof bridge.unsubscribe>[0]);
    window.removeEventListener('hashchange', onHash);
  };
}
