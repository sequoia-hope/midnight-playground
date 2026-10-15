// Runs the loop's delivery to its results in the web client
// (docs/vision/peninsula.md, stage 5): the car put on the garage's marker,
// the hold, the countdown, the drop's beacon, then the car put at the drop,
// the results ("Keep cruising"), and the cruise again after Enter.
//
//   node tools/parity/event-finish.mjs [--backend webgl2] [--phone] [--out <dir>]
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
// The store run's start (metres along the loop) and its drop (x, z), as
// mp_levels::peninsula's events() places them.
const START = Number(opt('--start', '8311'));
const DROP = opt('--drop', '2082.07,7879.36').split(',').map(Number);

const browser = await puppeteer.launch({ executablePath: '/usr/bin/google-chrome', headless: 'new', args: chromeFlags({ backend, race: true, angle: 'gl' }) });
const page = await browser.newPage();
const seen = new Map();
page.on('console', (m) => { const t = m.text(); const k = t.slice(0, 120); if (!/still waiting/.test(t) && (seen.get(k) ?? 0) < 3) { seen.set(k, (seen.get(k) ?? 0) + 1); console.log(`[${m.type()}] ${t.slice(0, 2000)}`); } });
page.on('pageerror', (e) => console.log(`[pageerror] ${e}`));
if (args.includes('--phone')) await page.setViewport({ width: 844, height: 390, deviceScaleFactor: 3, isMobile: true, hasTouch: true, isLandscape: true });
else await page.setViewport({ width: 1280, height: 800 });
const q = new URLSearchParams({ level: 'peninsula', autostart: 'sports', seed: '1', pursuit: '0', hq: '0', cruise: '1' });
const url = `${base}dist/${opt('--dist', 'next')}/index.html?${q}`;
console.log(url);
await page.goto(url);
await page.waitForFunction('(window.__mp?.race?.time ?? -1) >= 0.5', { timeout: 180000, polling: 200 });
const read = () => page.evaluate(() => { const r = window.__mp?.race; return r ? { s: r.s, speed: r.speed, mode: r.mode, time: r.time, road: r.road, vx: r.vx, vz: r.vz, hold: r.hold, event: r.event, thr: r.input?.throttle, brk: r.input?.brake } : null; });
const shot = async (name) => { await page.screenshot({ path: path.join(out, `${name}.png`) }); console.log(name, JSON.stringify(await read())); };
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
const stage = (c) => page.evaluate((c) => window.__mp.stage(c), c);

await wait(1500);
// Long enough for the town to stream in: a busy page ticks the race
// slower than the clock, and the hold counts ticks.
await stage({ cmd: 'reset', s: START, lat: 2 });
await wait(6000);
await shot('d1-at-the-garage');
await page.keyboard.down('ArrowUp');
await page.keyboard.down('ArrowDown');
await wait(600);
console.log('holding', JSON.stringify(await read()));
await wait(1200);
await page.keyboard.up('ArrowUp');
await page.keyboard.up('ArrowDown');
await wait(300);
await shot('d2a-event-card');
await wait(4700);
await shot('d2-delivering');
await page.keyboard.down('ArrowUp');
await wait(4000);
await page.keyboard.up('ArrowUp');
await shot('d2b-under-way');
for (const [p, v] of [['player.x', DROP[0]], ['player.z', DROP[1]], ['player.vx', 0], ['player.vz', 0]]) await stage({ cmd: 'set', path: p, value: v });
await wait(6000);
await shot('d3-results');
await page.keyboard.press('Enter');
await wait(3000);
await shot('d4-cruising-again');
await browser.close();
