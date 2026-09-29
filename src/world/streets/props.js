import * as THREE from 'three';
import { mergeGeometries } from 'three/addons/utils/BufferGeometryUtils.js';
import { staticMesh } from '../city/geom.js';
import { glowTexture } from '../textures.js';
import { VENDING, STALL, POSTER, AWNING_C, S_TILE, puddleTexture } from './textures.js';

// Street furniture and effects for Downtown Streets. Everything small is
// appended into Streets' chunked GeoBuilders (one draw call per chunk and
// material, however many hydrants), and the few animated things (steam,
// neon flicker) run in shaders off a single time uniform.

// ── Material patches ─────────────────────────────────────────────
// A faint emissive proportional to albedo: the city's bounce light. Without
// it anything a street lamp doesn't hit renders flat black at night.
export function ambientPatch(mat, rgb, key) {
  mat.onBeforeCompile = (shader) => {
    shader.fragmentShader = shader.fragmentShader.replace('#include <emissivemap_fragment>',
      `#include <emissivemap_fragment>\ntotalEmissiveRadiance += diffuseColor.rgb * vec3(${rgb.map((v) => v.toFixed(4)).join(', ')});`);
  };
  mat.customProgramCacheKey = () => 'streets-amb-' + key;
  return mat;
}

// Neon tubes: per-sign data (seed, mode) in `ndata`. Mode 0 hums steadily,
// 1 buzzes and drops out, 2 switches on and off, 3 has a dying tube that
// stutters at a lower level.
export function neonFlicker(mat, time, key) {
  mat.onBeforeCompile = (shader) => {
    shader.uniforms.uNTime = time;
    shader.vertexShader = shader.vertexShader
      .replace('#include <common>', '#include <common>\nattribute vec3 ndata;\nvarying vec3 vND;')
      .replace('#include <uv_vertex>', '#include <uv_vertex>\nvND = ndata;');
    shader.fragmentShader = shader.fragmentShader
      .replace('#include <common>', `#include <common>
uniform float uNTime;
varying vec3 vND;
float nHash(float p) { return fract(sin(p * 91.345) * 47453.5453); }`)
      .replace('#include <opaque_fragment>', `{
  float t = uNTime, sd = vND.x * 97.0, m = vND.y;
  float k = 0.94 + 0.06 * sin(t * 60.0 + sd);
  if (m > 0.5 && m < 1.5) k *= nHash(floor(t * 13.0) + sd) < 0.18 ? 0.12 : 1.0;
  else if (m > 1.5 && m < 2.5) k *= fract(t * 0.35 + vND.x) < 0.72 ? 1.0 : 0.06;
  else if (m > 2.5) k *= 0.35 + 0.65 * step(0.45, nHash(floor(t * 7.0) + sd));
  outgoingLight *= k;
}
#include <opaque_fragment>`);
  };
  mat.customProgramCacheKey = () => 'streets-neon-' + key;
  return mat;
}

// GeoBuilder writes its data channel as `color`; rename it for the shaders
// that read it as data (so three doesn't switch on vertex colours).
export function emitData(geo, name) {
  const c = geo.getAttribute('color');
  if (c) { geo.deleteAttribute('color'); geo.setAttribute(name, c); }
  return geo;
}

// ── Geometry helpers ─────────────────────────────────────────────
const _m = new THREE.Matrix4(), _n3 = new THREE.Matrix3(), _v = new THREE.Vector3();

// Append any geometry (placed by matrix m) into a GeoBuilder with one colour.
export function addGeo(B, geo, m, col) {
  const g = geo.index ? geo.toNonIndexed() : geo;
  const P = g.getAttribute('position'), N = g.getAttribute('normal');
  _n3.getNormalMatrix(m);
  for (let i = 0; i < P.count; i++) {
    _v.fromBufferAttribute(P, i).applyMatrix4(m);
    B.pos.push(_v.x, _v.y, _v.z);
    _v.fromBufferAttribute(N, i).applyMatrix3(_n3).normalize();
    B.nor.push(_v.x, _v.y, _v.z);
    B.uv.push(0, 0);
    if (B.col) B.col.push(col[0], col[1], col[2]);
    if (B.cell) B.cell.push(0);
  }
}

// A flat quad facing outward normal (nx, nz), centred at (cx, cz), spanning
// w along the wall and y0..y1, with atlas UVs that read left to right.
export function facing(B, cx, cz, tx, tz, nx, nz, w, y0, y1, cell, col, uv = [0, 0, 1, 1]) {
  if (nx * -tz + nz * tx < 0) { tx = -tx; tz = -tz; }
  const ax = cx - tx * w / 2, az = cz - tz * w / 2, bx = cx + tx * w / 2, bz = cz + tz * w / 2;
  B.quad([ax, y0, az], [bx, y0, bz], [bx, y1, bz], [ax, y1, az], [[uv[0], uv[1]], [uv[2], uv[1]], [uv[2], uv[3]], [uv[0], uv[3]]], col, cell);
}

