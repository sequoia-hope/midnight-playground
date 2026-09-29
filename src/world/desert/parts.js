import * as THREE from 'three';
import { mergeGeometries, mergeVertices } from 'three/addons/utils/BufferGeometryUtils.js';
import { lerp, mulberry32, makeNoise2D } from '../../util/math.js';
import { rockTexture, detailTexture } from '../textures.js';

// Desert Run geometry kit: plants, rock pinnacles and railway rolling
// stock. Everything is vertex-coloured and merged into one geometry per
// object, so the scenery can instance it.

const V3 = (x, y, z) => new THREE.Vector3(x, y, z);

// Non-indexed, uv-free, flat-shaded unless told otherwise.
export function prep(geo, smooth = false) {
  const g = geo.index ? geo.toNonIndexed() : geo;
  if (g.hasAttribute('uv')) g.deleteAttribute('uv');
  if (!smooth || !g.hasAttribute('normal')) g.computeVertexNormals();
  return g;
}

export function paint(geo, hex, jitter = 0, rng = Math.random) {
  const c = new THREE.Color(hex);
  const n = geo.getAttribute('position').count;
  const a = new Float32Array(n * 3);
  for (let i = 0; i < n; i += 3) {
    const k = 1 + (rng() - 0.5) * jitter;
    for (let j = 0; j < 3 && i + j < n; j++) { a[(i + j) * 3] = c.r * k; a[(i + j) * 3 + 1] = c.g * k; a[(i + j) * 3 + 2] = c.b * k; }
  }
  geo.setAttribute('color', new THREE.BufferAttribute(a, 3));
  return geo;
}

function merge(parts) {
  const g = mergeGeometries(parts, false);
  for (const p of parts) p.dispose();
  g.computeBoundingSphere();
  return g;
}

// A tapered limb from a to b (world-ish local coords).
function limb(a, b, r0, r1, seg = 6) {
  const dir = new THREE.Vector3().subVectors(b, a);
  const len = dir.length();
  const g = new THREE.CylinderGeometry(r1, r0, len, seg, 1, true);
  g.translate(0, len / 2, 0);
  g.applyQuaternion(new THREE.Quaternion().setFromUnitVectors(V3(0, 1, 0), dir.normalize()));
  g.translate(a.x, a.y, a.z);
  return g;
}

// ── Sandstone ────────────────────────────────────────────────────
// Triplanar strata, tinted by vertex and instance colour.
export function sandstoneMaterial(key = 'desert-rock') {
  const m = new THREE.MeshStandardMaterial({ color: 0xffffff, roughness: 0.95, metalness: 0, vertexColors: true });
  const tex = rockTexture();
  m.onBeforeCompile = (sh) => {
    sh.uniforms.tRock = { value: tex };
    sh.uniforms.tDetail = { value: detailTexture() };
    sh.vertexShader = sh.vertexShader
      .replace('#include <common>', '#include <common>\nvarying vec3 vRP;\nvarying vec3 vRN;')
      .replace('#include <fog_vertex>', `#include <fog_vertex>
        vec4 rp_ = vec4(transformed, 1.0);
        vec3 rn_ = objectNormal;
        #ifdef USE_INSTANCING
          rp_ = instanceMatrix * rp_;
          rn_ = mat3(instanceMatrix) * rn_;
        #endif
        vRP = (modelMatrix * rp_).xyz;
        vRN = normalize(mat3(modelMatrix) * rn_);`);
    sh.fragmentShader = sh.fragmentShader
      .replace('#include <common>', '#include <common>\nvarying vec3 vRP;\nvarying vec3 vRN;\nuniform sampler2D tRock;\nuniform sampler2D tDetail;')
      .replace('#include <map_fragment>', `
        vec3 w_ = pow(abs(normalize(vRN)), vec3(3.0));
        w_ /= (w_.x + w_.y + w_.z);
        vec3 a_ = texture2D(tRock, vec2(vRP.z * 0.07, vRP.y * 0.16)).rgb;
        vec3 b_ = texture2D(tRock, vec2(vRP.x * 0.07, vRP.y * 0.16)).rgb;
        vec3 c_ = texture2D(tRock, vRP.xz * 0.09).rgb;
        diffuseColor.rgb *= (a_ * w_.x + b_ * w_.z + c_ * w_.y) * 1.35;
        // Desert varnish: the value noise stretched tall reads as dark
        // streaks washed down steep faces; flats and overhangs stay clean.
        float st_ = texture2D(tDetail, vec2((vRP.x + vRP.z) * 0.11, vRP.y * 0.006)).r;
        float stp_ = 1.0 - abs(normalize(vRN).y);
        diffuseColor.rgb *= 1.0 - 0.45 * smoothstep(0.7, 0.86, st_) * stp_ * stp_;`);
  };
  m.customProgramCacheKey = () => key;
  return m;
}

