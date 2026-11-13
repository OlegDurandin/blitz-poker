import { afterEach, expect, it, vi } from 'vitest';

afterEach(() => { vi.unstubAllEnvs(); vi.unstubAllGlobals(); vi.resetModules(); });
it.each([
  ['https://blitz.85-137-95-37.sslip.io', 'https://blitz.85-137-95-37.sslip.io'],
  ['https://example.vk-apps.ru', 'https://pvp.example.test'],
  ['http://localhost:5173', 'https://pvp.example.test'],
])('selects the configured API for %s', async (origin, expected) => {
  vi.resetModules();
  vi.stubEnv('VITE_PVP_URL', 'https://pvp.example.test');
  vi.stubEnv('VITE_PVP_PROXY_ORIGIN', 'https://blitz.85-137-95-37.sslip.io');
  vi.stubGlobal('window', { location: { origin, hash: '' } });
  expect((await import('./config')).PVP_URL).toBe(expected);
});

it('uses canonical HTTPS for Telegram when the optional server-to-server proxy is disabled', async () => {
  vi.resetModules();
  vi.stubEnv('VITE_PVP_URL', 'https://pvp.example.test');
  vi.stubEnv('VITE_PVP_PROXY_ORIGIN', '');
  vi.stubGlobal('window', { location: { origin: 'https://blitz.85-137-95-37.sslip.io', hash: '' } });
  expect((await import('./config')).PVP_URL).toBe('https://pvp.example.test');
});
