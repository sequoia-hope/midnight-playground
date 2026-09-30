import * as THREE from 'three';
import { mergeGeometries } from 'three/addons/utils/BufferGeometryUtils.js';
import { glowTexture } from '../world/textures.js';

// Procedural vehicles — no assets.
//
// Bodies are lofts: at stations along the car we take the side profile's
// roof-line and sill-line heights (wheel arches cut into the sill) and sweep a
// rounded cross-section between them whose half-width varies with height and
// length (plan-view corner rounding, tumblehome, hips, a character-line ridge,
// flared arches with a lip). Each face is assigned a material by where it sits
// (paint, glass, black trim, stripes), which is how windows, pillars and
// racing stripes come out of the same loft.
//
// Everything that sits ON a panel — light units, grilles, vents, panel gaps,
// window surrounds — is a decal: an outline drawn in a 2D view (top, side,
// front or rear) and projected onto the loft surface, lifted a few mm along
// the surface normal. That keeps lamps and shut lines hugging the curved
// body instead of being boxes stuck through it.
//
// Parts are gathered into per-material buckets and merged; the merged
// geometry is cached per kind/LOD/variant so traffic shares it. Each instance
// owns only its paint (racers) and its light materials, so brake/head lights
// work per car. On the high LOD, vertex colours carry detail inside a bucket
// (lens housings vs lit LED strips, seat leather, soot in exhaust tips); the
// light materials multiply their emission by it, so one bucket can hold a dim
// lens and a blazing LED line. The low LOD has no vertex colours (parked cars
// get re-merged by material in the scenery, which drops them).
//
// Frame: origin on the ground centred between the axles, +Z forward, +Y up,
// right = -X. Left-side wheels sit at +X.

const RACERS = new Set(['sports', 'muscle', 'super', 'rally', 'electric']);
const TRAFFIC_COLORS = [0xb8bcc2, 0x2b2f36, 0xe8e6e0, 0x7a1f1f, 0x1f3a5f, 0x4a5a3a, 0x8c7a5a, 0x5f6670, 0x9a9a92, 0x243048];
const PI = Math.PI;
const clamp01 = (v) => Math.min(1, Math.max(0, v));
const smooth = (a, b, v) => { const t = clamp01((v - a) / (b - a)); return t * t * (3 - 2 * t); };
const bump = (v, c, w) => Math.exp(-(((v - c) / w) ** 2));
const inRange = (v, a, b) => v >= a && v <= b;
const AMBER = [1, 0.42, 0.06];

// ── textures ──────────────────────────────────────────────────────
function canvasTex(size, draw) {
  const c = document.createElement('canvas');
  c.width = c.height = size;
  draw(c.getContext('2d'), size);
  const t = new THREE.CanvasTexture(c);
  t.wrapS = t.wrapT = THREE.RepeatWrapping;
  t.colorSpace = THREE.SRGBColorSpace;
  t.anisotropy = 4;
  return t;
}
// 2×2 twill carbon weave: tows alternate direction on a diagonal staircase,
// each shaded across its width so the weave catches the light.
function carbonTexture() {
  return canvasTex(64, (g, S) => {
    const n = 8, c = S / n;
    for (let i = 0; i < n; i++) {
      for (let j = 0; j < n; j++) {
        const horiz = ((i + j) & 3) < 2;
        const grd = horiz ? g.createLinearGradient(0, j * c, 0, (j + 1) * c) : g.createLinearGradient(i * c, 0, (i + 1) * c, 0);
        const b = horiz ? 44 : 26;
        grd.addColorStop(0, `rgb(${b - 16},${b - 16},${b - 13})`);
        grd.addColorStop(0.5, `rgb(${b + 16},${b + 16},${b + 20})`);
        grd.addColorStop(1, `rgb(${b - 16},${b - 16},${b - 13})`);
        g.fillStyle = grd;
        g.fillRect(i * c, j * c, c, c);
      }
    }
  });
}

// ── shared materials ──────────────────────────────────────────────
// Two sets: 'hi' materials read vertex colours, 'lo' ones don't (see header).
let SHARED = null;
function shared() {
  if (SHARED) return SHARED;
  const mk = (vc) => ({
    glass: vc
      ? new THREE.MeshPhysicalMaterial({ color: 0x0a0e14, metalness: 0.15, roughness: 0.04, clearcoat: 1, clearcoatRoughness: 0.02, transparent: true, opacity: 0.78, vertexColors: true })
      : new THREE.MeshStandardMaterial({ color: 0x151c26, metalness: 0.55, roughness: 0.16 }),
    trim: new THREE.MeshStandardMaterial({ color: 0x111214, metalness: 0.25, roughness: 0.55, vertexColors: vc }),
    chrome: new THREE.MeshStandardMaterial({ color: 0x9ea2a8, metalness: 1, roughness: 0.2, vertexColors: vc }),
    plate: new THREE.MeshStandardMaterial({ color: 0xe9e7df, metalness: 0, roughness: 0.6, vertexColors: vc }),
    tire: new THREE.MeshStandardMaterial({ color: 0x161616, metalness: 0, roughness: 0.9, vertexColors: vc }),
    rim: new THREE.MeshStandardMaterial({ color: 0xc4c8ce, metalness: 0.95, roughness: 0.22, vertexColors: vc }),
    rimDark: new THREE.MeshStandardMaterial({ color: 0x2c2e33, metalness: 0.85, roughness: 0.3, vertexColors: vc }),
    rimSteel: new THREE.MeshStandardMaterial({ color: 0x9a9da3, metalness: 0.7, roughness: 0.38, vertexColors: vc }),
    rimTractor: new THREE.MeshStandardMaterial({ color: 0xbfa35e, metalness: 0.1, roughness: 0.75, vertexColors: vc }),
    rimWhite: new THREE.MeshStandardMaterial({ color: 0xe8e8e4, metalness: 0.3, roughness: 0.35, vertexColors: vc }),
    rimAero: new THREE.MeshStandardMaterial({ color: 0x9aa0a8, metalness: 0.9, roughness: 0.2, vertexColors: vc }),
    rimChrome: new THREE.MeshStandardMaterial({ color: 0xc4c8cc, metalness: 1, roughness: 0.15, vertexColors: vc }),
    rimGold: new THREE.MeshStandardMaterial({ color: 0xc9a14a, metalness: 0.9, roughness: 0.28, vertexColors: vc }),
    // Brake discs, barrel insides and backing plates share one metal
    // material; vertex colour sets how bright each part is.
    brake: new THREE.MeshStandardMaterial({ color: 0x8a8d92, metalness: 0.8, roughness: 0.42, vertexColors: vc }),
    cargo: new THREE.MeshStandardMaterial({ color: 0xe4e2dc, metalness: 0.1, roughness: 0.55, vertexColors: vc }),
    seat: new THREE.MeshStandardMaterial({ color: 0x1c1a18, metalness: 0, roughness: 0.9, vertexColors: vc }),
  });
  SHARED = { hi: mk(true), lo: mk(false) };
  SHARED.hi.carbon = new THREE.MeshPhysicalMaterial({ color: 0x8c8c8c, map: carbonTexture(), metalness: 0.3, roughness: 0.5, clearcoat: 0.7, clearcoatRoughness: 0.12 });
  SHARED.lo.carbon = new THREE.MeshStandardMaterial({ color: 0x202124, metalness: 0.3, roughness: 0.45 });
  SHARED.caliper = [0xc41d1d, 0xe8b400, 0x1d5fc4].map((color) => new THREE.MeshStandardMaterial({ color, metalness: 0.3, roughness: 0.4 }));
  return SHARED;
}
const stripeMats = new Map();
function stripeMat(color, hi) {
  const key = color + (hi ? 'h' : 'l');
  if (!stripeMats.has(key)) {
    stripeMats.set(key, hi
      ? new THREE.MeshPhysicalMaterial({ color, metalness: 0.3, roughness: 0.35, clearcoat: 1, clearcoatRoughness: 0.05, vertexColors: true })
      : new THREE.MeshStandardMaterial({ color, metalness: 0.2, roughness: 0.5 }));
  }
  return stripeMats.get(key);
}
const lowPaints = new Map();
function lowPaint(color, rough = 0.5) {
  const key = color + ':' + rough;
  if (!lowPaints.has(key)) lowPaints.set(key, new THREE.MeshStandardMaterial({ color, metalness: 0.3, roughness: rough }));
  return lowPaints.get(key);
}
// Light material whose emission is scaled by the vertex colour (and whose
// albedo ignores it), so lens housings stay dim while LED strips blaze.
function lightMat(params, hi) {
  const m = new THREE.MeshStandardMaterial({ ...params, vertexColors: hi });
  if (hi) {
    m.onBeforeCompile = (s) => {
      s.fragmentShader = s.fragmentShader
        .replace('#include <color_fragment>', '')
        .replace('#include <emissivemap_fragment>', '#include <emissivemap_fragment>\n#ifdef USE_COLOR\n\ttotalEmissiveRadiance *= vColor.rgb;\n#endif');
    };
    m.customProgramCacheKey = () => 'car-light-vc';
  }
  return m;
}

// ── profile helpers ───────────────────────────────────────────────
// Piecewise-linear y(z) through points sorted by z.
function interp(pts, z) {
  if (z <= pts[0][0]) return pts[0][1];
  for (let i = 1; i < pts.length; i++) {
    if (z <= pts[i][0]) {
      const a = pts[i - 1], b = pts[i];
      const t = (z - a[0]) / (b[0] - a[0] || 1e-9);
      return a[1] + (b[1] - a[1]) * t;
    }
  }
  return pts[pts.length - 1][1];
}
// Monotone cubic through the profile points: a smooth roof/bonnet line with
// no overshoot, so few points still give a flowing silhouette.
function spline(pts) {
  const n = pts.length;
  if (n < 3) return (z) => interp(pts, z);
  const d = [], m = new Array(n);
  for (let i = 0; i < n - 1; i++) d.push((pts[i + 1][1] - pts[i][1]) / (pts[i + 1][0] - pts[i][0] || 1e-9));
  m[0] = d[0]; m[n - 1] = d[n - 2];
  for (let i = 1; i < n - 1; i++) m[i] = d[i - 1] * d[i] <= 0 ? 0 : (d[i - 1] + d[i]) / 2;
  for (let i = 0; i < n - 1; i++) {
    if (d[i] === 0) { m[i] = m[i + 1] = 0; continue; }
    const a = m[i] / d[i], b = m[i + 1] / d[i], s = a * a + b * b;
    if (s > 9) { const t = 3 / Math.sqrt(s); m[i] = t * a * d[i]; m[i + 1] = t * b * d[i]; }
  }
  return (z) => {
    if (z <= pts[0][0]) return pts[0][1];
    if (z >= pts[n - 1][0]) return pts[n - 1][1];
    let i = 0;
    while (z > pts[i + 1][0]) i++;
    const h = pts[i + 1][0] - pts[i][0], t = (z - pts[i][0]) / h;
    const t2 = t * t, t3 = t2 * t;
    return (2 * t3 - 3 * t2 + 1) * pts[i][1] + (t3 - 2 * t2 + t) * h * m[i] + (-2 * t3 + 3 * t2) * pts[i + 1][1] + (t3 - t2) * h * m[i + 1];
  };
}
// Sill line with semicircular wheel arches cut into it.
function sill(pts, arches) {
  return (z) => {
    let y = interp(pts, z);
    for (const [zc, yc, R] of arches) {
      const d = z - zc;
      if (Math.abs(d) <= R) y = Math.max(y, yc + Math.sqrt(R * R - d * d));
    }
    return y;
  };
}
// Station list: uniform spacing plus every key z (profile vertices, arch
// samples, window edges) so creases land exactly on a station.
function stations(z0, z1, n, extra = []) {
  const zs = [];
  for (let i = 0; i <= n; i++) zs.push(z0 + ((z1 - z0) * i) / n);
  for (const z of extra) if (z > z0 && z < z1) zs.push(z);
  zs.sort((a, b) => a - b);
  const out = [];
  for (const z of zs) if (!out.length || z - out[out.length - 1] > 0.004) out.push(z);
  if (z1 - out[out.length - 1] < 0.004) out[out.length - 1] = z1;
  return out;
}
function archZs(arches, n) {
  const out = [];
  for (const [zc, , R] of arches) {
    out.push(zc - R - 0.006, zc + R + 0.006, zc - R - 0.05, zc + R + 0.05, zc - R - 0.12, zc + R + 0.12);
    for (let k = 0; k <= n; k++) out.push(zc + R * Math.cos((PI * k) / n));
  }
  return out;
}
// Extra stations packed towards both ends, where plan-view rounding bends fast.
function endZs(z0, z1, r, n) {
  const out = [];
  for (let k = 1; k <= n; k++) {
    const d = r * (1 - Math.cos((k / (n + 1)) * PI / 2));
    out.push(z0 + d, z1 - d);
  }
  return out;
}

// ── loft ─────────────────────────────────────────────────────────
// o: { zs, top(z), bot(z), halfW(y,z), rTop, rBot, crown, nb, nt,
//      side: [absolute y levels], sideN, crownX: [absolute x], nCrown,
//      mat(tag, y, z, ax) -> bucket, capMat(front) -> bucket, caps: bool,
//      col(x, y, z) -> vertex colour }
// Tags: 0 underside, 1 flank, 2 shoulder, 3 top.
// Returns the half ring from the bottom centre up to the top centre.
function halfRing(o, z) {
  const yt = o.top(z);
  const yb = Math.min(o.bot(z), yt - 0.004);
  const h = yt - yb;
  const rb = Math.min(o.rBot, h * 0.3);
  const crown = Math.min(o.crown, h * 0.12);
  const rt = Math.min(o.rTop, Math.max(0, h - rb - crown) * 0.6);
  const H = [{ x: 0, y: yb, t: 0 }];
  const hwb = Math.max(0.004, o.halfW(yb + rb, z));
  H.push({ x: Math.max(0.002, hwb - rb), y: yb, t: 0 });
  for (let k = 1; k <= o.nb; k++) {
    const a = -PI / 2 + (PI / 2) * (k / o.nb);
    H.push({ x: Math.max(0.003, hwb - rb + rb * Math.cos(a)), y: yb + rb + rb * Math.sin(a), t: 1 });
  }
  const yLo = yb + rb, yHi = yt - crown - rt;
  const lv = [];
  for (let k = 1; k <= o.sideN; k++) lv.push(yLo + ((yHi - yLo) * k) / (o.sideN + 1));
  for (const y of o.side || []) lv.push(y);
  lv.sort((a, b) => a - b);
  // Clamp into the flank and keep strictly ascending: coincident ring points
  // give zero-area faces, and a vertex with only those gets a NaN normal
  // (which bloom smears into a black blotch).
  const n = lv.length, eps = Math.max(1e-4, Math.min(0.002, (yHi - yLo) / (n + 2)));
  for (let k = 0; k < n; k++) {
    let y = Math.min(yHi - eps * (n - k), Math.max(yLo + eps * (k + 1), lv[k]));
    if (k && y <= lv[k - 1] + eps * 0.5) y = lv[k - 1] + eps * 0.5;
    lv[k] = y;
  }
  for (const y of lv) H.push({ x: Math.max(0.004, o.halfW(y, z)), y, t: 1 });
  const hwt = Math.max(0.006, o.halfW(yHi, z));
  const cx = Math.max(0.004, hwt - rt), cy = yHi;
  for (let k = 0; k <= o.nt; k++) {
    const a = (PI / 2) * (k / o.nt);
    H.push({ x: cx + rt * Math.cos(a), y: cy + rt * Math.sin(a) + (k === 0 ? 0 : 1e-5 * k), t: 2 });
  }
  const xs = [];
  for (let k = o.nCrown; k >= 1; k--) xs.push((cx * k) / (o.nCrown + 1));
  for (const x of o.crownX || []) xs.push(Math.min(cx * 0.995, x));
  xs.sort((a, b) => b - a);
  for (let k = 0; k < xs.length; k++) {
    const lim = cx * (1 - (0.004 * (k + 1)));
    let x = Math.min(xs[k], lim);
    if (k && x >= xs[k - 1] - 1e-4) x = xs[k - 1] - 1e-4;
    xs[k] = Math.max(1e-4 * (xs.length - k), x);
  }
  for (const x of xs) {
    const f = cx > 0 ? x / cx : 0;
    H.push({ x, y: yt - crown * f * f, t: 3 });
  }
  H.push({ x: 0, y: yt, t: 3 });
  return H;
}

// Replace NaN / zero normals (degenerate faces) with a sane fallback.
function fixNormals(g, fb = [0, 1, 0]) {
  const N = g.attributes.normal;
  if (!N) return g;
  const a = N.array;
  for (let i = 0; i < a.length; i += 3) {
    const l = Math.hypot(a[i], a[i + 1], a[i + 2]);
    if (!(l > 1e-8)) { a[i] = fb[0]; a[i + 1] = fb[1]; a[i + 2] = fb[2]; }
  }
  return g;
}

function loft(o) {
  const halves = o.zs.map((z) => halfRing(o, z));
  const rings = halves.map((half) => {
    const ring = half.slice();
    for (let k = half.length - 2; k >= 1; k--) ring.push({ x: -half[k].x, y: half[k].y, t: half[k].t });
    return ring;
  });
  const M = rings[0].length;
  const pos = [], cols = [];
  rings.forEach((ring, i) => {
    for (const p of ring) {
      pos.push(p.x, p.y, o.zs[i]);
      const c = o.col ? o.col(p.x, p.y, o.zs[i]) : 1;
      if (Array.isArray(c)) cols.push(c[0], c[1], c[2]); else cols.push(c, c, c);
    }
  });
  const buckets = {};
  const all = [];
  for (let i = 0; i < rings.length - 1; i++) {
    for (let j = 0; j < M; j++) {
      const j2 = (j + 1) % M;
      const A = i * M + j, B = i * M + j2, C = (i + 1) * M + j2, D = (i + 1) * M + j;
      const pa = rings[i][j], pb = rings[i][j2], pc = rings[i + 1][j2], pd = rings[i + 1][j];
      const tag = Math.min(pa.t, pb.t);
      const y = (pa.y + pb.y + pc.y + pd.y) / 4;
      const ax = (Math.abs(pa.x) + Math.abs(pb.x) + Math.abs(pc.x) + Math.abs(pd.x)) / 4;
      const z = (o.zs[i] + o.zs[i + 1]) / 2;
      const b = o.mat(tag, y, z, ax);
      (buckets[b] ||= []).push(A, B, C, A, C, D);
      all.push(A, B, C, A, C, D);
    }
  }
  // Smooth normals over the whole skin, then split per material.
  const skin = new THREE.BufferGeometry();
  skin.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
  skin.setIndex(all);
  skin.computeVertexNormals();
  fixNormals(skin);
  const P = skin.attributes.position.array, N = skin.attributes.normal.array;
  const out = {};
  for (const [b, idx] of Object.entries(buckets)) {
    const p = new Float32Array(idx.length * 3), n = new Float32Array(idx.length * 3), c = new Float32Array(idx.length * 3);
    idx.forEach((v, k) => {
      for (let e = 0; e < 3; e++) { p[k * 3 + e] = P[v * 3 + e]; n[k * 3 + e] = N[v * 3 + e]; c[k * 3 + e] = cols[v * 3 + e]; }
    });
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.BufferAttribute(p, 3));
    g.setAttribute('normal', new THREE.BufferAttribute(n, 3));
    g.setAttribute('color', new THREE.BufferAttribute(c, 3));
    out[b] = g;
  }
  skin.dispose();
  // Flat end caps (fan from the ring centroid).
  if (o.caps !== false) {
    for (const [ri, dir] of [[0, -1], [rings.length - 1, 1]]) {
      const ring = rings[ri], z = o.zs[ri];
      const ys = ring.map((p) => p.y);
      if (Math.max(...ys) - Math.min(...ys) < 0.01) continue;
      const cy = ys.reduce((a, b) => a + b, 0) / ys.length;
      const T = new Tris();
      const nn = [0, 0, dir];
      const cc = o.col ? o.col(0, cy, z) : 1;
      for (let j = 0; j < M; j++) {
        const a = ring[j], b = ring[(j + 1) % M];
        T.tri([0, cy, z], [a.x, a.y, z], [b.x, b.y, z], nn, nn, nn, cc);
      }
      const g = T.geo();
      const b = o.capMat ? o.capMat(dir > 0) : o.mat(1, cy, z, 0);
      out[b] = out[b] ? mergeGeometries([out[b], g]) : g;
    }
  }
  return { geo: out, S: new Surf(o) };
}