// Lumpy boulder (unit size), flat underside.
export function boulderGeometry(seed, detail = 0, squash = 0.7, sharp = 0.42) {
  const g = new THREE.IcosahedronGeometry(1, detail);
  const n = makeNoise2D(seed);
  const pos = g.getAttribute('position');
  for (let i = 0; i < pos.count; i++) {
    const x = pos.getX(i), y = pos.getY(i), z = pos.getZ(i);
    const d = n(x * 1.1 + z * 2.3 + 5, y * 1.3 - z * 0.7) * 0.6 + n(x * 2.9 - z * 1.7, y * 2.6 + x) * 0.3;
    const r = 1 + d * sharp;
    pos.setXYZ(i, x * r, Math.max(y * r * squash, -0.3), z * r);
  }
  return paint(prep(g), 0xffffff);
}

// ── Plants ───────────────────────────────────────────────────────
// Joshua tree: a shaggy trunk forking into crooked arms, each ending in a
// spiky rosette. ~6–9 m tall at unit scale.
export function joshuaGeometry(seed) {
  const rng = mulberry32(seed);
  const parts = [];
  const bark = 0x6b5a48, leaf = 0x6f7a3c, dead = 0x5e4e3c;
  const rosette = (at, s) => {
    // A burst of stiff dagger leaves: slim three-sided spikes fanning up
    // and out from the branch tip, over a small dark core.
    const core = new THREE.OctahedronGeometry(0.28 * s, 0);
    core.translate(at.x, at.y + 0.2 * s, at.z);
    parts.push(paint(prep(core), 0x4e5a2c, 0.2, rng));
    for (let i = 0; i < 8; i++) {
      const az = (i / 8) * Math.PI * 2 + rng() * 0.6;
      const up = i < 3 ? lerp(0.75, 0.95, rng()) : lerp(0.2, 0.7, rng());
      const dir = V3(Math.cos(az) * (1 - up), up, Math.sin(az) * (1 - up)).normalize();
      const L = lerp(0.6, 0.85, rng()) * s;
      const b = new THREE.ConeGeometry(0.07 * s, L, 3, 1, true);
      b.translate(0, L / 2, 0);
      b.applyQuaternion(new THREE.Quaternion().setFromUnitVectors(V3(0, 1, 0), dir));
      b.translate(at.x, at.y + 0.2 * s, at.z);
      parts.push(paint(prep(b), [leaf, 0x7c8a44, 0x62703a][i % 3], 0.2, rng));
    }
    // Dead leaves hanging below: a skirt.
    const sk = new THREE.CylinderGeometry(0.24 * s, 0.12 * s, 0.5 * s, 5, 1, true);
    sk.translate(at.x, at.y - 0.1 * s, at.z);
    parts.push(paint(prep(sk), dead, 0.2, rng));
  };
  const grow = (a, dir, len, r, depth) => {
    const b = a.clone().addScaledVector(dir, len);
    parts.push(paint(prep(limb(a, b, r, r * 0.8, 5)), bark, 0.2, rng));
    if (depth <= 0 || (depth < 2 && rng() < 0.25)) { rosette(b, 0.8 + r * 1.2); return; }
    const k = 2 + (rng() < 0.4 ? 1 : 0);
    for (let i = 0; i < k; i++) {
      const az = (i / k) * Math.PI * 2 + rng() * 1.2;
      const up = lerp(0.35, 0.9, rng());
      const d2 = new THREE.Vector3(Math.cos(az) * (1 - up), up, Math.sin(az) * (1 - up)).normalize();
      grow(b, d2, len * lerp(0.55, 0.8, rng()), r * 0.72, depth - 1);
    }
  };
  grow(V3(0, 0, 0), V3((rng() - 0.5) * 0.2, 1, (rng() - 0.5) * 0.2).normalize(), lerp(2.2, 3.2, rng()), 0.26, 2 + (rng() < 0.5 ? 1 : 0));
  return merge(parts);
}

// Saguaro: ribbed column with up-turned arms.
export function saguaroGeometry(seed) {
  const rng = mulberry32(seed);
  const parts = [];
  const green = 0x55703e;
  const H = lerp(6, 10, rng());
  const col = (a, b, r) => parts.push(paint(prep(limb(a, b, r, r * 0.95, 10)), green, 0.12, rng));
  const top = (at, r) => {
    const s = new THREE.SphereGeometry(r * 0.95, 10, 5, 0, Math.PI * 2, 0, Math.PI / 2);
    s.translate(at.x, at.y, at.z);
    parts.push(paint(prep(s), green, 0.12, rng));
  };
  col(V3(0, 0, 0), V3(0, H, 0), 0.36);
  top(V3(0, H, 0), 0.36);
  const arms = Math.floor(rng() * 4);
  for (let i = 0; i < arms; i++) {
    const az = rng() * Math.PI * 2, y0 = lerp(2.2, H * 0.6, rng()), out = lerp(0.9, 1.5, rng());
    const dx = Math.cos(az), dz = Math.sin(az);
    const a = V3(dx * 0.2, y0, dz * 0.2), b = V3(dx * out, y0 + 0.5, dz * out);
    col(a, b, 0.24);
    const c = V3(dx * out, y0 + lerp(1.8, 3.5, rng()), dz * out);
    col(b, c, 0.24);
    top(c, 0.24);
  }
  return merge(parts);
}

