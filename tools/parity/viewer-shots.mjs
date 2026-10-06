// The level viewer's pictures and costs (SPEC 8.6, roadmap WP 6.9), for the
// owner's review: every level from high above (the overview, with its route
// and zones), a free-fly view at street level, an orbit, the ride, and a
// view with fog off and the far plane extended beside the same view as the
// game draws it; and per camera the frame time (requestAnimationFrame,
// uncapped), draw calls and triangles (the viewer's counter) and the wasm
// memory's high-water mark. `--race` measures a race (autopilot) on the
// same build and machine for comparison (SPEC 6.6's budgets are the
// race's).
//
//   cargo xtask web --release
//   node tools/parity/viewer-shots.mjs [--levels coast,seaside] [--device desktop|iphone]
//       [--secs 4] [--race] [--no-shots] [--menu]
//
// Writes parity/report/viewer/<level>-<view>.png, viewer-<device>.json and
// index.html (all devices measured so far). The working tree is answered by
// request interception (the e2e harness's), so no server is needed; view
// the page through the registered server at /parity/report/viewer/.

import puppeteer from 'puppeteer-core';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { openGame, sleep, LEVEL_IDS } from './e2e/harness.mjs';
import { ROOT } from './lib/jstree.mjs';

const args = process.argv.slice(2);
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
const levels = opt('--levels', LEVEL_IDS.join(',')).split(',');
const device = opt('--device', 'desktop');
const secs = Number(opt('--secs', 4));
const shots = !args.includes('--no-shots');
const race = args.includes('--race');
const OUT = path.join(ROOT, 'parity/report/viewer');
await fs.mkdir(OUT, { recursive: true });

const browser = await puppeteer.launch({
  executablePath: process.env.CHROME_PATH || '/usr/bin/google-chrome',
  headless: 'new',
  args: [
    '--enable-unsafe-webgpu', '--enable-features=Vulkan', '--use-angle=vulkan', '--ignore-gpu-blocklist', '--mute-audio',
    '--autoplay-policy=document-user-activation-required',
    // Frame rate uncapped, as the baselines are measured.
    '--disable-gpu-vsync', '--disable-frame-rate-limit',
  ],
  protocolTimeout: 600000,
});

const vw = (g) => g.eval(() => window.__mr.viewer);
const set = async (g, o) => { await g.eval((x) => window.__mr.viewer.set(x), o); await g.frames(3); };
const settled = (g) => g.waitFor(() => !window.__mr.viewer.flying, { timeout: 15000, what: 'the flight' });

// Frame times over `ms` while `during` runs, and the counters at the end.
async function measure(g, ms, during = async () => {}) {
  const rec = g.eval((t) => new Promise((res) => {
    const d = []; let last = null; const t0 = performance.now();
    const f = (now) => { if (last !== null) d.push(now - last); last = now; if (now - t0 < t) requestAnimationFrame(f); else res(d); };
    requestAnimationFrame(f);
  }), ms);
  await during();
  const d = (await rec).sort((a, b) => a - b);
  const q = (p) => d[Math.min(d.length - 1, Math.floor(p * d.length))];
  const v = await g.eval(() => window.__mr.viewer || {});
  return {
    frames: d.length, p50: +q(0.5).toFixed(2), p95: +q(0.95).toFixed(2), max: +d[d.length - 1].toFixed(1),
    mean: +(d.reduce((a, b) => a + b, 0) / d.length).toFixed(2),
    draws: v.draws, tris: v.tris, shadowDraws: v.shadowDraws, shadowTris: v.shadowTris,
    wasmMB: Math.round(await g.eval(() => window.__mr.wasmMemoryBytes() / 1048576)),
  };
}

async function shot(g, name) {
  // The review pictures are the desktop's; a phone adds its own.
  if (!shots || (device !== 'desktop' && !name.startsWith('touch-'))) return;
  await g.frames(8);
  await g.shot(name, OUT);
}

