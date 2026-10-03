import * as THREE from 'three';
import { mergeGeometries, mergeVertices } from 'three/addons/utils/BufferGeometryUtils.js';
import { lerp, clamp, smoothstep, mulberry32, makeNoise2D } from '../../util/math.js';
import { paint, prep } from './parts.js';

// Desert Run, second kit: sculpted sandstone (strata-shaded rock, the arch,
// buttes), the rest of the Mojave flora, tumbleweeds, the spectators' camp
// and the dry-lake decals. Like parts.js, everything is vertex-coloured and
// merged into one geometry per object so the scenery can instance it.

const V3 = (x, y, z) => new THREE.Vector3(x, y, z);

function merge(parts) {
  const g = mergeGeometries(parts, false);
  for (const p of parts) p.dispose();
  g.computeBoundingSphere();
  return g;
}

function limb(a, b, r0, r1, seg = 5) {
  const dir = new THREE.Vector3().subVectors(b, a);
  const len = dir.length();
  const g = new THREE.CylinderGeometry(r1, r0, len, seg, 1, true);
  g.translate(0, len / 2, 0);
  g.applyQuaternion(new THREE.Quaternion().setFromUnitVectors(V3(0, 1, 0), dir.normalize()));
  g.translate(a.x, a.y, a.z);
  return g;
}

// ── Sandstone shading ────────────────────────────────────────────
// Red Rock palette: iron-red, salmon, cream and chocolate layers.
export const STRATA = [0xc8744a, 0xe2b088, 0xb45e3e, 0xd8966a, 0xa4543a, 0xe8c49c, 0xbe6a44].map((h) => new THREE.Color(h));
const VARNISH = new THREE.Color(0x3e2a22);
const SHADE = new THREE.Color(0x6a3a2a);

// Paints a (non-indexed) rock geometry: colour bands by height, the
// underside and foot darkened as if ambient-occluded, and desert-varnish
// streaks running down the steep faces. `period` is the band height in
// local units; `lift` offsets the bands so instances of one shape differ.
export function strataPaint(geo, { seed = 1, period = 0.22, lift = 0, varnish = 0.55, ao = 0.5, bands = STRATA, bleach = 0.12 } = {}) {
  const n = makeNoise2D(seed);
  const pos = geo.getAttribute('position'), nor = geo.getAttribute('normal');
  geo.computeBoundingBox();
  const { min, max } = geo.boundingBox;
  const H = Math.max(1e-3, max.y - min.y);
  const col = new Float32Array(pos.count * 3);
  const c = new THREE.Color();
  const L = bands.length;
  for (let i = 0; i < pos.count; i++) {
    const x = pos.getX(i), y = pos.getY(i), z = pos.getZ(i);
    const ny = nor.getY(i);
    const v = (y + lift) / period + n(x * 0.8 + 3, z * 0.8) * 0.9 + 50;
    const k = Math.floor(v), fr = smoothstep(0.72, 1, v - k);
    c.copy(bands[k % L]).lerp(bands[(k + 1) % L], fr);
    // Sun-bleached tops, shadowed undersides.
    const up = clamp(ny, -1, 1);
    if (up > 0) c.lerp(bands[5 % L], bleach * up);
    else c.lerp(SHADE, -up * 0.45 * ao);
    const foot = 1 - smoothstep(0, 0.3, (y - min.y) / H);
    c.multiplyScalar(1 - foot * 0.35 * ao);
    // Varnish: dark streaks hanging down from ledges on steep faces.
    const steep = 1 - Math.abs(ny);
    const a = Math.atan2(z, x);
    const streak = smoothstep(0.1, 0.55, n(Math.cos(a) * 3.1 + x * 1.7, Math.sin(a) * 3.1 + z * 1.7 + y * 0.12));
    const fade = 0.4 + 0.6 * smoothstep(0.1, 0.9, (y - min.y) / H);
    c.lerp(VARNISH, varnish * streak * steep * steep * fade);
    col[i * 3] = c.r; col[i * 3 + 1] = c.g; col[i * 3 + 2] = c.b;
  }
  geo.setAttribute('color', new THREE.BufferAttribute(col, 3));
  return geo;
}

