// Texture reference (roadmap WP 3.2, SPEC 5.7): the shared textures of
// src/world/textures.js, and a set of canvas probes for the parts of the
// Canvas 2D subset textures.js does not reach, drawn by headless Chrome
// with the parity kernel on and the bundled fonts (assets/fonts/fonts.json)
// registered under the names the game asks for.
//
//   node tools/parity/textures.mjs [--check]
//
// Writes the pixels as PNGs to parity/cache/<js-tree-key>/textures/ (and
// probes/), and the case definitions with a small summary of each image
// (size, SHA-256 of the RGBA, 8×8 block means) to
// parity/golden/textures/{textures,probes}.json. The Rust side
// (crates/mr_worldgen/tests/textures.rs, crates/mr_canvas/tests/probes.rs)
// draws the same cases and compares: every pixel when the cached PNGs are
// there, the block means otherwise; the noise-only textures must be
// bit-identical. --check captures again and fails if any image changed.

import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { createHash } from 'node:crypto';
import { launch, openGame } from '../../test/e2e/harness.js';
import { cacheDir, ROOT } from './lib/jstree.mjs';

const CHECK = process.argv.includes('--check');

// ── The cases ────────────────────────────────────────────────────────────
// fn and args call src/world/textures.js; part picks a texture out of an
// object result. exact: the texture is computed without any drawing (pixel
// data put on the canvas, or a DataTexture), so the port must match bit for
// bit.

const FW = 'bold 64px "Arial Narrow", Arial, sans-serif';
const FW3 = 'bold 54px "Arial Narrow", Arial, sans-serif';
// The signs the game draws: freeway.js (City and the cruise loop) and
// Harbor.js, one of each distinct shape of call.
const SIGNS = [
  ['loop-downtown', ['LOOP', 'Downtown', 'Next 2 exits'], { arrow: 'up', font: FW3 }],
  ['exit7', ['EXIT 7', 'Harbor Blvd', '1 MILE'], { font: FW3 }],
  ['exit8', ['EXIT 8', 'Grand Ave'], { arrow: 'right', font: FW }],
  ['loop-tunnel', ['LOOP', 'Central Tunnel'], { arrow: 'up', font: FW }],
  ['harbor-tunnel', ['HARBOR TUNNEL', 'LIGHTS ON'], { bg: '#f2c230', fg: '#111', border: '#111', font: FW }],
  ['i9-west', ['I-9 WEST', 'Downtown Meridian'], { arrow: 'up', font: FW }],
  ['exit41', ['EXIT 41', 'Airport Rd', '1 MILE'], { font: FW3 }],
  ['port-gate', ['PORT MERIDIAN', 'TERMINAL GATE'], { bg: '#123a52', fg: '#ffffff', border: '#ffffff', w: 512, h: 160, font: 'bold 54px "Arial Narrow", Arial, sans-serif' }],
  ['meridian-star', ['MERIDIAN STAR'], { bg: '#1f3550', fg: '#ffffff', border: null, w: 512, h: 96, font: 'bold 64px Arial, sans-serif' }],
  ['meridian-star-2', ['MERIDIAN STAR', 'PORT MERIDIAN'], { bg: '#1f3550', fg: '#ffffff', border: null, w: 512, h: 160, font: 'bold 56px Arial, sans-serif' }],
  ['port-meridian', ['PORT MERIDIAN'], { bg: '#123a52', fg: '#ffffff', border: '#ffffff', w: 1024, h: 180, font: 'bold 110px "Arial Narrow", Arial, sans-serif' }],
];

