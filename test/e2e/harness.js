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

import puppeteer from 'puppeteer-core';
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const ORIGIN = 'https://midnight-racer.test';
const BASE = process.env.MR_BASE_URL ? process.env.MR_BASE_URL.replace(/\/?$/, '/') : ORIGIN + '/';

const TYPES = {
  '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css',
  '.json': 'application/json', '.webmanifest': 'application/manifest+json',
  '.png': 'image/png', '.svg': 'image/svg+xml',
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
export function launch() {
  return puppeteer.launch({
    executablePath: process.env.CHROME_PATH || '/usr/bin/google-chrome',
    headless: process.env.MR_HEADFUL ? false : 'new',
    args: [
      '--use-angle=vulkan', '--enable-gpu', '--ignore-gpu-blocklist',
      '--autoplay-policy=document-user-activation-required',
    ],
  });
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Console noise that isn't the game's fault: the web font is blocked on purpose.
const IGNORE = [/Failed to load resource/, /KHR_parallel_shader_compile/];

export class Game {
  constructor(page, cdp, context) {
    this.page = page;
    this.cdp = cdp;
    this.context = context;
    this.errors = [];
    this.warnings = [];
  }

  // Evaluate in the page without a user gesture. Takes a function (with
  // JSON-able args) or an expression string; returns the JSON-able result.
  async eval(fn, ...args) {
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

  // What a player would see: which screen is up, and the race and audio state.
  snapshot() {
    return this.eval(() => {
      const shown = ['loading', 'menu', 'pause', 'results'].filter((id) => !document.getElementById(id).classList.contains('hidden'));
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
    return this.cdp.send('Input.dispatchTouchEvent', {
      type, touchPoints: points.map((p) => ({ x: p.x, y: p.y, id: p.id ?? 0, radiusX: 8, radiusY: 8, force: 1 })),
    });
  }

  async reload() {
    await this.page.reload();
    await this.waitReady();
  }

  async waitReady(timeout = 60000) {
    await this.waitFor('window.__ready === true', { timeout, what: 'the game to load' });
  }

  async close() { await this.context.close(); }
}

// Open the game in a fresh profile (so no settings leak between tests).
// storage: localStorage entries to seed before the game first boots, e.g.
// { 'mr.level': 'coast' } (values are JSON-encoded here). A reload keeps
// whatever the game has saved since. path: another page, e.g. 'music.html'.
export async function openGame(browser, { device = 'desktop', query = '', storage = {}, path: page_ = '' } = {}) {
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
  await page.goto(BASE + page_ + (query ? '?' + query.replace(/^\?/, '') : ''));
  await game.waitReady();
  return game;
}
