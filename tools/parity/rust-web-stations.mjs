// The screenshot stations (DECISIONS D17) from the Rust client's web build
// at the JS shots' 1280 × 800 (WP 3.9, D496): one page in headless Chrome on
// WebGPU (rust-web.mjs's flags and request interception: no server, no
// port), the scenery frozen, then for each station of the JS run the fly
// camera moved there (`__mr.flyTo`), three settled frames waited for
// (`__mr.flyQuiet`: no pipeline compiling, the environment map built) and
// `__mr.screenshot` saved. The client builds every level itself by default
// (D678), with no scene download, so any level fits through the
// interception (D106's 100 MB limit). `--query world=export` draws the
// exported scene instead; an export over that limit needs `--server`, which
// loads the page from the registered dev server (`--port`, else `$PORT`,
// else `proj port`; no default port), as rust-perf.mjs does.
//
//   cargo xtask web --release && node tools/parity/rust-web-stations.mjs \
//       --level sierra [--only 06250:10000 | --only name,name] [--out <dir>] \
//       [--query "world=export"] [--backend webgl2] [--server [--port N]]
//
// Writes <out>/<level>/<station>.png (default parity/cache/<key>/shots/
// rust-web/), the layout `cargo xtask parity shots --a <dir> --b <js run>`
// compares.

import puppeteer from 'puppeteer-core';
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { ROOT, jsTreeKey } from './lib/jstree.mjs';

const args = process.argv.slice(2);
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
const level = opt('--level', 'sierra');
const jsRun = opt('--js-run', 'a');
const query = opt('--query', '');
const server = args.includes('--server');
const only = opt('--only', null);
const backend = opt('--backend', 'webgpu');
const timeoutMs = Number(opt('--timeout', 300000));
const key = await jsTreeKey();
const out = path.resolve(opt('--out', path.join(ROOT, 'parity/cache', key, 'shots/rust-web')), level);
const stationsFile = path.join(ROOT, 'parity/cache', key, 'shots', jsRun, level, 'stations.json');
if (!fs.existsSync(stationsFile)) {
  console.error(`no ${stationsFile}: run node tools/parity/shots.mjs --only ${level} first`);
  process.exit(1);
}
let stations = JSON.parse(fs.readFileSync(stationsFile, 'utf8')).stations;
if (only) {
  if (only.includes(':')) {
    const [a, b] = only.split(':').map(Number);
    stations = stations.filter((s) => /^\d{5}/.test(s.name) && +s.name.slice(0, 5) >= a && +s.name.slice(0, 5) <= b);
  } else {
    const names = only.split(',');
    stations = stations.filter((s) => names.includes(s.name));
  }
}

// The registered server (as rust-perf.mjs): it serves the main checkout,
// and a worktree sits below it.
function serverBase() {
  let port = opt('--port', null) || process.env.PORT;
  if (!port) port = execFileSync('proj', ['port'], { cwd: ROOT, encoding: 'utf8' }).trim();
  if (!/^\d+$/.test(port)) throw new Error(`no server port (gave "${port}")`);
  const common = path.resolve(ROOT, execFileSync('git', ['rev-parse', '--git-common-dir'], { cwd: ROOT, encoding: 'utf8' }).trim());
  const rel = path.relative(path.dirname(common), ROOT);
  if (rel.startsWith('..')) throw new Error(`${ROOT} is not below the served checkout`);
  return `http://127.0.0.1:${port}${rel ? '/' + rel.split(path.sep).map(encodeURIComponent).join('/') : ''}`;
}

const ORIGIN = server ? serverBase() : 'https://midnight-racer.test';
const TYPES = {
  '.html': 'text/html', '.js': 'text/javascript', '.json': 'application/json', '.wasm': 'application/wasm',
  '.png': 'image/png', '.bin': 'application/octet-stream', '.mrscene': 'application/octet-stream',
};

