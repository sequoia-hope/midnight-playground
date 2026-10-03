// The golden for mr_worldgen::three_geom (roadmap WP 3.1, SPEC 5.2): every
// three.js r180 generator the world code uses, over a spread of parameters
// (the world code's own calls among them, and the edges: few segments,
// partial sweeps, open ends, bevel on and off, fractional counts), plus
// Shape/Path, triangulateShape, CatmullRomCurve3, the geometry transforms
// and normals, mergeGeometries and mergeVertices, taken with the parity
// kernel on. crates/mr_worldgen/tests/three_geom.rs builds the same cases in
// the same order and requires the same bits; read the two side by side.
//
//   NODE_OPTIONS=--import=./tools/parity/kernel/register.mjs \
//     node tools/parity/three-geom.mjs [--check]
//
// Writes parity/golden/three_geom/three_geom.json. A geometry is recorded as
// its attributes in order (name, item size, array type, count, FNV-1a 64 of
// the typed array's little-endian bytes, the first values as hex bits), its
// index (array type, values hashed as little-endian u32, the first values),
// its groups, and its bounds if computed. Other results (curve samples,
// triangulations, matrices) are sequences of doubles stored as in the math
// golden (math-golden.mjs). --check regenerates the file in memory and fails
// if it would change.

import fs from 'node:fs';
import path from 'node:path';
import { ROOT } from './lib/jstree.mjs';
import { fnv1a64, hashHex } from './lib/trace.mjs';
import { kernelInstalled } from '../../src/parity/kernel.js';
import { mulberry32 } from '../../src/util/math.js';

if (!kernelInstalled()) {
  console.error('three-geom: run with NODE_OPTIONS=--import=./tools/parity/kernel/register.mjs');
  process.exit(1);
}

// 'three' and 'three/addons/...' resolve to vendor/three, as in the game.
await import('../../test/unit/support/three.js');
const THREE = await import('three');
const { mergeGeometries, mergeVertices } = await import('three/addons/utils/BufferGeometryUtils.js');

const OUT = path.join(ROOT, 'parity/golden/three_geom/three_geom.json');
const HEAD = 16;
const PI = Math.PI;
const V2 = (x, y) => new THREE.Vector2(x, y);
const V3 = (x, y, z) => new THREE.Vector3(x, y, z);

// ── Recording ───────────────────────────────────────────────────────────

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

const cases = {};
const G = (name, make) => { cases[name] = geo(make()); };
const S = (name, make) => { cases[name] = seq(make()); };

// ── Generators ──────────────────────────────────────────────────────────

G('box/default', () => new THREE.BoxGeometry(1, 1, 1, 1, 1, 1));
G('box/sized', () => new THREE.BoxGeometry(2, 3, 4, 1, 1, 1));
G('box/segs222', () => new THREE.BoxGeometry(2, 2, 2, 2, 2, 2));
G('box/odd', () => new THREE.BoxGeometry(1.5, 0.25, 3, 2.7, 1, 3.2));
G('box/negative', () => new THREE.BoxGeometry(-1, 2, 0.5, 1, 2, 1));

G('plane/default', () => new THREE.PlaneGeometry(1, 1, 1, 1));
G('plane/grid', () => new THREE.PlaneGeometry(2, 3, 4, 5));
G('plane/strip', () => new THREE.PlaneGeometry(10, 1, 1, 3));
G('plane/frac', () => new THREE.PlaneGeometry(1, 1, 2.5, 1.5));

G('circle/default', () => new THREE.CircleGeometry(1, 32, 0, 2 * PI));
G('circle/hex', () => new THREE.CircleGeometry(2, 6, 0, 2 * PI));
G('circle/quarter', () => new THREE.CircleGeometry(1, 3, 0, PI / 2));
G('circle/min', () => new THREE.CircleGeometry(1, 2, 0, 2 * PI));
G('circle/arc', () => new THREE.CircleGeometry(0.5, 16, 1, 4));

