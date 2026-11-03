// VK tooling with the app id taken from the repo-root .env (VK_APP_ID).
//   node scripts/vk.mjs deploy   upload web/dist to VK Mini Apps hosting
//   node scripts/vk.mjs tunnel   dev server + HTTPS tunnel, set as the app's dev URL
import { spawn } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const web = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const envFile = resolve(web, '../.env');

function appId() {
  // Node 20.2 has no --env-file; read only the one key we need.
  const fromEnv = process.env.VK_APP_ID;
  const line = existsSync(envFile)
    ? readFileSync(envFile, 'utf8').split('\n').find((l) => l.startsWith('VK_APP_ID='))
    : undefined;
  const id = (fromEnv ?? line?.slice('VK_APP_ID='.length) ?? '').trim().replace(/^["']|["']$/g, '');
  if (!/^\d+$/.test(id)) {
    console.error('VK_APP_ID must be set to the numeric Mini App id (in .env or the environment).');
    process.exit(1);
  }
  return id;
}

function run(bin, args, env = {}) {
  const child = spawn(resolve(web, 'node_modules/.bin', bin), args, {
    cwd: web,
    stdio: 'inherit',
    // Node 20.0–20.3 gives each connect attempt 250 ms when racing IPv4/IPv6,
    // which fails over slow links (VPN) with ERR_SOCKET_CONNECTION_TIMEOUT.
    env: {
      ...process.env,
      NODE_OPTIONS: `${process.env.NODE_OPTIONS ?? ''} --no-network-family-autoselection`.trim(),
      ...env,
    },
  });
  return child;
}

const id = appId();
switch (process.argv[2]) {
  case 'deploy': {
    run('vk-miniapps-deploy', [], { MINI_APPS_APP_ID: id }).on('exit', (code) => process.exit(code ?? 1));
    break;
  }
  case 'tunnel': {
    // HMR has to reach the page through the tunnel's HTTPS port, not 5173.
    const vite = run('vite', [], { VK_TUNNEL: '1' });
    const tunnel = run('vk-tunnel', [`--app_id=${id}`]);
    const stop = () => {
      vite.kill();
      tunnel.kill();
    };
    vite.on('exit', stop);
    tunnel.on('exit', stop);
    process.on('SIGINT', stop);
    break;
  }
  default:
    console.error('usage: node scripts/vk.mjs <deploy|tunnel>');
    process.exit(1);
}