fs.mkdirSync(out, { recursive: true });
const browser = await puppeteer.launch({
  executablePath: process.env.CHROME_PATH || '/usr/bin/google-chrome',
  headless: 'new',
  args: backend === 'webgl2'
    ? ['--disable-blink-features=WebGPU', '--disable-features=WebGPU', '--use-angle=vulkan', '--enable-gpu', '--ignore-gpu-blocklist', '--mute-audio']
    : ['--enable-unsafe-webgpu', '--enable-features=Vulkan', '--use-angle=vulkan', '--ignore-gpu-blocklist', '--mute-audio'],
  dumpio: !!process.env.MR_DUMPIO,
});
const errors = [];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
try {
  const page = await browser.newPage();
  await page.setViewport({ width: 1280, height: 800, deviceScaleFactor: 1 });
  const cdp = await page.createCDPSession();
  await cdp.send('Browser.setDownloadBehavior', { behavior: 'allow', downloadPath: out });
  page.on('console', (m) => {
    const t = m.text();
    if (m.type() === 'error' && !/Failed to load resource/.test(t)) errors.push(t);
    if (process.env.MR_VERBOSE) console.log(`[page ${m.type()}] ${t}`);
  });
  page.on('pageerror', (e) => errors.push(String(e)));
  if (!server) await page.setRequestInterception(true);
  if (!server) page.on('request', (req) => {
    const u = new URL(req.url());
    if (u.origin !== ORIGIN) return req.continue();
    let p = path.join(ROOT, decodeURIComponent(u.pathname));
    if (p.endsWith('/')) p = path.join(p, 'index.html');
    if (!fs.existsSync(p)) return req.respond({ status: 404, body: 'not found' });
    req.respond({ status: 200, contentType: TYPES[path.extname(p)] || 'application/octet-stream', body: fs.readFileSync(p) });
  });
  const first = stations[0];
  const t0 = Date.now();
  await page.goto(`${ORIGIN}/dist/next/index.html?level=${level}&freeze=1&s=${first.s}&h=${first.h}&back=${first.back}&lat=${first.lat}&yaw=${first.yaw}&pitch=${first.pitch}&${query}`);
  for (;;) {
    const s = await page.evaluate(() => ({ state: window.__mr?.state, ready: window.__mr?.ready, error: window.__mr?.error }));
    if (s.error || s.state === 'failed') throw new Error(`client failed: ${s.error}`);
    if (s.ready) break;
    if (Date.now() - t0 > timeoutMs) throw new Error(`not ready after ${timeoutMs} ms (state ${s.state})`);
    await sleep(500);
  }
  console.log(`${level}: ready in ${((Date.now() - t0) / 1000).toFixed(1)} s; ${stations.length} stations`);
  for (const st of stations) {
    await page.evaluate((p) => window.__mr.flyTo(p), st);
    const ts = Date.now();
    for (;;) {
      const q = await page.evaluate(() => window.__mr.flyQuiet || 0);
      if (q >= 3) break;
      if (Date.now() - ts > 60000) throw new Error(`${st.name}: not settled`);
      await sleep(50);
    }
    const name = `${st.name}.png`;
    const file = path.join(out, name);
    fs.rmSync(file, { force: true });
    await page.evaluate((n) => window.__mr.screenshot(n), name);
    for (let i = 0; i < 200 && !fs.existsSync(file); i++) await sleep(50);
    // The download is written in place; wait for its size to hold.
    let size = -1;
    for (let i = 0; i < 40; i++) {
      const n = fs.existsSync(file) ? fs.statSync(file).size : -1;
      if (n > 0 && n === size) break;
      size = n;
      await sleep(50);
    }
    if (!fs.existsSync(file)) errors.push(`${st.name}: no screenshot`);
    else process.stdout.write(`  ${st.name}\n`);
  }
  const backendSeen = await page.evaluate(() => window.__mr.backend);
  if (backendSeen !== backend) errors.push(`the page picked ${backendSeen}, expected ${backend}`);
} catch (e) {
  errors.push(String(e.message || e));
} finally {
  await browser.close();
}
if (errors.length) {
  console.error(`errors:\n  ${errors.join('\n  ')}`);
  process.exit(1);
}
console.log(`written to ${out}`);