export const CASES = [
  { name: 'detail', fn: 'detailTexture', args: [], exact: true },
  { name: 'terrainDetail', fn: 'terrainDetailTexture', args: [], exact: true },
  { name: 'rock', fn: 'rockTexture', args: [] },
  { name: 'asphalt0', fn: 'asphaltTexture', args: [0] },
  { name: 'asphalt1', fn: 'asphaltTexture', args: [1] },
  { name: 'asphalt2', fn: 'asphaltTexture', args: [2] },
  { name: 'gravel', fn: 'gravelTexture', args: [] },
  { name: 'concrete', fn: 'concreteTexture', args: [] },
  { name: 'chevron', fn: 'chevronTexture', args: [] },
  { name: 'checker10', fn: 'checkerTexture', args: [10] },
  { name: 'checker8', fn: 'checkerTexture', args: [8] },
  { name: 'glow', fn: 'glowTexture', args: [] },
  { name: 'smoke', fn: 'smokeTexture', args: [] },
  ...[0, 1, 2, 3, 4, 5].flatMap((v) => [
    { name: `facade${v}-map`, fn: 'facadeTextures', args: [v], part: 'map' },
    { name: `facade${v}-emissive`, fn: 'facadeTextures', args: [v], part: 'emissive' },
  ]),
  ...SIGNS.map(([name, lines, opts]) => ({ name: `sign-${name}`, fn: 'signTexture', args: [lines, opts], part: 'texture' })),
];

// ── Probes ───────────────────────────────────────────────────────────────
// Small scenes in a tiny language both sides interpret: [method, ...args]
// calls the context method; ['=', prop, value] sets a property;
// ['grad', 'fillStyle'|'strokeStyle', 'linear'|'radial', [coords], [[offset,
// colour], ...]] makes and assigns a gradient; ['image', {w, h, ops}, ...
// drawImage args] draws another probe canvas.

const SRC = { w: 40, h: 30, ops: [['=', 'fillStyle', '#204080'], ['fillRect', 0, 0, 40, 30], ['=', 'fillStyle', 'rgba(255,200,0,0.8)'], ['beginPath'], ['arc', 20, 15, 11, 0, 7], ['fill'], ['=', 'fillStyle', '#fff'], ['fillRect', 4, 4, 6, 6]] };

