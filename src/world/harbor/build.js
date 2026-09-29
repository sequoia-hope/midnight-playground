import * as THREE from 'three';
import { mergeGeometries } from 'three/addons/utils/BufferGeometryUtils.js';
import { staticMesh } from '../city/geom.js';

// Geometry helpers for the harbour: a batcher that merges everything of one
// material into a few spatially-chunked meshes, plus 3D beams, tapered boxes
// and swept ribbons along arbitrary frame lists.

const KEEP = new Set(['position', 'normal', 'uv', 'color']);
const _c = new THREE.Color();

// sRGB hex → linear [r, g, b] for vertex colours.
export const col = (hex) => { _c.setHex(hex); return [_c.r, _c.g, _c.b]; };

export class Batch {
  constructor(group) {
    this.group = group;
    this.map = new Map();
  }

  add(geo, mat, { cast = false, receive = true, chunk = 700 } = {}) {
    let g = geo.index ? geo.toNonIndexed() : geo;
    if (g !== geo) geo.dispose();
    for (const k of Object.keys(g.attributes)) if (!KEEP.has(k)) g.deleteAttribute(k);
    if (!g.attributes.normal) g.computeVertexNormals();
    const n = g.attributes.position.count;
    if (!n) return;
    if (!g.attributes.uv) g.setAttribute('uv', new THREE.Float32BufferAttribute(new Float32Array(n * 2), 2));
    if (mat.vertexColors) {
      if (!g.attributes.color) g.setAttribute('color', new THREE.Float32BufferAttribute(new Float32Array(n * 3).fill(1), 3));
    } else if (g.attributes.color) g.deleteAttribute('color');
    g.computeBoundingSphere();
    const c = g.boundingSphere.center;
    const key = `${mat.uuid}:${Math.floor(c.x / chunk)}:${Math.floor(c.z / chunk)}:${cast ? 1 : 0}`;
    let b = this.map.get(key);
    if (!b) this.map.set(key, (b = { mat, cast, receive, geos: [] }));
    b.geos.push(g);
  }

  flush() {
    for (const b of this.map.values()) {
      const geo = b.geos.length === 1 ? b.geos[0] : mergeGeometries(b.geos, false);
      if (b.geos.length > 1) b.geos.forEach((g) => g.dispose());
      geo.computeBoundingSphere();
      this.group.add(staticMesh(geo, b.mat, { cast: b.cast, receive: b.receive }));
    }
    this.map.clear();
  }
}

// Set a flat vertex colour on a geometry.
export function tint(geo, rgb) {
  const n = geo.attributes.position.count;
  const a = new Float32Array(n * 3);
  for (let i = 0; i < n; i++) { a[i * 3] = rgb[0]; a[i * 3 + 1] = rgb[1]; a[i * 3 + 2] = rgb[2]; }
  geo.setAttribute('color', new THREE.Float32BufferAttribute(a, 3));
  return geo;
}

const _a = new THREE.Vector3(), _b = new THREE.Vector3(), _n = new THREE.Vector3();

// Quad whose normal points along `out` regardless of vertex order.
export function quadOut(geo, p0, p1, p2, p3, out, color = null, uvs = null) {
  _a.set(p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]);
  _b.set(p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]);
  _n.crossVectors(_a, _b);
  const u = uvs || [[0, 0], [1, 0], [1, 1], [0, 1]];
  if (_n.x * out[0] + _n.y * out[1] + _n.z * out[2] < 0) geo.quad(p0, p3, p2, p1, [u[0], u[3], u[2], u[1]], color);
  else geo.quad(p0, p1, p2, p3, u, color);
}

// Rectangular beam between two 3D points (w across, h in the "up-ish" axis).
export function beam(geo, A, B, w, h, color = null, caps = true) {
  const d = new THREE.Vector3(B[0] - A[0], B[1] - A[1], B[2] - A[2]);
  const L = d.length();
  if (L < 1e-4) return;
  d.divideScalar(L);
  const ref = Math.abs(d.y) < 0.9 ? new THREE.Vector3(0, 1, 0) : new THREE.Vector3(1, 0, 0);
  const u = new THREE.Vector3().crossVectors(d, ref).normalize();
  const v = new THREE.Vector3().crossVectors(u, d).normalize();
  const P = (base, su, sv) => [base[0] + u.x * su * w / 2 + v.x * sv * h / 2, base[1] + u.y * su * w / 2 + v.y * sv * h / 2, base[2] + u.z * su * w / 2 + v.z * sv * h / 2];
  const sides = [[1, 0], [-1, 0], [0, 1], [0, -1]];
  for (const [su, sv] of sides) {
    const out = [u.x * su + v.x * sv, u.y * su + v.y * sv, u.z * su + v.z * sv];
    // Two corners of this face at each end.
    const c1 = su !== 0 ? [su, -1] : [-1, sv], c2 = su !== 0 ? [su, 1] : [1, sv];
    const uvs = [[0, 0], [L / 4, 0], [L / 4, 1], [0, 1]];
    quadOut(geo, P(A, ...c1), P(B, ...c1), P(B, ...c2), P(A, ...c2), out, color, uvs);
  }
  if (caps) {
    quadOut(geo, P(A, -1, -1), P(A, 1, -1), P(A, 1, 1), P(A, -1, 1), [-d.x, -d.y, -d.z], color);
    quadOut(geo, P(B, -1, -1), P(B, 1, -1), P(B, 1, 1), P(B, -1, 1), [d.x, d.y, d.z], color);
  }
}