// Rock of a given character, unit size, flat underside at y ≈ -0.25.
// kind: 'round' (weathered boulder), 'block' (fresh rockfall with broken
// faces), 'slab' (a fallen ledge), 'scree' (cheap talus chip).
export function rockGeometry(seed, kind = 'round') {
  const n = makeNoise2D(seed);
  const rng = mulberry32(seed);
  let g;
  if (kind === 'round') g = new THREE.IcosahedronGeometry(1, 1);
  else if (kind === 'scree') g = new THREE.IcosahedronGeometry(1, 0);
  else g = new THREE.BoxGeometry(2, 2, 2, 2, 2, 2);
  g.deleteAttribute('uv'); g.deleteAttribute('normal');
  g = mergeVertices(g);
  const p = g.getAttribute('position');
  const sq = kind === 'slab' ? 0.38 : kind === 'block' ? 0.8 : kind === 'scree' ? 0.6 : 0.72;
  const round = kind === 'block' || kind === 'slab' ? 0.35 : 1;
  // Random cutting planes knock corners off, giving broken facets.
  const cuts = [];
  for (let k = 0; k < (kind === 'round' ? 3 : 6); k++) {
    const d = V3(rng() - 0.5, rng() * 0.8 - 0.1, rng() - 0.5).normalize();
    cuts.push({ d, o: lerp(0.62, 0.85, rng()) });
  }
  const v = new THREE.Vector3();
  for (let i = 0; i < p.count; i++) {
    v.set(p.getX(i), p.getY(i), p.getZ(i));
    const l = v.length() || 1;
    // Blend box toward sphere so edges round off.
    v.lerp(v.clone().multiplyScalar(1.15 / l), round * 0.6);
    const d = n(v.x * 1.1 + v.z * 2.3 + 5, v.y * 1.3 - v.z * 0.7) * 0.5 + n(v.x * 2.9 - v.z * 1.7, v.y * 2.6 + v.x) * 0.22;
    v.multiplyScalar(1 + d * (kind === 'round' ? 0.4 : 0.22));
    for (const c of cuts) {
      const t = v.dot(c.d) - c.o;
      if (t > 0) v.addScaledVector(c.d, -t);   // clip onto the plane: a clean facet, no folds
    }
    p.setXYZ(i, v.x, Math.max(v.y * sq, -0.25), v.z);
  }
  const f = g.toNonIndexed();
  g.dispose();
  f.computeVertexNormals();
  return strataPaint(f, { seed: seed + 11, period: kind === 'slab' ? 0.09 : 0.2, varnish: kind === 'scree' ? 0.25 : 0.5, ao: 0.6 });
}

// Hoodoo: a banded column of soft rock pinched into necks, flaring into a
// pedestal at the foot and carrying a hard, dark, overhanging caprock.
// Unit height (y 0..1 plus the cap), radius ~0.12.
export function hoodooGeometry(seed) {
  const rng = mulberry32(seed);
  const n = makeNoise2D(seed + 7);
  const prof = [];
  const necks = 1 + Math.floor(rng() * 3);
  const N = 17;
  for (let i = 0; i <= N; i++) {
    const t = i / N;
    let r = lerp(0.19, 0.085, Math.pow(t, 0.8));
    r += 0.16 * Math.exp(-t * 16);                  // pedestal flare
    for (let k = 0; k < necks; k++) {
      const c = 0.3 + (k + 0.5) / necks * 0.6 + (rng() - 0.5) * 0.05;
      r *= 1 - 0.38 * Math.exp(-Math.pow((t - c) / 0.06, 2));
    }
    // Hard beds stand proud, soft beds weather back.
    r *= 1 + 0.07 * Math.sin(t * 38 + rng() * 0.3);
    prof.push(new THREE.Vector2(r, t));
  }
  prof.push(new THREE.Vector2(0.001, 1));
  let g = new THREE.LatheGeometry(prof, 12);
  const p = g.getAttribute('position');
  for (let i = 0; i < p.count; i++) {
    const x = p.getX(i), y = p.getY(i), z = p.getZ(i);
    const a = Math.atan2(z, x);
    // Wrap-safe roughness: noise on (cos, sin) of the angle, not x/z.
    const d = 1 + n(Math.cos(a) * 1.3 + y * 3, Math.sin(a) * 1.3 - y * 2) * 0.22 + n(Math.cos(a) * 4, Math.sin(a) * 4 + y * 9) * 0.06;
    p.setXYZ(i, x * d, y, z * d);
  }
  g.deleteAttribute('uv');
  g.deleteAttribute('normal');
  g.computeVertexNormals(); // smooth (indexed)
  const gi = g.toNonIndexed();
  g.dispose();
  const bands = [0xd98a5c, 0xeec39a, 0xc86e48, 0xe4a878, 0xbc6444, 0xf0d0a8].map((h) => new THREE.Color(h));
  strataPaint(gi, { seed, period: 0.09, bands, varnish: 0.5, ao: 0.4, bleach: 0.05 });
  // Caprock: a hard grey-brown slab, wider than the neck below it.
  const cap = rockGeometry(seed + 3, 'slab');
  cap.scale(0.1 + rng() * 0.03, 0.2, 0.095 + rng() * 0.03);
  cap.translate(0, 1.01, 0);
  const cc = cap.getAttribute('color');
  const tint = new THREE.Color(0x7a5c4a);
  for (let i = 0; i < cc.count; i++) {
    const c = new THREE.Color(cc.getX(i), cc.getY(i), cc.getZ(i)).lerp(tint, 0.7);
    cc.setXYZ(i, c.r, c.g, c.b);
  }
  return merge([gi, cap]);
}