// ── triangle soup builder ─────────────────────────────────────────
const colArr = (c) => (Array.isArray(c) ? c : [c, c, c]);
class Tris {
  constructor() { this.p = []; this.n = []; this.c = []; }
  // Triangle with per-vertex normals, wound so its face agrees with them.
  tri(a, b, c, na, nb, nc, ca = 1, cb = ca, cc = ca) {
    const ux = b[0] - a[0], uy = b[1] - a[1], uz = b[2] - a[2];
    const vx = c[0] - a[0], vy = c[1] - a[1], vz = c[2] - a[2];
    const fx = uy * vz - uz * vy, fy = uz * vx - ux * vz, fz = ux * vy - uy * vx;
    if (fx * (na[0] + nb[0] + nc[0]) + fy * (na[1] + nb[1] + nc[1]) + fz * (na[2] + nb[2] + nc[2]) < 0) {
      [b, c] = [c, b]; [nb, nc] = [nc, nb]; [cb, cc] = [cc, cb];
    }
    this.p.push(a[0], a[1], a[2], b[0], b[1], b[2], c[0], c[1], c[2]);
    this.n.push(na[0], na[1], na[2], nb[0], nb[1], nb[2], nc[0], nc[1], nc[2]);
    const A = colArr(ca), B = colArr(cb), C = colArr(cc);
    this.c.push(A[0], A[1], A[2], B[0], B[1], B[2], C[0], C[1], C[2]);
  }
  quad(a, b, c, d, na, nb, nc, nd, ca = 1, cb = ca, cc = ca, cd = ca) {
    this.tri(a, b, c, na, nb, nc, ca, cb, cc);
    this.tri(a, c, d, na, nc, nd, ca, cc, cd);
  }
  // Flat-shaded triangle facing roughly along `hint`.
  flat(a, b, c, hint, col = 1) {
    const ux = b[0] - a[0], uy = b[1] - a[1], uz = b[2] - a[2];
    const vx = c[0] - a[0], vy = c[1] - a[1], vz = c[2] - a[2];
    let fx = uy * vz - uz * vy, fy = uz * vx - ux * vz, fz = ux * vy - uy * vx;
    const l = Math.hypot(fx, fy, fz);
    if (!(l > 1e-12)) return;
    fx /= l; fy /= l; fz /= l;
    if (fx * hint[0] + fy * hint[1] + fz * hint[2] < 0) { fx = -fx; fy = -fy; fz = -fz; }
    const f = [fx, fy, fz];
    this.tri(a, b, c, f, f, f, col);
  }
  flatQuad(a, b, c, d, hint, col = 1) { this.flat(a, b, c, hint, col); this.flat(a, c, d, hint, col); }
  // Append a copy mirrored across X (winding is fixed up by tri()).
  mirrorX() {
    const p = this.p.slice(), n = this.n.slice(), c = this.c.slice();
    for (let i = 0; i < p.length; i += 9) {
      const v = (k) => [-p[i + k * 3], p[i + k * 3 + 1], p[i + k * 3 + 2]];
      const w = (k) => [-n[i + k * 3], n[i + k * 3 + 1], n[i + k * 3 + 2]];
      const q = (k) => [c[i + k * 3], c[i + k * 3 + 1], c[i + k * 3 + 2]];
      this.tri(v(0), v(1), v(2), w(0), w(1), w(2), q(0), q(1), q(2));
    }
    return this;
  }
  geo() {
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.Float32BufferAttribute(this.p, 3));
    g.setAttribute('normal', new THREE.Float32BufferAttribute(this.n, 3));
    g.setAttribute('color', new THREE.Float32BufferAttribute(this.c, 3));
    return fixNormals(g);
  }
}

// ── surface sampling + decals ─────────────────────────────────────
// A view maps 2D (u, v) to a point on the loft:
//   top:   u = x, v = z  (dropped onto the upper surface)
//   side:  u = z, v = y  (pushed onto the +X flank)
//   front: u = x, v = y  (pushed back onto the nose)
//   rear:  u = x, v = y  (pushed forward onto the tail)
const VIEW_DIR = { top: [0, 1, 0], side: [1, 0, 0], front: [0, 0, 1], rear: [0, 0, -1] };
class Surf {
  constructor(o) { this.o = o; this.z0 = o.zs[0]; this.z1 = o.zs[o.zs.length - 1]; this.cache = new Map(); }
  ring(z) {
    const k = Math.round(z * 2e4);
    let r = this.cache.get(k);
    if (!r) { r = halfRing(this.o, z); if (this.cache.size < 4000) this.cache.set(k, r); }
    return r;
  }
  topY(z, x) {
    const H = this.ring(z);
    x = Math.abs(x);
    for (let i = H.length - 1; i > 0; i--) {
      const a = H[i], b = H[i - 1];
      if ((x >= a.x && x <= b.x) || (x <= a.x && x >= b.x)) return a.y + (b.y - a.y) * ((x - a.x) / ((b.x - a.x) || 1e-9));
    }
    let m = H[0];
    for (const p of H) if (p.x > m.x) m = p;
    return m.y;
  }
  sideX(z, y) {
    const H = this.ring(z);
    let best = -1;
    for (let i = 1; i < H.length; i++) {
      const a = H[i - 1], b = H[i];
      if ((y >= a.y && y <= b.y) || (y <= a.y && y >= b.y)) {
        const x = a.x + (b.x - a.x) * ((y - a.y) / ((b.y - a.y) || 1e-9));
        if (x > best) best = x;
      }
    }
    if (best < 0) best = y < H[0].y ? H[1].x : H[H.length - 2].x;
    return best;
  }
  inside(z, x, y) {
    const H = halfRing(this.o, z);
    const ax = Math.abs(x);
    let c = false;
    for (let i = 0, j = H.length - 1; i < H.length; j = i++) {
      const a = H[i], b = H[j];
      if ((a.y > y) !== (b.y > y) && ax < ((b.x - a.x) * (y - a.y)) / (b.y - a.y) + a.x) c = !c;
    }
    return c;
  }
  endZ(x, y, front) {
    const span = Math.min(1.4, (this.z1 - this.z0) * 0.45);
    let zo = front ? this.z1 : this.z0, zi = front ? this.z1 - span : this.z0 + span;
    if (this.inside(zo, x, y)) return zo;
    // Outside the silhouette: no surface to land on (the decal drops it,
    // rather than smearing it along the flank).
    if (!this.inside(zi, x, y)) return NaN;
    for (let k = 0; k < 18; k++) {
      const m = (zo + zi) / 2;
      if (this.inside(m, x, y)) zi = m; else zo = m;
    }
    return (zo + zi) / 2;
  }
  map(view, u, v) {
    switch (view) {
      case 'top': return [u, this.topY(v, u), v];
      case 'side': return [this.sideX(u, v), v, u];
      case 'front': return [u, v, this.endZ(u, v, true)];
      default: return [u, v, this.endZ(u, v, false)];
    }
  }
  // Surface point and normal (facing the viewer), lifted `off` along it;
  // null where the view ray misses the body.
  at(view, u, v, off = 0) {
    const h = 0.005;
    const p = this.map(view, u, v);
    if (Number.isNaN(p[2])) return null;
    let pu = this.map(view, u + h, v), pv = this.map(view, u, v + h);
    if (Number.isNaN(pu[2])) pu = this.map(view, u - h, v).map((c, i) => 2 * p[i] - c);
    if (Number.isNaN(pv[2])) pv = this.map(view, u, v - h).map((c, i) => 2 * p[i] - c);
    const ax = pu[0] - p[0], ay = pu[1] - p[1], az = pu[2] - p[2];
    const bx = pv[0] - p[0], by = pv[1] - p[1], bz = pv[2] - p[2];
    let n = [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx];
    const d = VIEW_DIR[view];
    let l = Math.hypot(n[0], n[1], n[2]);
    if (!(l > 1e-10)) { n = d.slice(); l = 1; }
    n = [n[0] / l, n[1] / l, n[2] / l];
    if (n[0] * d[0] + n[1] * d[1] + n[2] * d[2] < 0) n = [-n[0], -n[1], -n[2]];
    return { p: [p[0] + n[0] * off, p[1] + n[1] * off, p[2] + n[2] * off], n };
  }
}

// Filled decal: polygon [[u,v],...] in a view, meshed in rows along v (each
// row spans the polygon's u-extent, so shapes should be u-convex per row).
function decal(S, view, poly, { off = 0.004, nu = 6, nv = 3, col = 1, mirror = false } = {}) {
  let vmin = Infinity, vmax = -Infinity;
  for (const [, v] of poly) { vmin = Math.min(vmin, v); vmax = Math.max(vmax, v); }
  const vs = [];
  for (let i = 0; i <= nv; i++) vs.push(vmin + ((vmax - vmin) * i) / nv);
  for (const [, v] of poly) vs.push(v);
  vs.sort((a, b) => a - b);
  const rows = [];
  for (const v of vs) {
    if (rows.length && v - rows[rows.length - 1].v < 1e-4) continue;
    const us = [];
    for (let i = 0; i < poly.length; i++) {
      const a = poly[i], b = poly[(i + 1) % poly.length];
      if (Math.abs(a[1] - b[1]) < 1e-9) { if (Math.abs(a[1] - v) < 1e-6) us.push(a[0], b[0]); continue; }
      if (v >= Math.min(a[1], b[1]) - 1e-7 && v <= Math.max(a[1], b[1]) + 1e-7) us.push(a[0] + ((b[0] - a[0]) * (v - a[1])) / (b[1] - a[1]));
    }
    if (!us.length) continue;
    const u0 = Math.min(...us), u1 = Math.max(...us);
    const pts = [];
    for (let j = 0; j <= nu; j++) pts.push(S.at(view, u0 + ((u1 - u0) * j) / nu, v, off));
    rows.push({ v, pts });
  }
  const T = new Tris();
  const cf = typeof col === 'function' ? col : () => col;
  for (let i = 0; i < rows.length - 1; i++) {
    const A = rows[i].pts, B = rows[i + 1].pts;
    for (let j = 0; j < nu; j++) {
      if (!A[j] || !A[j + 1] || !B[j] || !B[j + 1]) continue;
      T.quad(A[j].p, A[j + 1].p, B[j + 1].p, B[j].p, A[j].n, A[j + 1].n, B[j + 1].n, B[j].n,
        cf(j / nu, i / (rows.length - 1)), cf((j + 1) / nu, i / (rows.length - 1)), cf((j + 1) / nu, (i + 1) / (rows.length - 1)), cf(j / nu, (i + 1) / (rows.length - 1)));
    }
  }
  if (mirror) T.mirrorX();
  return T.geo();
}
// Ribbon decal of width w along a polyline in a view.
function ribbon(S, view, pts, w, { off = 0.004, col = 1, closed = false, mirror = false, step = 0.04 } = {}) {
  const q = [];
  const segs = closed ? pts.length : pts.length - 1;
  for (let i = 0; i < segs; i++) {
    const a = pts[i], b = pts[(i + 1) % pts.length];
    const n = Math.max(1, Math.ceil(Math.hypot(b[0] - a[0], b[1] - a[1]) / step));
    for (let k = 0; k < n; k++) q.push([a[0] + ((b[0] - a[0]) * k) / n, a[1] + ((b[1] - a[1]) * k) / n]);
  }
  if (!closed) q.push(pts[pts.length - 1]);
  const L = [], R = [];
  for (let i = 0; i < q.length; i++) {
    const p = q[i];
    const pa = q[i > 0 ? i - 1 : closed ? q.length - 1 : 0], pb = q[i < q.length - 1 ? i + 1 : closed ? 0 : q.length - 1];
    let tu = pb[0] - pa[0], tv = pb[1] - pa[1];
    const l = Math.hypot(tu, tv) || 1;
    tu /= l; tv /= l;
    const hw = (typeof w === 'function' ? w(i / Math.max(1, q.length - 1)) : w) / 2;
    L.push(S.at(view, p[0] - tv * hw, p[1] + tu * hw, off));
    R.push(S.at(view, p[0] + tv * hw, p[1] - tu * hw, off));
  }
  const T = new Tris();
  for (let i = 0; i < q.length - (closed ? 0 : 1); i++) {
    const j = (i + 1) % q.length;
    if (!L[i] || !R[i] || !L[j] || !R[j]) continue;
    T.quad(L[i].p, R[i].p, R[j].p, L[j].p, L[i].n, R[i].n, R[j].n, L[j].n, col);
  }
  if (mirror) T.mirrorX();
  return T.geo();
}
// Offset a polygon outward by d (miter, clamped at sharp corners).
function expandPoly(poly, d) {
  const n = poly.length;
  let area = 0;
  for (let i = 0; i < n; i++) { const a = poly[i], b = poly[(i + 1) % n]; area += a[0] * b[1] - b[0] * a[1]; }
  const s = area > 0 ? 1 : -1;
  return poly.map((p, i) => {
    const a = poly[(i + n - 1) % n], b = poly[(i + 1) % n];
    let e1 = [p[0] - a[0], p[1] - a[1]], e2 = [b[0] - p[0], b[1] - p[1]];
    const l1 = Math.hypot(...e1) || 1, l2 = Math.hypot(...e2) || 1;
    e1 = [e1[0] / l1, e1[1] / l1]; e2 = [e2[0] / l2, e2[1] / l2];
    const n1 = [e1[1], -e1[0]], n2 = [e2[1], -e2[0]];
    let nx = n1[0] + n2[0], ny = n1[1] + n2[1];
    const l = Math.hypot(nx, ny) || 1;
    nx /= l; ny /= l;
    const k = d / Math.max(0.4, nx * n1[0] + ny * n1[1]);
    return [p[0] + s * nx * k, p[1] + s * ny * k];
  });
}
const ellipse = (cu, cv, ru, rv, n = 12, rot = 0) => {
  const out = [];
  for (let i = 0; i < n; i++) {
    const a = (i / n) * 2 * PI, x = Math.cos(a) * ru, y = Math.sin(a) * rv;
    out.push([cu + x * Math.cos(rot) - y * Math.sin(rot), cv + x * Math.sin(rot) + y * Math.cos(rot)]);
  }
  return out;
};

// ── sweeps and lathes ─────────────────────────────────────────────
// Sweep a closed 2D section along an axis (smooth around, flat end caps).
//   axis 'x': section [z, y];  axis 'z': section [x, y]
// fn(t) -> { s: scale, du, dv } optionally tapers/offsets the section.
function sweep(sec, axis, a0, a1, { n = 1, caps = true, fn, col = 1 } = {}) {
  const T = new Tris();
  let cu = 0, cv = 0;
  for (const [u, v] of sec) { cu += u; cv += v; }
  cu /= sec.length; cv /= sec.length;
  const P3 = (a, u, v) => (axis === 'x' ? [a, v, u] : [u, v, a]);
  const N3 = (u, v) => (axis === 'x' ? [0, v, u] : [u, v, 0]);
  const m = sec.length;
  const ring = (t) => {
    const f = fn ? fn(t) : {};
    const s = f.s ?? 1, du = f.du ?? 0, dv = f.dv ?? 0;
    return sec.map(([u, v]) => [cu + (u - cu) * s + du, cv + (v - cv) * s + dv]);
  };
  const rings = [];
  for (let i = 0; i <= n; i++) rings.push({ a: a0 + ((a1 - a0) * i) / n, r: ring(i / n) });
  const norms = rings.map(({ r }) => r.map((p, k) => {
    const pa = r[(k + m - 1) % m], pb = r[(k + 1) % m];
    let nu = pb[1] - pa[1], nv = -(pb[0] - pa[0]);
    if (nu * (p[0] - cu) + nv * (p[1] - cv) < 0) { nu = -nu; nv = -nv; }
    const l = Math.hypot(nu, nv) || 1;
    return N3(nu / l, nv / l);
  }));
  for (let i = 0; i < n; i++) {
    const A = rings[i], B = rings[i + 1];
    for (let k = 0; k < m; k++) {
      const k2 = (k + 1) % m;
      T.quad(P3(A.a, ...A.r[k]), P3(A.a, ...A.r[k2]), P3(B.a, ...B.r[k2]), P3(B.a, ...B.r[k]),
        norms[i][k], norms[i][k2], norms[i + 1][k2], norms[i + 1][k], col);
    }
  }
  if (caps) {
    for (const [R, dir] of [[rings[0], -1], [rings[n], 1]]) {
      const contour = R.r.map(([u, v]) => new THREE.Vector2(u, v));
      const faces = THREE.ShapeUtils.triangulateShape(contour, []);
      const nn = axis === 'x' ? [dir, 0, 0] : [0, 0, dir];
      for (const [a, b, c] of faces) T.tri(P3(R.a, ...R.r[a]), P3(R.a, ...R.r[b]), P3(R.a, ...R.r[c]), nn, nn, nn, col);
    }
  }
  return T.geo();
}
// Lathe about the X axis: prof = [[radius, x], ...]; cols is a grey level,
// or an array of them per profile point.
function latheX(prof, seg, cols = 1, phi0 = 0, phiLen = 2 * PI) {
  const g = new THREE.LatheGeometry(prof.map(([r, x]) => new THREE.Vector2(r, x)), seg, phi0, phiLen);
  g.rotateZ(-PI / 2);
  const n = g.attributes.position.count, np = prof.length;
  const c = new Float32Array(n * 3);
  for (let i = 0; i < n; i++) {
    const v = Array.isArray(cols) ? cols[i % np] : cols;
    c[i * 3] = c[i * 3 + 1] = c[i * 3 + 2] = v;
  }
  g.setAttribute('color', new THREE.BufferAttribute(c, 3));
  return g;
}
// Airfoil section [z, y] (leading edge forward at +chord/2), cambered for
// downforce.
function airfoil(chord, thick, n = 7, camber = 0.05) {
  const up = [], lo = [];
  for (let i = 0; i <= n; i++) {
    const x = (1 - Math.cos((i / n) * PI)) / 2;
    const yt = 5 * thick * (0.2969 * Math.sqrt(x) - 0.126 * x - 0.3516 * x * x + 0.2843 * x ** 3 - 0.1036 * x ** 4) * chord;
    const yc = -camber * chord * 4 * x * (1 - x);
    up.push([(0.5 - x) * chord, yc + yt]);
    lo.push([(0.5 - x) * chord, yc - yt]);
  }
  const out = up.slice(0, n);
  for (let i = n; i >= 1; i--) out.push(lo[i]);
  return out;
}

// ── parts ─────────────────────────────────────────────────────────
// Extrude a (z,y) outline across `width` centred on X (flat-shaded).
function shapeFrom(pts) {
  const s = new THREE.Shape();
  s.moveTo(pts[0][0], pts[0][1]);
  for (let i = 1; i < pts.length; i++) s.lineTo(pts[i][0], pts[i][1]);
  s.closePath();
  return s;
}
function extrude(pts, width, bevel = 0.01) {
  const depth = Math.max(0.005, width - 2 * bevel);
  const g = new THREE.ExtrudeGeometry(shapeFrom(pts), {
    depth, steps: 1, curveSegments: 1,
    bevelEnabled: bevel > 0, bevelThickness: bevel, bevelSize: bevel, bevelOffset: -bevel, bevelSegments: 1,
  });
  g.translate(0, 0, -depth / 2);
  g.rotateY(-PI / 2);
  g.computeVertexNormals();
  return g;
}
function prep(geom, pos = [0, 0, 0], rot = [0, 0, 0], col = 1) {
  let g = geom.index ? geom.toNonIndexed() : geom;
  if (g !== geom) geom.dispose();
  for (const k of Object.keys(g.attributes)) if (k !== 'position' && k !== 'normal' && k !== 'color') g.deleteAttribute(k);
  if (!g.attributes.normal) g.computeVertexNormals();
  if (!g.attributes.color || col !== 1) {
    const n = g.attributes.position.count, c = new Float32Array(n * 3), cc = colArr(col);
    const old = g.attributes.color?.array;
    for (let i = 0; i < n; i++) for (let e = 0; e < 3; e++) c[i * 3 + e] = (old ? old[i * 3 + e] : 1) * cc[e];
    g.setAttribute('color', new THREE.BufferAttribute(c, 3));
  }
  if (rot[0] || rot[1] || rot[2]) g.applyMatrix4(new THREE.Matrix4().makeRotationFromEuler(new THREE.Euler(rot[0], rot[1], rot[2], 'YXZ')));
  g.translate(pos[0], pos[1], pos[2]);
  g.groups = [];
  return g;
}
// Box-projected UVs (5 cm tiles) for textured buckets such as carbon.
function boxUV(g, tile = 0.05) {
  const P = g.attributes.position.array, N = g.attributes.normal.array, n = P.length / 3;
  const uv = new Float32Array(n * 2);
  for (let i = 0; i < n; i++) {
    const ax = Math.abs(N[i * 3]), ay = Math.abs(N[i * 3 + 1]), az = Math.abs(N[i * 3 + 2]);
    const [u, v] = ax >= ay && ax >= az ? [P[i * 3 + 2], P[i * 3 + 1]] : ay >= az ? [P[i * 3], P[i * 3 + 2]] : [P[i * 3], P[i * 3 + 1]];
    uv[i * 2] = u / tile; uv[i * 2 + 1] = v / tile;
  }
  g.setAttribute('uv', new THREE.BufferAttribute(uv, 2));
}

class Parts {
  constructor(lod) { this.b = {}; this.hi = lod === 'high'; }
  add(bucket, geom, pos, rot, col) {
    if (!geom || !geom.attributes.position.count) return this;
    (this.b[bucket] ||= []).push(prep(geom, pos, rot, col));
    return this;
  }
  addAll(map) { for (const [k, g] of Object.entries(map.geo || map)) this.add(k, g); return this; }
  box(bucket, w, h, d, pos, rot, col) { return this.add(bucket, new THREE.BoxGeometry(w, h, d), pos, rot, col); }
  // Mirrored pair across X.
  pair(bucket, make, x, y, z, rot = [0, 0, 0], col) {
    this.add(bucket, make(), [x, y, z], rot, col);
    this.add(bucket, make(), [-x, y, z], [rot[0], -rot[1], -rot[2]], col);
    return this;
  }
  cyl(bucket, rt, rb, h, seg, pos, rot, col) { return this.add(bucket, new THREE.CylinderGeometry(rt, rb, h, seg), pos, rot, col); }
  build() {
    const out = {};
    for (const [k, list] of Object.entries(this.b)) {
      out[k] = mergeGeometries(list);
      fixNormals(out[k]);
      if (k === 'carbon' && this.hi) boxUV(out[k]);
      if (!this.hi) out[k].deleteAttribute('color');
      out[k].computeBoundingSphere();
      for (const g of list) g.dispose();
    }
    return out;
  }
}
const Box = (w, h, d) => () => new THREE.BoxGeometry(w, h, d);
const Disc = (r, t, seg) => () => new THREE.CylinderGeometry(r, r, t, seg);