const box = (w, h, d, y = 0) => { const g = new THREE.BoxGeometry(w, h, d); g.translate(0, y + h / 2, 0); return g; };
const cyl = (r0, r1, h, seg, y = 0) => { const g = new THREE.CylinderGeometry(r0, r1, h, seg); g.translate(0, y + h / 2, 0); return g; };
const merge = (list) => mergeGeometries(list.map((g) => (g.index ? g.toNonIndexed() : g)).map((g) => { for (const a of Object.keys(g.attributes)) if (a !== 'position' && a !== 'normal') g.deleteAttribute(a); return g; }));

// Small kit, built once: each is [geometry, colour] parts.
let KIT = null;
function kit() {
  if (KIT) return KIT;
  KIT = {
    hydrant: [
      [merge([cyl(0.15, 0.17, 0.55, 6, 0.05), new THREE.ConeGeometry(0.16, 0.14, 6).translate(0, 0.67, 0)]), null],
      [merge([box(0.46, 0.1, 0.1, 0.36), cyl(0.2, 0.2, 0.06, 6, 0)]), [0.55, 0.55, 0.52]],
    ],
    bin: [
      [cyl(0.27, 0.24, 0.85, 8), [0.12, 0.22, 0.16]],
      [merge([cyl(0.3, 0.3, 0.08, 8, 0.85), new THREE.ConeGeometry(0.29, 0.16, 8).translate(0, 1.01, 0)]), [0.18, 0.3, 0.22]],
    ],
    news: [
      [box(0.5, 1.0, 0.45), null],
      [box(0.44, 0.3, 0.02, 0.55).translate(0, 0, 0.23), [0.8, 0.8, 0.75]],
    ],
    meter: [
      [cyl(0.04, 0.04, 1.1, 6), [0.3, 0.32, 0.34]],
      [box(0.2, 0.32, 0.16, 1.1), [0.5, 0.52, 0.55]],
    ],
    bench: [
      [merge([box(1.8, 0.06, 0.45, 0.42), box(1.8, 0.4, 0.05, 0.5).translate(0, 0, -0.22)]), [0.45, 0.3, 0.18]],
      [merge([box(0.06, 0.42, 0.4).translate(-0.8, 0, 0), box(0.06, 0.42, 0.4).translate(0.8, 0, 0)]), [0.18, 0.18, 0.2]],
    ],
    planter: [
      [box(2.2, 0.6, 1.2), [0.42, 0.4, 0.38]],
      [merge([new THREE.IcosahedronGeometry(0.55, 0).scale(1.6, 0.6, 0.9).translate(0, 0.75, 0)]), [0.12, 0.24, 0.1]],
    ],
    bollard: [[cyl(0.1, 0.12, 0.9, 6), [0.22, 0.22, 0.24]]],
    cone: [[cyl(0.03, 0.18, 0.7, 8), [1.0, 0.35, 0.05]], [cyl(0.1, 0.13, 0.12, 8, 0.3), [0.95, 0.95, 0.95]], [box(0.4, 0.04, 0.4), [0.1, 0.1, 0.1]]],
  };
  return KIT;
}

export function putKit(B, name, m, tint) {
  for (const [geo, col] of kit()[name]) addGeo(B, geo, m, col || tint);
}

const trs = (x, y, z, yaw = 0, s = 1) => _m.clone().compose(new THREE.Vector3(x, y, z), new THREE.Quaternion().setFromEuler(new THREE.Euler(0, yaw, 0)), new THREE.Vector3(s, s, s));
// three.js yaw that turns local +Z onto the horizontal direction (dx, dz).
const yawZ = (dx, dz) => Math.atan2(dx, dz);

