// The golden for mp_worldgen's builders (roadmap WP 3.3): the JS builders
// themselves, imported from the game under Node with the parity kernel —
// valley/Builder.js (Builder, PaintBuilder), beach/ColorBuilder.js,
// city/geom.js (GeoBuilder, staticMesh, instanced, trs, yawOf), Road.js's
// extrude and run helpers over real level tracks — plus THREE.Color, the
// built-in materials' parameters as the scene export writes them, and the
// progress labels of World.build. crates/mp_worldgen/tests/builders.rs
// makes the same calls in the same order and requires the same bits; read
// the two side by side.
//
//   node --import=./tools/parity/kernel/register.mjs tools/parity/builders.mjs [--check]
//
// (or NODE_OPTIONS=--import=./tools/parity/kernel/register.mjs). Writes
// parity/golden/builders/builders.json. Geometries are recorded as in
// three-geom.mjs (attributes in order with the FNV-1a 64 of the typed
// array's bytes and the first values as hex bits, the index, groups and
// bounds); a mesh adds its name, flags, matrix and material; an instanced
// mesh its instance arrays and bounding sphere; numbers are sequences of
// doubles (hash and head). Material parameters are JSON as
// tools/parity/lib/scene-page.js writes them. --check regenerates the file
// in memory and fails if it would change.

import fs from 'node:fs';
import path from 'node:path';
import { ROOT } from './lib/jstree.mjs';
import { fnv1a64, hashHex } from './lib/trace.mjs';
import { kernelInstalled } from '../../src/parity/kernel.js';

if (!kernelInstalled()) {
  console.error('builders: run with --import=./tools/parity/kernel/register.mjs');
  process.exit(1);
}

// 'three' and 'three/addons/...' resolve to vendor/three, as in the game.
await import('../../test/unit/support/three.js');
const THREE = await import('three');
const { Builder, PaintBuilder } = await import('../../src/world/valley/Builder.js');
const { ColorBuilder } = await import('../../src/world/beach/ColorBuilder.js');
const { GeoBuilder, staticMesh, instanced, trs, yawOf } = await import('../../src/world/city/geom.js');
const { extrude, runs } = await import('../../src/world/Road.js');
const { Track } = await import('../../src/track/Track.js');
const { LEVELS } = await import('../../src/levels/index.js');

const OUT = path.join(ROOT, 'parity/golden/builders/builders.json');
const HEAD = 16;
const V3 = (x, y, z) => new THREE.Vector3(x, y, z);

// console.warn is what the builders log (a missing material); keep it.
const warnings = [];
console.warn = (...args) => warnings.push(args.join(' '));

// ── Recording (as three-geom.mjs) ───────────────────────────────────────

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

// A mesh a builder made: `mats` names the materials the case passed in;
// any other is recorded in full the first time it is seen.
function obj(o, mats, seen) {
  let mat = mats.get(o.material);
  if (mat === undefined) {
    if (!seen.has(o.material)) seen.set(o.material, { id: 'new' + seen.size, ...material(o.material) });
    mat = seen.get(o.material);
    mat = seen.get(o.material).id;
  }
  const out = {
    name: o.name, type: o.isInstancedMesh ? 'InstancedMesh' : o.type,
    cast: o.castShadow, receive: o.receiveShadow, auto: o.matrixAutoUpdate,
    matrix: o.matrix.elements.map(bits64), material: mat, geo: geo(o.geometry),
  };
  if (o.isInstancedMesh) {
    out.count = o.count;
    out.capacity = o.instanceMatrix.count;
    out.matrices = attr(o.instanceMatrix);
    out.colors = o.instanceColor ? attr(o.instanceColor) : null;
    out.bsphere = o.boundingSphere ? [...o.boundingSphere.center.toArray(), o.boundingSphere.radius].map(bits64) : null;
  }
  return out;
}

function meshes(list, matsObj) {
  const mats = new Map(Object.entries(matsObj).map(([k, m]) => [m, k]));
  const seen = new Map();
  const out = list.map((o) => obj(o, mats, seen));
  return { meshes: out, materials: [...seen.values()], warnings: warnings.splice(0) };
}

const cases = {};
const G = (name, make) => { cases[name] = geo(make()); };
const S = (name, make) => { cases[name] = seq(make()); };
const O = (name, make) => { cases[name] = make(); };

// A material for each bucket key: plain, the builders only pass it on.
const mat = () => new THREE.MeshStandardMaterial();

