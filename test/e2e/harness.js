// Browser harness for the end-to-end tests: real Chrome (puppeteer-core),
// real GPU, the game loaded exactly as a player gets it.
//
// No web server and no port: every request to ORIGIN is answered from the
// working tree by request interception. Set MR_BASE_URL to test a running
// copy instead (`proj url midnight-racer`, or the GitHub Pages build).
//
// Everything the tests read from the page goes through Runtime.evaluate with
// userGesture: false. puppeteer's own page.evaluate() runs as a user gesture,
// which hands the page user activation before the test has touched anything,
// and that hides exactly the kind of bug these tests exist for (audio or
// fullscreen that only unlocks inside a real tap).
//
// target: 'rust' (MR_TARGET=rust, `npm run test:e2e:rust`) runs the same
// suites against the Rust build in dist/next/ (SPEC 8.5): Chrome gets
// WebGPU, a selector is looked up through `__mr.ui(id)` and tapped at its
// centre, and the page's `__race`, `__game`, `__audio`, ... and the DOM the
// suites read are stand-ins over `window.__mr` (rust-bridge.js). The
// default, 'js', is the JS game, exactly as before.

import puppeteer from 'puppeteer-core';
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { selectorId, installBridge, RUST_ARGS, RUST_TYPES, RUST_PAGE } from './rust-bridge.js';

export const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const ORIGIN = 'https://midnight-racer.test';
const BASE = process.env.MR_BASE_URL ? process.env.MR_BASE_URL.replace(/\/?$/, '/') : ORIGIN + '/';
export const TARGET = process.env.MR_TARGET || 'js';

const TYPES = {
  '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css',
  '.json': 'application/json', '.webmanifest': 'application/manifest+json',
  '.png': 'image/png', '.jpg': 'image/jpeg', '.svg': 'image/svg+xml', '.mp3': 'audio/mpeg',
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
};