// ── detail kit ────────────────────────────────────────────────────
// Light unit: a black bezel, a lens (dim emission) and optional lit
// elements drawn on top (full emission via vertex colour).
function lamp(P, S, view, poly, { bucket = 'head', bezel = 'trim', bezelW = 0.012, lens = 0.1, off = 0.004, mirror = true, nu = 6, nv = 3 } = {}) {
  // Enough columns that the lens follows a rounded corner instead of
  // cutting a chord through it (and vanishing into the body).
  nu = Math.max(nu, 4);
  if (bezel) P.add(bezel, decal(S, view, expandPoly(poly, bezelW), { off: off * 0.55, nu, nv, mirror }));
  P.add(bucket, decal(S, view, poly, { off, nu, nv, mirror, col: lens }));
}
function glow(P, S, view, pts, w, bucket, { col = 1, off = 0.0065, mirror = true, closed = false, step } = {}) {
  P.add(bucket, ribbon(S, view, pts, w, { off, col, mirror, closed, step }));
}
// Panel shut line: a thin dark ribbon just proud of the paint.
function gap(P, S, view, pts, { w = 0.0065, mirror = true, closed = false, step = 0.05, bucket = 'trim', col = 1 } = {}) {
  P.add(bucket, ribbon(S, view, pts, w, { off: 0.0016, mirror, closed, step, col }));
}
function dot(P, S, view, u, v, r, bucket, { col = 1, off = 0.007, mirror = true, n = 10, rv } = {}) {
  P.add(bucket, decal(S, view, ellipse(u, v, r, rv ?? r, n), { off, nu: 3, nv: 3, mirror, col }));
}
// Mirror a triangle-soup geometry across X (keeping it front-facing).
function mirrorGeo(g) {
  const P = g.attributes.position.array, N = g.attributes.normal.array, C = g.attributes.color?.array;
  for (let i = 0; i < P.length; i += 3) { P[i] = -P[i]; N[i] = -N[i]; }
  for (let t = 0; t < P.length; t += 9) {
    for (const A of [P, N, C]) {
      if (!A) continue;
      for (let e = 0; e < 3; e++) { const x = A[t + 3 + e]; A[t + 3 + e] = A[t + 6 + e]; A[t + 6 + e] = x; }
    }
  }
  return g;
}
// Race number on both flanks from seven-segment strokes. On the side view
// u runs forward, so on the left flank text reads towards -u; the right
// flank is laid out the other way round and then mirrored across.
const SEG = { 0: 'abcdef', 1: 'bc', 2: 'abged', 3: 'abgcd', 4: 'fgbc', 5: 'afgcd', 6: 'afgedc', 7: 'abc', 8: 'abcdefg', 9: 'abcdfg', P: 'abefg', O: 'abcdef', L: 'def', I: 'i', C: 'adef', E: 'adefg' };
function number(P, S, view, str, cu, cv, h, bucket = 'trim') {
  const w = h * 0.5, gapW = h * 0.22, t = h * 0.14;
  const total = str.length * w + (str.length - 1) * gapW;
  for (const dir of [-1, 1]) {
    [...str].forEach((ch, i) => {
      const left = cu - dir * (total / 2 - i * (w + gapW)), right = left + dir * w;
      const top = cv + h / 2, mid = cv, bot = cv - h / 2;
      const seg = { i: [[(left + right) / 2, top], [(left + right) / 2, bot]], a: [[left, top], [right, top]], b: [[right, top], [right, mid]], c: [[right, mid], [right, bot]], d: [[left, bot], [right, bot]], e: [[left, mid], [left, bot]], f: [[left, top], [left, mid]], g: [[left, mid], [right, mid]] };
      for (const k of SEG[ch] || '') {
        const g = ribbon(S, view, seg[k], t, { off: 0.0045, step: 1 });
        P.add(bucket, dir > 0 ? mirrorGeo(g) : g);
      }
    });
  }
}
// Door mirror: stalk plus a rounded pod with the glass facing back.
function mirrors(P, hi, { x, y, z, w = 0.19, h = 0.1, d = 0.14, shell = 'paint', base = 0.78 }) {
  const seg = hi ? [12, 8] : [6, 4];
  for (const sx of [1, -1]) {
    const pod = new THREE.SphereGeometry(1, seg[0], seg[1]);
    pod.scale(w / 2, h / 2, d / 2);
    P.add(shell, pod, [sx * x, y, z]);
    P.add('trim', new THREE.BoxGeometry(Math.max(0.02, x - base), 0.03, 0.07), [sx * (x + base) / 2, y - h * 0.3, z + 0.02]);
    if (hi) {
      const gl = new THREE.CircleGeometry(1, 12);
      gl.scale(w * 0.42, h * 0.36, 1);
      P.add('chrome', gl, [sx * x, y, z - d * 0.46], [0, PI, 0], 0.55);
    }
  }
}
// Exhaust tip: polished tube, rolled lip and a sooty inside, pointing back.
function tip(P, hi, x, y, z, r, len = 0.14, { mirror = true, oval = 1 } = {}) {
  const seg = hi ? 14 : 8;
  const parts = [
    latheX([[r, 0], [r, len]], seg, 1),
    latheX([[r, len], [r * 0.8, len + 0.004]], seg, 1.3),
    latheX([[r * 0.8, len + 0.004], [r * 0.8, 0.02]], seg, 0.08),
  ];
  for (const sx of mirror ? [1, -1] : [1]) {
    for (const g0 of parts) {
      const g = g0.clone();
      g.rotateY(PI / 2); // X axis → -Z (pointing back)
      g.scale(1, oval, 1);
      P.add('chrome', g, [sx * x, y, z]);
    }
  }
}
// Rear wing: cambered carbon/paint airfoil with end plates and uprights.
function wing(P, hi, { span, chord, thick = 0.12, y, z, pitch = -0.1, bucket = 'carbon', plates = true, plateH = 0.16, uprights = [], upBucket = 'carbon', baseY }) {
  const sec = airfoil(chord, thick, hi ? 8 : 4);
  P.add(bucket, sweep(sec, 'x', -span / 2, span / 2), [0, y, z], [pitch, 0, 0]);
  if (plates) {
    const pl = [[chord * 0.55, plateH * 0.25], [-chord * 0.6, plateH * 0.45], [-chord * 0.65, -plateH * 0.55], [chord * 0.3, -plateH * 0.5]];
    P.pair(bucket, () => extrude(pl, 0.014, 0.003), span / 2 + 0.007, y, z);
  }
  for (const ux of uprights) {
    const top = y - thick * chord * 0.3, bot = baseY;
    const sh = [[chord * 0.15, top], [-chord * 0.2, top], [-chord * 0.12, bot], [chord * 0.28, bot]];
    P.pair(upBucket, () => extrude(sh.map(([a, b]) => [a + z, b]), 0.022, 0.004), ux, 0, 0);
  }
}
// Interior glimpsed through the glass: seat backs and headrests (whose top
// sits at `top`, a hand under the roof), dash and steering wheel. Only the
// parts above the belt line matter; the rest hides inside the body.
function interior(P, { z, top, x = 0.36, dashZ, dashY, seat = 2.2, cage = false, lean = 0.28, wheelR = 0.17 }) {
  const col = seat;
  const hz = z - 0.2 * Math.sin(lean);
  for (const sx of [1, -1]) {
    P.add('trim', new THREE.BoxGeometry(0.24, 0.16, 0.1), [sx * x, top - 0.08, hz - 0.06], [-lean, 0, 0], col);
    P.add('trim', new THREE.BoxGeometry(0.1, 0.05, 0.05), [sx * x, top - 0.18, hz - 0.04], [-lean, 0, 0], 0.5);
    P.add('trim', new THREE.BoxGeometry(0.44, 0.55, 0.12), [sx * x, top - 0.47, z], [-lean, 0, 0], col);
    P.pair('trim', () => new THREE.BoxGeometry(0.07, 0.4, 0.16), sx * x + 0.2, top - 0.52, z + 0.04, [-lean, 0, 0], col);
  }
  P.add('trim', new THREE.BoxGeometry(1.36, 0.12, 0.4), [0, dashY, dashZ], [0.12, 0, 0], 1.3);
  P.add('trim', new THREE.TorusGeometry(wheelR, 0.022, 5, 16), [x, dashY + 0.02, dashZ - 0.3], [-0.45, 0, 0], 0.7);
  P.add('trim', new THREE.BoxGeometry(0.03, 0.05, wheelR * 2), [x, dashY + 0.02, dashZ - 0.3], [-0.45 - PI / 2, 0, 0], 0.7);
  if (cage) {
    P.add('chrome', new THREE.TorusGeometry(0.64, 0.022, 5, 14, PI), [0, top - 0.64, z - 0.28], [0, 0, 0], 0.4);
    P.pair('chrome', () => new THREE.CylinderGeometry(0.02, 0.02, 0.9, 5), 0.6, top - 0.3, z - 0.7, [0.9, 0, 0], 0.4);
  }
}
// Black wheel-arch liners so the arches read as holes, not see-through.
function wheelWells(P, zs, yc, R, W, hi) {
  for (const z of zs) {
    const g = new THREE.CylinderGeometry(R - 0.012, R - 0.012, W - 0.16, hi ? 14 : 8, 1, true, PI / 2, PI);
    P.add('trim', g, [0, yc, z], [0, 0, PI / 2], 0.5);
  }
}

// ── police kit ────────────────────────────────────────────────────
// Rounded rectangle [u, v] (counter-clockwise), for swept sections.
function roundRect(cu, cv, hu, hv, r, n = 3) {
  const out = [];
  for (const [sx, sv, a0] of [[1, 1, 0], [-1, 1, PI / 2], [-1, -1, PI], [1, -1, 1.5 * PI]]) {
    for (let k = 0; k <= n; k++) {
      const a = a0 + (PI / 2) * (k / n);
      out.push([cu + sx * (hu - r) + r * Math.cos(a), cv + sv * (hv - r) + r * Math.sin(a)]);
    }
  }
  return out;
}
// Roof lightbar sitting on y at z: a black base on feet, 2×4 lens segments
// (red on the driver's side, +X; blue on the right) in the lightRed and
// lightBlue buckets, chrome end caps. Returns the siren layout: glow spots
// over each half and the anchor for the shared flash light.
function lightbar(P, hi, { y, z, w = 1.24, d = 0.3, h = 0.1 }) {
  const yb = y + 0.03, gapC = 0.07;
  P.box('trim', w, 0.03, d, [0, y + 0.015, z]);
  const seg = (w / 2 - gapC / 2 - 0.03) / 4;
  if (hi) {
    P.pair('trim', Box(0.07, 0.06, d * 0.7), w * 0.36, y - 0.02, z);
    const sec = roundRect(z, yb + h / 2, d / 2 - 0.005, h / 2, 0.03);
    for (let i = 0; i < 4; i++) {
      const x0 = gapC / 2 + i * seg + 0.004, x1 = x0 + seg - 0.008;
      P.add('lightRed', sweep(sec, 'x', x0, x1));
      P.add('lightBlue', sweep(sec, 'x', -x1, -x0));
    }
    // Dividers, centre block and end caps.
    P.add('trim', sweep(roundRect(z, yb + h / 2 - 0.004, d / 2 - 0.012, h / 2 - 0.006, 0.026), 'x', -w / 2 + 0.03, w / 2 - 0.03), undefined, undefined, 0.6);
    P.add('trim', sweep(roundRect(z, yb + h / 2 + 0.003, d / 2 - 0.002, h / 2 + 0.003, 0.03), 'x', -gapC / 2, gapC / 2), undefined, undefined, 0.5);
    for (const sx of [1, -1]) P.add('chrome', sweep(roundRect(z, yb + h / 2, d / 2, h / 2 + 0.004, 0.035), 'x', sx > 0 ? w / 2 - 0.03 : -w / 2, sx > 0 ? w / 2 : -w / 2 + 0.03), undefined, undefined, 0.8);
  } else {
    const hw = seg * 2;
    P.box('lightRed', hw * 2 - 0.01, h, d - 0.01, [gapC / 2 + hw, yb + h / 2, z]);
    P.box('lightBlue', hw * 2 - 0.01, h, d - 0.01, [-gapC / 2 - hw, yb + h / 2, z]);
    P.box('trim', gapC + 0.01, h + 0.006, d, [0, yb + h / 2, z]);
  }
  const gy = yb + h * 0.6, gx = gapC / 2 + seg * 2;
  return {
    anchor: [0, yb + h + 0.25, z],
    glows: [{ p: [gx, gy, z], blue: 0, size: 2 }, { p: [-gx, gy, z], blue: 1, size: 2 }],
  };
}
// Push bar in front of the nose: two padded uprights and two cross bars on
// arms back to the bumper, with a pair of small strobes on the top bar.
function pushBar(P, hi, { z, y0, y1, x = 0.36, back = 0.14 }) {
  const my = (y0 + y1) / 2, hgt = y1 - y0;
  P.pair('trim', Box(0.07, hgt, 0.06), x, my, z, undefined, 1.4);
  P.box('trim', 2 * x + 0.12, 0.07, 0.05, [0, y0 + hgt * 0.3, z - 0.005], undefined, 1.4);
  P.box('trim', 2 * x + 0.02, 0.05, 0.05, [0, y1 - 0.05, z], undefined, 1.4);
  P.pair('trim', Box(0.05, 0.05, back), x, y0 + hgt * 0.3, z - back / 2);
  P.box('lightRed', 0.16, 0.035, 0.03, [x * 0.45, y1 - 0.05, z + 0.03]);
  P.box('lightBlue', 0.16, 0.035, 0.03, [-x * 0.45, y1 - 0.05, z + 0.03]);
  if (hi) {
    P.pair('trim', Box(0.085, hgt * 0.82, 0.03), x, my, z + 0.04, undefined, 0.45);  // rubber pads
    P.pair('trim', Box(0.05, 0.05, back), x, y1 - 0.05, z - back / 2);
  }
}
// Slicktop strobes for the interceptor livery: a red/blue pair in the grille
// and a pair of bars on the rear deck.
function strobes(P, S, hi, { grille: [gx0, gx1, gy0, gy1], deck: [dx, dy, dz] }) {
  const rect = (u0, u1) => [[u0, gy0], [u1, gy0], [u1, gy1], [u0, gy1]];
  const o = { off: 0.012, nu: hi ? 3 : 1, nv: 1 };
  P.add('lightRed', decal(S, 'front', rect(gx0, gx1), o));
  P.add('lightBlue', decal(S, 'front', rect(-gx1, -gx0), o));
  const seg = hi ? 3 : 1, sec = roundRect(dz, dy + 0.025, 0.035, 0.025, 0.012, seg);
  P.add('trim', sweep(roundRect(dz, dy + 0.012, 0.045, 0.014, 0.01, seg), 'x', -dx - 0.2, dx + 0.2));
  P.add('lightRed', sweep(sec, 'x', dx - 0.18, dx + 0.18));
  P.add('lightBlue', sweep(sec, 'x', -dx - 0.18, -dx + 0.18));
  const gy = (gy0 + gy1) / 2, gz = S.z1 + 0.02, gxm = (gx0 + gx1) / 2;
  return {
    anchor: [0, gy, S.z1 + 0.3],
    glows: [
      { p: [gxm, gy, gz], blue: 0, size: 0.9 }, { p: [-gxm, gy, gz], blue: 1, size: 0.9 },
      { p: [dx, dy + 0.03, dz], blue: 0, size: 1.3 }, { p: [-dx, dy + 0.03, dz], blue: 1, size: 1.3 },
    ],
  };
}

// ── wheels ────────────────────────────────────────────────────────
// Groups: 0 tyre, 1 rim, 2 brake/inner metal. The wheel's outboard face is +X.
const wheelCache = new Map();
function wheelGeometry(r, w, lod, style) {
  const hi = lod === 'high';
  const key = [r, w, lod, style.spokes, style.rimFrac, style.type].join('|');
  if (wheelCache.has(key)) return wheelCache.get(key);
  const rimR = r * style.rimFrac;
  const hw = w / 2;
  const tyre = [], rim = [], inner = [];
  if (hi) {
    const sw = r - rimR;
    // Sidewall bulges past the rim, a rounded shoulder, then the tread.
    tyre.push(latheX([
      [rimR + 0.012, -hw + 0.02], [rimR + sw * 0.45, -hw - 0.006], [r - 0.03, -hw + 0.004], [r - 0.008, -hw + 0.022], [r, -hw + 0.045],
      [r, hw - 0.045], [r - 0.008, hw - 0.022], [r - 0.03, hw - 0.004], [rimR + sw * 0.45, hw + 0.006], [rimR + 0.012, hw - 0.02],
    ], 30, [1.2, 1.25, 1.1, 0.95, 0.85, 0.85, 0.95, 1.1, 1.25, 1.2]));
    // Rim: outer lip, inner barrel (faces inward), backing plate.
    const lipX = hw * 0.8;
    rim.push(latheX([[rimR + 0.016, lipX - 0.012], [rimR + 0.012, lipX + 0.006], [rimR - 0.012, lipX + 0.008], [rimR - 0.022, lipX - 0.01]], 30, [0.9, 1.15, 1.1, 0.8]));
    inner.push(latheX([[rimR - 0.004, lipX - 0.01], [rimR - 0.004, -hw * 0.75]], 24, 0.45));
    inner.push(latheX([[rimR, -hw * 0.72], [0, -hw * 0.72]], 16, 0.06));
    // Brake disc (with a darker hat) sitting inboard of the spokes.
    const dx = -hw * 0.18, dt = 0.026;
    inner.push(latheX([[rimR * 0.86, dx + dt / 2], [rimR * 0.4, dx + dt / 2]], 24, [0.95, 0.75]));
    inner.push(latheX([[rimR * 0.86, dx - dt / 2], [rimR * 0.86, dx + dt / 2]], 24, 0.6));
    inner.push(latheX([[rimR * 0.4, dx + dt / 2], [rimR * 0.4, dx + 0.05], [rimR * 0.2, dx + 0.05]], 16, 0.35));
    const faceX = lipX - 0.006;
    const hubR = rimR * 0.3, dish = style.dish ?? 0.05;
    const T = new Tris();
    const spoke = (th, w0, w1, depth, col = 1) => {
      // Radial stations from hub to lip; the face curves back (concave) toward the hub.
      const st = [0, 0.35, 0.7, 1];
      const ring = st.map((s) => {
        const rr = hubR * 0.9 + (rimR * 0.97 - hubR * 0.9) * s;
        const x = faceX - dish * (1 - s) ** 1.6;
        const ww = (w0 + (w1 - w0) * s) / 2;
        const c = Math.cos(th), sn = Math.sin(th);
        const P2 = (t, xx) => [xx, rr * c - t * sn, rr * sn + t * c];
        return { fl: P2(ww, x), fr: P2(-ww, x), bl: P2(ww * 0.8, x - depth), br: P2(-ww * 0.8, x - depth), rad: [0, c, sn], tan: [0, -sn, c] };
      });
      for (let i = 0; i < ring.length - 1; i++) {
        const a = ring[i], b = ring[i + 1];
        T.flatQuad(a.fl, a.fr, b.fr, b.fl, [1, 0, 0], col);
        T.flatQuad(a.fl, a.bl, b.bl, b.fl, a.tan, col * 0.85);
        T.flatQuad(a.fr, a.br, b.br, b.fr, [-a.tan[0], -a.tan[1], -a.tan[2]], col * 0.85);
      }
    };
    const n = style.spokes;
    if (style.type === 'aero') {
      rim.push(latheX([[rimR - 0.02, faceX], [rimR * 0.72, faceX - 0.018], [rimR * 0.35, faceX - 0.02], [hubR * 0.9, faceX - 0.012]], 30, [1, 0.95, 0.9, 0.9]));
      for (let k = 0; k < 5; k++) spoke((k / 5) * 2 * PI + 0.3, 0.02, 0.07, 0.01, 0.12);
    } else if (style.type === 'steel' || style.type === 'hub') {
      rim.push(latheX([[rimR - 0.02, faceX], [rimR * 0.62, faceX - 0.03], [hubR, faceX - 0.035]], 24, [1, 0.9, 0.9]));
    } else {
      const split = style.type === 'split';
      for (let k = 0; k < n; k++) {
        const th = (k / n) * 2 * PI;
        if (split) { spoke(th - 0.075, 0.03, 0.032, 0.03); spoke(th + 0.075, 0.03, 0.032, 0.03); }
        else spoke(th, style.w0 ?? 0.05, style.w1 ?? (n > 6 ? 0.035 : 0.06), 0.035);
      }
    }
    rim.push(T.geo());
    // Hub face, centre cap and lug nuts.
    const hx = faceX - dish;
    rim.push(latheX([[hubR * 1.05, hx - 0.004], [hubR * 0.75, hx + 0.004], [hubR * 0.45, hx + 0.006]], 18, 0.9));
    rim.push(latheX([[hubR * 0.45, hx + 0.006], [hubR * 0.3, hx + 0.018], [0, hx + 0.022]], 14, [0.35, 0.3, 0.3]));
    for (let k = 0; k < 5; k++) {
      const a = (k / 5) * 2 * PI;
      const nut = new THREE.CylinderGeometry(0.011, 0.012, 0.022, 6);
      nut.rotateZ(PI / 2);
      nut.translate(hx + 0.01, Math.cos(a) * hubR * 0.68, Math.sin(a) * hubR * 0.68);
      rim.push(prep(nut, undefined, undefined, 1.3));
    }
  } else {
    const e = Math.min(0.04, w * 0.18);
    // Outer sidewall + tread only: the inner face is never seen.
    tyre.push(latheX([[r, -hw], [r, hw - e], [r - e, hw], [rimR, hw]], 12, [0.9, 0.9, 1.1, 1.2]));
    if (style.type === 'steel' || style.type === 'hub') {
      rim.push(latheX([[rimR * 1.02, hw * 0.8], [rimR * 0.82, hw * 0.9], [rimR * 0.35, hw * 0.94], [0, hw * 0.95]], 12, 1));
    } else {
      // Racer at low LOD: flat face with a dark recess ring standing in for spokes.
      rim.push(latheX([[rimR * 1.02, hw * 0.82], [rimR * 0.9, hw * 0.84]], 12, 1));
      inner.push(latheX([[rimR * 0.9, hw * 0.7], [rimR * 0.3, hw * 0.72]], 12, 0.1));
      rim.push(latheX([[rimR * 0.3, hw * 0.84], [0, hw * 0.86]], 12, 1));
    }
  }
  const merge = (list) => (list.length ? mergeGeometries(list.map((g) => prep(g))) : null);
  const groups = [merge(tyre), merge(rim), merge(inner)];
  // Keep three groups (tyre, rim, brake) even when the brake group is empty.
  // (A trailing empty brake group is dropped: traffic wheels draw twice, not three times.)
  if (!groups[2]) groups.pop();
  const parts = groups.map((g) => g || prep(new THREE.BufferGeometry().setAttribute('position', new THREE.Float32BufferAttribute([0, 0, 0, 0, 0, 0, 0, 0, 0], 3)).setAttribute('normal', new THREE.Float32BufferAttribute([1, 0, 0, 1, 0, 0, 1, 0, 0], 3))));
  if (!hi) for (const g of parts) g.deleteAttribute('color');
  const geom = mergeGeometries(parts, true);
  fixNormals(geom, [1, 0, 0]);
  geom.computeBoundingSphere();
  wheelCache.set(key, geom);
  return geom;
}
const caliperCache = new Map();
function caliperGeom(rimR) {
  const k = rimR.toFixed(3);
  if (caliperCache.has(k)) return caliperCache.get(k);
  // Annular sector (in the wheel's Z/Y plane) extruded across X.
  const ro = rimR * 0.9, ri = rimR * 0.6, a0 = PI * 0.04, a1 = PI * 0.4;
  const s = new THREE.Shape();
  s.absarc(0, 0, ro, a0, a1, false);
  s.absarc(0, 0, ri, a1, a0, true);
  const g = new THREE.ExtrudeGeometry(s, { depth: 0.07, bevelEnabled: false, curveSegments: 4 });
  g.translate(0, 0, -0.035);
  g.rotateY(PI / 2); // shape x → -Z (behind the axle), extrusion → X
  g.clearGroups(); // one draw call, not caps + sides
  g.computeVertexNormals();
  caliperCache.set(k, g);
  return g;
}

