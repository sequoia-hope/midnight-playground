// Whole-race recordings from the game oracle (roadmap WP 0.4, SPEC 4.6):
// the real game in headless Chrome, real Race.update, with every parity
// hook on (kernel, fixed 1/120 s ticks, quantised input, seeded streams) and
// the autopilot driving. After every tick the trace record is taken
// (tools/parity/lib/trace.mjs); the file keeps every tick's hash and the
// full record every 120 ticks.
//
//   node tools/parity/sim-race.mjs [--only sierra-race,...] [--check]
//
// Recordings go to parity/cache/<js-tree-key>/sim-races/ (regenerated on
// demand); a summary of each (ticks, final hash, results) is committed to
// parity/golden/sim/races.json. --check records each twice and fails unless
// the two files are byte for byte the same.
//
// The capture fails on any page error, and on any error the main loop
// caught and carried on from (main.js keeps running; a recording must not).

import fs from 'node:fs';
import path from 'node:path';
import { launch, openGame } from '../../test/e2e/harness.js';
import { cacheDir, jsTreeKey, ROOT } from './lib/jstree.mjs';
import { readTrace, hashHex } from './lib/trace.mjs';

// Every level in race mode with a different car, so all five specs run;
// Hot Pursuit on every level with police; the cruise loop for three
// minutes; and one hot pursuit from heat 5 for roadblocks, spikes and boxes.
export const RECORDINGS = [
  { id: 'sierra-race', level: 'sierra', car: 'sports' },
  { id: 'coast-race', level: 'coast', car: 'muscle' },
  { id: 'streets-race', level: 'streets', car: 'super' },
  { id: 'desert-race', level: 'desert', car: 'rally' },
  { id: 'seaside-race', level: 'seaside', car: 'electric' },
  { id: 'cruise-3min', level: 'cruise', car: 'sports', ticks: 3 * 60 * 120 },
  { id: 'sierra-pursuit', level: 'sierra', car: 'sports', pursuit: 1 },
  { id: 'coast-pursuit', level: 'coast', car: 'rally', pursuit: 1 },
  { id: 'streets-pursuit', level: 'streets', car: 'muscle', pursuit: 1 },
  { id: 'desert-pursuit', level: 'desert', car: 'electric', pursuit: 1 },
  { id: 'sierra-pursuit-heat5', level: 'sierra', car: 'super', pursuit: 5 },
];

const TICKS_PER_FRAME = 16;
const MAX_TICKS = 15 * 60 * 120; // a race that runs 15 minutes has gone wrong
const SEED = 1;

export function queryFor(r) {
  return [
    'parity=1', `seed=${SEED}`, `ticks=${TICKS_PER_FRAME}`, `level=${r.level}`, `autostart=${r.car}`, 'autodrive=1',
    r.pursuit ? `pursuit=1&heat=${r.pursuit}` : 'pursuit=0',
  ].join('&');
}

// Runs before the page's own scripts: catches window.__parity as hooks.js
// publishes it and attaches the recorder to it.
function recorderInit(maxTicks, stopTicks) {
  const traceUrl = new URL('/tools/parity/lib/trace.mjs', location.href).href;
  const rec = { tick: 0, done: false, error: null, mod: null, writer: null };
  window.__rec = rec;
  import(traceUrl).then((m) => { rec.mod = m; }, (e) => { rec.error = 'loading the trace module: ' + e; });
  let parity;
  Object.defineProperty(window, '__parity', {
    configurable: true,
    get: () => parity,
    set(p) {
      parity = p;
      p.onTick = (race, input) => {
        if (rec.done || rec.error) return;
        try {
          if (!rec.mod) throw new Error('trace module not loaded before the first tick');
          rec.writer ??= new rec.mod.TraceWriter(null);
          rec.tick++;
          rec.writer.add(rec.tick, rec.mod.traceRecord(rec.mod.fromRace(race, rec.tick, p.rngs, input)));
          if (p.errors.length) throw new Error('the main loop caught: ' + p.errors[0]);
          // Done at the tick the results are reported (or after a fixed run).
          if ((stopTicks ? rec.tick >= stopTicks : race.reported) || rec.tick >= maxTicks) {
            rec.done = true;
            // (An array with extra properties: JSON keeps only the elements.)
            const res = race.cruise ? race.cruiseResults() : race.results();
            rec.results = race.cruise ? res : { list: [...res], pursuit: res.pursuit ?? null, laps: res.laps ?? null };
            if (!stopTicks && !race.reported) rec.error = `no result after ${rec.tick} ticks`;
          }
        } catch (e) { rec.error = String(e.stack || e); }
      };
    },
  });
}

