import * as THREE from 'three';
import { mergeGeometries } from 'three/addons/utils/BufferGeometryUtils.js';
import { clamp, lerp, smoothstep, mulberry32, fbm, makeNoise2D } from '../util/math.js';
import { rockTexture, gravelTexture, smokeTexture } from './textures.js';
import { rockGeometry, coniferGeometry, grassClumpGeometry, flowerGeometry, shrubGeometry, foliageMaterial, makeNoise3D } from './valley/flora.js';

// Sierra Pass scenery: boulders and outcrops on the rock walls, pine forest
// on the gentler ground, the start gantry with a roadside diner, warning
// signs for the switchbacks, a summit lookout and a waterfall in the canyon.
//
// Everything is instanced or merged per material and chunked along the road
// so frustum culling does useful work. Placement is deterministic.

const CHUNK = 480;          // metres of road per instancing chunk
const NEAR_FOREST = 420;    // detailed pines out to this distance from road

// ── Terrain surface as rendered ─────────────────────────────────────
// Coarse terrain tiles interpolate between 16/32 m samples, so the exact
// height function can sit metres above or below what is drawn. Objects are
// placed on the rendered triangles instead.
class SurfaceSampler {
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

// Triplanar strata for rocks, instancing-aware (TerrainMesh's patch assumes
// no instance matrix).
function rockMaterial(vertexColors = true) {
  const m = new THREE.MeshStandardMaterial({ color: 0xffffff, roughness: 0.92, metalness: 0, vertexColors });
  const tex = rockTexture();
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
  m.customProgramCacheKey = () => 'mtn-rock' + (vertexColors ? '-vc' : '');
  return m;
}

const UP = new THREE.Vector3(0, 1, 0);
const _m4 = new THREE.Matrix4(), _q = new THREE.Quaternion(), _p = new THREE.Vector3(), _s = new THREE.Vector3(), _e = new THREE.Euler(), _c = new THREE.Color();
function instanced(geo, mat, items, { cast = false, receive = true } = {}) {
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
function placed(geo, x, y, z, yaw = 0, sx = 1, sy = 1, sz = 1) {
  const g = geo.clone();
  _e.set(0, yaw, 0);
  _q.setFromEuler(_e);
  _m4.compose(_p.set(x, y, z), _q, _s.set(sx, sy, sz));
  g.applyMatrix4(_m4);
  return g;
}

// Collapse a static prop hierarchy (e.g. a parked car) into one mesh per
// material, so decoration doesn't cost a draw call per part.
function bakeStatic(root) {
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

function mergedMesh(geos, mat, { cast = false, receive = true } = {}) {
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
function makeCanvas(w, h) {
  const c = document.createElement('canvas');
  c.width = w; c.height = h;
  return [c, c.getContext('2d')];
}
function canvasTex(c) {
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = THREE.SRGBColorSpace;
  t.anisotropy = 8;
  return t;
}
const FONT = '"Arial Narrow", "Helvetica Neue", Arial, sans-serif';

// Sign faces drawn into one atlas so every sign is a single draw call.
class SignAtlas {
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

function roundRect(g, x, y, w, h, r) {
  g.beginPath();
  g.moveTo(x + r, y); g.lineTo(x + w - r, y); g.quadraticCurveTo(x + w, y, x + w, y + r);
  g.lineTo(x + w, y + h - r); g.quadraticCurveTo(x + w, y + h, x + w - r, y + h);
  g.lineTo(x + r, y + h); g.quadraticCurveTo(x, y + h, x, y + h - r);
  g.lineTo(x, y + r); g.quadraticCurveTo(x, y, x + r, y);
  g.closePath();
}

function diamond(g, w, h, inner) {
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

function panel(g, w, h, bg, fg, border, lines, sizes) {
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

// ── The scenery module ───────────────────────────────────────────────
export default class Mountain {
  constructor() {
    this.label = 'Mountain scenery';
    this.exclusions = [];
  }

  // Cut the diner pull-out and the summit lookout into the hillside before
  // the terrain fields are rasterised.
  plan(world) {
    const t = world.track;
    const wall = t.wallR[40];
    this.diner = { s: 26, side: 1 };
    this.diner.lat = wall + 17;
    const dp = t.pointAt(this.diner.s, this.diner.lat);
    this.diner.x = dp.x; this.diner.z = dp.z;
    this.diner.y = t.surfaceY(this.diner.s, wall) - 0.12;
    world.terrain.addFlatten(dp.x, dp.z, 17, 18, this.diner.y);
    this.exclusions.push({ x: dp.x, z: dp.z, r: 22 });

    const sm = t.tag('summit')[0];
    if (sm) {
      const s = Math.round((sm.s0 + sm.s1) / 2);
      // Lookout on the side facing the valley.
      const vs = t.zoneStart[1] + 800;
      const toward = (t.px[vs] - t.px[s]) * t.rx[s] + (t.pz[vs] - t.pz[s]) * t.rz[s] > 0 ? 1 : -1;
      const w = toward > 0 ? t.wallR[s] : t.wallL[s];
      const lat = toward * (w + 12);
      const p = t.pointAt(s, lat);
      this.lookout = { s, side: toward, lat, x: p.x, z: p.z, y: t.surfaceY(s, toward * w) - 0.1 };
      world.terrain.addFlatten(p.x, p.z, 12, 14, this.lookout.y);
      this.exclusions.push({ x: p.x, z: p.z, r: 16 });
    }
  }

  async build(world) {
    const t0 = performance.now();
    this.world = world;
    this.track = world.track;
    this.terrain = world.terrain;
    this.surf = new SurfaceSampler(world.terrain);
    this.group = new THREE.Group();
    this.group.name = 'mountain';
    this.stats = { tris: 0, calls: 0 };

    this.buildRocks();
    await new Promise((r) => setTimeout(r, 0));
    this.buildForest();
    await new Promise((r) => setTimeout(r, 0));
    this.buildVerge();
    this.buildSnow();
    this.buildSigns();
    this.buildSnowPoles();
    this.buildDelineators();
    this.buildStartArea();
    await this.buildDiner();
    await this.buildLookout();
    this.buildWaterfall();

    world.scene.add(this.group);
    this.group.traverse((o) => {
      if (!o.isMesh) return;
      this.stats.calls++;
      const g = o.geometry;
      const tri = (g.index ? g.index.count : g.getAttribute('position').count) / 3;
      this.stats.tris += tri * (o.isInstancedMesh ? o.count : 1);
    });
    console.info(`[Mountain] ${this.stats.calls} meshes, ${(this.stats.tris / 1000).toFixed(0)}k tris, ${(performance.now() - t0).toFixed(0)} ms`);
  }

  excluded(x, z) {
    for (const e of this.exclusions) if (Math.pow(x - e.x, 2) + Math.pow(z - e.z, 2) < e.r * e.r) return true;
    return false;
  }

  // Clear of every stretch of road (switchback legs included)?
  clearOfRoad(x, z, radius) {
    const info = this.terrain.roadInfo(x, z);
    if (!info.near) return info.d > radius + 12;
    const i = this.track.idx(info.s);
    const wall = Math.max(this.track.wallL[i], this.track.wallR[i]);
    return info.d - radius >= wall + 0.6;
  }

  get mEnd() { return this.track.zoneStart[1]; }

  // ── Rocks ──────────────────────────────────────────────────────────
  buildRocks() {
    const t = this.track, rng = mulberry32(4242);
    const nChunks = Math.ceil(this.mEnd / CHUNK) + 1;
    // Rocks the chase camera passes close to get the detailed mesh; the
    // outcrops scattered up the slopes use a coarser one.
    const big = Array.from({ length: nChunks }, () => []);
    const outcrops = Array.from({ length: nChunks }, () => []);
    const scree = [];
    const S = {};
    // Instance tints multiply the rock's baked vertex colours (granite
    // greys with a few warmer, iron-stained stones).
    const palette = [0xc4bdb3, 0xb3aca2, 0xcac0b0, 0xa8a29a, 0xd2c8b6, 0xb8ad9d, 0xc9b39a];
    const addRock = (list, x, z, size, embed, opts = {}) => {
      const sx = size * (opts.stretch ?? lerp(0.8, 1.35, rng()));
      const sz = size * lerp(0.75, 1.25, rng());
      const sy = size * (opts.flat ?? lerp(0.7, 1.2, rng()));
      // Slabs by the road lie along it, so their long axis can't poke in.
      const aligned = opts.yaw !== undefined;
      const ry = aligned ? opts.yaw + (rng() - 0.5) * 0.5 : rng() * Math.PI * 2;
      const reach = aligned ? sz * 0.95 + sx * 0.28 : Math.max(sx, sz) * 0.95;
      if (this.excluded(x, z)) return false;
      if (!this.clearOfRoad(x, z, reach)) return false;
      this.surf.sample(x, z, S);
      if (opts.onSlope) {
        // Lie the slab along the face and sink it along the normal, so it
        // reads as an outcrop rather than a shelf sticking out.
        const n = new THREE.Vector3(-S.gx, 1, -S.gz).normalize();
        const q = new THREE.Quaternion().setFromUnitVectors(UP, n);
        q.multiply(new THREE.Quaternion().setFromAxisAngle(UP, ry));
        const e = sy * embed;
        list.push({
          x: x - n.x * e, y: S.h - n.y * e, z: z - n.z * e, sx, sy, sz, q,
          col: palette[Math.floor(rng() * palette.length)], b: lerp(0.8, 1.12, rng()) * (opts.dark ? 0.78 : 1),
        });
        return true;
      }
      list.push({
        x, z, y: S.h - sy * Math.min(0.75, embed + S.slope * 0.2),
        sx, sy, sz,
        rx: (rng() - 0.5) * 0.5, ry, rz: (rng() - 0.5) * 0.5,
        col: palette[Math.floor(rng() * palette.length)], b: lerp(0.8, 1.12, rng()),
      });
      return true;
    };

    for (let s = 0; s < this.mEnd; s += 4) {
      const i = t.idx(s);
      const ch = big[Math.floor(s / CHUNK)];
      t.frame(s, S);
      const roadYaw = -Math.atan2(S.fz, S.fx);
      for (const side of [-1, 1]) {
        const kind = side < 0 ? t.sideL[i] : t.sideR[i];
        const wall = side < 0 ? t.wallL[i] : t.wallR[i];
        const at = (lat) => [S.x + S.rx * lat * side, S.z + S.rz * lat * side];
        if (kind === 1) {
          // Boulders heaped at the foot of the rock wall.
          if (rng() < 0.5) {
            const size = lerp(0.5, 2.4, Math.pow(rng(), 1.6));
            const [x, z] = at(wall + 0.7 + size + rng() * rng() * 6);
            addRock(ch, x, z, size, 0.3, { yaw: roadYaw });
          }
          // Larger slabs set into the face above.
          if (rng() < 0.16) {
            const size = lerp(2.5, 7, rng());
            const [x, z] = at(wall + 4 + rng() * 12);
            addRock(ch, x, z, size, 0.5, { stretch: lerp(1.2, 2.2, rng()), flat: lerp(0.5, 0.9, rng()), yaw: roadYaw, onSlope: true });
          }
        } else if (rng() < 0.12) {
          const size = lerp(0.6, 2.2, Math.pow(rng(), 2));
          const [x, z] = at(wall + 2 + rng() * 10);
          addRock(ch, x, z, size, 0.35);
        }
        // Scree along the shoulder, thickest below the rock walls.
        const nScree = kind === 1 ? 1 + (rng() < 0.45 ? 1 : 0) : rng() < 0.25 ? 1 : 0;
        for (let k = 0; k < nScree; k++) {
          const size = lerp(0.1, 0.5, Math.pow(rng(), 1.5));
          const [x, z] = at(wall + 0.7 + size + rng() * rng() * 4);
          addRock(scree, x, z, size, 0.25);
        }
      }
    }

    // Outcrops on steep slopes away from the road give the cliffs texture.
    const b = t.bounds;
    const x1 = this.terrain.x1 + 150;
    for (let x = b.minX - 450; x < x1; x += 11) {
      for (let z = b.minZ - 450; z < b.maxZ + 450; z += 11) {
        const px = x + (rng() - 0.5) * 9, pz = z + (rng() - 0.5) * 9;
        const r0 = rng();
        if (r0 > 0.5) continue;
        const wm = this.terrain.zoneWeights(px)[0];
        if (wm < 0.5) continue;
        const far = this.terrain.far(px, pz);
        if (far.d < 14 || far.d > 320 || far.s > this.mEnd + 100) continue;
        this.surf.sample(px, pz, S);
        if (S.slope < 0.85) continue;
        if (r0 > 0.22 * smoothstep(0.85, 1.6, S.slope)) continue;
        const size = lerp(3, 9, Math.pow(rng(), 1.5)) * (far.d < 60 ? 0.6 : 1);
        const ci = clamp(Math.floor(far.s / CHUNK), 0, nChunks - 1);
        addRock(far.d < 45 ? big[ci] : outcrops[ci], px, pz, size, 0.42, { stretch: lerp(1.0, 1.8, rng()), flat: lerp(0.7, 1.1, rng()), onSlope: true, dark: true });
      }
    }

    const mat = rockMaterial();
    const geoBig = [rockGeometry(3, 2, { crag: true }), rockGeometry(17, 2, { crag: true, squash: 0.8 })];
    const geoFar = rockGeometry(5, 1, { crag: true });
    const geoSmall = rockGeometry(9, 0, { squash: 0.8, lichen: 0.4 });
    big.forEach((items, k) => {
      if (!items.length) return;
      this.group.add(instanced(geoBig[k & 1], mat, items, { cast: true }));
    });
    for (let k = 0; k < nChunks; k += 2) {
      const items = outcrops[k].concat(outcrops[k + 1] || []);
      if (items.length) this.group.add(instanced(geoFar, mat, items, { cast: true }));
    }
    if (scree.length) this.group.add(instanced(geoSmall, mat, scree, { cast: false }));
    this.rockMat = mat;
    this.rockCount = big.reduce((a, l) => a + l.length, 0) + outcrops.reduce((a, l) => a + l.length, 0) + scree.length;
  }

  // ── Forest ─────────────────────────────────────────────────────────
  buildForest() {
    const t = this.track, T = this.terrain, rng = mulberry32(777);
    const n2 = T.noise2;
    const nChunks = Math.ceil(this.mEnd / CHUNK) + 1;
    // Three levels of detail, chosen by distance from the road (the camera
    // never leaves it): full trees on the verge, simpler ones in the forest
    // behind, and a few-triangle version for the far hillsides.
    const VERGE = 75;
    const verge = { spruce: Array.from({ length: nChunks }, () => []), fir: Array.from({ length: nChunks }, () => []) };
    const mid = Array.from({ length: nChunks }, () => []);
    const farQuads = new Map();
    const S = {};
    // Tints over the baked greens: most trees neutral, some bluer (spruce),
    // some olive or sun-scorched, a few darker.
    const greens = [0xffffff, 0xf0f4e8, 0xdde8ea, 0xfff2d0, 0xc9d6c4, 0xe6efff, 0xb8c4b0, 0xf5e8c8];
    const forestMask = (x, z) => smoothstep(0.1, 0.35, fbm(n2, x / 420 + 7, z / 420, 3));
    // Stands of one species rather than salt-and-pepper mixing.
    const firMask = (x, z) => fbm(n2, x / 160 - 3, z / 160 + 11, 2) > 0.08;
    const b = t.bounds;
    const x1 = T.x1 + 250;

    // Detailed pines near the road.
    for (let x = b.minX - NEAR_FOREST; x < x1; x += 5) {
      for (let z = b.minZ - NEAR_FOREST; z < b.maxZ + NEAR_FOREST; z += 5) {
        const px = x + (rng() - 0.5) * 4.5, pz = z + (rng() - 0.5) * 4.5;
        const r = rng();
        const wm = T.zoneWeights(px)[0];
        if (wm <= 0.02) continue;
        const f = forestMask(px, pz);
        let p = (0.03 + f * 0.55) * wm;
        const far = T.far(px, pz);
        if (far.d > NEAR_FOREST || far.s > this.mEnd + 150) continue;
        // Forest crowds the road where the ground allows it.
        p *= 1 + 0.9 * (1 - smoothstep(12, 70, far.d)) * smoothstep(0.02, 0.2, f + 0.1);
        if (r > p) continue;
        if (this.excluded(px, pz) || !this.clearOfRoad(px, pz, 2.2)) continue;
        this.surf.sample(px, pz, S);
        if (S.slope > 0.8) continue;
        const line = 330 + fbm(n2, px / 300, pz / 300, 2) * 60;
        const alt = smoothstep(line - 50, line + 15, S.h);
        if (rng() > 1 - alt) continue;
        // Stunted near the tree line.
        const sc = lerp(0.7, 1.9, Math.pow(rng(), 1.3)) * (0.8 + f * 0.35) * (1 - alt * 0.35);
        const ci = clamp(Math.floor(far.s / CHUNK), 0, nChunks - 1);
        const fir = firMask(px, pz) ? rng() < 0.8 : rng() < 0.15;
        const list = far.d < VERGE ? (fir ? verge.fir : verge.spruce)[ci] : mid[ci];
        list.push({
          x: px, z: pz, y: S.h - 0.3,
          sx: sc * lerp(0.8, 1.15, rng()), sy: sc * lerp(0.9, 1.3, rng()) * (fir && far.d >= VERGE ? 1.1 : 1), sz: sc * lerp(0.8, 1.15, rng()),
          ry: rng() * 6.28, rx: (rng() - 0.5) * 0.06, rz: (rng() - 0.5) * 0.06,
          col: greens[Math.floor(rng() * greens.length)], b: lerp(0.8, 1.15, rng()),
        });
      }
    }

    // Distant forest cover: a two-tier silhouette per tree on a finer grid,
    // so the far slopes read as continuous forest with ragged edges.
    const minX = T.minX + 200, maxX = x1, minZ = T.minZ + 200, maxZ = T.maxZ - 200;
    for (let x = minX; x < maxX; x += 12.5) {
      for (let z = minZ; z < maxZ; z += 12.5) {
        const px = x + (rng() - 0.5) * 11, pz = z + (rng() - 0.5) * 11;
        const r = rng();
        const wm = T.zoneWeights(px)[0];
        if (wm <= 0.02) continue;
        const f = forestMask(px, pz);
        const p = (0.012 + f * 0.62) * wm;
        if (r > p) continue;
        const far = T.far(px, pz);
        if (far.d <= NEAR_FOREST || far.d > 2300) continue;
        this.surf.sample(px, pz, S);
        if (S.slope > 0.75) continue;
        const line = 340 + fbm(n2, px / 300, pz / 300, 2) * 70;
        if (S.h > line + rng() * 30) continue;
        const sc = lerp(1.1, 2.1, rng()) * (S.h > line - 40 ? 0.75 : 1);
        const key = Math.floor((px - T.minX) / 3000) + ',' + Math.floor((pz - T.minZ) / 3000);
        if (!farQuads.has(key)) farQuads.set(key, []);
        farQuads.get(key).push({
          x: px, z: pz, y: S.h - 0.5,
          sx: sc, sy: sc * lerp(0.85, 1.25, rng()), sz: sc, ry: rng() * 6.28,
          col: greens[Math.floor(rng() * greens.length)], b: lerp(0.75, 1.1, rng()),
        });
      }
    }

    const mat = foliageMaterial();
    const geoS = coniferGeometry('spruce', 0, 11), geoF = coniferGeometry('fir', 0, 12), geoMid = coniferGeometry('spruce', 1, 13);
    for (let k = 0; k < nChunks; k++) {
      if (verge.spruce[k].length) this.group.add(instanced(geoS, mat, verge.spruce[k], { cast: true }));
      if (verge.fir[k].length) this.group.add(instanced(geoF, mat, verge.fir[k], { cast: true }));
    }
    // The mid-distance forest is cheap per tree; pairs of chunks share a mesh.
    for (let k = 0; k < nChunks; k += 2) {
      const items = mid[k].concat(mid[k + 1] || []);
      if (items.length) this.group.add(instanced(geoMid, mat, items, { cast: true }));
    }
    const farGeo = coniferGeometry('spruce', 2, 14);
    const farMat = new THREE.MeshLambertMaterial({ vertexColors: true, side: THREE.DoubleSide });
    for (const items of farQuads.values()) this.group.add(instanced(farGeo, farMat, items, { cast: false, receive: false }));
    this.treeCount = mid.reduce((a, l) => a + l.length, 0) + verge.spruce.reduce((a, l) => a + l.length, 0) + verge.fir.reduce((a, l) => a + l.length, 0);
    this.farTreeCount = [...farQuads.values()].reduce((a, l) => a + l.length, 0);
  }

  // ── Verge: grass tussocks, wildflowers, shrubs ──────────────────
  // Ground cover on the shoulders and the open slopes just off the road,
  // where the chase camera sees it streak past. Clumps are tiny instanced
  // templates, chunked coarsely since each chunk costs a draw call.
  buildVerge() {
    const t = this.track, T = this.terrain, rng = mulberry32(9191);
    const n2 = T.noise2;
    const CH = CHUNK * 2;
    const nChunks = Math.ceil(this.mEnd / CH) + 1;
    const grass = Array.from({ length: nChunks }, () => []);
    const flowers = Array.from({ length: nChunks }, () => []);
    const shrubs = Array.from({ length: nChunks }, () => []);
    const S = {}, H = {};
    const straw = [0xffffff, 0xf2e6c4, 0xe8dcb0, 0xfff4dc];
    const green = [0x9fbf6a, 0xb4c878, 0x8cae62, 0xc8d090];
    const bloom = [0xffd23a, 0xfff4e0, 0x9a6ee0, 0xff8a2a, 0x6e8ef0, 0xffe070, 0xe86aa0];
    const shrubCol = [0x5d6e3a, 0x4f6034, 0x6f7442, 0x57663c, 0x7a6e44];
    for (let s = 6; s < this.mEnd + 60; s += 2.5) {
      const i = t.idx(s);
      t.frame(s, S);
      const ci = clamp(Math.floor(s / CH), 0, nChunks - 1);
      for (const side of [-1, 1]) {
        const kind = side < 0 ? t.sideL[i] : t.sideR[i];
        const wall = side < 0 ? t.wallL[i] : t.wallR[i];
        const tries = kind === 1 ? (rng() < 0.4 ? 1 : 0) : 5;
        for (let k = 0; k < tries; k++) {
          const lat = side * (wall + 0.9 + rng() * rng() * (kind === 1 ? 2.5 : 26));
          const x = S.x + S.rx * lat + S.fx * (rng() - 0.5) * 2.5, z = S.z + S.rz * lat + S.fz * (rng() - 0.5) * 2.5;
          if (this.excluded(x, z) || !this.clearOfRoad(x, z, 0.3)) continue;
          this.surf.sample(x, z, H);
          if (H.slope > 1.1 || H.h > 300) continue;
          // Patches: lush where the noise says damp, straw-dry elsewhere.
          const wet = fbm(n2, x / 60 + 5, z / 60, 2);
          const sc = lerp(0.85, 1.7, rng()) * (H.h > 270 ? 0.7 : 1);
          grass[ci].push({
            x, z, y: H.h - 0.05, sx: sc, sy: sc * lerp(0.8, 1.3, rng()), sz: sc, ry: rng() * 6.28,
            rx: -H.gz * 0.5, rz: H.gx * 0.5,
            col: (wet > 0.1 ? green : straw)[Math.floor(rng() * 4)], b: lerp(0.8, 1.1, rng()),
          });
          // Drifts of wildflowers in the damper patches.
          if (wet > 0.05 && rng() < 0.35 && H.h < 285) {
            const f = Math.floor((fbm(n2, x / 25, z / 25 + 9, 2) * 0.5 + 0.5) * bloom.length * 1.6) % bloom.length;
            flowers[ci].push({ x: x + (rng() - 0.5), z: z + (rng() - 0.5), y: H.h - 0.02, sx: 1, sy: lerp(0.8, 1.2, rng()), sz: 1, ry: rng() * 6.28, col: bloom[f], b: 1 });
          }
        }
        // Shrubs a little further out, some tucked against the rock walls.
        if (rng() < (kind === 1 ? 0.06 : 0.22)) {
          const lat = side * (wall + (kind === 1 ? 1.4 : 2.5 + rng() * 22));
          const x = S.x + S.rx * lat, z = S.z + S.rz * lat;
          const sz = lerp(0.6, 1.6, Math.pow(rng(), 1.4));
          if (this.excluded(x, z) || !this.clearOfRoad(x, z, sz)) continue;
          this.surf.sample(x, z, H);
          if (H.slope > 1.0 || H.h > 295) continue;
          shrubs[ci].push({ x, z, y: H.h - 0.15 * sz, sx: sz * lerp(1, 1.6, rng()), sy: sz, sz: sz * lerp(1, 1.4, rng()), ry: rng() * 6.28, col: shrubCol[Math.floor(rng() * shrubCol.length)], b: lerp(0.8, 1.2, rng()) });
        }
      }
    }
    const mat = foliageMaterial();
    const gGeo = grassClumpGeometry(11, 5), fGeo = flowerGeometry(5, 9), sGeo = shrubGeometry(21);
    const shrubMat = foliageMaterial({ side: THREE.FrontSide });
    for (let k = 0; k < nChunks; k++) {
      if (grass[k].length) this.group.add(instanced(gGeo, mat, grass[k]));
    }
    for (let k = 0; k < nChunks; k += 2) {
      const f = flowers[k].concat(flowers[k + 1] || []), sh = shrubs[k].concat(shrubs[k + 1] || []);
      if (f.length) this.group.add(instanced(fGeo, mat, f));
      if (sh.length) this.group.add(instanced(sGeo, shrubMat, sh, { cast: true }));
    }
    this.vergeCount = [grass, flowers, shrubs].map((l) => l.reduce((a, c) => a + c.length, 0));
  }

  // ── Snow patches at the summit ─────────────────────────────────────
  // Irregular discs draped on the rendered terrain wherever it rises above a
  // ragged snow line, on the gentler ground. One merged mesh.
  buildSnow() {
    const t = this.track, T = this.terrain, rng = mulberry32(606);
    const n2 = T.noise2;
    const H = {};
    const pos = [], col = [], idx = [];
    // mound > 0 heaps the middle up instead of lying flat.
    const patch = (cx, cz, R, lift = 0.12, mound = 0) => {
      const RINGS = 3, SEG = 11;
      const base = pos.length / 3;
      const ph = rng() * 10;
      this.surf.sample(cx, cz, H);
      pos.push(cx, H.h + lift + mound, cz); col.push(1, 1, 1);
      for (let r = 1; r <= RINGS; r++) {
        for (let j = 0; j < SEG; j++) {
          const a = (j / SEG) * Math.PI * 2;
          const edge = 0.65 + 0.35 * Math.sin(a * 3 + ph) * Math.cos(a * 2 - ph * 0.7) + (rng() - 0.5) * 0.25;
          const rr = R * (r / RINGS) * (r === RINGS ? edge : lerp(1, edge, 0.5));
          const x = cx + Math.cos(a) * rr, z = cz + Math.sin(a) * rr;
          this.surf.sample(x, z, H);
          pos.push(x, H.h + lift * (r === RINGS ? 0.4 : 1) + mound * (1 - Math.pow(r / RINGS, 2)), z);
          // Thin, dirty edge; clean, bright middle.
          const v = r === RINGS ? 0.62 : r === RINGS - 1 ? 0.88 : 1;
          col.push(v, v, v * 1.02);
        }
      }
      for (let j = 0; j < SEG; j++) idx.push(base, base + 1 + ((j + 1) % SEG), base + 1 + j);
      for (let r = 1; r < RINGS; r++) {
        const a0 = base + 1 + (r - 1) * SEG, b0 = base + 1 + r * SEG;
        for (let j = 0; j < SEG; j++) {
          const j1 = (j + 1) % SEG;
          idx.push(a0 + j, a0 + j1, b0 + j, a0 + j1, b0 + j1, b0 + j);
        }
      }
    };
    // Hillside patches around the summit.
    const sm = t.tag('summit')[0];
    const sMid = sm ? (sm.s0 + sm.s1) / 2 : this.mEnd * 0.6;
    const c = t.frame(sMid, {});
    let n = 0;
    for (let k = 0; k < 8000 && n < 320; k++) {
      const x = c.x + (rng() - 0.5) * 2400, z = c.z + (rng() - 0.5) * 2400;
      if (T.zoneWeights(x)[0] < 0.6) continue;
      this.surf.sample(x, z, H);
      const line = 292 + fbm(n2, x / 180, z / 180 + 4, 3) * 40;
      if (H.h < line || H.slope > 0.55) continue;
      if (!this.clearOfRoad(x, z, 25) || this.excluded(x, z)) continue;
      // Only on ground that stays gentle across the whole patch; on a steep
      // face a draped disc reads as a white shard.
      const R = lerp(4, 14, Math.pow(rng(), 1.5));
      const h0 = H.h;
      let steep = false;
      for (let j = 0; j < 6 && !steep; j++) {
        const a = (j / 6) * Math.PI * 2;
        if (Math.abs(this.surf.sample(x + Math.cos(a) * R, z + Math.sin(a) * R, H).h - h0) > R * 0.45) steep = true;
      }
      if (steep) continue;
      patch(x, z, R);
      n++;
    }
    if (!idx.length) return;
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
    g.setAttribute('color', new THREE.Float32BufferAttribute(col, 3));
    g.setIndex(idx);
    g.computeVertexNormals();
    const nrm = g.getAttribute('normal');
    for (let i = 0; i < nrm.count; i++) if (nrm.getY(i) < 0) nrm.setXYZ(i, -nrm.getX(i), -nrm.getY(i), -nrm.getZ(i));
    const mat = new THREE.MeshStandardMaterial({ color: 0xb8c0cc, roughness: 0.75, vertexColors: true, polygonOffset: true, polygonOffsetFactor: -2, polygonOffsetUnits: -2 });
    const m = new THREE.Mesh(g, mat);
    m.receiveShadow = true;
    m.matrixAutoUpdate = false;
    this.group.add(m);
    this.snowPatches = n;
  }

  // ── Signs ──────────────────────────────────────────────────────────
  buildSigns() {
    const t = this.track;
    const atlas = new SignAtlas();
    const signs = []; // {s, rect, w, h, height, side, lat?}
    const add = (s, rect, w, h, height = 2.1, side = 1, extraPlate = null) => signs.push({ s, rect, w, h, height, side, extraPlate, diamond: !!rect.diamond });

    const mph = (n) => atlas.add(200, 240, (g, w, h) => panel(g, w, h, '#f5c518', '#111', '#111', ['' + n, 'MPH'], [110, 52]));
    const tagD = (r) => { r.diamond = true; return r; };
    const hairpin = (dir) => tagD(atlas.add(256, 256, (g, w, h) => diamond(g, w, h, (g2, cx, cy) => {
      g2.save();
      g2.translate(cx, cy);
      g2.scale(dir, 1);
      g2.lineWidth = 16; g2.lineCap = 'butt';
      g2.beginPath();
      g2.moveTo(22, 62); g2.lineTo(22, -8);
      g2.arc(-4, -8, 26, 0, Math.PI, true);
      g2.lineTo(-30, 28);
      g2.stroke();
      g2.beginPath(); g2.moveTo(-52, 22); g2.lineTo(-8, 22); g2.lineTo(-30, 58); g2.closePath(); g2.fill();
      g2.restore();
    })));
    const textDiamond = (lines, size) => tagD(atlas.add(256, 256, (g, w, h) => diamond(g, w, h, (g2, cx, cy) => {
      g2.textAlign = 'center'; g2.textBaseline = 'middle'; g2.font = `bold ${size}px ${FONT}`;
      lines.forEach((l, i) => g2.fillText(l, cx, cy + (i - (lines.length - 1) / 2) * size * 1.05));
    })));
    const hairpinL = hairpin(-1), hairpinR = hairpin(1);
    const pin15 = mph(15);
    for (const hp of t.tag('hairpin')) {
      add(hp.s0 - 75, hp.turn < 0 ? hairpinL : hairpinR, 1.2, 1.2, 2.5, 1, { rect: pin15, w: 0.75, h: 0.9 });
    }
    const sb = t.tag('switchbacks')[0];
    if (sb) {
      const r = atlas.add(512, 256, (g, w, h) => panel(g, w, h, '#f5c518', '#111', '#111', ['SWITCHBACKS', 'NEXT 2 MILES'], [70, 58]));
      add(sb.s0 - 170, r, 2.6, 1.3, 2.0);
    }
    const cy = t.tag('canyon')[0];
    if (cy) add(cy.s0 - 50, textDiamond(['FALLING', 'ROCKS'], 44), 1.2, 1.2, 2.4);
    const sm = t.tag('summit')[0];
    if (sm) {
      const r = atlas.add(512, 288, (g, w, h) => panel(g, w, h, '#5a3a22', '#f3ead8', '#f3ead8', ['SIERRA PASS', 'SUMMIT', 'ELEV 7,240 FT'], [58, 44, 48]));
      add(sm.s0 + 30, r, 2.8, 1.575, 2.0);
    }
    const ds = t.tag('descent')[0];
    if (ds) {
      const r = atlas.add(512, 256, (g, w, h) => panel(g, w, h, '#f5c518', '#111', '#111', ['TRUCKS', 'USE LOW GEAR'], [70, 58]));
      add(ds.s0 - 60, r, 2.4, 1.2, 2.0);
      add(ds.s0 + 40, textDiamond(['7%', 'GRADE'], 58), 1.2, 1.2, 2.4);
      const g2 = atlas.add(512, 256, (g, w, h) => {
        panel(g, w, h, '#0b6b3a', '#fff', '#fff', [], []);
        g.fillStyle = '#fff'; g.font = `bold 58px ${FONT}`; g.textBaseline = 'middle';
        g.textAlign = 'left'; g.fillText('Mill Valley', 36, 88); g.fillText('Meridian', 36, 172);
        g.textAlign = 'right'; g.fillText('3', w - 36, 88); g.fillText('9', w - 36, 172);
      });
      add(ds.s0 + 420, g2, 3.2, 1.6, 2.2);
    }
    const lim = atlas.add(200, 256, (g, w, h) => {
      panel(g, w, h, '#f8f8f4', '#111', '#111', ['SPEED', 'LIMIT', '45'], [44, 44, 96]);
    });
    add(240, lim, 0.9, 1.15, 2.1);
    add(1650, lim, 0.9, 1.15, 2.1);

    const tex = atlas.texture();
    const faceMat = new THREE.MeshStandardMaterial({ map: tex, roughness: 0.45, metalness: 0.1, emissive: 0xffffff, emissiveMap: tex, emissiveIntensity: 0.04, alphaTest: 0.5 });
    this.world.addNight(faceMat, 'emissiveIntensity', 0.04, 0.55);
    const metalMat = new THREE.MeshStandardMaterial({ color: 0x8d9197, metalness: 0.6, roughness: 0.45 });
    const faces = [], metal = [];
    const S = {}, H = {};
    const plate = (rect, w, h, x, y, z, yaw, isDiamond = !!rect.diamond) => {
      const g = new THREE.PlaneGeometry(w, h);
      const uv = g.getAttribute('uv');
      for (let k = 0; k < uv.count; k++) uv.setXY(k, lerp(rect.u0, rect.u1, uv.getX(k)), lerp(rect.v0, rect.v1, uv.getY(k)));
      faces.push(placed(g, x, y, z, yaw));
      const back = isDiamond ? new THREE.PlaneGeometry(w * 0.69, h * 0.69).rotateZ(Math.PI / 4) : new THREE.PlaneGeometry(w, h);
      back.rotateY(Math.PI);
      back.translate(0, 0, -0.02);
      metal.push(placed(back, x, y, z, yaw));
    };
    for (const sg of signs) {
      t.frame(sg.s, S);
      const i = t.idx(sg.s);
      const wall = sg.side > 0 ? t.wallR[i] : t.wallL[i];
      const kind = sg.side > 0 ? t.sideR[i] : t.sideL[i];
      // Keep the whole plate outside the corridor, not just the post.
      const lat = sg.side * (wall + (kind === 1 ? 0.75 : 1.1) + sg.w * 0.5);
      const x = S.x + S.rx * lat, z = S.z + S.rz * lat;
      const roadY = S.y - lat * S.bank;
      this.surf.sample(x, z, H);
      const base = clamp(H.h, roadY - 4, roadY + 0.6);
      const yaw = Math.atan2(-S.fx, -S.fz) + sg.side * 0.12;
      const top = roadY + sg.height + sg.h;
      let bottom = roadY + sg.height;
      if (sg.extraPlate) {
        const ep = sg.extraPlate;
        plate(ep.rect, ep.w, ep.h, x, bottom - 0.08 - ep.h / 2, z, yaw);
        bottom -= ep.h + 0.1;
      }
      plate(sg.rect, sg.w, sg.h, x, roadY + sg.height + sg.h / 2, z, yaw);
      const postH = top - 0.1 - base;
      const post = new THREE.CylinderGeometry(0.045, 0.045, postH, 6);
      post.translate(0, base + postH / 2, 0);
      const dx = -Math.sin(yaw) * 0.05, dz = -Math.cos(yaw) * 0.05;
      metal.push(placed(post, x + dx, 0, z + dz, yaw));
      if (sg.w > 2) {
        const off = sg.w * 0.32;
        const px2 = Math.cos(yaw) * off, pz2 = -Math.sin(yaw) * off;
        metal.pop();
        metal.push(placed(post, x + dx + px2, 0, z + dz + pz2, yaw), placed(post, x + dx - px2, 0, z + dz - pz2, yaw));
      }
    }
    const fm = mergedMesh(faces, faceMat);
    const mm = mergedMesh(metal, metalMat, { cast: true });
    if (fm) this.group.add(fm);
    if (mm) this.group.add(mm);
  }

  buildSnowPoles() {
    const t = this.track, S = {};
    const items = [];
    for (let s = 200; s < this.mEnd - 100; s += 16) {
      t.frame(s, S);
      if (S.y < 215) continue;
      const side = (Math.floor(s / 16) & 1) ? 1 : -1;
      const i = t.idx(s);
      const lat = side * ((side > 0 ? t.wallR[i] : t.wallL[i]) + 0.75);
      const x = S.x + S.rx * lat, z = S.z + S.rz * lat;
      if (!this.clearOfRoad(x, z, 0.1)) continue;
      const ground = this.surf.sample(x, z).h;
      items.push({ x, z, y: Math.min(S.y - lat * S.bank - 0.2, ground - 0.05), sx: 1, sy: 1, sz: 1 });
    }
    if (!items.length) return;
    const [c, g] = makeCanvas(8, 64);
    for (let k = 0; k < 8; k++) { g.fillStyle = k % 2 ? '#111' : '#ff6a13'; g.fillRect(0, k * 8, 8, 8); }
    const tex = canvasTex(c);
    const geo = new THREE.CylinderGeometry(0.035, 0.035, 2.4, 5, 1, true);
    geo.translate(0, 1.2, 0);
    const mat = new THREE.MeshStandardMaterial({ map: tex, roughness: 0.6, emissive: 0xffffff, emissiveMap: tex, emissiveIntensity: 0.05 });
    this.world.addNight(mat, 'emissiveIntensity', 0.05, 0.5);
    this.group.add(instanced(geo, mat, items));
  }

  // Roadside delineators below the snow-pole line: white posts with a black
  // band and a reflector (amber on the right, white on the left, as seen
  // by the driver) that lights up in headlights after dark.
  buildDelineators() {
    const t = this.track, S = {};
    const posts = [], refl = [];
    for (let s = 40; s < this.mEnd - 60; s += 33) {
      t.frame(s, S);
      if (S.y >= 215) continue;
      const i = t.idx(s);
      for (const side of [-1, 1]) {
        const kind = side < 0 ? t.sideL[i] : t.sideR[i];
        if (kind === 1) continue;
        const lat = side * ((side > 0 ? t.wallR[i] : t.wallL[i]) + 0.8);
        const x = S.x + S.rx * lat, z = S.z + S.rz * lat;
        if (!this.clearOfRoad(x, z, 0.05)) continue;
        const ground = this.surf.sample(x, z).h;
        const y = Math.min(S.y - lat * S.bank - 0.1, ground - 0.02);
        const ry = Math.atan2(S.fx, S.fz) + Math.PI;
        posts.push({ x, y, z, sx: 1, sy: 1, sz: 1, ry });
        refl.push({ x, y, z, sx: 1, sy: 1, sz: 1, ry, col: side > 0 ? 0xffa726 : 0xf4f6ff });
      }
    }
    if (!posts.length) return;
    const post = new THREE.BoxGeometry(0.11, 1.15, 0.07);
    post.translate(0, 0.575, 0);
    const band = new THREE.BoxGeometry(0.115, 0.14, 0.075);
    band.translate(0, 1.0, 0);
    const colorOf = (g, v) => { const n = g.getAttribute('position').count; g.setAttribute('color', new THREE.BufferAttribute(new Float32Array(n * 3).fill(v), 3)); return g; };
    const postGeo = mergeGeometries([colorOf(post.toNonIndexed(), 0.85), colorOf(band.toNonIndexed(), 0.02)], false);
    const postMat = new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.55 });
    this.group.add(instanced(postGeo, postMat, posts));
    // Reflector faces the approaching traffic (local +Z after the yaw).
    const r = new THREE.PlaneGeometry(0.07, 0.16);
    r.translate(0, 0.8, 0.04);
    const reflMat = new THREE.MeshStandardMaterial({ color: 0xffffff, emissive: 0xffffff, emissiveIntensity: 0.1, roughness: 0.3 });
    reflMat.onBeforeCompile = (sh) => {
      // Emissive takes the instance colour so one material does amber and white.
      sh.fragmentShader = sh.fragmentShader.replace('#include <emissivemap_fragment>', '#include <emissivemap_fragment>\n#ifdef USE_INSTANCING_COLOR\ntotalEmissiveRadiance *= vColor;\n#endif');
    };
    this.world.addNight(reflMat, 'emissiveIntensity', 0.1, 1.6);
    this.group.add(instanced(r, reflMat, refl, { receive: false }));
  }

  // ── Start gantry ───────────────────────────────────────────────────
  buildStartArea() {
    const t = this.track, S = {};
    const s = t.startS;
    t.frame(s, S);
    const wl = t.wallL[t.idx(s)], wr = t.wallR[t.idx(s)];
    const yaw = Math.atan2(S.fx, S.fz); // local +z → forward
    const steel = new THREE.MeshStandardMaterial({ color: 0x2b2f36, metalness: 0.7, roughness: 0.35 });
    const geos = [];
    const H = 7.2;
    for (const lat of [-(wl + 1.25), wr + 1.25]) {
      const x = S.x + S.rx * lat, z = S.z + S.rz * lat;
      const y = S.y - lat * S.bank;
      // Truss-style leg: two uprights with rungs.
      for (const off of [-0.35, 0.35]) {
        const up = new THREE.BoxGeometry(0.16, H + 1.5, 0.16);
        up.translate(0, (H + 1.5) / 2 - 1.2, off);
        geos.push(placed(up, x, y, z, yaw));
      }
      for (let k = 0; k < 7; k++) {
        const rung = new THREE.BoxGeometry(0.08, 0.08, 0.7);
        rung.translate(0, 0.5 + k * 1.0, 0);
        geos.push(placed(rung, x, y, z, yaw));
      }
      const foot = new THREE.BoxGeometry(1.1, 0.5, 1.4);
      foot.translate(0, -0.05, 0);
      geos.push(placed(foot, x, y, z, yaw));
    }
    const span = wl + wr + 2.5;
    const cx = S.x + S.rx * ((wr - wl) / 2), cz = S.z + S.rz * ((wr - wl) / 2);
    for (const dy of [H, H - 0.9]) {
      const beam = new THREE.BoxGeometry(span, 0.18, 0.18);
      beam.translate(0, S.y + dy, 0);
      geos.push(placed(beam, cx, 0, cz, yaw));
    }
    const m = mergedMesh(geos, steel, { cast: true });
    this.group.add(m);

    // Banner: front says SIERRA PASS, back (seen when looking back) START.
    const bannerTex = (front) => {
      const [c, g] = makeCanvas(1024, 160);
      g.fillStyle = '#0d0f14'; g.fillRect(0, 0, 1024, 160);
      const sq = 20;
      for (let y = 0; y < 160; y += sq) for (let x = 0; x < 120; x += sq) {
        g.fillStyle = ((x + y) / sq) % 2 ? '#f2f2f2' : '#111';
        g.fillRect(x, y, sq, sq); g.fillRect(1024 - 120 + x, y, sq, sq);
      }
      g.fillStyle = '#ff3860'; g.fillRect(120, 0, 784, 8); g.fillRect(120, 152, 784, 8);
      g.fillStyle = '#fff'; g.textAlign = 'center'; g.textBaseline = 'middle';
      g.font = `italic 900 84px ${FONT}`;
      g.fillText(front ? 'SIERRA PASS' : 'START', 512, 70);
      g.font = `bold 26px ${FONT}`; g.fillStyle = '#ffb347';
      g.fillText(front ? 'STAGE 1 · MIDNIGHT RACER' : 'SIERRA PASS', 512, 132);
      return canvasTex(c);
    };
    const bw = span - 1.6, bh = bw * 160 / 1024;
    for (const front of [true, false]) {
      const tex = bannerTex(front);
      const mat = new THREE.MeshStandardMaterial({ map: tex, roughness: 0.7, emissive: 0xffffff, emissiveMap: tex, emissiveIntensity: 0.15 });
      const g = new THREE.PlaneGeometry(bw, bh);
      // Front faces approaching cars (−forward); back faces forward.
      g.rotateY(front ? Math.PI : 0);
      g.translate(0, S.y + H + 0.1 - bh / 2, front ? -0.05 : 0.05);
      const mesh = new THREE.Mesh(placed(g, cx, 0, cz, yaw), mat);
      mesh.matrixAutoUpdate = false;
      this.group.add(mesh);
    }
  }

  // ── Roadside diner at the foot of the pass ─────────────────────────
  async buildDiner() {
    const d = this.diner;
    if (!d) return;
    const t = this.track, S = {};
    t.frame(d.s, S);
    const y = d.y;
    // Building faces the road: its local +z points back toward the road.
    const yaw = Math.atan2(-S.rx * d.side, -S.rz * d.side);
    const fwdYaw = Math.atan2(S.fx, S.fz);
    const g0 = new THREE.CircleGeometry(16, 28);
    g0.rotateX(-Math.PI / 2);
    g0.translate(0, y + 0.06, 0);
    const gravel = new THREE.MeshStandardMaterial({ map: gravelTexture(), roughness: 1, color: 0xb0a898, polygonOffset: true, polygonOffsetFactor: -1 });
    gravel.map = gravelTexture().clone();
    gravel.map.repeat.set(6, 6);
    gravel.map.needsUpdate = true;
    const pad = new THREE.Mesh(placed(g0, d.x, 0, d.z), gravel);
    pad.receiveShadow = true;
    this.group.add(pad);

    const siding = new THREE.MeshStandardMaterial({ color: 0xd8ccb0, roughness: 0.85 });
    const roof = new THREE.MeshStandardMaterial({ color: 0x5b3b2c, roughness: 0.9 });
    const trim = new THREE.MeshStandardMaterial({ color: 0xa3272c, roughness: 0.6 });
    const glass = new THREE.MeshStandardMaterial({ color: 0x3a3226, roughness: 0.2, metalness: 0.2, emissive: 0xffc27a, emissiveIntensity: 0.25 });
    this.world.addNight(glass, 'emissiveIntensity', 0.25, 1.6);
    const wallG = [], roofG = [], trimG = [], glassG = [];
    // Local frame: origin at pad centre, +z toward road.
    const bx = d.x + S.rx * d.side * 4, bz = d.z + S.rz * d.side * 4;
    const P = (geo, lx, ly, lz, extraYaw = 0) => { geo.rotateY(extraYaw); geo.translate(lx, ly, lz); return placed(geo, bx, y, bz, yaw); };
    wallG.push(P(new THREE.BoxGeometry(13, 4, 7), 0, 2, 0));
    // Gable roof as a triangular prism.
    const roofShape = new THREE.Shape();
    roofShape.moveTo(-4.1, 0); roofShape.lineTo(0, 2.2); roofShape.lineTo(4.1, 0); roofShape.closePath();
    const rg = new THREE.ExtrudeGeometry(roofShape, { depth: 14, bevelEnabled: false });
    rg.translate(0, 0, -7);
    rg.rotateY(Math.PI / 2);
    roofG.push(P(rg, 0, 4, 0));
    trimG.push(P(new THREE.BoxGeometry(13.2, 0.35, 7.2), 0, 3.9, 0));
    trimG.push(P(new THREE.BoxGeometry(13.1, 0.5, 7.1), 0, 0.25, 0));
    // Porch roof + posts.
    roofG.push(P(new THREE.BoxGeometry(13, 0.2, 2.6), 0, 3.3, 4.8));
    for (const px of [-6, -2, 2, 6]) trimG.push(P(new THREE.BoxGeometry(0.18, 3.2, 0.18), px, 1.6, 5.9));
    // Windows & door.
    for (const px of [-5, -2.6, 2.6, 5]) glassG.push(P(new THREE.PlaneGeometry(1.9, 1.5), px, 2.1, 3.52));
    glassG.push(P(new THREE.PlaneGeometry(1.2, 2.3), 0, 1.15, 3.52));
    // Gas canopy with two pumps, between diner and road.
    const cx0 = 9, cz0 = 7;
    roofG.push(P(new THREE.BoxGeometry(7, 0.5, 5), cx0, 4.6, cz0));
    trimG.push(P(new THREE.BoxGeometry(7.05, 0.3, 5.05), cx0, 4.3, cz0));
    for (const [a, b] of [[-3, -2], [3, -2], [-3, 2], [3, 2]]) wallG.push(P(new THREE.BoxGeometry(0.25, 4.4, 0.25), cx0 + a, 2.2, cz0 + b));
    for (const a of [-1.5, 1.5]) {
      wallG.push(P(new THREE.BoxGeometry(0.7, 1.6, 0.5), cx0 + a, 0.8, cz0));
      trimG.push(P(new THREE.BoxGeometry(0.72, 0.35, 0.52), cx0 + a, 1.55, cz0));
    }
    const lit = new THREE.MeshStandardMaterial({ color: 0xffffff, emissive: 0xfff2d8, emissiveIntensity: 0.4 });
    this.world.addNight(lit, 'emissiveIntensity', 0.4, 2.2);
    const litG = [P(new THREE.PlaneGeometry(6.4, 4.4).rotateX(Math.PI / 2), cx0, 4.34, cz0)];
    for (const [geos, mat] of [[wallG, siding], [roofG, roof], [trimG, trim], [glassG, glass], [litG, lit]]) {
      const m = mergedMesh(geos, mat, { cast: mat !== glass && mat !== lit });
      if (m) this.group.add(m);
    }

    // Neon roof sign + tall pole sign by the road.
    const neonTex = (() => {
      const [c, g] = makeCanvas(512, 128);
      g.fillStyle = '#1a0f10'; roundRect(g, 4, 4, 504, 120, 18); g.fill();
      g.strokeStyle = '#ff4a6a'; g.lineWidth = 6; roundRect(g, 12, 12, 488, 104, 14); g.stroke();
      g.font = `italic 900 78px ${FONT}`; g.textAlign = 'center'; g.textBaseline = 'middle';
      g.shadowColor = '#ff3860'; g.shadowBlur = 18; g.fillStyle = '#ffd1dc';
      g.fillText('DINER', 256, 68);
      return canvasTex(c);
    })();
    const neon = new THREE.MeshStandardMaterial({ map: neonTex, emissive: 0xffffff, emissiveMap: neonTex, emissiveIntensity: 0.7, roughness: 0.5, side: THREE.DoubleSide });
    this.world.addNight(neon, 'emissiveIntensity', 0.7, 2.6);
    const ns = new THREE.PlaneGeometry(5, 1.25);
    this.group.add(new THREE.Mesh(P(ns, 0, 5.9, 1.2), neon));
    const poleTex = (() => {
      const [c, g] = makeCanvas(256, 320);
      g.fillStyle = '#2b1d14'; roundRect(g, 4, 4, 248, 312, 20); g.fill();
      g.fillStyle = '#f1e4c8'; roundRect(g, 14, 14, 228, 292, 14); g.fill();
      g.fillStyle = '#7a2a1c'; g.textAlign = 'center'; g.textBaseline = 'middle';
      g.font = `900 46px ${FONT}`; g.fillText('PINE', 128, 62); g.fillText('RIDGE', 128, 110);
      g.fillStyle = '#2b1d14'; g.fillRect(34, 142, 188, 4);
      g.font = `bold 40px ${FONT}`; g.fillText('GAS · EATS', 128, 186);
      g.font = `bold 30px ${FONT}`; g.fillStyle = '#a3272c'; g.fillText('OPEN 24 HRS', 128, 240);
      g.font = `bold 24px ${FONT}`; g.fillStyle = '#2b1d14'; g.fillText('LAST STOP BEFORE PASS', 128, 280);
      return canvasTex(c);
    })();
    const poleMat = new THREE.MeshStandardMaterial({ map: poleTex, emissive: 0xffffff, emissiveMap: poleTex, emissiveIntensity: 0.12, roughness: 0.6 });
    this.world.addNight(poleMat, 'emissiveIntensity', 0.12, 1.2);
    const wr = this.track.wallR[this.track.idx(d.s + 22)];
    const sp = this.track.pointAt(d.s + 22, d.side * (wr + 2.2));
    const faceYaw = Math.atan2(-S.fx, -S.fz) + d.side * 0.5;
    const board = new THREE.PlaneGeometry(2.6, 3.25);
    board.translate(0, 6.2, 0);
    const boardBack = new THREE.PlaneGeometry(2.6, 3.25);
    boardBack.rotateY(Math.PI); boardBack.translate(0, 6.2, -0.05);
    const bm = new THREE.Mesh(placed(board, sp.x, sp.y, sp.z, faceYaw), poleMat);
    this.group.add(bm);
    const pg = new THREE.CylinderGeometry(0.14, 0.18, 7.8, 8);
    pg.translate(0, 3.9 - 0.8, -0.12);
    const poleMesh = mergedMesh([placed(pg, sp.x, sp.y, sp.z, faceYaw), placed(boardBack, sp.x, sp.y, sp.z, faceYaw)], new THREE.MeshStandardMaterial({ color: 0x3a2a1e, roughness: 0.8 }), { cast: true });
    this.group.add(poleMesh);

    // A parked pickup, if the vehicle module is present.
    try {
      const { buildVehicle } = await import('../vehicles/CarModel.js');
      const v = buildVehicle('pickup', { color: 0x6b7f5a, seed: 12, lod: 'low' });
      const lp = new THREE.Vector3(-8.5, 0, 6.5).applyAxisAngle(new THREE.Vector3(0, 1, 0), yaw);
      v.root.position.set(bx + lp.x, y + 0.05, bz + lp.z);
      v.root.rotation.y = yaw + 0.3;
      v.setHeadlights?.(0);
      this.group.add(bakeStatic(v.root));
    } catch (e) { /* optional */ }
  }

  // ── Summit lookout ─────────────────────────────────────────────────
  async buildLookout() {
    const L = this.lookout;
    if (!L) return;
    const t = this.track, S = {};
    t.frame(L.s, S);
    const out = { x: S.rx * L.side, z: S.rz * L.side }; // away from road
    const yaw = Math.atan2(out.x, out.z);
    const y = L.y;
    const gravel = new THREE.MeshStandardMaterial({ map: gravelTexture().clone(), roughness: 1, color: 0xa39c90, polygonOffset: true, polygonOffsetFactor: -1 });
    gravel.map.repeat.set(5, 5);
    gravel.map.needsUpdate = true;
    const g0 = new THREE.CircleGeometry(11.3, 28);
    g0.rotateX(-Math.PI / 2);
    g0.translate(L.x, y + 0.06, L.z);
    const pad = new THREE.Mesh(g0, gravel);
    pad.receiveShadow = true;
    this.group.add(pad);

    // Low stone wall around the outer edge.
    const stones = [];
    for (let a = -1.25; a <= 1.25; a += 0.085) {
      const r = 10.7;
      const lx = Math.sin(a) * r, lz = Math.cos(a) * r;
      const bxg = new THREE.BoxGeometry(1.2, 0.8 + (Math.abs(Math.sin(a * 13)) * 0.15), 0.7);
      bxg.rotateY(a);
      bxg.translate(lx, 0.4, lz);
      stones.push(placed(bxg, L.x, y, L.z, yaw));
    }
    const stoneMat = rockMaterial(false);
    stoneMat.color.setHex(0xb8ad9c);
    this.group.add(mergedMesh(stones, stoneMat, { cast: true }));

    // Coin-op viewer and a bench.
    const metal = new THREE.MeshStandardMaterial({ color: 0x3d6b4f, metalness: 0.6, roughness: 0.4 });
    const wood = new THREE.MeshStandardMaterial({ color: 0x6d5038, roughness: 0.9 });
    const mg = [], wg = [];
    const P = (geo, lx, ly, lz) => { geo.translate(lx, ly, lz); return placed(geo, L.x, y, L.z, yaw); };
    mg.push(P(new THREE.CylinderGeometry(0.08, 0.12, 1.1, 8), 2, 0.55, 9.6));
    mg.push(P(new THREE.BoxGeometry(0.5, 0.35, 0.7), 2, 1.25, 9.6));
    mg.push(P(new THREE.CylinderGeometry(0.09, 0.09, 0.4, 8).rotateX(Math.PI / 2), 1.88, 1.3, 10.05));
    mg.push(P(new THREE.CylinderGeometry(0.09, 0.09, 0.4, 8).rotateX(Math.PI / 2), 2.12, 1.3, 10.05));
    wg.push(P(new THREE.BoxGeometry(2.2, 0.08, 0.5), -3, 0.5, 8.4));
    wg.push(P(new THREE.BoxGeometry(2.2, 0.5, 0.08), -3, 0.8, 8.65));
    for (const lx of [-3.9, -2.1]) wg.push(P(new THREE.BoxGeometry(0.1, 0.5, 0.5), lx, 0.25, 8.4));
    // Flagpole.
    mg.push(P(new THREE.CylinderGeometry(0.05, 0.07, 8, 6), 6, 4, 6));
    this.group.add(mergedMesh(mg, metal, { cast: true }));
    this.group.add(mergedMesh(wg, wood, { cast: true }));
    const flag = new THREE.Mesh(new THREE.PlaneGeometry(1.8, 1.1, 8, 1), new THREE.MeshStandardMaterial({ color: 0xd6423a, side: THREE.DoubleSide, roughness: 0.8 }));
    const fp = new THREE.Vector3(6, 7.3, 6).applyAxisAngle(new THREE.Vector3(0, 1, 0), yaw);
    flag.position.set(L.x + fp.x, y + fp.y, L.z + fp.z);
    const pos = flag.geometry.getAttribute('position');
    const base = Float32Array.from(pos.array);
    for (let k = 0; k < pos.count; k++) pos.setX(k, base[k * 3] + 0.9);
    let ft = 0;
    this.world.updaters.push((dt, night, camera) => {
      if (camera && camera.position.distanceToSquared(flag.position) > 600 * 600) return;
      ft += dt;
      for (let k = 0; k < pos.count; k++) {
        const x = base[k * 3] + 0.9;
        pos.setZ(k, Math.sin(ft * 6 - x * 3) * 0.12 * x);
      }
      pos.needsUpdate = true;
      flag.rotation.y = yaw + 1.2 + Math.sin(ft * 0.7) * 0.2;
    });
    this.group.add(flag);

    try {
      const { buildVehicle } = await import('../vehicles/CarModel.js');
      const v = buildVehicle('sedan', { color: 0x2f4f7f, seed: 5, lod: 'low' });
      const lp = new THREE.Vector3(-5.5, 0, 3).applyAxisAngle(new THREE.Vector3(0, 1, 0), yaw);
      v.root.position.set(L.x + lp.x, y + 0.05, L.z + lp.z);
      v.root.rotation.y = yaw + 0.2;
      v.setHeadlights?.(0);
      this.group.add(bakeStatic(v.root));
    } catch (e) { /* optional */ }
  }

  // ── Canyon waterfall ───────────────────────────────────────────────
  buildWaterfall() {
    const t = this.track, S = {}, H = {};
    // Find the tallest rock wall between the last hairpin and the summit.
    const hp = t.tag('hairpin');
    const s0 = hp.length ? hp[hp.length - 1].s1 + 60 : 1600;
    const sEnd = (t.tag('summit')[0]?.s0 ?? 2200) - 40;
    let best = null;
    for (let s = s0; s < sEnd; s += 6) {
      const i = t.idx(s);
      for (const side of [-1, 1]) {
        if ((side < 0 ? t.sideL : t.sideR)[i] !== 1) continue;
        // Needs a straight-ish stretch so the sheet is visible ahead.
        if (Math.abs(t.kSmooth[i]) > 0.012) continue;
        t.frame(s, S);
        const wall = side < 0 ? t.wallL[i] : t.wallR[i];
        const lat = side * (wall + 22);
        this.surf.sample(S.x + S.rx * lat, S.z + S.rz * lat, H);
        const rise = H.h - S.y;
        if (!best || rise > best.rise) best = { s, side, rise, wall };
      }
    }
    if (!best || best.rise < 12) return;
    t.frame(best.s, S);
    const { side, wall } = best;
    // Walk up the face collecting a spine of points hugging the terrain.
    const spine = [];
    for (let l = wall + 1.2; l < wall + 60; l += 0.8) {
      const lat = side * l;
      const x = S.x + S.rx * lat, z = S.z + S.rz * lat;
      this.surf.sample(x, z, H);
      spine.push({ l, x, z, y: H.h });
      if (H.h - S.y > 40 || (spine.length > 8 && H.slope < 0.35)) break;
    }
    if (spine.length < 6) return;
    // A ribbon hugging the face, widening as it falls. Built three times:
    // a dark wet streak on the rock, the main sheet, and a faster, fainter
    // veil in front of it, so the fall has depth and visible motion.
    const ribbon = (off, w0, w1, cols = 7) => {
      const pos = [], uv = [], idx = [];
      let v = 0;
      for (let r = 0; r < spine.length; r++) {
        const p = spine[spine.length - 1 - r]; // top first
        if (r > 0) { const q = spine[spine.length - r]; v += Math.hypot(p.x - q.x, p.y - q.y, p.z - q.z); }
        const frac = r / (spine.length - 1);
        const halfW = lerp(w0, w1, Math.pow(frac, 0.8)) * (1 + 0.12 * Math.sin(r * 0.9));
        for (let c = 0; c < cols; c++) {
          const u = c / (cols - 1);
          const a = (u - 0.5) * 2 * halfW;
          // Bulge the middle outward so the sheet reads as falling water.
          const bulge = off + (1 - Math.pow(2 * u - 1, 2)) * 0.25;
          pos.push(p.x + S.fx * a - S.rx * side * bulge, p.y + 0.35, p.z + S.fz * a - S.rz * side * bulge);
          uv.push(u, v / 6);
        }
      }
      for (let r = 0; r < spine.length - 1; r++) for (let c = 0; c < cols - 1; c++) {
        const a = r * cols + c, b = a + 1, cc = a + cols, d = cc + 1;
        idx.push(a, cc, b, b, cc, d);
      }
      const geo = new THREE.BufferGeometry();
      geo.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
      geo.setAttribute('uv', new THREE.Float32BufferAttribute(uv, 2));
      geo.setIndex(idx);
      geo.computeVertexNormals();
      return geo;
    };
    const [c, g] = makeCanvas(128, 256);
    const rng = mulberry32(3);
    g.fillStyle = 'rgba(255,255,255,0.4)'; g.fillRect(0, 0, 128, 256);
    for (let k = 0; k < 320; k++) {
      const x = rng() * 128, w = 1 + rng() * 4, y = rng() * 256, h = 60 + rng() * 190;
      const grd = g.createLinearGradient(0, y, 0, y + h);
      const a = 0.3 + rng() * 0.65;
      grd.addColorStop(0, 'rgba(255,255,255,0)'); grd.addColorStop(0.3, `rgba(255,255,255,${a})`); grd.addColorStop(1, 'rgba(255,255,255,0)');
      g.fillStyle = grd;
      g.fillRect(x, y, w, h); g.fillRect(x, y - 256, w, h);
    }
    const edge = g.createLinearGradient(0, 0, 128, 0);
    edge.addColorStop(0, 'rgba(0,0,0,1)'); edge.addColorStop(0.18, 'rgba(0,0,0,0.1)'); edge.addColorStop(0.82, 'rgba(0,0,0,0.1)'); edge.addColorStop(1, 'rgba(0,0,0,1)');
    g.globalCompositeOperation = 'destination-out'; g.fillStyle = edge; g.fillRect(0, 0, 128, 256);
    const tex = new THREE.CanvasTexture(c);
    tex.wrapS = THREE.ClampToEdgeWrapping; tex.wrapT = THREE.RepeatWrapping;
    const tex2 = tex.clone();
    tex2.repeat.set(1, 0.6);
    const wet = new THREE.Mesh(ribbon(0.12, 2.2, 5.2, 5), new THREE.MeshBasicMaterial({ color: 0x0d1418, transparent: true, opacity: 0.45, depthWrite: false, polygonOffset: true, polygonOffsetFactor: -2, polygonOffsetUnits: -2 }));
    wet.renderOrder = 1;
    this.group.add(wet);
    const mat = new THREE.MeshLambertMaterial({ color: 0xe6f2ff, map: tex, alphaMap: tex, transparent: true, depthWrite: false, side: THREE.DoubleSide, emissive: 0x6d8fb0, emissiveIntensity: 0.35 });
    const sheet = new THREE.Mesh(ribbon(0.45, 1.3, 3.6), mat);
    sheet.renderOrder = 2;
    this.group.add(sheet);
    const veilMat = mat.clone();
    veilMat.map = tex2; veilMat.alphaMap = tex2; veilMat.opacity = 0.55;
    const veil = new THREE.Mesh(ribbon(0.85, 1.0, 4.2, 5), veilMat);
    veil.renderOrder = 3;
    this.group.add(veil);

    // Plunge pool with churning foam, spray, and boulders around the rim.
    const base = spine[0];
    const pool = new THREE.Mesh(new THREE.CircleGeometry(3.4, 24).rotateX(-Math.PI / 2), new THREE.MeshStandardMaterial({ color: 0x2a4b5c, roughness: 0.08, metalness: 0.3 }));
    pool.position.set(base.x + S.rx * side * 1.2, base.y + 0.12, base.z + S.rz * side * 1.2);
    this.group.add(pool);
    const [fc, fg] = makeCanvas(128, 128);
    const fr = mulberry32(8);
    for (let k = 0; k < 160; k++) {
      const a = fr() * Math.PI * 2, r = 20 + fr() * 42;
      fg.fillStyle = `rgba(255,255,255,${0.15 + fr() * 0.5})`;
      fg.beginPath(); fg.arc(64 + Math.cos(a) * r * 0.9, 64 + Math.sin(a) * r * 0.9, 2 + fr() * 7, 0, Math.PI * 2); fg.fill();
    }
    const foamTex = new THREE.CanvasTexture(fc);
    const foam = new THREE.Mesh(new THREE.CircleGeometry(3.3, 24).rotateX(-Math.PI / 2), new THREE.MeshLambertMaterial({ color: 0xf2f8ff, map: foamTex, transparent: true, depthWrite: false, emissive: 0x6d8fb0, emissiveIntensity: 0.3 }));
    foam.position.copy(pool.position).y += 0.04;
    foam.renderOrder = 2;
    this.group.add(foam);
    const ring = [];
    const H2 = {};
    for (let k = 0; k < 14; k++) {
      const a = (k / 14) * Math.PI * 2;
      const r = 3.6 + (k % 3) * 0.4;
      const x = pool.position.x + Math.cos(a) * r, z = pool.position.z + Math.sin(a) * r;
      if (!this.clearOfRoad(x, z, 0.6)) continue;
      this.surf.sample(x, z, H2);
      const sz = 0.5 + ((k * 7) % 5) * 0.2;
      ring.push({ x, z, y: Math.min(H2.h, pool.position.y + 0.3) - sz * 0.25, sx: sz * 1.3, sy: sz, sz: sz, ry: a * 3, col: 0x8c8880, b: 0.8 });
    }
    if (ring.length) this.group.add(instanced(rockGeometry(31, 1, { lichen: 1.6 }), this.rockMat, ring, { cast: true }));
    const smokeMat = new THREE.SpriteMaterial({ map: smokeTexture(), color: 0xe8f2ff, transparent: true, opacity: 0.32, depthWrite: false });
    const sprays = [];
    for (let k = 0; k < 5; k++) {
      const sp = new THREE.Sprite(smokeMat);
      sp.position.set(pool.position.x + (k - 2) * S.fx * 1.1, pool.position.y + 0.8 + (k % 3) * 0.7, pool.position.z + (k - 2) * S.fz * 1.1);
      sp.scale.setScalar(4 + k);
      sprays.push(sp);
      this.group.add(sp);
    }
    let wt = 0;
    this.world.updaters.push((dt, night, camera) => {
      wt += dt;
      tex.offset.y = -wt * 0.9;
      tex2.offset.y = -wt * 1.5;
      if (camera && camera.position.distanceToSquared(pool.position) > 800 * 800) return;
      foam.rotation.y = wt * 0.35;
      sprays.forEach((sp, k) => {
        const a = wt * 0.9 + k * 2.1;
        sp.scale.setScalar(3.5 + k * 0.8 + Math.sin(a) * 0.9);
        sp.material.rotation = a * 0.1;
      });
    });
    this.waterfall = { s: best.s, side, height: best.rise };
  }
}