// ── loft presets ─────────────────────────────────────────────────
function detail(lod) {
  return lod === 'high'
    ? { nb: 2, nt: 5, sideN: 4, nCrown: 4, uni: 32, archN: 8, cabUni: 20, cabSide: 2, endN: 5 }
    : { nb: 1, nt: 2, sideN: 1, nCrown: 1, uni: 8, archN: 4, cabUni: 6, cabSide: 0, endN: 2 };
}
// Standard car body: paint everywhere, black underside/arch liners. The
// roof/bonnet line is splined through the profile points.
function bodyLoft(lod, { z0, z1, top, bot, arches, halfW, rTop = 0.1, rBot = 0.05, crown = 0.025, crownX, extraZ = [], mat, side = [], endR = 0.2, col, linear = false }) {
  const D = detail(lod);
  const botF = sill(bot, arches);
  const topF = linear ? (z) => interp(top, z) : spline(top);
  const archY = arches.flatMap(([, yc, R]) => [yc + R + 0.02, yc + R + 0.07]);
  const zs = stations(z0, z1, D.uni, [...top.map((p) => p[0]), ...bot.map((p) => p[0]), ...archZs(arches, D.archN), ...endZs(z0, z1, endR, D.endN), ...extraZ]);
  return loft({
    zs, top: topF, bot: botF, halfW, rTop, rBot, crown, crownX, side: lod === 'high' ? [...side, ...archY] : side,
    nb: D.nb, nt: D.nt, sideN: D.sideN, nCrown: D.nCrown,
    mat: mat || ((tag) => (tag === 0 ? 'trim' : 'paint')),
    capMat: () => 'paint',
    // A touch of ambient occlusion low on the body and under the sills.
    col: col || ((x, y) => 0.62 + 0.38 * smooth(0.14, 0.5, y)),
  });
}
// Greenhouse sitting on the body. Default: glass sides and screens, painted
// roof and pillars; `cPillar` paints the rear quarter; `pillars` colours the
// A-pillars/roof rails ('paint' or 'trim').
function cabinLoft(lod, { z0, z1, top, bodyTop, halfW, roof, bPillar, cPillar, pillars = 'paint', rTop = 0.08, crown = 0.02, crownX, extraZ = [], stripe, mat, linear = false }) {
  const D = detail(lod);
  const topF = linear ? (z) => interp(top, z) : spline(top);
  const bodyF = typeof bodyTop === 'function' ? bodyTop : spline(bodyTop);
  const botF = (z) => bodyF(z) - 0.035;
  const zs = stations(z0, z1, D.cabUni, [...top.map((p) => p[0]), roof[0], roof[1], ...(bPillar || []), ...(cPillar != null ? [cPillar] : []), ...extraZ]);
  return loft({
    zs, top: topF, bot: botF, halfW, rTop, rBot: 0.01, crown, crownX,
    nb: 0, nt: D.nt, sideN: D.cabSide, nCrown: D.nCrown,
    mat: mat || ((tag, y, z, ax) => {
      if (tag === 2) return inRange(z, roof[0], roof[1]) ? 'paint' : pillars;
      if (tag === 3) return inRange(z, roof[0], roof[1]) ? (stripe && stripe(ax) ? 'stripe' : 'paint') : 'glass';
      if (bPillar && inRange(z, bPillar[0], bPillar[1])) return 'trim';
      if (cPillar != null && z < cPillar) return 'paint';
      return 'glass';
    }),
    capMat: () => 'glass',
  });
}
// Half-width shaper shared by the body presets.
//   nose/tail: plan taper (fraction of W) over noseLen/tailLen
//   endR: plan-view corner radius at both ends
//   crease: { y, d, h } a ridge along the flank (the character line)
//   flare/lip: arch flares and a raised lip right at the arch edge
function shaper({ W, z0, z1, nose = 0.1, tail = 0.06, taperLen = 0.6, noseLen = taperLen, tailLen = taperLen, endR = 0.2, tumble = 0.05, y0 = 0.6, y1 = 0.9, tuck = 0.04, tuckY = [0.12, 0.38], hips = 0, hipZ = 0, hipW = 0.6, hipY = [0.3, 0.8], crease, arches = [], flare = 0, flareW = 0.22, lip = 0 }) {
  return (y, z) => {
    let f = 1;
    const tn = clamp01((z1 - z) / noseLen);
    const tt = clamp01((z - z0) / tailLen);
    f -= nose * (1 - tn) * (1 - tn) + tail * (1 - tt) * (1 - tt);
    f -= tumble * smooth(y0, y1, y);
    f -= tuck * (1 - smooth(tuckY[0], tuckY[1], y));
    let hw = (W / 2) * f;
    const d = Math.min(z1 - z, z - z0);
    if (d < endR) hw -= endR - Math.sqrt(Math.max(0, endR * endR - (endR - d) ** 2));
    if (hips) hw += hips * bump(z, hipZ, hipW) * smooth(hipY[0] - 0.2, hipY[0], y) * (1 - smooth(hipY[1], hipY[1] + 0.15, y));
    if (crease) hw += crease.d * Math.max(0, 1 - Math.abs(y - crease.y) / crease.h) * (crease.z ? smooth(crease.z[0], crease.z[0] + 0.3, z) * (1 - smooth(crease.z[1] - 0.3, crease.z[1], z)) : 1);
    for (const [zc, yc, R] of arches) {
      const dist = Math.hypot(z - zc, y - yc);
      hw += flare * (1 - smooth(R, R + flareW, dist)) * smooth(yc - R * 0.6, yc, y) + lip * bump(dist, R + 0.035, 0.045);
    }
    return Math.max(0.004, hw);
  };
}
// Side-glass top edge (where the cabin's flank meets its shoulder).
function glassTop(C, z) {
  const H = C.ring(z);
  for (const p of H) if (p.t === 2) return p.y;
  return H[H.length - 1].y;
}
// Black window surround: belt line plus the upper frame of the side glass.
function dlo(P, C, bodyF, zA, zB, { w = 0.022, bucket = 'trim', col = 1, rear = true } = {}) {
  const n = 14, belt = [], upper = [];
  for (let i = 0; i <= n; i++) {
    const z = zB + ((zA - zB) * i) / n;
    belt.push([z, bodyF(z) + w * 0.4]);
    upper.push([z, glassTop(C, z) - w * 0.45]);
  }
  P.add(bucket, ribbon(C, 'side', belt, w, { off: 0.003, mirror: true, col }));
  P.add(bucket, ribbon(C, 'side', upper, w, { off: 0.003, mirror: true, col }));
  if (rear) P.add(bucket, ribbon(C, 'side', [[zB + w * 0.4, bodyF(zB) + 0.01], [zB + w * 0.4, glassTop(C, zB) - 0.01]], w, { off: 0.003, mirror: true, col }));
}

