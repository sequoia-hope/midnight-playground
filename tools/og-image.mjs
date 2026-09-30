// Renders og-image.png, the picture link previews show (Slack, iMessage,
// Discord and so on read it from the og:image tag in index.html):
// 1200 × 630, a Hot Pursuit on Interstate 9 at night with the logo.
//
//   node tools/og-image.mjs [--out og-image.png] [--scale 1]
//
// Like the browser tests, it serves the working tree to headless Chrome by
// request interception, so it needs no server and no port. Only the web
// font is fetched from outside, so the logo is set in the real typeface.

import puppeteer from 'puppeteer-core';
import fs from 'node:fs/promises';
import path from 'node:path';
import { parseArgs } from 'node:util';
import { ROOT } from '../test/e2e/harness.js';

const { values: opt } = parseArgs({ options: {
  out: { type: 'string', default: path.join(ROOT, 'og-image.png') },
  scale: { type: 'string', default: '1' },
} });
const ORIGIN = 'https://midnight-racer.test';
const FONTS = /^https:\/\/fonts\.(googleapis|gstatic)\.com\//;
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.json': 'application/json', '.webmanifest': 'application/manifest+json', '.png': 'image/png', '.svg': 'image/svg+xml' };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const browser = await puppeteer.launch({
  executablePath: process.env.CHROME_PATH || '/usr/bin/google-chrome',
  headless: 'new',
  args: ['--use-angle=vulkan', '--enable-gpu', '--ignore-gpu-blocklist'],
});
try {
  const page = await browser.newPage();
  await page.setViewport({ width: 1200, height: 630, deviceScaleFactor: Number(opt.scale) });
  await page.setRequestInterception(true);
  page.on('request', async (req) => {
    const u = new URL(req.url());
    if (FONTS.test(req.url())) return req.continue();
    if (u.origin !== ORIGIN) return req.abort('blockedbyclient');
    const rel = decodeURIComponent(u.pathname).replace(/^\/+/, '') || 'index.html';
    const file = path.resolve(ROOT, rel);
    if (!file.startsWith(ROOT + path.sep)) return req.respond({ status: 403, body: '' });
    try {
      await req.respond({ status: 200, contentType: TYPES[path.extname(file)] || 'application/octet-stream', body: await fs.readFile(file) });
    } catch { await req.respond({ status: 404, body: '' }); }
  });
  page.on('pageerror', (e) => console.error('page error:', e.message));

  await page.goto(`${ORIGIN}/?level=sierra&autostart=super&pursuit=1&heat=3&cops=3`);
  await page.waitForFunction(() => window.__race?.state === 'racing', { timeout: 90000 });

  // The scene: the player flat out down the freeway with two units on its
  // tail and a rival ahead, the police lights on.
  await page.evaluate(() => {
    const r = window.__race, p = window.__pursuit, t = r.track;
    const s = t.zones[2].s0 + 900;
    r.phys.reset(s, -1.8);
    const f = t.frame(s);
    r.player.vx = f.fx * 42; r.player.vz = f.fz * 42;
    p.state = 'pursuit'; p.propT = 999;
    const [a, b] = p.units;
    p.activate(a, s - 11, -3.6, 41, 'chase'); a.target = r.playerBody;
    p.activate(b, s - 19, 1.4, 41, 'chase'); b.target = r.playerBody;
    r.ais.forEach((ai, i) => { ai.s = s + 38 + i * 30; ai.lat = i % 2 ? 2.4 : -1; ai.speed = 40; ai.writePos(); });
    // Low, ahead and to one side of the car, looking back past it.
    r.cam.update = function () {
      const v = r.player, fx = Math.cos(v.yaw), fz = Math.sin(v.yaw), rx = -fz, rz = fx;
      this.camera.position.set(v.x + fx * 4.6 + rx * 4.4, v.y + 1.05, v.z + fz * 4.6 + rz * 4.4);
      this.camera.lookAt(v.x - fx * 5 - rx * 0.6, v.y + 0.8, v.z - fz * 5 - rz * 0.6);
      this.camera.fov = 50;
      this.camera.updateProjectionMatrix();
    };
  });
  await sleep(1400); // the units close in and the lights get going

  // Freeze a moment, swap the HUD for the logo, and shoot.
  await page.evaluate(() => {
    document.getElementById('hud').classList.add('hidden');
    document.getElementById('touch').classList.add('hidden');
    const logo = Object.assign(document.createElement('div'), { className: 'logo' });
    logo.innerHTML = 'MIDNIGHT<span>RACER</span>';
    logo.style.cssText = 'position:fixed;left:54px;top:40px;z-index:20;font-size:96px;text-align:left;filter:drop-shadow(0 4px 18px rgba(0,0,0,.7))';
    const tag = Object.assign(document.createElement('div'), { textContent: 'Street racing in your browser' });
    tag.style.cssText = 'position:fixed;left:60px;top:178px;z-index:20;font:600 24px Rajdhani,sans-serif;letter-spacing:.18em;text-transform:uppercase;color:#fff;text-shadow:0 2px 10px rgba(0,0,0,.8)';
    document.body.append(logo, tag);
  });
  await page.evaluate(() => document.fonts.ready);
  await sleep(250);
  await page.screenshot({ path: opt.out, type: 'png' });
  console.log('wrote', path.relative(process.cwd(), opt.out));
} finally {
  await browser.close();
}
