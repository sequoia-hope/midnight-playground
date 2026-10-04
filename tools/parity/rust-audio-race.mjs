// The race's audio in the Rust client's web build, against the JS game's
// (roadmap M5; DECISIONS D516): headless Chrome on the GPU, the page and its
// files answered from the working tree by request interception (no server,
// no port), the scripted race of the audio reference's `race` drive
// (Sierra, the sports car, seed 1, the autopilot; parity/golden/audio/
// drives.json), with `?audiolog=1` so the client records every call it
// makes on its audio as the JS reference's facade recorder writes them.
//
//   cargo xtask web --release && node tools/parity/rust-audio-race.mjs
//       [--backend webgpu|webgl2] [--scene <url>] [--ticks 5400] [--tolerance 1e-9]
//
// Checks, in order:
//   1. Nothing plays before the first gesture: the page loads, the race is
//      made, the audio graph is built on a context that stays suspended
//      with its clock at 0 (Chrome's autoplay policy as a phone's: a user
//      activation is required; the page is read through Runtime.evaluate
//      without a user gesture, as test/e2e/harness.js does, since
//      puppeteer's page.evaluate counts as one).
//   2. A key press (the gesture) starts the context: it runs.
//   3. The calls of the race's first `--ticks` ticks are the JS drive's
//      (`drive-race.jsonl.gz`), line for line: the same methods in the
//      same ticks with the same arguments, bit for bit, except the pans the
//      JS takes from its camera's matrix (the impacts against cars and the
//      rival engines), which differ in the last bits (three's quaternion
//      round trip): those within `--tolerance`. A contact's pan depends on
//      which ticks share a rendered frame (the JS drive ran two a frame),
//      so on a browser frame of another length it may differ more; those are
//      counted and reported. Calls before the race's `startRace` sequence
//      (the graph built behind the loading screen) and the gestures'
//      wake-ups (`init`, `unlock` after it) are reported, not compared.
//
// The scene is Sierra's base export by default (the full one is over the
// interception's size limit, D106); the sound does not depend on it.

import puppeteer from 'puppeteer-core';
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { ROOT, cacheDir } from './lib/jstree.mjs';

const args = process.argv.slice(2);
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
const backend = opt('--backend', 'webgpu');
const ticks = Number(opt('--ticks', 5400));
const tolerance = Number(opt('--tolerance', 1e-9));
const timeoutMs = Number(opt('--timeout', 600000));
const scene = opt('--scene', '../../' + path.relative(ROOT, path.join(cacheDir('scenes'), 'sierra.base.mrscene')));

const ORIGIN = 'https://midnight-racer.test';
const TYPES = {
  '.html': 'text/html', '.js': 'text/javascript', '.json': 'application/json', '.wasm': 'application/wasm',
  '.png': 'image/png', '.bin': 'application/octet-stream', '.mrscene': 'application/octet-stream', '.mp3': 'audio/mpeg',
};

const js = zlib.gunzipSync(fs.readFileSync(path.join(ROOT, 'parity/golden/audio/drive-race.jsonl.gz'))).toString('utf8')
  .split('\n').filter(Boolean).map((l) => JSON.parse(l)).filter((c) => c[0] < ticks);