export const PROBES = [
  { name: 'paths', w: 160, h: 120, ops: [
    ['=', 'fillStyle', '#123'], ['fillRect', 0, 0, 160, 120],
    ['=', 'fillStyle', 'hsl(30,80%,55%)'], ['beginPath'], ['moveTo', 10, 100], ['quadraticCurveTo', 40, 10, 70, 100], ['closePath'], ['fill'],
    ['=', 'strokeStyle', 'white'], ['=', 'lineWidth', 3], ['beginPath'], ['arc', 110, 40, 25, 0, Math.PI, true], ['stroke'],
    ['=', 'lineCap', 'round'], ['=', 'lineJoin', 'round'], ['=', 'lineWidth', 7], ['=', 'strokeStyle', 'rgba(120,220,140,0.7)'],
    ['beginPath'], ['moveTo', 90, 110], ['lineTo', 120, 75], ['lineTo', 150, 110], ['stroke'],
    ['=', 'lineCap', 'butt'], ['=', 'lineJoin', 'miter'], ['=', 'lineWidth', 2.5], ['=', 'strokeStyle', '#f0c'], ['strokeRect', 6.5, 6.5, 40, 20],
    ['=', 'fillStyle', 'black'], ['beginPath'], ['ellipse', 60, 60, 20, 8, 0.6, Math.PI, 0], ['fill'],
    ['=', 'fillStyle', 'rgba(255,255,255,0.5)'], ['beginPath'], ['rect', 125, 85, 30, 30], ['rect', 135, 95, 10, 10], ['fill'],
  ] },
  { name: 'clip', w: 128, h: 96, ops: [
    ['=', 'fillStyle', '#2a2a2a'], ['fillRect', 0, 0, 128, 96],
    ['save'], ['beginPath'], ['rect', 20, 10, 60, 50], ['clip'],
    ['=', 'fillStyle', '#e33'], ['beginPath'], ['arc', 70, 50, 30, 0, 7], ['fill'],
    ['=', 'font', 'bold 40px "Arial Narrow", Arial'], ['=', 'fillStyle', '#fff'], ['fillText', 'CLIP', 14, 50],
    ['restore'],
    ['=', 'fillStyle', 'rgba(0,200,255,0.6)'], ['fillRect', 60, 40, 50, 40],
    ['clearRect', 90, 70, 30, 20],
  ] },
  { name: 'composite', w: 160, h: 80, ops: [
    ['=', 'fillStyle', '#335'], ['fillRect', 0, 0, 160, 80],
    ['=', 'globalCompositeOperation', 'lighter'], ['=', 'fillStyle', 'rgba(200,80,40,0.8)'], ['fillRect', 10, 10, 40, 40], ['fillRect', 30, 30, 40, 40],
    // lighten over a backdrop drawn by an earlier op: Chrome's GPU canvas
    // reads the destination once per batch for this mode, so two
    // overlapping lighten draws in a row do not see each other (the game
    // never overlaps them: cityTextures.js draws disjoint cells).
    ['=', 'fillStyle', '#6a6'], ['fillRect', 80, 10, 30, 60],
    ['=', 'globalCompositeOperation', 'lighten'], ['=', 'fillStyle', '#a33'], ['fillRect', 95, 20, 30, 40],
    ['=', 'globalCompositeOperation', 'destination-out'], ['=', 'fillStyle', 'rgba(0,0,0,0.7)'], ['beginPath'], ['arc', 140, 40, 16, 0, 7], ['fill'],
    ['=', 'globalCompositeOperation', 'source-over'], ['=', 'globalAlpha', 0.5], ['=', 'fillStyle', '#ff0'], ['fillRect', 0, 60, 160, 10],
  ] },
  { name: 'gradients', w: 128, h: 96, ops: [
    ['grad', 'fillStyle', 'linear', [0, 0, 128, 0], [[0, '#000'], [0.5, 'rgba(255,0,0,0.5)'], [1, '#ff0']]], ['fillRect', 0, 0, 128, 40],
    ['grad', 'fillStyle', 'linear', [0, 40, 0, 96], [[0, '#0a4'], [0.3, '#0a4'], [0.3, '#048'], [1, 'rgba(0,0,0,0)']]], ['fillRect', 0, 40, 64, 56],
    ['grad', 'fillStyle', 'radial', [96, 68, 0, 96, 68, 28], [[0, '#fff'], [0.4, 'rgba(255,180,60,0.6)'], [1, 'rgba(255,0,0,0)']]], ['fillRect', 64, 40, 64, 56],
  ] },
  { name: 'shadow', w: 160, h: 100, ops: [
    ['=', 'fillStyle', '#0b1020'], ['fillRect', 0, 0, 160, 100],
    ['=', 'shadowColor', 'rgba(255,120,40,0.9)'], ['=', 'shadowBlur', 14], ['=', 'fillStyle', '#ffd080'], ['fillRect', 20, 20, 40, 30],
    ['=', 'shadowColor', '#3cf'], ['=', 'shadowBlur', 8], ['=', 'font', 'bold 36px Arial'], ['=', 'fillStyle', '#e8fbff'], ['fillText', 'NEON', 72, 60],
    ['=', 'shadowBlur', 0], ['=', 'fillStyle', '#fff'], ['fillRect', 20, 70, 30, 10],
  ] },
  { name: 'filter', w: 128, h: 128, ops: [
    ['=', 'fillStyle', '#000'], ['fillRect', 0, 0, 128, 128],
    ['=', 'filter', 'blur(6px)'], ['=', 'fillStyle', 'rgba(255,255,255,0.8)'], ['beginPath'], ['ellipse', 64, 64, 30, 16, 0.5, 0, 7], ['fill'],
    ['=', 'filter', 'none'], ['=', 'fillStyle', '#f00'], ['fillRect', 10, 10, 10, 10],
  ] },
  { name: 'text-align', w: 256, h: 160, ops: [
    ['=', 'fillStyle', '#f4f0e0'], ['fillRect', 0, 0, 256, 160],
    ['=', 'strokeStyle', '#c00'], ['=', 'lineWidth', 1], ['beginPath'], ['moveTo', 128.5, 0], ['lineTo', 128.5, 160], ['stroke'],
    ['=', 'fillStyle', '#123'], ['=', 'font', 'bold 26px "Arial Narrow", Arial, sans-serif'],
    ['=', 'textAlign', 'left'], ['=', 'textBaseline', 'alphabetic'], ['fillText', 'Left alpha', 128, 30],
    ['=', 'textAlign', 'right'], ['=', 'textBaseline', 'middle'], ['fillText', 'Right middle', 128, 60],
    ['=', 'textAlign', 'center'], ['=', 'textBaseline', 'top'], ['fillText', 'Center top', 128, 80],
    ['=', 'textBaseline', 'bottom'], ['fillText', 'Center bottom', 128, 140],
    ['=', 'textBaseline', 'middle'], ['fillText', 'SQUEEZED INTO NINETY', 128, 150, 90],
  ] },
  { name: 'text-faces', w: 320, h: 240, ops: [
    ['=', 'fillStyle', '#20242c'], ['fillRect', 0, 0, 320, 240],
    ['=', 'textAlign', 'center'], ['=', 'textBaseline', 'middle'], ['=', 'fillStyle', '#fff'],
    ['=', 'font', 'italic 900 34px "Arial Narrow", Arial, sans-serif'], ['fillText', 'HOLLOW PT.', 160, 22],
    ['=', 'font', 'bold 34px "Arial Black", Arial'], ['fillText', 'TACOS', 160, 58],
    ['=', 'font', 'italic bold 30px Georgia, serif'], ['fillText', 'Boardwalk Cafe', 160, 94],
    ['=', 'font', 'bold 40px "Brush Script MT", "Segoe Script", cursive'], ['fillText', 'Ice Cream', 160, 132],
    ['=', 'font', 'bold 28px "Courier New", monospace'], ['fillText', 'REG 4.19', 160, 168],
    ['=', 'font', '700 22px Arial'], ['fillText', 'GAS · EATS · OPEN 24 HRS', 160, 198],
    ['=', 'font', '600 22px Rajdhani, Arial Narrow, sans-serif'], ['fillText', 'LAP 2/3  0:42.18', 160, 226],
  ] },
  { name: 'text-transform', w: 200, h: 160, ops: [
    ['=', 'fillStyle', '#e8e0c8'], ['fillRect', 0, 0, 200, 160],
    ['=', 'fillStyle', '#7a2e22'], ['=', 'textAlign', 'center'], ['=', 'textBaseline', 'middle'], ['=', 'font', 'bold 28px Georgia, serif'],
    ['save'], ['translate', 60, 80], ['rotate', -Math.PI / 2], ['fillText', 'Ice Cold', 0, 10], ['restore'],
    ['save'], ['translate', 140, 50], ['scale', -1, 1], ['fillText', 'MIRROR', 0, 0], ['restore'],
    ['=', 'strokeStyle', '#123'], ['=', 'lineWidth', 2], ['=', 'font', 'bold 34px "Arial Black", Arial'], ['strokeText', 'OPEN', 140, 120],
  ] },
  { name: 'draw-image', w: 160, h: 90, ops: [
    ['=', 'fillStyle', '#eee'], ['fillRect', 0, 0, 160, 90],
    ['image', SRC, 4, 4, 40, 30],
    ['image', SRC, 50, 4, 80, 60],
    ['image', SRC, 10, 5, 20, 20, 4, 50, 30, 30],
    ['image', SRC, 140, 60, 15, 12],
  ] },
  { name: 'pixels', w: 64, h: 64, ops: [
    ['=', 'fillStyle', 'rgba(40,120,200,0.5)'], ['fillRect', 0, 0, 64, 64],
    ['putGradient', 8, 8, 32, 32],
    ['=', 'fillStyle', 'rgba(255,255,255,0.3)'], ['fillRect', 20, 20, 30, 30],
  ] },
];