G('cylinder/default', () => new THREE.CylinderGeometry(1, 1, 1, 32, 1, false, 0, 2 * PI));
G('cylinder/post', () => new THREE.CylinderGeometry(0.1, 0.2, 2, 8, 1, false, 0, 2 * PI));
G('cylinder/open', () => new THREE.CylinderGeometry(0.5, 0.5, 1, 6, 1, true, 0, 2 * PI));
G('cylinder/topzero', () => new THREE.CylinderGeometry(0, 1, 2, 12, 3, false, 0, 2 * PI));
G('cylinder/bottomzero', () => new THREE.CylinderGeometry(1, 0, 2, 5, 2, false, 0, 2 * PI));
G('cylinder/arc', () => new THREE.CylinderGeometry(1, 1, 1, 8, 2, false, 0.3, PI));
G('cylinder/tri', () => new THREE.CylinderGeometry(0.3, 0.3, 1, 3, 1, false, 0, 2 * PI));
G('cylinder/frac', () => new THREE.CylinderGeometry(0.4, 0.6, 1.5, 7.6, 2.2, false, 0, 2 * PI));

G('cone/default', () => new THREE.ConeGeometry(1, 1, 32, 1, false, 0, 2 * PI));
G('cone/six', () => new THREE.ConeGeometry(0.5, 2, 6, 1, false, 0, 2 * PI));
G('cone/open', () => new THREE.ConeGeometry(1, 1, 4, 3, true, 0, 2 * PI));
G('cone/half', () => new THREE.ConeGeometry(2, 1, 8, 1, false, 0, PI));

G('sphere/default', () => new THREE.SphereGeometry(1, 32, 16, 0, 2 * PI, 0, PI));
G('sphere/low', () => new THREE.SphereGeometry(1, 8, 6, 0, 2 * PI, 0, PI));
G('sphere/min', () => new THREE.SphereGeometry(2, 3, 2, 0, 2 * PI, 0, PI));
G('sphere/halfphi', () => new THREE.SphereGeometry(1, 12, 8, 0, PI, 0, PI));
G('sphere/hemi', () => new THREE.SphereGeometry(1, 10, 6, 0, 2 * PI, 0, PI / 2));
G('sphere/band', () => new THREE.SphereGeometry(1, 10, 6, 0, 2 * PI, PI / 4, PI / 2));
G('sphere/frac', () => new THREE.SphereGeometry(0.5, 7.5, 4.2, 0, 2 * PI, 0, PI));

G('icosahedron/0', () => new THREE.IcosahedronGeometry(1, 0));
G('icosahedron/1', () => new THREE.IcosahedronGeometry(1, 1));
G('icosahedron/2', () => new THREE.IcosahedronGeometry(2, 2));
G('icosahedron/3', () => new THREE.IcosahedronGeometry(0.5, 3));
G('octahedron/0', () => new THREE.OctahedronGeometry(0.28, 0));
G('octahedron/1', () => new THREE.OctahedronGeometry(1, 1));
G('dodecahedron/0', () => new THREE.DodecahedronGeometry(1, 0));
G('dodecahedron/rock', () => new THREE.DodecahedronGeometry(0.62, 0));
G('dodecahedron/1', () => new THREE.DodecahedronGeometry(1, 1));
G('tetrahedron/0', () => new THREE.TetrahedronGeometry(1, 0));
G('tetrahedron/2', () => new THREE.TetrahedronGeometry(1, 2));

G('torus/default', () => new THREE.TorusGeometry(1, 0.4, 12, 48, 2 * PI));
G('torus/thin', () => new THREE.TorusGeometry(1, 0.2, 6, 12, 2 * PI));
G('torus/arc', () => new THREE.TorusGeometry(2, 0.5, 3, 8, PI));
G('torus/quarter', () => new THREE.TorusGeometry(0.5, 0.1, 8, 24, PI / 2));

G('capsule/default', () => new THREE.CapsuleGeometry(1, 1, 4, 8, 1));
G('capsule/streets', () => new THREE.CapsuleGeometry(0.19, 0.85, 3, 6, 1));
G('capsule/coast', () => new THREE.CapsuleGeometry(0.28, 2.3, 3, 8, 1));
G('capsule/valley', () => new THREE.CapsuleGeometry(0.4, 0.95, 3, 8, 1));
G('capsule/flat', () => new THREE.CapsuleGeometry(1, 0, 2, 5, 1));
G('capsule/segs', () => new THREE.CapsuleGeometry(0.5, 1, 1, 3, 3));
G('capsule/frac', () => new THREE.CapsuleGeometry(0.5, -1, 2.5, 4.9, 1.5));

