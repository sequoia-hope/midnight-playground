// The e2e harness for the Rust web build (roadmap WP 6.2; SPEC 8.5): the
// JS suites' `test/e2e/harness.js` API, driving `dist/next/` through
// `window.__mp` instead of the DOM. The harness itself is Playground Kit's
// js/harness.mjs (D1233): headless Chrome on the GPU with WebGPU, the
// working tree answered by request interception (no server, no port), a
// fresh context per game, the same devices as the JS harness. This file
// adds what is Midnight's: the JS selectors as the Rust build's ids, the
// Settings-screen detour, the race's flow helpers, and the scene
// substitution (a full export over 90 MB is answered with its
// terrain-road-sky one, `<level>.base.mrscene`, since the full exports
// are too big for request interception, DECISIONS D106; `{ scenes:
// 'base' }` always serves the base ones, `'full'` never).
//
// The JS suites themselves run against the Rust build through the JS
// harness's `target: 'rust'` (WP 6.7, `npm run test:e2e:rust`); this one
// stays for the Rust-only suites here and the picture tools.

import fs from 'node:fs/promises';
import path from 'node:path';
import { ROOT } from '../lib/jstree.mjs';
import { DEVICES, Game, launch as kitLaunch, openGame as kitOpenGame, sleep } from '../../../../game-engine/js/harness.mjs';

export { DEVICES, sleep };
if (process.env.MP_VERBOSE) process.env.PK_VERBOSE = '1';

export function launch() {
  return kitLaunch({ root: ROOT, headful: !!process.env.MP_HEADFUL });
}

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

export class MpGame extends Game {
  snapshot() {
    return this.evalG((G) => ({
      screen: G?.screen,
      mode: G?.mode,
      race: G?.race ? G.race.state : null,
      audio: G?.audio?.context ?? 'none',
      state: G?.state,
      fullscreen: !!document.fullscreenElement,
      touchUI: !!G?.touchUi,
      level: G?.level,
    }));
  }

  ui(sel) { return super.ui(idOf(sel)); }

  // A control's centre in CSS px, scrolled to the middle first; fails if it
  // is not shown, off screen, or another control is on top of that point.
  // The options live on the Settings screen (D1196): a control not shown
  // from the menu or the pause screen opens Settings first, and one not
  // shown from Settings closes it, as a player would.
  async center(sel) {
    const id = idOf(sel);
    for (let tries = 0; tries < 2; tries++) {
      const r = await this.centerOnce(id);
      if (!r.error) return r;
      const screen = await this.screen();
      const via = screen === 'menu' ? 'btn-settings' : screen === 'pause' ? 'btn-settings-pause' : screen === 'settings' ? 'btn-settings-done' : null;
      if (!via || tries) throw new Error(r.error);
      await this.evalG((G, v) => G.stage({ cmd: 'act', id: v }), via);
      await this.frames(3);
    }
    throw new Error('unreachable');
  }

  async waitReady(timeout = 240000) {
    await this.waitFor((G) => {
      if (G?.state === 'failed') throw new Error('client failed: ' + G.error);
      return G?.ready === true && G.screen !== 'loading' && !!G.uiNodes;
    }, { timeout, interval: 250, what: 'the client to load' });
    // The screen's first layouts (the pixel ratio changes with it, D575).
    await this.frames(6);
  }
}

// Open the Rust build in a fresh profile. storage: localStorage entries
// seeded before boot ({ 'mr.level': 'seaside' }, JSON-encoded here);
// scenes: 'auto' (default), 'base' or 'full'; downloads: a directory for
// __mp.screenshot().
export function openGame(browser, { device = 'desktop', query = '', storage = {}, scenes = 'auto', downloads = null, wait = true } = {}) {
  const rewrite = async (rel) => {
    if (/\/[a-z]+\.mrscene$/.test(rel) && scenes !== 'full') {
      const big = await fs.stat(path.resolve(ROOT, rel)).then((st) => st.size > 90e6).catch(() => false);
      if (scenes === 'base' || big) return rel.replace(/\/([a-z]+)\.mrscene$/, '/$1.base.mrscene');
    }
    return rel;
  };
  return kitOpenGame(browser, { root: ROOT, global: '__mp', page: 'dist/next/index.html', device, query, storage, rewrite, downloads, wait, GameClass: MpGame });
}

// flow-helpers.js, for the Rust build.
export const stored = (game, key) => game.eval((k) => {
  const v = localStorage.getItem('mr.' + k);
  return v === null ? null : JSON.parse(v);
}, key);

export const raceStarted = () => !!window.__mp?.race && ['countdown', 'racing'].includes(window.__mp.race.state)
  && window.__mp.mode === 'race' && window.__mp.screen === 'none';

export async function startFromMenu(game, { touch = false, timeout = 30000 } = {}) {
  await (touch ? game.tap('#btn-start') : game.click('#btn-start'));
  await game.waitFor(raceStarted, { timeout, what: 'the race to start' });
}

export const markRace = (game) => game.eval(() => { window.__marked = window.__mp.races; });
export const newRaceStarted = () => window.__mp.races > window.__marked && !!window.__mp.race
  && ['countdown', 'racing'].includes(window.__mp.race.state) && window.__mp.mode === 'race';

export async function waitRacing(game, timeout = 20000) {
  await game.waitFor(() => window.__mp.race?.state === 'racing', { timeout, what: 'the countdown to end' });
}

export async function expectScreen(game, name, timeout = 8000) {
  await game.waitFor(`window.__mp.screen === ${JSON.stringify(name)}`, { timeout, what: `the "${name}" screen` });
}

export const isShown = (game, sel) => game.ui(sel).then((u) => !!u && u.visible && u.w > 0 && u.h > 0);
