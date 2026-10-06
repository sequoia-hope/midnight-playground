// The staged Hot Pursuit scenes (roadmap WP 8.1 and 8.2, L4; DECISIONS
// D930, D950): each scene of parity/golden/pursuit/scenes.json drawn by the
// JS game's own PursuitView.js and by the Rust client's play::police and
// play::fx, at a fly-camera station of the level, the scenery frozen, the
// kernel on, 1280 × 800.
//
//   node tools/parity/pursuit-scenes.mjs --side js|rust [--only a,b]
//       [--out <dir>] [--backend webgpu|webgl2] [--timeout ms]
//
// JS: the game answered from the worktree by request interception
// (Math.random seeded as for the scene export before the page's scripts,
// lib/seed-random.mjs), then in the page a stand-in race on the game's
// scene (the level's track, a player Vehicle with its model, an Effects with
// the player added, the pursuit and police streams of src/parity/sim.js)
// and a new PursuitView on it with the scene's heat and options; the units
// activated and their sirens set, the roadblock or spikes laid, then with
// Math.random replaced by mulberry32(scene seed) each frame: PursuitView's
// clock, the events due, player.sync, pv.sync, effects.update; restored
// after. Rust: the web build (dist/next) from the registered server
// (--port, else $PORT, else `proj port`; no default), ?pv=<scene>, the
// screenshot once the scene is staged and the station settled.
//
// Writes <out>/<side>/<level>/<scene>.png (default
// parity/report/pursuit/scenes), the layout `cargo xtask parity shots
// --a <dir> --b <dir>` compares.

import puppeteer from 'puppeteer-core';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { ROOT } from './lib/jstree.mjs';
import { seedRandom, RANDOM_SEED } from './lib/seed-random.mjs';

const args = process.argv.slice(2);
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
const side = opt('--side', 'js');
const backend = opt('--backend', 'webgpu');
const timeoutMs = Number(opt('--timeout', 240000));
const only = opt('--only', null)?.split(',');
const DEFS = JSON.parse(fs.readFileSync(path.join(ROOT, 'parity/golden/pursuit/scenes.json'), 'utf8'));
const scenes = DEFS.scenes.filter((s) => !only || only.includes(s.name));
const tag = `${side}${side === 'rust' && backend === 'webgl2' ? '-gl' : ''}`;
const OUT = path.resolve(opt('--out', path.join(ROOT, 'parity/report/pursuit/scenes')), tag);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function serverBase() {
  let port = opt('--port', null) || process.env.PORT;
  if (!port) port = execFileSync('proj', ['port'], { cwd: ROOT, encoding: 'utf8' }).trim();
  if (!/^\d+$/.test(port)) throw new Error(`no server port (proj port gave "${port}")`);
  const common = path.resolve(ROOT, execFileSync('git', ['rev-parse', '--git-common-dir'], { cwd: ROOT, encoding: 'utf8' }).trim());
  const rel = path.relative(path.dirname(common), ROOT);
  if (rel.startsWith('..')) throw new Error(`${ROOT} is not below the served checkout`);
  return `http://127.0.0.1:${port}${rel ? '/' + rel.split(path.sep).map(encodeURIComponent).join('/') : ''}`;
}

const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css', '.json': 'application/json', '.png': 'image/png', '.jpg': 'image/jpeg', '.svg': 'image/svg+xml', '.webmanifest': 'application/manifest+json', '.mp3': 'audio/mpeg', '.wasm': 'application/wasm', '.bin': 'application/octet-stream' };
const camQuery = (c) => `s=${c.s}&h=${c.h}&back=${c.back}&lat=${c.lat}&yaw=${c.yaw}&pitch=${c.pitch}`;