// Lathe profiles: three's default, Valley's hay bale, a CarModel-style
// partial lathe, a clamped phiLength, and a random profile.
const baleProfile = () => {
  const R = 0.78, L = 1.25, prof = [];
  for (let k = 0; k <= 4; k++) prof.push(V2((k / 4) * (R - 0.12), L / 2));
  for (let k = 1; k <= 2; k++) { const a = (k / 2) * PI / 2; prof.push(V2(R - 0.12 + Math.sin(a) * 0.12, L / 2 - 0.12 + Math.cos(a) * 0.12)); }
  const n0 = prof.length;
  for (let k = n0 - 1; k >= 0; k--) prof.push(V2(prof[k].x, -prof[k].y));
  return prof;
};
const randomProfile = () => {
  const r = mulberry32(31), prof = [];
  for (let k = 0; k < 12; k++) prof.push(V2(0.05 + r() * 0.3, k * 0.1 + r() * 0.05));
  return prof;
};
const defaultLathe = () => [V2(0, -0.5), V2(0.5, 0), V2(0, 0.5)];
G('lathe/default', () => new THREE.LatheGeometry(defaultLathe(), 12, 0, 2 * PI));
G('lathe/bale', () => new THREE.LatheGeometry(baleProfile(), 11, 0, 2 * PI));
G('lathe/partial', () => new THREE.LatheGeometry([V2(0.3, -0.1), V2(0.32, 0), V2(0.3, 0.1), V2(0.1, 0.12)], 9, 0.5, PI));
G('lathe/clamped', () => new THREE.LatheGeometry(defaultLathe(), 5, 0, 7));
G('lathe/random', () => new THREE.LatheGeometry(randomProfile(), 12, 0, 2 * PI));

// ── Shapes and extrusion ────────────────────────────────────────────────

const shapeOf = (pts) => new THREE.Shape(pts.map(([x, y]) => V2(x, y)));
const closedShape = (pts) => {
  const s = new THREE.Shape();
  pts.forEach(([x, y], i) => (i ? s.lineTo(x, y) : s.moveTo(x, y)));
  s.closePath();
  return s;
};
const trapezoid = () => shapeOf([[-0.3, 0], [0.3, 0], [0.2, 0.82], [-0.2, 0.82]]);
const roof = () => closedShape([[-3.4, 0], [0, 2.1], [3.4, 0]]);
const hull = () => closedShape([[-5.5, -1.6], [3.4, -1.8], [5.6, 0], [3.4, 1.8], [-5.5, 1.6]]);
const surfboard = () => {
  const W = 0.6, L = 2.2, s = new THREE.Shape();
  s.moveTo(-W / 2, -L / 2);
  s.lineTo(W / 2, -L / 2);
  s.quadraticCurveTo(W / 2, L * 0.2, 0, L / 2);
  s.quadraticCurveTo(-W / 2, L * 0.2, -W / 2, -L / 2);
  return s;
};
const caliper = () => {
  const rimR = 0.3, ro = rimR * 0.9, ri = rimR * 0.6, a0 = PI * 0.04, a1 = PI * 0.4;
  const s = new THREE.Shape();
  s.absarc(0, 0, ro, a0, a1, false);
  s.absarc(0, 0, ri, a1, a0, true);
  return s;
};
const barn = () => {
  const W = 3, H = 2.5, R = 2;
  return closedShape([[-W, 0], [W, 0], [W, H], [0.72 * W, H + 0.62 * R], [0, H + R], [-0.72 * W, H + 0.62 * R], [-W, H]]);
};
const holed = () => {
  const s = closedShape([[-1, -1], [1, -1], [1, 1], [-1, 1]]);
  const h = new THREE.Path();
  h.absarc(0.2, 0.1, 0.4, 0, 2 * PI, true);
  s.holes.push(h);
  const h2 = new THREE.Path();
  h2.moveTo(-0.8, -0.8); h2.lineTo(-0.8, -0.4); h2.lineTo(-0.4, -0.4); h2.lineTo(-0.4, -0.8);
  s.holes.push(h2);
  return s;
};
const curvy = () => {
  const s = new THREE.Shape();
  s.moveTo(0, 0);
  s.bezierCurveTo(0.5, -0.3, 1.2, 0.2, 1.4, 0.9);
  s.splineThru([V2(1.1, 1.4), V2(0.6, 1.5), V2(0.2, 1.2)]);
  s.absellipse(-0.1, 0.6, 0.35, 0.6, PI / 2, 3 * PI / 2, false, 0.3);
  s.lineTo(0, 0);
  return s;
};
const triangle = () => shapeOf([[0, 0.5], [-0.5, -0.5], [0.5, -0.5]]);
const flat = (depth) => ({ depth, bevelEnabled: false });

