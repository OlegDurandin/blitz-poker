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