// Box centred on (cx, cz), bottom y0 to top y1, tapering from a0×w0 to a1×w1
// (a along the unit direction (ax, az), w across it).
export function frustum(geo, cx, cz, y0, y1, ax, az, a0, w0, a1, w1, color = null, tile = 8) {
  const rx = -az, rz = ax;
  const corner = (a, w, p, q) => [cx + ax * p * a / 2 + rx * q * w / 2, cz + az * p * a / 2 + rz * q * w / 2];
  const ring = [[1, 1], [1, -1], [-1, -1], [-1, 1]];
  for (let i = 0; i < 4; i++) {
    const [p0, q0] = ring[i], [p1, q1] = ring[(i + 1) % 4];
    const b0 = corner(a0, w0, p0, q0), b1 = corner(a0, w0, p1, q1);
    const t0 = corner(a1, w1, p0, q0), t1 = corner(a1, w1, p1, q1);
    const mx = (p0 + p1) / 2, mq = (q0 + q1) / 2;
    const out = [ax * mx + rx * mq, 0, az * mx + rz * mq];
    const len = Math.hypot(b1[0] - b0[0], b1[1] - b0[1]);
    quadOut(geo, [b0[0], y0, b0[1]], [b1[0], y0, b1[1]], [t1[0], y1, t1[1]], [t0[0], y1, t0[1]], out, color,
      [[0, y0 / tile], [len / tile, y0 / tile], [len / tile, y1 / tile], [0, y1 / tile]]);
  }
  const T = ring.map(([p, q]) => { const c = corner(a1, w1, p, q); return [c[0], y1, c[1]]; });
  quadOut(geo, T[0], T[1], T[2], T[3], [0, 1, 0], color);
}

// Swept cross-section along a list of frames {x, z, fx, fz, ...}. profile:
// [{o, y(fr, o)}] in order of increasing offset (to the frame's right) for
// upward-facing surfaces.
export function ribbon(frames, profile, { uS = 4, vS = 8 } = {}) {
  const pos = [], uv = [], idx = [];
  const P = profile.length;
  let dist = 0;
  frames.forEach((fr, r) => {
    if (r > 0) dist += Math.hypot(fr.x - frames[r - 1].x, fr.z - frames[r - 1].z);
    const rx = -fr.fz, rz = fr.fx;
    for (const pr of profile) {
      const y = pr.y(fr, pr.o);
      pos.push(fr.x + rx * pr.o, y, fr.z + rz * pr.o);
      uv.push(pr.o / uS, dist / vS);
    }
  });
  for (let r = 0; r < frames.length - 1; r++) {
    for (let p = 0; p < P - 1; p++) {
      const i0 = r * P + p, i1 = i0 + 1, i2 = i0 + P, i3 = i2 + 1;
      idx.push(i0, i1, i2, i1, i3, i2);
    }
  }
  const g = new THREE.BufferGeometry();
  g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
  g.setAttribute('uv', new THREE.Float32BufferAttribute(uv, 2));
  g.setIndex(idx);
  g.computeVertexNormals();
  return g;
}

// Matrix for a unit cylinder (height along +Y, centred) spanning A→B.
const _up = new THREE.Vector3(0, 1, 0), _q = new THREE.Quaternion();
export function spanMatrix(A, B, r = 1) {
  const d = new THREE.Vector3(B[0] - A[0], B[1] - A[1], B[2] - A[2]);
  const L = d.length();
  _q.setFromUnitVectors(_up, d.clone().divideScalar(L));
  return new THREE.Matrix4().compose(
    new THREE.Vector3((A[0] + B[0]) / 2, (A[1] + B[1]) / 2, (A[2] + B[2]) / 2), _q.clone(), new THREE.Vector3(r, L, r));
}