// Runs in the JS page: the scene staged on the game's own scene.
async function stageJs(def) {
  const THREE = window.__THREE;
  const { Effects } = await import('/src/game/Effects.js');
  const { PursuitView } = await import('/src/game/PursuitView.js');
  const { buildVehicle } = await import('/src/vehicles/CarModel.js');
  const { Vehicle } = await import('/src/vehicles/Vehicle.js');
  const { CAR_SPECS } = await import('/src/vehicles/CarPhysics.js');
  const { mulberry32, smoothstep } = await import('/src/util/math.js');
  const { simStreams } = await import('/src/parity/sim.js');
  const w = window.__world, camera = window.__camera, t = w.track, scene = w.realScene;
  const group = new THREE.Group();
  scene.add(group);
  // The player's car, as Race builds it.
  const P = def.player, spec = CAR_SPECS[P.kind];
  const pm = buildVehicle(P.kind, { color: P.color ?? spec.color, lod: 'high', seed: 1 });
  pm.root.traverse((o) => { if (o.isMesh) o.castShadow = true; });
  group.add(pm.root);
  const player = new Vehicle(pm, { kind: P.kind, mass: spec.mass, name: 'You', color: P.color ?? spec.color });
  player.place(t, P.s, P.lat);
  const sp = P.speed || 0;
  player.vx = Math.cos(player.yaw) * sp; player.vz = Math.sin(player.yaw) * sp;
  player.onGround = true;
  const effects = new Effects(scene, w.renderer, camera);
  effects.resize(window.innerHeight * w.renderer.getPixelRatio(), camera.fov);
  effects.addCar(player, { player: true });
  const streams = simStreams(def.seed);
  const race = {
    track: t, level: w.level, buildVehicle, group, scene, camera, effects, player, spec,
    playerBody: { s: P.s, lat: P.lat, v: player }, ais: [], hud: {},
    phys: { spiked: P.spiked || 0, damage: 0 }, world: { sky: { night: def.night } },
  };
  const pv = new PursuitView(race, { heat: def.heat, cops: 6, flash: def.flash, hq: def.hq, rng: streams.pursuit, policeRng: streams.police });
  pv.damage = P.damage || 0;
  const pu = pv.pursuit;
  for (const u of def.units) {
    const unit = pu.units[u.unit];
    pu.activate(unit, u.s, u.lat, u.speed || 0, u.mode, u.dir || 1);
    unit.siren = u.siren;
  }
  if (def.roadblock) pu.placeRoadblock(def.roadblock.s);
  if (def.spikes) pu.placeSpikes(def.spikes.s);
  pu.events.length = 0;
  const lightsOn = smoothstep(0.25, 0.6, def.night);
  const orig = Math.random;
  Math.random = mulberry32(def.seed);
  try {
    for (let k = 0; k < def.frames; k++) {
      pv.t = def.t + k * def.dt;
      for (const e of def.events.filter((e) => e.frame === k)) {
        const F = t.frame(e.s);
        pu.events.push({ type: e.type, x: F.x + F.rx * e.lat, z: F.z + F.rz * e.lat, player: true });
      }
      player.sync(t, def.dt);
      pv.sync(def.dt, def.night, lightsOn);
      effects.update(def.dt, def.night, new Map([[player, { nitro: false, skid: 0, launch: false }]]));
    }
  } finally {
    Math.random = orig;
  }
  window.__pvStaged = {
    units: pu.units.map((u) => u.active && u.v.model.root.visible),
    far: pv.cars.filter((u) => u.active).map((u) => !!u.v.model.isFar),
    light: pv.light ? { intensity: pv.light.intensity, color: pv.light.color.toArray(), pos: pv.light.position.toArray() } : null,
    smoke: effects.smoke.alpha.filter((a) => a > 0).length, sparks: effects.sparks.alpha.filter((a) => a > 0).length,
  };
  return window.__pvStaged;
}