// Creosote / brittlebush: a loose low mound (unit radius), vertex-coloured
// white so instances carry the colour.
export function bushGeometry(seed) {
  const n = makeNoise2D(seed);
  const rng = mulberry32(seed);
  const parts = [];
  // A loose clump of three or four small mounds, ~1 m tall at unit scale.
  const k = 4;
  for (let j = 0; j < k; j++) {
    // Only the central mound needs the finer sphere; the soft normals
    // below hide the facets on the small ones.
    const g = new THREE.IcosahedronGeometry(1, j === 0 ? 1 : 0);
    const p = g.getAttribute('position');
    for (let i = 0; i < p.count; i++) {
      const x = p.getX(i), y = p.getY(i), z = p.getZ(i);
      const r = 1 + n(x * 2.1 + z + j, y * 2.3) * 0.35;
      p.setXYZ(i, x * r, Math.max(y * r, -0.2), z * r);
    }
    const a = (j / k) * Math.PI * 2 + rng();
    const s = j === 0 ? 0.62 : lerp(0.35, 0.5, rng());
    const hs = s * lerp(0.9, 1.3, rng());
    g.scale(s, hs, s);
    // Sit each mound on the ground (its underside was clamped at -0.2)
    // rather than floating it up on a shadow like a mushroom.
    g.translate(j === 0 ? 0 : Math.cos(a) * 0.5, hs * 0.12, j === 0 ? 0 : Math.sin(a) * 0.5);
    // Soft, rounded shading: weld and smooth the normals.
    g.deleteAttribute('normal'); g.deleteAttribute('uv');
    const w = mergeVertices(g);
    w.computeVertexNormals();
    const flat = w.toNonIndexed();
    // Foliage normals: lean them toward "out from the whole clump" so the
    // bush shades like one soft mound rather than faceted lumps.
    const p2 = flat.getAttribute('position'), nn = flat.getAttribute('normal'), v = new THREE.Vector3();
    for (let i = 0; i < p2.count; i++) {
      v.set(p2.getX(i), p2.getY(i) + 0.3, p2.getZ(i)).normalize().lerp(new THREE.Vector3(nn.getX(i), nn.getY(i), nn.getZ(i)), 0.4).normalize();
      nn.setXYZ(i, v.x, v.y, v.z);
    }
    parts.push(paint(flat, 0xffffff, 0.25, rng));
  }
  return merge(parts);
}

// Juniper: a twisted trunk and a few dark foliage clumps.
export function juniperGeometry(seed) {
  const rng = mulberry32(seed);
  const parts = [];
  const bark = 0x5a4636;
  // A short twisted trunk splitting into crooked limbs, with ragged
  // blue-green foliage along and at the ends of them.
  const base = V3(0, 0, 0), fork = V3((rng() - 0.5) * 0.4, 0.9, (rng() - 0.5) * 0.4);
  parts.push(paint(prep(limb(base, fork, 0.2, 0.15, 5)), bark, 0.2, rng));
  const clump = (at, sc, i) => {
    const b = new THREE.DodecahedronGeometry(1, 0);
    b.scale(sc * lerp(0.9, 1.3, rng()), sc * lerp(0.55, 0.8, rng()), sc * lerp(0.9, 1.3, rng()));
    b.rotateY(rng() * 3);
    b.translate(at.x, at.y, at.z);
    parts.push(paint(prep(b), [0x4a5a3c, 0x55664a, 0x3e4c34, 0x5c6a50][i % 4], 0.25, rng));
  };
  const limbs = 2 + Math.floor(rng() * 2);
  let ci = 0;
  for (let i = 0; i < limbs; i++) {
    const az = (i / limbs) * Math.PI * 2 + rng();
    const end = fork.clone().add(V3(Math.cos(az) * lerp(0.8, 1.5, rng()), lerp(0.6, 1.6, rng()), Math.sin(az) * lerp(0.8, 1.5, rng())));
    parts.push(paint(prep(limb(fork, end, 0.12, 0.07, 4)), bark, 0.2, rng));
    clump(end, lerp(0.7, 1.0, rng()), ci++);
    clump(fork.clone().lerp(end, 0.5).add(V3(0, 0.25, 0)), lerp(0.5, 0.7, rng()), ci++);
  }
  clump(fork.clone().add(V3(0, 1.1, 0)), 0.8, ci++);
  return merge(parts);
}