export async function record(browser, r) {
  const t0 = Date.now();
  const game = await openGame(browser, { query: queryFor(r), init: recorderInit, initArgs: [MAX_TICKS, r.ticks || 0] });
  try {
    await game.waitFor(() => window.__rec.done || window.__rec.error || false, { timeout: 30 * 60 * 1000, interval: 500, what: r.id + ' to finish' });
    const st = await game.eval(() => ({ error: window.__rec.error, tick: window.__rec.tick, kernel: Math.__kernel === true }));
    if (st.error) throw new Error(`${r.id}: ${st.error}`);
    if (game.errors.length) throw new Error(`${r.id}: page errors: ${game.errors.join(' | ')}`);
    if (!st.kernel) throw new Error(`${r.id}: the kernel was not on`);
    const meta = { id: r.id, level: r.level, car: r.car, pursuit: r.pursuit || 0, seed: SEED, query: queryFor(r), jsTree: jsTreeKey(), source: 'game' };
    const b64 = await game.eval((m) => {
      window.__rec.writer.meta = m;
      const bytes = window.__rec.writer.finish();
      let s = '';
      for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
      return btoa(s);
    }, meta);
    const results = await game.eval(() => window.__rec.results);
    return { bytes: Buffer.from(b64, 'base64'), ticks: st.tick, results, seconds: (Date.now() - t0) / 1000 };
  } finally {
    await game.close();
  }
}

function summary(r, out) {
  const tr = readTrace(new Uint8Array(out.bytes));
  const res = out.results;
  return {
    ticks: out.ticks,
    finalHash: hashHex(tr.hashes.at(-1)),
    results: res.cruise ? res : res.list.map((x) => ({ place: x.place, name: x.name, time: x.time, estimated: x.estimated })),
    ...(res.pursuit ? { pursuit: res.pursuit } : {}),
    ...(res.laps ? { laps: res.laps } : {}),
  };
}

async function main() {
  const args = process.argv.slice(2);
  const check = args.includes('--check');
  const only = args.includes('--only') ? args[args.indexOf('--only') + 1].split(',') : null;
  const list = RECORDINGS.filter((r) => !only || only.includes(r.id));
  const dir = cacheDir('sim-races');
  const goldenPath = path.join(ROOT, 'parity/golden/sim/races.json');
  const golden = fs.existsSync(goldenPath) ? JSON.parse(fs.readFileSync(goldenPath, 'utf8')) : { recordings: {} };
  const browser = await launch();
  let failed = 0;
  try {
    for (const r of list) {
      const a = await record(browser, r);
      const file = path.join(dir, r.id + '.trace');
      fs.writeFileSync(file, a.bytes);
      let note = '';
      if (check) {
        const b = await record(browser, r);
        const same = Buffer.compare(a.bytes, b.bytes) === 0;
        note = same ? ', second run identical' : ', SECOND RUN DIFFERS';
        if (!same) failed++;
      }
      golden.recordings[r.id] = { query: queryFor(r), ...summary(r, a) };
      console.log(`${r.id}: ${a.ticks} ticks, ${(a.bytes.length / 1e6).toFixed(1)} MB, ${a.seconds.toFixed(0)} s${note}`);
    }
  } finally {
    await browser.close();
  }
  golden.format = 'parity/trace-format.md';
  golden.jsTree = jsTreeKey();
  fs.mkdirSync(path.dirname(goldenPath), { recursive: true });
  fs.writeFileSync(goldenPath, JSON.stringify(golden, null, 1) + '\n');
  if (failed) { console.error(`${failed} recording(s) not reproducible`); process.exit(1); }
}

if (import.meta.url === `file://${process.argv[1]}`) await main();
