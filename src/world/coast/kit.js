import * as THREE from 'three';
import { mergeGeometries } from 'three/addons/utils/BufferGeometryUtils.js';
import { makeNoise2D } from '../../util/math.js';
import { rockTexture } from '../textures.js';

// Scenery building kit for the coast (copied from Mountain.js, whose helpers
// are module-private): rendered-surface sampling, instancing/merging, rock
// geometry and material, and sign drawing into a single atlas.

// ── Terrain surface as rendered ─────────────────────────────────────
// Coarse terrain tiles interpolate between 16/32 m samples, so the exact
// height function can sit metres above or below what is drawn. Objects are
// placed on the rendered triangles instead.
export class SurfaceSampler {
  constructor(terrain) {
    this.T = terrain;
    this.steps = new Map();
    for (const t of terrain.tileList()) this.steps.set(t.i + ',' + t.j, t.step);
    this.cache = new Map();
  }
  h(x, z) {
    const T = this.T;
    const key = Math.round((x - T.minX) / 4) * 65536 + Math.round((z - T.minZ) / 4);
    let v = this.cache.get(key);
    if (v === undefined) { v = T.heightAt(x, z); this.cache.set(key, v); }
    return v;
  }
  sample(x, z, out = {}) {
    const T = this.T;
    const ti = Math.floor((x - T.minX) / 256), tj = Math.floor((z - T.minZ) / 256);
    const step = this.steps.get(ti + ',' + tj) ?? 32;
    const x0 = T.minX + ti * 256, z0 = T.minZ + tj * 256;
    const gx = (x - x0) / step, gz = (z - z0) / step;
    const ci = Math.floor(gx), cj = Math.floor(gz);
    const fx = gx - ci, fz = gz - cj;
    const ax = x0 + ci * step, az = z0 + cj * step;
    const ha = this.h(ax, az), hb = this.h(ax + step, az);
    const hc = this.h(ax, az + step), hd = this.h(ax + step, az + step);
    let h;
    if ((ci + cj) & 1) {
      h = fx + fz < 1 ? ha + (hb - ha) * fx + (hc - ha) * fz : hd + (hc - hd) * (1 - fx) + (hb - hd) * (1 - fz);
    } else {
      h = fz > fx ? ha + (hd - hc) * fx + (hc - ha) * fz : ha + (hb - ha) * fx + (hd - hb) * fz;
    }
    out.h = h;
    out.gx = (hb - ha + hd - hc) * 0.5 / step;
    out.gz = (hc - ha + hd - hb) * 0.5 / step;
    out.slope = Math.hypot(out.gx, out.gz);
    out.step = step;
    return out;
  }
}

// ── Geometry factories ───────────────────────────────────────────────
export function rockGeometry(seed, detail, squash = 0.72, sharp = 0.42) {
  const g = new THREE.IcosahedronGeometry(1, detail);
  const n = makeNoise2D(seed);
  const pos = g.getAttribute('position');
  for (let i = 0; i < pos.count; i++) {
    const x = pos.getX(i), y = pos.getY(i), z = pos.getZ(i);
    const d = n(x * 1.1 + z * 2.3 + 5, y * 1.3 - z * 0.7) * 0.6 + n(x * 2.9 - z * 1.7, y * 2.6 + x) * 0.3;
    const r = 1 + d * sharp;
    // Flatten the underside so rocks sit rather than balance.
    pos.setXYZ(i, x * r, Math.max(y * r * squash, -0.3), z * r);
  }
  g.deleteAttribute('uv');
  g.computeVertexNormals(); // non-indexed → faceted
  return g;
}

export function colorize(geo, hex) {
  const c = new THREE.Color(hex);
  const n = geo.getAttribute('position').count;
  const a = new Float32Array(n * 3);
  for (let i = 0; i < n; i++) { a[i * 3] = c.r; a[i * 3 + 1] = c.g; a[i * 3 + 2] = c.b; }
  geo.setAttribute('color', new THREE.BufferAttribute(a, 3));
  return geo;
}

export function prep(geo) {
  const g = geo.index ? geo.toNonIndexed() : geo;
  if (g.hasAttribute('uv')) g.deleteAttribute('uv');
  g.computeVertexNormals();
  return g;
}