// Yucca: a rosette of stiff blades.
export function yuccaGeometry(seed) {
  const rng = mulberry32(seed);
  const parts = [];
  for (let i = 0; i < 16; i++) {
    const az = (i / 16) * Math.PI * 2 + rng() * 0.3;
    const up = lerp(0.5, 0.95, rng());
    const dir = V3(Math.cos(az) * (1 - up), up, Math.sin(az) * (1 - up)).normalize();
    const b = new THREE.ConeGeometry(0.06, lerp(0.8, 1.2, rng()), 3, 1);
    b.translate(0, 0.5, 0);
    b.applyQuaternion(new THREE.Quaternion().setFromUnitVectors(V3(0, 1, 0), dir));
    parts.push(paint(prep(b), 0x7d8a52, 0.3, rng));
  }
  return merge(parts);
}

// Washingtonia fan palm for the oasis: tall slim trunk, a shaggy skirt of
// dead fronds, and a round crown.
export function palmGeometry(seed) {
  const rng = mulberry32(seed);
  const parts = [];
  const H = lerp(10, 15, rng());
  const lean = V3((rng() - 0.5) * 0.6, H, (rng() - 0.5) * 0.6);
  parts.push(paint(prep(limb(V3(0, 0, 0), lean, 0.32, 0.24, 7)), 0x7a6650, 0.15, rng));
  const sk = new THREE.CylinderGeometry(0.75, 0.42, 3.2, 8, 1, true);
  sk.translate(lean.x, H - 1.7, lean.z);
  parts.push(paint(prep(sk), 0x9a8058, 0.2, rng));
  // Fan leaves: a stalk out from the crown, then a pleated fan that droops.
  const N = 22;
  for (let i = 0; i < N; i++) {
    const az = (i / N) * Math.PI * 2 + rng() * 0.25;
    const up = lerp(-0.55, 0.45, (i % 3) / 2 * 0.7 + rng() * 0.3);
    const dir = V3(Math.cos(az), up, Math.sin(az)).normalize();
    const side = V3(-Math.sin(az), 0, Math.cos(az));
    const a = V3(lean.x, H, lean.z);
    const b = a.clone().addScaledVector(dir, 1.3);
    // Fan: a triangle fan of blades spreading from b, drooping at the tips.
    const pos = [];
    const blades = 6, L = lerp(1.6, 2.2, rng()), spread = 1.1;
    const tip = (k) => {
      const t = k / blades - 0.5;
      return b.clone().addScaledVector(dir, L * (1 - Math.abs(t) * 0.4)).addScaledVector(side, t * spread * 2).add(V3(0, -0.5 - Math.abs(t) * 0.3, 0));
    };
    for (let k = 0; k < blades; k++) {
      const p0 = tip(k), p1 = tip(k + 1);
      pos.push(b.x, b.y, b.z, p0.x, p0.y, p0.z, p1.x, p1.y, p1.z);
      pos.push(b.x, b.y, b.z, p1.x, p1.y, p1.z, p0.x, p0.y, p0.z); // both faces
    }
    const f = new THREE.BufferGeometry();
    f.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
    f.computeVertexNormals();
    parts.push(paint(f, [0x5a7438, 0x668040, 0x4e6630][i % 3], 0.2, rng));
    parts.push(paint(prep(limb(a, b, 0.05, 0.04, 3)), 0x6a6a3c, 0, rng));
  }
  return merge(parts);
}

// ── Course furniture ─────────────────────────────────────────────
// Traffic cone: orange with a white reflective band (unit ≈ 0.75 m tall).
export function coneGeometry() {
  const parts = [];
  const base = new THREE.BoxGeometry(0.42, 0.04, 0.42);
  base.translate(0, 0.02, 0);
  parts.push(paint(prep(base), 0x1a1a1a));
  const lo = new THREE.CylinderGeometry(0.12, 0.17, 0.28, 10, 1, true); lo.translate(0, 0.18, 0);
  const band = new THREE.CylinderGeometry(0.085, 0.12, 0.2, 10, 1, true); band.translate(0, 0.42, 0);
  const hi = new THREE.CylinderGeometry(0.02, 0.085, 0.26, 10, 1, true); hi.translate(0, 0.65, 0);
  parts.push(paint(prep(lo), 0xff5a14), paint(prep(band), 0xf2f2f2), paint(prep(hi), 0xff5a14));
  return merge(parts);
}

// ── Railway rolling stock ────────────────────────────────────────
// Each car is built along local +X (length), centred, wheels on y = 0.
// Returns geometry plus its length.
function box(parts, w, h, d, x, y, z, hex, rng, jit = 0.04) {
  const g = new THREE.BoxGeometry(w, h, d);
  g.translate(x, y + h / 2, z);
  parts.push(paint(prep(g), hex, jit, rng));
}
const darker = (hex, k) => new THREE.Color(hex).multiplyScalar(k).getHex();

