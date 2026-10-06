// The race HUD beside the JS's (roadmap WP 6.3, D820+): the JS game and the
// Rust web build at the same race time, same seed, the autopilot driving.
//
//   node tools/parity/hud-shots.mjs --side js|rust --level sierra --time 20
//       --device desktop|iphone|iphone-land [--car sports] [--dist next]
//       [--backend webgpu|webgl2] [--perf secs] [--out dir] [--query k=v&…]
//       (a `pursuit=…` in --query replaces the default pursuit=0)
//       [--stage wreck --stage-at t]  (Hot Pursuit, WP 8.3: at race time t
//                         the car is wrecked and dispatch says a line, for
//                         the hold card, the damage bar and the radio;
//                         the Rust side stages it from the page's frame
//                         loop, a frame or two late)
//       [--countdown c]  (instead of --time: the countdown at c seconds, for
//                         the centre pop; the JS shot is then taken at once)
//   node tools/parity/hud-shots.mjs --index [--out dir]
//
// JS: answered from the worktree by interception (its Google Fonts request
// with the bundled Rajdhani, as ui-shots.mjs does), ?parity=1 (fixed
// ticks, ticks=16 to fast-forward), stopped exactly at --time through
// __parity.onTick. Rust: through the registered server (--port, else
// $PORT, else `proj port`), the page itself asking for the screenshot the
// first frame the race time reaches --time. Writes
// <out>/<level>-<time>-<car>-<device>-<js|rust>[-gl].png (default
// parity/report/hud); --index writes the side-by-side page.

import puppeteer from 'puppeteer-core';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const args = process.argv.slice(2);
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
const OUT = path.resolve(opt('--out', path.join(ROOT, 'parity/report/hud')));
fs.mkdirSync(OUT, { recursive: true });

const DEVICES = {
  desktop: { viewport: { width: 1280, height: 800, deviceScaleFactor: 1 } },
  iphone: {
    userAgent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1',
    viewport: { width: 390, height: 844, deviceScaleFactor: 3, isMobile: true, hasTouch: true, isLandscape: false },
  },
  'iphone-land': {
    userAgent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1',
    viewport: { width: 844, height: 390, deviceScaleFactor: 3, isMobile: true, hasTouch: true, isLandscape: true },
  },
};

if (args.includes('--index')) {
  const all = fs.readdirSync(OUT).filter((f) => f.endsWith('.png'));
  const files = all.filter((f) => /-(js|rust)(-gl)?\.png$/.test(f));
  // Crops from tools/parity/hud-crops.py: JS above, Rust below.
  const crops = all.filter((f) => f.startsWith('crop-')).sort()
    .map((f) => `<h2>${f} (JS above, Rust below)</h2><a href="${f}"><img src="${f}" style="max-width:96vw"></a>`);
  const shots = [...new Set(files.map((f) => f.replace(/-(js|rust)(-gl)?\.png$/, '')))].sort();
  const rows = shots.map((s) => {
    const dev = Object.keys(DEVICES).find((d) => s.endsWith('-' + d)) || 'desktop';
    const w = DEVICES[dev].viewport.width;
    const figs = ['js', 'rust', 'rust-gl'].filter((k) => files.includes(`${s}-${k}.png`))
      .map((k) => `<figure><img src="${s}-${k}.png" width="${w}"><figcaption>${{ js: 'JS game', rust: 'Rust (WebGPU)', 'rust-gl': 'Rust (WebGL2)' }[k]}</figcaption></figure>`);
    return `<h2>${s}</h2><div class="pair">${figs.join('')}</div>`;
  });
  fs.writeFileSync(path.join(OUT, 'index.html'), `<!doctype html><meta charset="utf-8"><title>HUD: JS and Rust</title>
<style>body{background:#111;color:#ddd;font:14px system-ui;margin:16px} .pair{display:flex;gap:12px;flex-wrap:wrap;align-items:flex-start} figure{margin:0} img{display:block;height:auto;max-width:48vw;border:1px solid #333} h2{font-size:15px;margin:20px 0 6px}</style>
<h1>The race HUD, JS game against the Rust build</h1>
<p>Same level, seed 1, the autopilot, the same race time. Click a picture for full size.</p>
${crops.join('\n')}
${rows.join('\n').replace(/<img src="([^"]+)"/g, '<a href="$1"><img src="$1"').replace(/<figcaption>/g, '</a><figcaption>')}`);
  console.log(`wrote ${path.relative(ROOT, OUT)}/index.html`);
  process.exit(0);
}