// The natural arch: a span swept along a flattened semicircle whose
// cross-section is a rounded box, thick where it springs from its
// buttresses and thin at the crown. Soft beds are cut back into grooves,
// noise erodes the whole thing and fallen blocks lie at its feet.
// Local: x across the road (legs at ±R), y up from the road, z along it.
export function archGeometry(seed, { R = 21, H = 30, groundL = 0, groundR = 0, base = 0 } = {}) {
  const n = makeNoise2D(seed), rng = mulberry32(seed);
  const NS = 72, NR = 20;
  const pos = [], idx = [];
  const path = (t) => {
    const a = Math.PI * t;
    const s = Math.sin(a);
    return V3(-R * Math.cos(a) + Math.sin(a * 2) * 1.5, H * Math.pow(s, 0.55), 0);
  };
  const tmp = V3(0, 0, 0);
  // Extend the legs down into the ground beyond the path's ends.
  const P = [];
  for (let i = 0; i <= NS; i++) {
    const t = i / NS;
    const c = path(t);
    const e = 0.004;
    const tg = path(Math.min(1, t + e)).sub(path(Math.max(0, t - e))).normalize();
    P.push({ c, tg, t });
  }
  const sink = (gy) => ({ c: V3(0, 0, 0), tg: V3(0, 1, 0), gy });
  const first = P[0], last = P[NS];
  P.unshift({ c: V3(first.c.x, groundL - 6, 0), tg: V3(0, 1, 0), t: 0 });
  P.push({ c: V3(last.c.x, groundR - 6, 0), tg: V3(0, -1, 0), t: 1 });
  const ring = P.length;
  for (let i = 0; i < ring; i++) {
    const { c, tg, t } = P[i];
    // Frame: tangent, in-plane normal (outward), road axis.
    const nrm = V3(-tg.y, tg.x, 0);
    if (nrm.y < 0 || (Math.abs(nrm.y) < 1e-3 && c.x * nrm.x < 0)) nrm.multiplyScalar(-1);
    const foot = Math.pow(1 - Math.sin(Math.PI * t), 2.2);
    const crown = Math.pow(Math.sin(Math.PI * t), 6);
    const a = lerp(3.4, 8.5, foot) * (1 + 0.25 * crown) * (1 + 0.12 * n(t * 7, 1.3));    // radial half-thickness
    const b = lerp(4.6, 10.5, foot) * (1 + 0.15 * n(t * 5, 4.1));                          // half-depth along road
    for (let k = 0; k < NR; k++) {
      const u = (k / NR) * Math.PI * 2;
      const cu = Math.cos(u), su = Math.sin(u);
      // Rounded box: superellipse exponent 0.6.
      const ex = Math.sign(cu) * Math.pow(Math.abs(cu), 0.6), ez = Math.sign(su) * Math.pow(Math.abs(su), 0.6);
      tmp.copy(c).addScaledVector(nrm, ex * a);
      tmp.z += ez * b;
      // Erosion: big lumps, then grooves where soft beds are cut back.
      const wy = tmp.y + base;
      const soft = smoothstep(0.55, 0.8, (((wy / 2.7) % 1) + 1) % 1) * (1 - smoothstep(0.85, 1, (((wy / 2.7) % 1) + 1) % 1));
      const bump = n(tmp.x * 0.09 + tmp.z * 0.05, wy * 0.08) * 1.6 + n(tmp.x * 0.3 + 7, wy * 0.25 + tmp.z * 0.3) * 0.5;
      const inward = bump - soft * 0.9;
      const ox = ex * nrm.x, oy = ex * nrm.y, oz = ez;
      const ol = Math.hypot(ox, oy, oz) || 1;
      tmp.x += (ox / ol) * inward; tmp.y += (oy / ol) * inward; tmp.z += (oz / ol) * inward * 1.1;
      pos.push(tmp.x, tmp.y, tmp.z);
    }
    if (i < ring - 1) for (let k = 0; k < NR; k++) {
      const a0 = i * NR + k, a1 = i * NR + (k + 1) % NR, b0 = a0 + NR, b1 = a1 + NR;
      idx.push(a0, b0, a1, a1, b0, b1);
    }
  }
  let g = new THREE.BufferGeometry();
  g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
  g.setIndex(idx);
  g.computeVertexNormals();
  const gi = g.toNonIndexed();
  g.dispose();
  // Smooth normals from the indexed version survive toNonIndexed.
  paintWorldStrata(gi, base, { seed, varnish: 0.6 });
  const parts = [gi];
  // Buttress masses and fallen blocks round the feet.
  for (const side of [-1, 1]) {
    const gy = side < 0 ? groundL : groundR;
    for (let k = 0; k < 7; k++) {
      const big = k < 2;
      const r = rockGeometry(seed * 7 + k * 13 + (side > 0 ? 5 : 0), big ? 'round' : rng() < 0.5 ? 'block' : 'slab');
      const s = big ? lerp(6, 9, rng()) : lerp(1.2, 3.2, rng());
      r.scale(s * lerp(0.9, 1.4, rng()), s * (big ? 1.2 : lerp(0.6, 1.0, rng())), s * lerp(0.9, 1.4, rng()));
      r.rotateY(rng() * 6.3);
      const out = big ? lerp(2, 5, rng()) : lerp(4, 14, rng());
      r.translate(side * (R + out), gy + (big ? s * 0.2 : s * 0.1), (rng() - 0.5) * (big ? 10 : 26));
      repaintWorld(r, base, seed + k);
      parts.push(r);
    }
  }
  return merge(parts);
}

