// The e2e harness for the Rust web build (roadmap WP 6.2; SPEC 8.5): the
// JS suites' `test/e2e/harness.js` API, driving `dist/next/` through
// `window.__mr` instead of the DOM. Headless Chrome on the GPU with WebGPU
// (rust-web.mjs's flags), the working tree answered by request
// interception (no server, no port), a fresh profile per game, the same
// devices as the JS harness.
//
// Controls are found by the id their DOM element had (`btn-start`,
// `opt-hq`, `lvl-tab-sierra`, …): `__mr.ui(id)` gives the box in CSS px,
// `__mr.reveal(id)` scrolls it to the middle (`scrollIntoView`), and a tap
// or click lands on its centre after checking nothing else is on top.
//
// Scenes: the full exports of Sierra (138 MB), Coast and the city levels
// are too big for request interception (DECISIONS D106), so by default a
// level's scene request is answered with its terrain-road-sky export
// (`<level>.base.mrscene`) when the full one is over 90 MB (Seaside's
// full export, 39 MB, passes); `{ scenes: 'base' }` always serves the
// base ones, `'full'` never.
// The JS suites themselves run against the Rust build through the JS
// harness's `target: 'rust'` (WP 6.7, `npm run test:e2e:rust`); this one
// stays for the Rust-only suites here and the picture tools.

import puppeteer from 'puppeteer-core';
import fs from 'node:fs/promises';
import path from 'node:path';
import { ROOT } from '../lib/jstree.mjs';

const ORIGIN = 'https://midnight-racer.test';
const TYPES = {
  '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css',
  '.json': 'application/json', '.wasm': 'application/wasm', '.ttf': 'font/ttf',
  '.png': 'image/png', '.jpg': 'image/jpeg', '.svg': 'image/svg+xml',
};

export const DEVICES = {
  desktop: { viewport: { width: 1280, height: 800, deviceScaleFactor: 1 } },
  phone: {
    userAgent: 'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Mobile Safari/537.36',
    viewport: { width: 915, height: 412, deviceScaleFactor: 2, isMobile: true, hasTouch: true, isLandscape: true },
  },
  phonePortrait: {
    userAgent: 'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Mobile Safari/537.36',
    viewport: { width: 412, height: 915, deviceScaleFactor: 2, isMobile: true, hasTouch: true, isLandscape: false },
  },
  // The owner's iPhone, held sideways (the screenshot comparisons).
  iphone: {
    userAgent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1',
    viewport: { width: 844, height: 390, deviceScaleFactor: 3, isMobile: true, hasTouch: true, isLandscape: true },
  },
};

export function launch() {
  return puppeteer.launch({
    executablePath: process.env.CHROME_PATH || '/usr/bin/google-chrome',
    headless: process.env.MR_HEADFUL ? false : 'new',
    args: [
      '--enable-unsafe-webgpu', '--enable-features=Vulkan', '--use-angle=vulkan', '--ignore-gpu-blocklist', '--mute-audio',
      '--autoplay-policy=document-user-activation-required',
    ],
    protocolTimeout: 600000,
  });
}

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const IGNORE = [/Failed to load resource/, /KHR_parallel_shader_compile/];

