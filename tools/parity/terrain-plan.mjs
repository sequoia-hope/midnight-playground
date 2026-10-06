// What the scenery tells the terrain before its heights are final (roadmap
// WP 3.4): every level's flattens, carves and Desert's railway bed, as the
// scenery modules' plan() registers them in World.build, so that
// mp_worldgen's Terrain can be held to the JS heights (L2) and the JS
// terrain mesh (L3) before the scenery itself is ported (WP 3.6 on).
//
//   NODE_OPTIONS=--import=./tools/parity/kernel/register.mjs \
//     node tools/parity/terrain-plan.mjs [--check]
//
// Runs World.build's first two stages under Node with the parity kernel:
// the Track, `new Terrain`, each scenery module made as loadScenery makes it
// and its plan() run (a failing one dropped, as World.build drops it), then
// buildFields() and resolveFlattens(). The browser's Math.random is seeded
// for the scene export (D20); it is seeded the same way here, though no
// plan() draws from it.
//
// Writes parity/golden/terrain/<level>.json: the plan as hex f64 bits, in
// registration order, plus the SHA-256 of the heights at the 10,000 points
// of the world golden (tools/parity/lib/scene-page.js terrainDump, same
// points), which must equal parity/golden/world/<level>.json's: that proves
// the plan recorded here is the one the game's world had. When the cache
// holds the browser's dump, every height is also compared with it.
//
// It also records a digest of the terrain meshes of the browser's scene
// export (L3), when the cache has the export; without it the committed
// value is kept.
//
// --check regenerates in memory and fails if a file would change.

import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { ROOT, jsTreeKey } from './lib/jstree.mjs';
import { seedRandom, RANDOM_SEED } from './lib/seed-random.mjs';
import { kernelInstalled } from '../../src/parity/kernel.js';

if (!kernelInstalled()) {
  console.error('terrain-plan: run with NODE_OPTIONS=--import=./tools/parity/kernel/register.mjs');
  process.exit(1);
}
seedRandom(RANDOM_SEED);

await import('../../test/unit/support/three.js');
const { LEVELS } = await import('../../test/unit/support/levels.js');
const { Track } = await import('../../src/track/Track.js');
const { Terrain } = await import('../../src/world/Terrain.js');

const check = process.argv.includes('--check');
const OUT = path.join(ROOT, 'parity/golden/terrain');
const IDS = ['sierra', 'coast', 'streets', 'desert', 'seaside', 'cruise'];

const f64 = new Float64Array(1);
const u32 = new Uint32Array(f64.buffer);
const bits = (x) => { f64[0] = x; return u32[1].toString(16).padStart(8, '0') + u32[0].toString(16).padStart(8, '0'); };
const sha = (buf) => createHash('sha256').update(buf).digest('hex');

// World.loadScenery and the plan stage of World.build.
async function plan(level) {
  const track = new Track(level);
  const terrain = new Terrain(track, level);
  const world = { level, track, terrain, zoneIndex: (key) => level.zones.findIndex((z) => z.key === key) };
  const names = [...new Set(level.zones.map((z) => z.scenery).filter(Boolean))];
  const kept = [], failed = [];
  for (const name of names) {
    const mod = await import(`../../src/world/${name}.js`);
    const zone = level.zones.findIndex((z) => z.scenery === name);
    const s = new mod.default({ zone, key: level.zones[zone].key, level });
    try { s.plan?.(world); kept.push(name); } catch (e) { failed.push(`${name}: ${e.message}`); }
  }
  terrain.buildFields();
  terrain.resolveFlattens();
  return { track, terrain, kept, failed };
}

// tools/parity/lib/scene-page.js terrainDump, point for point.
function points(t, T, total = 10000, along = 6000) {
  const xz = new Float64Array(total * 2), h = new Float64Array(total);
  const halton = (i, b) => { let f = 1, r = 0; while (i > 0) { f /= b; r += f * (i % b); i = Math.floor(i / b); } return r; };
  const p = {};
  for (let i = 0; i < total; i++) {
    let x, z;
    if (i < along) {
      const s = ((i + 0.5) / along) * t.length;
      const g = (i * 0.6180339887498949) % 1;
      t.pointAt(s, -60 + 120 * g, p);
      x = p.x; z = p.z;
    } else {
      const k = i - along + 1;
      x = T.minX + (T.maxX - T.minX) * halton(k, 2);
      z = T.minZ + (T.maxZ - T.minZ) * halton(k, 3);
    }
    xz[i * 2] = x; xz[i * 2 + 1] = z;
    h[i] = T.heightAt(x, z);
  }
  return { xz, h };
}