// Strata by absolute height (so the arch's bands line up with the walls').
function paintWorldStrata(geo, base, { seed = 1, varnish = 0.5 } = {}) {
  const n = makeNoise2D(seed + 5);
  const pos = geo.getAttribute('position'), nor = geo.getAttribute('normal');
  const col = new Float32Array(pos.count * 3), c = new THREE.Color();
  const bands = [0xc9774c, 0xe0ae84, 0xb45e3c, 0xd89a6c, 0xa8523a, 0xc4845a, 0xe6bc92].map((h) => new THREE.Color(h));
  for (let i = 0; i < pos.count; i++) {
    const x = pos.getX(i), y = pos.getY(i), z = pos.getZ(i), ny = nor.getY(i);
    const v = (y + base) / 3.4 + n(x * 0.05, z * 0.05) * 0.4 + 100, k = Math.floor(v);
    c.copy(bands[k % 7]).lerp(bands[(k + 1) % 7], smoothstep(0.7, 1, v - k));
    if (ny < 0) c.lerp(SHADE, -ny * 0.35);
    const steep = 1 - Math.abs(ny);
    const streak = smoothstep(0.05, 0.5, n(x * 0.45 + z * 0.3, y * 0.02 + 3));
    c.lerp(VARNISH, varnish * streak * steep * (0.5 + 0.5 * smoothstep(5, 28, y)));
    col[i * 3] = c.r; col[i * 3 + 1] = c.g; col[i * 3 + 2] = c.b;
  }
  geo.setAttribute('color', new THREE.BufferAttribute(col, 3));
}
function repaintWorld(geo, base, seed) {
  paintWorldStrata(geo, base, { seed, varnish: 0.4 });
}

// Butte / mesa for the far horizon: a skirt of talus, sheer banded cliffs
// and a flat caprock. Unit radius, unit height; `spire` makes a narrow
// pinnacle. Cheap: it's only ever seen kilometres away.
export function butteGeometry(seed, spire = false) {
  const rng = mulberry32(seed), n = makeNoise2D(seed);
  const S = spire ? 9 : 14;
  const prof = spire
    ? [[1.6, 0], [0.8, 0.22], [0.5, 0.3], [0.46, 0.7], [0.4, 0.95], [0.44, 0.97], [0.0, 1.0]]
    : [[1.7, 0], [1.25, 0.18], [1.02, 0.3], [1.0, 0.62], [0.9, 0.66], [0.88, 0.95], [0.92, 0.97], [0.0, 1.0]];
  const pos = [], idx = [];
  const rows = prof.length;
  const wob = Array.from({ length: S }, (_, k) => 1 + n(Math.cos(k / S * 6.283) * 1.4, Math.sin(k / S * 6.283) * 1.4) * 0.28 + (rng() - 0.5) * 0.08);
  for (let r = 0; r < rows; r++) {
    const [rad, y] = prof[r];
    for (let k = 0; k < S; k++) {
      const a = (k / S) * Math.PI * 2;
      pos.push(Math.cos(a) * rad * wob[k], y, Math.sin(a) * rad * wob[k]);
    }
  }
  for (let r = 0; r < rows - 1; r++) for (let k = 0; k < S; k++) {
    const a0 = r * S + k, a1 = r * S + (k + 1) % S, b0 = a0 + S, b1 = a1 + S;
    idx.push(a0, b0, a1, a1, b0, b1);
  }
  const g = new THREE.BufferGeometry();
  g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
  g.setIndex(idx);
  const f = g.toNonIndexed();
  g.dispose();
  f.computeVertexNormals();
  const bands = [0xb86a48, 0xd49a70, 0xa85a3c, 0xc88460, 0x9a5038, 0xdcae84].map((h) => new THREE.Color(h));
  return strataPaint(f, { seed, period: spire ? 0.08 : 0.07, bands, varnish: 0.45, ao: 0.3, bleach: 0.2 });
}

