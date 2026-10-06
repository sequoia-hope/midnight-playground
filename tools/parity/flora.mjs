// The golden for mp_worldgen::flora (roadmap WP 3.6): the game's own
// src/world/valley/flora.js under Node with the parity kernel, over every
// call the scenery makes (Mountain, Valley, Raceway) and the defaults and
// edges besides: 3-D noise samples, rocks (every option), conifers (both
// kinds at every detail level), canopies (every kind at both levels),
// grass, flowers, shrubs and the foliage material.
// crates/mp_worldgen/tests/flora.rs makes the same calls and requires the
// same bits.
//
//   node --import=./tools/parity/kernel/register.mjs tools/parity/flora.mjs [--check]
//
// Writes parity/golden/flora/flora.json; geometries, sequences and
// materials are recorded as builders.mjs records them.

import fs from 'node:fs';
import path from 'node:path';
import { ROOT } from './lib/jstree.mjs';
import { fnv1a64, hashHex } from './lib/trace.mjs';
import { kernelInstalled } from '../../src/parity/kernel.js';

if (!kernelInstalled()) {
  console.error('flora: run with --import=./tools/parity/kernel/register.mjs');
  process.exit(1);
}

await import('../../test/unit/support/three.js');
const THREE = await import('three');
const F = await import('../../src/world/valley/flora.js');

const OUT = path.join(ROOT, 'parity/golden/flora/flora.json');
const HEAD = 16;

const f64 = new Float64Array(1);
const u32 = new Uint32Array(f64.buffer);
const bits64 = (x) => { f64[0] = x; return u32[1].toString(16).padStart(8, '0') + u32[0].toString(16).padStart(8, '0'); };
const f32 = new Float32Array(1);
const u32b = new Uint32Array(f32.buffer);
const bits32 = (x) => { f32[0] = x; return u32b[0].toString(16).padStart(8, '0'); };

const TYPES = new Map([
  [Float32Array, 'f32'], [Float64Array, 'f64'], [Uint8Array, 'u8'], [Uint16Array, 'u16'],
  [Uint32Array, 'u32'], [Int8Array, 'i8'], [Int16Array, 'i16'], [Int32Array, 'i32'],
]);

function seq(values) {
  values = values.map((v) => (Number.isNaN(v) ? NaN : v));
  const bytes = new Uint8Array(new Float64Array(values).buffer);
  return { n: values.length, hash: hashHex(fnv1a64(bytes)), head: values.slice(0, HEAD).map(bits64) };
}

function attr(a) {
  const arr = a.array;
  const type = TYPES.get(arr.constructor);
  const bytes = new Uint8Array(arr.buffer, arr.byteOffset, arr.byteLength);
  const head = Array.from(arr.slice(0, HEAD), (v) => (type === 'f32' ? bits32(v) : type === 'f64' ? bits64(v) : v));
  return { itemSize: a.itemSize, type, normalized: a.normalized, n: arr.length, hash: hashHex(fnv1a64(bytes)), head };
}

function geo(g) {
  if (g === null) return { null: true };
  const out = { attrs: Object.entries(g.attributes).map(([name, a]) => ({ name, ...attr(a) })) };
  if (g.index) {
    const arr = g.index.array;
    const vals = new Uint32Array(arr.length);
    vals.set(arr);
    out.index = {
      type: TYPES.get(arr.constructor), n: arr.length,
      hash: hashHex(fnv1a64(new Uint8Array(vals.buffer))), head: Array.from(vals.slice(0, HEAD)),
    };
  } else {
    out.index = null;
  }
  out.groups = g.groups.map((x) => [x.start, x.count, x.materialIndex]);
  if (g.boundingBox) out.bbox = [...g.boundingBox.min.toArray(), ...g.boundingBox.max.toArray()].map(bits64);
  if (g.boundingSphere) out.bsphere = [...g.boundingSphere.center.toArray(), g.boundingSphere.radius].map(bits64);
  return out;
}

// Material parameters as scene-page.js writes them.
const num = (v) => (Number.isFinite(v) ? v : { num: String(v) });
function value(v) {
  if (v === null || v === undefined) return null;
  const t = typeof v;
  if (t === 'number') return num(v);
  if (t === 'boolean' || t === 'string') return v;
  if (t === 'function') return undefined;
  if (v.isColor) return { color: [v.r, v.g, v.b] };
  if (v.isVector2 || v.isVector3 || v.isVector4 || v.isQuaternion) return { vec: v.toArray().map(num) };
  if (v.isMatrix3 || v.isMatrix4) return { mat: [...v.elements].map(num) };
  if (v.isEuler) return { euler: [v.x, v.y, v.z, v.order] };
  if (Array.isArray(v)) return v.map((x) => value(x) ?? null);
  if (t === 'object') {
    const out = {};
    for (const [k, x] of Object.entries(v)) { const c = value(x); if (c !== undefined) out[k] = c; }
    return out;
  }
  throw new Error('cannot serialise a ' + t);
}
function material(m) {
  const params = {};
  for (const key of Object.keys(m)) {
    if (['uuid', 'id', 'name', 'type', 'version', 'userData', '_listeners', 'uniforms', 'uniformsGroups', 'vertexShader', 'fragmentShader'].includes(key)) continue;
    if (/^is[A-Z]/.test(key)) continue;
    const v = value(m[key]);
    if (v !== undefined) params[key.replace(/^_/, '')] = v;
  }
  return { type: m.type, params };
}