// ── THREE.Color ─────────────────────────────────────────────────────────

const HEXES = [0x000000, 0xffffff, 0x4a3a2c, 0xd9d0bc, 0x0a0b0c, 0x808080, 0x123456, 0xfedcba, 0x010203, 0x0b0b0b, 0x8a6a4a, 0xff7f00];
const rgb = (c) => [c.r, c.g, c.b];
S('color/hex', () => HEXES.flatMap((h) => rgb(new THREE.Color(h))));
S('color/hex-frac', () => [255.7, 0x123456 + 0.9, -1, 0x1ffffff].flatMap((h) => rgb(new THREE.Color().setHex(h))));
const HSL = [[0, 0, 0.5], [0.1, 0.6, 0.3], [0.55, 1, 0.7], [-0.2, 0.5, 0.5], [1.3, 0.4, 0.2], [0.9, 1.2, -0.1], [0.33, 0.25, 0.5], [0.66, 0.8, 0.9]];
S('color/hsl', () => HSL.flatMap(([h, s, l]) => rgb(new THREE.Color().setHSL(h, s, l))));
S('color/get-hsl', () => HEXES.flatMap((h) => { const o = new THREE.Color(h).getHSL({}); return [o.h, o.s, o.l]; }));
S('color/offset-hsl', () => HEXES.flatMap((h) => rgb(new THREE.Color(h).offsetHSL(0.05, -0.1, 0.02))));
S('color/get-hex', () => HEXES.map((h) => new THREE.Color(h).multiplyScalar(0.7).getHex()));
S('color/round-trip', () => HEXES.map((h) => new THREE.Color(h).getHex()));
const STYLES = ['#abc', '#a1b2c3', 'rgb(10,20,30)', 'rgb( 255 , 0 , 300 )', 'rgb(10%,50%,100%)', 'rgba(1,2,3,0.5)', 'hsl(120,50%,25%)', 'hsl(300.5, 20%, 75.5%)', '#12', 'nonsense'];
S('color/style', () => STYLES.flatMap((s) => rgb(new THREE.Color(0x336699).setStyle(s))));
S('color/ops', () => {
  const a = new THREE.Color(0x4a3a2c), b = new THREE.Color(0xd9d0bc);
  const out = [];
  out.push(...rgb(a.clone().lerp(b, 0.3)));
  out.push(...rgb(new THREE.Color().lerpColors(a, b, 0.65)));
  out.push(...rgb(a.clone().lerpHSL(b, 0.4)));
  out.push(...rgb(a.clone().multiply(b)));
  out.push(...rgb(a.clone().add(b).addScalar(-0.1)));
  out.push(...rgb(new THREE.Color(0.2, 0.5, 0.9).convertSRGBToLinear()));
  out.push(...rgb(new THREE.Color(0.2, 0.5, 0.9).convertLinearToSRGB()));
  out.push(...rgb(new THREE.Color().setScalar(0.3)));
  out.push(...rgb(new THREE.Color().setRGB(3.5, 2.4, 1.2).multiplyScalar(0.25 + 0.6)));
  return out;
});
O('color/hex-string', () => HEXES.map((h) => new THREE.Color(h).offsetHSL(0.02, 0, -0.05).getHexString()));

// ── Materials ───────────────────────────────────────────────────────────

O('material/defaults', () => ['MeshStandardMaterial', 'MeshPhysicalMaterial', 'MeshLambertMaterial', 'MeshBasicMaterial',
  'LineBasicMaterial', 'SpriteMaterial', 'PointsMaterial'].map((T) => material(new THREE[T]())));
O('material/made', () => {
  const out = [
    new THREE.MeshStandardMaterial({ color: 0x8a6a4a, roughness: 0.9, metalness: 0.1 }),
    new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.85 }),
    new THREE.MeshStandardMaterial({ color: '#a1b2c3', emissive: 0xffaa33, emissiveIntensity: 0.4, side: THREE.DoubleSide, transparent: true, opacity: 0.5, depthWrite: false }),
    new THREE.MeshStandardMaterial({ size: 3, flatShading: true }),
    new THREE.MeshBasicMaterial({ color: 0xffffff, fog: false, toneMapped: false }),
    new THREE.MeshLambertMaterial({ color: 0x335522, flatShading: true, alphaTest: 0.5 }),
    new THREE.PointsMaterial({ size: 2, sizeAttenuation: false, color: 0xffcc88, transparent: true, blending: THREE.AdditiveBlending }),
    new THREE.MeshPhysicalMaterial({ color: 0x112233, clearcoat: 1, clearcoatRoughness: 0.1, reflectivity: 0.5, sheenColor: 0x445566 }),
    new THREE.SpriteMaterial({ color: new THREE.Color(0.2, 0.4, 0.6), opacity: 0.8 }),
    new THREE.LineBasicMaterial({ color: 0x00ff00, linewidth: 2 }),
  ];
  warnings.splice(0);
  return out.map(material);
});

