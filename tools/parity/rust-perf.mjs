// Frame time, memory, load time and reloads of the Rust client's web build
// (roadmap WP 2.6, gate G1), and the same flight of the JS game beside it,
// in headless Chrome on the dev machine's GPU.
//
//   cargo xtask web --release
//   node tools/parity/rust-perf.mjs --level sierra [--backend webgpu|webgl2]
//       [--hq 1|0] [--secs N] [--reloads 10] [--reload-levels sierra,coast]
//       [--query "t=0.2&cars=0"]   (more of the page's parameters)
//   node tools/parity/rust-perf.mjs --game js --level sierra [--secs N]
//
// The Rust side runs the client's own measurement page (`index.html?perf=1`,
// crates/mr_game/web/perf.js: the page the owner opens on the phone), and
// reads `window.__mr.perf`. The JS side opens the JS game's debug fly camera
// with the same parameters (s = 80, v = 60, h = 5, back = 14, as
// tools/parity/perf-baseline.mjs) and records the same frame times with the
// same requestAnimationFrame recorder.
//
// Both are loaded through the registered dev server (the full exports are
// too big for request interception, DECISIONS D106): `--url <base>`, else
// http://127.0.0.1 on `--port`, else `$PORT`, else `proj port`; there is no
// default port. The server serves the main checkout, so a worktree is reached under
// its path there. Frame rate is uncapped (no vsync, no frame-rate limit),
// as perf-baseline.mjs measures the JS game. `--backend webgl2` also turns
// WebGPU off in Chrome, so the page must fall back by itself.
//
// Prints the page's report and a JSON summary; writes the full result to
// parity/report/perf/<game>-<level>-<backend>.json.

import puppeteer from 'puppeteer-core';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { ROOT } from './lib/jstree.mjs';

const args = process.argv.slice(2);
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
const game = opt('--game', 'rust');
const level = opt('--level', 'sierra');
const backend = opt('--backend', 'webgpu');
const hq = opt('--hq', '1');
const secs = opt('--secs', null);
const reloads = opt('--reloads', game === 'rust' ? '10' : '0');
const reloadLevels = opt('--reload-levels', null);
const capped = args.includes('--capped');
const timeoutS = Number(opt('--timeout', 1800));

