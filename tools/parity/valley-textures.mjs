// The canvas textures of src/world/Valley.js (roadmap WP 3.7) as Chrome draws
// them with the bundled fonts, the reference WP 3.2's threshold gate holds
// the port to: the scene export draws its text with the machine's own fonts,
// so its sign pixels are not a like-for-like reference (DECISIONS D332).
//
//   node tools/parity/valley-textures.mjs [--check]
//
// Opens tools/parity/textures.html (the parity kernel on, the faces of
// assets/fonts/fonts.json registered under the names the game asks for),
// imports the game's own Valley.js and makes its textures through the
// game's code: makeMaterials() (the valley sign, the store sign, the neon
// "OPEN"), buildMill() on a stub world (the mill sign) and buildCropsMesh()
// on one corn run (the corn strip). The wood signs draw their plank noise
// from Math.random, so each canvas, as it is created, finds Math.random
// reset to the seeded stream (0x5eed) at the position the scene export had
// there (crates/mr_worldgen/src/valley/mod.rs SIGN_RANDOM_AT).
//
// Writes PNGs to parity/cache/<key>/textures/valley/ and the summary
// (Chrome version, fonts manifest hash, per image the SHA-256 of its RGBA and
// 8×8 block means) to parity/golden/valley/textures.json. --check captures
// again and fails if an image differs from the golden.

import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { createHash } from 'node:crypto';
import { launch, openGame } from '../../test/e2e/harness.js';
import { cacheDir, ROOT } from './lib/jstree.mjs';
import { RANDOM_SEED } from './lib/seed-random.mjs';

const CHECK = process.argv.includes('--check');
// As SIGN_RANDOM_AT in crates/mr_worldgen/src/valley/mod.rs.
const SIGN_RANDOM_AT = [11880, 12132, 19468];

// In the page: every texture, by the game's code.
async function pageCapture(seed, at) {
  const mulberry = (s, skip) => {
    let a = s >>> 0;
    const f = () => {
      a = (a + 0x6d2b79f5) >>> 0;
      let t = a;
      t = Math.imul(t ^ (t >>> 15), t | 1);
      t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
    for (let i = 0; i < skip; i++) f();
    return f;
  };
  // Each wood sign's canvas resets Math.random to its stream position.
  const queue = [];
  const make = document.createElement.bind(document);
  document.createElement = (tag) => {
    if (tag === 'canvas' && queue.length) {
      const n = queue.shift();
      if (n !== null) Math.random = mulberry(seed, n);
    }
    return make(tag);
  };
  const THREE = await import('three');
  const { default: Valley } = await import('../../src/world/Valley.js');
  const { PaintBuilder } = await import('../../src/world/valley/Builder.js');
  const out = {};
  const grab = (name, tex) => { out[name] = window.__pixels(tex.image); };

  queue.push(at[0], at[1], null);
  const v = new Valley();
  v.makeMaterials();
  grab('signValley', v.M.signValley.map);
  grab('signStore', v.M.signStore.map);
  grab('neon', v.M.neon.emissiveMap);

  // buildMill on a stub world: only the sign's canvas is wanted.
  queue.push(at[2]);
  v.B = new PaintBuilder(v.paints);
  v.mill = { x: 0, z: 0, i: 0, nx: 1, nz: 0, y: 0 };
  v.T = { heightAt: () => 0 };
  v.ground = { height: () => 0 };
  v.avoid = [];
  v.creekS = 100;
  v.t = {
    frame: () => ({ x: 0, y: 0, z: 0, fx: 0, fz: 1, rx: 1, rz: 0, wallL: 5, wallR: 5 }),
    pointAt: () => ({ x: 0, y: 0, z: 0 }),
  };
  v.buildMill();
  grab('signMill', v.M.signMill.map);

  // buildCropsMesh on one corn run.
  queue.length = 0;
  v.group = new THREE.Group();
  v.cropRuns = [{ kind: 'corn', pts: [{ x: 0, y: 0, z: 0, r: 0.5 }, { x: 0, y: 0, z: 3, r: 0.5 }, { x: 0, y: 0, z: 6, r: 0.5 }] }];
  v.buildCropsMesh();
  grab('corn', v.group.children[0].material.map);
  return out;
}

function png(width, height, rgba) {
  const chunk = (type, data) => {
    const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
    const td = Buffer.concat([Buffer.from(type), data]);
    const crc = Buffer.alloc(4); crc.writeUInt32BE(zlib.crc32(td));
    return Buffer.concat([len, td, crc]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0); ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; ihdr[9] = 6;
  const raw = Buffer.alloc((width * 4 + 1) * height);
  for (let y = 0; y < height; y++) rgba.copy(raw, y * (width * 4 + 1) + 1, y * width * 4, (y + 1) * width * 4);
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr), chunk('IDAT', zlib.deflateSync(raw, { level: 9 })), chunk('IEND', Buffer.alloc(0))]);
}

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

async function capture() {
  const browser = await launch();
  try {
    const game = await openGame(browser, { path: 'tools/parity/textures.html', query: 'kernel=1' });
    const chrome = await browser.version();
    const r = await game.eval(pageCapture, RANDOM_SEED, SIGN_RANDOM_AT);
    if (game.errors.length) throw new Error('page errors:\n  ' + game.errors.join('\n  '));
    await game.close();
    const cases = Object.entries(r).map(([name, x]) => ({ name, width: x.width, height: x.height, rgba: Buffer.from(x.rgba, 'base64') }));
    return { chrome, cases };
  } finally {
    await browser.close();
  }
}

const FILE = path.join(ROOT, 'parity/golden/valley/textures.json');
const run = await capture();
if (CHECK) {
  const golden = JSON.parse(fs.readFileSync(FILE, 'utf8'));
  let bad = 0;
  for (const c of run.cases) {
    const g = golden.cases.find((x) => x.name === c.name);
    if (!g || g.sha256 !== sha(c.rgba)) { bad++; console.log(`  ${c.name}: differs from the golden`); }
  }
  if (bad) process.exit(1);
  console.log('valley textures: the capture and the golden agree');
  process.exit(0);
}
const dir = cacheDir('textures/valley');
for (const c of run.cases) fs.writeFileSync(path.join(dir, c.name + '.png'), png(c.width, c.height, c.rgba));
const fontsHash = sha(fs.readFileSync(path.join(ROOT, 'assets/fonts/fonts.json')));
const keep = [];
const text = JSON.stringify({
  note: 'Generated by tools/parity/valley-textures.mjs: the canvas textures of src/world/Valley.js as Chrome draws them (kernel on, bundled fonts, the plank noise at the scene export\'s Math.random positions), each with the SHA-256 of its RGBA and per-channel means over an 8x8 grid of blocks. The PNGs are in parity/cache/<key>/textures/valley/.',
  chrome: run.chrome,
  fonts: fontsHash.slice(0, 16),
  cases: run.cases.map((c) => ({ name: c.name, width: c.width, height: c.height, sha256: sha(c.rgba), blocks: blocks(c.width, c.height, c.rgba) })),
}, (k, v) => {
  if (k === 'blocks' && Array.isArray(v)) { keep.push(JSON.stringify(v)); return `@@${keep.length - 1}@@`; }
  return v;
}, 1).replace(/"@@(\d+)@@"/g, (_, i) => keep[+i]);
fs.mkdirSync(path.dirname(FILE), { recursive: true });
fs.writeFileSync(FILE, text + '\n');
console.log(`valley textures: ${run.cases.map((c) => `${c.name} ${c.width}×${c.height}`).join(', ')} from ${run.chrome}`);