// ── Kerbside furniture ───────────────────────────────────────────
// Hydrants, bins, newspaper boxes and parking meters along the kerbs of
// the blocks near the route; district decides the mix.
export function kerbFurniture(S) {
  const { PX, PZ, HW } = S.G;
  const rng = S.rng;
  const seen = new Set();
  for (const b of S.blocks.values()) {
    if (b.tier !== 0) continue;
    const x0 = b.i * PX + HW + 0.7, x1 = (b.i + 1) * PX - HW - 0.7, z0 = b.j * PZ + HW + 0.7, z1 = (b.j + 1) * PZ - HW - 0.7;
    const put = (x, z, nx, nz) => {
      const key = Math.round(x / 2) + ',' + Math.round(z / 2);
      if (seen.has(key) || !S.onKerb(x, z)) return;
      seen.add(key);
      const y = S.ground(x, z) + 0.15;
      const B = S.bPlain.at(x, z);
      const r = rng(), yaw = yawZ(nx, nz);
      if (r < 0.2) putKit(B, 'hydrant', trs(x, y, z, yaw), b.district === 1 ? [0.85, 0.82, 0.75] : b.district === 2 ? [0.75, 0.62, 0.1] : [0.7, 0.1, 0.08]);
      else if (r < 0.55) putKit(B, 'bin', trs(x, y, z, rng() * 6), null);
      else if (r < 0.7 && b.district !== 1) {
        const cols = [[0.1, 0.25, 0.55], [0.7, 0.1, 0.1], [0.85, 0.7, 0.1]];
        for (let k = 0; k < 1 + Math.floor(rng() * 3); k++) {
          const px = x - nz * k * 0.55, pz = z + nx * k * 0.55;
          putKit(B, 'news', trs(px, y, pz, yaw), cols[Math.floor(rng() * 3)]);
        }
      } else if (b.district === 1) {
        for (let k = 0; k < 3; k++) putKit(B, 'meter', trs(x - nz * k * 6, S.ground(x - nz * k * 6, z + nx * k * 6) + 0.15, z + nx * k * 6, yaw), null);
      } else putKit(B, 'bollard', trs(x, y, z), null);
    };
    // Along each kerb, facing the road (n is toward the road).
    // Only the kerbs you can see from the route.
    const near = (x, z) => S.routeDist(x, z) < 40;
    for (let x = x0 + 9; x < x1 - 9; x += 18 + rng() * 18) { if (near(x, z0)) put(x, z0, 0, -1); if (near(x + 5, z1)) put(x + 5, z1, 0, 1); }
    for (let z = z0 + 9; z < z1 - 9; z += 18 + rng() * 18) { if (near(x0, z)) put(x0, z, -1, 0); if (near(x1, z + 5)) put(x1, z + 5, 1, 0); }
  }
}

// ── Things against shop fronts ───────────────────────────────────
// fronts: {fa, tx, tz, nx, nz, L, y (street floor), district, shop}
export function frontProps(S) {
  const rng = S.rng;
  for (const f of S.fronts) {
    if (f.tier !== 0) continue;
    // Vending machines in pairs beside Neon District doors.
    if (f.district === 0 && rng() < 0.3 && f.L > 5) {
      const u = rng() < 0.5 ? 0.9 : f.L - 0.9 - 1.1;
      const n = 1 + (rng() < 0.5 ? 1 : 0);
      for (let k = 0; k < n; k++) {
        const uu = u + k * 1.1;
        const cx = f.fa[0] + f.tx * (uu + 0.5) + f.nx * 0.42, cz = f.fa[1] + f.tz * (uu + 0.5) + f.nz * 0.42;
        if (!S.onKerb(cx, cz)) continue;
        const y = S.ground(cx, cz) + 0.15;
        S.bPlain.at(cx, cz).box(cx, y, cz, 1.0, 1.9, 0.8, Math.atan2(f.tz, f.tx), { color: [0.75, 0.76, 0.78] });
        facing(S.bStreet.at(cx, cz), cx + f.nx * 0.41, cz + f.nz * 0.41, f.tx, f.tz, f.nx, f.nz, 0.96, y + 0.02, y + 1.88, VENDING + 16 * S.seed(), [1, 1, 1]);
        S.spill.push({ x: cx + f.nx * 1.3, z: cz + f.nz * 1.3, r: 1.8, col: [0.25, 0.3, 0.36], tx: f.tx, tz: f.tz });
      }
    }
    // Food stalls with awnings and steam, a few per Neon District street.
    if (f.district === 0 && rng() < 0.1 && f.L > 7) stall(S, f);
    // Warm light from lit shop windows spilling across the pavement.
    if (f.shop !== undefined && f.lit) {
      const cx = f.fa[0] + f.tx * f.L / 2 + f.nx * 2.2, cz = f.fa[1] + f.tz * f.L / 2 + f.nz * 2.2;
      S.spill.push({ x: cx, z: cz, r: Math.min(f.L * 0.45, 5), col: f.lit, tx: f.tx, tz: f.tz, long: true });
    }
  }
}