// Two three-piece trucks: side frames, bolster, wheelsets.
function trucks(parts, L, rng, inset = 2.4) {
  for (const x of [-L / 2 + inset, L / 2 - inset]) {
    box(parts, 2.2, 0.35, 1.9, x, 0.75, 0, 0x1c1c1e, rng);                 // bolster
    for (const z of [-0.86, 0.86]) {
      box(parts, 3.0, 0.42, 0.14, x, 0.3, z, 0x242222, rng);               // side frame
      box(parts, 0.9, 0.3, 0.16, x, 0.55, z, 0x2a2826, rng);               // spring nest
    }
    for (const dx of [-0.9, 0.9]) for (const dz of [-0.76, 0.76]) {
      const w = new THREE.CylinderGeometry(0.46, 0.46, 0.14, 10);
      w.rotateX(Math.PI / 2);
      w.translate(x + dx, 0.46, dz);
      parts.push(paint(prep(w), 0x3a3230, 0, rng));
    }
  }
  box(parts, L - 0.8, 0.35, 2.7, 0, 1.05, 0, 0x25221f, rng); // underframe
  // Couplers.
  for (const s of [-1, 1]) box(parts, 0.7, 0.25, 0.3, s * (L / 2 + 0.2), 1.0, 0, 0x2a2826, rng);
}

// Side ribs every `step` metres along both faces.
function ribs(parts, L, h, y, z, hex, rng, step = 1.25, t = 0.08) {
  for (let x = -L / 2 + step / 2; x < L / 2; x += step) for (const s of [-1, 1]) box(parts, t, h, t, x, y, s * z, hex, rng, 0.02);
}
// Corner ladders with grab irons.
function ladders(parts, L, y0, z, rng, hex = 0xd8c040) {
  for (const sx of [-1, 1]) for (const sz of [-1, 1]) {
    const x = sx * (L / 2 - 0.35);
    for (const dx of [-0.22, 0.22]) box(parts, 0.04, 2.6, 0.04, x + dx, y0, sz * z, 0x2a2a2a, rng, 0);
    for (let k = 0; k < 6; k++) box(parts, 0.5, 0.04, 0.05, x, y0 + 0.3 + k * 0.42, sz * z, hex, rng, 0);
  }
}

