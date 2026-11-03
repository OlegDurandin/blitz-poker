import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import { engine, initEngine } from './engine';
import type { EngineResult, HandRecord, PlayerView, ReplayFrame } from './types';

beforeAll(async () => {
  await initEngine(readFileSync(new URL('./generated/poker.wasm', import.meta.url)));
});

function ok(res: EngineResult) {
  if (!res.ok) throw new Error(`${res.error.code}: ${res.error.message}`);
  return res;
}

const chips = (v: PlayerView) => v.stacks[0] + v.stacks[1] + v.bets[0] + v.bets[1] + v.pot;

describe('wasm engine', () => {
  it('plays a full match with the bot, conserving chips and hiding bot cards', () => {
    let res = ok(engine.newMatch('0123456789abcdef'));
    let steps = 0;
    while (res.view.phase !== 'match_over') {
      expect(++steps).toBeLessThan(400);
      const v = res.view;
      expect(chips(v)).toBe(1200);
      if (v.phase === 'in_hand') expect(v.opponent_hole).toBeNull();
      if (v.phase === 'hand_over') {
        res = ok(engine.next(res.state));
      } else if (v.to_act === 'bot') {
        res = ok(engine.botAct(res.state));
        expect(res.bot_action).toBeDefined();
      } else {
        const legal = v.legal!;
        // Mix in min-raises so raise validation is exercised too.
        const action =
          legal.raise && steps % 5 === 0
            ? ({ type: 'raise_to', amount: legal.raise.min_to } as const)
            : legal.check
              ? ({ type: 'check' } as const)
              : ({ type: 'call' } as const);
        res = ok(engine.act(res.state, action));
      }
    }
    const result = res.view.result!;
    expect(result.final_stacks[0] + result.final_stacks[1]).toBe(1200);
    expect(res.view.history).toHaveLength(result.hands_played);
  });

  it('restores a snapshot without reshuffling', () => {
    const start = ok(engine.newMatch('feedfacecafebeef'));
    const restored = ok(engine.load(JSON.parse(JSON.stringify(start.state))));
    expect(restored.view.hole).toEqual(start.view.hole);
    expect(restored.view).toEqual(start.view);
  });

  it('rejects out-of-range raises and leaves the caller state untouched', () => {
    const t = ok(engine.newTutorial());
    const raise = t.view.legal!.raise!;
    const low = engine.act(t.state, { type: 'raise_to', amount: raise.min_to - 1 });
    const high = engine.act(t.state, { type: 'raise_to', amount: raise.max_to + 1 });
    expect(low.ok || high.ok).toBe(false);
    expect(!low.ok && low.error.code).toBe('invalid_amount');
    expect(!high.ok && high.error.code).toBe('invalid_amount');
  });

  it('reports incompatible and tampered saves', () => {
    const t = ok(engine.newMatch('00000000000000aa'));
    const old = { ...(t.state as object), schema: 99 };
    const res = engine.load(old);
    expect(!res.ok && res.error.code).toBe('incompatible_save');
    const cheat = { ...(t.state as object), stacks: [1000, 600] };
    const res2 = engine.load(cheat);
    expect(!res2.ok && res2.error.code).toBe('corrupt_save');
  });

  it('archives every hand and replays it to the recorded outcome without touching the match', () => {
    let res = ok(engine.newMatch('5eed5eed5eed5eed'));
    let guard = 0;
    while (res.view.phase !== 'match_over') {
      expect(++guard).toBeLessThan(400);
      const v = res.view;
      if (v.phase === 'hand_over') res = ok(engine.next(res.state));
      else if (v.to_act === 'bot') res = ok(engine.botAct(res.state));
      else {
        // Fold now and then so some bot hands stay hidden.
        const a = v.legal!.fold && guard % 3 === 0 ? 'fold' : v.legal!.check ? 'check' : 'call';
        res = ok(engine.act(res.state, { type: a }));
      }
    }
    const before = JSON.stringify(res.state);
    const history = engine.history(res.state);
    if (!history.ok) throw new Error(history.error.message);
    expect(history.hands).toHaveLength(res.view.result!.hands_played);

    // Internal snapshot: both players' hole cards per hand (never shown in the UI).
    const internal = (res.state as { history: Array<{ holes: [string[], string[]] }> }).history;
    let expectedStart: [number, number] = [600, 600];
    history.hands.forEach((h: HandRecord, i) => {
      expect(h.start_stacks).toEqual(expectedStart);
      const r = engine.replay(h);
      if (!r.ok) throw new Error(r.error.message);
      const frames: ReplayFrame[] = r.frames;
      const last = frames.at(-1)!;
      expect(last.event.type).toBe('award');
      expect(last.stacks[0] - h.start_stacks[0]).toBe(h.outcome!.net[0]);
      for (const f of frames) expect(f.stacks[0] + f.stacks[1] + f.bets[0] + f.bets[1] + f.pot).toBe(1200);
      if (!h.outcome!.showdown) {
        const json = JSON.stringify(frames) + JSON.stringify(h);
        for (const c of internal[i].holes[1]) expect(json).not.toContain(`"${c}"`);
      }
      expectedStart = last.stacks;
    });
    expect(JSON.stringify(res.state)).toBe(before);
  });

  it('archives an abandoned match with its unfinished hand', () => {
    let res = ok(engine.newMatch('abad0abad0abad00'));
    if (res.view.to_act === 'bot') res = ok(engine.botAct(res.state));
    const history = engine.history(res.state);
    if (!history.ok) throw new Error(history.error.message);
    const current = history.hands.at(-1)!;
    expect(current.outcome).toBeNull();
    const r = engine.replay(current);
    expect(r.ok && r.frames.at(-1)!.event.type).toBe('unfinished');
  });

  it('tutorial is a single labelled hand with a passive bot', () => {
    let res = ok(engine.newTutorial());
    expect(res.view.kind).toBe('tutorial');
    expect(res.view.hands_total).toBe(1);
    expect(res.view.hole).toEqual(['As', 'Kc']);
    res = ok(engine.act(res.state, { type: 'call' }));
    res = ok(engine.botAct(res.state));
    expect(res.bot_action).toEqual({ type: 'check' });
  });
});