// ── Builder and PaintBuilder (valley/Builder.js) ────────────────────────

function piecesNoNormal() {
  // Non-indexed, position only, plus an attribute normalise() drops.
  const g = new THREE.BufferGeometry();
  g.setAttribute('position', new THREE.Float32BufferAttribute([0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 0.5, 0, 1, 0], 3));
  g.setAttribute('color', new THREE.Float32BufferAttribute(new Array(18).fill(0.5), 3));
  return g;
}

function drawBuilder(B) {
  B.box('wall', 4, 3, 0.2, 1, 0, 2);
  B.box('wall', 2, 1, 1, -1, 0.5, 0, 0.3, 0.1, -0.2);
  B.setFrame(10, 2, -5, 0.7);
  B.cbox('roof', 5, 0.2, 4, 0, 3, 0, 0.2, 0, 0);
  B.pushFrame(1, 0, 1, -0.4);
  B.put('post', new THREE.CylinderGeometry(0.1, 0.12, 2, 6), 0, 1, 0);
  B.put('post', new THREE.CylinderGeometry(0.1, 0.12, 2, 6), 0.5, 1, 0.5, 0.1, 0.2, 0.3, 1.5, 0.5, 2);
  B.beam('beam', V3(0, 0, 0), V3(1, 2, 3));
  B.beam('beam', V3(0, 0, 0), V3(0, 3, 0), 0.3);
  B.beam('beam', V3(0.5, 2, 0), V3(0.5, 0, 0));
  B.pushFrame(0, 3, 0, 1.1);
  B.box('wall', 1, 1, 1, 0, 0, 0, 2.5);
  B.popFrame();
  B.popFrame();
  B.add('misc', piecesNoNormal());
  B.add('misc', new THREE.TorusGeometry(1, 0.2, 4, 8), new THREE.Matrix4().makeTranslation(0, 5, 0));
  B.add('misc', new THREE.LatheGeometry([new THREE.Vector2(0, 0), new THREE.Vector2(1, 0.5), new THREE.Vector2(0.5, 1)], 5));
  B.setFrame(-3, 0, 4);
  B.put('tube', new THREE.TubeGeometry(new THREE.CatmullRomCurve3([V3(0, 0, 0), V3(1, 1, 0), V3(2, 0, 1)]), 8, 0.1, 4, false), 0, 0, 0);
  B.cbox('wall', 0.5, 0.5, 0.5, 0, 0, 0);
}

O('builder/build', () => {
  const B = new Builder();
  drawBuilder(B);
  const M = { wall: mat(), roof: mat(), post: mat(), beam: mat(), misc: mat() };
  return meshes(B.build(M, { castShadow: ['wall', 'post'] }), M);
});
O('builder/build-default-opts', () => {
  const B = new Builder();
  B.box('a', 1, 2, 3, 0, 0, 0);
  B.box('b', 1, 2, 3, 5, 0, 0, 0.5);
  const M = { a: mat(), b: mat() };
  return meshes(B.build(M), M);
});
G('builder/merge-all', () => {
  const B = new Builder();
  drawBuilder(B);
  return B.mergeAll();
});

