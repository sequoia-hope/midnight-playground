// The staged effect scenes (roadmap WP 4.4, L4; DECISIONS D804): each scene
// of parity/golden/effects/scenes.json drawn by the JS game's own
// Effects.js and by the Rust client's play::fx, at a fly-camera station of
// the level, the scenery frozen, the kernel on, 1280 × 800.
//
//   node tools/parity/effects-scenes.mjs --side js|rust [--only a,b]
//       [--out <dir>] [--backend webgpu|webgl2] [--nofx] [--timeout ms]
//
// JS: the game answered from the worktree by request interception
// (Math.random seeded as for the scene export before the page's scripts,
// lib/seed-random.mjs), then in the page a new Effects on the game's scene
// with bare cars (a body group for the flames), Math.random replaced by
// mulberry32(scene seed) while the frames run (the bursts, then
// effects.update, each frame), and restored. --nofx: the same frame with
// the effects hidden. Rust: the web build (dist/next) from the registered
// server (--port, else $PORT, else `proj port`; no default), ?fx=<scene>,
// the screenshot once the scene is staged and the station settled.
//
// Writes <out>/<side>[-nofx]/<level>/<scene>.png (default
// parity/report/effects/scenes), the layout `cargo xtask parity shots
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
const nofx = args.includes('--nofx');
const timeoutMs = Number(opt('--timeout', 240000));
const only = opt('--only', null)?.split(',');
const DEFS = JSON.parse(fs.readFileSync(path.join(ROOT, 'parity/golden/effects/scenes.json'), 'utf8'));
const scenes = DEFS.scenes.filter((s) => !only || only.includes(s.name));
const tag = `${side}${side === 'rust' && backend === 'webgl2' ? '-gl' : ''}${nofx ? '-nofx' : ''}`;
const OUT = path.resolve(opt('--out', path.join(ROOT, 'parity/report/effects/scenes')), tag);
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
async function stageJs(def, hide) {
  const THREE = window.__THREE;
  const { Effects } = await import('/src/game/Effects.js');
  const { mulberry32 } = await import('/src/util/math.js');
  const w = window.__world, camera = window.__camera, t = w.track;
  const fx = new Effects(w.realScene, w.renderer, camera);
  fx.resize(window.innerHeight * w.renderer.getPixelRatio(), camera.fov);
  const pose = (c, k) => {
    const sp = c.speed || 0, F = t.frame(c.s + sp * k * def.dt), lat = c.lat || 0;
    const yaw = Math.atan2(F.fz, F.fx) + (c.yaw || 0);
    return { x: F.x + F.rx * lat, y: F.y, z: F.z + F.rz * lat, yaw, visualYaw: 0, vx: Math.cos(yaw) * sp, vz: Math.sin(yaw) * sp, onGround: true };
  };
  const vs = def.cars.map((c) => {
    const body = new THREE.Group();
    w.realScene.add(body);
    const v = { model: { root: { visible: true }, body, exhausts: c.exhausts.map((e) => new THREE.Vector3().fromArray(e)), dims: { wheelBase: c.wheelBase, track: c.track } } };
    fx.addCar(v, { player: !!c.player });
    return v;
  });
  const orig = Math.random;
  Math.random = mulberry32(def.seed);
  try {
    for (let k = 0; k < def.frames; k++) {
      def.cars.forEach((c, i) => Object.assign(vs[i], pose(c, k)));
      for (const b of def.bursts.filter((b) => b.frame === k)) {
        const F = t.frame(b.s);
        const x = F.x + F.rx * b.lat, y = F.y + b.h, z = F.z + F.rz * b.lat;
        if (b.kind === 'sparks') fx.sparksAt(x, y, z, b.n, b.vx || 0, b.vz || 0);
        else fx.smokeAt(x, y, z, b.amount, b.vx || 0, b.vz || 0, def.night);
      }
      const extras = new Map();
      def.cars.forEach((c, i) => extras.set(vs[i], { nitro: !!c.nitro, skid: c.skid || 0, launch: !!c.launch }));
      fx.update(def.dt, def.night, extras);
    }
  } finally {
    Math.random = orig;
  }
  for (const v of vs) {
    v.model.body.position.set(v.x, v.y, v.z);
    v.model.body.rotation.set(0, Math.PI / 2 - v.yaw, 0);
  }
  if (hide) {
    for (const c of fx.cars) { c.pool.visible = false; for (const f of c.flames) f.visible = false; }
    fx.smoke.points.visible = false; fx.sparks.points.visible = false; fx.skids.mesh.visible = false;
  }
  window.__fxStaged = { smoke: fx.smoke.alpha.filter((a) => a > 0).length, sparks: fx.sparks.alpha.filter((a) => a > 0).length, skids: fx.skids.next };
  return window.__fxStaged;
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
      const n = await page.evaluate(stageJs, def, nofx);
      await page.evaluate(() => new Promise((r) => { let k = 0; const f = () => (++k >= 6 ? r() : requestAnimationFrame(f)); requestAnimationFrame(f); }));
      await page.screenshot({ path: file });
      console.log(`${def.name}: ${JSON.stringify(n)}`);
    } else {
      const cdp = await page.createCDPSession();
      await cdp.send('Browser.setDownloadBehavior', { behavior: 'allow', downloadPath: dir });
      const fxq = nofx ? '' : `&fx=${def.name}`;
      await page.goto(`${serverBase()}/dist/next/index.html?level=${def.level}&freeze=1&${camQuery(def.cam)}${fxq}`);
      for (;;) {
        const s = await page.evaluate(() => ({ st: window.__mr?.state, err: window.__mr?.error, ready: window.__mr?.ready, staged: window.__mr?.fxStaged || 0, quiet: window.__mr?.flyQuiet || 0 }));
        if (s.err || s.st === 'failed') throw new Error(`${def.name}: ${s.err}`);
        if (s.ready && (nofx || s.staged >= 4) && s.quiet >= 3) break;
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