const cases = {};
const G = (name, make) => { cases[name] = geo(make()); };
const S = (name, make) => { cases[name] = seq(make()); };
const O = (name, make) => { cases[name] = make(); };

// ── makeNoise3D ─────────────────────────────────────────────────────────
for (const seed of [1, 3, 9, 21, 44]) {
  S('noise3d/' + seed, () => {
    const n = F.makeNoise3D(seed);
    const out = [];
    for (let k = 0; k < 400; k++) {
      const t = k * 0.6180339887498949;
      out.push(n(t * 3.7 - 40, (k % 23) * 0.41 - 4.7, (k % 17) * 1.37 - 5.5));
    }
    out.push(n(0, 0, 0), n(-0.5, 255.5, 256.25), n(1e10 + 0.3, -3e9, Math.pow(2, 31) + 0.7));
    return out;
  });
}

// ── rockGeometry ────────────────────────────────────────────────────────
G('rock/default', () => F.rockGeometry(7));
G('rock/mtn-big-0', () => F.rockGeometry(3, 2, { crag: true }));
G('rock/mtn-big-1', () => F.rockGeometry(17, 2, { crag: true, squash: 0.8 }));
G('rock/mtn-far', () => F.rockGeometry(5, 1, { crag: true }));
G('rock/mtn-small', () => F.rockGeometry(9, 0, { squash: 0.8, lichen: 0.4 }));
G('rock/mtn-pool', () => F.rockGeometry(31, 1, { lichen: 1.6 }));
G('rock/tint', () => F.rockGeometry(12, 1, { tint: [0.7, 0.5, 0.4], lichen: 0 }));

// ── coniferGeometry ─────────────────────────────────────────────────────
G('conifer/default', () => F.coniferGeometry());
for (const kind of ['spruce', 'fir']) for (const lod of [0, 1, 2]) {
  G(`conifer/${kind}-${lod}`, () => F.coniferGeometry(kind, lod, 11 + lod));
}
G('conifer/mtn-fir', () => F.coniferGeometry('fir', 0, 12));
G('conifer/mtn-mid', () => F.coniferGeometry('spruce', 1, 13));
G('conifer/mtn-far', () => F.coniferGeometry('spruce', 2, 14));

// ── canopyGeometry ──────────────────────────────────────────────────────
G('canopy/default', () => F.canopyGeometry());
for (const kind of ['orchard', 'poplar', 'shade', 'willow']) for (const lod of [0, 1]) {
  G(`canopy/${kind}-${lod}`, () => F.canopyGeometry(kind, 5, lod));
}
G('canopy/valley-orchard', () => F.canopyGeometry('orchard', 5));
G('canopy/valley-poplar', () => F.canopyGeometry('poplar', 6));
G('canopy/valley-shade', () => F.canopyGeometry('shade', 7));
G('canopy/valley-willow', () => F.canopyGeometry('willow', 8));
G('canopy/valley-shade-far', () => F.canopyGeometry('shade', 7, 1));

// ── Ground cover ────────────────────────────────────────────────────────
G('grass/default', () => F.grassClumpGeometry());
G('grass/mtn', () => F.grassClumpGeometry(11, 5));
G('grass/valley', () => F.grassClumpGeometry(11, 15));
G('flower/default', () => F.flowerGeometry());
G('flower/mtn', () => F.flowerGeometry(5, 9));
G('flower/valley', () => F.flowerGeometry(6, 19));
G('shrub/default', () => F.shrubGeometry());
G('shrub/hedge', () => F.shrubGeometry(44, 0));
G('shrub/detail-2', () => F.shrubGeometry(5, 2));

// ── foliageMaterial ─────────────────────────────────────────────────────
O('foliage/default', () => material(F.foliageMaterial()));
O('foliage/front', () => material(F.foliageMaterial({ side: THREE.FrontSide })));
O('foliage/front-rough', () => material(F.foliageMaterial({ side: THREE.FrontSide, roughness: 0.95 })));
O('foliage/rough', () => material(F.foliageMaterial({ roughness: 0.95 })));

// ── Write ───────────────────────────────────────────────────────────────

const out = {
  note: 'Generated by tools/parity/flora.mjs with the parity kernel on (three.js r' + THREE.REVISION + '). Do not edit.',
  cases,
};
const text = JSON.stringify(out, null, 1) + '\n';
if (process.argv.includes('--check')) {
  const old = fs.readFileSync(OUT, 'utf8');
  if (old !== text) {
    console.error('flora: regenerated file differs from', path.relative(ROOT, OUT));
    process.exit(1);
  }
  console.log('flora: identical');
} else {
  fs.mkdirSync(path.dirname(OUT), { recursive: true });
  fs.writeFileSync(OUT, text);
  console.log('flora: wrote', path.relative(ROOT, OUT), `(${Object.keys(cases).length} cases, ${text.length} bytes)`);
}
