#!/usr/bin/env node
// Scene export (roadmap WP 0.5, SPEC 5.1, 5.4, 5.7): drives the JS game in
// headless Chrome through the e2e harness (no server, no port), with the
// parity kernel on and the scenery frozen, and writes for each level
//
//   parity/cache/<key>/scenes/<level>.mrscene        the scene (crates/mr_scene/FORMAT.md)
//   parity/cache/<key>/scenes/<level>.digest.json    digest of the live three.js objects
//   parity/cache/<key>/world/<level>/track.{json,bin}    every Track array and value
//   parity/cache/<key>/world/<level>/terrain.{json,bin}  heights at 10,000 points
//   parity/golden/world/<level>.json                 sha256 per Track array and of the terrain
//                                                    heights; the scene's counts, kinds and
//                                                    the sha256 of its digest (committed)
//
// plus models.mrscene and golden models.json: every car model, the effects
// and the pursuit props. `cargo xtask parity scene-check` reads each
// .mrscene back with mr_scene and checks it against the digest.
//
//   node tools/parity/scene-export.mjs [options]
//     --levels a,b     levels to export (default: all six)
//     --base           terrain, road and sky only (<level>.base.mrscene; no dumps, no goldens)
//     --no-models      skip models.mrscene
//     --check          compare with the committed goldens instead of writing them,
//                      and with the cached files of an earlier run; exit 1 on a difference
//
// Math.random is replaced by a seeded mulberry32 before the page loads, so
// the two world-generation spots that draw from it come out the same every
// run (docs/rust-port/DECISIONS.md, D20).
import fs from 'node:fs';
import { seedRandom, RANDOM_SEED } from './lib/seed-random.mjs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { launch, openGame } from '../../test/e2e/harness.js';
import { cacheDir, ROOT } from './lib/jstree.mjs';

const LEVELS = ['sierra', 'coast', 'streets', 'desert', 'seaside', 'cruise'];
const QUERY = 'kernel=1&freeze=1&s=0';
const CHUNK = 24 << 20;

const argv = process.argv.slice(2);
const flag = (f) => argv.includes(f);
const opt = (f) => { const i = argv.indexOf(f); return i >= 0 ? argv[i + 1] : null; };
const levels = opt('--levels') ? opt('--levels').split(',') : LEVELS;
const base = flag('--base');
const check = flag('--check');
const withModels = !flag('--no-models') && !base;
for (const l of levels) if (!LEVELS.includes(l)) throw new Error('unknown level ' + l);

const sceneDir = cacheDir('scenes');
const worldDir = cacheDir('world');
const goldenDir = path.join(ROOT, 'parity', 'golden', 'world');
fs.mkdirSync(goldenDir, { recursive: true });
const pageScript = fs.readFileSync(path.join(ROOT, 'tools/parity/lib/scene-page.js'), 'utf8');

const sha = (buf) => createHash('sha256').update(buf).digest('hex');
const problems = [];


async function pull(game, length) {
  const parts = [];
  for (let at = 0; at < length; at += CHUNK) {
    const b64 = await game.eval(`window.__mrSceneExport.readBinary(${at}, ${Math.min(length, at + CHUNK)})`);
    parts.push(Buffer.from(b64, 'base64'));
  }
  const bin = Buffer.concat(parts);
  if (bin.length !== length) throw new Error(`binary is ${bin.length} bytes, expected ${length}`);
  return bin;
}

// .mrscene: magic, version, header length, JSON header (padded with spaces
// to 8 bytes), binary.
function sceneFile(headerJson, bin) {
  const json = Buffer.from(headerJson, 'utf8');
  const head = Buffer.alloc(16);
  head.write('MRSCENE\0', 0, 'latin1');
  head.writeUInt32LE(1, 8);
  head.writeUInt32LE(json.length, 12);
  const pad = (8 - ((16 + json.length) % 8)) % 8;
  return Buffer.concat([head, json, Buffer.alloc(pad, 0x20), bin]);
}

// JSON with one array element or object entry per line below the top two
// levels, so a committed digest diffs line by line.
function stableJson(v, depth = 0) {
  const ind = '  '.repeat(depth);
  if (Array.isArray(v)) {
    if (!v.length) return '[]';
    if (depth >= 2 || v.every((x) => x === null || typeof x !== 'object')) return JSON.stringify(v);
    return '[\n' + v.map((x) => ind + '  ' + stableJson(x, depth + 1)).join(',\n') + '\n' + ind + ']';
  }
  if (v && typeof v === 'object') {
    if (depth >= 2) return JSON.stringify(v);
    const e = Object.entries(v);
    if (!e.length) return '{}';
    return '{\n' + e.map(([k, x]) => ind + '  ' + JSON.stringify(k) + ': ' + stableJson(x, depth + 1)).join(',\n') + '\n' + ind + '}';
  }
  return JSON.stringify(v);
}

// Write a cache file; with --check, first compare it with the one an
// earlier run left (reproducibility).
function put(file, buf) {
  if (check && fs.existsSync(file)) {
    const old = fs.readFileSync(file);
    if (!old.equals(buf)) problems.push(`${path.relative(ROOT, file)}: differs from the previous run (sha256 ${sha(old).slice(0, 12)} → ${sha(buf).slice(0, 12)})`);
  }
  fs.writeFileSync(file, buf);
}