// ── Flora ────────────────────────────────────────────────────────
// Pinyon pine: short trunk, a dense rounded crown of dark clumps.
export function pinyonGeometry(seed) {
  const rng = mulberry32(seed);
  const parts = [];
  const H = lerp(3.2, 4.5, rng());
  parts.push(paint(prep(limb(V3(0, 0, 0), V3((rng() - 0.5) * 0.4, H * 0.45, (rng() - 0.5) * 0.4), 0.22, 0.14, 5)), 0x4e3a2c, 0.2, rng));
  const cols = [0x2f4a2c, 0x3a5634, 0x2a4028, 0x44603a];
  for (let i = 0; i < 9; i++) {
    const t = i / 9;
    const y = lerp(H * 0.35, H * 0.95, t) + (rng() - 0.5) * 0.3;
    const rr = lerp(1.4, 0.4, t) * lerp(0.8, 1.1, rng());
    const a = rng() * 6.3;
    const b = new THREE.IcosahedronGeometry(1, 0);
    b.scale(lerp(0.7, 1.0, rng()) * lerp(1.3, 0.6, t), lerp(0.55, 0.8, rng()), lerp(0.7, 1.0, rng()) * lerp(1.3, 0.6, t));
    b.rotateY(rng() * 3);
    b.translate(Math.cos(a) * rr * 0.6, y, Math.sin(a) * rr * 0.6);
    parts.push(paint(prep(b), cols[i % 4], 0.3, rng));
  }
  return merge(parts);
}

// Big sagebrush: a low, twiggy silver-grey mound made of many small puffs,
// vertex-coloured white so instances carry the tint.
export function sageGeometry(seed) {
  const rng = mulberry32(seed);
  const parts = [];
  for (let i = 0; i < 8; i++) {
    const a = rng() * 6.3, r = Math.sqrt(rng()) * 0.55;
    const s = lerp(0.24, 0.4, rng());
    const b = new THREE.IcosahedronGeometry(1, 0);
    b.scale(s, s * lerp(0.8, 1.3, rng()), s);
    b.rotateY(rng() * 3);
    const cx = Math.cos(a) * r, cy = s * 0.8 + (0.55 - r) * 0.5, cz = Math.sin(a) * r;
    b.translate(cx, cy, cz);
    const f = prep(b);
    // Foliage normals: bend each facet's normal toward "out from the bush"
    // so the mound shades soft and round instead of like faceted stone.
    const p = f.getAttribute('position'), nn = f.getAttribute('normal'), v = new THREE.Vector3(), w = new THREE.Vector3();
    for (let i = 0; i < p.count; i++) {
      v.set(p.getX(i) - cx, p.getY(i) - cy, p.getZ(i) - cz).normalize();
      w.set(p.getX(i), p.getY(i) + 0.2, p.getZ(i)).normalize();
      v.lerp(w, 0.5).normalize();
      nn.setXYZ(i, v.x, v.y, v.z);
    }
    parts.push(paint(f, 0xffffff, 0.35, rng));
  }
  // A few bare stems showing at the base.
  for (let i = 0; i < 4; i++) {
    const a = rng() * 6.3;
    parts.push(paint(prep(limb(V3(0, 0, 0), V3(Math.cos(a) * 0.35, 0.45, Math.sin(a) * 0.35), 0.04, 0.02, 3)), 0x6a5c4c, 0.1, rng));
  }
  return merge(parts);
}

// Teddy-bear cholla: a stubby trunk of jointed segments, the spines
// catching the light pale gold.
export function chollaGeometry(seed) {
  const rng = mulberry32(seed);
  const parts = [];
  const gold = 0xc8c07a, dark = 0x5a5040;
  const seg = (a, dir, len, r, depth) => {
    const b = a.clone().addScaledVector(dir, len);
    const g = new THREE.CylinderGeometry(r * 0.9, r, len, 5, 1, true);
    g.translate(0, len / 2, 0);
    g.applyQuaternion(new THREE.Quaternion().setFromUnitVectors(V3(0, 1, 0), dir.clone().normalize()));
    g.translate(a.x, a.y, a.z);
    parts.push(paint(prep(g), depth === 3 ? dark : gold, 0.18, rng));
    if (depth <= 0) return;
    const k = depth === 3 ? 3 : 1 + (rng() < 0.6 ? 1 : 0);
    for (let i = 0; i < k; i++) {
      const az = rng() * 6.3, up = lerp(0.2, 0.8, rng());
      const d2 = V3(Math.cos(az) * (1 - up), up, Math.sin(az) * (1 - up)).normalize();
      seg(b, d2, lerp(0.3, 0.45, rng()), r * 0.85, depth - 1);
    }
  };
  seg(V3(0, 0, 0), V3(0, 1, 0), lerp(0.7, 1.0, rng()), 0.12, 3);
  return merge(parts);
}