G('extrude/default', () => new THREE.ExtrudeGeometry(shapeOf([[0.5, 0.5], [-0.5, 0.5], [-0.5, -0.5], [0.5, -0.5]]), {}));
G('extrude/trapezoid', () => new THREE.ExtrudeGeometry(trapezoid(), flat(2.0)));
G('extrude/trapezoid-cw', () => new THREE.ExtrudeGeometry(shapeOf([[-0.2, 0.82], [0.2, 0.82], [0.3, 0], [-0.3, 0]]), flat(2.0)));
G('extrude/roof', () => new THREE.ExtrudeGeometry(roof(), flat(9.6)));
G('extrude/hull', () => new THREE.ExtrudeGeometry(hull(), flat(1.7)));
G('extrude/barn', () => new THREE.ExtrudeGeometry(barn(), flat(6)));
G('extrude/surfboard', () => new THREE.ExtrudeGeometry(surfboard(), { depth: 0.08, bevelEnabled: true, bevelThickness: 0.15, bevelSize: 0.12, bevelSegments: 2, curveSegments: 8 }));
G('extrude/car', () => {
  const width = 0.3, bevel = 0.01, depth = Math.max(0.005, width - 2 * bevel);
  const g = new THREE.ExtrudeGeometry(closedShape([[-1, 0], [1, 0], [0.9, 0.4], [-0.8, 0.45]]), {
    depth, steps: 1, curveSegments: 1,
    bevelEnabled: bevel > 0, bevelThickness: bevel, bevelSize: bevel, bevelOffset: -bevel, bevelSegments: 1,
  });
  g.translate(0, 0, -depth / 2);
  g.rotateY(-PI / 2);
  g.computeVertexNormals();
  return g;
});
G('extrude/caliper', () => {
  const g = new THREE.ExtrudeGeometry(caliper(), { depth: 0.07, bevelEnabled: false, curveSegments: 4 });
  g.translate(0, 0, -0.035);
  g.rotateY(PI / 2);
  g.clearGroups();
  g.computeVertexNormals();
  return g;
});
G('extrude/steps', () => new THREE.ExtrudeGeometry(trapezoid(), { depth: 2, steps: 3, bevelEnabled: false }));
G('extrude/holes', () => new THREE.ExtrudeGeometry(holed(), { curveSegments: 6 }));
G('extrude/curvy', () => new THREE.ExtrudeGeometry(curvy(), { curveSegments: 6, depth: 0.5, bevelThickness: 0.1, bevelSize: 0.05, bevelSegments: 2 }));
G('extrude/multi', () => new THREE.ExtrudeGeometry([trapezoid(), roof()], flat(1.5)));

G('shape/triangle', () => new THREE.ShapeGeometry(triangle(), 12));
G('shape/holes', () => new THREE.ShapeGeometry(holed(), 5));
G('shape/curvy', () => new THREE.ShapeGeometry(curvy(), 5));
G('shape/closed', () => new THREE.ShapeGeometry(barn(), 12));
G('shape/array', () => new THREE.ShapeGeometry([triangle(), holed()], 4));

S('path/curvy-points', () => curvy().getPoints(7).flatMap((p) => [p.x, p.y]));
S('path/caliper-points', () => caliper().getPoints(4).flatMap((p) => [p.x, p.y]));
S('path/surfboard-spaced', () => surfboard().getSpacedPoints(10).flatMap((p) => [p.x, p.y]));
S('path/curvy-length', () => { const s = curvy(); return [s.getLength(), ...s.getCurveLengths()]; });

// ── triangulateShape ────────────────────────────────────────────────────

