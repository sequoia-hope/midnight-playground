// Race-at-night pictures (the headlight spot, D760/D761; the effects and
// their headlight pools, WP 4.4, D805): the JS game and the Rust web build
// at the same race time, same seed, the autopilot driving.
//
//   node tools/parity/race-night.mjs --side js|rust --level coast --time 20
//       --device desktop|iphone --hq 1|0 [--dist next] [--backend webgpu|webgl2]
//       [--name tag] [--perf secs] [--out dir]
//
// JS: answered from the worktree by interception, ?parity=1 (fixed ticks,
// ticks=16 to fast-forward), stopped exactly at --time through
// __parity.onTick; pictured as drawn (-js), with the effects hidden
// (-jsnofx) and also with the spot off (-jsnospot). Rust: through the
// registered server (--port, else $PORT, else `proj port`), the page
// itself asking for the screenshot the first frame the race time reaches
// --time. Writes <out>/<level>-<time>-<device>-hq<0|1>-<tag>[-gl].png
// (default parity/report/effects); tools/parity/lum.py measures boxes.

import puppeteer from 'puppeteer-core';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const args = process.argv.slice(2);
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
const side = opt('--side', 'rust');
const level = opt('--level', 'coast');
const tick = Number(opt('--time', 6)); // race seconds after the start
const device = opt('--device', 'desktop');
const hq = opt('--hq', '1') === '1';
const dist = opt('--dist', 'next');
const backend = opt('--backend', 'webgpu');
const tag = opt('--name', side === 'js' ? 'js' : dist);
const perfSecs = Number(opt('--perf', 0));
const timeoutMs = Number(opt('--timeout', 420000));
const OUT = path.resolve(opt('--out', path.join(ROOT, 'parity/report/effects')));
const name = `${level}-${tick}-${device}-hq${hq ? 1 : 0}-${tag}${backend === 'webgl2' ? '-gl' : ''}.png`;
fs.mkdirSync(OUT, { recursive: true });

