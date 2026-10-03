// Desktop performance baseline of the JS game (roadmap WP 0.8): the debug
// fly camera along each level's route at a fixed speed, frame rate
// uncapped, high quality, read from the stats overlay's numbers
// (window.__stats, ?stats=1). The phones' numbers are read by the owner from
// the same overlay (docs/rust-port/BASELINE.md says how); this gives the
// desktop row and the method.
//
//   node tools/parity/perf-baseline.mjs [--speed 60] [--only sierra,...]
//
// Prints a markdown table for BASELINE.md. Run it with the machine otherwise
// idle: other work on the GPU or CPU changes the numbers.

import puppeteer from 'puppeteer-core';
import { openGame } from '../../test/e2e/harness.js';

const args = process.argv.slice(2);
const SPEED = args.includes('--speed') ? Number(args[args.indexOf('--speed') + 1]) : 60;
const ONLY = args.includes('--only') ? args[args.indexOf('--only') + 1].split(',') : null;
const LEVELS = ['sierra', 'coast', 'streets', 'desert', 'seaside', 'cruise'].filter((l) => !ONLY || ONLY.includes(l));
const MAX_SECONDS = 240;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const browser = await puppeteer.launch({
  executablePath: process.env.CHROME_PATH || '/usr/bin/google-chrome',
  headless: 'new',
  // As the e2e harness, plus no vsync and no frame cap, so frame times mean
  // something (headless is otherwise held at 60 fps).
  args: ['--use-angle=vulkan', '--enable-gpu', '--ignore-gpu-blocklist', '--disable-gpu-vsync', '--disable-frame-rate-limit', '--enable-precise-memory-info'],
});
const rows = [];
try {
  for (const level of LEVELS) {
    // Start a little past the line, on the route's own time of day.
    const g = await openGame(browser, { query: `level=${level}&stats=1&s=80&v=${SPEED}&h=5&back=14` });
    const load = await g.eval(() => window.__stats.loadS ?? null);
    const len = await g.eval(() => (window.__world.track.loop ? window.__world.track.length : window.__world.track.roadEnd - 100));
    const seconds = Math.min(MAX_SECONDS, len / SPEED);
    const samples = [];
    const t0 = Date.now();
    await sleep(1500);
    while ((Date.now() - t0) / 1000 < seconds) {
      await sleep(500);
      samples.push(await g.eval(() => window.__stats));
    }
    const fps = samples.map((s) => s.fps).sort((a, b) => a - b);
    const med = fps[Math.floor(fps.length / 2)], p5 = fps[Math.floor(fps.length * 0.05)];
    const row = {
      level, seconds: Math.round(seconds), load: (await g.eval(() => window.__stats.loadS)) ?? load,
      fpsMedian: med, fps5: p5, worstMs: Math.max(...samples.map((s) => s.worstMs)),
      cpuMs: +(samples.reduce((a, s) => a + s.cpuMs, 0) / samples.length).toFixed(2),
      calls: Math.max(...samples.map((s) => s.calls)), tris: Math.max(...samples.map((s) => s.tris)),
      heapMB: Math.max(...samples.map((s) => s.heapMB ?? 0)),
    };
    rows.push(row);
    if (g.errors.length) console.error(`${level}: page errors: ${g.errors.join(' | ')}`);
    console.error(JSON.stringify(row));
    await g.close();
  }
} finally {
  await browser.close();
}
console.log(`| Level | Flown | Load to menu | fps median | fps 5th pct | Worst frame | CPU per frame | Peak calls | Peak tris | Peak heap |`);
console.log(`|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|`);
for (const r of rows) console.log(`| ${r.level} | ${r.seconds} s | ${r.load} s | ${r.fpsMedian} | ${r.fps5} | ${r.worstMs} ms | ${r.cpuMs} ms | ${r.calls} | ${(r.tris / 1e6).toFixed(2)} M | ${r.heapMB} MB |`);