const side = opt('--side', 'rust');
const level = opt('--level', 'sierra');
const cd = args.includes('--countdown') ? Number(opt('--countdown', 1.85)) : null;
const time = cd != null ? `cd${cd}` : Number(opt('--time', 20));
const device = opt('--device', 'desktop');
const car = opt('--car', 'sports');
const dist = opt('--dist', 'next');
const backend = opt('--backend', 'webgpu');
const perfSecs = Number(opt('--perf', 0));
const query = opt('--query', '');
const timeoutMs = Number(opt('--timeout', 300000));
const stage = opt('--stage', '');
const stageAt = Number(opt('--stage-at', 0));
// The staged scenes: what the JS calls, and the Rust staging commands.
const LINE = { text: 'Unit 12, speeder on Red Rock Canyon, moving to intercept.', parts: ['Unit 12.', 'Speeder on Red Rock Canyon, moving to intercept.'] };
const STAGES = {
  wreck: {
    js: (race, line) => { race.pv.hurt(1.0); race.pv.say(line, true); },
    rust: (line) => [{ cmd: 'hurt', d: 1.0 }, { cmd: 'say', text: line.text, parts: line.parts, force: true }],
  },
};
if (stage && !STAGES[stage]) throw new Error(`no stage ${stage}`);
const name = `${level}-${time}${stage ? '-' + stage : ''}-${car}-${device}-${side}${side === 'rust' && backend === 'webgl2' ? '-gl' : ''}.png`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function baseUrl() {
  let port = opt('--port', process.env.PORT);
  if (!port) port = execFileSync('proj', ['port'], { cwd: ROOT, encoding: 'utf8' }).trim();
  if (!/^\d+$/.test(port)) throw new Error(`no port: --port, $PORT and proj port gave nothing ("${port}")`);
  const common = path.resolve(ROOT, execFileSync('git', ['rev-parse', '--git-common-dir'], { cwd: ROOT, encoding: 'utf8' }).trim());
  const rel = path.relative(path.dirname(common), ROOT);
  return `http://127.0.0.1:${port}/${rel ? rel.split(path.sep).join('/') + '/' : ''}`;
}

