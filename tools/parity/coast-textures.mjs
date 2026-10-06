// Level 2's canvas textures as Chrome draws them with the bundled fonts
// (roadmap WP 7.1, as city-textures.mjs does for WP 3.8: WP 3.2's
// threshold gate needs a reference drawn with the same faces as mp_canvas,
// and the scene export is drawn with the machine's own fonts).
//
//   node tools/parity/coast-textures.mjs [--check]
//
// Opens the game on the Coast Highway (?kernel=1&freeze=1&s=0, Math.random
// seeded as the scene export seeds it) with every face of
// assets/fonts/fonts.json registered under the family the JS names before
// the page's scripts run, lets it build, and walks the groups `coast`,
// `beach` and `harbor` as tools/parity/coast-golden.mjs lists their
// textures: the drawables depth first, their materials in order of first
// use, each material's texture-valued parameters in property order, then
// its uniforms (a ShaderMaterial's own; a patched material's that three's
// ShaderLib lacks, read from the compiled program as the exporter reads
// them), four-channel textures only. Every canvas among them is read back
// once (by canvas), so each entry names its picture. Unlike City's capture,
// every canvas is held to it, lettered or not: one reference for all.
//
// Writes the RGBA to parity/cache/<key>/coast/canvas-<k>.rgba and a summary
// (per group the entries; per picture its size, SHA-256 and 8x8 block
// means; the font manifest's hash; the Chrome version) to
// parity/golden/coast/textures.json. crates/mp_worldgen/tests/coast.rs
// compares the Rust pictures with it. --check captures twice and fails if a
// picture changed or the golden would.

import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { launch, openGame } from '../../test/e2e/harness.js';
import { cacheDir, ROOT } from './lib/jstree.mjs';
import { seedRandom, RANDOM_SEED } from './lib/seed-random.mjs';

const CHECK = process.argv.includes('--check');
const OUT = path.join(ROOT, 'parity/golden/coast/textures.json');
const GROUPS = ['coast', 'beach', 'harbor'];
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
    window.__mpFonts = (window.__mpFonts || []).concat([face.load()]);
  }
}

