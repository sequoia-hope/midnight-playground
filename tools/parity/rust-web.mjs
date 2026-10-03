// Screenshots of the Rust client's web build (roadmap WP 2.5): headless
// Chrome on the GPU with WebGPU (DECISIONS D106's flags), the page and its
// files answered from the working tree by request interception (no server,
// no port, `.wasm` as application/wasm), `window.__mr.ready` waited for,
// then `__mr.screenshot(name)` (a download: headless Chrome does not
// composite a WebGPU canvas into its own screenshots).
//
//   cargo xtask web && node tools/parity/rust-web.mjs --level seaside \
//       [--query "s=300&h=2.2&back=7&pitch=-0.04&freeze=1"] [--out <dir>] [--name shot]
//
// Prints the page's errors (a WGSL error Chrome's compiler finds, a failed
// scene) and fails on any. Scenes over about 100 MB do not pass through the
// interception (D106); use the native client or the registered server for
// those.

import puppeteer from 'puppeteer-core';
import fs from 'node:fs';
import path from 'node:path';
import { ROOT } from './lib/jstree.mjs';

const args = process.argv.slice(2);
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
const level = opt('--level', 'seaside');
const query = opt('--query', 's=300&h=2.2&back=7&pitch=-0.04&freeze=1');
const out = path.resolve(opt('--out', path.join(ROOT, 'parity/report/rust-web')));
const name = opt('--name', `${level}.png`);
const timeoutMs = Number(opt('--timeout', 240000));

const ORIGIN = 'https://midnight-racer.test';
const TYPES = {
  '.html': 'text/html', '.js': 'text/javascript', '.json': 'application/json', '.wasm': 'application/wasm',
  '.png': 'image/png', '.bin': 'application/octet-stream', '.mrscene': 'application/octet-stream',
};

fs.mkdirSync(out, { recursive: true });
const browser = await puppeteer.launch({
  executablePath: process.env.CHROME_PATH || '/usr/bin/google-chrome',
  headless: 'new',
  args: ['--enable-unsafe-webgpu', '--enable-features=Vulkan', '--use-angle=vulkan', '--ignore-gpu-blocklist'],
  dumpio: !!process.env.MR_DUMPIO,
});
const errors = [];
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
  await page.setRequestInterception(true);
  page.on('request', (req) => {
    const u = new URL(req.url());
    if (u.origin !== ORIGIN) return req.continue();
    let p = path.join(ROOT, decodeURIComponent(u.pathname));
    if (p.endsWith('/')) p = path.join(p, 'index.html');
    if (!fs.existsSync(p)) { if (process.env.MR_VERBOSE) console.log('404', u.pathname); return req.respond({ status: 404, body: 'not found' }); }
    req.respond({ status: 200, contentType: TYPES[path.extname(p)] || 'application/octet-stream', body: fs.readFileSync(p) });
  });
  const t0 = Date.now();
  await page.goto(`${ORIGIN}/dist/next/index.html?level=${level}&${query}`);
  for (;;) {
    const s = await page.evaluate(() => ({ state: window.__mr?.state, ready: window.__mr?.ready, error: window.__mr?.error }));
    if (s.error || s.state === 'failed') throw new Error(`client failed: ${s.error}`);
    if (s.ready) break;
    if (Date.now() - t0 > timeoutMs) throw new Error(`not ready after ${timeoutMs} ms (state ${s.state})`);
    await new Promise((r) => setTimeout(r, 500));
  }
  // A few frames for the environment map and the shadow map.
  await new Promise((r) => setTimeout(r, 1500));
  const file = path.join(out, name);
  fs.rmSync(file, { force: true });
  await page.evaluate((n) => window.__mr.screenshot(n), name);
  for (let i = 0; i < 100 && !fs.existsSync(file); i++) await new Promise((r) => setTimeout(r, 100));
  const info = await page.evaluate(() => ({ readyMs: window.__mr.readyMs, frames: window.__mr.frames, counts: window.__mr.counts }));
  console.log(`${level}: ${fs.existsSync(file) ? file : 'no screenshot'}; ready at ${Math.round(info.readyMs)} ms, ${info.frames} frames`);
} catch (e) {
  errors.push(String(e.message || e));
} finally {
  await browser.close();
}
if (errors.length) {
  console.error(`errors:\n  ${errors.join('\n  ')}`);
  process.exit(1);
}
