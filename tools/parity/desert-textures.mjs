// Desert's lettered canvas textures as Chrome draws them with the bundled
// fonts (roadmap WP 7.3, as city-textures.mjs does for WP 3.8: WP 3.2's
// threshold gate needs a reference drawn with the same faces as mr_canvas,
// and the scene export is drawn with the machine's own fonts).
//
//   node tools/parity/desert-textures.mjs [--check]
//
// Opens the game on Desert Run (?kernel=1&freeze=1&s=0, Math.random seeded
// as the scene export seeds it) with every face of assets/fonts/fonts.json
// registered under the family the JS names before the page's scripts run,
// lets it build, and reads back the canvas of every texture the group
// `desert` uses as both `map` and `emissiveMap` (the start gantry's two
// banners, the two painted sign atlases, the neon atlas, the finish
// banner), in order of first use. Writes the RGBA to
// parity/cache/<key>/desert/<level>-canvas-<k>.rgba and a summary (size,
// SHA-256, 8x8 block means, the font manifest's hash, the Chrome version)
// to parity/golden/desert/textures.json. crates/mr_worldgen/tests/desert.rs
// compares the Rust pictures with it. --check captures again and fails if
// a picture changed.

import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { launch, openGame } from '../../test/e2e/harness.js';
import { cacheDir, ROOT } from './lib/jstree.mjs';
import { seedRandom, RANDOM_SEED } from './lib/seed-random.mjs';

const CHECK = process.argv.includes('--check');
const OUT = path.join(ROOT, 'parity/golden/desert/textures.json');
const LEVELS = ['desert'];
const sha = (buf) => createHash('sha256').update(buf).digest('hex');

const manifestText = fs.readFileSync(path.join(ROOT, 'assets/fonts/fonts.json'));
const manifest = JSON.parse(manifestText);
const faces = manifest.faces.map((f) => ({
  family: f.family, weight: f.weight.join(' '), style: f.style,
  b64: fs.readFileSync(path.join(ROOT, 'assets/fonts', f.file)).toString('base64'),
}));

// Runs in the page before its scripts: the seeded Math.random, then the
// faces (parsed from memory, so they are ready before the world draws).
function init(seedSrc, seed, faces) {
  (new Function('return ' + seedSrc))()(seed);
  for (const f of faces) {
    const s = atob(f.b64);
    const bytes = new Uint8Array(s.length);
    for (let i = 0; i < s.length; i++) bytes[i] = s.charCodeAt(i);
    const face = new FontFace(f.family, bytes, { weight: f.weight, style: f.style });
    document.fonts.add(face);
    window.__mrFonts = (window.__mrFonts || []).concat([face.load()]);
  }
}

async function capture(level) {
  const browser = await launch();
  try {
    const game = await openGame(browser, {
      query: `level=${level}&kernel=1&freeze=1&s=0`,
      init, initArgs: [seedRandom.toString(), RANDOM_SEED, faces],
    });
    const out = await game.eval(async () => {
      await Promise.all(window.__mrFonts || []);
      const loaded = (window.__mrFonts || []).length;
      const g = window.__world.scene.getObjectByName('desert');
      const mats = [];
      g.traverse((o) => {
        if ((o.isMesh || o.isSprite) && !Array.isArray(o.material) && !mats.includes(o.material)) mats.push(o.material);
      });
      const seen = [];
      const pics = [];
      for (const m of mats) {
        const t = m.map;
        if (!t || !t.isCanvasTexture || m.emissiveMap !== t || seen.includes(t.image)) continue;
        seen.push(t.image);
        const c = t.image;
        const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
        let s = '';
        for (let i = 0; i < d.length; i += 0x8000) s += String.fromCharCode.apply(null, d.subarray(i, i + 0x8000));
        pics.push({ width: c.width, height: c.height, rgba: btoa(s) });
      }
      return { loaded, pics };
    });
    if (out.loaded !== faces.length) throw new Error(`${out.loaded} of ${faces.length} faces registered`);
    const errors = game.errors;
    if (errors.length) throw new Error('page errors: ' + errors.slice(0, 3).join(' | '));
    const chrome = await browser.version();
    await game.close();
    return { chrome, pics: out.pics.map((p) => ({ ...p, rgba: Buffer.from(p.rgba, 'base64') })) };
  } finally {
    await browser.close();
  }
}

// Per channel means over an 8×8 grid of blocks, two decimals (textures.mjs).
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

async function captureAll() {
  const out = {};
  let chrome = null;
  for (const level of LEVELS) {
    const r = await capture(level);
    chrome = r.chrome;
    out[level] = r.pics;
  }
  return { chrome, levels: out };
}

const run = await captureAll();
const summary = {
  note: 'Generated by tools/parity/desert-textures.mjs: the lettered canvas textures of the group desert (map and emissiveMap), as Chrome draws them in the game with the bundled fonts; size, SHA-256 of the RGBA, 8x8 block means. The RGBA is in parity/cache/<key>/desert/.',
  chrome: run.chrome,
  fonts: sha(manifestText).slice(0, 16),
  levels: Object.fromEntries(LEVELS.map((l) => [l, run.levels[l].map((p) => ({
    width: p.width, height: p.height, sha256: sha(p.rgba), blocks: blocks(p.width, p.height, p.rgba),
  }))])),
};
const keep = [];
const text = JSON.stringify(summary, (k, v) => {
  if (k === 'blocks') { keep.push(JSON.stringify(v)); return `@@${keep.length - 1}@@`; }
  return v;
}, 1).replace(/"@@(\d+)@@"/g, (_, i) => keep[+i]) + '\n';
if (CHECK) {
  const run2 = await captureAll();
  let bad = 0;
  for (const l of LEVELS) {
    run.levels[l].forEach((p, k) => {
      if (sha(p.rgba) !== sha(run2.levels[l][k]?.rgba ?? Buffer.alloc(0))) { bad++; console.log(`  ${l} canvas ${k}: differs between two captures`); }
    });
  }
  if (!fs.existsSync(OUT) || fs.readFileSync(OUT, 'utf8') !== text) { bad++; console.log(`  ${path.relative(ROOT, OUT)}: would change`); }
  if (bad) process.exit(1);
  console.log('desert-textures: two captures and the golden agree');
  process.exit(0);
}
const dir = cacheDir('desert');
for (const l of LEVELS) run.levels[l].forEach((p, k) => fs.writeFileSync(path.join(dir, `${l}-canvas-${k}.rgba`), p.rgba));
fs.mkdirSync(path.dirname(OUT), { recursive: true });
fs.writeFileSync(OUT, text);
console.log(`desert-textures: ${LEVELS.map((l) => `${l} ${run.levels[l].length}`).join(', ')} pictures from ${run.chrome}; RGBA in ${path.relative(ROOT, dir)}, summary in ${path.relative(ROOT, OUT)}`);