// One Chrome per test file. The autoplay policy is spelled out so audio
// behaves as it does on a phone: nothing plays until a real gesture.
export function launch({ target = TARGET } = {}) {
  if (target === 'rust') {
    return puppeteer.launch({
      executablePath: process.env.CHROME_PATH || '/usr/bin/google-chrome',
      headless: process.env.MR_HEADFUL ? false : 'new',
      args: RUST_ARGS,
      protocolTimeout: 600000,
    });
  }
  return puppeteer.launch({
    executablePath: process.env.CHROME_PATH || '/usr/bin/google-chrome',
    headless: process.env.MR_HEADFUL ? false : 'new',
    args: [
      '--use-angle=vulkan', '--enable-gpu', '--ignore-gpu-blocklist', '--mute-audio',
      '--autoplay-policy=document-user-activation-required',
    ],
  });
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Console noise that isn't the game's fault: the web font is blocked on purpose.
const IGNORE = [/Failed to load resource/, /KHR_parallel_shader_compile/];

export class Game {
  constructor(page, cdp, context, target = 'js') {
    this.page = page;
    this.cdp = cdp;
    this.context = context;
    this.target = target;
    this.errors = [];
    this.warnings = [];
  }

  // Evaluate in the page without a user gesture. Takes a function (with
  // JSON-able args) or an expression string; returns the JSON-able result.
  async eval(fn, ...args) {
    if (this.target === 'rust') return this.evalRust(fn, ...args);
    const expression = typeof fn === 'function' ? `(${fn})(...${JSON.stringify(args)})` : fn;
    const res = await this.cdp.send('Runtime.evaluate', {
      expression, returnByValue: true, awaitPromise: true, userGesture: false,
    });
    if (res.exceptionDetails) {
      const d = res.exceptionDetails;
      throw new Error(`page eval failed: ${d.exception?.description || d.text}`);
    }
    return res.result.value;
  }

  // Poll until fn() is truthy; on timeout, fail with a snapshot of the game.
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

  // The Rust build: the same evaluation inside the bridge (rust-bridge.js),
  // then, if it staged anything, the frame that applied it.
  async evalRust(fn, ...args) {
    const inner = typeof fn === 'function' ? `(${fn})(...${JSON.stringify(args)})` : `(${fn})`;
    const res = await this.cdp.send('Runtime.evaluate', {
      // A JS page reached from the Rust one (music.html) has no bridge.
      expression: `(window.__mrShim ? window.__mrShim.run(() => ${inner}) : Promise.resolve(${inner}).then((v) => ({ v })))`,
      returnByValue: true, awaitPromise: true, userGesture: false,
    });
    if (res.exceptionDetails) {
      const d = res.exceptionDetails;
      throw new Error(`page eval failed: ${d.exception?.description || d.text}`);
    }
    const { v, wait } = res.result.value || {};
    if (wait) await this.staged(wait);
    return v;
  }

  // Wait for the client to have applied n stage commands.
  async staged(n, timeout = 10000) {
    const t0 = Date.now();
    for (;;) {
      const r = await this.cdp.send('Runtime.evaluate', { expression: `(window.__mr?.staged ?? 0) >= ${n}`, returnByValue: true });
      if (r.result.value) return;
      if (Date.now() - t0 > timeout) throw new Error(`the client did not apply staged command ${n}`);
      await sleep(10);
    }
  }

  // Wait for n frames of the page.
  frames(n = 2) {
    return this.cdp.send('Runtime.evaluate', {
      expression: `new Promise((res) => { let i = 0; const f = () => (++i >= ${n} ? res(true) : requestAnimationFrame(f)); requestAnimationFrame(f); })`,
      awaitPromise: true, returnByValue: true,
    }).catch(() => {}); // a navigation (the menu's Music player link) ends the wait
  }

  // What a player would see: which screen is up, and the race and audio state.
  snapshot() {
    if (this.target === 'rust') {
      return this.eval(() => {
        const m = window.__mr || {};
        return {
          screen: m.screen === undefined ? 'none' : m.screen,
          mode: m.mode,
          race: m.race ? m.race.state : null,
          audio: m.audio?.context ?? 'none',
          fullscreen: !!document.fullscreenElement,
          touchUI: !!m.touchUi,
        };
      });
    }
    return this.eval(() => {
      const shown = ['loading', 'menu', 'pause', 'results', 'padsetup'].filter((id) => !document.getElementById(id).classList.contains('hidden'));
      return {
        screen: shown.join(',') || 'none',
        mode: window.__game?.mode,
        race: window.__race ? window.__race.state : null,
        audio: window.__audio?.ctx?.state ?? 'none',
        fullscreen: !!document.fullscreenElement,
        touchUI: document.body.classList.contains('touch'),
      };
    });
  }

  screen() { return this.snapshot().then((s) => s.screen); }

  // Centre of an element in CSS px, scrolled into view the way a player
  // would scroll to it. Fails if something else sits on top of that point.
  async center(selector) {
    if (this.target === 'rust') return this.centerRust(selector);
    const r = await this.eval((sel) => {
      const el = document.querySelector(sel);
      if (!el) return { error: 'no element ' + sel };
      el.scrollIntoView({ block: 'center', inline: 'center' });
      const b = el.getBoundingClientRect();
      if (!b.width || !b.height) return { error: sel + ' is not visible' };
      const x = b.left + b.width / 2, y = b.top + b.height / 2;
      const top = document.elementFromPoint(x, y);
      if (!top || !(top === el || el.contains(top))) {
        return { error: `${sel} is covered by ${top ? top.tagName.toLowerCase() + (top.id ? '#' + top.id : '') + (top.className ? '.' + String(top.className).replace(/ /g, '.') : '') : 'nothing'}` };
      }
      return { x, y };
    }, selector);
    if (r.error) throw new Error(r.error);
    return r;
  }

  // The Rust build: the control with that id (`__mr.ui`), scrolled to the
  // middle of its screen (`__mr.reveal`); fails if it is not shown or
  // another control is on top of its centre (the client's own hit test).
  async centerRust(selector) {
    const id = selectorId(selector);
    if (!id) throw new Error(`no Rust control for ${selector}`);
    await this.cdp.send('Runtime.evaluate', { expression: `window.__mr.reveal(${JSON.stringify(id)})` });
    await this.frames(3);
    const res = await this.cdp.send('Runtime.evaluate', {
      expression: `(${(i) => {
        const u = window.__mr.ui(i);
        if (!u) return { error: 'no element ' + i };
        if (!u.visible || !u.w || !u.h) return { error: i + ' is not visible' };
        const x = u.x + u.w / 2, y = u.y + u.h / 2;
        let top = null;
        for (const [k, o] of Object.entries(window.__mr.uiNodes || {})) {
          if (!o.visible || !o.enabled || x < o.x || x > o.x + o.w || y < o.y || y > o.y + o.h) continue;
          if (!top || o.z > top.z || (o.z === top.z && o.w * o.h < top.w * top.h)) top = { k, ...o };
        }
        if (u.enabled && top && top.k !== i) return { error: `${i} is covered by ${top.k}` };
        return { x, y };
      }})(${JSON.stringify(id)})`,
      returnByValue: true,
    });
    const r = res.result.value;
    if (r.error) throw new Error(r.error);
    return r;
  }

  // A finger tap (touchstart → touchend), as on a phone.
  async tap(selector) {
    const { x, y } = await this.center(selector);
    await this.page.touchscreen.tap(x, y);
  }

  // A mouse click, as on a desktop.
  async click(selector) {
    const { x, y } = await this.center(selector);
    await this.page.mouse.click(x, y);
  }

  async key(code, holdMs = 0) {
    await this.page.keyboard.down(code);
    if (holdMs) await sleep(holdMs);
    await this.page.keyboard.up(code);
  }

  // Raw multi-touch: points is [{x, y, id}]; type is touchStart/Move/End.
  touch(type, points) {
    const sent = this.cdp.send('Input.dispatchTouchEvent', {
      type, touchPoints: points.map((p) => ({ x: p.x, y: p.y, id: p.id ?? 0, radiusX: 8, radiusY: 8, force: 1 })),
    });
    // The JS game reads a touch in its event handler; the Rust client in
    // its next frame.
    return this.target === 'rust' ? sent.then(() => this.frames(2)) : sent;
  }

  async reload() {
    await this.page.reload();
    await this.waitReady();
  }

  async waitReady(timeout = 60000) {
    if (this.target === 'rust') {
      // The wasm, the level's build and the shaders' warm-up take longer.
      await this.waitFor(() => {
        const m = window.__mr;
        if (m?.state === 'failed') throw new Error('client failed: ' + m.error);
        return window.__ready === true;
      }, { timeout: Math.max(timeout, 240000), interval: 250, what: 'the game to load' });
      await this.frames(6); // the screen's first layouts (D575)
      return;
    }
    await this.waitFor('window.__ready === true', { timeout, what: 'the game to load' });
  }

  async close() { await this.context.close(); }
}

// Open the game in a fresh profile (so no settings leak between tests).
// storage: localStorage entries to seed before the game first boots, e.g.
// { 'mr.level': 'coast' } (values are JSON-encoded here). A reload keeps
// whatever the game has saved since. path: another page, e.g. 'music.html'.
// init: a function run in the page before its own scripts (a fake gamepad),
// called with initArgs.
export async function openGame(browser, { device = 'desktop', query = '', storage = {}, path: page_ = '', init = null, initArgs = [], target = TARGET } = {}) {
  if (target === 'rust') return openRust(browser, { device, query, storage, path: page_, init, initArgs });
  const context = await browser.createBrowserContext();
  const page = await context.newPage();
  const cdp = await page.createCDPSession();
  const game = new Game(page, cdp, context);

  page.on('pageerror', (e) => game.errors.push(String(e.message || e)));
  page.on('console', (m) => {
    const text = m.text();
    if (IGNORE.some((re) => re.test(text))) return;
    if (m.type() === 'error') game.errors.push(text);
    else if (m.type() === 'warn' || m.type() === 'warning') game.warnings.push(text);
  });

  if (!process.env.MR_BASE_URL) {
    await page.setRequestInterception(true);
    page.on('request', async (req) => {
      const u = new URL(req.url());
      if (u.origin !== ORIGIN) return req.abort('blockedbyclient');
      const rel = decodeURIComponent(u.pathname).replace(/^\/+/, '') || 'index.html';
      const file = path.resolve(ROOT, rel);
      if (!file.startsWith(ROOT + path.sep)) return req.respond({ status: 403, body: 'forbidden' });
      try {
        const body = await fs.readFile(file);
        await req.respond({ status: 200, contentType: TYPES[path.extname(file)] || 'application/octet-stream', body });
      } catch {
        await req.respond({ status: 404, contentType: 'text/plain', body: 'not found: ' + rel });
      }
    });
    page.on('response', (r) => { if (r.status() >= 400) game.errors.push(`HTTP ${r.status()} ${r.url()}`); });
  }

  await page.emulate({ userAgent: DEVICES[device].userAgent || await browser.userAgent(), viewport: DEVICES[device].viewport });
  if (Object.keys(storage).length) {
    await page.evaluateOnNewDocument((entries) => {
      for (const [k, v] of Object.entries(entries)) if (localStorage.getItem(k) === null) localStorage.setItem(k, v);
    }, Object.fromEntries(Object.entries(storage).map(([k, v]) => [k, JSON.stringify(v)])));
  }
  if (init) await page.evaluateOnNewDocument(init, ...initArgs);
  // MR_QUERY adds parameters to every page, e.g. MR_QUERY=kernel=1 runs the
  // whole suite with the Rust port's parity kernel on (src/parity/hooks.js).
  const q = [query.replace(/^\?/, ''), process.env.MR_QUERY || ''].filter(Boolean).join('&');
  await page.goto(BASE + page_ + (q ? '?' + q : ''));
  await game.waitReady();
  return game;
}

// openGame for the Rust build (dist/next/, built by `cargo xtask web`):
// the same profile, devices, storage, init and interception, with the
// bridge installed before the page's scripts. `path` (music.html) is the
// JS page, which the Rust menu links to until it has its own player.
async function openRust(browser, { device, query, storage, path: page_, init, initArgs }) {
  const context = await browser.createBrowserContext();
  const page = await context.newPage();
  const cdp = await page.createCDPSession();
  const target = page_ ? 'js' : 'rust';
  const game = new Game(page, cdp, context, target);
  // The JS game handles an input event at once; the Rust client reads it
  // in its next frame. Each mouse, finger and key event the suites send
  // (through the Game's helpers or the page's own) waits for two frames.
  for (const [dev, names] of [[page.mouse, ['click', 'down', 'up']], [page.touchscreen, ['tap']], [page.keyboard, ['down', 'up']]]) {
    for (const n of names) {
      const f = dev[n].bind(dev);
      dev[n] = async (...a) => {
        const r = await f(...a);
        if (game.target === 'rust') await game.frames(2);
        return r;
      };
    }
  }

  page.on('pageerror', (e) => game.errors.push(String(e.message || e)));
  page.on('console', (m) => {
    const text = m.text();
    if (IGNORE.some((re) => re.test(text))) return;
    // winit cancels every touchstart on the canvas; Chrome says it could
    // not when a CDP touch arrives during a busy frame. The canvas is
    // touch-action: none, so nothing scrolls either way (DECISIONS D846).
    if (/Ignored attempt to cancel a touchstart event/.test(text)) return;
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
    const file = path.resolve(ROOT, rel);
    if (!file.startsWith(ROOT + path.sep)) return req.respond({ status: 403, body: 'forbidden' });
    try {
      const body = await fs.readFile(file);
      const ext = path.extname(file);
      await req.respond({ status: 200, contentType: RUST_TYPES[ext] || TYPES[ext] || 'application/octet-stream', body });
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
  if (target === 'rust') await page.evaluateOnNewDocument(installBridge, String(selectorId));
  if (init) await page.evaluateOnNewDocument(init, ...initArgs);
  const q = [query.replace(/^\?/, ''), process.env.MR_QUERY || ''].filter(Boolean).join('&');
  await page.goto(ORIGIN + '/' + (page_ || RUST_PAGE) + (q ? '?' + q : ''));
  await game.waitReady();
  return game;
}