const browser = await puppeteer.launch({
  executablePath: process.env.CHROME_PATH || '/usr/bin/google-chrome',
  headless: 'new',
  args: [
    '--autoplay-policy=document-user-activation-required',
    ...(backend === 'webgl2'
      ? ['--disable-blink-features=WebGPU', '--disable-features=WebGPU', '--use-angle=vulkan', '--enable-gpu', '--ignore-gpu-blocklist']
      : ['--enable-unsafe-webgpu', '--enable-features=Vulkan', '--use-angle=vulkan', '--ignore-gpu-blocklist']),
  ],
});
const errors = [];
const problems = [];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
try {
  const page = await browser.newPage();
  await page.setViewport({ width: 1280, height: 800, deviceScaleFactor: 1 });
  page.on('console', (m) => {
    const t = m.text();
    if (m.type() === 'error' && !/Failed to load resource/.test(t)) errors.push(t);
    if (process.env.MR_VERBOSE) console.log(`[page ${m.type()}] ${t}`);
  });
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.setRequestInterception(true);
  page.on('request', (req) => {
    const u = new URL(req.url());
    if (u.origin !== ORIGIN) return req.continue();
    let p = path.join(ROOT, decodeURIComponent(u.pathname));
    if (p.endsWith('/')) p = path.join(p, 'index.html');
    if (!fs.existsSync(p)) return req.respond({ status: 404, body: 'not found' });
    req.respond({ status: 200, contentType: TYPES[path.extname(p)] || 'application/octet-stream', body: fs.readFileSync(p) });
  });
  const query = `level=sierra&car=sports&seed=1&autodrive=1&touch=0&audiolog=1&backend=${backend}&scene=${encodeURIComponent(scene)}`;
  const cdp = await page.createCDPSession();
  // Read the page without handing it a user activation.
  const evaluate = async (expression) => {
    const r = await cdp.send('Runtime.evaluate', { expression, returnByValue: true, userGesture: false });
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || r.exceptionDetails.text);
    return r.result.value;
  };
  await page.goto(`${ORIGIN}/dist/next/index.html?${query}`);
  const t0 = Date.now();
  const look = () => evaluate(`({
    state: window.__mr?.state, ready: window.__mr?.ready, error: window.__mr?.error,
    audio: window.__mr?.audio, race: window.__mr?.race && { tick: window.__mr.race.tick, mode: window.__mr.race.mode },
  })`);

  // 1. The race and its audio graph exist; nothing plays.
  let s;
  for (;;) {
    s = await look();
    if (s.error || s.state === 'failed') throw new Error(`client failed: ${s.error}`);
    if (s.audio?.ready) break;
    if (s.audio && s.audio.context === 'running') problems.push('the context ran before any gesture');
    if (Date.now() - t0 > timeoutMs) throw new Error(`no audio graph after ${timeoutMs} ms (state ${s.state})`);
    await sleep(100);
  }
  const before = [];
  for (let i = 0; i < 20; i++) {
    s = await look();
    before.push(s.audio);
    await sleep(100);
  }
  const ran = before.filter((a) => a.context === 'running' || a.currentTime > 0);
  if (ran.length) problems.push(`before the gesture the context ran: ${JSON.stringify(ran[0])}`);
  console.log(`before the gesture: context ${before.at(-1).context}, clock ${before.at(-1).currentTime} s, graph built (${before.length} looks over 2 s; race ${s.ready ? `running, tick ${s.race?.tick}` : 'not started'})`);

  // 2. The gesture.
  // A key the game does not use (a modifier would not activate the page).
  await page.keyboard.press('KeyQ');
  for (;;) {
    s = await look();
    if (s.audio?.context === 'running') break;
    if (Date.now() - t0 > timeoutMs) throw new Error('the context did not start after the gesture');
    await sleep(50);
  }
  console.log(`after the gesture: context running (race tick ${s.race?.tick ?? 0})`);

  // 3. The race.
  let last = -1, lastAt = Date.now();
  for (;;) {
    s = await look();
    if (s.error || s.state === 'failed') throw new Error(`client failed: ${s.error}`);
    const tick = s.race?.tick ?? 0;
    if (tick >= ticks) break;
    if (tick !== last) { last = tick; lastAt = Date.now(); }
    if (Date.now() - lastAt > 30000) throw new Error(`the race stopped at tick ${tick}`);
    if (Date.now() - t0 > timeoutMs) throw new Error(`race at tick ${tick} after ${timeoutMs} ms`);
    await sleep(500);
  }
  const cost = await evaluate('window.__mr.audio');
  console.log(`the audio on the main thread: graph built in ${cost.prepareMs.toFixed(1)} ms behind the loading screen; per frame ${cost.frameMsMean.toFixed(3)} ms on average, ${cost.frameMsMax.toFixed(2)} ms at most (at tick ${cost.frameMsMaxTick})`);
  const all = (await evaluate('window.__mr.audioLog || []')).map((l) => JSON.parse(l));
  const start = all.findIndex((c) => c[1] === 'setPaused');
  if (start < 0) throw new Error('no startRace calls in the log');
  console.log(`before startRace: ${all.slice(0, start).map((c) => c[1]).join(', ') || 'nothing'}`);
  // startRace's own seven calls, then everything but the gestures' wake-ups.
  const race = all.slice(start).filter((c) => c[0] < ticks);
  const ours = [...race.slice(0, 7), ...race.slice(7).filter((c) => c[1] !== 'init' && c[1] !== 'unlock')];
  console.log(`gestures' wake-ups during the race: ${race.length - ours.length} calls (${race.slice(7).filter((c) => c[1] === 'init').map((c) => 'tick ' + c[0]).join(', ')})`);

  // Compare.
  let maxPan = 0, panAt = null, exact = 0, pans = 0, rivalMax = 0;
  const off = [];
  const panDiff = (a, b, where, rival) => {
    const d = Math.abs(a - b);
    pans++;
    if (d > maxPan) { maxPan = d; panAt = where; }
    if (rival) rivalMax = Math.max(rivalMax, d);
    if (d > tolerance) off.push({ where, rust: a, js: b, rival });
    return true;
  };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  let first = null;
  for (let i = 0; i < Math.max(ours.length, js.length); i++) {
    const a = ours[i], b = js[i];
    if (!a || !b) { first ??= { i, rust: a, js: b, why: 'one log ends' }; break; }
    if (same(a, b)) { exact++; continue; }
    const where = `line ${i + 1} (tick ${b[0]}, ${b[1]})`;
    let ok = a[0] === b[0] && a[1] === b[1] && a.length === b.length;
    if (ok && b[1] === 'impact') ok = same(a[2], b[2]) && panDiff(a[3], b[3], where, false);
    else if (ok && b[1] === 'setRivalEngines') {
      ok = a[2].length === b[2].length && a[2].every((r, k) => {
        const { pan: p1, ...r1 } = r, { pan: p2, ...r2 } = b[2][k];
        return same(r1, r2) && panDiff(p1, p2, where, true);
      });
    } else ok = false;
    if (!ok) { first ??= { i, rust: a, js: b, why: 'differs' }; }
  }
  if (first) problems.push(`first difference at line ${first.i + 1} (${first.why}):\n    rust ${JSON.stringify(first.rust)}\n    js   ${JSON.stringify(first.js)}`);
  console.log(`calls: ${ours.length} here, ${js.length} in the JS drive; ${exact} identical, the rest differ only in the camera's pans: ${pans} pans, the largest difference ${maxPan.toExponential(2)}${panAt ? ' at ' + panAt : ''} (rivals ${rivalMax.toExponential(2)}); ${off.length} beyond ${tolerance}`);
  for (const o of off.slice(0, 10)) console.log(`  ${o.where}: ${o.rival ? 'rival' : 'contact'} pan ${o.rust} against ${o.js}`);
  if (off.some((o) => o.rival)) problems.push('a rival engine\'s pan is off');
  const out = path.join(ROOT, 'parity/report/rust-audio');
  fs.mkdirSync(out, { recursive: true });
  fs.writeFileSync(path.join(out, 'race.jsonl'), all.map((c) => JSON.stringify(c)).join('\n') + '\n');
  console.log(`the client's log: ${path.relative(ROOT, path.join(out, 'race.jsonl'))}`);
} catch (e) {
  errors.push(String(e.message || e));
} finally {
  await browser.close();
}
for (const p of problems) console.error(`FAIL ${p}`);
if (errors.length) console.error(`errors:\n  ${errors.join('\n  ')}`);
process.exit(errors.length || problems.length ? 1 : 0);
