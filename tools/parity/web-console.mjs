// The Rust client's page console, for a build that does not come up: every
// message (but the warm-up's "still waiting" lines) for `--secs`, then the
// page's state (`__mp.ready`, backend, state). Through the registered dev
// server; `--backend webgl2` turns WebGPU off.
//
//   node tools/parity/web-console.mjs [--backend webgpu|webgl2] [--dist next] [--secs 60]
import puppeteer from 'puppeteer-core';
import { baseUrl, chromeFlags } from './lib/server.mjs';
const args = process.argv.slice(2);
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
const backend = opt('--backend', 'webgl2');
const base = baseUrl(opt);
const browser = await puppeteer.launch({ executablePath: '/usr/bin/google-chrome', headless: 'new', args: chromeFlags({ backend, angle: 'gl' }) });
const skip = /still waiting/;
const page = await browser.newPage();
const seen = new Map();
page.on('console', (m) => { const t = m.text(); const k = t.slice(0, 200); if (!seen.has(k)) { seen.set(k, 1); if (!skip.test(t)) console.log(`[${m.type()}] ${t.slice(0, 1500)}`); } });
page.on('pageerror', (e) => console.log(`[pageerror] ${e}`));
await page.setViewport({ width: 1280, height: 800 });
await page.goto(`${base}dist/${opt('--dist', 'next')}/index.html?level=sierra&s=300&h=2.2&back=7&freeze=1&t=0.3&hq=1`);
await new Promise((r) => setTimeout(r, Number(opt('--secs', 60)) * 1000));
console.log('state:', await page.evaluate(() => JSON.stringify({ ready: window.__mp?.ready, backend: window.__mp?.backend, state: window.__mp?.state })));
await browser.close();