function baseUrl() {
  const given = opt('--url', null);
  if (given) return given.endsWith('/') ? given : given + '/';
  let port = opt('--port', null) || process.env.PORT;
  if (!port) {
    try {
      port = execFileSync('proj', ['port'], { cwd: ROOT, encoding: 'utf8' }).trim();
    } catch (e) {
      throw new Error(`no server address: pass --url or --port, set $PORT, or register the project (proj port failed: ${e.message})`);
    }
  }
  if (!/^\d+$/.test(port)) throw new Error(`proj port gave "${port}"`);
  // The registered server serves the main checkout; a worktree sits below it.
  const common = path.resolve(ROOT, execFileSync('git', ['rev-parse', '--git-common-dir'], { cwd: ROOT, encoding: 'utf8' }).trim());
  const served = path.dirname(common);
  const rel = path.relative(served, ROOT);
  if (rel.startsWith('..')) throw new Error(`${ROOT} is not below the served checkout ${served}`);
  return `http://127.0.0.1:${port}/${rel ? rel.split(path.sep).map(encodeURIComponent).join('/') + '/' : ''}`;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const base = baseUrl();
const flags = ['--ignore-gpu-blocklist', '--enable-gpu', '--use-angle=vulkan', '--enable-precise-memory-info'];
if (!capped) flags.push('--disable-gpu-vsync', '--disable-frame-rate-limit');
if (game === 'rust' && backend === 'webgpu') flags.push('--enable-unsafe-webgpu', '--enable-features=Vulkan');
if (game === 'rust' && backend === 'webgl2') flags.push('--disable-blink-features=WebGPU', '--disable-features=WebGPU');

const browser = await puppeteer.launch({
  executablePath: process.env.CHROME_PATH || '/usr/bin/google-chrome',
  headless: 'new',
  args: flags,
  protocolTimeout: timeoutS * 1000,
  dumpio: !!process.env.MR_DUMPIO,
});
const errors = [];
let result = null;
const load = os.loadavg().map((x) => x.toFixed(1));
try {
  const page = await browser.newPage();
  await page.setViewport({ width: 1280, height: 800, deviceScaleFactor: 1 });
  page.on('console', (m) => {
    const t = m.text();
    if (m.type() === 'error' && !/Failed to load resource/.test(t)) errors.push(t);
    if (process.env.MR_VERBOSE) console.log(`[page ${m.type()}] ${t}`);
  });
  page.on('pageerror', (e) => errors.push(String(e)));
  const t0 = Date.now();
  if (game === 'rust') {
    const q = new URLSearchParams({ level, perf: '1', s: '80', v: '60', hq });
    if (secs) q.set('secs', secs);
    q.set('reloads', reloads);
    if (reloadLevels) q.set('reloadLevels', reloadLevels);
    for (const [k, v] of new URLSearchParams(opt('--query', ''))) q.set(k, v);
    const url = `${base}dist/next/index.html?${q}`;
    console.error(`rust ${backend}: ${url}  (load average ${load.join(' ')})`);
    await page.goto(url);
    let last = '';
    for (;;) {
      const p = await page.evaluate(() => ({ perf: window.__mr?.perf, state: window.__mr?.state, error: window.__mr?.error }));
      if (p.error || p.state === 'failed' || p.state === 'nogpu') throw new Error(`client ${p.state}: ${p.error || ''}`);
      if (p.perf?.phase === 'done') { result = p.perf.result; break; }
      const now = JSON.stringify(p.perf || p.state);
      if (now !== last && process.env.MR_VERBOSE) console.error(now);
      last = now;
      if (Date.now() - t0 > timeoutS * 1000) throw new Error(`not done after ${timeoutS} s (${now})`);
      await sleep(1000);
    }
  } else {
    // The JS game, flown as perf-baseline.mjs flies it, timed as perf.js
    // times the Rust client.
    const url = `${base}index.html?level=${level}&stats=1&s=80&v=60&h=5&back=14${hq === '0' ? '&hq=0' : ''}`;
    console.error(`js: ${url}  (load average ${load.join(' ')})`);
    await page.goto(url);
    await page.waitForFunction('window.__ready === true', { timeout: 120000, polling: 200 });
    const navToReady = await page.evaluate(() => performance.now());
    const len = await page.evaluate(() => (window.__world.track.loop ? window.__world.track.length : window.__world.track.roadEnd - 100));
    const flight = Number(secs) || Math.min(240, len / 60);
    result = await page.evaluate(async (flightS) => {
      const dts = [], at = [], ss = [];
      let last = null, t0 = null, on = true;
      const frame = (now) => {
        if (!on) return;
        requestAnimationFrame(frame);
        if (t0 === null) t0 = now;
        if (last !== null) { dts.push(now - last); at.push((now - t0) / 1000); ss.push(window.__dbg?.s || 0); }
        last = now;
      };
      requestAnimationFrame(frame);
      await new Promise((r) => setTimeout(r, flightS * 1000));
      on = false;
      return { dts, at, ss, loadS: window.__stats?.loadS ?? null, heapMB: performance.memory ? Math.round(performance.memory.usedJSHeapSize / 1048576) : null };
    }, flight);
    result.navToReadyMs = Math.round(navToReady);
    result.flight = summarise(result.dts, result.at, result.ss);
    delete result.dts; delete result.at; delete result.ss;
  }
} catch (e) {
  errors.push(String(e.message || e));
} finally {
  await browser.close();
}

// The same statistics perf.js computes, for the JS side.
function summarise(dts, at, ss) {
  const sorted = Float64Array.from(dts).sort();
  const pct = (p) => sorted[Math.min(sorted.length - 1, Math.floor((p / 100) * sorted.length))];
  const windows = [];
  let n = 0, sum = 0;
  for (const dt of dts) { n++; sum += dt; if (sum >= 500) { windows.push((1000 * n) / sum); n = 0; sum = 0; } }
  windows.sort((a, b) => a - b);
  const w = (p) => windows[Math.min(windows.length - 1, Math.floor((p / 100) * windows.length))];
  const r1 = (x) => Math.round(x * 10) / 10;
  return {
    seconds: r1(at[at.length - 1] || 0), frames: dts.length, fpsMedian: r1(w(50)), fps5: r1(w(5)),
    p50: r1(pct(50)), p90: r1(pct(90)), p95: r1(pct(95)), p99: r1(pct(99)), p999: r1(pct(99.9)), max: r1(sorted[sorted.length - 1]),
    over50First30: dts.filter((d, i) => d > 50 && at[i] <= 30).length, over50: dts.filter((d) => d > 50).length,
    over33: dts.filter((d) => d > 33.4).length,
    slow: dts.map((d, i) => ({ ms: r1(d), t: r1(at[i]), s: Math.round(ss[i]) })).filter((x) => x.ms > 50).slice(0, 20),
    km: [...dts.reduce((m, d, i) => {
      const k = Math.floor(ss[i] / 1000);
      if (!m.has(k)) m.set(k, []);
      m.get(k).push(d);
      return m;
    }, new Map())].map(([km, v]) => {
      const o = Float64Array.from(v).sort();
      const q = (p) => r1(o[Math.min(o.length - 1, Math.floor((p / 100) * o.length))]);
      return { km, frames: v.length, p50: q(50), p95: q(95), max: r1(o[o.length - 1]) };
    }),
  };
}

if (result) {
  result.loadAverage = load;
  result.loadAverageAfter = os.loadavg().map((x) => x.toFixed(1));
  const out = path.join(ROOT, 'parity/report/perf');
  fs.mkdirSync(out, { recursive: true });
  const file = path.join(out, `${game}-${level}-${game === 'rust' ? backend : 'webgl'}.json`);
  fs.writeFileSync(file, JSON.stringify(result, null, 1));
  if (result.text) console.log(result.text);
  const f = result.flight;
  console.log(JSON.stringify({
    game, level, backend: game === 'rust' ? result.device?.backend : 'webgl (three.js)',
    firstFrameS: result.load ? +(result.load.firstFrameMs / 1000).toFixed(2) : null,
    readyS: result.load ? +(result.load.readyMs / 1000).toFixed(2) : +(result.navToReadyMs / 1000).toFixed(2),
    jsLoadS: result.loadS ?? null,
    flight: f, wasmPeakMB: result.verdict?.wasmPeakMB ?? null,
    reloadsWasmMB: result.reloads?.map((x) => x.wasmMB) ?? null,
    load: `${result.loadAverage.join(' ')} → ${result.loadAverageAfter.join(' ')}`,
  }));
  console.error(`written ${file}`);
}
if (errors.length) {
  console.error(`errors:\n  ${errors.join('\n  ')}`);
  process.exit(1);
}
