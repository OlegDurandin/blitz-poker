// Loads the Rust engine (crates/poker-wasm) and exposes typed commands.
// ABI: alloc(len) → ptr, call(ptr, len) → response length, out_ptr() → response.

import type { EngineResult, HandRecord, HistoryResult, MatchState, Opponent, PlayerAction, ReplayResult } from './types';

interface Exports {
  memory: WebAssembly.Memory;
  alloc(len: number): number;
  call(ptr: number, len: number): number;
  out_ptr(): number;
}

let wasm: Exports | null = null;
const encoder = new TextEncoder();
const decoder = new TextDecoder();

/** `source` lets tests pass the module bytes directly. */
export async function initEngine(source?: BufferSource): Promise<void> {
  if (wasm) return;
  let bytes = source;
  if (!bytes) {
    const { default: url } = await import('./generated/poker.wasm?url');
    // arrayBuffer() rather than instantiateStreaming: some hosts serve .wasm
    // without the application/wasm MIME type.
    const response = await fetch(url);
    if (!response.ok) throw new Error(`engine download failed: ${response.status}`);
    bytes = await response.arrayBuffer();
  }
  const { instance } = await WebAssembly.instantiate(bytes, {});
  wasm = instance.exports as unknown as Exports;
}

function call<T = EngineResult>(request: object): T {
  if (!wasm) throw new Error('engine not initialised');
  const input = encoder.encode(JSON.stringify(request));
  const ptr = wasm.alloc(input.length);
  new Uint8Array(wasm.memory.buffer, ptr, input.length).set(input);
  const len = wasm.call(ptr, input.length);
  // Read after the call: memory may have grown and detached older views.
  const output = new Uint8Array(wasm.memory.buffer, wasm.out_ptr(), len);
  return JSON.parse(decoder.decode(output)) as T;
}

function randomSeed(): string {
  const words = crypto.getRandomValues(new Uint32Array(2));
  return Array.from(words, (w) => w.toString(16).padStart(8, '0')).join('');
}

export const engine = {
  newMatch: (opponent: Opponent = 'alex', seed: string = randomSeed()) => call({ cmd: 'new_match', seed, opponent }),
  newTutorial: () => call({ cmd: 'new_tutorial' }),
  /** Validates a stored snapshot and returns its view. */
  load: (state: MatchState) => call({ cmd: 'view', state }),
  act: (state: MatchState, action: PlayerAction) => call({ cmd: 'act', state, action }),
  botAct: (state: MatchState) => call({ cmd: 'bot_act', state }),
  next: (state: MatchState) => call({ cmd: 'next', state }),
  /** The human's hand records, including a hand still in progress. */
  history: (state: MatchState) => call<HistoryResult>({ cmd: 'history', state }),
  /** Public table states, event by event; never mutates any match. */
  replay: (hand: HandRecord) => call<ReplayResult>({ cmd: 'replay', hand }),
};