// Triplanar strata for rocks, instancing-aware (TerrainMesh's patch assumes
// no instance matrix).
export function rockMaterial() {
  const m = new THREE.MeshStandardMaterial({ color: 0xffffff, roughness: 0.92, metalness: 0 });
  const tex = rockTexture();
  m.userData.kind = 'TriplanarRock'; // MaterialKind for the scene export
  m.onBeforeCompile = (sh) => {
    sh.uniforms.tRock = { value: tex };
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
      .replace('#include <common>', '#include <common>\nvarying vec3 vRP;\nvarying vec3 vRN;\nuniform sampler2D tRock;')
      .replace('#include <map_fragment>', `
        vec3 w_ = pow(abs(normalize(vRN)), vec3(3.0));
        w_ /= (w_.x + w_.y + w_.z);
        vec3 a_ = texture2D(tRock, vec2(vRP.z * 0.09, vRP.y * 0.14)).rgb;
        vec3 b_ = texture2D(tRock, vec2(vRP.x * 0.09, vRP.y * 0.14)).rgb;
        vec3 c_ = texture2D(tRock, vRP.xz * 0.11).rgb;
        diffuseColor.rgb *= (a_ * w_.x + b_ * w_.z + c_ * w_.y) * 1.3;`);
  };
  m.customProgramCacheKey = () => 'coast-rock';
  return m;
}

const UP = new THREE.Vector3(0, 1, 0);
const _m4 = new THREE.Matrix4(), _q = new THREE.Quaternion(), _p = new THREE.Vector3(), _s = new THREE.Vector3(), _e = new THREE.Euler(), _c = new THREE.Color();
export function instanced(geo, mat, items, { cast = false, receive = true } = {}) {
  const im = new THREE.InstancedMesh(geo, mat, items.length);
  items.forEach((it, k) => {
    if (it.q) _q.copy(it.q);
    else { _e.set(it.rx || 0, it.ry || 0, it.rz || 0); _q.setFromEuler(_e); }
    _p.set(it.x, it.y, it.z);
    _s.set(it.sx, it.sy, it.sz);
    _m4.compose(_p, _q, _s);
    im.setMatrixAt(k, _m4);
    if (it.col !== undefined) im.setColorAt(k, _c.setHex(it.col).multiplyScalar(it.b ?? 1));
  });
  im.instanceMatrix.needsUpdate = true;
  if (im.instanceColor) im.instanceColor.needsUpdate = true;
  im.computeBoundingSphere();
  im.castShadow = cast;
  im.receiveShadow = receive;
  im.matrixAutoUpdate = false;
  im.updateMatrix();
  return im;
}

// Place a merged-geometry piece at a pose.
export function placed(geo, x, y, z, yaw = 0, sx = 1, sy = 1, sz = 1) {
  const g = geo.clone();
  _e.set(0, yaw, 0);
  _q.setFromEuler(_e);
  _m4.compose(_p.set(x, y, z), _q, _s.set(sx, sy, sz));
  g.applyMatrix4(_m4);
  return g;
}

// Collapse a static prop hierarchy (e.g. a parked car) into one mesh per
// material, so decoration doesn't cost a draw call per part.
export function bakeStatic(root) {
  root.updateMatrixWorld(true);
  const byMat = new Map();
  root.traverse((o) => {
    if (!o.isMesh || Array.isArray(o.material)) return;
    const g = o.geometry.clone().applyMatrix4(o.matrixWorld);
    if (!byMat.has(o.material)) byMat.set(o.material, []);
    byMat.get(o.material).push(g);
  });
  const out = new THREE.Group();
  for (const [mat, geos] of byMat) {
    const keep = ['position', 'normal', 'uv'];
    if (mat.vertexColors) keep.push('color');
    const norm = geos.map((g) => {
      const h = g.index ? g.toNonIndexed() : g;
      for (const a of Object.keys(h.attributes)) if (!keep.includes(a)) h.deleteAttribute(a);
      const n = h.getAttribute('position').count;
      if (!h.hasAttribute('uv')) h.setAttribute('uv', new THREE.BufferAttribute(new Float32Array(n * 2), 2));
      if (!h.hasAttribute('normal')) h.computeVertexNormals();
      if (mat.vertexColors && !h.hasAttribute('color')) h.setAttribute('color', new THREE.BufferAttribute(new Float32Array(n * 3).fill(1), 3));
      return h;
    });
    const m = new THREE.Mesh(mergeGeometries(norm, false), mat);
    m.castShadow = true;
    m.receiveShadow = true;
    m.matrixAutoUpdate = false;
    out.add(m);
  }
  return out;
}

