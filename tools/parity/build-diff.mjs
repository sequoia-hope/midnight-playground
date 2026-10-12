// Pixel comparison of two builds of the Rust client on WebGL2 (a WebGL2
// canvas composites into Chrome's own screenshots; a WebGPU one does not):
// the same frozen station of each level from dist/<a>/ and dist/<b>/,
// through the registered dev server, compared pixel for pixel. `dist/prof/`
// (`cargo xtask prof`, a copy of the previous dist/next/) is the usual
// "before". Prints, per level, how many pixels differ and the largest
// channel difference; writes the shots and a red overlay of the differing
// pixels to parity/report/build-diff/.
//
//   node tools/parity/build-diff.mjs [--a prof] [--b next] [--levels sierra,coast,seaside,desert]
//       [--query "s=300&h=2.2&back=7&pitch=-0.04&freeze=1&t=0.3&hq=1"]
import puppeteer from 'puppeteer-core';
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { ROOT } from './lib/jstree.mjs';
import { baseUrl, chromeFlags } from './lib/server.mjs';
const args = process.argv.slice(2);
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
const A = opt('--a', 'prof'), B = opt('--b', 'next');
const levels = opt('--levels', 'sierra,coast,seaside,desert').split(',');
const base = baseUrl(opt);
const query = opt('--query', 's=300&h=2.2&back=7&pitch=-0.04&freeze=1&t=0.3&hq=1');
const out = path.join(ROOT, 'parity/report/build-diff'); fs.mkdirSync(out, { recursive: true });
// Minimal PNG decode (8-bit RGBA, non-interlaced) for the comparison.
function decode(buf) {
  let p = 8, w = 0, h = 0, bpp = 4; const idat = [];
  while (p < buf.length) { const len = buf.readUInt32BE(p); const t = buf.toString('ascii', p + 4, p + 8); const d = buf.subarray(p + 8, p + 8 + len);
    if (t === 'IHDR') { w = d.readUInt32BE(0); h = d.readUInt32BE(4); if (d[8] !== 8 || (d[9] !== 6 && d[9] !== 2)) throw new Error('png not rgb(a)8'); bpp = d[9] === 6 ? 4 : 3; }
    else if (t === 'IDAT') idat.push(d); p += 12 + len; }
  const raw = zlib.inflateSync(Buffer.concat(idat)); const stride = w * bpp; const px = Buffer.alloc(w * h * bpp);
  for (let y = 0; y < h; y++) { const f = raw[y * (stride + 1)]; const row = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1)); const o = y * stride;
    for (let x = 0; x < stride; x++) { const a = x >= bpp ? px[o + x - bpp] : 0, b = y > 0 ? px[o - stride + x] : 0, c = x >= bpp && y > 0 ? px[o - stride + x - bpp] : 0; let v = row[x];
      if (f === 1) v += a; else if (f === 2) v += b; else if (f === 3) v += (a + b) >> 1; else if (f === 4) { const q = a + b - c, pa = Math.abs(q - a), pb = Math.abs(q - b), pc = Math.abs(q - c); v += pa <= pb && pa <= pc ? a : pb <= pc ? b : c; }
      px[o + x] = v & 255; } }
  return { w, h, bpp, px };
}
// Minimal PNG encode (8-bit RGB) for the overlay.
function encode(w, h, rgb) {
  const crcTable = new Int32Array(256).map((_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c; });
  const crc = (buf) => { let c = -1; for (const b of buf) c = crcTable[(c ^ b) & 255] ^ (c >>> 8); return (c ^ -1) >>> 0; };
  const chunk = (type, data) => { const len = Buffer.alloc(4); len.writeUInt32BE(data.length); const td = Buffer.concat([Buffer.from(type, 'ascii'), data]); const c = Buffer.alloc(4); c.writeUInt32BE(crc(td)); return Buffer.concat([len, td, c]); };
  const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 2;
  const raw = Buffer.alloc((w * 3 + 1) * h);
  for (let y = 0; y < h; y++) { raw[y * (w * 3 + 1)] = 0; rgb.copy(raw, y * (w * 3 + 1) + 1, y * w * 3, (y + 1) * w * 3); }
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr), chunk('IDAT', zlib.deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]);
}
const browser = await puppeteer.launch({ executablePath: process.env.CHROME_PATH || '/usr/bin/google-chrome', headless: 'new', args: chromeFlags({ backend: 'webgl2', angle: 'gl' }), protocolTimeout: 600000 });
try {
  for (const level of levels) {
    const shots = {};
    for (const dist of [A, B]) {
      const page = await browser.newPage();
      page.on('pageerror', (e) => console.error(`[${dist} ${level} page error] ${e}`));
      await page.setViewport({ width: 1280, height: 800, deviceScaleFactor: 1 });
      await page.goto(`${base}dist/${dist}/index.html?level=${level}&${query}`);
      await page.waitForFunction('window.__mp && window.__mp.ready', { timeout: 300000, polling: 200 });
      await new Promise((r) => setTimeout(r, 1500));
      const file = path.join(out, `${level}-${dist}.png`);
      await page.screenshot({ path: file });
      shots[dist] = decode(fs.readFileSync(file));
      await page.close();
    }
    const a = shots[A], b = shots[B];
    let diff = 0, maxd = 0;
    const over = Buffer.alloc(a.w * a.h * 3);
    for (let i = 0, o = 0; i < a.px.length; i += a.bpp, o += 3) {
      const d = Math.max(Math.abs(a.px[i] - b.px[i]), Math.abs(a.px[i + 1] - b.px[i + 1]), Math.abs(a.px[i + 2] - b.px[i + 2]));
      if (d) { diff++; if (d > maxd) maxd = d; over[o] = 255; } else { over[o] = b.px[i] / 3; over[o + 1] = b.px[i + 1] / 3; over[o + 2] = b.px[i + 2] / 3; }
    }
    fs.writeFileSync(path.join(out, `${level}-diff.png`), encode(a.w, a.h, over));
    console.log(`${level}: ${a.w}x${a.h}, ${diff} pixels differ (${(100 * diff / (a.w * a.h)).toFixed(3)}%), max channel difference ${maxd}`);
  }
} finally { await browser.close(); }
