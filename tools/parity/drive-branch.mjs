// Drives the Rust client's car along a corridor polyline with the keyboard
// (the open world, docs/vision/peninsula.md): from the race start on
// southcoast, round, down Highway 1 to the junction and up Highway 84,
// watching the page's console and tick counter for a hang.
//
//   node tools/parity/drive-branch.mjs [--backend webgl2] [--secs 150]
//        [--query "physics=sim"] [--out <dir>]
import puppeteer from 'puppeteer-core';
import fs from 'node:fs';
import path from 'node:path';
import { ROOT } from './lib/jstree.mjs';
import { baseUrl, chromeFlags } from './lib/server.mjs';

const args = process.argv.slice(2);
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
const backend = opt('--backend', 'webgl2');
const secs = Number(opt('--secs', 150));
const out = opt('--out', path.join(ROOT, 'parity/report/drive'));
fs.mkdirSync(out, { recursive: true });
const base = baseUrl(opt);

function corridor(id) {
  const txt = fs.readFileSync(path.join(ROOT, `assets/atlas/peninsula/corridors/${id}.txt`), 'utf8');
  return txt.split('\n').filter((l) => l && !l.startsWith('#')).map((l) => l.split(/\s+/).slice(0, 2).map(Number));
}
const hwy1 = corridor('coast-south');
const hwy84 = corridor('hwy-84-lower');

const browser = await puppeteer.launch({ executablePath: '/usr/bin/google-chrome', headless: 'new', args: chromeFlags({ backend, race: true, angle: 'gl' }) });
const page = await browser.newPage();
const seen = new Map();
page.on('console', (m) => { const t = m.text(); const k = t.slice(0, 120); if (!/still waiting/.test(t) && (seen.get(k) ?? 0) < 3) { seen.set(k, (seen.get(k) ?? 0) + 1); console.log(`[${m.type()}] ${t.slice(0, 2000)}`); } });
page.on('pageerror', (e) => console.log(`[pageerror] ${e}`));
if (args.includes('--phone')) await page.setViewport({ width: 844, height: 390, deviceScaleFactor: 3, isMobile: true, hasTouch: true, isLandscape: true });
else await page.setViewport({ width: 1280, height: 800 });
const q = new URLSearchParams({ level: 'southcoast', autostart: 'sports', seed: '1', pursuit: '0', hq: '0' });
for (const [k, v] of new URLSearchParams(opt('--query', ''))) q.set(k, v);
const url = `${base}dist/${opt('--dist', 'next')}/index.html?${q}`;
console.log(url);
await page.goto(url);
await page.waitForFunction('(window.__mp?.race?.time ?? -1) >= 0.5', { timeout: 180000, polling: 200 });
console.log('racing');
await page.evaluate(() => { window.__dts = []; let last = null; const f = (now) => { requestAnimationFrame(f); if (last !== null) window.__dts.push(now - last); last = now; }; requestAnimationFrame(f); });
const frames = () => page.evaluate(() => { const d = window.__dts; window.__dts = []; if (!d.length) return 'no frames'; d.sort((a, b) => a - b); const mean = d.reduce((a, b) => a + b, 0) / d.length; return `${d.length} frames mean ${mean.toFixed(1)} p95 ${d[Math.floor(d.length * 0.95)].toFixed(1)} max ${d[d.length - 1].toFixed(1)} ms`; });

const read = () => page.evaluate(() => { const r = window.__mp?.race; return r ? { x: r.x, z: r.z, yaw: r.yaw, s: r.s, lat: r.lat, speed: r.speed, tick: r.tick, mode: r.mode, hw: r.hw } : null; });
const withTimeout = (p, ms) => Promise.race([p, new Promise((_, rej) => setTimeout(() => rej(new Error('timeout')), ms))]);

// The route: Highway 1 from the start back to the junction, then 84.
let st0 = await read();
let k0 = 0, best = 1e18;
hwy1.forEach(([x, z], i) => { const d = (x - st0.x) ** 2 + (z - st0.z) ** 2; if (d < best) { best = d; k0 = i; } });
const route = hwy1.slice(0, k0 + 1).reverse().concat(hwy84.slice(1));
let at = 0;
const held = new Set();
const key = async (name, on) => { if (on && !held.has(name)) { held.add(name); await page.keyboard.down(name); } if (!on && held.has(name)) { held.delete(name); await page.keyboard.up(name); } };

let lastTick = -1, lastTickAt = Date.now(), last = null, shots = 0;
const t0 = Date.now();
while (Date.now() - t0 < secs * 1000) {
  let s;
  try { s = await withTimeout(read(), 5000); } catch (e) {
    console.log(`page unresponsive (${e.message}); last state ${JSON.stringify(last)}`);
    break;
  }
  if (!s) { await new Promise((r) => setTimeout(r, 100)); continue; }
  if (s.tick !== lastTick) { lastTick = s.tick; lastTickAt = Date.now(); } else if (Date.now() - lastTickAt > 3000) {
    console.log(`ticks stopped at ${s.tick}; state ${JSON.stringify(s)}`); break;
  }
  // Nearest route point near the last one, then the aim 25 m on.
  let bi = at, bd = 1e18;
  for (let i = Math.max(0, at - 5); i < Math.min(route.length, at + 40); i++) { const d = (route[i][0] - s.x) ** 2 + (route[i][1] - s.z) ** 2; if (d < bd) { bd = d; bi = i; } }
  at = bi;
  const aim = route[Math.min(route.length - 1, at + 5)];
  let alpha = Math.atan2(aim[1] - s.z, aim[0] - s.x) - s.yaw;
  alpha = Math.atan2(Math.sin(alpha), Math.cos(alpha));
  const want = at < k0 - 60 ? 18 : 12;
  await key('ArrowRight', alpha > 0.04);
  await key('ArrowLeft', alpha < -0.04);
  await key('ArrowUp', s.speed < want);
  await key('ArrowDown', s.speed > want + 3);
  if (!last || Math.floor((Date.now() - t0) / 5000) !== Math.floor((last.at - t0) / 5000)) {
    console.log(`t ${((Date.now() - t0) / 1000).toFixed(0)}s route ${at}/${route.length} s ${s.s.toFixed(0)} lat ${s.lat.toFixed(1)} speed ${s.speed.toFixed(1)} alpha ${alpha.toFixed(2)} tick ${s.tick}; ${await frames()}`);
    if (shots < 40) { try { await withTimeout(page.screenshot({ path: path.join(out, `drive-${String(shots++).padStart(2, '0')}.png`) }), 5000); } catch { console.log('screenshot timed out'); } }
  }
  last = { ...s, at: Date.now() };
  if (at >= route.length - 2) { console.log('end of the route'); break; }
  await new Promise((r) => setTimeout(r, 30));
}
console.log('final', JSON.stringify(last));
try { await withTimeout(browser.close(), 5000); } catch { process.exit(0); }