// Faces flattened, then the contour's length afterwards (the call drops a
// closing duplicate in place).
const tri = (contour, holes) => {
  const faces = THREE.ShapeUtils.triangulateShape(contour, holes);
  return [...faces.flat(), contour.length, ...holes.map((h) => h.length)];
};
const wobbly = (n, seed, holeR) => {
  const r = mulberry32(seed), pts = [];
  for (let k = 0; k < n; k++) {
    const a = (k / n) * 2 * PI, rad = (holeR ?? 10) * (0.7 + r() * 0.6);
    pts.push(V2(Math.cos(a) * rad, Math.sin(a) * rad));
  }
  return pts;
};
S('triangulate/square', () => tri([V2(0, 0), V2(1, 0), V2(1, 1), V2(0, 1)], []));
S('triangulate/star', () => tri(Array.from({ length: 10 }, (_, k) => V2(Math.cos(k * PI / 5) * (k % 2 ? 0.4 : 1), Math.sin(k * PI / 5) * (k % 2 ? 0.4 : 1))), []));
S('triangulate/wobbly-small', () => tri(wobbly(40, 3), []));
S('triangulate/wobbly-hashed', () => tri(wobbly(120, 4), []));
S('triangulate/dup-end', () => tri([V2(0, 0), V2(2, 0), V2(2, 1), V2(1, 2), V2(0, 1), V2(0, 0)], []));
S('triangulate/collinear', () => tri([V2(0, 0), V2(1, 0), V2(2, 0), V2(3, 0), V2(3, 1), V2(2, 1), V2(2, 2), V2(1, 2), V2(1, 1), V2(0, 1)], []));
S('triangulate/holes', () => tri(wobbly(30, 5), [wobbly(8, 6, 2).map((p) => V2(p.x - 3, p.y)).reverse(), wobbly(6, 7, 1.5).map((p) => V2(p.x + 4, p.y + 1)).reverse()]));
S('triangulate/holes-hashed', () => tri(wobbly(100, 8), [wobbly(12, 9, 2).reverse(), wobbly(5, 10, 1).map((p) => V2(p.x + 5, p.y)).reverse()]));
S('triangulate/ellipse', () => {
  const pts = [];
  for (let i = 0; i < 12; i++) {
    const a = (i / 12) * 2 * PI, x = Math.cos(a) * 0.4, y = Math.sin(a) * 0.25, rot = 0.3;
    pts.push(V2(0.1 + x * Math.cos(rot) - y * Math.sin(rot), 0.2 + x * Math.sin(rot) + y * Math.cos(rot)));
  }
  return tri(pts, []);
});
S('triangulate/self-touch', () => tri([V2(0, 0), V2(4, 0), V2(4, 4), V2(2, 2), V2(0, 4), V2(2, 2.5), V2(1, 1)], []));

// ── Curves and Tube ─────────────────────────────────────────────────────

