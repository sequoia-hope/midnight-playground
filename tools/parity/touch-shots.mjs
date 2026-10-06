// Side-by-side pictures of the touch controls (roadmap WP 6.5, 6.6): the
// JS game and the Rust web build racing on an emulated iPhone, held
// sideways and upright, in four states: at rest (thumb stick, pedal
// slider), both thumbs down (the stick pushed right, the slider on the
// gas), the Buttons choices with ▸ and GAS held, and tilt with the phone
// turned (Chrome's deviceorientation override). Into parity/report/touch/
// with an index page, to look at through the registered server
// (/parity/report/touch/).
//
//   cargo xtask web --release && node tools/parity/touch-shots.mjs [--side js|rust] [--only rest,thumbs,buttons,tilt]
//
// Both builds are answered from the working tree by request interception
// (ui-shots.mjs's way); the Rust canvas is captured with `__mp.screenshot`.

import fs from 'node:fs';
import path from 'node:path';
import { ROOT } from './lib/jstree.mjs';
import { launch, openGame, sleep, DEVICES } from './e2e/harness.mjs';
import { pose } from '../../test/unit/support/pose.js';

const args = process.argv.slice(2);
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
const side = opt('--side', 'both');
const only = opt('--only', 'rest,thumbs,buttons,tilt').split(',');
const OUT = path.join(ROOT, 'parity/report/touch');
const ORIGIN = 'https://midnight-racer.test';
const TYPES = {
  '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.json': 'application/json',
  '.png': 'image/png', '.jpg': 'image/jpeg', '.ttf': 'font/ttf', '.svg': 'image/svg+xml', '.webmanifest': 'application/manifest+json',
};
const FONT_CSS = [500, 600, 700].map((w) => `@font-face { font-family: 'Rajdhani'; font-weight: ${w}; src: url(${ORIGIN}/assets/fonts/rajdhani/Rajdhani-${{ 500: 'Medium', 600: 'SemiBold', 700: 'Bold' }[w]}.ttf); }`).join('\n');

// The owner's iPhone upright too (the harness has it sideways).
DEVICES.iphonePortrait = {
  userAgent: DEVICES.iphone.userAgent,
  viewport: { width: 390, height: 844, deviceScaleFactor: 3, isMobile: true, hasTouch: true, isLandscape: false },
};
const devices = ['iphone', 'iphonePortrait'];
const STATES = {
  rest: {},
  thumbs: {},
  buttons: { 'mr.steering': 'buttons', 'mr.pedals': 'buttons' },
  tilt: { 'mr.steering': 'tilt' },
};

async function openJs(browser, device, storage) {
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
  await page.emulate({ userAgent: DEVICES[device].userAgent, viewport: DEVICES[device].viewport });
  await page.evaluateOnNewDocument((e) => { for (const [k, v] of Object.entries(e)) localStorage.setItem(k, v); },
    Object.fromEntries(Object.entries(storage).map(([k, v]) => [k, JSON.stringify(v)])));
  const cdp = await page.createCDPSession();
  await page.goto(`${ORIGIN}/index.html?timescale=2`);
  const ev = (expr) => cdp.send('Runtime.evaluate', { expression: expr, returnByValue: true, awaitPromise: true }).then((r) => r.result.value);
  return { page, context, cdp, ev, errors, close: () => context.close() };
}

// What both sides do once racing: the thumbs or the tilt for the state.
async function act(state, g, { vw, vh, slider, pad }) {
  const touch = (type, pts) => g.cdp.send('Input.dispatchTouchEvent', { type, touchPoints: pts });
  if (state === 'thumbs') {
    const p = { x: Math.round(vw * 0.2), y: Math.round(vh * 0.7), id: 0 };
    const q = { x: slider.x + slider.w / 2, y: slider.y + slider.h * (1 - 0.72), id: 1 };
    await touch('touchStart', [p, q]);
    await sleep(100);
    await touch('touchMove', [{ ...p, x: p.x + 40 }, q]);
  } else if (state === 'buttons') {
    await touch('touchStart', [{ ...pad('right'), id: 0 }, { ...pad('throttle'), id: 1 }]);
  } else if (state === 'tilt') {
    await g.cdp.send('DeviceOrientation.setDeviceOrientationOverride', pose({ angle: vw > vh ? 90 : 0, turn: 0 }));
    await sleep(300);
    await g.cdp.send('DeviceOrientation.setDeviceOrientationOverride', pose({ angle: vw > vh ? 90 : 0, turn: 12 }));
  }
  await sleep(600);
}