// GE-style wide-cab road diesel: pilot and nose, four-window cab, long hood
// with radiator wings at the back, walkways and handrails, fuel tank.
export function locomotiveGeometry(scheme = 0) {
  const rng = mulberry32(66 + scheme);
  const parts = [], L = 22.5;
  const [body, top, stripe] = [[0xd8741c, 0x1e1e20, 0xf2d24a], [0x1f4a8a, 0xe8e4da, 0xc8322a]][scheme % 2];
  trucks(parts, L, rng, 3.3);
  // Fuel tank slung between the trucks.
  const ft = new THREE.CylinderGeometry(0.75, 0.75, 7.6, 10, 1);
  ft.rotateZ(Math.PI / 2); ft.scale(1, 0.8, 1.6); ft.translate(-0.4, 0.95, 0);
  parts.push(paint(prep(ft), 0x2a2a2c, 0.03, rng));
  box(parts, L, 0.28, 3.1, 0, 1.35, 0, 0x2a2a2c, rng);                     // deck
  box(parts, L - 0.2, 0.18, 3.14, 0, 1.3, 0, stripe, rng);                   // sill stripe
  // Long hood, narrower than the deck so the walkways show.
  const hx0 = -L / 2 + 0.8, hx1 = L / 2 - 6.2;
  box(parts, hx1 - hx0, 2.75, 2.3, (hx0 + hx1) / 2, 1.63, 0, body, rng);
  box(parts, hx1 - hx0, 0.3, 2.34, (hx0 + hx1) / 2, 4.38, 0, top, rng);
  // Hood doors: shallow panels and louvres along the sides.
  for (let x = hx0 + 0.8; x < hx1 - 0.6; x += 1.3) for (const s of [-1, 1]) {
    box(parts, 1.1, 1.9, 0.04, x, 2.0, s * 1.16, darker(body, 0.88), rng, 0.02);
    box(parts, 0.7, 0.3, 0.05, x, 3.5, s * 1.17, 0x2a2a2a, rng, 0);
  }
  // Radiator wings and fans at the back.
  box(parts, 3.8, 1.3, 3.02, hx0 + 2.1, 3.4, 0, body, rng);
  box(parts, 3.8, 0.12, 3.04, hx0 + 2.1, 4.7, 0, top, rng);
  for (const s of [-1, 1]) box(parts, 3.4, 1.0, 0.05, hx0 + 2.1, 3.55, s * 1.52, 0x2e2e30, rng, 0);
  for (const x of [hx0 + 1.2, hx0 + 3.0]) {
    const fan = new THREE.CylinderGeometry(0.7, 0.7, 0.14, 12);
    fan.translate(x, 4.85, 0);
    parts.push(paint(prep(fan), 0x2a2a2a, 0, rng));
  }
  // Exhaust stack and dynamic brake blister.
  box(parts, 0.9, 0.5, 0.5, hx0 + 6.5, 4.6, 0, 0x202022, rng);
  box(parts, 3.2, 0.45, 2.0, hx0 + 9.5, 4.6, 0, darker(body, 0.9), rng);
  // Cab.
  const cx0 = L / 2 - 6.2, cx1 = L / 2 - 2.4;
  box(parts, cx1 - cx0, 3.0, 3.06, (cx0 + cx1) / 2, 1.63, 0, body, rng);
  box(parts, cx1 - cx0 + 0.1, 0.35, 3.1, (cx0 + cx1) / 2, 4.63, 0, top, rng);          // roof
  box(parts, cx1 - cx0, 1.05, 3.08, (cx0 + cx1) / 2, 3.55, 0, top, rng);                // window band
  for (const s of [-1, 1]) for (const x of [cx0 + 0.8, cx0 + 2.3]) box(parts, 1.1, 0.8, 0.04, x, 3.68, s * 1.55, 0x101418, rng, 0);
  // Windshield: two dark panes raked back.
  for (const z of [-0.72, 0.72]) {
    const w = new THREE.BoxGeometry(0.06, 0.9, 1.25);
    w.rotateZ(0.12); w.translate(cx1 + 0.03, 4.1, z);
    parts.push(paint(prep(w), 0x0e1216, 0, rng));
  }
  // Short nose with a sloped top.
  box(parts, 2.0, 1.55, 2.7, cx1 + 1.0, 1.63, 0, body, rng);
  const slope = new THREE.BoxGeometry(2.1, 0.12, 2.72);
  slope.rotateZ(-0.35); slope.translate(cx1 + 0.95, 3.35, 0);
  parts.push(paint(prep(slope), body, 0.03, rng));
  box(parts, 0.12, 0.35, 2.4, cx1 + 1.95, 2.55, 0, stripe, rng);                        // nose chevron band
  box(parts, 0.1, 0.4, 1.0, cx1 + 2.02, 2.95, 0, 0xf0f0e0, rng, 0);                     // headlight bar
  // Pilot / snowplough, black-and-yellow.
  const plow = new THREE.BoxGeometry(0.5, 0.9, 3.0);
  plow.rotateZ(0.35); plow.translate(L / 2 + 0.15, 0.75, 0);
  parts.push(paint(prep(plow), 0x1c1c1c, 0, rng));
  box(parts, 0.2, 0.22, 3.12, L / 2 - 0.05, 1.1, 0, stripe, rng);
  // Walkway handrails and stanchions, both sides and across the ends.
  for (const s of [-1, 1]) {
    box(parts, hx1 - hx0 + 0.6, 0.05, 0.05, (hx0 + hx1) / 2, 2.55, s * 1.5, stripe, rng, 0);
    for (let x = hx0; x <= hx1; x += 2.2) box(parts, 0.05, 1.05, 0.05, x, 1.5, s * 1.5, stripe, rng, 0);
    // Steps at the corners.
    for (const x of [-L / 2 + 0.6, L / 2 - 1.2]) box(parts, 0.7, 0.9, 0.06, x, 0.45, s * 1.5, 0x2a2a2a, rng, 0);
  }
  box(parts, 0.05, 0.05, 3.0, -L / 2 + 0.3, 2.55, 0, stripe, rng, 0);
  // Horn and antenna on the cab roof.
  box(parts, 0.6, 0.2, 0.2, cx0 + 1.2, 4.98, 0.4, 0x6a6a6a, rng, 0);
  return { geo: merge(parts), L };
}

export function boxcarGeometry(hex, seed = 1) {
  const rng = mulberry32(seed);
  const parts = [], L = 17;
  trucks(parts, L, rng);
  box(parts, L, 3.3, 2.9, 0, 1.25, 0, hex, rng);
  box(parts, L + 0.05, 0.15, 3.0, 0, 4.55, 0, darker(hex, 0.7), rng);                // roof
  ribs(parts, L, 3.2, 1.3, 1.47, darker(hex, 0.85), rng, 1.3);
  box(parts, 3.4, 3.0, 3.04, 0, 1.3, 0, darker(hex, 0.78), rng);                     // plug door
  box(parts, 7.5, 0.1, 3.08, 0, 4.25, 0, 0x2a2826, rng, 0);                           // door track
  box(parts, 0.5, 0.35, 0.2, 5.5, 3.4, 1.5, 0xe8e2d4, rng, 0);                        // reporting marks
  box(parts, 0.5, 0.35, 0.2, -5.5, 3.4, -1.5, 0xe8e2d4, rng, 0);
  box(parts, 0.6, 0.06, 1.0, L / 2 - 0.4, 4.7, 0, 0x2a2826, rng, 0);                 // roof walk ends
  ladders(parts, L, 1.4, 1.5, rng);
  return { geo: merge(parts), L };
}