// The digest of the terrain meshes in the cached scene export (the base
// export if there is one; the meshes under the group `terrain`), as one
// SHA-256 over a line per mesh: vertex and index counts, each attribute's
// name and SHA-256 in order, the index's SHA-256. mp_worldgen's
// tests/terrain.rs makes the same lines from mp_scene's digest of its own
// meshes, so the L3 gate runs without the cache (and in wasm).
function sceneMeshes(id) {
  const dir = path.join(ROOT, 'parity/cache', jsTreeKey(), 'scenes');
  for (const name of [id + '.base', id]) {
    const scene = path.join(dir, name + '.mrscene'), dig = path.join(dir, name + '.digest.json');
    if (!fs.existsSync(scene) || !fs.existsSync(dig)) continue;
    const fd = fs.openSync(scene, 'r');
    const head = Buffer.alloc(16);
    fs.readSync(fd, head, 0, 16, 0);
    const json = Buffer.alloc(head.readUInt32LE(12));
    fs.readSync(fd, json, 0, json.length, 16);
    fs.closeSync(fd);
    const H = JSON.parse(json.toString('utf8'));
    const D = JSON.parse(fs.readFileSync(dig, 'utf8'));
    const group = H.nodes.find((n) => n.name === 'terrain');
    const lines = group.children.map((c) => {
      const m = D.meshes[H.nodes[c].mesh];
      return `${m.vertices} ${m.indices} ${Object.entries(m.attributes).map(([k, v]) => k + '=' + v).join(',')} ${m.index}`;
    });
    return {
      count: lines.length,
      vertices: group.children.reduce((a, c) => a + D.meshes[H.nodes[c].mesh].vertices, 0),
      sha256: sha(Buffer.from(lines.join('\n'))),
    };
  }
  return null;
}

const bytes = (a) => Buffer.from(a.buffer, a.byteOffset, a.byteLength);
let problems = 0;
fs.mkdirSync(OUT, { recursive: true });
for (const id of IDS) {
  const level = LEVELS.find((l) => l.id === id);
  const { track, terrain: T, kept, failed } = await plan(level);
  const { xz, h } = points(track, T);
  const golden = JSON.parse(fs.readFileSync(path.join(ROOT, 'parity/golden/world', id + '.json'), 'utf8'));
  const ga = golden.terrain.arrays;
  const xzOk = sha(bytes(xz)) === ga.xz.sha256, hOk = sha(bytes(h)) === ga.height.sha256;
  let cache = 'no cache';
  const bin = path.join(ROOT, 'parity/cache', jsTreeKey(), 'world', id, 'terrain.bin');
  if (fs.existsSync(bin)) {
    const b = fs.readFileSync(bin);
    const ref = new Float64Array(b.buffer.slice(b.byteOffset + 160000, b.byteOffset + 240000));
    let diff = 0, first = -1;
    for (let i = 0; i < h.length; i++) if (!Object.is(ref[i], h[i])) { diff++; if (first < 0) first = i; }
    cache = diff ? `${diff} heights differ from the browser's dump (first at point ${first})` : 'every height equals the browser dump';
  }
  console.log(`${id}: scenery ${kept.join(', ')}${failed.length ? ' (failed: ' + failed.join('; ') + ')' : ''}; ${T.flattens.length} flattens, ${T.carves.length} carves; points ${xzOk ? 'ok' : 'DIFFER'}, heights ${hOk ? 'ok' : 'DIFFER'}; ${cache}`);
  if (!xzOk || !hOk) problems++;
  const out = {
    note: 'Generated by tools/parity/terrain-plan.mjs with the parity kernel on. Do not edit. Numbers are hex f64 bits.',
    level: id,
    scenery: kept,
    failed,
    flattens: T.flattens.map((f) => ({
      x: bits(f.x), z: bits(f.z), r: bits(f.r), falloff: bits(f.falloff),
      y: f.y === null ? null : bits(f.y),
      ...(f.yResolved !== undefined ? { yResolved: bits(f.yResolved) } : {}),
    })),
    carves: T.carves.map((c) => ({
      points: c.points.map((p) => [bits(p.x), bits(p.z)]),
      width: bits(c.width), depth: bits(c.depth), underRoad: c.underRoad,
    })),
    desertRail: T.desertRail ? Object.fromEntries(['lat', 'half', 'drop', 's0', 's1'].map((k) => [k, bits(T.desertRail[k])])) : null,
    heights: { xz_sha256: ga.xz.sha256, height_sha256: ga.height.sha256 },
    meshes: null,
  };
  const file = path.join(OUT, id + '.json');
  // The terrain meshes of the browser's scene export, when the cache has
  // it; otherwise what the committed golden already says.
  const meshes = sceneMeshes(id);
  if (meshes) out.meshes = meshes;
  else if (fs.existsSync(file)) out.meshes = JSON.parse(fs.readFileSync(file, 'utf8')).meshes ?? null;
  console.log(`  terrain meshes: ${out.meshes ? `${out.meshes.count} meshes, ${out.meshes.vertices} vertices` : 'none recorded'}${meshes ? ' (from the cached export)' : ''}`);
  const text = JSON.stringify(out, null, 1) + '\n';
  if (check) {
    if (!fs.existsSync(file) || fs.readFileSync(file, 'utf8') !== text) { console.error(`${path.relative(ROOT, file)}: would change`); problems++; }
  } else fs.writeFileSync(file, text);
}
if (problems) { console.error(`terrain-plan: ${problems} problem(s)`); process.exit(1); }