O('paint/build', () => {
  const B = new PaintBuilder({ paint: ['p', 0x8a6a4a], trim: ['p', 0xeeeeee], roof: ['r', 0x553322] });
  B.box('paint', 4, 3, 0.2, 1, 0, 2);
  B.box('glass', 1, 1, 0.05, 1, 1, 2.1);
  B.setFrame(3, 0, 3, 0.25);
  B.box('trim', 4.2, 0.2, 0.3, 0, 3, 0);
  B.cbox('roof', 5, 0.2, 4, 0, 3.4, 0, 0.2, 0, 0);
  B.beam('trim', V3(0, 0, 0), V3(0.3, 2, -0.4), 0.1);
  B.put('glass', new THREE.PlaneGeometry(1, 2), 0, 1, 0, 0, Math.PI / 2, 0);
  B.box('paint', 2, 2, 2, -2, 0, 0, 0, 0, 0.3);
  const M = { p: mat(), r: mat(), glass: mat() };
  return meshes(B.build(M, { castShadow: true, receiveShadow: false }), M);
});
G('paint/merge-all', () => {
  // As Valley's millSailsGeometry: one painted bucket, merged for instancing.
  const B = new PaintBuilder({ wood: ['a', 0x4a3a2c], cloth: ['a', 0xd9d0bc] });
  for (let k = 0; k < 4; k++) {
    B.pushFrame(0, 0, 0, 0);
    B.box('wood', 0.2, 6, 0.1, 0, 0, 0, 0, 0, k * Math.PI / 2);
    B.cbox('cloth', 1.2, 4.5, 0.02, 0.7, 3, 0, 0, 0, k * Math.PI / 2);
    B.popFrame();
  }
  return B.mergeAll();
});
G('paint/merge-all-mixed', () => {
  const B = new PaintBuilder({ wood: ['a', 0x4a3a2c] });
  B.box('wood', 1, 1, 1, 0, 0, 0);
  B.box('bare', 1, 1, 1, 2, 0, 0);
  return B.mergeAll();
});

// ── ColorBuilder (beach/ColorBuilder.js) ────────────────────────────────

function drawColor(B) {
  B.box('wall', 6, 4, 5, 0, 0, 0, 0.1);
  B.box('trim', 6.2, 0.3, 5.2, 0, 4, 0, 0.1);
  B.box('glass', 1.2, 1.4, 0.05, 0, 1.2, 2.5, 0.1);
  B.channel = 'far';
  B.setFrame(20, 0, -10, 1.2);
  B.box('wall', 8, 6, 6, 0, 0, 0);
  B.cbox('roof', 8.4, 0.3, 6.4, 0, 6.15, 0);
  B.beam('trim', V3(-4, 6, -3), V3(4, 6, 3), 0.2);
  B.channel = 'near';
  B.setFrame(0, 0, 0);
  B.put('sign', new THREE.PlaneGeometry(3, 1), 0, 5, 2.6);
  B.box('roof', 6.4, 0.3, 5.4, 0, 4.3, 0, 0.1, 0.05, 0);
}
const PALETTE = { wall: 0xf2e8d5, trim: 0x2f6f8f, roof: 0xb5543a };

O('color-builder/build', () => {
  const B = new ColorBuilder(PALETTE);
  drawColor(B);
  const M = { glass: mat() };
  return meshes(B.build(M, { castShadow: ['glass'] }), M);
});
O('color-builder/build-solid', () => {
  const B = new ColorBuilder(PALETTE);
  drawColor(B);
  const M = { solid: mat(), glass: mat(), sign: mat() };
  return meshes(B.build(M, { castShadow: true }), M);
});

// ── GeoBuilder and helpers (city/geom.js) ───────────────────────────────

function drawGeo(g) {
  g.quad([0, 0, 0], [2, 0, 0], [2, 3, 0], [0, 3, 0], [[0, 0], [1, 0], [1, 1], [0, 1]], [0.5, 0.25, 0.125], 3);
  g.quad([1, 1, 1], [1, 1, 1], [3, 2, 1], [1, 4, 2]);
  g.quad([0, 0, 0], [1, 1, 1], [2, 2, 2], [3, 3, 3], null, null, 1);
  g.quad([0, 0, 0], [1, 0, 0], [2, 0, 0], [1, 0, 1]);
  g.tri([0, 5, 0], [1, 5, 1], [2, 5, 0], [0.9, 0.8, 0.7], 2);
  g.tri([0, 0, 0], [0, 0, 0], [1, 1, 1]);
  g.triUV([0, 1, 0], [1, 1, 0], [0, 1, 1], [0, 0], [1, 0], [0, 1], null, 4);
  g.triUV([0, 1, 0], [0, 1, 1], [1, 1, 0], [0, 0], [0, 1], [1, 0]);
  g.prism([[0, 0], [10, 0], [10, 8], [0, 8]], 0, 30);
  g.prism([[20, 0], [20, 6], [27, 9], [31, 3], [25, -2]], 2, 14.5, {
    tileW: 6, tileH: 3.5, uOff: 0.25, vOff: 0.5, vRef: 1, cell: 2, roofCell: 7, roofTile: 8, color: [0.3, 0.4, 0.5], roofColor: [0.6, 0.6, 0.6],
  });
  g.prism([[0, 20], [0, 26], [5, 26], [5, 20]], 0, 4, { roof: false, color: [0.1, 0.2, 0.3] });
  g.prism([[40, 0], [44, 0], [42, 3]], -1, 2, { vRef: 0 });
  g.box(5, 0, 5, 4, 3, 2, 0.6);
  g.box(-5, 1, 5, 2, 1, 6, -2.2, { bottom: true, color: [1, 0.5, 0], cell: 5 });
  g.box(0, 0, -9, 8, 2, 3, yawOf(1, 2), { tileW: 2, roof: false, bottom: true });
  g.box(3, 0, -3, 1, 1, 1, Math.PI / 3, { tileH: 0.5, uOff: 0.1 });
}