function stall(S, f) {
  const u = f.L * 0.5;
  const nx = f.nx, nz = f.nz;
  const cx = f.fa[0] + f.tx * u + nx * 2.3, cz = f.fa[1] + f.tz * u + nz * 2.3;
  if (!S.onKerb(cx + nx * 1.2, cz + nz * 1.2)) return;
  const y = S.ground(cx, cz) + 0.15;
  const yaw = Math.atan2(f.tz, f.tx);
  const P = S.bPlain.at(cx, cz), St = S.bStreet.at(cx, cz);
  // Counter and back posts; the lit front faces the road.
  P.box(cx, y, cz, 3, 1.05, 1.3, yaw, { color: [0.36, 0.24, 0.14] });
  facing(St, cx + nx * 0.66, cz + nz * 0.66, f.tx, f.tz, nx, nz, 3, y, y + 2.4, STALL + 16 * S.seed(), [1, 1, 1], [0, 0, 1, 1]);
  for (const s of [-1.45, 1.45]) P.box(cx + f.tx * s - nx * 0.6, y, cz + f.tz * s - nz * 0.6, 0.08, 2.5, 0.08, yaw, { color: [0.2, 0.15, 0.1] });
  // Sloped striped roof.
  const col = [[0.9, 0.2, 0.15], [0.95, 0.75, 0.2], [0.2, 0.45, 0.85]][Math.floor(S.rng() * 3)];
  const A = [cx - f.tx * 1.7 - nx * 0.7, y + 2.6, cz - f.tz * 1.7 - nz * 0.7], Bq = [cx + f.tx * 1.7 - nx * 0.7, y + 2.6, cz + f.tz * 1.7 - nz * 0.7];
  const C = [Bq[0] + nx * 2.0, y + 2.25, Bq[2] + nz * 2.0], D = [A[0] + nx * 2.0, y + 2.25, A[2] + nz * 2.0];
  const uvs = [[0, 0], [3.4 / S_TILE[AWNING_C][0], 0], [3.4 / S_TILE[AWNING_C][0], 1], [0, 1]];
  const cell = AWNING_C + 16 * S.seed();
  St.quad(A, D, C, Bq, [uvs[0], uvs[3], uvs[2], uvs[1]], col, cell);
  St.quad(A, Bq, C, D, uvs, col.map((v) => v * 0.6), cell);
  // Paper lanterns along the roof edge and a steam plume from the pots.
  const G = S.bGlow.at(cx, cz);
  for (const s of [-1.1, 0, 1.1]) {
    const lx = cx + f.tx * s + nx * 1.25, lz = cz + f.tz * s + nz * 1.25;
    G.box(lx, y + 1.75, lz, 0.32, 0.42, 0.32, yaw, { color: [3.2, 0.7, 0.3] });
  }
  S.steam.push({ x: cx + nx * 0.2, y: y + 1.1, z: cz + nz * 0.2, rate: 1 });
  S.spill.push({ x: cx + nx * 2.2, z: cz + nz * 2.2, r: 3, col: [0.5, 0.28, 0.1], tx: f.tx, tz: f.tz });
  S.signLights.push({ x: cx + nx * 1.2, y: y + 1.8, z: cz + nz * 1.2, col: '#ff8a3a' });
}

// ── Bus shelters ─────────────────────────────────────────────────
// Glass box, lit poster at one end, a bench; on the kerb facing the road.
export function busShelters(S) {
  const t = S.track, { HW } = S.G;
  const f = {};
  for (let s = t.startS + 260; s < t.length - 150; s += 170 + S.rng() * 120) {
    t.frame(s, f);
    if (f.zone === 0 || Math.abs(f.kappa) > 0.002) continue;
    const side = S.rng() < 0.5 ? 1 : -1;
    const lat = side * (HW + 2.0);
    const x = f.x + f.rx * lat, z = f.z + f.rz * lat;
    if (!S.onKerb(x, z) || !S.onKerb(x + f.fx * 3, z + f.fz * 3) || !S.onKerb(x - f.fx * 3, z - f.fz * 3)) continue;
    if (t.distanceToRoad(x, z, 20).d < HW + 1.2) continue;
    const y = S.ground(x, z) + 0.15;
    const nx = -f.rx * side, nz = -f.rz * side; // toward the road
    const yaw = Math.atan2(f.fz, f.fx);
    const P = S.bPlain.at(x, z);
    const L = 4.2;
    // Frame posts, roof, back glass (dark tinted).
    for (const u of [-L / 2, L / 2]) for (const d of [-0.7, 0.7]) P.box(x + f.fx * u + nx * d, y, z + f.fz * u + nz * d, 0.08, 2.4, 0.08, yaw, { color: [0.3, 0.32, 0.35] });
    P.box(x, y + 2.4, z, L + 0.3, 0.12, 1.7, yaw, { color: [0.25, 0.27, 0.3] });
    facing(P, x - nx * 0.7, z - nz * 0.7, f.fx, f.fz, nx, nz, L, y + 0.3, y + 2.2, 0, [0.12, 0.16, 0.2]);
    facing(P, x - nx * 0.72, z - nz * 0.72, f.fx, f.fz, -nx, -nz, L, y + 0.3, y + 2.2, 0, [0.12, 0.16, 0.2]);
    putKit(P, 'bench', trs(x - nx * 0.35, y, z - nz * 0.35, yawZ(nx, nz)), null);
    // Poster panel at one end, lit both sides, and a light strip under the roof.
    const ex = x + f.fx * L / 2, ez = z + f.fz * L / 2;
    const St = S.bStreet.at(x, z);
    const cell = POSTER + 16 * S.seed();
    facing(St, ex + f.fx * 0.06, ez + f.fz * 0.06, nx, nz, f.fx, f.fz, 1.4, y + 0.2, y + 2.2, cell, [1, 1, 1]);
    facing(St, ex - f.fx * 0.06, ez - f.fz * 0.06, nx, nz, -f.fx, -f.fz, 1.4, y + 0.2, y + 2.2, cell, [1, 1, 1]);
    S.bGlow.at(x, z).box(x, y + 2.33, z, L - 0.4, 0.06, 0.12, yaw, { color: [2.4, 2.6, 3] });
    S.spill.push({ x, z, r: 3.2, col: [0.2, 0.24, 0.3], tx: f.fx, tz: f.fz });
  }
}

