// Screenshot stations of the JS game with only the terrain, the road and the
// sky drawn (roadmap WP 2.4's gate): what `scene-export.mjs --base` exports
// (`<level>.base.mrscene`), seen from the same stations as `shots.mjs`, so
// the Rust client drawing the base export can be compared with it.
//
//   node tools/parity/base-shots.mjs [--run a] [--only sierra,...] [--step 250]
//
// Writes PNGs to parity/cache/<js-tree-key>/shots/<run>.base/<level>/ and a
// stations.json beside them. Everything else is as shots.mjs takes its
// shots (kernel on, scenery frozen, Math.random seeded, a fresh Chrome per
// level, 1280 × 800, high quality); the only difference is that every
// object of the scene except the terrain group, the road group and the
// sky's dome and lights is hidden (`visible = false`, which three skips
// when it draws and when it renders the shadow map). The game itself is not
// changed: the page is the game as shots.mjs loads it.

import fs from 'node:fs';
import path from 'node:path';
import { launch, openGame } from '../../test/e2e/harness.js';
import { cacheDir } from './lib/jstree.mjs';
import { seedRandom, RANDOM_SEED } from './lib/seed-random.mjs';

const args = process.argv.slice(2);
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
const RUN = opt('--run', 'a');
const STEP = Number(opt('--step', 250));
const ONLY = args.includes('--only') ? opt('--only').split(',') : null;
const LEVELS = ['sierra', 'coast', 'streets', 'desert', 'seaside', 'cruise'].filter((l) => !ONLY || ONLY.includes(l));

// As shots.mjs (which runs on import, so they are repeated here).
const VIEWS = {
  chase: { h: 2.2, back: 7, lat: 0, yaw: 0, pitch: -0.04 },
  high: { h: 40, back: 70, lat: 0, yaw: 0, pitch: -0.3 },
};
const ATTRACT = { s: 120, h: 7, back: 22, lat: 3, yaw: 0, pitch: -0.05 };

const frames = (n) => new Promise((resolve) => { let k = 0; const f = () => (++k >= n ? resolve() : requestAnimationFrame(f)); requestAnimationFrame(f); });

// In the page: hide all but what the base export holds (scene-page.js
// `exportWorld({ base: true })`: the 'terrain' group and world.road.group
// under world.root, plus the sky's dome, sun, its target and the hemisphere
// light). Returns what was hidden, for the log.
function baseOnly() {
  const world = window.__world;
  const keepRoots = new Set([world.root, world.sky.dome, world.sky.sun, world.sky.sun.target, world.sky.hemi]);
  const keepUnderRoot = new Set(world.root.children.filter((c) => c.name === 'terrain'));
  if (keepUnderRoot.size !== 1 || !world.road?.group) throw new Error('base-shots: terrain or road group not found');
  keepUnderRoot.add(world.road.group);
  let hidden = 0;
  for (const c of world.realScene.children) if (!keepRoots.has(c) && c.visible) { c.visible = false; hidden++; }
  for (const c of world.root.children) if (!keepUnderRoot.has(c) && c.visible) { c.visible = false; hidden++; }
  return hidden;
}

for (const level of LEVELS) {
  const browser = await launch();
  try {
    const dir = path.join(cacheDir(`shots/${RUN}.base`), level);
    fs.mkdirSync(dir, { recursive: true });
    const g = await openGame(browser, {
      query: `kernel=1&freeze=1&level=${level}&s=${ATTRACT.s}&h=${ATTRACT.h}&back=${ATTRACT.back}&lat=${ATTRACT.lat}&pitch=${ATTRACT.pitch}`,
      init: seedRandom, initArgs: [RANDOM_SEED],
    });
    const hidden = await g.eval(`(${baseOnly})()`);
    const len = await g.eval(() => (window.__world.track.loop ? window.__world.track.length : window.__world.track.roadEnd));
    const stations = [{ name: 'attract', ...ATTRACT }];
    for (let s = STEP; s < len - 30; s += STEP) for (const [view, p] of Object.entries(VIEWS)) stations.push({ name: `${String(s).padStart(5, '0')}-${view}`, s, ...p });
    for (const st of stations) {
      await g.eval((p) => Object.assign(window.__dbg, { s: p.s, h: p.h, back: p.back, lat: p.lat, yaw: p.yaw, pitch: p.pitch, speed: 0 }), st);
      await g.eval(`(${frames})(6)`);
      await g.page.screenshot({ path: path.join(dir, st.name + '.png') });
    }
    if (g.errors.length) throw new Error(`${level}: page errors: ${g.errors.join(' | ')}`);
    fs.writeFileSync(path.join(dir, 'stations.json'), JSON.stringify({ level, step: STEP, viewport: [1280, 800], base: true, stations }, null, 1));
    console.log(`${level}: ${stations.length} shots, terrain, road and sky only (${hidden} objects hidden)`);
    await g.close();
  } finally {
    await browser.close();
  }
}