const coasterBase = () => {
  const L = 300;
  return [
    [-20, 1.5, L - 84], [-8, 3, L - 86], [-5, 9, L - 78], [-6, 13, L - 66], [-10, 13.5, L - 56],
    [-19, 9, L - 50], [-20, 4, L - 40], [-12, 6, L - 30], [-6, 10, L - 20], [-12, 12, L - 10],
    [-20, 7, L - 12], [-21, 3, L - 26], [-21, 2, L - 60], [-21, 1.6, L - 76],
  ].map(([x, y, z]) => V3(x, y, z));
};
// Beach.js buildCoaster: the rails either side of a centripetal loop.
const coasterRails = () => {
  const curve = new THREE.CatmullRomCurve3(coasterBase(), true, 'centripetal');
  const N = 200, left = [], right = [];
  for (let i = 0; i < N; i++) {
    const u = i / N;
    const p = curve.getPointAt(u), tg = curve.getTangentAt(u);
    const side = V3(-tg.z, 0, tg.x).normalize();
    left.push(p.clone().addScaledVector(side, 0.55));
    right.push(p.clone().addScaledVector(side, -0.55));
  }
  return { curve, left, right };
};
S('curve/coaster', () => {
  const { curve } = coasterRails();
  const out = [curve.getLength()];
  for (let i = 0; i <= 50; i++) {
    const u = i / 50;
    out.push(...curve.getPoint(u).toArray(), ...curve.getPointAt(u).toArray(), ...curve.getTangentAt(u).toArray());
  }
  return out;
});
S('curve/rails', () => { const { left, right } = coasterRails(); return [...left, ...right].flatMap((p) => p.toArray()); });
const openPts = () => [V3(0, 0, 0), V3(1, 2, 0.5), V3(3, 2.5, -1), V3(4, 0, -2), V3(6, 1, 0)];
S('curve/open', () => {
  const out = [];
  for (const [type, tension] of [['centripetal', 0.5], ['chordal', 0.5], ['catmullrom', 0.5], ['catmullrom', 0.2]]) {
    const c = new THREE.CatmullRomCurve3(openPts(), false, type, tension);
    for (let i = 0; i <= 20; i++) out.push(...c.getPoint(i / 20).toArray());
    out.push(c.getLength(), ...c.getPointAt(0.37).toArray(), ...c.getTangent(1).toArray(), ...c.getTangent(0).toArray());
  }
  // Two points: three's shared scratch vector serves as both end points.
  const two = new THREE.CatmullRomCurve3([V3(0, 0, 0), V3(1, 1, 1)], false, 'centripetal', 0.5);
  for (let i = 0; i <= 8; i++) out.push(...two.getPoint(i / 8).toArray());
  return out;
});
S('curve/frenet', () => {
  const out = [];
  for (const closed of [false, true]) {
    const c = new THREE.CatmullRomCurve3(openPts(), closed, 'centripetal', 0.5);
    const f = c.computeFrenetFrames(24, closed);
    for (const k of ['tangents', 'normals', 'binormals']) for (const v of f[k]) out.push(...v.toArray());
  }
  return out;
});
G('tube/coaster-left', () => new THREE.TubeGeometry(new THREE.CatmullRomCurve3(coasterRails().left, true), 400, 0.09, 5, true));
G('tube/coaster-right', () => new THREE.TubeGeometry(new THREE.CatmullRomCurve3(coasterRails().right, true), 400, 0.09, 5, true));
G('tube/open', () => new THREE.TubeGeometry(new THREE.CatmullRomCurve3(openPts(), false, 'catmullrom', 0.5), 20, 0.2, 6, false));
G('tube/chordal', () => new THREE.TubeGeometry(new THREE.CatmullRomCurve3(openPts(), false, 'chordal', 0.5), 16, 0.3, 4, false));
G('tube/line', () => new THREE.TubeGeometry(new THREE.LineCurve3(V3(0, 0, 0), V3(1, 2, 3)), 4, 1, 8, false));
G('tube/quadratic', () => new THREE.TubeGeometry(new THREE.QuadraticBezierCurve3(V3(-1, -1, 0), V3(-1, 1, 0), V3(1, 1, 0)), 64, 1, 8, false));
G('tube/cubic', () => new THREE.TubeGeometry(new THREE.CubicBezierCurve3(V3(0, 0, 0), V3(1, 3, 0), V3(2, -1, 1), V3(3, 0, 2)), 12, 0.5, 3, false));

// ── Transforms and normals ──────────────────────────────────────────────