function golden(name, value) {
  const file = path.join(goldenDir, name);
  const text = stableJson(value) + '\n';
  if (check) {
    if (!fs.existsSync(file)) problems.push(`${path.relative(ROOT, file)}: no golden to compare with`);
    else if (fs.readFileSync(file, 'utf8') !== text) problems.push(`${path.relative(ROOT, file)}: differs from the golden`);
  } else {
    fs.writeFileSync(file, text);
  }
}

async function exportScene(game, what, name, opts) {
  const t0 = Date.now();
  const r = await game.eval(`window.__mrSceneExport.${what}(${JSON.stringify(opts)})`);
  const bin = await pull(game, r.binary_length);
  const file = sceneFile(r.header, bin);
  put(path.join(sceneDir, name + '.mrscene'), file);
  const digest = JSON.parse(r.digest);
  const text = Buffer.from(stableJson(digest) + '\n');
  put(path.join(sceneDir, name + '.digest.json'), text);
  return { digest, digestSha: sha(text), bytes: file.length, sha256: sha(file), ms: Date.now() - t0 };
}

async function dump(game, what, dir, name) {
  const index = await game.eval(`window.__mrSceneExport.${what}()`);
  const bin = await pull(game, index.binary_length);
  for (const a of index.arrays) a.sha256 = sha(bin.subarray(a.offset, a.offset + a.byte_length));
  put(path.join(dir, name + '.json'), Buffer.from(stableJson(index) + '\n'));
  put(path.join(dir, name + '.bin'), bin);
  return index;
}

// One level's page in a fresh Chrome. A fresh browser each time because
// canvas text drawn on the GPU can differ by a unit or two in a few pixels
// depending on what earlier pages drew (the GPU process's glyph caches),
// which would make one level's textures depend on the levels before it.
async function session(level, fn) {
  const browser = await launch();
  try {
    const t0 = Date.now();
    const query = `level=${level}&${QUERY}`;
    const game = await openGame(browser, { query, init: seedRandom, initArgs: [RANDOM_SEED] });
    const state = await game.eval(() => ({ id: window.__world?.level?.id, kernel: !!window.__parity?.kernel, freeze: !!window.__parity?.freeze }));
    if (state.id !== level || !state.kernel || !state.freeze) throw new Error(`${level}: page state ${JSON.stringify(state)}`);
    await game.eval(pageScript);
    const meta = { level, query, random_seed: RANDOM_SEED, three: await game.eval('window.__THREE.REVISION') };
    const row = await fn(game, meta);
    row.load_s = (Date.now() - t0) / 1000 - row.export_s;
    const errors = [...game.errors, ...(await game.eval('(window.__parity?.errors || []).map(String)'))];
    if (errors.length) problems.push(`${level}: page errors: ${errors.slice(0, 5).join(' | ')}`);
    await game.close();
    row.total_s = (Date.now() - t0) / 1000;
    console.log(`${row.scene}: ${row.mrscene_mb} MB, ${row.meshes} meshes, ${row.materials} materials, ${row.textures} textures, ${row.total_s.toFixed(1)} s (export ${row.export_s.toFixed(1)} s)`);
    return row;
  } finally {
    await browser.close();
  }
}

const sceneRow = (name, s) => ({ scene: name, export_s: s.ms / 1000, mrscene_mb: +(s.bytes / 1e6).toFixed(1), ...s.digest.counts });
const sums = (arrays) => Object.fromEntries(arrays.map((a) => [a.name, { component: a.component, length: a.length, sha256: a.sha256 }]));

const report = [];
for (const level of levels) {
  report.push(await session(level, async (game, meta) => {
    const name = base ? level + '.base' : level;
    const scene = await exportScene(game, 'exportWorld', name, { base, meta: { name, ...meta, base } });
    if (!base) {
      const dir = path.join(worldDir, level);
      fs.mkdirSync(dir, { recursive: true });
      const track = await dump(game, 'trackDump', dir, 'track');
      const terrain = await dump(game, 'terrainDump', dir, 'terrain');
      golden(level + '.json', {
        level,
        query: meta.query,
        track: { scalars: track.scalars, skipped: track.skipped, arrays: sums(track.arrays) },
        terrain: { count: terrain.count, along: terrain.along, bounds: terrain.bounds, arrays: sums(terrain.arrays) },
        // The full digest (~0.4 MB) stays in the cache beside the scene.
        scene: { counts: scene.digest.counts, kinds: scene.digest.kinds, digest_sha256: scene.digestSha },
      });
    }
    return sceneRow(name, scene);
  }));
}

// The models (cars, effects, pursuit props), always from a Sierra page: the
// spike strip is laid on its road.
if (withModels) {
  report.push(await session('sierra', async (game, meta) => {
    const m = await exportScene(game, 'exportModels', 'models', { meta: { name: 'models', ...meta } });
    golden('models.json', { query: meta.query, counts: m.digest.counts, kinds: m.digest.kinds, digest_sha256: m.digestSha });
    return sceneRow('models', m);
  }));
}

console.table(report.map((r) => ({ scene: r.scene, MB: r.mrscene_mb, nodes: r.nodes, meshes: r.meshes, materials: r.materials, textures: r.textures, vertices: r.vertices, load_s: +r.load_s.toFixed(1), export_s: +r.export_s.toFixed(1) })));
console.log(`scenes in ${path.relative(ROOT, sceneDir)}, dumps in ${path.relative(ROOT, worldDir)}`);
if (problems.length) {
  for (const p of problems) console.error('  ' + p);
  console.error(`scene-export: ${problems.length} problem(s)`);
  process.exit(1);
}
