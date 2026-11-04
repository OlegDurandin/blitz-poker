import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { execSync } from 'node:child_process';

/** Short commit of this build, shown on the home screen and sent at sign-in. */
function build(): string {
  try {
    const sha = execSync('git rev-parse --short HEAD').toString().trim();
    const dirty = execSync('git status --porcelain -- src').toString().trim() !== '';
    return dirty ? `${sha}+` : sha;
  } catch {
    return 'dev';
  }
}

export default defineConfig({
  plugins: [react()],
  define: { __BUILD__: JSON.stringify(build()) },
  // VK Mini Apps may be served from a sub-path; keep asset URLs relative.
  base: './',
  server: {
    host: true,
    port: 5173,
    strictPort: true,
    // Behind vk-tunnel (npm run tunnel) the page is served over HTTPS on 443.
    hmr: process.env.VK_TUNNEL ? { protocol: 'wss', clientPort: 443 } : undefined,
  },
  build: { target: 'es2020' },
});