const flags = ['--use-angle=vulkan', '--enable-gpu', '--ignore-gpu-blocklist', '--mute-audio'];
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
  page.on('console', (m) => { if (m.type() === 'error' && !/Failed to load resource|KHR_parallel/.test(m.text())) errors.push(m.text()); if (process.env.MP_VERBOSE) console.log(`[page] ${m.text()}`); });
  await page.emulate({ userAgent: DEVICES[device].userAgent || await browser.userAgent(), viewport: DEVICES[device].viewport });
  const t0 = Date.now();
  if (side === 'js') {
    const ORIGIN = 'https://midnight-racer.test';
    const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.json': 'application/json', '.png': 'image/png', '.jpg': 'image/jpeg', '.svg': 'image/svg+xml', '.webmanifest': 'application/manifest+json', '.mp3': 'audio/mpeg', '.ttf': 'font/ttf' };
    const FONT_CSS = [500, 600, 700].map((w) => `@font-face { font-family: 'Rajdhani'; font-weight: ${w}; src: url(${ORIGIN}/assets/fonts/rajdhani/Rajdhani-${{ 500: 'Medium', 600: 'SemiBold', 700: 'Bold' }[w]}.ttf); }`).join('\n');
    await page.setRequestInterception(true);
    page.on('request', (req) => {
      const u = new URL(req.url());
      if (u.hostname === 'fonts.googleapis.com') return req.respond({ status: 200, contentType: 'text/css', body: FONT_CSS });
      if (u.origin !== ORIGIN) return req.abort('blockedbyclient');
      const file = path.resolve(ROOT, decodeURIComponent(u.pathname).replace(/^\/+/, '') || 'index.html');
      if (!fs.existsSync(file) || fs.statSync(file).isDirectory()) return req.respond({ status: 404, body: 'not found' });
      req.respond({ status: 200, contentType: TYPES[path.extname(file)] || 'application/octet-stream', body: fs.readFileSync(file) });
    });
    // Stop exactly at `time`.
    await page.evaluateOnNewDocument((stopAt, stageSrc, at, line) => {
      window.__tk = 0;
      // eslint-disable-next-line no-new-func
      const staged = stageSrc ? new Function('return (' + stageSrc + ')')() : null;
      let p;
      Object.defineProperty(window, '__parity', {
        configurable: true,
        get() { return p; },
        set(v) {
          p = v;
          v.onTick = (race) => {
            window.__tk++;
            if (staged && !window.__staged && race.time >= at) { window.__staged = true; staged(race, line); }
            const done = typeof stopAt === 'number' ? race.time >= stopAt : race.state === 'countdown' && race.countdown <= Number(stopAt.slice(2));
            if (done) { window.__stopped = true; v.fixed.ticks = 0; }
          };
        },
      });
    }, time, stage ? String(STAGES[stage].js) : '', stageAt, LINE);
    await page.goto(`${ORIGIN}/index.html?level=${level}&autostart=${car}&autodrive=1&parity=1&seed=1&ticks=16${/(^|&)pursuit=/.test(query) ? '' : '&pursuit=0'}${query ? `&${query}` : ''}`);
    for (;;) {
      const s = await ev(() => ({ tk: window.__tk, stopped: window.__stopped }));
      if (s.stopped) break;
      if (Date.now() - t0 > timeoutMs) throw new Error(`js: tick ${s.tk} after ${timeoutMs} ms`);
      await sleep(100);
    }
    if (cd == null) await sleep(800); // a few frames of the stopped race
    let perf = null;
    if (perfSecs) perf = await ev(PERF, perfSecs);
    await page.screenshot({ path: path.join(OUT, name) });
    const info = await ev(() => ({ tk: window.__tk, time: window.__race.time, s: window.__race.player.s }));
    console.log(JSON.stringify({ file: path.join(OUT, name), ...info, perf }));
  } else {
    await cdp.send('Page.setDownloadBehavior', { behavior: 'allow', downloadPath: OUT });
    const url = `${baseUrl()}dist/${dist}/index.html?level=${level}&autostart=${car}&autodrive=1&seed=1${/(^|&)pursuit=/.test(query) ? '' : '&pursuit=0'}${query ? `&${query}` : ''}`;
    console.error(url);
    await page.goto(url);
    let perf = null;
    if (stage) {
      // Staged from the page's own frames, the first one at race time `at`.
      await ev((at, cmds) => {
        const f = () => { const r = window.__mp?.race; if (r && r.time >= at) { for (const c of cmds) window.__mp.stage(c); } else requestAnimationFrame(f); };
        requestAnimationFrame(f);
      }, stageAt, STAGES[stage].rust(LINE));
    }
    for (;;) {
      const s = await ev(() => ({ st: window.__mp?.state, err: window.__mp?.error, tk: window.__mp?.race?.time }));
      if (s.err || s.st === 'failed') throw new Error(`rust: ${s.err}`);
      if (perfSecs && !perf && cd == null && s.tk >= time - perfSecs * 1.1) perf = await ev(PERF, perfSecs);
      if (cd != null ? s.tk != null : s.tk >= time - 1.5) break;
      if (Date.now() - t0 > timeoutMs) throw new Error(`rust: time ${s.tk} after ${timeoutMs} ms`);
      await sleep(20);
    }
    const file = path.join(OUT, name);
    fs.rmSync(file, { force: true });
    await ev((n, t) => new Promise((res) => {
      const due = (r) => (typeof t === 'number' ? r.time >= t : r.state === 'countdown' && r.countdown <= Number(t.slice(2)));
      const f = () => { const r = window.__mp.race; if (r && due(r)) { window.__shotAt = [r.tick, r.time, r.countdown]; window.__mp.screenshot(n); res(); } else requestAnimationFrame(f); };
      requestAnimationFrame(f);
    }), name, time);
    const a = await ev(() => window.__shotAt);
    for (let i = 0; i < 100 && !fs.existsSync(file); i++) await sleep(100);
    const info = await ev(() => ({ backend: window.__mp.backend, s: window.__mp.race.s }));
    console.log(JSON.stringify({ file: fs.existsSync(file) ? file : 'no screenshot', at: a, ...info, perf }));
  }
} catch (e) {
  errors.push(String(e.message || e));
} finally {
  clearTimeout(timer);
  await browser.close();
}
if (errors.length) { console.error(`errors:\n  ${errors.join('\n  ')}`); process.exit(1); }
