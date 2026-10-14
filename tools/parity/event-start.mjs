// Starts an event at a place from the loop's cruise in the web client
// (docs/vision/peninsula.md, stage 5) and takes pictures on the way: the
// cruise on the first race's start (its beacon, its card, how to begin
// it), the hold, the event's countdown with the rivals on the grid, and
// the race under way.
//
//   node tools/parity/event-start.mjs [--backend webgl2] [--phone] [--out <dir>]
//        [--query "touch=1&assist=2"]
import puppeteer from 'puppeteer-core';
import fs from 'node:fs';
import path from 'node:path';
import { ROOT } from './lib/jstree.mjs';
import { baseUrl, chromeFlags } from './lib/server.mjs';

const args = process.argv.slice(2);
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
const backend = opt('--backend', 'webgl2');
const out = opt('--out', path.join(ROOT, 'parity/report/event'));
fs.mkdirSync(out, { recursive: true });
const base = baseUrl(opt);

const browser = await puppeteer.launch({ executablePath: '/usr/bin/google-chrome', headless: 'new', args: chromeFlags({ backend, race: true, angle: 'gl' }) });
const page = await browser.newPage();
const seen = new Map();
page.on('console', (m) => { const t = m.text(); const k = t.slice(0, 120); if (!/still waiting/.test(t) && (seen.get(k) ?? 0) < 3) { seen.set(k, (seen.get(k) ?? 0) + 1); console.log(`[${m.type()}] ${t.slice(0, 2000)}`); } });
page.on('pageerror', (e) => console.log(`[pageerror] ${e}`));
if (args.includes('--phone')) await page.setViewport({ width: 844, height: 390, deviceScaleFactor: 3, isMobile: true, hasTouch: true, isLandscape: true });
else await page.setViewport({ width: 1280, height: 800 });
const q = new URLSearchParams({ level: 'peninsula', autostart: 'sports', seed: '1', pursuit: '0', hq: '0', cruise: '1' });
for (const [k, v] of new URLSearchParams(opt('--query', ''))) q.set(k, v);
const url = `${base}dist/${opt('--dist', 'next')}/index.html?${q}`;
console.log(url);
await page.goto(url);
await page.waitForFunction('(window.__mp?.race?.time ?? -1) >= 0.5', { timeout: 180000, polling: 200 });
const read = () => page.evaluate(() => { const r = window.__mp?.race; return r ? { s: r.s, speed: r.speed, tick: r.tick, mode: r.mode, time: r.time } : null; });
const shot = async (name) => { await page.screenshot({ path: path.join(out, `${name}.png`) }); console.log(name, JSON.stringify(await read())); };
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

await wait(1500);
await shot('1-on-the-start');
await page.keyboard.down('ArrowUp');
await page.keyboard.down('ArrowDown');
await wait(600);
await shot('2-holding');
await wait(1200);
await page.keyboard.up('ArrowUp');
await page.keyboard.up('ArrowDown');
await wait(800);
await shot('3-countdown');
await wait(3500);
await page.keyboard.down('ArrowUp');
await wait(4000);
await shot('4-racing');
await page.keyboard.up('ArrowUp');
await browser.close();