async function jsShot(browser, device, state, dir) {
  const g = await openJs(browser, device, STATES[state]);
  for (let i = 0; i < 600 && !(await g.ev('window.__ready === true')); i++) await sleep(250);
  if (state === 'tilt') await g.cdp.send('DeviceOrientation.setDeviceOrientationOverride', pose({ angle: device === 'iphone' ? 90 : 0, turn: 0 }));
  await g.ev("document.getElementById('btn-start').click()");
  for (let i = 0; i < 300 && (await g.ev('window.__race?.state')) !== 'racing'; i++) await sleep(100);
  const box = (sel) => g.ev(`(() => { const b = document.querySelector(${JSON.stringify(sel)}).getBoundingClientRect(); return { x: b.left, y: b.top, w: b.width, h: b.height }; })()`);
  const vw = await g.ev('innerWidth'), vh = await g.ev('innerHeight');
  const slider = await box('#touch .t-slider');
  const pads = {};
  for (const n of ['right', 'throttle']) {
    const b = await box(`#touch [data-act="${n}"]`);
    pads[n] = { x: b.x + b.w / 2, y: b.y + b.h / 2 };
  }
  await act(state, g, { vw, vh, slider, pad: (n) => pads[n] });
  await g.page.screenshot({ path: path.join(dir, `${state}-js.png`) });
  if (g.errors.length) console.log(`js ${device} ${state}: ${g.errors.join(' | ')}`);
  await g.close();
}

async function rustShot(browser, device, state, dir) {
  const g = await openGame(browser, { device, query: 'timescale=2', storage: STATES[state], downloads: dir });
  if (state === 'tilt') await g.cdp.send('DeviceOrientation.setDeviceOrientationOverride', pose({ angle: device === 'iphone' ? 90 : 0, turn: 0 }));
  await g.tap('#btn-start');
  await g.waitFor(() => window.__mp.race?.state === 'racing' && window.__mp.screen === 'none', { timeout: 90000, what: 'racing' });
  const vw = await g.eval('innerWidth'), vh = await g.eval('innerHeight');
  const slider = await g.eval(() => window.__mp.ui('touch-slider'));
  const pad = (n) => g.eval((i) => { const u = window.__mp.ui('touch-' + i); return { x: u.x + u.w / 2, y: u.y + u.h / 2 }; }, n);
  const pads = { right: await pad('right'), throttle: await pad('throttle') };
  await act(state, g, { vw, vh, slider, pad: (n) => pads[n] });
  await g.shot(`${state}-rust.png`, dir);
  if (g.errors.length) console.log(`rust ${device} ${state}: ${g.errors.join(' | ')}`);
  await g.close();
}

const browser = await launch();
try {
  for (const device of devices) {
    const dir = path.join(OUT, device);
    fs.mkdirSync(dir, { recursive: true });
    for (const state of only) {
      console.log(`${device} ${state}`);
      if (side !== 'rust') await jsShot(browser, device, state, dir);
      if (side !== 'js') await rustShot(browser, device, state, dir);
    }
  }
} finally {
  await browser.close();
}

const rows = [];
for (const device of devices) {
  const vp = DEVICES[device].viewport;
  for (const s of Object.keys(STATES)) {
    const has = (k) => fs.existsSync(path.join(OUT, device, `${s}-${k}.png`));
    if (!has('js') && !has('rust')) continue;
    rows.push(`<h2>${device} (${vp.width}×${vp.height} @${vp.deviceScaleFactor}×): ${s}</h2><div class="pair">`
      + ['js', 'rust'].map((k) => `<figure><img src="${device}/${s}-${k}.png" width="${vp.width}"><figcaption>${k === 'js' ? 'JS game' : 'Rust build'}</figcaption></figure>`).join('')
      + '</div>');
  }
}
fs.writeFileSync(path.join(OUT, 'index.html'), `<!doctype html><meta charset="utf-8"><title>Touch controls: JS and Rust</title>
<style>body{background:#111;color:#ddd;font:14px system-ui;margin:16px} .pair{display:flex;gap:12px;flex-wrap:wrap} figure{margin:0} img{display:block;height:auto;max-width:48vw;border:1px solid #333} h2{font-size:15px;margin:20px 0 6px}</style>
<h1>Touch controls, JS game against the Rust build</h1>${rows.join('\n')}`);
console.log(`wrote ${path.relative(ROOT, OUT)}/index.html`);
