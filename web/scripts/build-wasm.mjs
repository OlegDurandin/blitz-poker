// Builds the Rust engine to WebAssembly and copies it next to the TS loader.
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const target = 'wasm32-unknown-unknown';

execFileSync(
  'cargo',
  ['build', '-p', 'poker-wasm', '--release', '--target', target],
  { cwd: root, stdio: 'inherit' },
);

const out = resolve(root, 'web/src/engine/generated');
mkdirSync(out, { recursive: true });
copyFileSync(resolve(root, `target/${target}/release/poker_wasm.wasm`), resolve(out, 'poker.wasm'));
console.log('engine → web/src/engine/generated/poker.wasm');