// ── Parked cars ──────────────────────────────────────────────────
// A few traffic models baked to vertex-coloured geometry and copied along
// the kerbs of the side streets the race doesn't use.
export async function parkedCars(S) {
  const { PX, PZ, HW } = S.G;
  const t = S.track, rng = S.rng;
  const kinds = ['sedan', 'hatch', 'van', 'pickup'];
  const slots = kinds.map(() => []);
  for (const b of S.blocks.values()) {
    if (b.tier !== 0) continue;
    const x0 = b.i * PX + HW, x1 = (b.i + 1) * PX - HW, z0 = b.j * PZ + HW, z1 = (b.j + 1) * PZ - HW;
    const dens = b.district === 1 ? 0.45 : 0.12;
    const edge = (ax, az, bx, bz, nx, nz) => {
      const L = Math.hypot(bx - ax, bz - az), tx = (bx - ax) / L, tz = (bz - az) / L;
      for (let u = 12; u < L - 12; u += 6.2) {
        if (rng() > dens) continue;
        const x = ax + tx * u + nx * 1.25, z = az + tz * u + nz * 1.25;
        // Only down the side streets you can see from the route.
        const rd = S.routeDist(x, z);
        if (rd > 28 || t.distanceToRoad(x, z, 30).d < HW + 5) continue;
        if (S.onKerb(x, z)) continue;
        const k = Math.floor(rng() * (b.district === 1 ? 2.6 : kinds.length));
        const yaw = Math.atan2(tx, tz) + (rng() < 0.5 ? 0 : Math.PI);
        // Pitch to the hill along the street.
        const y0 = S.ground(x - tx * 1.4, z - tz * 1.4), y1 = S.ground(x + tx * 1.4, z + tz * 1.4);
        slots[k].push({ x, y: (y0 + y1) / 2, z, yaw, rd, pitch: Math.atan2(y1 - y0, 2.8) * (Math.cos(yaw - Math.atan2(tx, tz)) > 0 ? -1 : 1) });
      }
    };
    // Each kerb, the car on the road side of it (n points into the road).
    edge(x0, z0, x1, z0, 0, -1); edge(x0, z1, x1, z1, 0, 1);
    edge(x0, z0, x0, z1, -1, 0); edge(x1, z0, x1, z1, 1, 0);
  }
  let buildVehicle;
  try { ({ buildVehicle } = await import('../../vehicles/CarModel.js')); } catch (e) { return; }
  // Baked into the chunked static geometry (culled per chunk, one draw call
  // each) with the paint colour in the vertex colours.
  const cols = [0xb8bcc2, 0x2b2f36, 0xe8e6e0, 0x7a1f1f, 0x1f3a5f, 0x4a5a3a, 0x8c7a5a, 0x5f6670, 0xc8a040, 0x243048];
  const e = new THREE.Euler(), q = new THREE.Quaternion(), p = new THREE.Vector3(), one = new THREE.Vector3(1, 1, 1), m = new THREE.Matrix4();
  const paint = new THREE.Color();
  kinds.forEach((kind, k) => {
    if (!slots[k].length) return;
    // Full traffic models where you pass close; a low-poly stand-in (a
    // tenth of the triangles) further down the side streets.
    const full = bakeCar(buildVehicle, kind), low = lowCar(kind);
    const n3 = new THREE.Matrix3(), v = new THREE.Vector3();
    for (const s of slots[k]) {
      const geo = s.rd < 16 && full ? full : low;
      const P = geo.getAttribute('position'), N = geo.getAttribute('normal'), C = geo.getAttribute('color');
      e.set(s.pitch, s.yaw, 0, 'YXZ');
      m.compose(p.set(s.x, s.y, s.z), q.setFromEuler(e), one);
      n3.getNormalMatrix(m);
      paint.set(cols[Math.floor(rng() * cols.length)]).convertSRGBToLinear();
      const B = S.bCar.at(s.x, s.z);
      for (let i = 0; i < P.count; i++) {
        v.fromBufferAttribute(P, i).applyMatrix4(m); B.pos.push(v.x, v.y, v.z);
        v.fromBufferAttribute(N, i).applyMatrix3(n3).normalize(); B.nor.push(v.x, v.y, v.z);
        B.uv.push(0, 0);
        // Magenta marks paint (see bakeCar).
        const r = C.getX(i), g = C.getY(i), bb = C.getZ(i);
        if (r > 0.99 && g < 0.01 && bb > 0.99) B.col.push(paint.r, paint.g, paint.b); else B.col.push(r, g, bb);
      }
    }
  });
}