fs.mkdirSync(OUT, { recursive: true });
const flags = ['--use-angle=vulkan', '--enable-gpu', '--ignore-gpu-blocklist', '--mute-audio'];
if (side === 'rust' && backend === 'webgpu') flags.push('--enable-unsafe-webgpu', '--enable-features=Vulkan');
if (side === 'rust' && backend === 'webgl2') flags.push('--disable-blink-features=WebGPU', '--disable-features=WebGPU');
const browser = await puppeteer.launch({ executablePath: process.env.CHROME_PATH || '/usr/bin/google-chrome', headless: 'new', args: flags, protocolTimeout: timeoutMs });
const timer = setTimeout(() => { console.error('timeout'); browser.close().finally(() => process.exit(2)); }, timeoutMs);
const errors = [];
try {
  for (const def of scenes) {
    const dir = path.join(OUT, def.level);
    fs.mkdirSync(dir, { recursive: true });
    const file = path.join(dir, `${def.name}.png`);
    // The JS side gets a fresh profile per scene; the Rust side its own page
    // in the default one, where the screenshot download is allowed.
    const context = side === 'js' ? await browser.createBrowserContext() : browser.defaultBrowserContext();
    const page = await context.newPage();
    await page.setViewport({ width: 1280, height: 800, deviceScaleFactor: 1 });
    page.on('pageerror', (e) => errors.push(`${def.name}: ${e}`));
    page.on('console', (m) => { if (m.type() === 'error' && !/Failed to load resource|KHR_parallel/.test(m.text())) errors.push(`${def.name}: ${m.text()}`); if (process.env.MR_VERBOSE) console.log(`[page] ${m.text()}`); });
    const t0 = Date.now();
    if (side === 'js') {
      const ORIGIN = 'https://midnight-racer.test';
      await page.setRequestInterception(true);
      page.on('request', (req) => {
        const u = new URL(req.url());
        if (u.origin !== ORIGIN) return req.abort('blockedbyclient');
        const f = path.resolve(ROOT, decodeURIComponent(u.pathname).replace(/^\/+/, '') || 'index.html');
        if (!fs.existsSync(f) || fs.statSync(f).isDirectory()) return req.respond({ status: 404, body: 'not found' });
        req.respond({ status: 200, contentType: TYPES[path.extname(f)] || 'application/octet-stream', body: fs.readFileSync(f) });
      });
      await page.evaluateOnNewDocument(seedRandom, RANDOM_SEED);
      await page.goto(`${ORIGIN}/index.html?kernel=1&freeze=1&level=${def.level}&${camQuery(def.cam)}`);
      await page.waitForFunction('window.__ready === true && !!window.__world?.track', { timeout: timeoutMs, polling: 200 });
      await page.evaluate(() => new Promise((r) => { let k = 0; const f = () => (++k >= 10 ? r() : requestAnimationFrame(f)); requestAnimationFrame(f); }));
      const n = await page.evaluate(stageJs, def);
      await page.evaluate(() => new Promise((r) => { let k = 0; const f = () => (++k >= 6 ? r() : requestAnimationFrame(f)); requestAnimationFrame(f); }));
      await page.screenshot({ path: file });
      console.log(`${def.name}: ${JSON.stringify(n)}`);
    } else {
      const cdp = await page.createCDPSession();
      await cdp.send('Browser.setDownloadBehavior', { behavior: 'allow', downloadPath: dir });
      await page.goto(`${serverBase()}/dist/next/index.html?level=${def.level}&freeze=1&${camQuery(def.cam)}&pv=${def.name}`);
      for (;;) {
        const s = await page.evaluate(() => ({ st: window.__mr?.state, err: window.__mr?.error, ready: window.__mr?.ready, staged: window.__mr?.pvStaged || 0, quiet: window.__mr?.flyQuiet || 0 }));
        if (s.err || s.st === 'failed') throw new Error(`${def.name}: ${s.err}`);
        if (s.ready && s.staged >= 4 && s.quiet >= 3) break;
        if (Date.now() - t0 > timeoutMs) throw new Error(`${def.name}: not staged (${JSON.stringify(s)})`);
        await sleep(100);
      }
      fs.rmSync(file, { force: true });
      await page.evaluate((n) => window.__mr.screenshot(n), `${def.name}.png`);
      let size = -1;
      for (let i = 0; i < 200; i++) {
        const k = fs.existsSync(file) ? fs.statSync(file).size : -1;
        if (k > 0 && k === size) break;
        size = k;
        await sleep(100);
      }
      if (!fs.existsSync(file)) errors.push(`${def.name}: no screenshot`);
      console.log(`${def.name}: ${((Date.now() - t0) / 1000).toFixed(1)} s, ${await page.evaluate(() => window.__mr.backend)}`);
    }
    if (side === 'js') await context.close(); else await page.close();
  }
} catch (e) {
  errors.push(String(e.message || e));
} finally {
  clearTimeout(timer);
  await browser.close();
}
if (errors.length) { console.error(`errors:\n  ${errors.join('\n  ')}`); process.exit(1); }