// ── In the page ──────────────────────────────────────────────────────────

function pageCase(c) {
  const T = window.__T;
  let r = T[c.fn](...c.args);
  if (c.part) r = r[c.part];
  const img = r.image;
  if (img instanceof HTMLCanvasElement) return window.__pixels(img);
  // A DataTexture.
  let s = '';
  const d = img.data;
  for (let i = 0; i < d.length; i += 0x8000) s += String.fromCharCode.apply(null, d.subarray(i, i + 0x8000));
  return { width: img.width, height: img.height, rgba: btoa(s) };
}

function pageProbe(p) {
  const run = (def) => {
    const c = document.createElement('canvas');
    c.width = def.w; c.height = def.h;
    const g = c.getContext('2d');
    for (const op of def.ops) {
      const [m, ...a] = op;
      if (m === '=') g[a[0]] = a[1];
      else if (m === 'grad') {
        const grd = a[1] === 'linear' ? g.createLinearGradient(...a[2]) : g.createRadialGradient(...a[2]);
        for (const [o, col] of a[3]) grd.addColorStop(o, col);
        g[a[0]] = grd;
      } else if (m === 'image') g.drawImage(run(a[0]), ...a.slice(1));
      else if (m === 'putGradient') {
        // Pixel data with varying alpha, put and read back.
        const [x, y, w, h] = a;
        const img = g.createImageData(w, h);
        for (let j = 0; j < h; j++) for (let i = 0; i < w; i++) {
          const k = (j * w + i) * 4;
          img.data[k] = i * 8; img.data[k + 1] = j * 8; img.data[k + 2] = 200; img.data[k + 3] = (i + j) * 4;
        }
        g.putImageData(img, x, y);
        const back = g.getImageData(x, y, w, h);
        g.putImageData(back, x + 24, y + 24);
      } else g[m](...a);
    }
    return c;
  };
  return window.__pixels(run(p));
}