// Low-poly parked car: extruded side profiles for body and cabin, simple
// wheels and lamp blocks, vertex coloured (magenta = paint).
function lowCar(kind) {
  const tall = kind === 'van', bed = kind === 'pickup';
  const prof = (pts, depth, col, y = 0) => {
    const g = new THREE.ExtrudeGeometry(new THREE.Shape(pts.map(([a, b]) => new THREE.Vector2(a, b))), { depth, bevelEnabled: false });
    g.translate(0, y, -depth / 2);
    g.rotateY(-Math.PI / 2); // profile x (length) onto +z
    return [g, col];
  };
  const PAINT = [1, 0, 1], GLASS = [0.06, 0.08, 0.11], DARK = [0.05, 0.05, 0.05];
  const parts = [
    prof([[-2.25, 0.3], [2.25, 0.3], [2.32, 0.62], [2.12, 0.84], [-2.2, 0.88], [-2.3, 0.6]], 1.76, PAINT),
    tall ? prof([[-2.2, 0.86], [1.3, 0.86], [1.9, 1.3], [1.8, 1.95], [-2.2, 1.98]], 1.7, GLASS)
      : bed ? prof([[-0.2, 0.86], [1.1, 0.86], [0.55, 1.5], [-0.2, 1.52]], 1.6, GLASS)
        : prof([[-1.4, 0.86], [0.9, 0.86], [0.25, 1.36], [-1.05, 1.38], [-1.62, 0.9]], 1.52, GLASS),
  ];
  const b = (w, h, d, x, y, z, col) => { const g = new THREE.BoxGeometry(w, h, d); g.translate(x, y, z); return [g, col]; };
  if (tall) parts.push(b(1.74, 0.06, 3.3, 0, 1.98, -0.35, PAINT));
  else if (!bed) parts.push(b(1.46, 0.05, 1.0, 0, 1.39, 0.35, PAINT));
  else parts.push(b(1.7, 0.12, 2.0, 0, 0.9, 1.2, DARK));
  for (const sx of [-0.82, 0.82]) {
    parts.push(b(0.36, 0.1, 0.05, sx * 0.9, 0.7, 2.29, [0.7, 0.7, 0.66]));
    parts.push(b(0.34, 0.12, 0.05, sx * 0.9, 0.72, -2.26, [0.5, 0.03, 0.02]));
  }
  for (const [x, z] of [[-0.8, 1.45], [0.8, 1.45], [-0.8, -1.45], [0.8, -1.45]]) {
    const w = new THREE.CylinderGeometry(0.33, 0.33, 0.24, 8);
    w.rotateZ(Math.PI / 2); w.translate(x, 0.33, z);
    parts.push([w, DARK]);
  }
  const geos = parts.map(([g, c]) => {
    const h = g.index ? g.toNonIndexed() : g;
    for (const a of Object.keys(h.attributes)) if (a !== 'position') h.deleteAttribute(a);
    h.computeVertexNormals();
    const n = h.getAttribute('position').count, col = new Float32Array(n * 3);
    for (let i = 0; i < n; i++) col.set(c, i * 3);
    h.setAttribute('color', new THREE.BufferAttribute(col, 3));
    return h;
  });
  return mergeGeometries(geos);
}