G('geo/color-cell', () => { const g = new GeoBuilder({ color: true, cell: true }); drawGeo(g); return g.build(); });
G('geo/plain', () => { const g = new GeoBuilder(); drawGeo(g); return g.build(); });
G('geo/color', () => { const g = new GeoBuilder({ color: true }); drawGeo(g); return g.build(); });
G('geo/empty', () => new GeoBuilder({ cell: true }).build());

const TRS = [
  [0, 0, 0], [1, 2, 3, 0.5], [1, 2, 3, -1.2, 2, 0.5, 3], [5, -1, 2, 0.3, 1, 1, 1, 0.2, -0.1], [0, 0, 0, Math.PI, 1, 1, 1, Math.PI / 2, 0.4],
];
S('trs', () => TRS.flatMap((a) => trs(...a).elements));
S('yaw-of', () => [[1, 0], [0, 1], [-1, 0], [0, -1], [3, 4], [-2, -0.5], [0, 0], [-0, -0]].map(([x, z]) => yawOf(x, z)));

O('instanced/some', () => {
  const g = new THREE.BoxGeometry(1, 2, 1);
  const m = mat();
  const im = instanced(g, m, [trs(1, 0, 2, 0.3), trs(-4, 1, 0, 1.2, 2, 1, 0.5), trs(10, 0, -3, -2, 1, 3, 1, 0.1, 0.2)], { cast: true });
  im.setColorAt(1, new THREE.Color(0x336699));
  im.setColorAt(2, new THREE.Color(0.25, 0.5, 2));
  return meshes([im], { m });
});
O('instanced/none', () => {
  const g = new THREE.CylinderGeometry(0.1, 0.1, 1, 6);
  const m = mat();
  return meshes([instanced(g, m, [], { receive: true })], { m });
});
O('instanced/same-centre', () => {
  // Two instances at one point: union keeps the larger radius.
  const g = new THREE.SphereGeometry(1, 6, 4);
  const m = mat();
  return meshes([instanced(g, m, [trs(1, 1, 1), trs(1, 1, 1, 0, 2, 2, 2)])], { m });
});
O('static-mesh', () => {
  const m = mat();
  const a = staticMesh(new THREE.BoxGeometry(1, 1, 1), m);
  const b = staticMesh(new THREE.BoxGeometry(2, 1, 1), m, { cast: true, receive: false, name: 'city:block' });
  return meshes([a, b], { m });
});

// ── Road.js: extrude and the run helpers ────────────────────────────────

const tracks = Object.fromEntries(['sierra', 'coast', 'cruise', 'desert'].map((id) => [id, new Track(LEVELS.find((l) => l.id === id))]));