// ── PNG out ──────────────────────────────────────────────────────────────

function png(width, height, rgba) {
  const chunk = (type, data) => {
    const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
    const td = Buffer.concat([Buffer.from(type), data]);
    const crc = Buffer.alloc(4); crc.writeUInt32BE(zlib.crc32(td));
    return Buffer.concat([len, td, crc]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0); ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; ihdr[9] = 6; ihdr[10] = 0; ihdr[11] = 0; ihdr[12] = 0;
  const raw = Buffer.alloc((width * 4 + 1) * height);
  for (let y = 0; y < height; y++) {
    raw[y * (width * 4 + 1)] = 0;
    rgba.copy(raw, y * (width * 4 + 1) + 1, y * width * 4, (y + 1) * width * 4);
  }
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr), chunk('IDAT', zlib.deflateSync(raw, { level: 9 })), chunk('IEND', Buffer.alloc(0))]);
}

// Per channel means over an 8×8 grid of blocks, two decimals.
function blocks(width, height, rgba) {
  const out = [];
  for (let by = 0; by < 8; by++) for (let bx = 0; bx < 8; bx++) {
    const x0 = Math.floor(bx * width / 8), x1 = Math.floor((bx + 1) * width / 8);
    const y0 = Math.floor(by * height / 8), y1 = Math.floor((by + 1) * height / 8);
    const s = [0, 0, 0, 0];
    for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) for (let k = 0; k < 4; k++) s[k] += rgba[(y * width + x) * 4 + k];
    const n = (x1 - x0) * (y1 - y0);
    out.push(s.map((v) => Math.round(v / n * 100) / 100));
  }
  return out;
}

const sha = (b) => createHash('sha256').update(b).digest('hex');

// JSON with one line per case: the block means and op lists stay compact.
function writeJson(file, obj) {
  const keep = [];
  const text = JSON.stringify(obj, (k, v) => {
    if ((k === 'blocks' || k === 'ops' || k === 'args') && Array.isArray(v)) { keep.push(JSON.stringify(v)); return `@@${keep.length - 1}@@`; }
    return v;
  }, 1).replace(/"@@(\d+)@@"/g, (_, i) => keep[+i]);
  fs.writeFileSync(file, text + '\n');
}

