// VK hosting lets clients cache index.html for 30 days, and the VK app on a
// phone has been seen running a build several deploys old. At start-up we
// fetch index.html past every cache; if it names a different bundle, reload
// once with a cache-busting parameter. VK signs only the vk_* parameters, so
// an extra one keeps the launch signature valid.

const RELOAD_FLAG = 'blitz.v1.fresh_reload';
const PARAM = 'fresh';

/** The bundle file this page is running, e.g. "index-BDSpvP5x.js". */
function runningBundle(): string | null {
  const s = document.querySelector<HTMLScriptElement>('script[type="module"][src*="assets/index-"]');
  return s ? (s.src.split('/').pop() ?? null) : null;
}

/** The bundle named by an index.html. */
export function bundleIn(html: string): string | null {
  return /assets\/(index-[A-Za-z0-9_-]+\.js)/.exec(html)?.[1] ?? null;
}

/** Resolves true when a reload to a newer build has been started. */
export async function reloadIfStale(timeoutMs = 2000): Promise<boolean> {
  if (import.meta.env.DEV) return false;
  const running = runningBundle();
  if (!running) return false;
  let tried = false;
  try {
    tried = sessionStorage.getItem(RELOAD_FLAG) === running;
  } catch {
    return false; // no way to prevent a reload loop: do nothing
  }
  if (tried) return false;
  try {
    const ctrl = new AbortController();
    const timer = window.setTimeout(() => ctrl.abort(), timeoutMs);
    const url = new URL('index.html', window.location.href);
    url.searchParams.set(PARAM, String(Date.now()));
    const res = await fetch(url, { cache: 'no-store', signal: ctrl.signal });
    window.clearTimeout(timer);
    if (!res.ok) return false;
    const latest = bundleIn(await res.text());
    if (!latest || latest === running) return false;
    sessionStorage.setItem(RELOAD_FLAG, running);
    const next = new URL(window.location.href);
    next.searchParams.set(PARAM, latest.replace(/^index-|\.js$/g, ''));
    window.location.replace(next.toString());
    return true;
  } catch {
    return false;
  }
}