// Ocotillo: a fan of long whip canes from one root, tipped red.
export function ocotilloGeometry(seed) {
  const rng = mulberry32(seed);
  const parts = [];
  const N = 11;
  for (let i = 0; i < N; i++) {
    const az = (i / N) * Math.PI * 2 + rng() * 0.4;
    const lean = lerp(0.12, 0.45, rng());
    const L = lerp(3.2, 5.2, rng());
    const a = V3(0, 0, 0), m = V3(Math.cos(az) * lean * L * 0.4, L * 0.5, Math.sin(az) * lean * L * 0.4);
    const b = V3(Math.cos(az) * lean * L, L, Math.sin(az) * lean * L);
    parts.push(paint(prep(limb(a, m, 0.05, 0.04, 3)), 0x5a5a3c, 0.15, rng));
    parts.push(paint(prep(limb(m, b, 0.04, 0.02, 3)), 0x626a3c, 0.15, rng));
    const tip = limb(b, b.clone().add(V3(0, 0.35, 0)), 0.03, 0.005, 3);
    parts.push(paint(prep(tip), 0xc83a24, 0.1, rng));
  }
  return merge(parts);
}

// Tumbleweed: a ball of thin, criss-crossing twigs. Each twig is a sliver
// triangle, both faces, so the ball reads as a tangle, not a solid.
export function tumbleweedGeometry(seed) {
  const rng = mulberry32(seed);
  const pos = [], col = [];
  const c = new THREE.Color();
  for (let i = 0; i < 70; i++) {
    const u = rng() * 2 - 1, a = rng() * 6.283, r = Math.sqrt(1 - u * u);
    const p = V3(Math.cos(a) * r, u, Math.sin(a) * r).multiplyScalar(lerp(0.55, 1, rng()));
    const d = V3(rng() - 0.5, rng() - 0.5, rng() - 0.5).normalize().multiplyScalar(lerp(0.5, 0.9, rng()));
    const w = V3(rng() - 0.5, rng() - 0.5, rng() - 0.5).normalize().multiplyScalar(0.11);
    const A = p.clone().sub(d), B = p.clone().add(d), C = p.clone().add(w);
    pos.push(A.x, A.y, A.z, B.x, B.y, B.z, C.x, C.y, C.z, A.x, A.y, A.z, C.x, C.y, C.z, B.x, B.y, B.z);
    c.setHex(0xa88a5c).multiplyScalar(lerp(0.7, 1.15, rng()));
    for (let k = 0; k < 6; k++) col.push(c.r, c.g, c.b);
  }
  const g = new THREE.BufferGeometry();
  g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
  g.setAttribute('color', new THREE.Float32BufferAttribute(col, 3));
  g.computeVertexNormals();
  // A denser, darker core so the ball still reads when the twigs go
  // sub-pixel at distance.
  const core = new THREE.IcosahedronGeometry(0.62, 0);
  return merge([g, paint(prep(core), 0x6e5e44, 0.3, rng)]);
}

// ── Roadside furniture ───────────────────────────────────────────
// White delineator post with an amber reflector (unit: 1.2 m tall).
export function delineatorGeometry() {
  const p = new THREE.BoxGeometry(0.1, 1.2, 0.06); p.translate(0, 0.6, 0);
  const r = new THREE.BoxGeometry(0.08, 0.16, 0.02); r.translate(0, 1.0, 0.04);
  return merge([paint(prep(p), 0xeeeeea), paint(prep(r), 0xffa020)]);
}

// Light-tower beam: an open cone, apex at the origin pointing down -Y,
// brightness fading along it in the colour attribute (drawn additive).
export function beamGeometry(len = 26, r = 9) {
  const g = new THREE.CylinderGeometry(r * 0.05, r, len, 18, 4, true);
  g.translate(0, -len / 2, 0);
  const p = g.getAttribute('position');
  const a = new Float32Array(p.count * 3);
  for (let i = 0; i < p.count; i++) {
    const f = Math.pow(1 - (-p.getY(i) / len), 1.6);
    a[i * 3] = f; a[i * 3 + 1] = f * 0.97; a[i * 3 + 2] = f * 0.9;
  }
  g.setAttribute('color', new THREE.BufferAttribute(a, 3));
  g.deleteAttribute('uv');
  return g;
}