// The JS selectors the suites use, as the Rust build's ids.
export function idOf(sel) {
  const m = sel.match(/^#level-pick \.lvl-tab:nth-child\((\d+)\)$/);
  if (m) return `lvl-tab-${LEVEL_IDS[Number(m[1]) - 1]}`;
  const c = sel.match(/^#car-pick \.pick:nth-child\((\d+)\)$/);
  if (c) return `pick-${CAR_IDS[Number(c[1]) - 1]}`;
  const t = sel.match(/^#touch \[data-tap="(\w+)"\]$/);
  if (t) return `touch-${t[1]}`;
  const v = sel.match(/^#(?:menu|pause) \.(vol-music|vol-sfx)$/);
  if (v) return v[1];
  return sel.replace(/^#/, '');
}
export const LEVEL_IDS = ['sierra', 'coast', 'streets', 'desert', 'seaside', 'cruise'];
export const CAR_IDS = ['sports', 'muscle', 'super', 'rally', 'electric'];

export class Game {
  constructor(page, cdp, context) {
    this.page = page;
    this.cdp = cdp;
    this.context = context;
    this.errors = [];
    this.warnings = [];
  }

  async eval(fn, ...args) {
    const expression = typeof fn === 'function' ? `(${fn})(...${JSON.stringify(args)})` : fn;
    const res = await this.cdp.send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true, userGesture: false });
    if (res.exceptionDetails) {
      const d = res.exceptionDetails;
      throw new Error(`page eval failed: ${d.exception?.description || d.text}`);
    }
    return res.result.value;
  }

  async waitFor(fn, { timeout = 15000, interval = 100, what = String(fn) } = {}) {
    const t0 = Date.now();
    for (;;) {
      const v = await this.eval(fn);
      if (v) return v;
      if (Date.now() - t0 > timeout) {
        throw new Error(`timed out after ${timeout} ms waiting for ${what}\n  game: ${JSON.stringify(await this.snapshot())}`);
      }
      await sleep(interval);
    }
  }

  snapshot() {
    return this.eval(() => ({
      screen: window.__mr?.screen,
      mode: window.__mr?.mode,
      race: window.__mr?.race ? window.__mr.race.state : null,
      audio: window.__mr?.audio?.context ?? 'none',
      state: window.__mr?.state,
      fullscreen: !!document.fullscreenElement,
      touchUI: !!window.__mr?.touchUi,
      level: window.__mr?.level,
    }));
  }

  screen() { return this.snapshot().then((s) => s.screen); }

  ui(sel) { return this.eval((id) => window.__mr.ui(id), idOf(sel)); }

  // A control's centre in CSS px, scrolled to the middle first; fails if it
  // is not shown, off screen, or another control is on top of that point.
  async center(sel) {
    const id = idOf(sel);
    await this.eval((i) => window.__mr.reveal(i), id);
    await this.frames(3);
    const r = await this.eval((i) => {
      const u = window.__mr.ui(i);
      if (!u) return { error: 'no control ' + i };
      if (!u.visible || !u.w || !u.h) return { error: i + ' is not visible' };
      const x = u.x + u.w / 2, y = u.y + u.h / 2;
      // The topmost control there, as the client's own hit test picks it.
      let top = null;
      for (const [k, o] of Object.entries(window.__mr.uiNodes || {})) {
        if (!o.visible || !o.enabled || x < o.x || x > o.x + o.w || y < o.y || y > o.y + o.h) continue;
        if (!top || o.z > top.z || (o.z === top.z && o.w * o.h < top.w * top.h)) top = { k, ...o };
      }
      if (u.enabled && top && top.k !== i) return { error: `${i} is covered by ${top.k}` };
      return { x, y };
    }, id);
    if (r.error) throw new Error(r.error);
    return r;
  }

  frames(n = 2) {
    return this.eval((k) => new Promise((res) => {
      let i = 0;
      const f = () => (++i >= k ? res(true) : requestAnimationFrame(f));
      requestAnimationFrame(f);
    }), n);
  }

  async tap(sel) {
    const { x, y } = await this.center(sel);
    await this.page.touchscreen.tap(x, y);
    await this.frames(2);
  }

  async click(sel) {
    const { x, y } = await this.center(sel);
    await this.page.mouse.click(x, y);
    await this.frames(2);
  }

  async key(code, holdMs = 0) {
    await this.page.keyboard.down(code);
    if (holdMs) await sleep(holdMs);
    else await this.frames(2);
    await this.page.keyboard.up(code);
    await this.frames(2);
  }

  async reload() {
    await this.page.reload();
    await this.waitReady();
  }

  async waitReady(timeout = 240000) {
    await this.waitFor(() => {
      const m = window.__mr;
      if (m?.state === 'failed') throw new Error('client failed: ' + m.error);
      return m?.ready === true && m.screen !== 'loading' && !!m.uiNodes;
    }, { timeout, interval: 250, what: 'the client to load' });
    // The screen's first layouts (the pixel ratio changes with it, D575).
    await this.frames(6);
  }

  async shot(name, dir) {
    const file = path.join(dir, name);
    await fs.rm(file, { force: true });
    await this.eval((n) => window.__mr.screenshot(n), name);
    for (let i = 0; i < 100; i++) {
      try { await fs.access(file); return file; } catch { await sleep(100); }
    }
    throw new Error('no screenshot ' + name);
  }

  async close() { await this.context.close(); }
}

// Open the Rust build in a fresh profile. storage: localStorage entries
// seeded before boot ({ 'mr.level': 'seaside' }, JSON-encoded here);
// scenes: 'auto' (default), 'base' or 'full'; downloads: a directory for
// __mr.screenshot().
export async function openGame(browser, { device = 'desktop', query = '', storage = {}, scenes = 'auto', downloads = null, wait = true } = {}) {
  const context = await browser.createBrowserContext();
  const page = await context.newPage();
  const cdp = await page.createCDPSession();
  const game = new Game(page, cdp, context);
  if (downloads) await cdp.send('Page.setDownloadBehavior', { behavior: 'allow', downloadPath: downloads });
  page.on('pageerror', (e) => game.errors.push(String(e.message || e)));
  page.on('console', (m) => {
    const text = m.text();
    if (IGNORE.some((re) => re.test(text))) return;
    if (m.type() === 'error') game.errors.push(text);
    else if (m.type() === 'warn' || m.type() === 'warning') game.warnings.push(text);
    if (process.env.MR_VERBOSE) console.log(`[page ${m.type()}] ${text}`);
  });
  await page.setRequestInterception(true);
  page.on('request', async (req) => {
    const u = new URL(req.url());
    if (u.origin !== ORIGIN) return req.abort('blockedbyclient');
    let rel = decodeURIComponent(u.pathname).replace(/^\/+/, '') || 'index.html';
    if (rel.endsWith('/')) rel += 'index.html';
    // A full export over 90 MB is answered with its terrain-road-sky one.
    if (/\/[a-z]+\.mrscene$/.test(rel) && scenes !== 'full') {
      const big = await fs.stat(path.resolve(ROOT, rel)).then((st) => st.size > 90e6).catch(() => false);
      if (scenes === 'base' || big) rel = rel.replace(/\/([a-z]+)\.mrscene$/, '/$1.base.mrscene');
    }
    const file = path.resolve(ROOT, rel);
    try {
      const body = await fs.readFile(file);
      await req.respond({ status: 200, contentType: TYPES[path.extname(file)] || 'application/octet-stream', body });
    } catch {
      await req.respond({ status: 404, contentType: 'text/plain', body: 'not found: ' + rel });
    }
  });
  page.on('response', (r) => { if (r.status() >= 400) game.errors.push(`HTTP ${r.status()} ${r.url()}`); });
  await page.emulate({ userAgent: DEVICES[device].userAgent || await browser.userAgent(), viewport: DEVICES[device].viewport });
  if (Object.keys(storage).length) {
    await page.evaluateOnNewDocument((entries) => {
      for (const [k, v] of Object.entries(entries)) if (localStorage.getItem(k) === null) localStorage.setItem(k, v);
    }, Object.fromEntries(Object.entries(storage).map(([k, v]) => [k, JSON.stringify(v)])));
  }
  const q = query.replace(/^\?/, '');
  await page.goto(`${ORIGIN}/dist/next/index.html${q ? '?' + q : ''}`);
  if (wait) await game.waitReady();
  return game;
}

// flow-helpers.js, for the Rust build.
export const stored = (game, key) => game.eval((k) => {
  const v = localStorage.getItem('mr.' + k);
  return v === null ? null : JSON.parse(v);
}, key);

export const raceStarted = () => !!window.__mr?.race && ['countdown', 'racing'].includes(window.__mr.race.state)
  && window.__mr.mode === 'race' && window.__mr.screen === 'none';

export async function startFromMenu(game, { touch = false, timeout = 30000 } = {}) {
  await (touch ? game.tap('#btn-start') : game.click('#btn-start'));
  await game.waitFor(raceStarted, { timeout, what: 'the race to start' });
}

export const markRace = (game) => game.eval(() => { window.__marked = window.__mr.races; });
export const newRaceStarted = () => window.__mr.races > window.__marked && !!window.__mr.race
  && ['countdown', 'racing'].includes(window.__mr.race.state) && window.__mr.mode === 'race';

export async function waitRacing(game, timeout = 20000) {
  await game.waitFor(() => window.__mr.race?.state === 'racing', { timeout, what: 'the countdown to end' });
}

export async function expectScreen(game, name, timeout = 8000) {
  await game.waitFor(`window.__mr.screen === ${JSON.stringify(name)}`, { timeout, what: `the "${name}" screen` });
}

export const isShown = (game, sel) => game.ui(sel).then((u) => !!u && u.visible && u.w > 0 && u.h > 0);
