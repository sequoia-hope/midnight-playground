// Screenshot stations of the JS game (roadmap WP 0.6, SPEC 12): for each
// level, a station every 250 m along the route, each seen from a chase-height
// view and a high view, at the route's own time of day, plus the menu's
// attract view at its starting pose. Debug fly camera only (no race, so no
// traffic or particles), scenery frozen (?freeze=1), the parity kernel on.
// Desktop viewport 1280×800, high quality (the desktop default).
//
//   node tools/parity/shots.mjs [--run a] [--only sierra,...] [--step 250]
//
// Writes PNGs to parity/cache/<js-tree-key>/shots/<run>/<level>/ and a
// stations.json beside them. Two runs (a, b) of the same tree give the
// JS-against-JS noise floor: cargo xtask parity shots --a <dir> --b <dir>.
//
// Stations are visited in increasing s, one page per level. The environment
// map refreshes whenever the time of day has moved 2.5 % of the route
// (main.js refreshEnv); at 250 m spacing every station on a sprint level is
// further than that from the last, so each is lit as if loaded there. The
// loops keep one time of day.

import fs from 'node:fs';
import path from 'node:path';
import { launch, openGame } from '../../test/e2e/harness.js';
import { cacheDir } from './lib/jstree.mjs';

const args = process.argv.slice(2);
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
const RUN = opt('--run', 'a');
const STEP = Number(opt('--step', 250));
const ONLY = args.includes('--only') ? opt('--only').split(',') : null;
const LEVELS = ['sierra', 'coast', 'streets', 'desert', 'seaside', 'cruise'].filter((l) => !ONLY || ONLY.includes(l));

// The fly camera's parameters (main.js flyCamera; the Rust client takes the
// same): camera at frame(s - back) + lat across, h up, looking at
// frame(s + 20) at 0.4 h, then yaw and pitch.
export const VIEWS = {
  chase: { h: 2.2, back: 7, lat: 0, yaw: 0, pitch: -0.04 },
  high: { h: 40, back: 70, lat: 0, yaw: 0, pitch: -0.3 },
};
// The attract camera's first pose behind the menu (main.js: attract.s = 120).
export const ATTRACT = { s: 120, h: 7, back: 22, lat: 3, yaw: 0, pitch: -0.05 };

const frames = (n) => new Promise((resolve) => { let k = 0; const f = () => (++k >= n ? resolve() : requestAnimationFrame(f)); requestAnimationFrame(f); });

const browser = await launch();
try {
  for (const level of LEVELS) {
    const dir = path.join(cacheDir(`shots/${RUN}`), level);
    fs.mkdirSync(dir, { recursive: true });
    const g = await openGame(browser, { query: `kernel=1&freeze=1&level=${level}&s=${ATTRACT.s}&h=${ATTRACT.h}&back=${ATTRACT.back}&lat=${ATTRACT.lat}&pitch=${ATTRACT.pitch}` });
    const len = await g.eval(() => (window.__world.track.loop ? window.__world.track.length : window.__world.track.roadEnd));
    const stations = [{ name: 'attract', ...ATTRACT }];
    for (let s = STEP; s < len - 30; s += STEP) for (const [view, p] of Object.entries(VIEWS)) stations.push({ name: `${String(s).padStart(5, '0')}-${view}`, s, ...p });
    for (const st of stations) {
      await g.eval((p) => Object.assign(window.__dbg, { s: p.s, h: p.h, back: p.back, lat: p.lat, yaw: p.yaw, pitch: p.pitch, speed: 0 }), st);
      await g.eval(`(${frames})(6)`);
      await g.page.screenshot({ path: path.join(dir, st.name + '.png') });
    }
    if (g.errors.length) throw new Error(`${level}: page errors: ${g.errors.join(' | ')}`);
    fs.writeFileSync(path.join(dir, 'stations.json'), JSON.stringify({ level, step: STEP, viewport: [1280, 800], stations }, null, 1));
    console.log(`${level}: ${stations.length} shots`);
    await g.close();
  }
} finally {
  await browser.close();
}