// ── vehicle specs ─────────────────────────────────────────────────
const SPECS = {
  // Front-engined GT: long bonnet, fastback, swept headlights, a slim LED
  // tail bar, quad-ish diffuser exits and a carbon wing.
  sports(P, lod, v) {
    const hi = lod === 'high';
    const r = 0.34, wb = 2.6, W = 1.9, R = r + 0.07, a = wb / 2;
    const z0 = -2.22, z1 = 2.25;
    const arches = [[a, r, R], [-a, r, R]];
    const top = [[-2.22, 0.6], [-2.17, 0.8], [-2.08, 0.89], [-1.9, 0.91], [-1.35, 0.9], [0.75, 0.865], [1.35, 0.81], [1.8, 0.725], [2.06, 0.63], [2.19, 0.53], [2.25, 0.44]];
    const bot = [[-2.22, 0.42], [-2.14, 0.24], [-1.8, 0.19], [1.8, 0.19], [2.14, 0.21], [2.25, 0.3]];
    const topF = spline(top);
    const stripe = v.stripes ? (ax) => ax < 0.17 : null;
    const cab = { z0: -1.95, z1: 0.82 };
    // Police livery: white doors on a black car (between the shut lines).
    const police = v.livery === 'police', WHITE = hi ? 'stripe' : 'plate', doors = [-0.62, 0.75];
    const body = bodyLoft(lod, {
      z0, z1, top, bot, arches, rTop: 0.13, endR: 0.26, crownX: stripe ? [0.17] : [], side: [0.56, 0.6, 0.64], extraZ: police ? doors : [],
      halfW: shaper({ W, z0, z1, nose: 0.1, tail: 0.05, noseLen: 0.95, tailLen: 0.7, endR: 0.26, tumble: 0.075, y0: 0.64, y1: 0.9, tuck: 0.05, hips: 0.045, hipZ: -a, hipW: 0.6, hipY: [0.45, 0.78], crease: { y: 0.6, d: 0.02, h: 0.05 }, arches, flare: 0.02, lip: 0.008 }),
      mat: (tag, y, z, ax) => {
        if (tag === 0) return 'trim';
        if (hi && tag === 3 && inRange(z, cab.z0 + 0.12, cab.z1 - 0.12) && ax < 0.6) return 'trim';
        if (police && tag !== 3 && inRange(z, doors[0], doors[1]) && y > 0.3) return WHITE;
        return tag === 3 && stripe && stripe(ax) ? 'stripe' : 'paint';
      },
    });
    P.addAll(body);
    const S = body.S;
    const cabin = cabinLoft(lod, {
      z0: cab.z0, z1: cab.z1, top: [[-1.95, 0.9], [-1.45, 1.02], [-0.92, 1.17], [-0.5, 1.245], [-0.12, 1.245], [0.12, 1.2], [0.82, 0.86]], bodyTop: topF,
      halfW: (y, z) => 0.745 * (1 - 0.2 * smooth(0.88, 1.25, y)) * (1 - 0.12 * smooth(-0.6, -1.95, z)),
      roof: [-0.66, 0.06], cPillar: -1.02, crownX: stripe ? [0.17] : null, stripe,
    });
    P.addAll(cabin);
    const C = cabin.S;
    wheelWells(P, [a, -a], r, R, W, hi);
    // Headlights: swept lenses on the bonnet corners, DRL brow, two projectors.
    const head = [[0.47, 2.14], [0.64, 2.1], [0.8, 2.0], [0.87, 1.88], [0.84, 1.83], [0.7, 1.9], [0.5, 2.05]];
    lamp(P, S, 'top', head, { nu: hi ? 8 : 2, nv: hi ? 5 : 1 });
    if (hi) {
      glow(P, S, 'top', [[0.5, 2.07], [0.7, 1.925], [0.84, 1.85]], 0.016, 'head');
      dot(P, S, 'top', 0.6, 2.06, 0.03, 'head', { col: 0.9 });
      dot(P, S, 'top', 0.71, 1.99, 0.028, 'head', { col: 0.9 });
      glow(P, S, 'top', [[0.8, 1.97], [0.85, 1.895]], 0.014, 'head', { col: AMBER.map((c) => c * 0.5) });
    }
    // Front: big lower intake with slats, a splitter, bonnet shut lines.
    const intake = [[-0.6, 0.22], [0.6, 0.22], [0.66, 0.34], [0.52, 0.39], [-0.52, 0.39], [-0.66, 0.34]];
    P.add('trim', decal(S, 'front', intake, { off: 0.003, nu: hi ? 8 : 2, nv: hi ? 3 : 1, col: 0.6 }));
    if (hi) {
      for (const y of [0.27, 0.32]) P.add('trim', ribbon(S, 'front', [[-0.58, y], [0.58, y]], 0.012, { off: 0.008, col: 3 }));
      gap(P, S, 'top', [[0.64, 0.85], [0.63, 1.5], [0.5, 2.0]]);
      gap(P, S, 'top', [[-0.44, 2.12], [0.44, 2.12]], { mirror: false });
      // Doors and handle.
      gap(P, S, 'side', [[0.78, 0.85], [0.73, 0.62], [0.76, 0.33]]);
      gap(P, S, 'side', [[-0.62, 0.86], [-0.64, 0.6], [-0.6, 0.33]]);
      P.add('chrome', ribbon(S, 'side', [[-0.3, 0.74], [-0.46, 0.74]], 0.026, { off: 0.004, mirror: true, col: 0.8 }));
      // Fuel flap.
      gap(P, S, 'side', ellipse(-1.62, 0.76, 0.07, 0.06, 10), { closed: true, mirror: false });
      dlo(P, C, topF, 0.66, -1.02);
      interior(P, { z: -0.45, top: 1.14, dashZ: 0.4, dashY: 0.93 });
    }
    P.add('carbon', sweep([[2.27, 0.19], [2.08, 0.19], [2.08, 0.21], [2.24, 0.215]], 'x', -0.76, 0.76));
    P.pair('carbon', () => sweep([[0, 0.19], [0.07, 0.19], [0.075, 0.23], [0, 0.26]], 'z', -0.86, 0.86), 0.86, 0, 0);
    mirrors(P, hi, { x: 0.94, y: 0.97, z: 0.5, base: 0.76 });
    // Rear: tail lamps with a C-shaped LED, a full-width light bar between.
    const tl = [[0.4, 0.74], [0.84, 0.72], [0.88, 0.78], [0.84, 0.835], [0.44, 0.825]];
    lamp(P, S, 'rear', tl, { bucket: 'tail', lens: 0.3, nu: hi ? 8 : 2, nv: hi ? 3 : 1 });
    if (hi) {
      glow(P, S, 'rear', [[0.46, 0.805], [0.83, 0.805], [0.86, 0.775], [0.82, 0.745]], 0.018, 'tail');
      glow(P, S, 'rear', [[-0.4, 0.785], [0.4, 0.785]], 0.012, 'tail', { mirror: false, col: 0.7 });
      gap(P, S, 'rear', [[-0.36, 0.72], [0.36, 0.72]], { mirror: false });
    }
    P.add('rev', decal(S, 'rear', [[0.3, 0.52], [0.44, 0.52], [0.44, 0.555], [0.3, 0.555]], { off: 0.004, nu: 2, nv: 1, mirror: true }));
    P.add('trim', decal(S, 'rear', [[-0.27, 0.45], [0.27, 0.45], [0.27, 0.585], [-0.27, 0.585]], { off: 0.002, nu: 2, nv: 1 }));
    P.box('plate', 0.46, 0.11, 0.02, [0, 0.517, -2.225]);
    // Diffuser with strakes and the tips inside it.
    P.add('carbon', sweep([[-2.24, 0.2], [-1.9, 0.2], [-2.18, 0.4], [-2.26, 0.4]], 'x', -0.72, 0.72));
    for (const x of [-0.18, 0.18]) P.box('carbon', 0.02, 0.17, 0.3, [x, 0.29, -2.12]);
    tip(P, hi, 0.46, 0.3, -2.14, 0.048, 0.13);
    tip(P, hi, 0.34, 0.3, -2.14, 0.048, 0.13);
    if (v.spoiler !== false) {
      wing(P, hi, { span: 1.62, chord: 0.3, y: 1.1, z: -1.96, pitch: -0.1, plateH: 0.14, uprights: [0.46], baseY: 0.88 });
    } else {
      P.add('paint', sweep([[-2.2, 0.88], [-2.02, 0.905], [-2.04, 0.93], [-2.23, 0.935]], 'x', -0.72, 0.72));
    }
    const siren = police ? strobes(P, S, hi, { grille: [0.2, 0.44, 0.28, 0.33], deck: [0.34, topF(-2.07), -2.07] }) : undefined;
    return {
      dims: { length: 4.47, width: W, height: 1.25, wheelRadius: r, wheelBase: wb, track: 1.62 },
      wheels: { r, w: 0.27, track: 1.62, zF: a, zR: -a, rimFrac: 0.7, spokes: 5, type: 'split' },
      head: [0, 0.64, 2.22], exhausts: [[0.4, 0.3, -2.3], [-0.4, 0.3, -2.3]], siren,
    };
  },

  // '69 muscle: long flat bonnet with a scoop, coke-bottle hips, chrome
  // bumpers, quad round lamps in a full-width grille, a full-width tail
  // panel with chrome surround.
  muscle(P, lod, v) {
    const hi = lod === 'high';
    const r = 0.36, wb = 2.8, W = 1.95, R = r + 0.07, a = wb / 2;
    const z0 = -2.37, z1 = 2.49;
    const arches = [[a, r, R], [-a, r, R]];
    const top = [[-2.37, 0.86], [-2.3, 0.945], [-2.1, 0.965], [-1.45, 0.975], [0.3, 0.98], [2.2, 0.945], [2.42, 0.91], [2.49, 0.86]];
    const bot = [[-2.37, 0.44], [-2.3, 0.27], [-1.9, 0.24], [1.9, 0.24], [2.4, 0.26], [2.49, 0.34]];
    const topF = spline(top);
    const stripe = v.stripes ? (ax) => ax > 0.08 && ax < 0.3 : null;
    const crownX = stripe ? [0.3, 0.08] : [];
    const cab = { z0: -1.5, z1: 0.36 };
    const police = v.livery === 'police', WHITE = hi ? 'stripe' : 'plate', doors = [-0.92, 0.39];
    const body = bodyLoft(lod, {
      z0, z1, top, bot, arches, rTop: 0.075, crown: 0.02, crownX, endR: 0.14, side: [0.66, 0.7, 0.74], extraZ: police ? doors : [],
      halfW: shaper({ W, z0, z1, nose: 0.035, tail: 0.035, taperLen: 0.5, endR: 0.14, tumble: 0.045, y0: 0.72, y1: 0.98, tuck: 0.045, hips: 0.04, hipZ: -a, hipW: 0.75, hipY: [0.5, 0.86], crease: { y: 0.7, d: 0.014, h: 0.06 }, arches, flare: 0.012, lip: 0.006 }),
      mat: (tag, y, z, ax) => {
        if (tag === 0) return 'trim';
        if (hi && tag === 3 && inRange(z, cab.z0 + 0.12, cab.z1 - 0.12) && ax < 0.64) return 'trim';
        if (police && tag !== 3 && inRange(z, doors[0], doors[1]) && y > 0.34) return WHITE;
        return tag === 3 && stripe && stripe(ax) ? 'stripe' : 'paint';
      },
    });
    P.addAll(body);
    const S = body.S;
    const cabin = cabinLoft(lod, {
      z0: cab.z0, z1: cab.z1, top: [[-1.5, 0.965], [-1.12, 1.3], [-0.95, 1.345], [-0.3, 1.345], [-0.14, 1.3], [0.36, 0.965]], bodyTop: topF,
      halfW: (y) => 0.78 * (1 - 0.15 * smooth(0.96, 1.34, y)),
      roof: [-1.02, -0.2], bPillar: [-0.64, -0.56], crownX: stripe ? [0.3, 0.08] : null, stripe, linear: true,
    });
    P.addAll(cabin);
    const C = cabin.S;
    wheelWells(P, [a, -a], r, R, W, hi);
    // Hood scoop with a black mouth.
    P.add('paint', sweep([[0.55, 0.955], [1.5, 0.955], [1.42, 1.02], [0.6, 1.06]], 'x', -0.3, 0.3));
    P.box('trim', 0.5, 0.05, 0.02, [0, 1.0, 1.43]);
    // Grille: full-width black panel on the flat nose with chrome surround
    // and horizontal bars; quad round lamps.
    const grille = [[-0.86, 0.5], [0.86, 0.5], [0.86, 0.84], [-0.86, 0.84]];
    P.add('trim', decal(S, 'front', grille, { off: 0.003, nu: hi ? 6 : 1, nv: 1, col: 0.7 }));
    if (hi) {
      P.add('chrome', ribbon(S, 'front', grille, 0.018, { off: 0.006, closed: true, step: 0.1 }));
      for (const y of [0.58, 0.65, 0.72, 0.79]) P.add('chrome', ribbon(S, 'front', [[-0.24, y], [0.24, y]], 0.008, { off: 0.006, col: 0.7 }));
    }
    for (const x of [0.64, 0.4]) {
      const rr = x > 0.5 ? 0.095 : 0.085;
      P.add('chrome', decal(S, 'front', ellipse(x, 0.67, rr + 0.018, rr + 0.018, hi ? 16 : 8), { off: 0.006, nu: 2, nv: 2, mirror: true }));
      P.add('head', decal(S, 'front', ellipse(x, 0.67, rr, rr, hi ? 16 : 8), { off: 0.009, nu: 3, nv: 3, mirror: true, col: hi ? (u, w) => 0.25 + 0.75 * bump(Math.hypot(u - 0.5, w - 0.5), 0, 0.28) : 1 }));
    }
    // Chrome bumpers.
    const bumper = (zc, dir) => sweep([[zc - dir * 0.08, 0.34], [zc + dir * 0.035, 0.34], [zc + dir * 0.06, 0.4], [zc + dir * 0.035, 0.46], [zc - dir * 0.08, 0.46]], 'x', -0.97, 0.97, { n: 1 });
    P.add('chrome', bumper(2.49, 1));
    P.add('chrome', bumper(-2.37, -1));
    P.pair('chrome', () => sweep([[0, 0.34], [0.06, 0.34], [0.07, 0.4], [0.06, 0.46], [0, 0.46]], 'z', 0, 0.32), 0.9, 0, 2.2);
    P.pair('chrome', () => sweep([[0, 0.34], [0.06, 0.34], [0.07, 0.4], [0.06, 0.46], [0, 0.46]], 'z', 0, 0.3), 0.9, 0, -2.45);
    // Tail panel: black inset, chrome surround, two long lamps, reverse.
    const panel = [[-0.9, 0.62], [0.9, 0.62], [0.9, 0.86], [-0.9, 0.86]];
    P.add('trim', decal(S, 'rear', panel, { off: 0.003, nu: 2, nv: 1 }));
    if (hi) P.add('chrome', ribbon(S, 'rear', panel, 0.016, { off: 0.005, closed: true, step: 0.1 }));
    const tl = [[0.16, 0.66], [0.86, 0.66], [0.86, 0.82], [0.16, 0.82]];
    P.add('tail', decal(S, 'rear', tl, { off: 0.006, nu: hi ? 6 : 1, nv: 1, mirror: true, col: 0.35 }));
    if (hi) {
      for (const x of [0.33, 0.51, 0.69]) P.add('chrome', ribbon(S, 'rear', [[x, 0.665], [x, 0.815]], 0.01, { off: 0.009, mirror: true }));
      glow(P, S, 'rear', [[0.18, 0.74], [0.84, 0.74]], 0.05, 'tail', { off: 0.008 });
    }
    P.add('rev', decal(S, 'rear', [[0.04, 0.68], [0.13, 0.68], [0.13, 0.8], [0.04, 0.8]], { off: 0.006, nu: 1, nv: 1, mirror: true }));
    P.box('plate', 0.5, 0.13, 0.02, [0, 0.54, -2.38]);
    if (hi) {
      gap(P, S, 'side', [[0.42, 0.95], [0.36, 0.7], [0.4, 0.34]]);
      gap(P, S, 'side', [[-0.9, 0.96], [-0.94, 0.7], [-0.88, 0.34]]);
      P.add('chrome', ribbon(S, 'side', [[-0.62, 0.86], [-0.76, 0.86]], 0.028, { off: 0.004, mirror: true, col: 0.9 }));
      gap(P, S, 'top', [[-0.84, -2.3], [0.84, -2.3]], { mirror: false });
      gap(P, S, 'top', [[0.84, -2.3], [0.84, -1.58]]);
      // Side marker lights near the corners.
      P.add('head', decal(S, 'side', [[2.1, 0.58], [2.24, 0.58], [2.24, 0.62], [2.1, 0.62]], { off: 0.004, nu: 1, nv: 1, mirror: true, col: AMBER.map((c) => c * 0.4) }));
      P.add('tail', decal(S, 'side', [[-2.08, 0.62], [-2.2, 0.62], [-2.2, 0.66], [-2.08, 0.66]], { off: 0.004, nu: 1, nv: 1, mirror: true, col: 0.3 }));
      P.add('chrome', ribbon(S, 'side', [[1.9, 0.36], [-1.85, 0.36]], 0.018, { off: 0.003, mirror: true, step: 0.2 }));
      dlo(P, C, topF, 0.2, -1.4, { bucket: 'chrome', w: 0.018, rear: false });
      interior(P, { z: -0.62, top: 1.24, dashZ: 0.1, dashY: 1.02, seat: [3.5, 2.2, 1.6] });
    }
    // Ducktail spoiler.
    if (v.spoiler !== false) P.add('paint', sweep([[-2.34, 0.955], [-2.12, 0.975], [-2.14, 1.0], [-2.36, 1.02]], 'x', -0.86, 0.86));
    mirrors(P, hi, { x: 0.93, y: 1.03, z: 0.26, w: 0.14, h: 0.08, d: 0.12, shell: 'chrome', base: 0.8 });
    tip(P, hi, 0.62, 0.27, -2.28, 0.05, 0.16);
    const siren = police ? strobes(P, S, hi, { grille: [0.06, 0.27, 0.645, 0.695], deck: [0.34, topF(-1.64), -1.64] }) : undefined;
    return {
      dims: { length: 4.86, width: W, height: 1.35, wheelRadius: r, wheelBase: wb, track: 1.64 },
      wheels: { r, w: 0.29, track: 1.64, zF: a, zR: -a, rimFrac: 0.62, spokes: 5, type: 'spoke', w0: 0.07, w1: 0.1, dish: 0.07, rimMat: 'rimChrome' },
      head: [0, 0.67, 2.5], exhausts: [[0.62, 0.27, -2.45], [-0.62, 0.27, -2.45]], siren,
    };
  },

  // Mid-engined wedge: cab-forward canopy, Y-shaped headlights on a low
  // nose, huge side intakes, louvred engine cover, hexagonal tail lamps,
  // centre exits and a tall carbon wing.
  super(P, lod, v) {
    const hi = lod === 'high';
    const r = 0.35, wb = 2.7, W = 2.05, R = r + 0.07, a = wb / 2;
    const z0 = -2.28, z1 = 2.29;
    const arches = [[a, r, R], [-a, r, R]];
    const top = [[-2.28, 0.7], [-2.22, 0.88], [-2.1, 0.93], [-0.95, 0.955], [0.95, 0.8], [1.6, 0.645], [2.1, 0.47], [2.29, 0.36]];
    const bot = [[-2.28, 0.44], [-2.18, 0.2], [-1.8, 0.16], [1.8, 0.16], [2.2, 0.19], [2.29, 0.25]];
    const topF = spline(top);
    const stripe = v.stripes ? (ax) => ax < 0.18 : null;
    const cab = { z0: -1.0, z1: 1.02 };
    const body = bodyLoft(lod, {
      z0, z1, top, bot, arches, rTop: 0.1, crown: 0.03, crownX: stripe ? [0.18] : [], endR: 0.3, side: [0.5, 0.58, 0.66],
      halfW: shaper({ W, z0, z1, nose: 0.16, tail: 0.04, noseLen: 1.05, tailLen: 0.5, endR: 0.3, tumble: 0.1, y0: 0.52, y1: 0.95, tuck: 0.06, hips: 0.055, hipZ: -a + 0.1, hipW: 0.65, hipY: [0.4, 0.82], crease: { y: 0.58, d: 0.024, h: 0.06 }, arches, flare: 0.025, lip: 0.01 }),
      mat: (tag, y, z, ax) => {
        if (tag === 0) return 'trim';
        if (hi && tag === 3 && inRange(z, cab.z0 + 0.1, cab.z1 - 0.2) && ax < 0.56) return 'trim';
        if (tag === 3 && z < -1.1 && z > -2.05 && ax < 0.56) return 'trim'; // engine cover glass/louvres
        return tag === 3 && stripe && stripe(ax) && z > -0.9 ? 'stripe' : 'paint';
      },
    });
    P.addAll(body);
    const S = body.S;
    const cabin = cabinLoft(lod, {
      z0: cab.z0, z1: cab.z1, top: [[-1.0, 0.95], [-0.45, 1.13], [-0.05, 1.14], [0.22, 1.1], [1.02, 0.79]], bodyTop: topF,
      halfW: (y, z) => 0.75 * (1 - 0.25 * smooth(0.8, 1.14, y)) * (1 - 0.08 * smooth(-0.4, -1.0, z)),
      roof: [-0.52, 0.04], cPillar: -0.62, pillars: 'trim', crownX: stripe ? [0.18] : null, stripe,
    });
    P.addAll(cabin);
    const C = cabin.S;
    wheelWells(P, [a, -a], r, R, W, hi);
    // Engine cover louvres.
    for (let k = 0; k < 6; k++) P.box('trim', 1.04, 0.02, 0.07, [0, topF(-1.22 - k * 0.14) + 0.008, -1.22 - k * 0.14], [-0.35, 0, 0], 2.5);
    // Y-shaped headlights on the nose slope.
    const head = [[0.5, 2.1], [0.66, 2.02], [0.86, 1.82], [0.88, 1.74], [0.8, 1.76], [0.62, 1.93], [0.48, 2.02]];
    lamp(P, S, 'top', head, { nu: hi ? 8 : 2, nv: hi ? 5 : 1, lens: 0.08 });
    if (hi) {
      glow(P, S, 'top', [[0.52, 2.07], [0.66, 1.99], [0.84, 1.8]], 0.014, 'head');
      glow(P, S, 'top', [[0.66, 1.99], [0.7, 1.9]], 0.012, 'head');
      dot(P, S, 'top', 0.76, 1.87, 0.024, 'head', { col: 0.9 });
      dot(P, S, 'top', 0.82, 1.81, 0.022, 'head', { col: 0.9 });
      gap(P, S, 'top', [[0.46, 1.1], [0.44, 1.7], [0.47, 2.0]]);
      gap(P, S, 'top', [[-0.44, 2.05], [0.44, 2.05]], { mirror: false });
    }
    // Front: three intakes and a splitter.
    P.add('trim', decal(S, 'front', [[0.4, 0.19], [0.88, 0.19], [0.84, 0.32], [0.42, 0.3]], { off: 0.003, nu: hi ? 4 : 1, nv: 1, mirror: true, col: 0.5 }));
    P.add('trim', decal(S, 'front', [[-0.32, 0.19], [0.32, 0.19], [0.3, 0.3], [-0.3, 0.3]], { off: 0.003, nu: hi ? 4 : 1, nv: 1, col: 0.5 }));
    P.add('carbon', sweep([[2.31, 0.15], [2.0, 0.15], [2.0, 0.17], [2.27, 0.175]], 'x', -0.84, 0.84));
    // Side intakes: black scoop ahead of the rear wheel plus a carbon blade.
    const scoop = [[-0.1, 0.36], [-0.8, 0.38], [-0.82, 0.66], [-0.36, 0.62]];
    P.add('trim', decal(S, 'side', scoop, { off: 0.003, nu: hi ? 5 : 1, nv: hi ? 4 : 1, mirror: true, col: 0.35 }));
    if (hi) {
      // Carbon lip round the scoop's leading edge, and slats inside it.
      P.add('carbon', ribbon(S, 'side', [[-0.82, 0.66], [-0.36, 0.62], [-0.1, 0.36]], 0.03, { off: 0.008, mirror: true }));
      for (const y of [0.45, 0.53]) P.add('trim', ribbon(S, 'side', [[-0.3 - (y - 0.36) * 0.9, y], [-0.8, y + 0.01]], 0.012, { off: 0.006, mirror: true, col: 3 }));
      gap(P, S, 'side', [[0.98, 0.78], [0.9, 0.55], [0.92, 0.3]]);
      dlo(P, C, topF, 0.85, -0.62);
      interior(P, { z: -0.36, top: 1.04, dashZ: 0.42, dashY: 0.9, seat: [4, 3.2, 0.6], lean: 0.4 });
    }
    P.pair('carbon', () => sweep([[0, 0.16], [0.08, 0.16], [0.085, 0.2], [0, 0.24]], 'z', -0.9, 0.9), 0.92, 0, 0);
    mirrors(P, hi, { x: 1.0, y: 0.9, z: 0.72, w: 0.17, h: 0.08, d: 0.14, shell: 'carbon', base: 0.8 });
    // Rear: hex lamps, grille mesh, big diffuser, centre tips, plate.
    const tl = [[0.5, 0.72], [0.66, 0.66], [0.86, 0.7], [0.86, 0.78], [0.66, 0.8], [0.5, 0.78]];
    P.add('trim', decal(S, 'rear', [[-0.92, 0.6], [0.92, 0.6], [0.92, 0.86], [-0.92, 0.86]], { off: 0.002, nu: hi ? 6 : 1, nv: 1, col: 0.8 }));
    lamp(P, S, 'rear', tl, { bucket: 'tail', lens: 0.15, bezel: null, nu: hi ? 6 : 2, nv: hi ? 3 : 1, off: 0.006 });
    if (hi) {
      glow(P, S, 'rear', [[0.52, 0.75], [0.66, 0.78], [0.84, 0.75]], 0.016, 'tail', { off: 0.009 });
      glow(P, S, 'rear', [[0.52, 0.73], [0.66, 0.68], [0.84, 0.72]], 0.016, 'tail', { off: 0.009 });
      for (let k = 0; k < 7; k++) P.add('chrome', ribbon(S, 'rear', [[-0.4, 0.63 + k * 0.03], [0.4, 0.63 + k * 0.03]], 0.006, { off: 0.005, col: 0.25 }));
    }
    P.add('rev', decal(S, 'rear', [[0.2, 0.5], [0.34, 0.5], [0.34, 0.54], [0.2, 0.54]], { off: 0.005, nu: 2, nv: 1, mirror: true }));
    P.box('plate', 0.5, 0.12, 0.02, [0, 0.47, -2.265]);
    P.add('carbon', sweep([[-2.3, 0.16], [-1.85, 0.16], [-2.2, 0.4], [-2.3, 0.4]], 'x', -0.8, 0.8));
    for (let k = -2; k <= 2; k++) if (k) P.box('carbon', 0.02, 0.22, 0.4, [k * 0.3, 0.27, -2.12]);
    tip(P, hi, 0.08, 0.4, -2.26, 0.055, 0.12, { mirror: true, oval: 0.75 });
    wing(P, hi, { span: 1.86, chord: 0.36, thick: 0.1, y: 1.2, z: -2.0, pitch: -0.12, plateH: 0.24, uprights: [0.52], baseY: 0.92 });
    return {
      dims: { length: 4.57, width: W, height: 1.14, wheelRadius: r, wheelBase: wb, track: 1.72 },
      wheels: { r, w: 0.3, track: 1.72, zF: a, zR: -a, rimFrac: 0.72, spokes: 5, type: 'split', dish: 0.035, caliper: 1 },
      head: [0, 0.55, 2.2], exhausts: [[0.08, 0.4, -2.4], [-0.08, 0.4, -2.4]],
    };
  },

  // Electric GT: long wheelbase, short overhangs, a glass canopy, no grille,
  // four-point LED headlights, full-width light blades, aero-disc wheels.
  electric(P, lod) {
    const hi = lod === 'high';
    const r = 0.36, wb = 2.9, W = 1.98, R = r + 0.065, a = wb / 2;
    const arches = [[a, r, R], [-a, r, R]];
    const z0 = -2.36, z1 = 2.38;
    const top = [[-2.36, 0.64], [-2.31, 0.86], [-2.18, 0.925], [-1.5, 0.94], [0.9, 0.9], [1.55, 0.81], [2.05, 0.68], [2.28, 0.56], [2.38, 0.44]];
    const bot = [[-2.36, 0.4], [-2.27, 0.24], [-1.95, 0.17], [1.95, 0.17], [2.3, 0.2], [2.38, 0.3]];
    const topF = spline(top);
    const cab = { z0: -1.98, z1: 1.05 };
    const body = bodyLoft(lod, {
      z0, z1, top, bot, arches, rTop: 0.14, crown: 0.03, endR: 0.3, side: [0.52, 0.6, 0.66],
      halfW: shaper({ W, z0, z1, nose: 0.14, tail: 0.07, noseLen: 0.95, tailLen: 0.8, endR: 0.3, tumble: 0.08, y0: 0.6, y1: 0.92, tuck: 0.06, hips: 0.04, hipZ: -a, hipW: 0.65, hipY: [0.45, 0.8], crease: { y: 0.6, d: 0.016, h: 0.07 }, arches, flare: 0.02, lip: 0.006 }),
      mat: (tag, y, z, ax) => {
        if (tag === 0) return 'trim';
        if (hi && tag === 3 && inRange(z, cab.z0 + 0.15, cab.z1 - 0.15) && ax < 0.62) return 'trim';
        return 'paint';
      },
    });
    P.addAll(body);
    const S = body.S;
    const cabin = cabinLoft(lod, {
      z0: cab.z0, z1: cab.z1, bodyTop: topF,
      top: [[-1.98, 0.93], [-1.35, 1.14], [-0.65, 1.265], [-0.1, 1.275], [0.3, 1.235], [1.05, 0.89]],
      halfW: (y, z) => 0.755 * (1 - 0.22 * smooth(0.9, 1.27, y)) * (1 - 0.1 * smooth(-0.9, -1.98, z)),
      roof: [-1.35, 0.3], bPillar: [-0.5, -0.42],
      // Panoramic glass roof with black pillars.
      mat: (tag, y, z) => {
        if (tag === 2) return 'trim';
        if (tag !== 3 && inRange(z, -0.5, -0.42)) return 'trim';
        return 'glass';
      },
    });
    P.addAll(cabin);
    const C = cabin.S;
    wheelWells(P, [a, -a], r, R, W, hi);
    // Headlights: teardrop lens with four LED points and a DRL arc.
    const head = [[0.52, 2.2], [0.72, 2.12], [0.84, 2.0], [0.82, 1.94], [0.66, 2.02], [0.5, 2.12]];
    lamp(P, S, 'top', head, { nu: hi ? 8 : 2, nv: hi ? 4 : 1, lens: 0.07 });
    if (hi) {
      for (const [x, z] of [[0.6, 2.13], [0.66, 2.1], [0.7, 2.06], [0.75, 2.03]]) dot(P, S, 'top', x, z, 0.017, 'head', { n: 8 });
      glow(P, S, 'top', [[0.53, 2.17], [0.72, 2.09], [0.82, 1.97]], 0.012, 'head');
      gap(P, S, 'top', [[0.6, 1.1], [0.58, 1.7], [0.5, 2.12]]);
      gap(P, S, 'top', [[-0.46, 2.21], [0.46, 2.21]], { mirror: false });
      gap(P, S, 'side', [[1.02, 0.86], [0.98, 0.6], [1.0, 0.32]]);
      gap(P, S, 'side', [[-0.46, 0.9], [-0.48, 0.6], [-0.44, 0.3]]);
      gap(P, S, 'side', [[-1.62, 0.9], [-1.66, 0.6], [-1.6, 0.34]]);
      P.add('trim', ribbon(S, 'side', [[0.6, 0.76], [0.44, 0.76]], 0.024, { off: 0.003, mirror: true, col: 3 }));
      P.add('trim', ribbon(S, 'side', [[-0.86, 0.78], [-1.02, 0.78]], 0.024, { off: 0.003, mirror: true, col: 3 }));
      dlo(P, C, topF, 0.9, -1.85, { rear: false });
      interior(P, { z: -0.4, top: 1.16, dashZ: 0.45, dashY: 0.95, seat: [5, 5, 5.2] });
    }
    // Front: lower intake with light hooks.
    P.add('trim', decal(S, 'front', [[-0.62, 0.22], [0.62, 0.22], [0.66, 0.34], [-0.66, 0.34]], { off: 0.003, nu: hi ? 6 : 1, nv: 1, col: 0.5 }));
    P.pair('accent', Box(0.22, 0.025, 0.04), 0.5, 0.36, 2.34);
    // Rear: full-width light bar with a black band, diffuser light.
    P.add('trim', decal(S, 'rear', [[-0.9, 0.72], [0.9, 0.72], [0.9, 0.82], [-0.9, 0.82]], { off: 0.003, nu: hi ? 8 : 1, nv: 1, col: 0.8 }));
    glow(P, S, 'rear', [[-0.88, 0.77], [0.88, 0.77]], 0.022, 'tail', { mirror: false, col: 0.8, off: 0.007 });
    P.add('tail', decal(S, 'rear', [[0.62, 0.735], [0.88, 0.735], [0.88, 0.805], [0.62, 0.805]], { off: 0.008, nu: 2, nv: 1, mirror: true, col: 1 }));
    P.box('accent', 1.3, 0.025, 0.04, [0, 0.3, -2.33]);
    P.add('carbon', sweep([[-2.4, 0.18], [-2.0, 0.18], [-2.3, 0.36], [-2.4, 0.36]], 'x', -0.76, 0.76));
    P.add('trim', decal(S, 'rear', [[-0.27, 0.46], [0.27, 0.46], [0.27, 0.58], [-0.27, 0.58]], { off: 0.002, nu: 2, nv: 1 }));
    P.box('plate', 0.46, 0.11, 0.02, [0, 0.52, -2.365]);
    P.add('rev', decal(S, 'rear', [[0.3, 0.45], [0.44, 0.45], [0.44, 0.48], [0.3, 0.48]], { off: 0.005, nu: 2, nv: 1, mirror: true }));
    mirrors(P, hi, { x: 0.93, y: 0.96, z: 0.76, w: 0.14, h: 0.07, d: 0.12, shell: 'trim', base: 0.76 });
    P.pair('accent', Box(0.012, 0.018, 1.9), 0.965, 0.3, 0);  // sill light line
    return {
      dims: { length: 4.74, width: W, height: 1.28, wheelRadius: r, wheelBase: wb, track: 1.7 },
      wheels: { r, w: 0.29, track: 1.7, zF: a, zR: -a, rimFrac: 0.74, spokes: 0, type: 'aero', rimMat: 'rimAero', caliper: 2 },
      head: [0, 0.6, 2.3], exhausts: [],
    };
  },

  // Rally hatch: boxy flared arches, a roof wing, a bonnet light pod, mud
  // flaps and a big single exhaust. Stripes plus a side graphic are the
  // livery.
  rally(P, lod, v) {
    const hi = lod === 'high';
    const r = 0.34, wb = 2.55, W = 1.9, R = r + 0.08, a = wb / 2;
    const arches = [[a, r, R], [-a, r, R]];
    const z0 = -2.02, z1 = 2.1;
    const top = [[-2.02, 0.95], [-1.96, 1.02], [0.75, 0.99], [1.55, 0.89], [1.96, 0.76], [2.07, 0.64], [2.1, 0.55]];
    const bot = [[-2.02, 0.44], [-1.95, 0.25], [-1.62, 0.21], [1.62, 0.21], [2.02, 0.26], [2.1, 0.38]];
    const topF = spline(top);
    const stripe = v.stripes ? (ax) => ax > 0.09 && ax < 0.22 : null;
    const base = shaper({ W: W - 0.14, z0, z1, nose: 0.1, tail: 0.04, taperLen: 0.5, endR: 0.16, tumble: 0.05, y0: 0.72, y1: 1.0, tuck: 0.04, crease: { y: 0.8, d: 0.01, h: 0.05 } });
    // Box flares over both axles, squared off with a crisp top edge.
    const flare = (y, z) => 0.075 * (smooth(-a - 0.72, -a - 0.5, z) * (1 - smooth(-a + 0.5, -a + 0.72, z)) + smooth(a - 0.72, a - 0.5, z) * (1 - smooth(a + 0.5, a + 0.72, z))) * smooth(0.3, 0.42, y) * (1 - smooth(0.7, 0.78, y));
    const cab = { z0: -1.95, z1: 0.78 };
    const body = bodyLoft(lod, {
      z0, z1, top, bot, arches, rTop: 0.09, crown: 0.02, crownX: stripe ? [0.22, 0.09] : null, endR: 0.16,
      extraZ: [-1.94, a - 0.72, a - 0.5, a + 0.5, a + 0.72, -a - 0.72, -a - 0.5, -a + 0.5, -a + 0.72], side: [0.7, 0.78, 0.8],
      halfW: (y, z) => base(y, z) + flare(y, z),
      mat: (tag, y, z, ax) => {
        if (tag === 0) return 'trim';
        if (hi && tag === 3 && inRange(z, cab.z0 + 0.15, cab.z1 - 0.12) && ax < 0.62) return 'trim';
        return tag === 3 && stripe && stripe(ax) ? 'stripe' : 'paint';
      },
    });
    P.addAll(body);
    const S = body.S;
    const cabin = cabinLoft(lod, {
      z0: cab.z0, z1: cab.z1, bodyTop: topF,
      top: [[-1.95, 1.0], [-1.87, 1.36], [-1.5, 1.46], [-0.2, 1.47], [0.05, 1.43], [0.78, 0.99]], linear: true,
      halfW: (y) => 0.75 * (1 - 0.13 * smooth(1.0, 1.46, y)),
      roof: [-1.55, -0.05], crownX: stripe ? [0.22, 0.09] : null,
      mat: (tag, y, z, ax) => {
        if (tag === 2) return 'paint';
        if (tag === 3) return inRange(z, -1.55, -0.05) ? (stripe && stripe(ax) ? 'stripe' : 'paint') : 'glass';
        if (inRange(z, -0.52, -0.43)) return 'trim';
        if (z < -1.5) return 'paint';
        return 'glass';
      },
    });
    P.addAll(cabin);
    const C = cabin.S;
    wheelWells(P, [a, -a], r, R, W, hi);
    // Headlights, and the four-lamp pod on the front bumper.
    const head = [[0.4, 0.66], [0.76, 0.63], [0.78, 0.72], [0.44, 0.76]];
    lamp(P, S, 'front', head, { nu: hi ? 6 : 2, nv: hi ? 3 : 1, lens: 0.12 });
    if (hi) {
      glow(P, S, 'front', [[0.46, 0.74], [0.76, 0.705]], 0.014, 'head');
      dot(P, S, 'front', 0.56, 0.69, 0.035, 'head', { col: 0.9 });
      dot(P, S, 'front', 0.7, 0.675, 0.035, 'head', { col: 0.9 });
    }
    P.add('trim', decal(S, 'front', [[-0.36, 0.56], [0.36, 0.56], [0.36, 0.72], [-0.36, 0.72]], { off: 0.003, nu: 2, nv: 1, col: 0.6 }));
    P.box('trim', 1.36, 0.24, 0.1, [0, 0.46, 2.12]);
    for (const x of [0.2, 0.52]) {
      P.pair('chrome', Disc(0.118, 0.06, hi ? 18 : 8), x, 0.46, 2.18, [PI / 2, 0, 0]);
      P.pair('head', Disc(0.1, 0.05, hi ? 18 : 8), x, 0.46, 2.19, [PI / 2, 0, 0]);
    }
    P.add('carbon', sweep([[2.15, 0.21], [1.98, 0.21], [1.98, 0.24], [2.13, 0.25]], 'x', -0.84, 0.84));
    if (hi) {
      for (const x of [0.2, 0.52]) P.pair('chrome', Box(0.2, 0.012, 0.01), x, 0.46, 2.22);
      P.pair('trim', Box(0.34, 0.02, 0.26), 0.32, 0.935, 1.25, [0.12, 0, 0], 2);  // bonnet vents
      gap(P, S, 'top', [[0.62, 0.8], [0.6, 1.8]]);
      gap(P, S, 'side', [[0.72, 0.98], [0.68, 0.6], [0.72, 0.36]]);
      gap(P, S, 'side', [[-0.52, 1.0], [-0.54, 0.6], [-0.5, 0.36]]);
      // Side livery: a slanted band in the stripe colour plus a number disc.
      if (stripe) {
        P.add('stripe', decal(S, 'side', [[1.4, 0.42], [1.6, 0.42], [-0.2, 0.9], [-0.45, 0.9]], { off: 0.0025, nu: 3, nv: 6, mirror: true }));
        P.add('plate', decal(S, 'side', ellipse(0.1, 0.66, 0.17, 0.15, 14), { off: 0.003, nu: 4, nv: 4, mirror: true }));
        number(P, S, 'side', '27', 0.1, 0.66, 0.15);
      }
      dlo(P, C, topF, 0.66, -1.5);
      interior(P, { z: -0.6, top: 1.36, dashZ: 0.3, dashY: 1.04, cage: true, seat: [1.4, 1.4, 6] });
    }
    P.box('trim', 0.34, 0.07, 0.3, [0, 1.5, -0.15]);           // roof scoop
    // Roof wing.
    wing(P, hi, { span: 1.66, chord: 0.36, thick: 0.14, y: 1.56, z: -1.92, pitch: 0.12, bucket: 'paint', plateH: 0.14, uprights: [], baseY: 1.44 });
    P.pair('trim', Box(0.04, 0.12, 0.1), 0.36, 1.5, -1.86);
    // Rear: tall corner lamps, bumper, diffuser, single big tip.
    const tl = [[0.6, 0.8], [0.8, 0.8], [0.8, 0.98], [0.68, 0.98]];
    lamp(P, S, 'rear', tl, { bucket: 'tail', lens: 0.2, nu: hi ? 3 : 1, nv: hi ? 4 : 1 });
    if (hi) glow(P, S, 'rear', [[0.69, 0.95], [0.78, 0.95], [0.78, 0.84]], 0.02, 'tail');
    P.add('rev', decal(S, 'rear', [[0.62, 0.72], [0.78, 0.72], [0.78, 0.76], [0.62, 0.76]], { off: 0.005, nu: 2, nv: 1, mirror: true }));
    P.box('trim', 1.84, 0.16, 0.12, [0, 0.38, -2.0]);
    P.box('plate', 0.5, 0.12, 0.02, [0, 0.6, -2.03]);
    tip(P, hi, -0.55, 0.3, -2.02, 0.07, 0.16, { mirror: false });
    // Mud flaps and side skirts.
    P.pair('trim', Box(0.34, 0.28, 0.015), 0.74, 0.2, -a - R - 0.03);
    P.pair('trim', Box(0.3, 0.22, 0.015), 0.74, 0.23, a - R - 0.03);
    P.pair('carbon', () => sweep([[0, 0.2], [0.07, 0.2], [0.075, 0.25], [0, 0.28]], 'z', -0.75, 0.75), 0.86, 0, 0);
    mirrors(P, hi, { x: 0.92, y: 1.05, z: 0.62, w: 0.16, h: 0.1, d: 0.12, base: 0.76 });
    return {
      dims: { length: 4.12, width: W, height: 1.6, wheelRadius: r, wheelBase: wb, track: 1.62 },
      wheels: { r, w: 0.26, track: 1.62, zF: a, zR: -a, rimFrac: 0.66, spokes: 8, type: 'spoke', w0: 0.04, w1: 0.03, dish: 0.03, rimMat: 'rimWhite' },
      head: [0, 0.62, 2.12], exhausts: [[-0.55, 0.3, -2.18]],
    };
  },

  sedan(P, lod) {
    const hi = lod === 'high';
    const r = 0.33, wb = 2.75, W = 1.82, R = r + 0.06, a = wb / 2;
    const z0 = -2.34, z1 = 2.4;
    const arches = [[a, r, R], [-a, r, R]];
    const top = [[-2.34, 0.88], [-2.24, 0.98], [-1.2, 1.01], [0.75, 0.99], [2.1, 0.9], [2.33, 0.82], [2.4, 0.74]];
    const bot = [[-2.34, 0.5], [-2.28, 0.28], [-1.9, 0.24], [1.9, 0.24], [2.35, 0.3], [2.4, 0.46]];
    const body = bodyLoft(lod, {
      z0, z1, top, bot, arches, rTop: 0.09, endR: 0.2,
      halfW: shaper({ W, z0, z1, nose: 0.07, tail: 0.05, taperLen: 0.6, endR: 0.2, tumble: 0.06, y0: 0.72, y1: 1.0, arches, flare: 0.01 }),
    });
    P.addAll(body);
    const S = body.S;
    P.addAll(cabinLoft(lod, {
      z0: -1.38, z1: 0.8, top: [[-1.38, 1.0], [-0.98, 1.4], [-0.75, 1.45], [-0.1, 1.45], [0.12, 1.41], [0.8, 0.98]], bodyTop: spline(top), linear: true,
      halfW: (y) => 0.79 * (1 - 0.14 * smooth(1.0, 1.44, y)),
      roof: [-0.82, 0.02], bPillar: [-0.42, -0.34],
    }));
    lamp(P, S, 'front', [[0.38, 0.72], [0.84, 0.72], [0.82, 0.83], [0.42, 0.83]], { nu: 2, nv: 1, lens: 0.2 });
    P.add('trim', decal(S, 'front', [[-0.34, 0.6], [0.34, 0.6], [0.36, 0.76], [-0.36, 0.76]], { off: 0.003, nu: 1, nv: 1 }));
    lamp(P, S, 'rear', [[0.46, 0.8], [0.84, 0.8], [0.84, 0.92], [0.5, 0.92]], { bucket: 'tail', nu: 2, nv: 1, lens: 0.3 });
    P.add('rev', decal(S, 'rear', [[0.34, 0.82], [0.44, 0.82], [0.44, 0.88], [0.34, 0.88]], { off: 0.005, nu: 2, nv: 1, mirror: true }));
    P.add('trim', sweep([[2.33, 0.3], [2.45, 0.3], [2.46, 0.42], [2.33, 0.44]], 'x', -0.86, 0.86));
    P.add('trim', sweep([[-2.28, 0.32], [-2.4, 0.32], [-2.41, 0.46], [-2.28, 0.48]], 'x', -0.86, 0.86));
    P.box('plate', 0.5, 0.12, 0.02, [0, 0.62, -2.345]);
    gap(P, S, 'side', [[0.78, 0.98], [0.76, 0.34]], { step: 1 });
    gap(P, S, 'side', [[-0.38, 1.0], [-0.4, 0.34]], { step: 1 });
    mirrors(P, hi, { x: 0.9, y: 1.04, z: 0.64, w: 0.14, h: 0.1, d: 0.1, base: 0.78 });
    return {
      dims: { length: 4.74, width: W, height: 1.45, wheelRadius: r, wheelBase: wb, track: 1.56 },
      wheels: { r, w: 0.23, track: 1.56, zF: a, zR: -a, rimFrac: 0.62, spokes: 0, type: 'hub' },
      head: [0, 0.76, 2.4], exhausts: [[0.5, 0.26, -2.3]],
    };
  },

  hatch(P, lod) {
    const hi = lod === 'high';
    const r = 0.31, wb = 2.5, W = 1.75, R = r + 0.06, a = wb / 2;
    const z0 = -1.97, z1 = 2.05;
    const arches = [[a, r, R], [-a, r, R]];
    const top = [[-1.97, 0.95], [-1.88, 1.02], [0.7, 0.98], [1.7, 0.86], [1.98, 0.74], [2.05, 0.62]];
    const bot = [[-1.97, 0.5], [-1.9, 0.28], [-1.6, 0.24], [1.6, 0.24], [2.0, 0.3], [2.05, 0.45]];
    const body = bodyLoft(lod, {
      z0, z1, top, bot, arches, rTop: 0.09, endR: 0.18,
      halfW: shaper({ W, z0, z1, nose: 0.09, tail: 0.04, taperLen: 0.5, endR: 0.18, tumble: 0.05, y0: 0.7, y1: 1.0, arches, flare: 0.012 }),
    });
    P.addAll(body);
    const S = body.S;
    P.addAll(cabinLoft(lod, {
      z0: -1.95, z1: 0.75, top: [[-1.95, 1.0], [-1.87, 1.38], [-1.5, 1.48], [-0.2, 1.49], [0.05, 1.45], [0.75, 0.98]], bodyTop: spline(top), linear: true,
      halfW: (y) => 0.77 * (1 - 0.12 * smooth(1.0, 1.48, y)),
      roof: [-1.55, -0.05],
      mat: (tag, y, z) => {
        if (tag === 2) return 'paint';
        if (tag === 3) return inRange(z, -1.55, -0.05) ? 'paint' : 'glass';
        if (inRange(z, -0.52, -0.43)) return 'trim';
        if (z < -1.5) return 'paint';
        return 'glass';
      },
    }));
    lamp(P, S, 'front', [[0.36, 0.7], [0.78, 0.7], [0.76, 0.8], [0.42, 0.82]], { nu: 2, nv: 1, lens: 0.2 });
    P.add('trim', decal(S, 'front', [[-0.32, 0.56], [0.32, 0.56], [0.3, 0.68], [-0.3, 0.68]], { off: 0.003, nu: 1, nv: 1 }));
    lamp(P, S, 'rear', [[0.62, 0.84], [0.8, 0.84], [0.8, 1.0], [0.68, 1.0]], { bucket: 'tail', nu: 1, nv: 1, lens: 0.3 });
    P.add('rev', decal(S, 'rear', [[0.64, 0.76], [0.78, 0.76], [0.78, 0.8], [0.64, 0.8]], { off: 0.005, nu: 2, nv: 1, mirror: true }));
    P.add('trim', sweep([[1.98, 0.3], [2.1, 0.3], [2.11, 0.42], [1.98, 0.44]], 'x', -0.82, 0.82));
    P.add('trim', sweep([[-1.92, 0.32], [-2.03, 0.32], [-2.04, 0.46], [-1.92, 0.48]], 'x', -0.82, 0.82));
    P.box('plate', 0.5, 0.12, 0.02, [0, 0.62, -1.975]);
    gap(P, S, 'side', [[0.72, 0.97], [0.7, 0.34]], { step: 1 });
    mirrors(P, hi, { x: 0.86, y: 1.04, z: 0.58, w: 0.13, h: 0.1, d: 0.1, base: 0.74 });
    return {
      dims: { length: 4.02, width: W, height: 1.49, wheelRadius: r, wheelBase: wb, track: 1.5 },
      wheels: { r, w: 0.21, track: 1.5, zF: a, zR: -a, rimFrac: 0.62, spokes: 0, type: 'hub' },
      head: [0, 0.74, 2.04], exhausts: [[0.45, 0.25, -1.95]],
    };
  },

  van(P, lod) {
    const hi = lod === 'high';
    const r = 0.34, wb = 3.0, W = 1.95, R = r + 0.07, a = wb / 2;
    const z0 = -2.43, z1 = 2.6;
    const arches = [[a, r, R], [-a, r, R]];
    const top = [[-2.43, 1.98], [-2.34, 2.07], [1.3, 2.08], [1.65, 1.97], [2.25, 1.18], [2.52, 1.02], [2.6, 0.9]];
    const bot = [[-2.43, 0.34], [-2.38, 0.3], [-1.9, 0.28], [1.9, 0.28], [2.55, 0.3], [2.6, 0.42]];
    const body = bodyLoft(lod, {
      z0, z1, top, bot, arches, rTop: 0.12, crown: 0.02, endR: 0.16, linear: true,
      side: [1.25, 1.92], extraZ: [0.55, 1.66, 2.22, -1.85, -0.2, -1.05, -0.95],
      halfW: shaper({ W, z0, z1, nose: 0.07, tail: 0.02, taperLen: 0.55, endR: 0.16, tumble: 0.05, y0: 1.2, y1: 2.08, tuck: 0.03, arches, flare: 0.012 }),
      mat: (tag, y, z) => {
        if (tag === 0) return 'trim';
        if (tag >= 2 && inRange(z, 1.66, 2.22)) return 'glass';
        if (tag === 1 && inRange(y, 1.25, 1.92) && inRange(z, 0.55, 2.25)) return 'glass';
        if (tag === 1 && inRange(y, 1.25, 1.92) && (inRange(z, -1.85, -1.05) || inRange(z, -0.95, -0.2))) return 'glass';
        return 'paint';
      },
    });
    P.addAll(body);
    const S = body.S;
    P.add('glass', decal(S, 'rear', [[-0.76, 1.3], [0.76, 1.3], [0.74, 1.84], [-0.74, 1.84]], { off: 0.004, nu: 1, nv: 1 }));
    P.add('trim', ribbon(S, 'rear', [[0, 1.3], [0, 1.84]], 0.03, { off: 0.006, step: 1 }));
    P.add('trim', ribbon(S, 'rear', [[0, 0.4], [0, 1.3]], 0.008, { off: 0.003, step: 1 }));
    lamp(P, S, 'front', [[0.44, 0.8], [0.84, 0.8], [0.82, 0.96], [0.48, 0.96]], { nu: 2, nv: 1, lens: 0.2 });
    P.add('trim', decal(S, 'front', [[-0.38, 0.66], [0.38, 0.66], [0.38, 0.92], [-0.38, 0.92]], { off: 0.003, nu: 1, nv: 1 }));
    lamp(P, S, 'rear', [[0.8, 0.8], [0.92, 0.8], [0.92, 1.22], [0.8, 1.22]], { bucket: 'tail', nu: 1, nv: 1, lens: 0.3 });
    P.add('rev', decal(S, 'rear', [[0.8, 0.66], [0.92, 0.66], [0.92, 0.74], [0.8, 0.74]], { off: 0.005, nu: 2, nv: 1, mirror: true }));
    P.add('trim', sweep([[2.52, 0.3], [2.66, 0.3], [2.67, 0.5], [2.52, 0.52]], 'x', -0.95, 0.95));
    P.add('trim', sweep([[-2.36, 0.3], [-2.49, 0.3], [-2.5, 0.5], [-2.36, 0.52]], 'x', -0.95, 0.95));
    P.box('plate', 0.5, 0.12, 0.02, [0, 0.64, -2.445]);
    // Sliding door rail and shut lines.
    gap(P, S, 'side', [[0.5, 1.95], [0.5, 0.36]], { step: 2 });
    gap(P, S, 'side', [[-0.98, 1.95], [-0.98, 0.36]], { step: 2, mirror: false });
    P.add('trim', ribbon(S, 'side', [[-0.98, 1.2], [-2.3, 1.2]], 0.03, { off: 0.004, step: 2 }));
    mirrors(P, hi, { x: 1.06, y: 1.46, z: 1.42, w: 0.1, h: 0.22, d: 0.12, shell: 'trim', base: 0.92 });
    return {
      dims: { length: 5.03, width: W, height: 2.08, wheelRadius: r, wheelBase: wb, track: 1.66 },
      wheels: { r, w: 0.23, track: 1.66, zF: a, zR: -a, rimFrac: 0.58, spokes: 0, type: 'steel' },
      head: [0, 0.86, 2.6], exhausts: [[0.55, 0.27, -2.42]],
    };
  },

  pickup(P, lod) {
    const hi = lod === 'high';
    const r = 0.38, wb = 3.3, W = 2.0, R = r + 0.08, a = wb / 2;
    const z0 = -2.58, z1 = 2.76;
    const arches = [[a, r, R], [-a, r, R]];
    const top = [[-2.58, 0.72], [-0.63, 0.72], [-0.6, 1.12], [0.9, 1.12], [2.5, 1.08], [2.72, 0.98], [2.76, 0.84]];
    const bot = [[-2.58, 0.5], [-2.54, 0.34], [-2.1, 0.32], [2.3, 0.32], [2.72, 0.36], [2.76, 0.5]];
    const body = bodyLoft(lod, {
      z0, z1, top, bot, arches, rTop: 0.07, crown: 0.02, endR: 0.12, linear: true,
      halfW: shaper({ W, z0, z1, nose: 0.04, tail: 0.01, taperLen: 0.45, endR: 0.12, tumble: 0.03, y0: 0.9, y1: 1.12, tuck: 0.03, arches, flare: 0.03, flareW: 0.16 }),
    });
    P.addAll(body);
    const S = body.S;
    P.addAll(cabinLoft(lod, {
      z0: -0.64, z1: 0.96, top: [[-0.64, 1.78], [-0.55, 1.84], [0.25, 1.84], [0.42, 1.8], [0.96, 1.1]], linear: true,
      bodyTop: (z) => 1.12,
      halfW: (y) => 0.9 * (1 - 0.08 * smooth(1.1, 1.84, y)),
      roof: [-0.64, 0.3], bPillar: [-0.08, 0.02],
    }));
    // Load bed: sides, tailgate, floor.
    P.pair('paint', Box(0.07, 0.42, 1.95), W / 2 - 0.035, 0.93, -1.6);
    P.box('paint', W, 0.42, 0.07, [0, 0.93, -2.56]);
    P.box('trim', W - 0.14, 0.02, 1.9, [0, 0.73, -1.6]);
    P.pair('trim', Box(0.09, 0.03, 1.95), W / 2 - 0.04, 1.155, -1.6);
    // Grille with chrome surround, lamps, bumpers.
    P.add('chrome', decal(S, 'front', [[-0.56, 0.62], [0.56, 0.62], [0.56, 0.98], [-0.56, 0.98]], { off: 0.003, nu: 1, nv: 1 }));
    P.add('trim', decal(S, 'front', [[-0.5, 0.66], [0.5, 0.66], [0.5, 0.94], [-0.5, 0.94]], { off: 0.006, nu: 1, nv: 1 }));
    P.box('chrome', 1.0, 0.03, 0.02, [0, 0.8, 2.77]);
    lamp(P, S, 'front', [[0.62, 0.76], [0.92, 0.76], [0.9, 0.96], [0.62, 0.96]], { nu: 1, nv: 1, lens: 0.25 });
    P.pair('tail', Box(0.1, 0.3, 0.05), 0.94, 0.95, -2.59);
    P.pair('rev', Box(0.08, 0.08, 0.04), 0.94, 0.75, -2.6);
    P.add('chrome', sweep([[2.72, 0.36], [2.84, 0.36], [2.86, 0.46], [2.84, 0.56], [2.72, 0.56]], 'x', -1.0, 1.0));
    P.add('chrome', sweep([[-2.56, 0.38], [-2.68, 0.38], [-2.7, 0.47], [-2.68, 0.56], [-2.56, 0.56]], 'x', -1.0, 1.0));
    P.box('plate', 0.5, 0.12, 0.02, [0, 0.9, -2.6]);
    gap(P, S, 'side', [[1.0, 1.1], [0.98, 0.4]], { step: 1 });
    gap(P, S, 'side', [[-0.6, 1.1], [-0.6, 0.4]], { step: 1 });
    mirrors(P, hi, { x: 1.06, y: 1.38, z: 0.84, w: 0.12, h: 0.2, d: 0.12, shell: 'trim', base: 0.9 });
    return {
      dims: { length: 5.36, width: W, height: 1.84, wheelRadius: r, wheelBase: wb, track: 1.72 },
      wheels: { r, w: 0.27, track: 1.72, zF: a, zR: -a, rimFrac: 0.6, spokes: 0, type: 'steel' },
      head: [0, 0.86, 2.76], exhausts: [[0.7, 0.3, -2.5]],
    };
  },

  boxtruck(P, lod) {
    const hi = lod === 'high';
    const r = 0.48, wb = 4.2, W = 2.3, R = r + 0.08, a = wb / 2;
    // Cab loft (the cargo box is a plain box behind it).
    const cabZ0 = 1.5, cabZ1 = 3.72;
    const body = bodyLoft(lod, {
      z0: cabZ0, z1: cabZ1, top: [[1.5, 2.62], [2.85, 2.62], [3.3, 1.62], [3.62, 1.47], [3.72, 1.3]], bot: [[1.5, 0.5], [3.72, 0.5]], linear: true,
      arches: [[a, r, R]], rTop: 0.12, crown: 0.02, side: [1.7, 2.5], extraZ: [2.2, 2.87, 3.28], endR: 0.14,
      halfW: shaper({ W, z0: cabZ0, z1: cabZ1, nose: 0.05, tail: 0, taperLen: 0.35, endR: 0.14, tumble: 0.03, y0: 1.5, y1: 2.6, tuck: 0.02 }),
      mat: (tag, y, z) => {
        if (tag === 0) return 'trim';
        if (tag >= 2 && inRange(z, 2.87, 3.28)) return 'glass';
        if (tag === 1 && inRange(y, 1.7, 2.5) && inRange(z, 2.2, 3.3)) return 'glass';
        return 'paint';
      },
    });
    P.addAll(body);
    const S = body.S;
    // Cargo box with corner posts and a roll-up door.
    P.box('cargo', 2.44, 2.5, 5.0, [0, 2.12, -0.95]);
    for (const sx of [1, -1]) P.box('trim', 0.05, 2.52, 0.05, [sx * 1.215, 2.12, -3.44], undefined, 3);
    P.box('trim', 2.46, 0.06, 0.05, [0, 3.36, -3.44], undefined, 3);
    for (let k = 1; k < 6; k++) P.box('trim', 2.3, 0.015, 0.012, [0, 0.9 + k * 0.4, -3.452], undefined, 6);
    P.box('trim', 1.0, 0.25, 4.6, [0, 0.72, -0.6]);
    P.box('trim', 2.3, 0.2, 0.2, [0, 0.62, -3.4]);
    P.pair('trim', Box(0.08, 0.55, 1.3), 1.08, 0.72, -a);
    lamp(P, S, 'front', [[0.66, 0.92], [1.0, 0.92], [1.0, 1.1], [0.66, 1.1]], { nu: 1, nv: 1, lens: 0.25 });
    P.add('trim', decal(S, 'front', [[-0.56, 0.86], [0.56, 0.86], [0.56, 1.34], [-0.56, 1.34]], { off: 0.003, nu: 1, nv: 1 }));
    P.add('chrome', sweep([[3.66, 0.5], [3.8, 0.5], [3.82, 0.6], [3.8, 0.72], [3.66, 0.72]], 'x', -1.15, 1.15));
    P.pair('tail', Box(0.14, 0.26, 0.05), 1.05, 0.92, -3.46);
    P.pair('rev', Box(0.1, 0.1, 0.04), 0.82, 0.92, -3.46);
    mirrors(P, hi, { x: 1.28, y: 2.1, z: 2.9, w: 0.1, h: 0.34, d: 0.12, shell: 'trim', base: 1.1 });
    return {
      dims: { length: 7.2, width: 2.44, height: 3.37, wheelRadius: r, wheelBase: wb, track: 1.9 },
      wheels: { r, w: 0.34, track: 1.9, zF: a, zR: -a, rimFrac: 0.58, spokes: 0, type: 'steel', rearW: 0.5 },
      head: [0, 1.0, 3.72], exhausts: [[0.9, 0.4, -3.3]],
    };
  },

  tractor(P, lod) {
    const hi = lod === 'high';
    const wb = 2.1, a = wb / 2, rF = 0.42, rR = 0.76;
    // Engine block / hood, rounded on top.
    const D = detail(lod);
    const hood = loft({
      zs: stations(-0.35, 1.8, D.cabUni, [1.72]),
      top: (z) => interp([[-0.35, 1.58], [1.62, 1.52], [1.8, 1.38]], z), bot: () => 0.82,
      halfW: (y) => 0.37 * (1 - 0.1 * smooth(1.3, 1.58, y)), rTop: 0.14, rBot: 0.02, crown: 0.02,
      nb: 0, nt: D.nt, sideN: 0, nCrown: 1, mat: (tag) => (tag === 0 ? 'trim' : 'paint'), capMat: (front) => (front ? 'trim' : 'paint'),
    });
    P.addAll(hood);
    for (let k = 0; k < 5; k++) P.box('chrome', 0.6, 0.02, 0.02, [0, 0.92 + k * 0.1, 1.81]);
    P.box('trim', 0.5, 0.22, 2.6, [0, 0.7, 0.1]);          // chassis
    P.box('trim', 1.3, 0.14, 0.18, [0, rF, a]);             // front axle
    P.box('trim', 1.5, 0.18, 0.2, [0, rR, -a]);             // rear axle housing
    // Fenders over the rear wheels.
    const n = hi ? 10 : 6, fender = [];
    for (let i = 0; i <= n; i++) { const t = 0.12 + (0.76 * i) / n; fender.push([Math.cos(t * PI) * 0.9, Math.sin(t * PI) * 0.9]); }
    for (let i = fender.length - 1; i >= 0; i--) fender.push([fender[i][0] * 0.94, fender[i][1] * 0.94]);
    for (const sx of [1, -1]) P.add('paint', extrude(fender, 0.5, 0.01), [sx * 0.78, rR, -a]);
    P.box('paint', 1.06, 0.06, 0.9, [0, 1.2, -a + 0.15]);  // platform
    P.box('seat', 0.5, 0.1, 0.45, [0, 1.46, -a - 0.05]);
    P.box('seat', 0.5, 0.45, 0.08, [0, 1.7, -a - 0.3], [-0.2, 0, 0]);
    P.cyl('trim', 0.03, 0.03, 0.7, 6, [0, 1.62, -0.35], [0.6, 0, 0]);
    P.add('trim', new THREE.TorusGeometry(0.19, 0.022, 5, hi ? 16 : 10), [0, 1.9, -0.55], [-0.95, 0, 0]);
    P.cyl('trim', 0.05, 0.05, 1.0, 8, [0.22, 1.95, 1.25]);
    P.cyl('trim', 0.065, 0.05, 0.12, 8, [0.22, 2.48, 1.25]);
    P.pair('head', Box(0.12, 0.1, 0.06), 0.28, 1.36, 1.79);
    P.pair('tail', Box(0.08, 0.08, 0.04), 0.95, 1.5, -a - 0.62);
    return {
      dims: { length: 3.4, width: 1.95, height: 2.54, wheelRadius: rR, wheelBase: wb, track: 1.5 },
      wheels: { r: rR, w: 0.42, track: 1.5, zF: a, zR: -a, rimFrac: 0.55, spokes: 0, type: 'steel', front: { r: rF, w: 0.2, track: 1.3 }, rimMat: 'rimTractor' },
      head: [0, 1.36, 1.84], exhausts: [[0.22, 2.54, 1.25]],
    };
  },

  // Patrol sedan: a full-size four-door between the traffic sedan and the GT
  // (lower roof, smoother nose), black with white doors and roof, a push bar
  // and a roof lightbar.
  police(P, lod) {
    const hi = lod === 'high';
    const WHITE = hi ? 'stripe' : 'plate';
    const r = 0.34, wb = 2.9, W = 1.9, R = r + 0.065, a = wb / 2;
    const z0 = -2.42, z1 = 2.52;
    const arches = [[a, r, R], [-a, r, R]];
    const top = [[-2.42, 0.86], [-2.34, 0.97], [-2.12, 1.0], [-1.2, 1.01], [0.8, 0.99], [1.7, 0.94], [2.3, 0.84], [2.46, 0.74], [2.52, 0.62]];
    const bot = [[-2.42, 0.48], [-2.36, 0.28], [-1.95, 0.24], [1.95, 0.24], [2.42, 0.28], [2.52, 0.4]];
    const topF = spline(top);
    const cab = { z0: -1.45, z1: 0.9 }, roof = [-0.95, 0.12], doors = [-1.32, 0.86], bp = [-0.42, -0.34];
    const body = bodyLoft(lod, {
      z0, z1, top, bot, arches, rTop: 0.11, endR: 0.22, side: [0.34], extraZ: doors,
      halfW: shaper({ W, z0, z1, nose: 0.08, tail: 0.05, noseLen: 0.8, tailLen: 0.6, endR: 0.22, tumble: 0.06, y0: 0.7, y1: 0.98, tuck: 0.04, hips: 0.02, hipZ: -a, hipW: 0.6, hipY: [0.5, 0.85], crease: { y: 0.72, d: 0.012, h: 0.05 }, arches, flare: 0.012 }),
      mat: (tag, y, z, ax) => {
        if (tag === 0) return 'trim';
        if (hi && tag === 3 && inRange(z, cab.z0 + 0.12, cab.z1 - 0.12) && ax < 0.66) return 'trim';
        if (tag !== 3 && inRange(z, doors[0], doors[1]) && y > 0.34) return WHITE;
        return 'paint';
      },
    });
    P.addAll(body);
    const S = body.S;
    const cabin = cabinLoft(lod, {
      z0: cab.z0, z1: cab.z1, top: [[-1.45, 1.0], [-1.05, 1.36], [-0.8, 1.435], [-0.1, 1.445], [0.18, 1.4], [0.9, 0.98]], bodyTop: topF,
      halfW: (y, z) => 0.8 * (1 - 0.16 * smooth(0.98, 1.44, y)) * (1 - 0.04 * smooth(-0.6, -1.45, z)),
      roof, bPillar: bp,
      mat: (tag, y, z) => {
        if (tag >= 2 && inRange(z, roof[0], roof[1])) return WHITE;
        if (tag === 2) return 'paint';
        if (tag === 1 && inRange(z, bp[0], bp[1])) return 'trim';
        return 'glass';
      },
    });
    P.addAll(cabin);
    const C = cabin.S;
    if (hi) wheelWells(P, [a, -a], r, R, W, hi);
    // Front: headlights, black grille, bumper and the push bar.
    lamp(P, S, 'front', [[0.4, 0.62], [0.8, 0.6], [0.8, 0.7], [0.44, 0.72]], { nu: hi ? 6 : 2, nv: hi ? 3 : 1, lens: 0.15 });
    if (hi) {
      glow(P, S, 'front', [[0.44, 0.7], [0.78, 0.68]], 0.014, 'head');
      dot(P, S, 'front', 0.54, 0.66, 0.035, 'head', { col: 0.9 });
      dot(P, S, 'front', 0.68, 0.655, 0.035, 'head', { col: 0.9 });
    }
    P.add('trim', decal(S, 'front', [[-0.36, 0.5], [0.36, 0.5], [0.38, 0.66], [-0.38, 0.66]], { off: 0.003, nu: hi ? 4 : 1, nv: 1, col: 0.6 }));
    P.add('trim', sweep([[2.44, 0.26], [2.58, 0.26], [2.59, 0.4], [2.44, 0.42]], 'x', -0.9, 0.9));
    pushBar(P, hi, { z: 2.66, y0: 0.3, y1: 0.86, x: 0.34 });
    // Rear: lamps, reverse, bumper, plate.
    lamp(P, S, 'rear', [[0.42, 0.78], [0.84, 0.78], [0.84, 0.9], [0.46, 0.9]], { bucket: 'tail', nu: hi ? 4 : 2, nv: 1, lens: 0.3 });
    if (hi) glow(P, S, 'rear', [[0.46, 0.84], [0.82, 0.84]], 0.02, 'tail');
    P.add('rev', decal(S, 'rear', [[0.3, 0.8], [0.4, 0.8], [0.4, 0.88], [0.3, 0.88]], { off: 0.005, nu: 2, nv: 1, mirror: true }));
    P.add('trim', sweep([[-2.34, 0.3], [-2.48, 0.3], [-2.49, 0.46], [-2.34, 0.48]], 'x', -0.9, 0.9));
    P.box('plate', 0.5, 0.12, 0.02, [0, 0.62, -2.43]);
    // Four doors: shut lines at the white panel edges and the B-pillar.
    gap(P, S, 'side', [[doors[1], 0.98], [doors[1] - 0.02, 0.36]], { step: hi ? 0.05 : 1 });
    gap(P, S, 'side', [[bp[0] + 0.04, 1.0], [bp[0] + 0.02, 0.36]], { step: hi ? 0.05 : 1 });
    gap(P, S, 'side', [[doors[0], 1.0], [doors[0], 0.8]], { step: hi ? 0.05 : 1 });
    mirrors(P, hi, { x: 0.93, y: 1.04, z: 0.72, w: 0.15, h: 0.1, d: 0.11, base: 0.78 });
    const zb = -0.28, yb = C.topY(zb, 0.56) - 0.004;
    const siren = lightbar(P, hi, { y: yb, z: zb, w: 1.24 });
    if (hi) {
      gap(P, S, 'top', [[-0.8, -2.3], [0.8, -2.3]], { mirror: false });
      gap(P, S, 'top', [[0.62, 1.0], [0.6, 2.0], [0.44, 2.38]]);
      for (const z of [0.28, -0.9]) P.add('chrome', ribbon(S, 'side', [[z, 0.8], [z - 0.14, 0.8]], 0.026, { off: 0.004, mirror: true, col: 0.8 }));
      number(P, S, 'side', 'POLICE', -0.24, 0.62, 0.14);
      dlo(P, C, topF, 0.8, -1.38, { rear: false });
      interior(P, { z: -0.1, top: 1.33, dashZ: 0.62, dashY: 1.02, seat: [1.4, 1.4, 1.5] });
      // Cage partition behind the front seats, pillar spotlight, antennas.
      P.add('trim', new THREE.BoxGeometry(1.4, 0.4, 0.02), [0, 1.18, -0.42], undefined, 0.5);
      P.add('chrome', new THREE.CylinderGeometry(0.055, 0.045, 0.14, 12), [0.86, 1.1, 0.74], [PI / 2, 0, 0]);
      P.add('chrome', new THREE.CylinderGeometry(0.012, 0.012, 0.18, 6), [0.82, 1.08, 0.68]);
      for (const x of [0.25, -0.25]) P.add('trim', new THREE.CylinderGeometry(0.004, 0.006, 0.55, 4), [x, 1.26, -2.0]);
    }
    return {
      dims: { length: 5.1, width: W, height: 1.59, wheelRadius: r, wheelBase: wb, track: 1.6 },
      wheels: { r, w: 0.245, track: 1.6, zF: a, zR: -a, rimFrac: 0.6, spokes: 0, type: 'steel', rimMat: 'rimDark' },
      head: [0, 0.66, 2.5], exhausts: [[0.5, 0.26, -2.4]], siren,
    };
  },

  // Police SUV: a tall two-box body with a near-vertical tail, black with
  // white doors and roof, a heavy push bar, roof rails and a lightbar.
  policeSuv(P, lod) {
    const hi = lod === 'high';
    const WHITE = hi ? 'stripe' : 'plate';
    const r = 0.39, wb = 2.95, W = 2.0, R = r + 0.08, a = wb / 2;
    const z0 = -2.42, z1 = 2.5;
    const arches = [[a, r, R], [-a, r, R]];
    const top = [[-2.42, 1.12], [-2.38, 1.16], [1.4, 1.17], [2.26, 1.12], [2.44, 1.04], [2.5, 0.9]];
    const bot = [[-2.42, 0.56], [-2.36, 0.42], [-1.95, 0.4], [1.95, 0.4], [2.44, 0.44], [2.5, 0.56]];
    const topF = (z) => interp(top, z);
    const doors = [-1.28, 1.02], bp = [-0.2, -0.1], cp = [-1.36, -1.24], roof = [-2.24, 0.52];
    const body = bodyLoft(lod, {
      z0, z1, top, bot, arches, rTop: 0.08, crown: 0.02, endR: 0.14, linear: true, side: [0.48], extraZ: doors,
      halfW: shaper({ W, z0, z1, nose: 0.04, tail: 0.02, taperLen: 0.45, endR: 0.14, tumble: 0.03, y0: 0.95, y1: 1.17, tuck: 0.03, tuckY: [0.4, 0.6], arches, flare: 0.03, flareW: 0.18 }),
      mat: (tag, y, z, ax) => {
        if (tag === 0) return 'trim';
        if (hi && tag === 3 && inRange(z, -2.3, 1.35) && ax < 0.84) return 'trim';
        if (tag !== 3 && inRange(z, doors[0], doors[1]) && y > 0.48) return WHITE;
        return 'paint';
      },
    });
    P.addAll(body);
    const S = body.S;
    const cabin = cabinLoft(lod, {
      z0: -2.4, z1: 1.45, top: [[-2.4, 1.16], [-2.36, 1.86], [-2.24, 1.93], [0.45, 1.94], [0.64, 1.9], [1.45, 1.17]], bodyTop: topF, linear: true,
      halfW: (y) => 0.93 * (1 - 0.09 * smooth(1.15, 1.93, y)),
      roof, bPillar: bp, rTop: 0.07,
      mat: (tag, y, z) => {
        if (tag >= 2 && inRange(z, roof[0], roof[1])) return WHITE;
        if (tag >= 2) return tag === 2 || z < roof[0] ? 'paint' : 'glass';
        if (inRange(z, bp[0], bp[1]) || inRange(z, cp[0], cp[1])) return 'trim';
        if (z < -2.3) return 'paint';
        return 'glass';
      },
    });
    P.addAll(cabin);
    const C = cabin.S;
    if (hi) wheelWells(P, [a, -a], r, R, W, hi);
    // Front: wide black grille between the lamps, bumper, big push bar.
    lamp(P, S, 'front', [[0.52, 0.86], [0.86, 0.84], [0.86, 0.98], [0.54, 1.0]], { nu: hi ? 5 : 2, nv: hi ? 3 : 1, lens: 0.15 });
    if (hi) {
      glow(P, S, 'front', [[0.55, 0.98], [0.84, 0.96]], 0.016, 'head');
      dot(P, S, 'front', 0.64, 0.92, 0.04, 'head', { col: 0.9 });
      dot(P, S, 'front', 0.77, 0.91, 0.04, 'head', { col: 0.9 });
    }
    P.add('trim', decal(S, 'front', [[-0.48, 0.66], [0.48, 0.66], [0.48, 1.0], [-0.48, 1.0]], { off: 0.003, nu: hi ? 4 : 1, nv: 1, col: 0.6 }));
    if (hi) for (const y of [0.74, 0.82, 0.9]) P.add('chrome', ribbon(S, 'front', [[-0.46, y], [0.46, y]], 0.012, { off: 0.006, col: 0.5 }));
    P.add('trim', sweep([[2.4, 0.4], [2.58, 0.4], [2.6, 0.6], [2.4, 0.64]], 'x', -1.0, 1.0));
    pushBar(P, hi, { z: 2.68, y0: 0.4, y1: 1.1, x: 0.42 });
    // Rear: tall corner lamps, a black band on the tailgate, bumper, plate.
    lamp(P, S, 'rear', [[0.66, 0.78], [0.84, 0.78], [0.84, 1.1], [0.7, 1.1]], { bucket: 'tail', nu: hi ? 2 : 1, nv: hi ? 3 : 1, lens: 0.3 });
    if (hi) glow(P, S, 'rear', [[0.76, 0.82], [0.76, 1.07]], 0.03, 'tail');
    P.add('rev', decal(S, 'rear', [[0.66, 0.68], [0.84, 0.68], [0.84, 0.74], [0.66, 0.74]], { off: 0.005, nu: 2, nv: 1, mirror: true }));
    P.add('trim', decal(S, 'rear', [[-0.56, 0.8], [0.56, 0.8], [0.56, 0.94], [-0.56, 0.94]], { off: 0.003, nu: 1, nv: 1 }));
    P.add('trim', sweep([[-2.34, 0.4], [-2.5, 0.4], [-2.51, 0.6], [-2.34, 0.62]], 'x', -1.0, 1.0));
    P.box('plate', 0.5, 0.12, 0.02, [0, 0.87, -2.43]);
    // Running boards and roof rails.
    P.pair('trim', Box(0.14, 0.04, 1.9), 0.98, 0.44, -0.12);
    P.pair('trim', Box(0.04, 0.04, 2.3), 0.76, 1.96, -0.85);
    gap(P, S, 'side', [[doors[1], 1.14], [doors[1] - 0.02, 0.5]], { step: hi ? 0.05 : 1 });
    gap(P, S, 'side', [[bp[0] + 0.04, 1.15], [bp[0] + 0.02, 0.5]], { step: hi ? 0.05 : 1 });
    gap(P, S, 'side', [[doors[0], 1.15], [doors[0], 0.9]], { step: hi ? 0.05 : 1 });
    mirrors(P, hi, { x: 1.06, y: 1.32, z: 1.2, w: 0.14, h: 0.18, d: 0.12, base: 0.9 });
    const zb = -0.02, yb = C.topY(zb, 0.6) - 0.004;
    const siren = lightbar(P, hi, { y: yb, z: zb, w: 1.36, d: 0.32 });
    if (hi) {
      gap(P, S, 'top', [[0.7, 1.4], [0.68, 2.36]]);
      gap(P, S, 'rear', [[-0.84, 0.72], [0.84, 0.72]], { mirror: false });
      for (const z of [0.52, -0.72]) P.add('chrome', ribbon(S, 'side', [[z, 1.0], [z - 0.16, 1.0]], 0.03, { off: 0.004, mirror: true, col: 0.8 }));
      number(P, S, 'side', 'POLICE', -0.12, 0.8, 0.16);
      dlo(P, C, topF, 1.35, -2.28, { rear: false });
      interior(P, { z: -0.35, top: 1.8, dashZ: 0.75, dashY: 1.25, seat: [1.4, 1.4, 1.5] });
      P.add('trim', new THREE.BoxGeometry(1.6, 0.5, 0.02), [0, 1.5, -0.65], undefined, 0.5);
      P.add('chrome', new THREE.CylinderGeometry(0.06, 0.05, 0.15, 12), [0.98, 1.4, 1.2], [PI / 2, 0, 0]);
      for (const x of [0.3, -0.3]) P.add('trim', new THREE.CylinderGeometry(0.004, 0.006, 0.5, 4), [x, 2.2, -1.6]);
    }
    return {
      dims: { length: 5.0, width: W, height: 2.1, wheelRadius: r, wheelBase: wb, track: 1.7 },
      wheels: { r, w: 0.27, track: 1.7, zF: a, zR: -a, rimFrac: 0.6, spokes: 0, type: 'steel', rimMat: 'rimDark' },
      head: [0, 0.92, 2.5], exhausts: [[0.6, 0.36, -2.4]], siren,
    };
  },
};

