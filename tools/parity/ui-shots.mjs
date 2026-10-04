// Side-by-side pictures of the screens (roadmap WP 6.2): the JS game and
// the Rust web build, each screen (loading, menu, pause, results,
// controller setup) at desktop (1280×800) and phone landscape (844×390 at
// 3×), into parity/report/ui/ with an index page to look at them through
// the registered server (/parity/report/ui/).
//
//   cargo xtask web --release && node tools/parity/ui-shots.mjs [--only menu,pause] [--device desktop|iphone]
//       [--side js|rust]
//
// Both builds are answered from the working tree by request interception
// (no server). The JS page's Google Fonts request is answered with the
// bundled Rajdhani files, so both draw the same font files. Both open on
// Sierra; the Rust build gets Sierra's terrain-road-sky export (the full
// export is too big for interception, D106), so the scenery behind the
// screens differs where it shows through. The Rust canvas is captured with
// `__mr.screenshot` (headless Chrome does not composite a WebGPU canvas into
// page screenshots); its loading screen is the page's own HTML, captured
// as a page screenshot.

import fs from 'node:fs';
import path from 'node:path';
import { ROOT } from './lib/jstree.mjs';
import { launch, openGame, sleep, DEVICES } from './e2e/harness.mjs';
import puppeteer from 'puppeteer-core';

const args = process.argv.slice(2);
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
const only = opt('--only', 'loading,menu,pause,results,padsetup').split(',');
const devices = opt('--device', 'desktop,iphone').split(',');
const side = opt('--side', 'both');
const OUT = path.join(ROOT, 'parity/report/ui');
const ORIGIN = 'https://midnight-racer.test';
const TYPES = {
  '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.json': 'application/json',
  '.png': 'image/png', '.jpg': 'image/jpeg', '.ttf': 'font/ttf', '.svg': 'image/svg+xml', '.webmanifest': 'application/manifest+json',
};
const FONT_CSS = [500, 600, 700].map((w) => `@font-face { font-family: 'Rajdhani'; font-weight: ${w}; src: url(${ORIGIN}/assets/fonts/rajdhani/Rajdhani-${{ 500: 'Medium', 600: 'SemiBold', 700: 'Bold' }[w]}.ttf); }`).join('\n');

// The JS game in headless Chrome (WebGL), as the JS harness opens it.
async function openJs(browser, device, { storage = {}, query = '' } = {}) {
  const context = await browser.createBrowserContext();
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.setRequestInterception(true);
  page.on('request', (req) => {
    const u = new URL(req.url());
    if (u.hostname === 'fonts.googleapis.com') return req.respond({ status: 200, contentType: 'text/css', body: FONT_CSS });
    if (u.origin !== ORIGIN) return req.abort('blockedbyclient');
    const rel = decodeURIComponent(u.pathname).replace(/^\/+/, '') || 'index.html';
    const file = path.resolve(ROOT, rel);
    if (!fs.existsSync(file) || fs.statSync(file).isDirectory()) return req.respond({ status: 404, body: 'not found' });
    req.respond({ status: 200, contentType: TYPES[path.extname(file)] || 'application/octet-stream', body: fs.readFileSync(file) });
  });
  await page.emulate({ userAgent: DEVICES[device].userAgent || await browser.userAgent(), viewport: DEVICES[device].viewport });
  if (Object.keys(storage).length) {
    await page.evaluateOnNewDocument((e) => { for (const [k, v] of Object.entries(e)) localStorage.setItem(k, v); },
      Object.fromEntries(Object.entries(storage).map(([k, v]) => [k, JSON.stringify(v)])));
  }
  await page.goto(`${ORIGIN}/index.html${query ? '?' + query : ''}`);
  const cdp = await page.createCDPSession();
  const ev = (expr) => cdp.send('Runtime.evaluate', { expression: expr, returnByValue: true, awaitPromise: true, userGesture: false }).then((r) => r.result.value);
  return { page, context, ev, errors };
}