async function capture() {
  const browser = await launch();
  try {
    const game = await openGame(browser, { path: 'tools/parity/textures.html', query: 'kernel=1' });
    const chrome = await browser.version();
    const cases = [];
    for (const c of CASES) {
      const r = await game.eval(pageCase, c);
      cases.push({ ...c, ...r, rgba: Buffer.from(r.rgba, 'base64') });
    }
    const probes = [];
    for (const p of PROBES) {
      const r = await game.eval(pageProbe, p);
      probes.push({ ...p, ...r, rgba: Buffer.from(r.rgba, 'base64') });
    }
    if (game.errors.length) throw new Error('page errors:\n  ' + game.errors.join('\n  '));
    await game.close();
    return { chrome, cases, probes };
  } finally {
    await browser.close();
  }
}

const fontsHash = sha(fs.readFileSync(path.join(ROOT, 'assets/fonts/fonts.json')));
const run1 = await capture();
if (CHECK) {
  const run2 = await capture();
  let bad = 0;
  for (const [a, b] of [[run1.cases, run2.cases], [run1.probes, run2.probes]]) {
    a.forEach((x, i) => { if (sha(x.rgba) !== sha(b[i].rgba)) { bad++; console.log(`  ${x.name}: differs between two captures`); } });
  }
  const golden = JSON.parse(fs.readFileSync(path.join(ROOT, 'parity/golden/textures/textures.json'), 'utf8'));
  for (const c of run1.cases) {
    const g = golden.cases.find((x) => x.name === c.name);
    if (!g || g.sha256 !== sha(c.rgba)) { bad++; console.log(`  ${c.name}: differs from the golden`); }
  }
  if (bad) { console.log(`${bad} difference(s)`); process.exit(1); }
  console.log('textures: two captures and the golden agree');
  process.exit(0);
}

const outDir = cacheDir('textures');
const probeDir = cacheDir('textures/probes');
const goldenDir = path.join(ROOT, 'parity/golden/textures');
fs.mkdirSync(goldenDir, { recursive: true });
const summary = (x) => ({ width: x.width, height: x.height, sha256: sha(x.rgba), blocks: blocks(x.width, x.height, x.rgba) });
for (const c of run1.cases) fs.writeFileSync(path.join(outDir, c.name + '.png'), png(c.width, c.height, c.rgba));
for (const p of run1.probes) fs.writeFileSync(path.join(probeDir, p.name + '.png'), png(p.width, p.height, p.rgba));
const head = { chrome: run1.chrome, fonts: fontsHash.slice(0, 16) };
writeJson(path.join(goldenDir, 'textures.json'), {
  note: 'Generated by tools/parity/textures.mjs: the shared textures of src/world/textures.js as Chrome draws them (kernel on, bundled fonts), each with the SHA-256 of its RGBA and per-channel means over an 8x8 grid of blocks. The PNGs are in parity/cache/<key>/textures/.',
  ...head,
  cases: run1.cases.map((c) => ({ name: c.name, fn: c.fn, args: c.args, part: c.part ?? null, exact: !!c.exact, ...summary(c) })),
});
writeJson(path.join(goldenDir, 'probes.json'), {
  note: 'Generated by tools/parity/textures.mjs: canvas probes for the Canvas 2D subset beyond textures.js, as Chrome draws them, with the same summary as textures.json. The PNGs are in parity/cache/<key>/textures/probes/.',
  ...head,
  probes: run1.probes.map((p) => ({ name: p.name, w: p.w, h: p.h, ops: p.ops, ...summary(p) })),
});
console.log(`textures: ${run1.cases.length} textures and ${run1.probes.length} probes from ${run1.chrome}\n  images in ${path.relative(ROOT, outDir)}\n  summaries in parity/golden/textures/`);
