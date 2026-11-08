import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { SeatView, relativeSeats, seatStatus } from '../components/SeatView';
import { LegalActions } from '../components/LegalActions';
import { Ring, RingBanner, tableMarks } from '../screens/Ring';
import { Table } from '../screens/Table';
import { TableOnline } from '../screens/TableOnline';
import { RaiseSheet } from '../components/RaiseSheet';
import { Home } from '../screens/Home';
import { engine, initEngine, tableEngine } from '../engine/engine';
import type { EngineResult, TableEngineResult, TableOutcome, TableView } from '../engine/types';
import type { Duel } from '../pvp/useDuel';
import { storage } from '../game/storage';

vi.mock('../platform', () => ({ platform: { name: 'vk', label: 'VK', canHost: true, getUser: async () => null, inviteLink: () => 'https://vk.com/app54872714#room=test', copy: async () => true, share: async () => true } }));
vi.mock('../game/analytics', () => ({ track: vi.fn() }));
const noop = () => {};
const tableProps = { paused: false, notice: null, onRetrySave: noop, onAct: noop, onNext: noop, onMenu: noop, onTutorialStep: noop, onSkipTutorial: noop };
function ok<T extends EngineResult | TableEngineResult>(r: T): Extract<T, { ok: true }> { if (!r.ok) throw new Error(r.error.message); return r as Extract<T, { ok: true }>; }
let views: TableView[];
let result: TableView;
beforeAll(async () => {
  await initEngine(readFileSync(new URL('../engine/generated/poker.wasm', import.meta.url)));
  views = [ok(tableEngine.start(['alex', 'tight'], '1234567890abcdef')).view, ok(tableEngine.start(['alex', 'tight', 'aggressor'], '1234567890abcdef')).view];
  let r = ok(tableEngine.start(['alex', 'tight', 'aggressor'], '1234567890abcdef'));
  let n = 0;
  while (r.view.phase === 'in_hand' && ++n < 100) r = r.view.to_act === r.view.me ? ok(tableEngine.act(r.state, { type: r.view.legal!.check ? 'check' : 'call' })) : ok(tableEngine.botAct(r.state));
  result = r.view;
  expect(result.phase).toBe('hand_over');
});

