import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
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