// ── The spectators' camp ─────────────────────────────────────────
// Class-C motorhome: cab-over body, stripes, a door and lit windows. The
// shell goes to 'solid' (vertex-coloured); windows are a separate glass
// geometry so they can glow at night. Front faces +Z, wheels on y = 0.
export function motorhome(rng) {
  const body = [], glass = [];
  const L = lerp(7.5, 9.5, rng()), W = 2.45, Hh = 3.2;
  const stripe = [0xb8322a, 0x2e5a8a, 0x8a5a2a, 0x3a7a5a][Math.floor(rng() * 4)];
  const box = (w, h, d, x, y, z, hex, into = body) => {
    const g = new THREE.BoxGeometry(w, h, d);
    g.translate(x, y + h / 2, z);
    into.push(paint(prep(g), hex));
  };
  box(W, Hh - 0.6, L - 2.2, 0, 0.6, -1.1, 0xeeebe2);                // coach box
  box(W, 0.9, 2.3, 0, Hh - 0.9, L / 2 - 1.9 - 0.3, 0xeeebe2);       // cab-over bunk
  box(W - 0.1, 1.6, 2.0, 0, 0.55, L / 2 - 1.0, 0xe4e0d6);           // cab
  box(W + 0.02, 0.22, L - 2.2, 0, 1.25, -1.1, stripe);
  box(W + 0.02, 0.12, L - 2.2, 0, 1.55, -1.1, 0x2a2a2a);
  box(W - 0.2, 0.3, 0.2, 0, 0.4, L / 2 + 0.05, 0x9a9a9a);           // bumper
  box(W, 0.35, 0.3, 0, 0.3, -L / 2 - 0.1, 0x3a3a3a);
  box(1.4, 0.3, 1.2, 0, Hh, -2.5, 0xd8d4ca);                        // roof AC
  for (const z of [L / 2 - 1.4, -L / 2 + 1.6]) for (const x of [-1.05, 1.05]) {
    const w = new THREE.CylinderGeometry(0.4, 0.4, 0.3, 10);
    w.rotateZ(Math.PI / 2); w.translate(x, 0.4, z);
    body.push(paint(prep(w), 0x1a1a1a));
  }
  box(W + 0.04, 0.55, 0.9, 0, 1.75, -2.8, 0x000000, glass);         // side windows
  box(W + 0.04, 0.55, 1.1, 0, 1.75, 0.6, 0x000000, glass);
  box(W - 0.3, 0.6, 0.06, 0, 1.2, L / 2 + 0.01, 0x000000, glass);   // windshield
  box(0.06, 1.8, 0.7, W / 2 + 0.01, 0.55, -0.6, 0x5a5a5a);           // door
  // Awning rolled out on the door side.
  box(0.08, 0.08, 4.2, W / 2 + 2.1, 2.45, -1.6, 0x9a9a9a);
  const aw = new THREE.BoxGeometry(2.2, 0.05, 4.2);
  aw.rotateZ(-0.12); aw.translate(W / 2 + 1.05, 2.62, -1.6);
  body.push(paint(prep(aw), stripe));
  for (const z of [-3.6, 0.4]) box(0.05, 2.4, 0.05, W / 2 + 2.1, 0, z, 0x9a9a9a);
  return { body: merge(body), glass: merge(glass), L };
}

// Dome tent (unit ≈ 2.4 m across) with a darker fly.
export function domeTentGeometry(seed) {
  const rng = mulberry32(seed);
  const g = new THREE.SphereGeometry(1.2, 8, 3, 0, Math.PI * 2, 0, Math.PI / 2);
  g.scale(1, 0.85, 1);
  const d = new THREE.BoxGeometry(0.7, 0.9, 0.05); d.translate(0, 0.45, 1.15);
  const pole = limb(V3(-1.2, 0, 0), V3(0, 1.05, 0), 0.02, 0.02, 3);
  const pole2 = limb(V3(0, 1.05, 0), V3(1.2, 0, 0), 0.02, 0.02, 3);
  return merge([paint(prep(g), 0xffffff, 0.12, rng), paint(prep(d), 0x303030), paint(prep(pole), 0x333333), paint(prep(pole2), 0x333333)]);
}

// Folding camp chair + cooler, a little scene at each fire.
export function campSetGeometry(seed) {
  const rng = mulberry32(seed);
  const parts = [];
  const cols = [0x2e5a8a, 0xb8322a, 0x3a6a3a, 0x303030];
  for (let i = 0; i < 3; i++) {
    const a = (i / 3) * Math.PI * 2 + rng() * 0.5;
    const x = Math.cos(a) * 2.2, z = Math.sin(a) * 2.2;
    const c = cols[Math.floor(rng() * 4)];
    const seat = new THREE.BoxGeometry(0.55, 0.06, 0.5); seat.translate(0, 0.45, 0);
    const back = new THREE.BoxGeometry(0.55, 0.55, 0.05); back.rotateX(-0.2); back.translate(0, 0.72, 0.27);
    const cp = [paint(prep(seat), c), paint(prep(back), c)];
    for (const lx of [-0.25, 0.25]) for (const lz of [-0.22, 0.22]) {
      const l = new THREE.BoxGeometry(0.03, 0.47, 0.03); l.rotateX(lz > 0 ? 0.35 : -0.35); l.translate(lx, 0.22, 0);
      cp.push(paint(prep(l), 0x222222));
    }
    const chair = merge(cp);
    chair.rotateY(Math.atan2(-x, -z) + Math.PI);
    chair.translate(x, 0, z);
    parts.push(chair);
  }
  const cooler = new THREE.BoxGeometry(0.7, 0.45, 0.42); cooler.translate(1.3, 0.22, -1.9);
  parts.push(paint(prep(cooler), rng() < 0.5 ? 0x2a6ab8 : 0xd83a2a));
  const lid = new THREE.BoxGeometry(0.72, 0.08, 0.44); lid.translate(1.3, 0.47, -1.9);
  parts.push(paint(prep(lid), 0xf0f0ea));
  // Fire ring: a circle of stones.
  for (let k = 0; k < 9; k++) {
    const a = (k / 9) * Math.PI * 2;
    const s = new THREE.IcosahedronGeometry(0.16, 0); s.scale(1, 0.7, 1); s.translate(Math.cos(a) * 0.6, 0.06, Math.sin(a) * 0.6);
    parts.push(paint(prep(s), 0x6a6258, 0.3, rng));
  }
  for (let k = 0; k < 4; k++) {
    const l = new THREE.BoxGeometry(0.8, 0.1, 0.1); l.rotateZ(0.35); l.rotateY((k / 4) * Math.PI); l.translate(0, 0.18, 0);
    parts.push(paint(prep(l), 0x3a2a1c));
  }
  return merge(parts);
}