const partsCache = new Map();
function getParts(kind, lod, variant) {
  const key = kind + '|' + lod + '|' + JSON.stringify(variant);
  if (partsCache.has(key)) return partsCache.get(key);
  const P = new Parts(lod);
  const layout = SPECS[kind](P, lod, variant);
  const res = { geoms: P.build(), layout };
  partsCache.set(key, res);
  return res;
}

// ── siren ─────────────────────────────────────────────────────────
const POLICE_KINDS = new Set(['police', 'policeSuv']);
const POLICE_BLACK = 0x0c0d10, POLICE_WHITE = 0xf4f3ee;
// Red/blue levels (0..1) for a siren mode at time t (s). 'flash' is an
// alternating quad-flash: a 0.8 s cycle, red bursting four times in the
// first half and blue in the second (2.5 bursts a second).
function sirenLevels(mode, t) {
  if (mode === true || mode === 'flash') {
    const ph = (((t * 1.25) % 1) + 1) % 1, burst = (ph * 8) % 1 < 0.55 ? 1 : 0;
    return ph < 0.5 ? [burst, 0] : [0, burst];
  }
  if (mode === 'steady') return [0.55, 0.55];
  if (mode === 'disabled') return [0.3, 0];
  return [0, 0];
}
// Additive glow billboards (one quad per spot, one draw call per car). The
// vertex shader turns each quad to the camera, pulls it towards the camera
// so the car's own roof doesn't cut it (further when far away, where the
// road would otherwise hide its lower half at grazing angles), and never
// lets it shrink below a minimum screen size, so a unit still reads as
// police a few hundred metres away.
function glowGeometry(spots) {
  const pos = [], corner = [], blue = [], size = [], idx = [];
  spots.forEach((s, i) => {
    for (const c of [[-1, -1], [1, -1], [1, 1], [-1, 1]]) { pos.push(...s.p); corner.push(...c); blue.push(s.blue); size.push(s.size); }
    idx.push(i * 4, i * 4 + 1, i * 4 + 2, i * 4, i * 4 + 2, i * 4 + 3);
  });
  const g = new THREE.BufferGeometry();
  g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
  g.setAttribute('corner', new THREE.Float32BufferAttribute(corner, 2));
  g.setAttribute('aBlue', new THREE.Float32BufferAttribute(blue, 1));
  g.setAttribute('aSize', new THREE.Float32BufferAttribute(size, 1));
  g.setIndex(idx);
  g.computeBoundingSphere();
  g.boundingSphere.radius += Math.max(...spots.map((s) => s.size));
  return g;
}
function glowMaterial() {
  return new THREE.ShaderMaterial({
    uniforms: { map: { value: glowTexture() }, uRed: { value: new THREE.Color(0, 0, 0) }, uBlue: { value: new THREE.Color(0, 0, 0) }, uMin: { value: 0.02 } },
    vertexShader: `
      attribute vec2 corner;
      attribute float aBlue;
      attribute float aSize;
      uniform float uMin;
      varying vec2 vUv;
      varying float vBlue;
      varying float vFar;
      void main() {
        vUv = corner * 0.5 + 0.5;
        vBlue = aBlue;
        vec4 mv = modelViewMatrix * vec4(position, 1.0);
        float d = length(mv.xyz);
        mv.xyz *= max(0.05, d - min(1.0 + d * d * 0.0015, d * 0.5)) / d;
        vec4 clip = projectionMatrix * mv;
        vec2 hs = vec2(projectionMatrix[0][0], projectionMatrix[1][1]) * (0.5 * aSize) / clip.w;
        float grow = max(1.0, uMin / hs.y);
        vFar = clamp((grow - 1.0) / 1.5, 0.0, 1.0);
        clip.xy += corner * hs * grow * clip.w;
        gl_Position = clip;
      }`,
    fragmentShader: `
      uniform sampler2D map;
      uniform vec3 uRed;
      uniform vec3 uBlue;
      varying vec2 vUv;
      varying float vBlue;
      varying float vFar;
      void main() {
        // Held at its minimum size far away: a harder, brighter core.
        float a = pow(texture2D(map, vUv).a, 2.0 - vFar);
        gl_FragColor = vec4(mix(uRed, uBlue, vBlue) * a * (1.0 + 1.5 * vFar), 1.0);
        #include <tonemapping_fragment>
        #include <colorspace_fragment>
      }`,
    transparent: true, depthWrite: false, blending: THREE.AdditiveBlending,
  });
}