G('transform/chain', () => new THREE.BoxGeometry(1, 2, 3, 2, 1, 1).rotateX(0.3).rotateY(-1.2).rotateZ(2.5).translate(1, 2, 3).scale(1, 2, 0.5));
G('transform/matrix', () => {
  const q = new THREE.Quaternion().setFromEuler(new THREE.Euler(0.4, -0.7, 1.1, 'XYZ'));
  const m = new THREE.Matrix4().compose(V3(3, -1, 2), q, V3(1.5, 0.5, -2));
  return new THREE.CylinderGeometry(0.5, 0.5, 2, 12, 1, false, 0, 2 * PI).applyMatrix4(m);
});
for (const order of ['XYZ', 'YXZ', 'ZXY', 'ZYX', 'YZX', 'XZY']) {
  G(`transform/quaternion-${order}`, () => new THREE.SphereGeometry(1, 6, 4, 0, 2 * PI, 0, PI).applyQuaternion(new THREE.Quaternion().setFromEuler(new THREE.Euler(0.3, 0.5, -0.9, order))));
  G(`transform/euler-${order}`, () => new THREE.SphereGeometry(1, 6, 4, 0, 2 * PI, 0, PI).applyMatrix4(new THREE.Matrix4().makeRotationFromEuler(new THREE.Euler(0.3, 0.5, -0.9, order))));
}
G('transform/lookat', () => new THREE.PlaneGeometry(2, 2, 1, 1).lookAt(V3(1, 2, 3)));
G('transform/lookat-up', () => new THREE.PlaneGeometry(2, 2, 1, 1).lookAt(V3(0, 5, 0)));
G('transform/lookat-origin', () => new THREE.PlaneGeometry(2, 2, 1, 1).lookAt(V3(0, 0, 0)));
G('transform/center', () => new THREE.TorusGeometry(1, 0.3, 6, 12, PI).center());
G('transform/bounds', () => {
  const g = new THREE.TorusGeometry(1, 0.3, 6, 12, PI);
  g.computeBoundingBox();
  g.computeBoundingSphere();
  g.rotateX(0.7).translate(0, 3, 0); // recomputes both
  return g;
});
G('transform/normals-indexed', () => { const g = new THREE.SphereGeometry(1, 8, 6, 0, 2 * PI, 0, PI); g.scale(1, 0.5, 2); g.computeVertexNormals(); return g; });
G('transform/normals-added', () => { const g = new THREE.TorusGeometry(1, 0.4, 5, 7, 2 * PI); g.deleteAttribute('normal'); g.computeVertexNormals(); return g; });
G('transform/nonindexed', () => { const g = new THREE.BoxGeometry(1, 1, 1, 1, 2, 1).toNonIndexed(); g.computeVertexNormals(); return g; });
G('transform/nonindexed-lathe', () => new THREE.LatheGeometry(baleProfile(), 11, 0, 2 * PI).toNonIndexed());
S('math/matrices', () => {
  const out = [];
  const q = new THREE.Quaternion().setFromEuler(new THREE.Euler(0.4, -0.7, 1.1, 'YXZ'));
  const m = new THREE.Matrix4().compose(V3(3, -1, 2), q, V3(1.5, 0.5, -2));
  out.push(...m.elements, m.determinant(), ...m.clone().invert().elements);
  const p = new THREE.Vector3(), qq = new THREE.Quaternion(), s = new THREE.Vector3();
  m.decompose(p, qq, s);
  out.push(...p.toArray(), ...qq.toArray(), ...s.toArray());
  const n = new THREE.Matrix3().getNormalMatrix(m);
  out.push(...n.elements);
  out.push(...new THREE.Matrix4().makeRotationAxis(V3(1, 2, 2).normalize(), 0.9).elements);
  out.push(...new THREE.Matrix4().makeBasis(V3(1, 0, 0), V3(0, 0, 1), V3(0, -1, 0)).multiply(m).elements);
  for (const [a, b] of [[V3(0, 1, 0), V3(1, 0, 0)], [V3(0, 1, 0), V3(0, -1, 0)], [V3(1, 0, 0), V3(-1, 0, 0)], [V3(0, 0, 1), V3(0.6, 0, 0.8)], [V3(0, 1, 0), V3(0.3, 0.4, -0.866).normalize()]]) {
    out.push(...new THREE.Quaternion().setFromUnitVectors(a, b).toArray());
  }
  const qa = new THREE.Quaternion().setFromAxisAngle(V3(0, 1, 0), 0.8);
  const qb = new THREE.Quaternion().setFromEuler(new THREE.Euler(0.2, 0.1, -0.3, 'ZYX'));
  out.push(...qa.clone().multiply(qb).toArray(), ...qa.clone().premultiply(qb).toArray(), ...qa.clone().slerp(qb, 0.35).toArray());
  out.push(...V3(1, 2, 3).applyAxisAngle(V3(0, 0, 1), 0.5).toArray(), ...V3(1, 2, 3).applyQuaternion(qb).toArray());
  out.push(...new THREE.Quaternion().setFromRotationMatrix(new THREE.Matrix4().makeRotationFromEuler(new THREE.Euler(2.8, 0.1, -2.9, 'XYZ'))).toArray());
  return out;
});

// ── mergeGeometries and mergeVertices ───────────────────────────────────