// A standing spectator: legs, torso, head (≈1.75 m), white so instances
// carry a shirt colour; skin/jeans baked in.
export function personGeometry(seed) {
  const rng = mulberry32(seed);
  const legs = new THREE.BoxGeometry(0.34, 0.85, 0.2); legs.translate(0, 0.43, 0);
  const torso = new THREE.BoxGeometry(0.44, 0.62, 0.24); torso.translate(0, 1.16, 0);
  const arms = new THREE.BoxGeometry(0.62, 0.5, 0.14); arms.translate(0, 1.2, 0.02);
  const head = new THREE.IcosahedronGeometry(0.12, 0); head.scale(1, 1.2, 1); head.translate(0, 1.62, 0);
  return merge([paint(prep(legs), 0x2e3a52, 0.1, rng), paint(prep(torso), 0xffffff), paint(prep(arms), 0xe8e8e8), paint(prep(head), 0xc89a78)]);
}

// ── Silver Lake decals ───────────────────────────────────────────
// Cracked playa mud: polygon plates with dark, slightly curled edges on a
// transparent ground, fading out at the tile's rim so tiles overlap
// seamlessly. Channels: rgb colour, alpha coverage.
export function crackDecalTexture(seed = 5) {
  const S = 512;
  const cv = document.createElement('canvas');
  cv.width = cv.height = S;
  const g = cv.getContext('2d');
  const img = g.createImageData(S, S);
  const rng = mulberry32(seed);
  const cells = [];
  const N = 15;
  for (let j = 0; j < N; j++) for (let i = 0; i < N; i++) cells.push([(i + 0.15 + rng() * 0.7) * S / N, (j + 0.15 + rng() * 0.7) * S / N, rng()]);
  // Grid lookup: only the 3×3 neighbourhood can hold the nearest seeds.
  const cellAt = (i, j) => cells[clamp(j, 0, N - 1) * N + clamp(i, 0, N - 1)];
  for (let y = 0; y < S; y++) for (let x = 0; x < S; x++) {
    const ci = Math.floor(x / (S / N)), cj = Math.floor(y / (S / N));
    let d1 = 1e9, d2 = 1e9, own = null;
    for (let dj = -1; dj <= 1; dj++) for (let di = -1; di <= 1; di++) {
      const c = cellAt(ci + di, cj + dj);
      const d = Math.pow(x - c[0], 2) + Math.pow(y - c[1], 2);
      if (d < d1) { d2 = d1; d1 = d; own = c; } else if (d < d2) d2 = d;
    }
    const e = Math.sqrt(d2) - Math.sqrt(d1);
    const k = (y * S + x) * 4;
    const r = Math.hypot(x - S / 2, y - S / 2) / (S / 2);
    const rim = 1 - smoothstep(0.3, 0.98, r);
    // Crack: dark line; lip: a pale curled edge just inside the plate.
    const crack = 1 - smoothstep(1.2, 3.2, e);
    const lip = smoothstep(2.5, 4, e) * (1 - smoothstep(4, 9, e));
    const plate = own[2];
    let R = 226 + plate * 18, G = R * 0.955, B = R * 0.9, A = 0.04 + plate * 0.1;
    R += lip * 22; G += lip * 22; B += lip * 22; A = Math.max(A, lip * 0.4);
    R = lerp(R, 70, crack); G = lerp(G, 62, crack); B = lerp(B, 56, crack); A = lerp(A, 0.85, crack);
    img.data[k] = R; img.data[k + 1] = G; img.data[k + 2] = B; img.data[k + 3] = 255 * A * rim;
  }
  g.putImageData(img, 0, 0);
  const t = new THREE.CanvasTexture(cv);
  t.colorSpace = THREE.SRGBColorSpace;
  t.anisotropy = 8;
  return t;
}