// ── public ────────────────────────────────────────────────────────
export const VEHICLE_KINDS = Object.keys(SPECS);

// Paint finish per racer: metallic flake under clearcoat, a solid gloss for
// the muscle car and rally car, a pearl sheen on the supercar.
const FINISH = {
  sports: { metalness: 0.5, roughness: 0.36 },
  muscle: { metalness: 0.15, roughness: 0.28 },
  super: { metalness: 0.45, roughness: 0.3, sheen: 0.6 },
  rally: { metalness: 0.1, roughness: 0.3 },
  electric: { metalness: 0.35, roughness: 0.36 },
  police: { metalness: 0.2, roughness: 0.3 },
};

export function buildVehicle(kind, opts = {}) {
  if (!SPECS[kind]) throw new Error('unknown vehicle kind ' + kind);
  const SH = shared();
  const racer = RACERS.has(kind);
  const lod = opts.lod ?? (racer ? 'high' : 'low');
  const hi = lod === 'high';
  const M = hi ? SH.hi : SH.lo;
  const seed = Math.abs(opts.seed ?? 0);
  // Police: the patrol kinds, or a racer body in police livery (interceptor).
  const livery = opts.livery === 'police' && (kind === 'muscle' || kind === 'sports') ? 'police' : undefined;
  const police = POLICE_KINDS.has(kind) || !!livery;
  const color = opts.color ?? (police ? POLICE_BLACK : kind === 'tractor' ? [0x9a3b2b, 0x3f6b3a, 0x8f7a3a][seed % 3] : TRAFFIC_COLORS[seed % TRAFFIC_COLORS.length]);
  const variant = { stripes: opts.stripes ?? (!livery && (kind === 'muscle' || kind === 'rally')), spoiler: opts.spoiler ?? !livery, livery };
  const { geoms, layout } = getParts(kind, lod, variant);

  // Per-instance materials.
  const owned = [];
  let paint;
  if (hi) {
    const f = (police ? FINISH.police : FINISH[kind]) || { metalness: 0.4, roughness: 0.4 };
    paint = new THREE.MeshPhysicalMaterial({ color, metalness: f.metalness, roughness: f.roughness, clearcoat: 1, clearcoatRoughness: 0.09, vertexColors: true });
    if (f.sheen) { paint.sheen = f.sheen; paint.sheenRoughness = 0.35; paint.sheenColor = new THREE.Color(color).lerp(new THREE.Color(0xffffff), 0.6); }
    owned.push(paint);
  } else {
    paint = lowPaint(color, kind === 'tractor' ? 0.85 : 0.5);
  }
  const head = lightMat({ color: hi ? 0x9aa0a8 : 0xd8d8d0, metalness: hi ? 0.85 : 0, emissive: 0xfff1d6, emissiveIntensity: 0.3, roughness: hi ? 0.12 : 0.2 }, hi);
  const tail = lightMat({ color: hi ? 0x5c0707 : 0x4a0606, metalness: hi ? 0.3 : 0, emissive: 0xff1712, emissiveIntensity: 0.3, roughness: hi ? 0.12 : 0.3 }, hi);
  const rev = lightMat({ color: 0x9a9a9a, emissive: 0xffffff, emissiveIntensity: 0, roughness: 0.3 }, hi);
  owned.push(head, tail, rev);
  // Light accents (the electric car's blades), which flare under boost.
  const accent = geoms.accent ? new THREE.MeshStandardMaterial({ color: 0x0b2a33, emissive: 0x3fe4ff, emissiveIntensity: 1.4, roughness: 0.3, vertexColors: hi }) : null;
  if (accent) owned.push(accent);
  const c = new THREE.Color(color);
  const stripeColor = opts.stripeColor ?? (police ? POLICE_WHITE : c.r + c.g + c.b > 2.4 ? 0x151515 : 0xf1efe8);
  // Siren lenses (police only): per instance, like the head/tail lights.
  const lightRed = geoms.lightRed ? lightMat({ color: hi ? 0x7a0c0c : 0x5a0808, metalness: hi ? 0.1 : 0, emissive: 0xff1a0c, emissiveIntensity: 0, roughness: 0.15 }, hi) : null;
  const lightBlue = geoms.lightBlue ? lightMat({ color: hi ? 0x0c1c8a : 0x0a1668, metalness: hi ? 0.1 : 0, emissive: 0x1840ff, emissiveIntensity: 0, roughness: 0.15 }, hi) : null;
  if (lightRed) owned.push(lightRed, lightBlue);
  const matFor = {
    paint, head, tail, rev, accent, lightRed, lightBlue,
    glass: M.glass, trim: M.trim, chrome: M.chrome, plate: M.plate, cargo: M.cargo, seat: M.seat, carbon: M.carbon,
    stripe: stripeMat(stripeColor, hi),
  };

  const root = new THREE.Group();
  root.name = 'vehicle:' + kind;
  const body = new THREE.Group();
  body.name = 'body';
  root.add(body);
  for (const [bucket, geom] of Object.entries(geoms)) {
    const mesh = new THREE.Mesh(geom, matFor[bucket] ?? M.trim);
    mesh.castShadow = !['head', 'tail', 'rev', 'accent', 'glass', 'lightRed', 'lightBlue'].includes(bucket);
    mesh.name = bucket;
    // Transparent glass after the body so the interior shows through it.
    if (bucket === 'glass' && hi) mesh.renderOrder = 1;
    body.add(mesh);
  }

  // Wheels: [fl, fr, rl, rr]; left = +X. Front wheels hang off steer pivots.
  const wl = layout.wheels;
  const rimMat = wl.rimMat ? M[wl.rimMat]
    : opts.rim === 'dark' ? M.rimDark
    : racer ? (seed % 2 ? M.rimDark : M.rim)
    : M.rimSteel;
  const wheelMats = [M.tire, rimMat, M.brake];
  const style = hi ? wl : { ...wl, type: racer ? 'racer' : wl.type };
  const makeWheel = (r, w, sx) => {
    const wheel = new THREE.Object3D();
    wheel.name = 'wheel';
    wheel.userData.radius = r;
    const mesh = new THREE.Mesh(wheelGeometry(r, w, lod, style), wheelMats);
    mesh.castShadow = true;
    if (sx < 0) mesh.rotation.y = PI; // rim face outboard on the right side
    wheel.add(mesh);
    return wheel;
  };
  // Calipers sit on the (non-rotating) hub carriers: steer pivots at the
  // front, fixed points at the rear.
  const calMat = SH.caliper[wl.caliper ?? 0];
  const addCaliper = (parent, r, w, sx) => {
    const cal = new THREE.Mesh(caliperGeom(r * wl.rimFrac), calMat);
    cal.position.set(sx * -w * 0.09, 0, 0);
    if (sx < 0) cal.rotation.y = PI;
    parent.add(cal);
  };
  const front = wl.front ?? { r: wl.r, w: wl.w, track: wl.track };
  const wheels = [], steerPivots = [];
  for (const sx of [1, -1]) {
    const pivot = new THREE.Object3D();
    pivot.name = 'steerPivot';
    pivot.position.set((sx * front.track) / 2, front.r, wl.zF);
    const wheel = makeWheel(front.r, front.w, sx);
    pivot.add(wheel);
    if (hi && racer) addCaliper(pivot, front.r, front.w, sx);
    root.add(pivot);
    steerPivots.push(pivot);
    wheels.push(wheel);
  }
  const rearW = wl.rearW ?? wl.w;
  for (const sx of [1, -1]) {
    const wheel = makeWheel(wl.r, rearW, sx);
    wheel.position.set((sx * wl.track) / 2, wl.r, wl.zR);
    root.add(wheel);
    wheels.push(wheel);
    if (hi && racer) {
      const hub = new THREE.Object3D();
      hub.position.copy(wheel.position);
      root.add(hub);
      addCaliper(hub, wl.r, rearW, sx);
    }
  }

  const headlightAnchor = new THREE.Object3D();
  headlightAnchor.name = 'headlightAnchor';
  headlightAnchor.position.fromArray(layout.head);
  body.add(headlightAnchor);

  let brake = 0, lights = 0;
  const updateTail = () => {
    const base = 0.3 + lights * 0.9;
    tail.emissiveIntensity = base + (4 - base) * brake;
  };

  const handle = {
    root, body, wheels, steerPivots,
    kind,
    dims: { ...layout.dims },
    exhausts: layout.exhausts.map((p) => new THREE.Vector3().fromArray(p)),
    headlightAnchor,
    paint,
    setBrake(v) { brake = Math.min(1, Math.max(0, v)); updateTail(); },
    setHeadlights(v) {
      lights = Math.min(1, Math.max(0, v));
      head.emissiveIntensity = 0.3 + lights * 2.7;
      updateTail();
    },
    setReverse(on) { rev.emissiveIntensity = on ? 2.5 : 0; },
    setBoost(v) { if (accent) accent.emissiveIntensity = 1.4 + 5 * Math.min(1, Math.max(0, v)); },
    dispose() {
      for (const m of owned) m.dispose();
      root.removeFromParent();
    },
  };

  // Siren: lens emission, glow billboards and the anchor for the game's
  // shared flash light, all driven by setSiren(mode, t) with mode 'off' |
  // 'flash' | 'steady' | 'disabled' (true/false mean flash/off).
  const siren = layout.siren;
  if (siren) {
    siren.geo ||= glowGeometry(siren.glows);
    const glowMat = glowMaterial();
    owned.push(glowMat);
    const sirenGlow = new THREE.Mesh(siren.geo, glowMat);
    sirenGlow.name = 'sirenGlow';
    sirenGlow.renderOrder = 2;
    body.add(sirenGlow);
    const sirenAnchor = new THREE.Object3D();
    sirenAnchor.name = 'sirenAnchor';
    sirenAnchor.position.fromArray(siren.anchor);
    body.add(sirenAnchor);
    let lr = 0, lb = 0;
    Object.assign(handle, {
      sirenGlow, sirenAnchor,
      setSiren(mode, t = 0) {
        [lr, lb] = sirenLevels(mode, t);
        lightRed.emissiveIntensity = lr * 6;
        lightBlue.emissiveIntensity = lb * 8;
        glowMat.uniforms.uRed.value.setRGB(2.4 * lr, 0.12 * lr, 0.05 * lr);
        glowMat.uniforms.uBlue.value.setRGB(0.1 * lb, 0.35 * lb, 3.2 * lb);
        sirenGlow.visible = lr + lb > 0;
      },
      sirenColor() { return { r: lr, b: lb }; },
    });
    handle.setSiren('off');
  }
  return handle;
}

// Triangle count of a built vehicle (budget checks / debugging).
export function triangleCount(obj) {
  let tris = 0;
  obj.traverse((o) => {
    if (o.isMesh) {
      const g = o.geometry;
      tris += (g.index ? g.index.count : g.attributes.position.count) / 3;
    }
  });
  return tris;
}