const trio = () => [
  new THREE.BoxGeometry(1, 1, 1, 1, 1, 1).translate(2, 0, 0),
  new THREE.CylinderGeometry(0.5, 0.5, 1, 8, 1, false, 0, 2 * PI),
  new THREE.SphereGeometry(1, 8, 6, 0, 2 * PI, 0, PI),
];
G('merge/indexed', () => mergeGeometries(trio(), false));
G('merge/groups', () => mergeGeometries(trio(), true));
G('merge/nonindexed', () => mergeGeometries([new THREE.BoxGeometry(1, 1, 1, 1, 1, 1).toNonIndexed(), new THREE.IcosahedronGeometry(1, 0)], true));
G('merge/attribute-order', () => mergeGeometries([new THREE.LatheGeometry(defaultLathe(), 12, 0, 2 * PI), new THREE.BoxGeometry(1, 1, 1, 1, 1, 1)], false));
G('merge/mixed-index', () => mergeGeometries([new THREE.BoxGeometry(1, 1, 1, 1, 1, 1), new THREE.IcosahedronGeometry(1, 0)], false));
G('merge/missing-attribute', () => { const b = new THREE.BoxGeometry(1, 1, 1, 1, 1, 1); b.deleteAttribute('uv'); return mergeGeometries([new THREE.BoxGeometry(1, 1, 1, 1, 1, 1), b], false); });
G('merge/uint32', () => mergeGeometries(Array.from({ length: 130 }, (_, k) => new THREE.SphereGeometry(1, 32, 16, 0, 2 * PI, 0, PI).translate(k * 3, 0, 0)), false));
G('merge/colors', () => {
  const parts = [new THREE.BoxGeometry(1, 1, 1, 1, 1, 1), new THREE.ConeGeometry(0.5, 1, 6, 1, false, 0, 2 * PI)].map((g, k) => {
    g = g.toNonIndexed();
    g.deleteAttribute('uv');
    const n = g.attributes.position.count, c = new Float32Array(n * 3);
    for (let i = 0; i < n * 3; i++) c[i] = (i % 7) / 7 + k * 0.1;
    g.setAttribute('color', new THREE.BufferAttribute(c, 3));
    return g;
  });
  return mergeGeometries(parts, false);
});

G('mergeVertices/icosahedron-2', () => { const g = new THREE.IcosahedronGeometry(1, 2); g.deleteAttribute('normal'); g.deleteAttribute('uv'); return mergeVertices(g); });
G('mergeVertices/box-222', () => { const g = new THREE.BoxGeometry(2, 2, 2, 2, 2, 2); g.deleteAttribute('uv'); g.deleteAttribute('normal'); return mergeVertices(g); });
G('mergeVertices/icosahedron-full', () => mergeVertices(new THREE.IcosahedronGeometry(1, 1)));
G('mergeVertices/indexed', () => mergeVertices(new THREE.SphereGeometry(1, 8, 6, 0, 2 * PI, 0, PI)));
G('mergeVertices/tolerance', () => { const g = new THREE.TorusGeometry(1, 0.4, 8, 12, 2 * PI); g.deleteAttribute('uv'); return mergeVertices(g.toNonIndexed(), 0.05); });
G('mergeVertices/flora', () => {
  let g = new THREE.IcosahedronGeometry(1, 1);
  g.deleteAttribute('normal');
  g.deleteAttribute('uv');
  g.scale(1, 0.7, 1);
  g.translate(0.1, 0.2, 0);
  g = mergeVertices(g);
  g.computeVertexNormals();
  return g.toNonIndexed();
});

// ── Write ───────────────────────────────────────────────────────────────

const out = {
  note: 'Generated by tools/parity/three-geom.mjs with the parity kernel on (three.js r' + THREE.REVISION + '). Do not edit.',
  cases,
};
const text = JSON.stringify(out, null, 1) + '\n';
if (process.argv.includes('--check')) {
  const old = fs.readFileSync(OUT, 'utf8');
  if (old !== text) {
    console.error('three-geom: regenerated file differs from', path.relative(ROOT, OUT));
    process.exit(1);
  }
  console.log('three-geom: identical');
} else {
  fs.mkdirSync(path.dirname(OUT), { recursive: true });
  fs.writeFileSync(OUT, text);
  console.log('three-geom: wrote', path.relative(ROOT, OUT), `(${Object.keys(cases).length} cases, ${text.length} bytes)`);
}