async function capture() {
  const browser = await launch();
  try {
    const game = await openGame(browser, {
      query: 'level=coast&kernel=1&freeze=1&s=0',
      init, initArgs: [seedRandom.toString(), RANDOM_SEED, faces],
    });
    const out = await game.eval(async (groups) => {
      await Promise.all(window.__mpFonts || []);
      const loaded = (window.__mpFonts || []).length;
      const THREE = window.__THREE;
      const world = window.__world;
      const renderer = world.renderer;
      const SHADER_ID = {
        MeshStandardMaterial: 'physical', MeshPhysicalMaterial: 'physical', MeshLambertMaterial: 'lambert',
        MeshBasicMaterial: 'basic', LineBasicMaterial: 'basic', SpriteMaterial: 'sprite', PointsMaterial: 'points',
      };
      const isPatched = (m) => Object.hasOwn(m, 'onBeforeCompile') && m.onBeforeCompile !== THREE.Material.prototype.onBeforeCompile;
      const four = (t) => !(t.isDataTexture && t.format === THREE.RedFormat);
      const canvases = [];
      const pics = [];
      const picture = (t) => {
        const c = t.image;
        if (!t.isCanvasTexture && !(c instanceof HTMLCanvasElement)) return null;
        let k = canvases.indexOf(c);
        if (k >= 0) return k;
        canvases.push(c);
        const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
        let s = '';
        for (let i = 0; i < d.length; i += 0x8000) s += String.fromCharCode.apply(null, d.subarray(i, i + 0x8000));
        pics.push({ width: c.width, height: c.height, rgba: btoa(s) });
        return pics.length - 1;
      };
      const result = {};
      for (const name of groups) {
        const g = world.scene.getObjectByName(name);
        if (!g) { result[name] = null; continue; }
        const mats = [];
        const walk = (o) => {
          if ((o.isMesh || o.isPoints || o.isLine || o.isSprite) && !mats.includes(Array.isArray(o.material) ? o.material[0] : o.material)) {
            mats.push(Array.isArray(o.material) ? o.material[0] : o.material);
          }
          for (const c of o.children) walk(c);
        };
        for (const c of g.children) walk(c);
        const entries = [];
        mats.forEach((m, k) => {
          for (const key of Object.keys(m)) {
            const t = m[key];
            if (t?.isTexture && four(t)) entries.push({ material: k, key: 'params.' + key.replace(/^_/, ''), picture: picture(t) });
          }
          let uniforms = [];
          if (m.isShaderMaterial) uniforms = Object.entries(m.uniforms);
          else if (isPatched(m)) {
            const compiled = renderer.properties.get(m).uniforms;
            const base = THREE.ShaderLib[SHADER_ID[m.type]].uniforms;
            uniforms = Object.entries(compiled).filter(([k]) => !(k in base));
          }
          for (const [key, u] of uniforms) {
            const t = u.value;
            if (t?.isTexture && four(t)) entries.push({ material: k, key: 'uniforms.' + key, picture: picture(t) });
          }
        });
        result[name] = entries;
      }
      return { loaded, groups: result, pics };
    }, GROUPS);
    if (out.loaded !== faces.length) throw new Error(`${out.loaded} of ${faces.length} faces registered`);
    const errors = game.errors;
    if (errors.length) throw new Error('page errors: ' + errors.slice(0, 3).join(' | '));
    const chrome = await browser.version();
    await game.close();
    return { chrome, groups: out.groups, pics: out.pics.map((p) => ({ ...p, rgba: Buffer.from(p.rgba, 'base64') })) };
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

const run = await capture();
const summary = {
  note: 'Generated by tools/parity/coast-textures.mjs: the canvas textures of the groups coast, beach and harbor on Level 2, as Chrome draws them in the game with the bundled fonts; per group the texture entries in the order coast-golden.mjs lists them, per picture its size, SHA-256 of the RGBA and 8x8 block means. The RGBA is in parity/cache/<key>/coast/.',
  chrome: run.chrome,
  fonts: sha(manifestText).slice(0, 16),
  groups: run.groups,
  pictures: run.pics.map((p) => ({ width: p.width, height: p.height, sha256: sha(p.rgba), blocks: blocks(p.width, p.height, p.rgba) })),
};
const keep = [];
const text = JSON.stringify(summary, (k, v) => {
  if (k === 'blocks') { keep.push(JSON.stringify(v)); return `@@${keep.length - 1}@@`; }
  return v;
}, 1).replace(/"@@(\d+)@@"/g, (_, i) => keep[+i]) + '\n';
if (CHECK) {
  const run2 = await capture();
  let bad = 0;
  run.pics.forEach((p, k) => {
    if (sha(p.rgba) !== sha(run2.pics[k]?.rgba ?? Buffer.alloc(0))) { bad++; console.log(`  canvas ${k}: differs between two captures`); }
  });
  if (!fs.existsSync(OUT) || fs.readFileSync(OUT, 'utf8') !== text) { bad++; console.log(`  ${path.relative(ROOT, OUT)}: would change`); }
  if (bad) process.exit(1);
  console.log('coast-textures: two captures and the golden agree');
  process.exit(0);
}
const dir = cacheDir('coast');
run.pics.forEach((p, k) => fs.writeFileSync(path.join(dir, `canvas-${k}.rgba`), p.rgba));
fs.mkdirSync(path.dirname(OUT), { recursive: true });
fs.writeFileSync(OUT, text);
console.log(`coast-textures: ${run.pics.length} pictures from ${run.chrome} (${GROUPS.map((g) => `${g} ${run.groups[g]?.length ?? 0} entries`).join(', ')}); RGBA in ${path.relative(ROOT, dir)}, summary in ${path.relative(ROOT, OUT)}`);