const DEVICES = {
  desktop: { viewport: { width: 1280, height: 800, deviceScaleFactor: 1 } },
  iphone: {
    userAgent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1',
    viewport: { width: 390, height: 844, deviceScaleFactor: 3, isMobile: true, hasTouch: true, isLandscape: false },
  },
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function baseUrl() {
  let port = process.env.PORT;
  if (!port) port = execFileSync('proj', ['port'], { cwd: ROOT, encoding: 'utf8' }).trim();
  if (!/^\d+$/.test(port)) throw new Error(`no port: proj port gave "${port}"`);
  const common = path.resolve(ROOT, execFileSync('git', ['rev-parse', '--git-common-dir'], { cwd: ROOT, encoding: 'utf8' }).trim());
  const rel = path.relative(path.dirname(common), ROOT);
  return `http://127.0.0.1:${port}/${rel ? rel.split(path.sep).join('/') + '/' : ''}`;
}

const flags = ['--use-angle=vulkan', '--enable-gpu', '--ignore-gpu-blocklist'];
if (perfSecs) flags.push('--disable-gpu-vsync', '--disable-frame-rate-limit');
if (side === 'rust' && backend === 'webgpu') flags.push('--enable-unsafe-webgpu', '--enable-features=Vulkan');
if (side === 'rust' && backend === 'webgl2') flags.push('--disable-blink-features=WebGPU', '--disable-features=WebGPU');
const browser = await puppeteer.launch({
  executablePath: process.env.CHROME_PATH || '/usr/bin/google-chrome',
  headless: 'new', args: flags, protocolTimeout: timeoutMs,
});
const errors = [];
const timer = setTimeout(() => { console.error('timeout'); browser.close().finally(() => process.exit(2)); }, timeoutMs);
// Frame times over `secs` seconds (rAF intervals), median and p90 in ms.
const PERF = (secs) => new Promise((res) => {
  const t = []; let last = performance.now(); const end = last + secs * 1000;
  const f = (now) => { t.push(now - last); last = now; if (now < end) requestAnimationFrame(f); else { t.sort((a, b) => a - b); res({ n: t.length, median: t[t.length >> 1], p90: t[Math.floor(t.length * 0.9)] }); } };
  requestAnimationFrame(f);
});
try {
  const context = await browser.createBrowserContext();
  const page = await context.newPage();
  const cdp = await page.createCDPSession();
  const ev = async (fn, ...a) => {
    const expression = typeof fn === 'function' ? `(${fn})(...${JSON.stringify(a)})` : fn;
    const r = await cdp.send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true, userGesture: false });
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || r.exceptionDetails.text);
    return r.result.value;
  };
  page.on('pageerror', (e) => errors.push(String(e)));
  page.on('console', (m) => { if (m.type() === 'error' && !/Failed to load resource|KHR_parallel/.test(m.text())) errors.push(m.text()); if (process.env.MR_VERBOSE) console.log(`[page] ${m.text()}`); });
  await page.emulate({ userAgent: DEVICES[device].userAgent || await browser.userAgent(), viewport: DEVICES[device].viewport });
  await page.evaluateOnNewDocument((h) => { localStorage.setItem('mr.hq', JSON.stringify(h)); }, hq);
  const t0 = Date.now();
  if (side === 'js') {
    const ORIGIN = 'https://midnight-racer.test';
    const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.json': 'application/json', '.png': 'image/png', '.jpg': 'image/jpeg', '.svg': 'image/svg+xml', '.webmanifest': 'application/manifest+json', '.mp3': 'audio/mpeg', '.wasm': 'application/wasm' };
    await page.setRequestInterception(true);
    page.on('request', (req) => {
      const u = new URL(req.url());
      if (u.origin !== ORIGIN) return req.abort('blockedbyclient');
      const file = path.resolve(ROOT, decodeURIComponent(u.pathname).replace(/^\/+/, '') || 'index.html');
      if (!fs.existsSync(file) || fs.statSync(file).isDirectory()) return req.respond({ status: 404, body: 'not found' });
      req.respond({ status: 200, contentType: TYPES[path.extname(file)] || 'application/octet-stream', body: fs.readFileSync(file) });
    });
    // Count the race's ticks from the first; stop exactly at `tick`.
    await page.evaluateOnNewDocument((stopAt) => {
      window.__tk = 0;
      let p;
      Object.defineProperty(window, '__parity', {
        configurable: true,
        get() { return p; },
        set(v) {
          p = v;
          v.onTick = (race) => { window.__tk++; if (race.time >= stopAt) { window.__stopped = true; v.fixed.ticks = 0; } };
        },
      });
    }, tick);
    await page.goto(`${ORIGIN}/index.html?level=${level}&autostart=sports&autodrive=1&parity=1&seed=1&ticks=16&pursuit=0`);
    for (;;) {
      const s = await ev(() => ({ tk: window.__tk, stopped: window.__stopped }));
      if (s.stopped) break;
      if (Date.now() - t0 > timeoutMs) throw new Error(`js: tick ${s.tk} after ${timeoutMs} ms`);
      await sleep(100);
    }
    await sleep(800); // a few frames of the stopped race
    let perf = null;
    if (perfSecs) perf = await ev(PERF, perfSecs);
    await page.screenshot({ path: path.join(OUT, name) });
    // The same frozen frame without the effects (pools, smoke, sparks,
    // skids), then also without the spot.
    await ev(() => { const e = window.__race.effects; for (const c of e.cars) c.pool.visible = false; e.smoke.points.visible = false; e.sparks.points.visible = false; e.skids.mesh.visible = false; });
    await sleep(500);
    await page.screenshot({ path: path.join(OUT, name.replace('-js.png', '-jsnofx.png')) });
    const spot = await ev(() => window.__race.headlight.intensity);
    await ev(() => { window.__race.headlight.intensity = 0; });
    await sleep(500);
    await page.screenshot({ path: path.join(OUT, name.replace('-js.png', '-jsnospot.png')) });
    await ev((v) => { window.__race.headlight.intensity = v; }, spot);
    const info = await ev(() => ({ tk: window.__tk, time: window.__race.time, s: window.__race.player.s, night: window.__world?.sky?.night, spot: window.__race.headlight.intensity }));
    console.log(JSON.stringify({ file: path.join(OUT, name), ...info, perf }));
  } else {
    await cdp.send('Page.setDownloadBehavior', { behavior: 'allow', downloadPath: OUT });
    const url = `${baseUrl()}dist/${dist}/index.html?level=${level}&autostart=sports&autodrive=1&seed=1&pursuit=0&hq=${hq ? 1 : 0}`;
    console.error(url);
    await page.goto(url);
    let perf = null;
    for (;;) {
      const s = await ev(() => ({ st: window.__mr?.state, err: window.__mr?.error, tk: window.__mr?.race?.time, backend: window.__mr?.backend }));
      if (s.err || s.st === 'failed') throw new Error(`rust: ${s.err}`);
      if (perfSecs && !perf && s.tk >= tick - perfSecs * 1.1) perf = await ev(PERF, perfSecs);
      if (s.tk >= tick - 1.5) break;
      if (Date.now() - t0 > timeoutMs) throw new Error(`rust: tick ${s.tk} after ${timeoutMs} ms`);
      await sleep(20);
    }
    const file = path.join(OUT, name);
    fs.rmSync(file, { force: true });
    // The page asks for the screenshot itself, the first frame the race
    // time reaches `tick` seconds (no round trip through the tool).
    await ev((n, t) => new Promise((res) => {
      const f = () => { const r = window.__mr.race; if (r && r.time >= t) { window.__shotAt = [r.tick, r.time]; window.__mr.screenshot(n); res(); } else requestAnimationFrame(f); };
      requestAnimationFrame(f);
    }), name, tick);
    const a = await ev(() => window.__shotAt);
    const b = await ev(() => [window.__mr.race.tick, window.__mr.race.time]);
    for (let i = 0; i < 100 && !fs.existsSync(file); i++) await sleep(100);
    const info = await ev(() => ({ backend: window.__mr.backend, time: window.__mr.race.time, s: window.__mr.race.s }));
    console.log(JSON.stringify({ file: fs.existsSync(file) ? file : 'no screenshot', ticks: [a, b], ...info, perf }));
  }
} catch (e) {
  errors.push(String(e.message || e));
} finally {
  clearTimeout(timer);
  await browser.close();
}
if (errors.length) { console.error(`errors:\n  ${errors.join('\n  ')}`); process.exit(1); }