async function jsShots(browser, device, dir) {
  const shot = (page, name) => page.screenshot({ path: path.join(dir, `${name}-js.png`) });
  if (only.includes('loading')) {
    const g = await openJs(browser, device);
    for (let i = 0; i < 100; i++) {
      const p = await g.ev("parseFloat(document.getElementById('load-fill')?.style.width || '0')");
      if (p > 20) break;
      await sleep(50);
    }
    await shot(g.page, 'loading');
    await g.context.close();
  }
  const g = await openJs(browser, device, { query: 'timescale=2&autodrive=1' });
  for (let i = 0; i < 600 && !(await g.ev('window.__ready === true')); i++) await sleep(250);
  await sleep(1500);
  if (only.includes('menu')) await shot(g.page, 'menu');
  if (only.includes('padsetup')) {
    await g.ev("document.getElementById('btn-pad').click()");
    await sleep(500);
    await shot(g.page, 'padsetup');
    await g.ev("document.getElementById('pad-done').click()");
    await sleep(300);
  }
  if (only.includes('pause') || only.includes('results')) {
    await g.ev("document.getElementById('btn-start').click()");
    for (let i = 0; i < 200 && (await g.ev("window.__race?.state")) !== 'racing'; i++) await sleep(100);
    if (only.includes('pause')) {
      await g.page.keyboard.press('Escape');
      await sleep(800);
      await shot(g.page, 'pause');
      await g.page.keyboard.press('Escape');
      await sleep(300);
    }
    if (only.includes('results')) {
      await g.ev(`(() => { const r = window.__race, t = r.track; const s = t.finishS - 250; r.phys.reset(s, 0);
        const F = t.frame(s); r.player.vx = F.fx * 45; r.player.vz = F.fz * 45; r.lastS = s; r.odo = s;
        r.ais.forEach((a, i) => { a.s = t.startS + 40 - i * 12; a.lat = i % 2 ? 2 : -2; a.speed = 0; a.writePos(); }); })()`);
      for (let i = 0; i < 400 && (await g.ev("document.getElementById('results').classList.contains('hidden')")); i++) await sleep(100);
      await sleep(800);
      await shot(g.page, 'results');
    }
  }
  if (g.errors.length) console.log(`js ${device}: page errors: ${g.errors.join(' | ')}`);
  await g.context.close();
}

async function rustShots(browser, device, dir) {
  const name = (s) => `${s}-rust.png`;
  if (only.includes('loading')) {
    const g = await openGame(browser, { device, wait: false });
    for (let i = 0; i < 400; i++) {
      const p = await g.eval("parseFloat(document.getElementById('bar')?.style.width || '0')");
      if (p > 40) break;
      await sleep(50);
    }
    await g.page.screenshot({ path: path.join(dir, name('loading')) });
    await g.close();
  }
  const g = await openGame(browser, { device, query: 'timescale=2&autodrive=1', downloads: dir });
  const press = (sel) => (device === 'desktop' ? g.click(sel) : g.tap(sel));
  await sleep(1500);
  if (only.includes('menu')) await g.shot(name('menu'), dir);
  if (only.includes('padsetup')) {
    await g.eval(() => window.__mr.stage({ cmd: 'padsetup' }));
    await g.frames(4);
    await g.shot(name('padsetup'), dir);
    await press('#pad-done');
  }
  if (only.includes('pause') || only.includes('results')) {
    await press('#btn-start');
    await g.waitFor(() => window.__mr.race?.state === 'racing', { timeout: 60000, what: 'racing' });
    if (only.includes('pause')) {
      await g.key('Escape');
      await sleep(800);
      await g.shot(name('pause'), dir);
      await g.key('Escape');
      await sleep(300);
    }
    if (only.includes('results')) {
      await g.eval(() => window.__mr.stage({ cmd: 'finish' }));
      await g.waitFor(() => window.__mr.screen === 'results', { timeout: 60000, what: 'results' });
      await sleep(800);
      await g.shot(name('results'), dir);
    }
  }
  if (g.errors.length) console.log(`rust ${device}: page errors: ${g.errors.join(' | ')}`);
  await g.close();
}

const browser = await launch();
try {
  for (const device of devices) {
    const dir = path.join(OUT, device);
    fs.mkdirSync(dir, { recursive: true });
    if (side !== 'rust') {
      console.log(`${device}: JS…`);
      await jsShots(browser, device, dir);
    }
    if (side !== 'js') {
      console.log(`${device}: Rust…`);
      await rustShots(browser, device, dir);
    }
  }
} finally {
  await browser.close();
}

// The index page: each screen, JS left, Rust right, at CSS size.
const rows = [];
for (const device of devices) {
  const vp = DEVICES[device].viewport;
  for (const s of ['loading', 'menu', 'pause', 'results', 'padsetup']) {
    const has = (k) => fs.existsSync(path.join(OUT, device, `${s}-${k}.png`));
    if (!has('js') && !has('rust')) continue;
    rows.push(`<h2>${device} (${vp.width}×${vp.height} @${vp.deviceScaleFactor}×): ${s}</h2><div class="pair">`
      + ['js', 'rust'].map((k) => `<figure><img src="${device}/${s}-${k}.png" width="${vp.width}"><figcaption>${k === 'js' ? 'JS game' : 'Rust build'}</figcaption></figure>`).join('')
      + '</div>');
  }
}
fs.writeFileSync(path.join(OUT, 'index.html'), `<!doctype html><meta charset="utf-8"><title>Screens: JS and Rust</title>
<style>body{background:#111;color:#ddd;font:14px system-ui;margin:16px} .pair{display:flex;gap:12px;flex-wrap:wrap} figure{margin:0} img{display:block;height:auto;max-width:48vw;border:1px solid #333} h2{font-size:15px;margin:20px 0 6px}</style>
<h1>Screens, JS game against the Rust build</h1>${rows.join('\n')}`);
console.log(`wrote ${path.relative(ROOT, OUT)}/index.html`);
void puppeteer;