const roadProfile = [{ lat: (f) => -f.hw }, { lat: 0 }, { lat: (f) => f.hw }];
G('extrude/sierra-surface', () => extrude(tracks.sierra, [[0, 560]], roadProfile, { vScale: 4 }));
G('extrude/coast-surface-end', () => extrude(tracks.coast, [[tracks.coast.length - 333.3, tracks.coast.length]], roadProfile, { vScale: 4 }));
for (const side of [-1, 1]) {
  const W = (f) => (side < 0 ? f.wallL : f.wallR);
  const prof = [
    { lat: (f) => side * f.hw, dy: -0.01, u: 0 },
    { lat: (f) => side * W(f), dy: -0.08, u: 0.5 },
    { lat: (f) => side * (W(f) + 2.5), dy: -1.6, u: 1 },
  ];
  if (side < 0) prof.reverse();
  G(`extrude/shoulder${side}`, () => extrude(tracks.sierra, [[1000, 1560]], prof, { vScale: 6, color: [0.45, 0.42, 0.33] }));
  const city = [
    { lat: (f) => side * f.hw, dy: 0, u: 0 },
    { lat: (f) => side * (W(f) + 0.35), dy: 0, u: 0.4 },
    { lat: (f) => side * (W(f) + 0.35), dy: -1.6, u: 1 },
  ];
  if (side < 0) city.reverse();
  G(`extrude/city${side}`, () => extrude(tracks.cruise, [[tracks.cruise.length - 300, tracks.cruise.length + 260]], city, { vScale: 6, color: [0.62, 0.62, 0.6] }));
}
const wave = (s) => Math.sin(s / 300) > 0.2;
S('runs/sierra', () => runs(tracks.sierra, wave).flat());
S('runs/step-range', () => runs(tracks.desert, (s) => Math.cos(s / 77) < -0.3, 1, 500.5, 2100).flat());
S('runs/open-end', () => runs(tracks.sierra, (s) => s > tracks.sierra.length - 50, 3).flat());
G('extrude/guardrail', () => {
  const side = 1;
  const prof = [
    { lat: (f) => side * (f.hw + (side < 0 ? f.wallL - f.hw : f.wallR - f.hw) + 0.15), dy: 0.48, u: 0 },
    { lat: (f) => side * (f.hw + (side < 0 ? f.wallL - f.hw : f.wallR - f.hw) + 0.1), dy: 0.64, u: 0.5 },
    { lat: (f) => side * (f.hw + (side < 0 ? f.wallL - f.hw : f.wallR - f.hw) + 0.15), dy: 0.8, u: 1 },
  ];
  return extrude(tracks.sierra, runs(tracks.sierra, wave).slice(0, 4), prof, { step: 2, vScale: 4 });
});
G('extrude/mixed', () => extrude(tracks.desert, [[100, 100.3], [200, 263.7], [263.7, 264], [900.25, 1001]], [
  { lat: -6, dy: 0.5, gapAfter: true },
  { lat: (f) => -f.hw, dy: (f, s) => 0.1 * Math.sin(s / 10), abs: false },
  { lat: (f) => f.hw * 0.5, abs: true, dy: (f) => f.y + 2, u: 0.75 },
  { lat: 7.5, dy: 0 },
], { step: 3, vScale: 2.4, color: (f, s, p) => [p / 4, f.hw / 10, s / 1000] }));
G('extrude/runout', () => extrude(tracks.sierra, [[tracks.sierra.length - 20, tracks.sierra.length + 90]], roadProfile, { step: 5 }));
G('extrude/none', () => extrude(tracks.sierra, [[10, 10.2]], roadProfile));

// ── World.build's progress labels ───────────────────────────────────────

// Each scenery class sets `label` as a field or in its constructor.
function sceneryLabel(name) {
  const src = fs.readFileSync(path.join(ROOT, 'src/world', name + '.js'), 'utf8');
  const m = /(?:this\.)?label = '([^']*)';/.exec(src);
  return m ? m[1] : undefined;
}
O('world/progress', () => {
  const out = {};
  for (const L of LEVELS) {
    if (L.id === 'seaside') continue; // needs its survey data (D54); same flow
    const names = [...new Set(L.zones.map((z) => z.scenery).filter(Boolean))];
    const steps = [['Surveying the route', 0.02], ['Shaping the land', 0.06], ['Sculpting terrain', 0.1], ['Paving roads', 0.67]];
    if (L.sea) steps.push(['Filling the sea', 0.7]);
    let k = 0;
    for (const n of names) steps.push([sceneryLabel(n) || 'Building scenery', 0.72 + (k++ / names.length) * 0.26]);
    steps.push(['Ready', 1]);
    out[L.id] = steps.map(([l, f]) => [l, bits64(f)]);
  }
  return out;
});

// ── Write ───────────────────────────────────────────────────────────────

const out = {
  note: 'Generated by tools/parity/builders.mjs with the parity kernel on (three.js r' + THREE.REVISION + '). Do not edit.',
  cases,
};
const text = JSON.stringify(out, null, 1) + '\n';
if (process.argv.includes('--check')) {
  const old = fs.readFileSync(OUT, 'utf8');
  if (old !== text) {
    console.error('builders: regenerated file differs from', path.relative(ROOT, OUT));
    process.exit(1);
  }
  console.log('builders: identical');
} else {
  fs.mkdirSync(path.dirname(OUT), { recursive: true });
  fs.writeFileSync(OUT, text);
  console.log('builders: wrote', path.relative(ROOT, OUT), `(${Object.keys(cases).length} cases, ${text.length} bytes)`);
}