function bakeCar(buildVehicle, kind) {
  let v;
  try { v = buildVehicle(kind, { color: 0xffffff, seed: 3, lod: 'low' }); } catch (e) { return null; }
  v.setHeadlights?.(0);
  v.root.updateMatrixWorld(true);
  const parts = [];
  v.root.traverse((o) => {
    if (!o.isMesh) return;
    let g = o.geometry.clone().applyMatrix4(o.matrixWorld);
    const mats = Array.isArray(o.material) ? o.material : [o.material];
    const groups = Array.isArray(o.material) && g.groups.length ? g.groups : [{ start: 0, count: g.index ? g.index.count : g.getAttribute('position').count, materialIndex: 0 }];
    const idx = g.index;
    g = g.index ? g.toNonIndexed() : g;
    const n = g.getAttribute('position').count;
    const col = new Float32Array(n * 3);
    for (const gr of groups) {
      const m = mats[gr.materialIndex] || mats[0];
      const c = m === v.paint ? new THREE.Color(1, 0, 1) : (m.color ? m.color.clone() : new THREE.Color(0.1, 0.1, 0.1));
      // Tail lamps read as dim red lenses; paint is marked for recolouring.
      if (m.emissive && m.emissive.r > 0.9 && m.emissive.g < 0.2) c.setRGB(0.5, 0.03, 0.02);
      const s0 = idx ? gr.start : gr.start, s1 = Math.min(n, s0 + gr.count);
      for (let i = s0; i < s1; i++) col.set([c.r, c.g, c.b], i * 3);
    }
    for (const a of Object.keys(g.attributes)) if (a !== 'position' && a !== 'normal') g.deleteAttribute(a);
    g.setAttribute('color', new THREE.BufferAttribute(col, 3));
    parts.push(g);
  });
  v.dispose?.();
  return parts.length ? mergeGeometries(parts) : null;
}

// ── Steam ────────────────────────────────────────────────────────
// Soft puffs rising from street vents and stall pots, lit by the street.
// All animation is in the vertex shader off one time uniform.
export function buildSteam(S) {
  if (!S.steam.length) return;
  const N = 10;
  const pos = [], seed = [];
  for (const e of S.steam) for (let k = 0; k < N; k++) { pos.push(e.x, e.y, e.z); seed.push(k / N, Math.random()); }
  const g = new THREE.BufferGeometry();
  g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
  g.setAttribute('aSeed', new THREE.Float32BufferAttribute(seed, 2));
  g.computeBoundingSphere();
  g.boundingSphere.radius += 6;
  const time = { value: 0 };
  const m = new THREE.ShaderMaterial({
    uniforms: { uTime: time, uScale: { value: 600 } },
    vertexShader: `
      attribute vec2 aSeed;
      uniform float uTime, uScale;
      varying float vA;
      varying float vS;
      void main() {
        float life = fract(uTime * 0.28 + aSeed.x + aSeed.y * 0.1);
        vec3 p = position + vec3(sin(aSeed.y * 40.0 + life * 3.0) * 0.5 * life, life * 4.5, cos(aSeed.y * 23.0) * 0.4 * life);
        vec4 mv = modelViewMatrix * vec4(p, 1.0);
        gl_Position = projectionMatrix * mv;
        gl_PointSize = (0.8 + life * 3.2) * uScale / -mv.z;
        // Fade in, thin out as it rises, and never fill the camera.
        vA = smoothstep(0.0, 0.15, life) * (1.0 - life) * smoothstep(2.0, 9.0, -mv.z);
        vS = aSeed.y;
      }`,
    fragmentShader: `
      varying float vA;
      varying float vS;
      void main() {
        vec2 p = gl_PointCoord - 0.5;
        float r2 = dot(p, p) * 4.0;
        // Lumpy soft puff.
        float lump = 0.75 + 0.25 * sin(atan(p.y, p.x) * 3.0 + vS * 20.0);
        float a = max(0.0, 1.0 - r2 / lump);
        a = a * a * vA * 0.16;
        gl_FragColor = vec4(vec3(0.6, 0.58, 0.64) * a, a);
      }`,
    transparent: true, depthWrite: false, blending: THREE.CustomBlending,
    blendSrc: THREE.OneFactor, blendDst: THREE.OneMinusSrcAlphaFactor,
  });
  const pts = new THREE.Points(g, m);
  pts.userData.dynamic = true;
  S.group.add(pts);
  S.world.updaters.push((dt, n, camera) => {
    time.value += dt;
    // Point size in pixels needs the viewport height.
    if (camera) m.uniforms.uScale.value = 0.5 * (S.world.renderer?.domElement?.height || 720) / Math.tan((camera.fov || 60) * Math.PI / 360);
  });
}