export function tankGeometry(hex, seed = 2) {
  const rng = mulberry32(seed);
  const parts = [], L = 16;
  trucks(parts, L, rng);
  const t = new THREE.CylinderGeometry(1.45, 1.45, L - 1.2, 14);
  t.rotateZ(Math.PI / 2);
  t.translate(0, 2.8, 0);
  parts.push(paint(prep(t), hex, 0.03, rng));
  for (const x of [-(L - 1.2) / 2, (L - 1.2) / 2]) {
    const cap = new THREE.SphereGeometry(1.45, 12, 6, 0, Math.PI * 2, 0, Math.PI / 2);
    cap.scale(0.35, 1, 1);
    cap.rotateZ(x > 0 ? -Math.PI / 2 : Math.PI / 2);
    cap.translate(x, 2.8, 0);
    parts.push(paint(prep(cap), hex, 0.03, rng));
  }
  // Tank bands, walkway and the dome with its platform.
  for (const x of [-4.5, 0, 4.5]) {
    const b = new THREE.CylinderGeometry(1.48, 1.48, 0.12, 14, 1, true);
    b.rotateZ(Math.PI / 2); b.translate(x, 2.8, 0);
    parts.push(paint(prep(b), darker(hex, 0.7), 0, rng));
  }
  box(parts, 1.2, 0.5, 1.2, 0, 4.2, 0, 0x3a3634, rng);
  box(parts, 2.4, 0.06, 1.8, 0, 4.25, 0, 0x2a2826, rng, 0);
  box(parts, 0.05, 0.9, 1.8, -1.2, 4.3, 0, 0xd8c040, rng, 0);
  box(parts, 0.05, 0.9, 1.8, 1.2, 4.3, 0, 0xd8c040, rng, 0);
  box(parts, 2.2, 0.35, 0.04, -4.5, 2.2, 1.49, 0xd8a020, rng, 0);                    // placard band
  return { geo: merge(parts), L };
}

// Covered hopper: ribbed body with sloped ends, a rounded roof with
// hatches, three discharge bays underneath.
export function hopperGeometry(hex, seed = 3) {
  const rng = mulberry32(seed);
  const parts = [], L = 18;
  trucks(parts, L, rng);
  box(parts, L - 2.4, 2.9, 3.1, 0, 1.7, 0, hex, rng);
  for (const s of [-1, 1]) {
    const e = new THREE.BoxGeometry(1.6, 2.4, 3.1);
    e.rotateZ(s * 0.45); e.translate(s * (L / 2 - 1.1), 2.6, 0);
    parts.push(paint(prep(e), hex, 0.04, rng));
  }
  const roof = new THREE.CylinderGeometry(1.55, 1.55, L - 2.4, 12, 1, false, 0, Math.PI);
  roof.rotateZ(Math.PI / 2); roof.rotateX(Math.PI / 2); roof.scale(1, 0.32, 1); roof.translate(0, 4.6, 0);
  parts.push(paint(prep(roof), darker(hex, 0.92), 0.03, rng));
  for (let x = -6; x <= 6; x += 2) box(parts, 0.6, 0.12, 0.6, x, 5.0, 0, darker(hex, 0.7), rng, 0);
  ribs(parts, L - 2.4, 2.8, 1.75, 1.57, darker(hex, 0.8), rng, 1.5, 0.1);
  for (const x of [-4.5, 0, 4.5]) {
    const c = new THREE.ConeGeometry(1.5, 1.2, 4);
    c.rotateY(Math.PI / 4);
    c.rotateX(Math.PI);
    c.translate(x, 1.3, 0);
    parts.push(paint(prep(c), hex, 0.05, rng));
  }
  ladders(parts, L, 1.4, 1.58, rng);
  return { geo: merge(parts), L };
}