export function mergedMesh(geos, mat, { cast = false, receive = true } = {}) {
  if (!geos.length) return null;
  const norm = geos.map((g) => {
    let h = g.index ? g.toNonIndexed() : g;
    for (const a of Object.keys(h.attributes)) if (a !== 'position' && a !== 'normal' && a !== 'uv') h.deleteAttribute(a);
    if (!h.hasAttribute('uv')) h.setAttribute('uv', new THREE.BufferAttribute(new Float32Array(h.getAttribute('position').count * 2), 2));
    return h;
  });
  const m = new THREE.Mesh(mergeGeometries(norm, false), mat);
  m.castShadow = cast;
  m.receiveShadow = receive;
  m.matrixAutoUpdate = false;
  m.updateMatrix();
  return m;
}

// ── Canvas helpers ───────────────────────────────────────────────────
export function makeCanvas(w, h) {
  const c = document.createElement('canvas');
  c.width = w; c.height = h;
  return [c, c.getContext('2d')];
}
export function canvasTex(c) {
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = THREE.SRGBColorSpace;
  t.anisotropy = 8;
  return t;
}
export const FONT = '"Arial Narrow", "Helvetica Neue", Arial, sans-serif';

// Sign faces drawn into one atlas so every sign is a single draw call.
export class SignAtlas {
  constructor() {
    this.W = 2048; this.H = 1024; this.cell = 256;
    [this.canvas, this.g] = makeCanvas(this.W, this.H);
    this.x = 0; this.y = 0; this.rowH = 0;
  }
  // w,h in pixels; draw(g, w, h) paints in local coords.
  add(w, h, draw) {
    if (this.x + w > this.W) { this.x = 0; this.y += this.rowH + 4; this.rowH = 0; }
    const x = this.x, y = this.y;
    this.g.save();
    this.g.translate(x, y);
    this.g.beginPath(); this.g.rect(0, 0, w, h); this.g.clip();
    draw(this.g, w, h);
    this.g.restore();
    this.x += w + 4;
    this.rowH = Math.max(this.rowH, h);
    return { u0: x / this.W, u1: (x + w) / this.W, v0: 1 - (y + h) / this.H, v1: 1 - y / this.H };
  }
  texture() { return canvasTex(this.canvas); }
}

export function roundRect(g, x, y, w, h, r) {
  g.beginPath();
  g.moveTo(x + r, y); g.lineTo(x + w - r, y); g.quadraticCurveTo(x + w, y, x + w, y + r);
  g.lineTo(x + w, y + h - r); g.quadraticCurveTo(x + w, y + h, x + w - r, y + h);
  g.lineTo(x + r, y + h); g.quadraticCurveTo(x, y + h, x, y + h - r);
  g.lineTo(x, y + r); g.quadraticCurveTo(x, y, x + r, y);
  g.closePath();
}

export function diamond(g, w, h, inner) {
  g.clearRect(0, 0, w, h);
  g.save();
  g.translate(w / 2, h / 2);
  g.rotate(Math.PI / 4);
  const s = w * 0.69;
  roundRect(g, -s / 2, -s / 2, s, s, 12);
  g.fillStyle = '#111'; g.fill();
  roundRect(g, -s / 2 + 7, -s / 2 + 7, s - 14, s - 14, 9);
  g.fillStyle = '#f5c518'; g.fill();
  g.restore();
  g.fillStyle = '#111'; g.strokeStyle = '#111';
  inner(g, w / 2, h / 2);
}

export function panel(g, w, h, bg, fg, border, lines, sizes) {
  roundRect(g, 2, 2, w - 4, h - 4, 16); g.fillStyle = border; g.fill();
  roundRect(g, 9, 9, w - 18, h - 18, 11); g.fillStyle = bg; g.fill();
  g.fillStyle = fg; g.textAlign = 'center'; g.textBaseline = 'middle';
  const total = sizes.reduce((a, b) => a + b * 1.12, 0);
  let y = h / 2 - total / 2;
  lines.forEach((l, i) => {
    g.font = `bold ${sizes[i]}px ${FONT}`;
    y += sizes[i] * 0.56;
    g.fillText(l, w / 2, y + 2);
    y += sizes[i] * 0.56;
  });
}


export const UP_AXIS = new THREE.Vector3(0, 1, 0);