// ── Wet road: puddles and light spill ────────────────────────────
// Additive coloured blobs: puddles on the road that catch the nearest sign
// or lamp, and warm pools on the pavement in front of lit windows.
export function buildPuddlesAndSpill(S) {
  const t = S.track, { HW } = S.G;
  const pos = [], uv = [], col = [];
  const quad = (cx, cy, cz, ax, az, a, bx, bz, b, c) => {
    const P = [[cx - ax * a - bx * b, cz - az * a - bz * b], [cx + ax * a - bx * b, cz + az * a - bz * b], [cx + ax * a + bx * b, cz + az * a + bz * b], [cx - ax * a + bx * b, cz - az * a + bz * b]];
    const U = [[0, 0], [1, 0], [1, 1], [0, 1]];
    const cr = (P[1][1] - P[0][1]) * (P[2][0] - P[0][0]) - (P[1][0] - P[0][0]) * (P[2][1] - P[0][1]);
    const order = cr >= 0 ? [0, 1, 2, 0, 2, 3] : [0, 2, 1, 0, 3, 2];
    for (const q of order) { pos.push(P[q][0], cy(P[q][0], P[q][1]), P[q][1]); uv.push(...U[q]); col.push(...c); }
  };
  // Puddles: along the route near the kerbs, tinted by the brightest light nearby.
  const f = {}, tmp = new THREE.Color();
  const lights = S.signLights;
  const rng = S.rng;
  for (let s = 20; s < t.length - 20; s += 9 + rng() * 16) {
    t.frame(s, f);
    const lat = (rng() < 0.5 ? -1 : 1) * (HW - 0.8 - rng() * 2.2);
    const x = f.x + f.rx * lat, z = f.z + f.rz * lat;
    let best = null, bd = 18;
    for (const L of lights) { const d = Math.hypot(L.x - x, L.z - z); if (d < bd) { bd = d; best = L; } }
    const c = best ? tmp.set(best.col) : tmp.setRGB(0.5, 0.45, 0.55);
    const k = (best ? 0.3 * (1 - bd / 22) : 0.05) + 0.04;
    const sz = 0.8 + rng() * 1.6;
    quad(x, (px, pz) => S.surfaceAt(px, pz) + 0.04, z, f.fx, f.fz, sz * (1.2 + rng()), f.rx, f.rz, sz, [c.r * k, c.g * k, c.b * k]);
  }
  const pmat = new THREE.MeshBasicMaterial({ map: puddleTexture(), vertexColors: true, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending, polygonOffset: true, polygonOffsetFactor: -4, polygonOffsetUnits: -4 });
  const mk = (m) => {
    if (!pos.length) return;
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
    g.setAttribute('uv', new THREE.Float32BufferAttribute(uv, 2));
    g.setAttribute('color', new THREE.Float32BufferAttribute(col, 3));
    g.computeBoundingSphere();
    const mesh = staticMesh(g, m, { receive: false });
    mesh.renderOrder = 2;
    S.group.add(mesh);
    pos.length = uv.length = col.length = 0;
  };
  mk(pmat);
  // Spill: soft ellipses on the pavement.
  for (const sp of S.spill) {
    const k = sp.long ? 0.55 : 0.6;
    const a = sp.long ? sp.r * 1.3 : sp.r, b = sp.long ? 2.2 : sp.r;
    quad(sp.x, (px, pz) => S.ground(px, pz) + 0.19, sp.z, sp.tx, sp.tz, a, -sp.tz, sp.tx, b, sp.col.map((v) => v * k));
  }
  mk(new THREE.MeshBasicMaterial({ map: glowTexture(), vertexColors: true, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending, polygonOffset: true, polygonOffsetFactor: -4, polygonOffsetUnits: -4 }));
}

// ── Overhead wires ───────────────────────────────────────────────
export function buildWires(S) {
  if (!S.cables.length) return;
  const g = new THREE.BufferGeometry();
  g.setAttribute('position', new THREE.Float32BufferAttribute(S.cables, 3));
  g.computeBoundingSphere();
  S.group.add(new THREE.LineSegments(g, new THREE.LineBasicMaterial({ color: 0x0c0c0e })));
}

// A sagging wire from a to b, appended as line segments.
export function wire(S, a, b, sag, n = 8) {
  let prev = a;
  for (let q = 1; q <= n; q++) {
    const u = q / n;
    const p = [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u - sag * 4 * u * (1 - u), a[2] + (b[2] - a[2]) * u];
    S.cables.push(...prev, ...p);
    prev = p;
  }
}