const results = { device, load: os.loadavg().map((x) => +x.toFixed(1)), date: new Date().toISOString(), levels: {} };
for (const level of levels) {
  const r = (results.levels[level] = {});
  const t0 = Date.now();
  const g = await openGame(browser, { device, query: `view=god&level=${level}&panel=0`, downloads: OUT });
  try {
    await g.waitFor(() => window.__mr.viewer?.ready === true, { timeout: 120000, what: 'the viewer' });
    r.readyS = +((Date.now() - t0) / 1000).toFixed(1);
    await set(g, { count: true });
    await g.frames(30);
    const len = (await vw(g)).routeLength;
    const w = await g.eval('innerWidth'), h = await g.eval('innerHeight');
    // Free fly at street level, 30 % along the route, looking along it.
    await set(g, { mode: 'free', route: len * 0.3, speed: 30 });
    let v = await vw(g);
    await set(g, { cam: [v.pose.x, v.pose.y - 12.3, v.pose.z, v.pose.yaw, -0.03] });
    await shot(g, `${level}-street.png`);
    r.free = await measure(g, secs * 1000, async () => {
      await g.page.keyboard.down('KeyW'); await sleep(secs * 1000); await g.page.keyboard.up('KeyW');
    });
    // Orbit about the route 60 % along, circling while measured.
    await set(g, { mode: 'orbit', route: len * 0.6 });
    v = await vw(g);
    const c = v.orbit, yaw = v.pose.yaw + 0.9, pitch = -0.45, d = 140;
    const fx = -Math.sin(yaw) * Math.cos(pitch), fy = Math.sin(pitch), fz = -Math.cos(yaw) * Math.cos(pitch);
    await set(g, { cam: [c.x - fx * d, c.y - fy * d, c.z - fz * d, yaw, pitch], orbit: [c.x, c.y, c.z] });
    await shot(g, `${level}-orbit.png`);
    r.orbit = await measure(g, secs * 1000, async () => {
      await g.page.mouse.move(w * 0.7, h * 0.5);
      await g.page.mouse.down();
      const n = Math.round(secs * 30);
      for (let i = 0; i < n; i++) { await g.page.mouse.move(w * 0.7 - i * 2, h * 0.5); await sleep(33); }
      await g.page.mouse.up();
    });
    // The overview, with the panel open once for the picture.
    await set(g, { mode: 'over' });
    await settled(g);
    await shot(g, `${level}-over.png`);
    r.overview = await measure(g, secs * 1000);
    if (device === 'desktop') {
      await set(g, { folded: false });
      await shot(g, `${level}-panel.png`);
      await set(g, { folded: true });
    } else {
      // A phone: the panel open over the street view, and the touch
      // controls.
      await set(g, { mode: 'free', route: len * 0.3, folded: false });
      await shot(g, `touch-${device}-${level}.png`);
      await set(g, { folded: true });
    }
    // Fog off and the far plane out, beside the game's view: 120 m above
    // the road half way, looking along it.
    await set(g, { mode: 'free', route: len * 0.5 });
    v = await vw(g);
    await set(g, { cam: [v.pose.x, v.pose.y + 110, v.pose.z, v.pose.yaw, -0.12], fog: true, far: false });
    await shot(g, `${level}-far-game.png`);
    r.game = await measure(g, secs * 500);
    await set(g, { fog: false, far: true });
    await shot(g, `${level}-far-open.png`);
    r.far = await measure(g, secs * 1000);
    // The ride.
    await set(g, { mode: 'ride', ride: { s: len * 0.2, v: 40, h: 5, back: 14, lat: 0, yaw: 0, pitch: -0.08 } });
    await settled(g);
    await shot(g, `${level}-ride.png`);
    r.ride = await measure(g, secs * 1000);
    r.wasmPeakMB = Math.round(await g.eval(() => window.__mr.wasmMemoryBytes() / 1048576));
    r.errors = g.errors.filter((e) => !/Ignored attempt to cancel a touchstart/.test(e));
    console.log(level, JSON.stringify(r));
  } catch (e) {
    r.error = String(e.message || e);
    console.error(level, r.error);
  } finally { await g.close(); }
  if (race) {
    const rg = await openGame(browser, { device, query: `level=${level}&autostart=sports&autodrive=1` });
    try {
      await rg.waitFor(() => window.__mr.race?.state === 'racing', { timeout: 120000, what: 'the race' });
      await sleep(1500);
      r.race = await measure(rg, secs * 1000);
    } catch (e) {
      r.race = { error: String(e.message || e) };
    } finally { await rg.close(); }
  }
  await fs.writeFile(path.join(OUT, `viewer-${device}.json`), JSON.stringify(results, null, 1));
}
// The menu with its Level viewer button.
if (shots && args.includes('--menu')) {
  const m = await openGame(browser, { device, storage: { 'mr.level': levels[0] }, downloads: OUT });
  try { await m.frames(10); await m.shot(`menu-${device}.png`, OUT); } finally { await m.close(); }
}
await browser.close();