describe('3.1 presentation invariants', () => {
  it.each([2, 3, 4])('rotates %i seats while preserving their absolute indices', n => {
    for (let me = 0; me < n; me++) {
      expect(relativeSeats(n, me)).toHaveLength(n - 1);
      expect(relativeSeats(n, me)).not.toContain(me);
      expect(new Set([...relativeSeats(n, me), me]).size).toBe(n);
    }
  });
  it('has one status, with disconnected distinct from folded', () => {
    expect(seatStatus({ name: 'A', folded: true, action: 'Сброс' })).toBe('Сбросил');
    expect(seatStatus({ name: 'A', disconnected: true, folded: true })).toBe('Нет связи');
    expect(seatStatus({ name: 'A', allIn: true })).toBe('All-in');
    expect(seatStatus({ name: 'A', eliminated: true })).toBe('Выбыл');
    const html = renderToStaticMarkup(<SeatView name="A" leftMs={7000} />);
    expect(html).not.toContain('seat-view__clock');
  });
  it('renders only server legal actions, and blocks them during reconnect', () => {
    const legal = { fold: true, check: false, call: 80, call_is_all_in: false, raise: null };
    const html = renderToStaticMarkup(<LegalActions legal={legal} disabled turnKey="1" onAct={noop} />);
    expect(html).toContain('Колл 80'); expect(html).not.toContain('Чек'); expect(html).not.toContain('Рейз');
    expect(html.match(/disabled=""/g)).toHaveLength(2);
  });
  it.each([0, 1])('renders all seats with exactly one actor at a multiway table (%i)', i => {
    const view = views[i];
    const html = renderToStaticMarkup(<Ring {...tableProps} view={view} />);
    expect(html.match(/class="seat-view(?: |")/g)).toHaveLength(view.seats.length);
    expect(html.match(/class="seat-view[^"\n]*is-active/g)).toHaveLength(1);
    expect(html.match(/aria-label="Общие карты"/g)).toHaveLength(1);
    expect(html.match(/card--back/g)).toHaveLength((view.seats.length - 1) * 2);
  });
  it('keeps opponent cards hidden after a fold in heads-up', () => {
    let r = ok(engine.newMatch('alex', '1234567890abcdef'));
    while (r.view.to_act !== 'human') r = ok(engine.botAct(r.state));
    r = ok(engine.act(r.state, { type: 'fold' }));
    expect(r.view.outcome?.showdown).toBeNull();
    const html = renderToStaticMarkup(<Table {...tableProps} view={r.view} />);
    expect(html.match(/card--back/g)).toHaveLength(2);
    expect(html).toContain('Вы сбросили карты');
  });
  it('keeps all participants in shared best-five marks', () => {
    const outcome = { ...result.outcome!, showdown: [0,1,2,3].map(() => ({ hole: ['As','Ad'] as [string,string], best: ['Kh'], category: 'pair' as const })) };
    const marks = tableMarks(outcome, 2, ['A','B','C','D'])('Kh').marks;
    expect(marks.map(m => m.label)).toEqual(['1','2','3','4']);
    expect(marks[2].color).toBe('#38BDF8');
  });
  it('shows authoritative side-pot amounts and total awards without guessing shares', () => {
    // Presentation contract fixture: server is authoritative for payouts.
    const outcome: TableOutcome = { ...result.outcome!, pots: [{amount:300, eligible:[0,1,2], winners:[0,1]}, {amount:100, eligible:[1,2], winners:[2]}], awarded:[150,150,100,0], net:[50,50,0,-100], showdown:null, pot:400 };
    const html = renderToStaticMarkup(<RingBanner view={result} outcome={outcome} />);
    expect(html).toContain('Банк разделён'); expect(html).toContain('Побочные: 1'); expect(html).toContain('Побочный банк 1');
    expect(html.match(/\+150/g)).toHaveLength(2); expect(html).toContain('+100');
  });
  it.skipIf(!process.env.BLITZ_UI_OUTPUT)('writes a reproducible visual fixture gallery from real WASM views', () => {
    const dir = process.env.BLITZ_UI_OUTPUT!; mkdirSync(dir, { recursive: true });
    const css = readFileSync(new URL('../styles.css', import.meta.url), 'utf8');
    const duel = { state: { conn:'online', connError:null, room:null, preview:null, tablePreview:null, error:null, pending:false, reused:false, table: { table_id:'qa-lobby', code:'112474', status:'lobby', close_reason:null, my_seat:0, you_are_host:true, expires_at_ms:null, game:null, again:{votes:0,needed:0,mine:false}, seats:[{type:'player', name:'Вы', photo:null, connected:true, reconnect_deadline_ms:null, is_host:true, is_me:true, gone:false, again:false}, {type:'bot',opponent:'alex'}, {type:'bot',opponent:'aggressor'}, {type:'empty'}] } }, serverNow:()=>0, createTable:noop, removeBot:noop, addBot:noop, startTable:noop } as unknown as Duel;
    const home = <Home stats={storage.loadStats()} hasSavedMatch={false} hasSavedTable={false} tutorialInProgress={false} justFinished={false} opponent="alex" onStart={noop} onContinue={noop} onRestart={noop} onContinueTutorial={noop} onSkipTutorial={noop} onRules={noop} onStats={noop} onPickOpponent={noop} onTable={noop} onDuel={noop} onFriends={noop} onJoinByCode={noop} />;
    let mid = ok(tableEngine.start(['alex', 'tight', 'aggressor'], 'feedfacecafebeef'));
    let guard = 0;
    while (++guard < 150 && !(mid.view.street === 'turn' && mid.view.to_act === mid.view.me)) {
      if (mid.view.phase === 'hand_over') mid = ok(tableEngine.next(mid.state));
      else if (mid.view.phase === 'match_over') break;
      else mid = mid.view.to_act === mid.view.me ? ok(tableEngine.act(mid.state, { type: mid.view.legal!.check ? 'check' : 'call' })) : ok(tableEngine.botAct(mid.state));
    }
    const fixtures = {
      home,
      table2:<Table {...tableProps} view={ok(engine.newMatch('alex','1234567890abcdef')).view} />,
      table3:<Ring {...tableProps} view={views[0]} />,
      table4:<Ring {...tableProps} view={mid.view} />,
      showdown:<Ring {...tableProps} view={result} />,
      lobby:<TableOnline duel={duel} inviteId={null} seats={4} onExit={noop} onRules={noop} />,
      raise:<><Ring {...tableProps} view={views[1]} /><RaiseSheet option={{kind:'raise',min_to:40,max_to:600,step:20,presets:[{kind:'half_pot',to:60,enabled:true},{kind:'pot',to:80,enabled:true},{kind:'all_in',to:600,enabled:true}]}} onConfirm={noop} onCancel={noop} /></>,
    };
    for (const [key,node] of Object.entries(fixtures)) {
      const markup = renderToStaticMarkup(node);
      writeFileSync(`${dir}/${key}.html`, `<!doctype html><html lang="ru"><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><style>${css}</style><div class="app"><div class="column">${markup}</div></div><script>setTimeout(()=>{ const width=innerWidth; const bad=[...document.querySelectorAll('button,.board,.pot,.seat__stack')].filter(e=>{const r=e.getBoundingClientRect();return r.width&& (r.left < -1 || r.right>width+1)}).map(e=>e.className);parent.postMessage({fixture:'${key}',width,overflow:document.documentElement.scrollWidth>width,clipped:bad},'*'); },300)</script>`);
    }
  });
});