// Open gondola, heaped with scrap or ballast.
export function gondolaGeometry(hex, seed = 11, load = 'scrap') {
  const rng = mulberry32(seed);
  const parts = [], L = 16;
  trucks(parts, L, rng);
  box(parts, L, 0.25, 3.0, 0, 1.25, 0, darker(hex, 0.6), rng);
  for (const s of [-1, 1]) box(parts, L, 1.6, 0.1, 0, 1.4, s * 1.45, hex, rng);
  for (const s of [-1, 1]) box(parts, 0.1, 1.6, 3.0, s * (L / 2 - 0.05), 1.4, 0, hex, rng);
  ribs(parts, L, 1.6, 1.4, 1.52, darker(hex, 0.8), rng, 1.0, 0.1);
  if (load === 'scrap') {
    for (let i = 0; i < 18; i++) {
      const g = new THREE.BoxGeometry(lerp(0.6, 2.2, rng()), lerp(0.2, 0.7, rng()), lerp(0.4, 1.8, rng()));
      g.rotateY(rng() * 3); g.rotateX((rng() - 0.5) * 0.6); g.rotateZ((rng() - 0.5) * 0.6);
      g.translate(lerp(-L / 2 + 1.5, L / 2 - 1.5, rng()), 2.8 + rng() * 0.4, (rng() - 0.5) * 1.8);
      parts.push(paint(prep(g), [0x6a4a34, 0x5a5a5c, 0x7a5a3a, 0x3a3a3c][i % 4], 0.2, rng));
    }
  } else {
    const heap = new THREE.CylinderGeometry(0.2, 1.4, 0.9, 8, 1);
    heap.scale(L / 3.2, 1, 1); heap.translate(0, 3.3, 0);
    parts.push(paint(prep(heap), 0x8a7a68, 0.15, rng));
  }
  return { geo: merge(parts), L };
}

// Enclosed autorack: tall, with perforated side panels (light and dark
// bands suggest the holes).
export function autorackGeometry(hex, seed = 12) {
  const rng = mulberry32(seed);
  const parts = [], L = 26;
  trucks(parts, L, rng);
  box(parts, L, 5.0, 3.1, 0, 1.25, 0, hex, rng);
  box(parts, L + 0.05, 0.2, 3.14, 0, 6.25, 0, 0xd8d6d0, rng);
  for (let x = -L / 2 + 0.7; x < L / 2; x += 1.2) for (const s of [-1, 1]) {
    box(parts, 0.9, 3.4, 0.04, x, 2.2, s * 1.57, darker(hex, 0.55), rng, 0.05);
  }
  box(parts, L, 0.25, 3.14, 0, 1.3, 0, darker(hex, 0.8), rng);
  box(parts, 0.1, 4.6, 3.1, L / 2, 1.4, 0, darker(hex, 0.7), rng);
  box(parts, 0.1, 4.6, 3.1, -L / 2, 1.4, 0, darker(hex, 0.7), rng);
  return { geo: merge(parts), L };
}

// Centre-beam flatcar stacked with wrapped lumber bundles.
export function lumberGeometry(seed = 13) {
  const rng = mulberry32(seed);
  const parts = [], L = 22;
  trucks(parts, L, rng);
  box(parts, L, 0.35, 3.0, 0, 1.25, 0, 0x4a4a4c, rng);
  box(parts, 0.4, 3.3, 3.0, -L / 2 + 0.3, 1.6, 0, 0x4a4a4c, rng);
  box(parts, 0.4, 3.3, 3.0, L / 2 - 0.3, 1.6, 0, 0x4a4a4c, rng);
  box(parts, L - 0.4, 0.3, 0.3, 0, 4.6, 0, 0x4a4a4c, rng);
  for (let x = -L / 2 + 1.2; x < L / 2 - 1; x += 1.8) box(parts, 0.15, 3.0, 0.2, x, 1.6, 0, 0x4a4a4c, rng, 0);
  const cols = [0xe4d8b4, 0xd8c89a, 0xf0ece0, 0xc4ac7a];
  for (let k = 0; k < 4; k++) for (let j = 0; j < 3; j++) for (const s of [-1, 1]) {
    box(parts, 4.9, 0.95, 1.3, -L / 2 + 3.1 + k * 5.2, 1.6 + j * 1.0, s * 0.8, cols[(k + j + (s > 0 ? 1 : 0)) % 4], rng, 0.08);
  }
  return { geo: merge(parts), L };
}

export function stackGeometry(cols, seed = 4) {
  const rng = mulberry32(seed);
  const parts = [], L = 16;
  trucks(parts, L, rng);
  box(parts, L - 1, 0.4, 2.8, 0, 0.9, 0, 0x2a2826, rng);
  // Containers with corrugation ribs and door-end detail.
  const cont = (len, x, y, hex) => {
    box(parts, len, 2.6, 2.45, x, y, 0, hex, rng);
    ribs(parts, len - 0.2, 2.5, y + 0.05, 1.23, darker(hex, 0.82), rng, 0.55, 0.06);
    box(parts, len + 0.04, 0.12, 2.49, x, y + 2.5, 0, darker(hex, 0.75), rng, 0);
  };
  cont(6.05, -3.15, 1.3, cols[0]);
  cont(6.05, 3.15, 1.3, cols[1]);
  cont(12.2, 0, 3.92, cols[2]);
  return { geo: merge(parts), L };
}