// The index page: every device measured so far.
const files = (await fs.readdir(OUT)).filter((f) => /^viewer-.*\.json$/.test(f));
const all = await Promise.all(files.map(async (f) => JSON.parse(await fs.readFile(path.join(OUT, f), 'utf8'))));
const have = new Set(await fs.readdir(OUT));
const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;');
const MODES = [['free', 'Free fly'], ['orbit', 'Orbit'], ['overview', 'Overview'], ['game', 'Game view (fog, 9 km)'], ['far', 'Fog off, far plane out'], ['ride', 'Ride'], ['race', 'Race (autopilot)']];
let html = `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Level viewer review</title><style>
:root { --bg: #f6f7f9; --fg: #15171c; --dim: #5b6170; --line: #d9dce3; --card: #fff; }
@media (prefers-color-scheme: dark) { :root { --bg: #0d0f14; --fg: #e8ebf2; --dim: #9aa1b2; --line: #2a2f3a; --card: #161a22; } }
body { margin: 0; padding: 16px; background: var(--bg); color: var(--fg); font: 15px/1.45 system-ui, sans-serif; }
h1 { font-size: 22px; margin: 0 0 4px; } h2 { font-size: 18px; margin: 28px 0 8px; } p { margin: 4px 0 10px; color: var(--dim); max-width: 60em; }
.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 380px), 1fr)); gap: 10px; }
figure { margin: 0; background: var(--card); border: 1px solid var(--line); border-radius: 8px; overflow: hidden; }
figure img { width: 100%; display: block; } figcaption { padding: 6px 10px; font-size: 13px; color: var(--dim); }
table { border-collapse: collapse; font-size: 13px; background: var(--card); } td, th { border: 1px solid var(--line); padding: 4px 8px; text-align: right; white-space: nowrap; }
th:first-child, td:first-child { text-align: left; } .wrap { overflow-x: auto; max-width: 100%; }
</style></head><body><h1>Level viewer: every level</h1>
<p>The level viewer (<code>?view=god</code>, SPEC 8.6) on the Rust web build, headless Chrome on the dev machine's GPU. Open one yourself: <code>dist/next/index.html?view=god&amp;level=coast</code>, or the menu's <b>Level viewer</b> button. Frame times are requestAnimationFrame intervals with the frame rate uncapped; the machine is shared, so compare modes within a run rather than with other days. Draws and triangles count the camera's view and the sun's shadow map (three's way: an object a draw).</p>`;
for (const res of all) {
  html += `<h2>Costs: ${esc(res.device)}</h2><p>${esc(res.date)}, load average ${esc(res.load.join(' / '))}.</p><div class="wrap"><table><tr><th>Level</th><th>Ready</th>`;
  for (const [, label] of MODES) html += `<th>${esc(label)}<br>ms p50 / p95 / max</th>`;
  html += `<th>Draws + shadow (free / orbit / overview / far)</th><th>M tris (free / orbit / overview / far)</th><th>Wasm MB, peak</th></tr>`;
  for (const [lv, r] of Object.entries(res.levels)) {
    html += `<tr><td>${esc(lv)}</td><td>${r.readyS ?? ''} s</td>`;
    for (const [k] of MODES) {
      const m = r[k];
      html += `<td>${m && m.p50 !== undefined ? `${m.p50} / ${m.p95} / ${m.max}` : m?.error ? 'error' : ''}</td>`;
    }
    const ds = ['free', 'orbit', 'overview', 'far'].map((k) => (r[k] ? `${r[k].draws}+${r[k].shadowDraws}` : '')).join(' / ');
    const ts = ['free', 'orbit', 'overview', 'far'].map((k) => (r[k] ? ((r[k].tris + r[k].shadowTris) / 1e6).toFixed(2) : '')).join(' / ');
    html += `<td>${ds}</td><td>${ts}</td><td>${r.wasmPeakMB ?? ''}${r.error ? ' (error: ' + esc(r.error) + ')' : ''}</td></tr>`;
  }
  html += '</table></div>';
}
const VIEWS = [['over', 'From high above (the overview: route in blue, zones in gold)'], ['street', 'Free fly at street level, 30 % along'], ['orbit', 'Orbit about the route, 60 % along'], ['far-game', 'Half way, 120 m up, as the game draws it (fog, 9 km far plane)'], ['far-open', 'The same with fog off and the far plane extended'], ['ride', 'Ride (the fly camera), 20 % along'], ['panel', 'The panel open over the overview']];
for (const lv of LEVEL_IDS) {
  const imgs = VIEWS.filter(([k]) => have.has(`${lv}-${k}.png`));
  if (!imgs.length) continue;
  html += `<h2>${esc(lv)}</h2><div class="grid">`;
  for (const [k, cap] of imgs) html += `<figure><a href="${lv}-${k}.png"><img loading="lazy" src="${lv}-${k}.png" alt="${esc(lv)}: ${esc(cap)}"></a><figcaption>${esc(cap)}</figcaption></figure>`;
  html += '</div>';
}
const extra = [...have].filter((f) => /^(touch|menu)-.*\.png$/.test(f)).sort();
if (extra.length) {
  html += `<h2>Touch and the menu's button</h2><div class="grid">`;
  for (const f of extra) html += `<figure><a href="${f}"><img loading="lazy" src="${f}" alt="${esc(f)}"></a><figcaption>${esc(f.replace(/\.png$/, ''))}</figcaption></figure>`;
  html += '</div>';
}
html += '</body></html>\n';
await fs.writeFile(path.join(OUT, 'index.html'), html);
console.log('wrote', path.join(OUT, 'index.html'));
