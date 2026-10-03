import * as THREE from 'three';
import { mergeGeometries } from 'three/addons/utils/BufferGeometryUtils.js';
import { clamp, lerp, smoothstep, mulberry32, fbm, makeNoise2D } from '../util/math.js';
import { gravelTexture, glowTexture } from './textures.js';
import {
  SurfaceSampler, rockGeometry, colorize, prep, rockMaterial, instanced, placed, bakeStatic, mergedMesh,
  makeCanvas, canvasTex, FONT, SignAtlas, roundRect, diamond, panel,
} from './coast/kit.js';

// Hollow Point — the cliff road of Level 2, at blue hour turning to dawn.
//
// The sea is on the left of the road. Along the cliff base an animated surf
// ribbon follows the real shoreline (found by marching out from the road to
// where the rendered terrain meets the water); sea stacks and rocks stand in
// the surf with foam rings of their own, and a natural arch stands off the
// point. Monterey cypress lean inland away from the wind, coastal scrub and
// ice plant cover the tops, a lighthouse on its headland sweeps a beam over
// the water, and fishing boats sit offshore with their running lights on.
// The start has a gantry and a surfers' coffee pull-out; a vista pull-out
// looks out from the bluff.

const CHUNK = 720;
const tick = () => new Promise((r) => setTimeout(r, 0));

// ── Shaders ───────────────────────────────────────────────────────────
const foamVert = /* glsl */`
varying vec2 vUv;
varying float vPh;
#include <fog_pars_vertex>
void main() {
  vUv = uv;
  vec4 p = vec4(position, 1.0);
  #ifdef USE_INSTANCING
    p = instanceMatrix * p;
    vPh = instanceMatrix[3].x * 0.13 + instanceMatrix[3].z * 0.071;
  #else
    vPh = 0.0;
  #endif
  vec4 mvPosition = modelViewMatrix * p;
  gl_Position = projectionMatrix * mvPosition;
  #include <fog_vertex>
}`;

const foamFrag = /* glsl */`
uniform float uTime;
uniform float uBright;
uniform float uSwell;
uniform vec3 uColor;
varying vec2 vUv;
varying float vPh;
#include <fog_pars_fragment>
float h21(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
float vn(vec2 p) {
  vec2 i = floor(p), f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(mix(h21(i), h21(i + vec2(1, 0)), u.x), mix(h21(i + vec2(0, 1)), h21(i + vec2(1, 1)), u.x), u.y);
}
void main() {
  float v = vUv.y;            // 0 = against the rock, 1 = open water
  float t = uTime + vPh;
  float n = vn(vec2(vUv.x * 2.3, v * 3.0 - t * 0.35)) * 0.6 + vn(vec2(vUv.x * 7.0 + t * 0.21, v * 9.0 - t * 0.5)) * 0.4;
  // Swell lines rolling in toward the rock, breaking into foam.
  float band = fract(v * 2.1 + t * 0.2);
  float swell = smoothstep(0.0, 0.07, band) * (1.0 - smoothstep(0.07, 0.33, band));
  // Surge: the contact foam breathes with the waves.
  float surge = 0.7 + 0.3 * sin(t * 1.3 + vUv.x * 0.7);
  float contact = 1.0 - smoothstep(0.08, 0.42 * surge, v + (n - 0.5) * 0.45);
  float lace = smoothstep(0.35, 0.8, n) * (1.0 - smoothstep(0.3, 0.9, v));
  float foam = max(contact * (0.45 + 0.7 * n), max(swell * smoothstep(0.4, 0.75, n) * (1.0 - v) * uSwell, lace * 0.45));
  foam *= smoothstep(0.0, 0.05, v) * (1.0 - smoothstep(0.8, 1.0, v));
  // Rings (uSwell < 0.5) break into ragged arcs so they don't read as circles.
  if (uSwell < 0.5) foam *= smoothstep(0.35, 0.65, vn(vec2(vUv.x * 1.3 + vPh * 3.0, t * 0.15))) * 0.75;
  gl_FragColor = vec4(uColor * uBright, clamp(foam, 0.0, 1.0) * 0.8);
  #include <fog_fragment>
}`;

const beamVert = /* glsl */`
varying float vAlong;
varying float vDepth;
varying vec3 vN;
varying vec3 vV;
#include <fog_pars_vertex>
void main() {
  vAlong = 1.0 - uv.y;        // 0 at the lamp, 1 at the far end
  vec4 mvPosition = modelViewMatrix * vec4(position, 1.0);
  vDepth = -mvPosition.z;
  vN = normalize(normalMatrix * normal);
  vV = normalize(-mvPosition.xyz);
  gl_Position = projectionMatrix * mvPosition;
  #include <fog_vertex>
}`;

const beamFrag = /* glsl */`
uniform vec3 uColor;
uniform float uStrength;
varying float vAlong;
varying float vDepth;
varying vec3 vN;
varying vec3 vV;
#include <fog_pars_fragment>
void main() {
  vec3 n = vN / max(length(vN), 1e-4);
  vec3 v = vV / max(length(vV), 1e-4);
  float core = pow(clamp(abs(dot(n, v)), 1e-4, 1.0), 1.6);
  float along = clamp(vAlong, 0.0, 1.0);
  float fall = pow(max(1.0 - along, 1e-4), 2.0) * smoothstep(0.0, 0.04, along);
  // Fade out as the cone sweeps over the camera: up close it would read as
  // a flat sheet, not a shaft of light in haze.
  float near = smoothstep(15.0, 120.0, vDepth);
  float a = clamp(core * fall * near * uStrength, 0.0, 2.0);
  vec3 c = uColor * a;
  #ifdef USE_FOG
    #ifdef FOG_EXP2
      float fogF = 1.0 - exp(-fogDensity * fogDensity * vFogDepth * vFogDepth);
    #else
      float fogF = smoothstep(fogNear, fogFar, vFogDepth);
    #endif
    c *= 1.0 - fogF * 0.8;
  #endif
  gl_FragColor = vec4(c, 1.0);
}`;

function foamMaterial(swell = 1) {
  const uniforms = THREE.UniformsUtils.merge([THREE.UniformsLib.fog, {
    uTime: { value: 0 }, uBright: { value: 1 }, uSwell: { value: swell }, uColor: { value: new THREE.Color(0xeef4f5) },
  }]);
  const m = new THREE.ShaderMaterial({
    vertexShader: foamVert, fragmentShader: foamFrag, uniforms,
    transparent: true, depthWrite: false, fog: true,
    polygonOffset: true, polygonOffsetFactor: -2, polygonOffsetUnits: -2,
  });
  m.userData.kind = 'Surf'; // MaterialKind for the scene export
  return m;
}

// Flat ring on the water, uv.y = 0 at the inner (rock) edge → 1 outside.
function ringGeometry(inner = 1, outer = 2.1, seg = 28) {
  const pos = [], uv = [], idx = [];
  for (let i = 0; i <= seg; i++) {
    const a = (i / seg) * Math.PI * 2;
    const c = Math.cos(a), s = Math.sin(a);
    pos.push(c * inner, 0, s * inner, c * outer, 0, s * outer);
    uv.push(i / seg * 6, 0, i / seg * 6, 1);
  }
  for (let i = 0; i < seg; i++) {
    const a = i * 2;
    idx.push(a, a + 2, a + 1, a + 1, a + 2, a + 3);
  }
  const g = new THREE.BufferGeometry();
  g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
  g.setAttribute('uv', new THREE.Float32BufferAttribute(uv, 2));
  g.setIndex(idx);
  return g;
}

// Windswept Monterey cypress, leaning toward local +X (inland).
function cypressGeometry(variant) {
  const rng = mulberry32(900 + variant);
  const parts = [];
  const trunk = prep(new THREE.CylinderGeometry(0.22, 0.38, 3.2, 6, 1, true));
  trunk.translate(0, 1.6, 0);
  parts.push(colorize(trunk, 0x4a3a2c));
  const upper = prep(new THREE.CylinderGeometry(0.14, 0.24, 3.6, 5, 1, true));
  upper.translate(0, 1.8, 0);
  upper.rotateZ(-0.62);
  upper.translate(0.2, 2.9, 0);
  parts.push(colorize(upper, 0x4a3a2c));
  // A second limb forking off the trunk toward the lee side.
  const limb = prep(new THREE.CylinderGeometry(0.1, 0.17, 3.0, 5, 1, true));
  limb.translate(0, 1.5, 0);
  limb.rotateZ(-0.95);
  limb.rotateY(0.5);
  limb.translate(0.1, 2.6, 0.2);
  parts.push(colorize(limb, 0x4a3a2c));
  // Foliage in flat, wind-sheared shelves, darker underneath and in the
  // middle of the crown, lighter on the windward crest.
  const greens = [0x2a3f2c, 0x2c4430, 0x33503a, 0x3a5538, 0x415e3c];
  const blobs = 6 + variant % 2;
  for (let k = 0; k < blobs; k++) {
    const b = prep(new THREE.IcosahedronGeometry(1, 0));
    const f = k / (blobs - 1);
    b.scale(lerp(2.4, 1.4, f) * lerp(0.9, 1.25, rng()), lerp(0.75, 0.5, f) * lerp(0.85, 1.15, rng()), lerp(2.0, 1.2, f) * lerp(0.85, 1.2, rng()));
    b.rotateY(rng() * 3);
    b.translate(lerp(-0.8, 3.9, f) + (rng() - 0.5) * 0.8, lerp(5.0, 6.2, f) + (rng() - 0.5) * 0.6 + (k % 2) * 0.35, (rng() - 0.5) * 2.0);
    parts.push(colorize(b, greens[Math.min(greens.length - 1, (k % 2) * 2 + Math.floor(rng() * 3))]));
  }
  // An underlayer of shade below the shelves.
  const u = prep(new THREE.IcosahedronGeometry(1, 0));
  u.scale(2.8, 0.45, 1.8);
  u.translate(1.4, 4.6, 0);
  parts.push(colorize(u, 0x1f3024));
  return mergeGeometries(parts, false);
}

// Soft scrub blob: a lumpy low-poly sphere with smooth (radial) normals.
function bushGeometry() {
  const b = new THREE.IcosahedronGeometry(1, 1);
  const n = makeNoise2D(17);
  const p = b.getAttribute('position');
  for (let i = 0; i < p.count; i++) {
    const x = p.getX(i), y = p.getY(i), z = p.getZ(i);
    const r = 1 + n(x * 1.7 + z, y * 1.9) * 0.18;
    p.setXYZ(i, x * r, Math.max(y * r * 0.62, -0.15), z * r);
  }
  b.deleteAttribute('uv');
  b.computeVertexNormals();
  b.translate(0, 0.2, 0);
  return colorize(b, 0xffffff);
}

// Monterey pine: a taller, straighter trunk under an irregular dome of dark
// needle clumps.
function pineGeometry(variant) {
  const rng = mulberry32(700 + variant);
  const parts = [];
  const trunk = prep(new THREE.CylinderGeometry(0.2, 0.42, 8.5, 6, 1, true));
  trunk.translate(0, 4.25, 0);
  parts.push(colorize(trunk, 0x4d3b2d));
  for (let k = 0; k < 3; k++) {
    const br = prep(new THREE.CylinderGeometry(0.06, 0.12, 3, 4, 1, true));
    br.translate(0, 1.5, 0);
    br.rotateZ(0.9 + rng() * 0.4);
    br.rotateY(k * 2.1 + rng());
    br.translate(0, 5.2 + k * 1.1, 0);
    parts.push(colorize(br, 0x4d3b2d));
  }
  const greens = [0x243a2a, 0x2d4631, 0x33503a, 0x283f2c, 0x3a5a3c];
  const n = 8;
  for (let k = 0; k < n; k++) {
    const b = prep(new THREE.IcosahedronGeometry(1, 0));
    const a = (k / n) * Math.PI * 2 + rng() * 0.6;
    const r = k === 0 ? 0 : lerp(1.2, 2.9, rng());
    const y = k === 0 ? 11.2 : lerp(7.4, 10.6, rng());
    b.scale(lerp(1.5, 2.3, rng()), lerp(0.9, 1.3, rng()), lerp(1.5, 2.3, rng()));
    b.rotateY(rng() * 3);
    b.translate(Math.cos(a) * r, y, Math.sin(a) * r);
    parts.push(colorize(b, greens[k % greens.length]));
  }
  return mergeGeometries(parts, false);
}

// Grass tuft: a fan of thin blades, dark at the root and pale at the tip
// (instance colour tints it from green to straw).
function tuftGeometry() {
  const pos = [], colr = [];
  const rng = mulberry32(41);
  const n = 9;
  for (let k = 0; k < n; k++) {
    const a = (k / n) * Math.PI * 2 + rng() * 0.5;
    const lean = lerp(0.25, 0.6, rng()), h = lerp(0.55, 1.0, rng()), w = 0.07;
    const ca = Math.cos(a), sa = Math.sin(a);
    const tx = ca * lean * h, tz = sa * lean * h;
    pos.push(-sa * w, 0, ca * w, sa * w, 0, -ca * w, tx, h, tz);
    colr.push(0.35, 0.36, 0.25, 0.35, 0.36, 0.25, 1, 1, 0.9);
  }
  const g = new THREE.BufferGeometry();
  g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
  g.setAttribute('color', new THREE.Float32BufferAttribute(colr, 3));
  g.computeVertexNormals();
  // Blades light like the ground under them, not like thin cards.
  const nrm = g.getAttribute('normal');
  for (let i = 0; i < nrm.count; i++) nrm.setXYZ(i, 0, 1, 0);
  return g;
}

// Sea stack: stacked strata blocks, each stepped back from the one below,
// with a scrubby cap. Unit height, about unit width; vertex colours carry
// the cap (the rock itself is tinted per instance).
function stackGeometry(seed) {
  const rng = mulberry32(seed);
  const parts = [];
  const tiers = 4;
  let y = 0, w = 1;
  for (let k = 0; k < tiers; k++) {
    const h = k === 0 ? 0.36 : lerp(0.18, 0.26, rng());
    const g = rockGeometry(seed * 7 + k, 1, 1.0, 0.28);
    g.scale(w * lerp(0.95, 1.1, rng()), h * 0.62, w * lerp(0.8, 1.0, rng()));
    g.rotateY(rng() * 6.3);
    g.translate((rng() - 0.5) * 0.15, y + h * 0.3, (rng() - 0.5) * 0.15);
    parts.push(colorize(g, 0xffffff));
    y += h * 0.8;
    w *= lerp(0.72, 0.88, rng());
  }
  const cap = prep(new THREE.IcosahedronGeometry(1, 1));
  cap.scale(w * 0.95, 0.07, w * 0.85);
  cap.translate(0, y + 0.02, 0);
  parts.push(colorize(cap, 0x6a7a4a));
  const g = mergeGeometries(parts, false);
  g.scale(1, 1 / (y + 0.05), 1);
  return g;
}

// Crag: a blocky slab of cliff rock whose faces step in horizontal ledges,
// set against steep ground so the terrain reads as sculpted cliff.
function cragGeometry(seed) {
  const g = new THREE.IcosahedronGeometry(1, 1);
  const n = makeNoise2D(seed);
  const pos = g.getAttribute('position');
  for (let i = 0; i < pos.count; i++) {
    const x = pos.getX(i), y = pos.getY(i), z = pos.getZ(i);
    // Ledges: pull the radius in and out in bands of height.
    const band = Math.floor((y + 1) * 2.5);
    const r = 1 + n(x * 1.3 + band * 3.1, z * 1.3) * 0.28 + (band % 2) * 0.08;
    pos.setXYZ(i, x * r, Math.max(-0.8, y) * (0.9 + (band % 2) * 0.1), z * r * 0.75);
  }
  g.deleteAttribute('uv');
  g.computeVertexNormals();
  return g;
}

export default class Coast {
  constructor(opts = {}) {
    this.zone = opts.zone ?? 0;
    this.label = 'Carving the coast';
    this.exclusions = [];
  }

  // ── Planning: pull-outs and the lighthouse headland ─────────────────
  plan(world) {
    const t = world.track, T = world.terrain;
    this.track = t; this.terrain = T;
    this.zEnd = t.zoneStart[this.zone + 1] ?? t.length;

    // Surfers' coffee pull-out beside the start, cut into the hillside.
    const s0 = 30;
    const wr = t.wallR[t.idx(s0)];
    const pp = t.pointAt(s0, wr + 14);
    this.pullout = { s: s0, side: 1, x: pp.x, z: pp.z, y: t.surfaceY(s0, wr) - 0.12 };
    T.addFlatten(pp.x, pp.z, 13, 16, this.pullout.y);
    this.exclusions.push({ x: pp.x, z: pp.z, r: 20 });

    // Lighthouse on a headland pad below road level, seaward of the road.
    const lh = t.tag('lighthouse')[0];
    if (lh) {
      const s = Math.round((lh.s0 + lh.s1) / 2) + 20;
      const wl = t.wallL[t.idx(s)];
      const p = t.pointAt(s, -(wl + 58));
      this.lighthouse = { s, x: p.x, z: p.z, y: t.py[t.idx(s)] - 8 };
      T.addFlatten(p.x, p.z, 24, 34, this.lighthouse.y);
      // A neck of land joining the headland to the cliff the road runs on.
      for (const [f, dy] of [[0.4, -4.5], [0.62, -6.5]]) {
        const q = t.pointAt(s + (f - 0.5) * 8, -(wl + 58 * f));
        T.addFlatten(q.x, q.z, 9, 16, t.py[t.idx(s)] + dy);
      }
      this.exclusions.push({ x: p.x, z: p.z, r: 27 });
    }

    // Vista pull-out on the bluff, seaward.
    const bl = t.tag('bluff')[0];
    if (bl) {
      const s = Math.round((bl.s0 + bl.s1) / 2);
      const wl = t.wallL[t.idx(s)];
      const p = t.pointAt(s, -(wl + 11));
      this.vista = { s, side: -1, x: p.x, z: p.z, y: t.surfaceY(s, -wl) - 0.15 };
      T.addFlatten(p.x, p.z, 10, 12, this.vista.y);
      this.exclusions.push({ x: p.x, z: p.z, r: 14 });
    }
  }

  async build(world) {
    const t0 = performance.now();
    this.world = world;
    this.track = world.track;
    this.terrain = world.terrain;
    this.seaY = world.level.sea?.y ?? 0;
    this.surf = new SurfaceSampler(world.terrain);
    this.group = new THREE.Group();
    this.group.name = 'coast';
    this.nChunks = Math.ceil((this.zEnd + 200) / CHUNK) + 1;
    this.foamMat = foamMaterial(1);
    this.ringMat = foamMaterial(0.15);
    this.rockMat = rockMaterial();          // tinted per instance
    this.archMat = rockMaterial();
    this.archMat.color.setHex(0xc4a98a);

    this.findShore();
    this.buildFoam();
    this.buildSeaRocks();
    await tick();
    this.buildArch();
    this.buildVegetation();
    this.buildGrass();
    await tick();
    this.buildCrags();
    this.buildOutcrops();
    this.buildLighthouse();
    this.buildStartArea();
    await this.buildPullout();
    await this.buildVista();
    this.buildSigns();
    this.buildPoles();
    this.buildDelineators();
    this.buildBoats();

    // Surf and foam brighten with the dawn; everything animates off one clock.
    let time = 0;
    world.updaters.push((dt, night) => {
      time += dt;
      for (const m of [this.foamMat, this.ringMat]) {
        m.uniforms.uTime.value = time;
        m.uniforms.uBright.value = lerp(1.0, 0.32, night);
      }
    });

    world.scene.add(this.group);
    let calls = 0, tris = 0;
    this.group.traverse((o) => {
      if (!o.isMesh && !o.isPoints && !o.isLineSegments) return;
      calls++;
      const g = o.geometry;
      if (o.isMesh) tris += (g.index ? g.index.count : g.getAttribute('position').count) / 3 * (o.isInstancedMesh ? o.count : 1);
    });
    this.stats = { calls, tris, crags: this.cragCount, grass: this.grassCount, trees: this.treeCount, bushes: this.bushCount, ms: Math.round(performance.now() - t0) };
    console.info(`[Coast] ${calls} draws, ${(tris / 1000).toFixed(0)}k tris, ${(performance.now() - t0).toFixed(0)} ms`);
  }

  // ── Placement helpers ───────────────────────────────────────────────
  excluded(x, z, pad = 0) {
    for (const e of this.exclusions) if (Math.pow(x - e.x, 2) + Math.pow(z - e.z, 2) < Math.pow(e.r + pad, 2)) return true;
    return false;
  }

  clearOfRoad(x, z, radius) {
    const info = this.terrain.roadInfo(x, z);
    if (!info.near) return info.d > radius + 12;
    const i = this.track.idx(info.s);
    const wall = Math.max(this.track.wallL[i], this.track.wallR[i]);
    return info.d - radius >= wall + 0.6;
  }

  inZone(x, need = 0.5) { return this.terrain.zoneWeights(x)[this.zone] >= need; }

  chunkOf(s) { return clamp(Math.floor(s / CHUNK), 0, this.nChunks - 1); }

  // ── Shoreline: march seaward from the road until the rendered ground
  // dips under the water. ─────────────────────────────────────────────
  findShore() {
    const t = this.track, S = {}, H = {};
    const sea = this.seaY;
    this.shore = [];
    for (let s = 0; s < this.zEnd + 180; s += 5) {
      t.frame(s, S);
      const wl = S.wallL;
      let prevD = wl + 3, prevH = this.surf.sample(S.x - S.rx * prevD, S.z - S.rz * prevD, H).h;
      let hit = null;
      for (let d = wl + 7; d < 520; d += 6) {
        const x = S.x - S.rx * d, z = S.z - S.rz * d;
        const h = this.surf.sample(x, z, H).h;
        if (h < sea + 0.2) {
          // Refine between prevD and d.
          let a = prevD, b = d, ha = prevH;
          for (let k = 0; k < 4; k++) {
            const m = (a + b) / 2;
            const hm = this.surf.sample(S.x - S.rx * m, S.z - S.rz * m, H).h;
            if (hm < sea + 0.2) b = m; else { a = m; ha = hm; }
          }
          hit = (a + b) / 2;
          break;
        }
        prevD = d; prevH = h;
      }
      if (hit === null) { this.shore.push(null); continue; }
      // A ray hitting water behind a sea stack gives a far-off point; keep it,
      // the segment split below handles jumps.
      this.shore.push({ s, d: hit, x: S.x - S.rx * hit, z: S.z - S.rz * hit, nx: -S.rx, nz: -S.rz });
    }
    // Split into continuous runs.
    this.shoreRuns = [];
    let run = [];
    for (const p of this.shore) {
      const last = run[run.length - 1];
      if (!p || (last && Math.hypot(p.x - last.x, p.z - last.z) > 22)) {
        if (run.length > 3) this.shoreRuns.push(run);
        run = p ? [p] : [];
      } else run.push(p);
    }
    if (run.length > 3) this.shoreRuns.push(run);
    // Recompute seaward normals from the shoreline itself.
    for (const r of this.shoreRuns) {
      for (let i = 0; i < r.length; i++) {
        const a = r[Math.max(0, i - 2)], b = r[Math.min(r.length - 1, i + 2)];
        let tx = b.x - a.x, tz = b.z - a.z;
        const l = Math.hypot(tx, tz) || 1;
        tx /= l; tz /= l;
        let nx = -tz, nz = tx;
        if (nx * r[i].nx + nz * r[i].nz < 0) { nx = -nx; nz = -nz; }
        r[i].nx = nx; r[i].nz = nz; r[i].tx = tx; r[i].tz = tz;
      }
    }
  }

  buildFoam() {
    const pos = [], uv = [], idx = [];
    const y = this.seaY + 0.1;
    for (const r of this.shoreRuns) {
      const base = pos.length / 3;
      let u = 0;
      r.forEach((p, i) => {
        if (i > 0) u += Math.hypot(p.x - r[i - 1].x, p.z - r[i - 1].z);
        const inner = 3.5, outer = 13;
        pos.push(p.x - p.nx * inner, y, p.z - p.nz * inner, p.x + p.nx * outer, y, p.z + p.nz * outer);
        uv.push(u / 9, 0, u / 9, 1);
      });
      for (let i = 0; i < r.length - 1; i++) {
        const a = base + i * 2;
        idx.push(a, a + 1, a + 2, a + 1, a + 3, a + 2);
      }
    }
    if (!idx.length) return;
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
    g.setAttribute('uv', new THREE.Float32BufferAttribute(uv, 2));
    g.setIndex(idx);
    g.computeBoundingSphere();
    const m = new THREE.Mesh(g, this.foamMat);
    m.renderOrder = 2;
    m.frustumCulled = true;
    this.group.add(m);
  }

  // ── Sea stacks and surf rocks, each with a foam ring ───────────────
  buildSeaRocks() {
    const rng = mulberry32(3131);
    const T = this.terrain, sea = this.seaY;
    const stacks = [], small = [], rings = [];
    const palette = [0xc9ad8c, 0xb89c7e, 0xa89484, 0xd2b99a, 0x9e8f82];
    const H = {};
    let lastStack = -1e9;
    for (const r of this.shoreRuns) {
      for (let i = 0; i < r.length; i++) {
        const p = r[i];
        // Big stacks, spaced out along the coast.
        if (p.s - lastStack > 55 && rng() < 0.22) {
          const off = lerp(28, 170, Math.pow(rng(), 1.4));
          const x = p.x + p.nx * off + p.tx * (rng() - 0.5) * 30, z = p.z + p.nz * off + p.tz * (rng() - 0.5) * 30;
          const bed = T.heightAt(x, z);
          if (bed < sea - 3) {
            const w = lerp(7, 16, rng());
            const hgt = lerp(10, 36, Math.pow(rng(), 1.3));
            stacks.push({ x, y: bed, z, sx: w, sy: hgt - bed + 1, sz: w * lerp(0.7, 1.2, rng()), ry: rng() * 6.3, col: palette[Math.floor(rng() * palette.length)], b: lerp(0.8, 1.05, rng()) });
            rings.push({ x, y: sea + 0.12, z, sx: w * 0.92, sy: 1, sz: w * 0.92, ry: rng() * 6.3 });
            lastStack = p.s;
          }
        }
        // Surf rocks strewn along the waterline.
        const n = rng() < 0.6 ? 1 + Math.floor(rng() * 3) : 0;
        for (let k = 0; k < n; k++) {
          const off = lerp(-1, 26, Math.pow(rng(), 1.6));
          const x = p.x + p.nx * off + p.tx * (rng() - 0.5) * 8, z = p.z + p.nz * off + p.tz * (rng() - 0.5) * 8;
          const bed = this.surf.sample(x, z, H).h;
          const size = lerp(1.2, 4.5, Math.pow(rng(), 1.7));
          if (bed > sea + 1.5) continue;
          small.push({ x, y: Math.min(bed, sea - 0.4), z, sx: size * lerp(0.8, 1.4, rng()), sy: size * lerp(0.7, 1.3, rng()), sz: size, ry: rng() * 6.3, col: palette[Math.floor(rng() * palette.length)], b: lerp(0.7, 1.0, rng()) });
          if (size > 2.3 && rng() < 0.6) rings.push({ x, y: sea + 0.12, z, sx: size * 0.95, sy: 1, sz: size * 0.95, ry: rng() * 6.3 });
        }
      }
    }
    // Two stack shapes; a vertex-coloured copy of the rock material shows
    // their green caps.
    if (stacks.length) {
      const stackMat = rockMaterial();
      stackMat.vertexColors = true;
      stackMat.customProgramCacheKey = () => 'coast-rock-vc';
      const half = Math.ceil(stacks.length / 2);
      this.group.add(instanced(stackGeometry(71), stackMat, stacks.slice(0, half), { cast: true }));
      if (stacks.length > half) this.group.add(instanced(stackGeometry(73), stackMat, stacks.slice(half), { cast: true }));
    }
    if (small.length) this.group.add(instanced(rockGeometry(72, 0, 0.8, 0.4), this.rockMat, small));
    if (rings.length) {
      const im = instanced(ringGeometry(0.85, 2.4), this.ringMat, rings, { receive: false });
      im.renderOrder = 2;
      this.group.add(im);
    }
    this.stackCount = stacks.length;
  }

  // ── Natural sea arch off the point ──────────────────────────────────
  buildArch() {
    const tag = this.track.tag('arch')[0];
    if (!tag || !this.shoreRuns.length) return;
    const sMid = (tag.s0 + tag.s1) / 2;
    let best = null;
    for (const r of this.shoreRuns) for (const p of r) if (!best || Math.abs(p.s - sMid) < Math.abs(best.s - sMid)) best = p;
    if (!best) return;
    // Walk out until the water is deep enough to stand an arch in.
    let off = 55, x = 0, z = 0;
    for (; off < 260; off += 10) {
      x = best.x + best.nx * off; z = best.z + best.nz * off;
      if (this.terrain.heightAt(x, z) < this.seaY - 6) break;
    }
    const n = makeNoise2D(55);
    const R = 17, tube = 6.2;
    const torus = new THREE.TorusGeometry(R, tube, 9, 30, Math.PI);
    torus.scale(1, 1.25, 1.2);
    const g = torus.toNonIndexed();
    const p = g.getAttribute('position');
    for (let i = 0; i < p.count; i++) {
      const px = p.getX(i), py = p.getY(i), pz = p.getZ(i);
      const d = n(px * 0.09 + pz * 0.05, py * 0.1) * 2.2 + n(px * 0.3, py * 0.3 + pz * 0.2) * 0.8;
      const l = Math.hypot(px, py) || 1;
      p.setXYZ(i, px + (px / l) * d, py + (py / l) * d + (py > 20 ? -1.2 : 0), pz * (1 + d * 0.05));
    }
    g.deleteAttribute('uv');
    g.computeVertexNormals();
    // Legs down to the sea bed, and a rubble apron.
    const legs = [];
    for (const side of [-1, 1]) {
      const leg = rockGeometry(80 + side, 1, 1.0, 0.3);
      leg.scale(tube * 1.35, 16, tube * 1.5);
      leg.translate(side * R, -10, 0);
      legs.push(leg);
      for (let k = 0; k < 3; k++) {
        const rb = rockGeometry(90 + k + side * 7, 0, 0.8, 0.45);
        rb.scale(3 + k, 2.5 + k * 0.6, 3.5 + k);
        rb.translate(side * (R + 7 + k * 3), -1.2, (k - 1) * 6);
        legs.push(rb);
      }
    }
    const merged = mergeGeometries([g, ...legs.map((l) => { if (l.hasAttribute('uv')) l.deleteAttribute('uv'); return l; })], false);
    const yaw = Math.atan2(-best.tz, best.tx);
    const mesh = new THREE.Mesh(placed(merged, x, this.seaY - 1.5, z, yaw), this.archMat);
    mesh.castShadow = true;
    mesh.receiveShadow = true;
    mesh.matrixAutoUpdate = false;
    this.group.add(mesh);
    // Foam where the legs meet the water.
    const rings = [];
    for (const side of [-1, 1]) {
      rings.push({ x: x + Math.cos(yaw) * side * R, y: this.seaY + 0.12, z: z - Math.sin(yaw) * side * R, sx: tube * 1.6, sy: 1, sz: tube * 1.6, ry: side });
    }
    const im = instanced(ringGeometry(0.85, 2.4), this.ringMat, rings, { receive: false });
    im.renderOrder = 2;
    this.group.add(im);
    this.arch = { x, z, yaw };
  }

  // ── Cypress, scrub and ice plant ────────────────────────────────────
  buildVegetation() {
    const t = this.track, T = this.terrain, rng = mulberry32(2024);
    const n2 = T.noise2;
    const cypress = [0, 1].map(() => Array.from({ length: this.nChunks }, () => []));
    const bushes = Array.from({ length: this.nChunks }, () => []);
    const S = {}, F = {};
    const b = t.bounds;
    const bushCols = [0x5f6e4c, 0x6b7458, 0x4d5a36, 0x6e5a3c, 0x55663e, 0x7a8068, 0x4a5638];
    const iceCols = [0x5f7d3a, 0xa8587c, 0x9c6a86, 0x7c8f3c, 0x6d8a40];
    const xEnd = T.x1 + 250;
    for (let x = b.minX - 380; x < xEnd; x += 7) {
      for (let z = b.minZ - 380; z < b.maxZ + 380; z += 7) {
        const px = x + (rng() - 0.5) * 6.5, pz = z + (rng() - 0.5) * 6.5;
        const r = rng();
        if (!this.inZone(px, 0.35)) continue;
        T.far(px, pz, F);
        if (F.d > 360 || F.s > this.zEnd + 120) continue;
        const seaSide = F.side < 0;
        this.surf.sample(px, pz, S);
        if (S.h < this.seaY + 3) continue;
        const roadY = t.py[t.idx(F.s)];
        // Sea side: only the cliff tops, not the faces.
        if (seaSide && (S.h < roadY - 25 || S.slope > 0.45)) continue;
        if (S.slope > 1.4) continue;
        if (this.excluded(px, pz, 2)) continue;
        const clump = smoothstep(-0.15, 0.35, fbm(n2, px / 230 + 3.1, pz / 230, 3));
        // Cypress: groves on the land side and scattered on the headlands.
        const pc = S.slope > 0.75 ? 0 : (seaSide ? 0.03 : 0.16) * (0.25 + clump) * (1 - smoothstep(0.4, 0.75, S.slope));
        if (r < pc) {
          if (!this.clearOfRoad(px, pz, 4.5)) continue;
          // Lean inland: local +X along the road's right vector (away from sea).
          const i = t.idx(F.s);
          // Pines stand in the sheltered groves inland; cypress take the wind.
          const v = !seaSide && F.d > 25 && rng() < 0.45 ? 1 : 0;
          const yaw = v ? rng() * 6.3 : Math.atan2(-t.rz[i], t.rx[i]) + (rng() - 0.5) * 0.6;
          const sc = v ? lerp(0.85, 1.35, rng()) : lerp(0.8, 1.6, Math.pow(rng(), 1.2));
          cypress[v][this.chunkOf(F.s)].push({ x: px, y: S.h - 0.3, z: pz, sx: sc, sy: sc * lerp(0.85, 1.15, rng()), sz: sc, ry: yaw, col: 0xffffff, b: lerp(0.75, 1.1, rng()) });
          continue;
        }
        // Scrub everywhere it can cling; ice plant mats nearer the edges.
        const pb = (0.14 + clump * 0.14) * (seaSide ? 1.1 : 0.8) * (1 - smoothstep(0.7, 1.4, S.slope) * 0.7) * (F.d < 70 ? 1.5 : 1);
        if (r < pc + pb) {
          if (!this.clearOfRoad(px, pz, 1.5)) continue;
          const ice = seaSide && rng() < 0.55;
          const col = ice ? iceCols[Math.floor(rng() * iceCols.length)] : bushCols[Math.floor(rng() * bushCols.length)];
          const sc = ice ? lerp(1.2, 2.6, rng()) : lerp(0.6, 1.8, Math.pow(rng(), 1.3));
          const steep = smoothstep(0.35, 0.8, S.slope);
          bushes[this.chunkOf(F.s)].push({ x: px, y: S.h - (ice ? 0.3 : 0.12) - steep * sc * 0.45, z: pz, sx: sc * lerp(0.8, 1.3, rng()), sy: ice ? sc * 0.4 : sc * lerp(lerp(0.6, 1.0, rng()), 1.25, steep), sz: sc, ry: rng() * 6.3, col, b: lerp(0.8, 1.05, rng()) });
        }
      }
    }
    const treeMat = new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.9, flatShading: true });
    const geos = [cypressGeometry(1), pineGeometry(1)];
    let trees = 0;
    cypress.forEach((chunks, v) => chunks.forEach((items) => {
      if (!items.length) return;
      trees += items.length;
      this.group.add(instanced(geos[v], treeMat, items, { cast: true }));
    }));
    // Roadside scrub: a denser band along the landward verge and cut slopes.
    for (let s = 10; s < this.zEnd + 60; s += 2.2) {
      const i = t.idx(s);
      if (rng() > 0.8) continue;
      const f = t.frame(s + (rng() - 0.5) * 2);
      const lat = f.wallR + 1.2 + Math.pow(rng(), 1.6) * 34;
      const x = f.x + f.rx * lat, z = f.z + f.rz * lat;
      this.surf.sample(x, z, S);
      if (S.slope > 1.6 || S.h > f.y + 22 || this.excluded(x, z, 1) || !this.clearOfRoad(x, z, 1.2)) continue;
      const sc = lerp(0.5, 1.4, Math.pow(rng(), 1.4));
      // On a slope a flat bush juts out like a shelf: keep it round and sunk in.
      const steep = smoothstep(0.35, 0.8, S.slope);
      bushes[this.chunkOf(s)].push({ x, y: S.h - 0.15 - steep * sc * 0.45, z, sx: sc * lerp(0.8, 1.2, rng()), sy: sc * lerp(lerp(0.55, 0.95, rng()), 1.25, steep), sz: sc, ry: rng() * 6.3, col: bushCols[Math.floor(rng() * bushCols.length)], b: lerp(0.8, 1.05, rng()) });
      // Ice plant spilling over the seaward verge beyond the guardrail.
      if (rng() < 0.35) {
        const latL = -(f.wallL + 1.0 + rng() * 5);
        const xl = f.x + f.rx * latL, zl = f.z + f.rz * latL;
        this.surf.sample(xl, zl, S);
        if (S.h > f.y - 4 && S.slope < 0.9 && !this.excluded(xl, zl) && this.clearOfRoad(xl, zl, 1)) {
          const sc2 = lerp(0.9, 2.0, rng());
          bushes[this.chunkOf(s)].push({ x: xl, y: S.h - 0.3, z: zl, sx: sc2, sy: sc2 * 0.4, sz: sc2 * lerp(0.7, 1.2, rng()), ry: rng() * 6.3, col: iceCols[Math.floor(rng() * iceCols.length)], b: lerp(0.8, 1.0, rng()) });
        }
      }
    }
    const bushMat = new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.95 });
    const bgeo = bushGeometry();
    let nb = 0;
    bushes.forEach((items) => { if (items.length) { nb += items.length; this.group.add(instanced(bgeo, bushMat, items)); } });
    this.treeCount = trees; this.bushCount = nb;
  }

  // ── Grass: tufts along both verges and over the cliff tops ──────────
  buildGrass() {
    const t = this.track, rng = mulberry32(7373);
    const chunks = Array.from({ length: this.nChunks }, () => []);
    const S = {}, F = {};
    const cols = [0x8a9a5a, 0x9aa070, 0xc8b47a, 0xb8a868, 0xa8a060, 0x9aa88a, 0x7d8c50, 0xd2bf86];
    for (let s = 6; s < this.zEnd + 80; s += 1.1) {
      const f = t.frame(s + (rng() - 0.5) * 0.8, F);
      const side = rng() < 0.55 ? 1 : -1;
      const wall = side > 0 ? f.wallR : f.wallL;
      // Thick on the verge, thinning out up the slopes.
      const lat = side * (wall + 1.2 + Math.pow(rng(), 2.2) * 38);
      const x = f.x + f.rx * lat, z = f.z + f.rz * lat;
      this.surf.sample(x, z, S);
      if (S.slope > 1.05 || S.h < this.seaY + 2 || Math.abs(S.h - f.y) > 14) continue;
      if (this.excluded(x, z, 0.5) || !this.clearOfRoad(x, z, 0.4)) continue;
      // A clump: many small tufts of one grass, the odd poppy among them.
      const n = 3 + Math.floor(rng() * 6);
      const base = cols[Math.floor(rng() * cols.length)];
      for (let k = 0; k < n; k++) {
        const r = rng() * 2.4, a = rng() * 6.3;
        const px = x + Math.cos(a) * r, pz = z + Math.sin(a) * r;
        const sc = lerp(0.5, 1.05, rng());
        const c = rng() < 0.03 ? 0xe8902a : rng() < 0.7 ? base : cols[Math.floor(rng() * cols.length)];
        chunks[this.chunkOf(s)].push({ x: px, y: this.surf.sample(px, pz, S).h - 0.05, z: pz, sx: sc, sy: sc * lerp(0.7, 1.2, rng()), sz: sc, ry: rng() * 6.3, col: c, b: lerp(0.8, 1.1, rng()) });
      }
    }
    const mat = new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 1, side: THREE.DoubleSide });
    const geo = tuftGeometry();
    let n = 0;
    chunks.forEach((items) => { if (items.length) { n += items.length; this.group.add(instanced(geo, mat, items)); } });
    this.grassCount = n;
  }

  // ── Crags: ledged slabs of rock on every steep face, seaward cliffs and
  // landward cuttings alike, so the height field reads as carved cliff ──
  buildCrags() {
    const t = this.track, rng = mulberry32(5151);
    const chunks = Array.from({ length: this.nChunks }, () => []);
    const S = {}, F = {};
    const palette = [0xc9ad8c, 0xb89c7e, 0xa89484, 0xd2b99a, 0x9e8f82, 0xb5a08a];
    for (let s = 0; s < this.zEnd + 120; s += 9) {
      t.frame(s + (rng() - 0.5) * 6, F);
      for (const side of [-1, 1]) {
        const wall = side > 0 ? F.wallR : F.wallL;
        for (let d = wall + 3; d < 260; d += lerp(6, 11, rng())) {
          const x = F.x + F.rx * side * d, z = F.z + F.rz * side * d;
          this.surf.sample(x, z, S);
          if (S.h < this.seaY - 2) break; // out over the water
          if (S.slope < 0.85 || rng() > smoothstep(0.85, 1.7, S.slope) * 0.8) continue;
          if (!this.inZone(x, 0.4) || this.excluded(x, z, 4) || !this.clearOfRoad(x, z, 3.5)) continue;
          // Face downhill; sink the slab back into the slope.
          const gl = S.slope, dx = -S.gx / gl, dz = -S.gz / gl;
          // Seaward faces already show their strata: there the slabs sit
          // deeper, as ledges rather than boulders.
          const sea = side < 0;
          const w = lerp(5, 14, Math.pow(rng(), 1.3)), hgt = lerp(2.5, sea ? 5 : 7, rng()), dep = lerp(2.2, 4, rng());
          const back = dep * (sea ? 0.8 : 0.55);
          chunks[this.chunkOf(s)].push({
            x: x - dx * back, y: S.h - hgt * 0.25, z: z - dz * back,
            sx: w, sy: hgt, sz: dep, ry: Math.atan2(dx, dz) + (rng() - 0.5) * 0.4,
            col: palette[Math.floor(rng() * palette.length)], b: lerp(0.78, 1.05, rng()),
          });
        }
      }
    }
    // The sea cliffs are near vertical, so a march across them barely
    // touches the face: dress them from the shoreline up instead, with
    // buttresses of stacked ledges pushed into the face.
    for (const run of this.shoreRuns) {
      for (let i = 0; i < run.length; i += 2) {
        const p = run[i];
        if (rng() < 0.3) continue;
        let top = -1e9;
        for (const d of [4, 9, 15]) top = Math.max(top, this.surf.sample(p.x - p.nx * d, p.z - p.nz * d, S).h);
        if (top < this.seaY + 8) continue;
        const ry = Math.atan2(p.nx, p.nz) + (rng() - 0.5) * 0.5;
        for (let y = this.seaY - 2; y < top - 3; y += lerp(5, 9, rng())) {
          const w = lerp(6, 13, rng()), hgt = lerp(3, 6.5, rng()), dep = lerp(3, 5, rng());
          const inset = dep * lerp(0.35, 0.65, rng()) + (rng() - 0.5) * 1.5;
          const x = p.x - p.nx * inset + p.tx * (rng() - 0.5) * 4, z = p.z - p.nz * inset + p.tz * (rng() - 0.5) * 4;
          if (this.excluded(x, z, 2) || !this.clearOfRoad(x, z, 3)) continue;
          chunks[this.chunkOf(p.s)].push({ x, y, z, sx: w, sy: hgt, sz: dep, ry, col: palette[Math.floor(rng() * palette.length)], b: lerp(0.75, 1.0, rng()) });
        }
      }
    }
    const geos = [cragGeometry(61), cragGeometry(62)];
    let n = 0;
    chunks.forEach((items) => {
      if (!items.length) return;
      n += items.length;
      const half = Math.ceil(items.length / 2);
      this.group.add(instanced(geos[0], this.rockMat, items.slice(0, half)));
      if (items.length > half) this.group.add(instanced(geos[1], this.rockMat, items.slice(half)));
    });
    this.cragCount = n;
  }

  // ── Delineator posts along both verges, reflectors glinting at dawn,
  // and yellow call boxes now and then ─────────────────────────────────
  buildDelineators() {
    const t = this.track, S = {};
    const posts = [], refl = [], boxes = [];
    for (let s = 40; s < this.zEnd + 40; s += 38) {
      t.frame(s, S);
      for (const side of [-1, 1]) {
        const wall = side > 0 ? S.wallR : S.wallL;
        const lat = side * (wall + 0.45);
        const x = S.x + S.rx * lat, z = S.z + S.rz * lat;
        if (this.excluded(x, z)) continue;
        const y = S.y - lat * S.bank;
        const yaw = Math.atan2(S.fx, S.fz);
        posts.push({ x, y: y - 0.2, z, sx: 1, sy: 1, sz: 1, ry: yaw });
        refl.push({ x: x - S.fx * 0.06, y: y + 0.85, z: z - S.fz * 0.06, sx: 1, sy: 1, sz: 1, ry: yaw, col: side > 0 ? 0xffffff : 0xffb020 });
      }
      if (s % 760 < 38 && s > 300) {
        const lat = S.wallR + 1.3;
        boxes.push({ x: S.x + S.rx * lat, y: S.y - lat * S.bank - 0.1, z: S.z + S.rz * lat, sx: 1, sy: 1, sz: 1, ry: Math.atan2(-S.rx, -S.rz) });
      }
    }
    if (!posts.length) return;
    const pg = new THREE.BoxGeometry(0.1, 1.3, 0.1); pg.translate(0, 0.65, 0);
    const band = new THREE.BoxGeometry(0.105, 0.2, 0.105); band.translate(0, 1.1, 0);
    const post = mergeGeometries([colorize(prep(pg), 0xf0f0ea), colorize(prep(band), 0x1a1a1a)], false);
    const cb = new THREE.BoxGeometry(0.5, 0.75, 0.35); cb.translate(0, 1.1, 0);
    const cbp = new THREE.BoxGeometry(0.12, 1.1, 0.12); cbp.translate(0, 0.55, 0);
    const callBox = mergeGeometries([colorize(prep(cb), 0xe0b52b), colorize(prep(cbp), 0x6a6a6a)], false);
    const mat = new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.6 });
    this.group.add(instanced(post, mat, posts, { cast: true }));
    if (boxes.length) this.group.add(instanced(callBox, mat, boxes, { cast: true }));
    // Reflectors: bright at blue hour (headlights would catch them).
    const rm = new THREE.MeshBasicMaterial({ color: 0xffffff });
    this.world.updaters.push((dt, night) => rm.color.setScalar(0.6 + 1.6 * night));
    const rg = new THREE.BoxGeometry(0.07, 0.16, 0.02);
    this.group.add(instanced(rg, rm, refl, { receive: false }));
  }

  // ── Rock outcrops on the landward slopes, boulders at the rock walls ─
  buildOutcrops() {
    const t = this.track, T = this.terrain, rng = mulberry32(4545);
    const chunks = Array.from({ length: this.nChunks }, () => []);
    const S = {}, F = {};
    const palette = [0xc9ad8c, 0xb89c7e, 0xa89484, 0xd2b99a, 0x9e8f82, 0x8e8074];
    const b = t.bounds;
    for (let x = b.minX - 350; x < T.x1 + 200; x += 12) {
      for (let z = b.minZ - 350; z < b.maxZ + 350; z += 12) {
        const px = x + (rng() - 0.5) * 10, pz = z + (rng() - 0.5) * 10;
        const r0 = rng();
        if (r0 > 0.45 || !this.inZone(px, 0.5)) continue;
        T.far(px, pz, F);
        if (F.side < 0 || F.d < 14 || F.d > 320 || F.s > this.zEnd + 100) continue;
        this.surf.sample(px, pz, S);
        if (S.slope < 0.8 || r0 > 0.25 * smoothstep(0.8, 1.5, S.slope)) continue;
        if (!this.clearOfRoad(px, pz, 6)) continue;
        const size = lerp(2.5, 8, Math.pow(rng(), 1.5)) * (F.d < 50 ? 0.6 : 1);
        chunks[this.chunkOf(F.s)].push({ x: px, y: S.h - size * 0.35, z: pz, sx: size * lerp(1, 1.7, rng()), sy: size * lerp(0.6, 1, rng()), sz: size, ry: rng() * 6.3, col: palette[Math.floor(rng() * palette.length)], b: lerp(0.75, 1.05, rng()) });
      }
    }
    // Boulders heaped at the foot of roadside rock walls.
    for (let s = 20; s < this.zEnd; s += 4) {
      const i = t.idx(s);
      for (const side of [-1, 1]) {
        if ((side < 0 ? t.sideL : t.sideR)[i] !== 1 || rng() > 0.35) continue;
        const f = t.frame(s + (rng() - 0.5) * 3);
        const wall = side < 0 ? f.wallL : f.wallR;
        const size = lerp(0.6, 2.2, Math.pow(rng(), 1.4));
        const lat = side * (wall + 0.7 + size);
        const x = f.x + f.rx * lat, z = f.z + f.rz * lat;
        if (!this.clearOfRoad(x, z, size)) continue;
        this.surf.sample(x, z, S);
        chunks[this.chunkOf(s)].push({ x, y: S.h - size * 0.3, z, sx: size * lerp(1, 1.6, rng()), sy: size * lerp(0.6, 1.1, rng()), sz: size, ry: Math.atan2(f.fx, f.fz), col: palette[Math.floor(rng() * palette.length)], b: lerp(0.75, 1.0, rng()) });
      }
    }
    const geo = rockGeometry(33, 1);
    chunks.forEach((items) => { if (items.length) this.group.add(instanced(geo, this.rockMat, items, { cast: true })); });
  }

  // ── Lighthouse ─────────────────────────────────────────────────────
  buildLighthouse() {
    const L = this.lighthouse;
    if (!L) return;
    const t = this.track, S = {};
    t.frame(L.s, S);
    const y = this.surf.sample(L.x, L.z).h;
    L.ground = y;
    const H = 19;
    // Tower: stacked bands, white and red.
    const bands = [];
    const nB = 6;
    for (let k = 0; k < nB; k++) {
      const r0 = lerp(2.7, 1.9, k / nB), r1 = lerp(2.7, 1.9, (k + 1) / nB);
      const c = new THREE.CylinderGeometry(r1, r0, H / nB, 20, 1, true);
      c.translate(0, H / nB * (k + 0.5), 0);
      bands.push(colorize(c, k % 2 ? 0xb3262c : 0xf2efe8));
    }
    const base = new THREE.CylinderGeometry(3.2, 3.4, 1.2, 20);
    base.translate(0, 0.4, 0);
    bands.push(colorize(base, 0xd8d2c6));
    const gallery = new THREE.CylinderGeometry(2.7, 2.5, 0.35, 20);
    gallery.translate(0, H + 0.15, 0);
    bands.push(colorize(gallery, 0x2a2d31));
    const rail = new THREE.TorusGeometry(2.6, 0.05, 4, 24);
    rail.rotateX(Math.PI / 2);
    rail.translate(0, H + 1.2, 0);
    bands.push(colorize(rail, 0x2a2d31));
    for (let k = 0; k < 12; k++) {
      const a = k / 12 * Math.PI * 2;
      const post = new THREE.BoxGeometry(0.06, 0.9, 0.06);
      post.translate(Math.cos(a) * 2.6, H + 0.75, Math.sin(a) * 2.6);
      bands.push(colorize(post, 0x2a2d31));
    }
    const dome = new THREE.SphereGeometry(1.55, 16, 8, 0, Math.PI * 2, 0, Math.PI / 2);
    dome.translate(0, H + 2.85, 0);
    bands.push(colorize(dome, 0x8a1c20));
    const vent = new THREE.ConeGeometry(0.3, 0.9, 8);
    vent.translate(0, H + 4.6, 0);
    bands.push(colorize(vent, 0x2a2d31));
    const towerMat = new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.55, metalness: 0.05 });
    const geos = bands.map((g) => {
      const h = g.index ? g.toNonIndexed() : g;
      if (h.hasAttribute('uv')) h.deleteAttribute('uv');
      h.computeVertexNormals();
      return placed(h, L.x, y, L.z);
    });
    const tower = new THREE.Mesh(mergeGeometries(geos, false), towerMat);
    tower.castShadow = true; tower.receiveShadow = true; tower.matrixAutoUpdate = false;
    this.group.add(tower);
    // Lantern room glass.
    const lanternMat = new THREE.MeshStandardMaterial({ color: 0xfff2c8, emissive: 0xffe2a0, emissiveIntensity: 1.2, roughness: 0.1, transparent: true, opacity: 0.9 });
    this.world.addNight(lanternMat, 'emissiveIntensity', 1.2, 5.5);
    const lg = new THREE.CylinderGeometry(1.35, 1.35, 1.9, 16);
    lg.translate(L.x, y + H + 1.35, L.z);
    const lantern = new THREE.Mesh(lg, lanternMat);
    this.group.add(lantern);
    // Keeper's cottage beside the tower, facing the road.
    const toRoad = Math.atan2(S.rx, S.rz); // local +z points roughly toward the road (road is to the right of the pad)
    const wall = new THREE.MeshStandardMaterial({ color: 0xf1ede4, roughness: 0.85 });
    const roof = new THREE.MeshStandardMaterial({ color: 0x9b2a24, roughness: 0.8 });
    const glass = new THREE.MeshStandardMaterial({ color: 0x3a3226, roughness: 0.25, emissive: 0xffc27a, emissiveIntensity: 0.4 });
    this.world.addNight(glass, 'emissiveIntensity', 0.3, 1.8);
    const cx = L.x + S.fx * 9 + S.rx * 3, cz = L.z + S.fz * 9 + S.rz * 3;
    const P = (geo, lx, ly, lz) => { geo.translate(lx, ly, lz); return placed(geo, cx, y, cz, toRoad); };
    const wg = [P(new THREE.BoxGeometry(9, 3.6, 6), 0, 1.8, 0), P(new THREE.BoxGeometry(2.2, 5.2, 1.2), 3, 2.6, -2.2)];
    const rs = new THREE.Shape();
    rs.moveTo(-3.4, 0); rs.lineTo(0, 2.1); rs.lineTo(3.4, 0); rs.closePath();
    const rg = new THREE.ExtrudeGeometry(rs, { depth: 9.6, bevelEnabled: false });
    rg.translate(0, 0, -4.8);
    rg.rotateY(Math.PI / 2);
    const rgeos = [P(rg, 0, 3.6, 0)];
    const gg = [];
    for (const lx of [-3, -1, 2.5]) gg.push(P(new THREE.PlaneGeometry(1.1, 1.3), lx, 2.0, 3.02));
    gg.push(P(new THREE.PlaneGeometry(1.0, 2.1), 0.8, 1.05, 3.02));
    for (const [geos2, mat, cast] of [[wg, wall, true], [rgeos, roof, true], [gg, glass, false]]) {
      const m = mergedMesh(geos2, mat, { cast });
      if (m) this.group.add(m);
    }

    // Rotating beam: twin cones from the lantern, plus a glow.
    const beamLen = 170;
    const beamGeo = new THREE.ConeGeometry(6, beamLen, 24, 1, true);
    beamGeo.translate(0, -beamLen / 2, 0); // apex at origin
    beamGeo.rotateX(Math.PI / 2);         // extends along −Z
    const beamMat = new THREE.ShaderMaterial({
      vertexShader: beamVert, fragmentShader: beamFrag,
      uniforms: THREE.UniformsUtils.merge([THREE.UniformsLib.fog, { uColor: { value: new THREE.Color(1.0, 0.93, 0.78) }, uStrength: { value: 0.5 } }]),
      transparent: true, depthWrite: false, blending: THREE.AdditiveBlending, side: THREE.DoubleSide, fog: true,
    });
    beamMat.userData.kind = 'LighthouseBeam'; // MaterialKind for the scene export
    const pivot = new THREE.Group();
    pivot.position.set(L.x, y + H + 1.35, L.z);
    const b1 = new THREE.Mesh(beamGeo, beamMat);
    const b2 = new THREE.Mesh(beamGeo, beamMat);
    b2.rotation.y = Math.PI;
    b1.rotation.x = b2.rotation.x = -0.03; // aimed a touch down at the water
    b1.frustumCulled = b2.frustumCulled = false;
    pivot.add(b1, b2);
    const glowMat = new THREE.SpriteMaterial({ map: glowTexture(), color: 0xffe7b0, transparent: true, blending: THREE.AdditiveBlending, depthWrite: false });
    const glow = new THREE.Sprite(glowMat);
    glow.scale.setScalar(9);
    glow.position.copy(pivot.position);
    this.group.add(pivot, glow);
    this.world.updaters.push((dt, night) => {
      pivot.rotation.y += dt * 0.9;
      beamMat.uniforms.uStrength.value = 0.03 + 0.32 * smoothstep(0.2, 0.8, night);
      glowMat.opacity = 0.25 + 0.75 * night;
      glow.scale.setScalar(6 + 8 * night);
    });
  }

  // ── Start gantry: HOLLOW POINT ─────────────────────────────────────
  buildStartArea() {
    const t = this.track, S = {};
    const s = t.startS;
    t.frame(s, S);
    const wl = t.wallL[t.idx(s)], wr = t.wallR[t.idx(s)];
    const yaw = Math.atan2(S.fx, S.fz);
    const steel = new THREE.MeshStandardMaterial({ color: 0x23282e, metalness: 0.7, roughness: 0.35 });
    const geos = [];
    const H = 7.2;
    for (const lat of [-(wl + 1.25), wr + 1.25]) {
      const x = S.x + S.rx * lat, z = S.z + S.rz * lat;
      const y = S.y - lat * S.bank;
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
      geos.push(placed(foot, x, y - 0.05, z, yaw));
    }
    const span = wl + wr + 2.5;
    const cx = S.x + S.rx * ((wr - wl) / 2), cz = S.z + S.rz * ((wr - wl) / 2);
    for (const dy of [H, H - 0.9]) {
      const beam = new THREE.BoxGeometry(span, 0.18, 0.18);
      beam.translate(0, S.y + dy, 0);
      geos.push(placed(beam, cx, 0, cz, yaw));
    }
    this.group.add(mergedMesh(geos, steel, { cast: true }));

    const bannerTex = (front) => {
      const [c, g] = makeCanvas(1024, 160);
      g.fillStyle = '#0a1016'; g.fillRect(0, 0, 1024, 160);
      const sq = 20;
      for (let y = 0; y < 160; y += sq) for (let x = 0; x < 120; x += sq) {
        g.fillStyle = ((x + y) / sq) % 2 ? '#f2f2f2' : '#111';
        g.fillRect(x, y, sq, sq); g.fillRect(1024 - 120 + x, y, sq, sq);
      }
      const grd = g.createLinearGradient(120, 0, 904, 0);
      grd.addColorStop(0, '#2ec4b6'); grd.addColorStop(1, '#ff9a3c');
      g.fillStyle = grd; g.fillRect(120, 0, 784, 8); g.fillRect(120, 152, 784, 8);
      g.fillStyle = '#fff'; g.textAlign = 'center'; g.textBaseline = 'middle';
      g.font = `italic 900 84px ${FONT}`;
      g.fillText(front ? 'HOLLOW POINT' : 'START', 512, 70);
      g.font = `bold 26px ${FONT}`; g.fillStyle = '#7fe3d8';
      g.fillText(front ? 'STAGE 2 · COAST HIGHWAY' : 'HOLLOW POINT', 512, 132);
      return canvasTex(c);
    };
    const bw = span - 1.6, bh = bw * 160 / 1024;
    for (const front of [true, false]) {
      const tex = bannerTex(front);
      const mat = new THREE.MeshStandardMaterial({ map: tex, roughness: 0.7, emissive: 0xffffff, emissiveMap: tex, emissiveIntensity: 0.2 });
      this.world.addNight(mat, 'emissiveIntensity', 0.15, 0.7);
      const g = new THREE.PlaneGeometry(bw, bh);
      g.rotateY(front ? Math.PI : 0);
      g.translate(0, S.y + H + 0.1 - bh / 2, front ? -0.05 : 0.05);
      const mesh = new THREE.Mesh(placed(g, cx, 0, cz, yaw), mat);
      mesh.matrixAutoUpdate = false;
      this.group.add(mesh);
    }
  }

  // ── Coffee shack and a surf van at the start pull-out ──────────────
  async buildPullout() {
    const d = this.pullout;
    if (!d) return;
    const t = this.track, S = {};
    t.frame(d.s, S);
    const y = d.y;
    const yaw = Math.atan2(-S.rx * d.side, -S.rz * d.side); // local +z faces the road
    const g0 = new THREE.CircleGeometry(13, 28);
    g0.rotateX(-Math.PI / 2);
    g0.translate(d.x, y + 0.06, d.z);
    const gravel = new THREE.MeshStandardMaterial({ map: gravelTexture().clone(), roughness: 1, color: 0xb9ae9c, polygonOffset: true, polygonOffsetFactor: -1 });
    gravel.map.repeat.set(5, 5);
    gravel.map.needsUpdate = true;
    const pad = new THREE.Mesh(g0, gravel);
    pad.receiveShadow = true;
    this.group.add(pad);

    const wood = new THREE.MeshStandardMaterial({ color: 0x8a6446, roughness: 0.9 });
    const roofM = new THREE.MeshStandardMaterial({ color: 0x3f4a4f, roughness: 0.7, metalness: 0.3 });
    const paint = new THREE.MeshStandardMaterial({ color: 0x2ec4b6, roughness: 0.6 });
    const warm = new THREE.MeshStandardMaterial({ color: 0x3a2e22, emissive: 0xffb866, emissiveIntensity: 0.6, roughness: 0.4 });
    this.world.addNight(warm, 'emissiveIntensity', 0.35, 1.0);
    const bx = d.x - S.rx * 2 - S.fx * 3, bz = d.z - S.rz * 2 - S.fz * 3;
    const P = (geo, lx, ly, lz) => { geo.translate(lx, ly, lz); return placed(geo, bx, y, bz, yaw); };
    const wg = [P(new THREE.BoxGeometry(5, 3, 3.6), 0, 1.5, 0)];
    // Shed roof, sloping back.
    const roofG = new THREE.BoxGeometry(6, 0.18, 4.8);
    roofG.rotateX(-0.16);
    const rgeos = [P(roofG, 0, 3.35, 0.2)];
    const pg = [P(new THREE.BoxGeometry(5.05, 0.45, 3.65), 0, 2.75, 0), P(new THREE.BoxGeometry(3.4, 0.12, 0.7), 0, 1.1, 2.1)];
    const side = new THREE.PlaneGeometry(0.9, 0.8);
    side.rotateY(-Math.PI / 2);
    const lit = [P(new THREE.PlaneGeometry(3, 1.2), 0, 1.85, 1.82), P(side, -2.52, 1.9, 0)];
    // Picnic table.
    wg.push(P(new THREE.BoxGeometry(2.2, 0.08, 0.9), 4.5, 0.8, 3.5), P(new THREE.BoxGeometry(2.2, 0.06, 0.35), 4.5, 0.48, 2.75), P(new THREE.BoxGeometry(2.2, 0.06, 0.35), 4.5, 0.48, 4.25));
    for (const lx of [3.6, 5.4]) wg.push(P(new THREE.BoxGeometry(0.1, 0.8, 1.6), lx, 0.4, 3.5));
    for (const [geos, mat, cast] of [[wg, wood, true], [rgeos, roofM, true], [pg, paint, true], [lit, warm, false]]) {
      const m = mergedMesh(geos, mat, { cast });
      if (m) this.group.add(m);
    }
    // Sign over the window.
    const [c, g] = makeCanvas(512, 128);
    g.fillStyle = '#0f1a1c'; roundRect(g, 4, 4, 504, 120, 16); g.fill();
    g.strokeStyle = '#2ec4b6'; g.lineWidth = 5; roundRect(g, 12, 12, 488, 104, 12); g.stroke();
    g.fillStyle = '#ffe2b8'; g.textAlign = 'center'; g.textBaseline = 'middle';
    g.font = `italic 900 54px ${FONT}`; g.fillText('HOLLOW PT. COFFEE', 256, 56);
    g.font = `bold 24px ${FONT}`; g.fillStyle = '#7fe3d8'; g.fillText('SURF · COFFEE · BAIT', 256, 100);
    const tex = canvasTex(c);
    const signMat = new THREE.MeshStandardMaterial({ map: tex, emissive: 0xffffff, emissiveMap: tex, emissiveIntensity: 0.5, roughness: 0.5 });
    this.world.addNight(signMat, 'emissiveIntensity', 0.3, 1.8);
    this.group.add(new THREE.Mesh(P(new THREE.PlaneGeometry(4.4, 1.1), 0, 3.25, 1.95), signMat));

    // String lights between two posts along the front.
    const bulbs = [];
    const postA = new THREE.Vector3(-4, 3.2, 4.2), postB = new THREE.Vector3(7, 3.2, 5.2);
    const poles = [];
    for (const p of [postA, postB]) poles.push(P(new THREE.CylinderGeometry(0.06, 0.06, 3.3, 5), p.x, 1.65, p.z));
    this.group.add(mergedMesh(poles, wood));
    for (let k = 0; k <= 14; k++) {
      const f = k / 14;
      const lx = lerp(postA.x, postB.x, f), lz = lerp(postA.z, postB.z, f);
      const ly = 3.1 - Math.sin(f * Math.PI) * 0.6;
      const v = new THREE.Vector3(lx, ly, lz).applyAxisAngle(new THREE.Vector3(0, 1, 0), yaw);
      bulbs.push({ x: bx + v.x, y: y + v.y, z: bz + v.z, sx: 1, sy: 1, sz: 1 });
    }
    const bulbMat = new THREE.MeshBasicMaterial({ color: new THREE.Color(3.5, 2.4, 1.2) });
    this.world.updaters.push((dt, night) => bulbMat.color.setRGB(3.5, 2.4, 1.2).multiplyScalar(0.25 + night));
    this.group.add(instanced(new THREE.IcosahedronGeometry(0.09, 0), bulbMat, bulbs, { receive: false }));

    // Vintage surf van with boards on the roof.
    try {
      const { buildVehicle } = await import('../vehicles/CarModel.js');
      const v = buildVehicle('van', { color: 0x9fd8cf, seed: 21, lod: 'low' });
      const lp = new THREE.Vector3(-6.5, 0, 4.5).applyAxisAngle(new THREE.Vector3(0, 1, 0), yaw);
      v.root.position.set(d.x + lp.x, y + 0.05, d.z + lp.z);
      v.root.rotation.y = yaw + 1.3;
      v.setHeadlights?.(0);
      const boards = new THREE.Group();
      const cols = [0xf2c14e, 0xff6b6b, 0xfafafa];
      const H = v.dims?.height ?? 2.2;
      cols.forEach((col, k) => {
        const b = new THREE.Mesh(new THREE.CapsuleGeometry(0.28, 2.3, 3, 8), new THREE.MeshStandardMaterial({ color: col, roughness: 0.4 }));
        b.scale.set(1, 1, 0.18);
        b.rotation.x = Math.PI / 2;
        b.position.set((k - 1) * 0.62, H + 0.12 + k * 0.05, 0);
        boards.add(b);
      });
      v.body.add(boards);
      this.group.add(bakeStatic(v.root));
    } catch (e) { /* optional */ }
  }

  // ── Vista pull-out on the bluff ────────────────────────────────────
  async buildVista() {
    const L = this.vista;
    if (!L) return;
    const t = this.track, S = {};
    t.frame(L.s, S);
    const out = { x: S.rx * L.side, z: S.rz * L.side };
    const yaw = Math.atan2(out.x, out.z);
    const y = L.y;
    const gravel = new THREE.MeshStandardMaterial({ map: gravelTexture().clone(), roughness: 1, color: 0xa99f90, polygonOffset: true, polygonOffsetFactor: -1 });
    gravel.map.repeat.set(4, 4);
    gravel.map.needsUpdate = true;
    const g0 = new THREE.CircleGeometry(9.6, 28);
    g0.rotateX(-Math.PI / 2);
    g0.translate(L.x, y + 0.06, L.z);
    const pad = new THREE.Mesh(g0, gravel);
    pad.receiveShadow = true;
    this.group.add(pad);
    const stones = [];
    for (let a = -1.3; a <= 1.3; a += 0.1) {
      const r = 9.2;
      const bx = new THREE.BoxGeometry(1.2, 0.75 + Math.abs(Math.sin(a * 11)) * 0.15, 0.7);
      bx.rotateY(a);
      bx.translate(Math.sin(a) * r, 0.37, Math.cos(a) * r);
      stones.push(placed(bx, L.x, y, L.z, yaw));
    }
    const stoneMat = rockMaterial();
    stoneMat.color.setHex(0xcdbfa8);
    this.group.add(mergedMesh(stones, stoneMat, { cast: true }));
    const metal = new THREE.MeshStandardMaterial({ color: 0x3d6b6b, metalness: 0.6, roughness: 0.4 });
    const wood = new THREE.MeshStandardMaterial({ color: 0x6d5038, roughness: 0.9 });
    const P = (geo, lx, ly, lz) => { geo.translate(lx, ly, lz); return placed(geo, L.x, y, L.z, yaw); };
    const mg = [
      P(new THREE.CylinderGeometry(0.08, 0.12, 1.1, 8), 1.5, 0.55, 8.2),
      P(new THREE.BoxGeometry(0.5, 0.35, 0.7), 1.5, 1.25, 8.2),
    ];
    const wg = [P(new THREE.BoxGeometry(2.2, 0.08, 0.5), -2.5, 0.5, 7.2), P(new THREE.BoxGeometry(2.2, 0.5, 0.08), -2.5, 0.8, 7.45)];
    for (const lx of [-3.4, -1.6]) wg.push(P(new THREE.BoxGeometry(0.1, 0.5, 0.5), lx, 0.25, 7.2));
    this.group.add(mergedMesh(mg, metal, { cast: true }));
    this.group.add(mergedMesh(wg, wood, { cast: true }));
    try {
      const { buildVehicle } = await import('../vehicles/CarModel.js');
      const v = buildVehicle('hatch', { color: 0xd9a441, seed: 8, lod: 'low' });
      const lp = new THREE.Vector3(3.5, 0, 2).applyAxisAngle(new THREE.Vector3(0, 1, 0), yaw);
      v.root.position.set(L.x + lp.x, y + 0.05, L.z + lp.z);
      v.root.rotation.y = yaw - 0.2;
      v.setHeadlights?.(0);
      this.group.add(bakeStatic(v.root));
    } catch (e) { /* optional */ }
  }

  // ── Road signs ─────────────────────────────────────────────────────
  buildSigns() {
    const t = this.track;
    const atlas = new SignAtlas();
    const signs = [];
    const add = (s, rect, w, h, height = 2.1, side = 1, extraPlate = null) => signs.push({ s, rect, w, h, height, side, extraPlate });
    const tagD = (r) => { r.diamond = true; return r; };
    const curve = (dir) => tagD(atlas.add(256, 256, (g, w, h) => diamond(g, w, h, (g2, cx, cy) => {
      g2.save(); g2.translate(cx, cy); g2.scale(dir, 1);
      g2.lineWidth = 17; g2.lineCap = 'butt';
      g2.beginPath(); g2.moveTo(-8, 60); g2.lineTo(-8, 8); g2.quadraticCurveTo(-8, -26, 26, -34); g2.stroke();
      g2.beginPath(); g2.moveTo(18, -60); g2.lineTo(54, -34); g2.lineTo(16, -8); g2.closePath(); g2.fill();
      g2.restore();
    })));
    const textDiamond = (lines, size) => tagD(atlas.add(256, 256, (g, w, h) => diamond(g, w, h, (g2, cx, cy) => {
      g2.textAlign = 'center'; g2.textBaseline = 'middle'; g2.font = `bold ${size}px ${FONT}`;
      lines.forEach((l, i) => g2.fillText(l, cx, cy + (i - (lines.length - 1) / 2) * size * 1.05));
    })));
    const mph = (n) => atlas.add(200, 240, (g, w, h) => panel(g, w, h, '#f5c518', '#111', '#111', ['' + n, 'MPH'], [110, 52]));
    const curveL = curve(-1), curveR = curve(1);
    const adv = { 25: mph(25), 35: mph(35) };
    // Warn before every sharp bend: sharpness from the laid segments.
    const segs = this.world.level.segments;
    let s0 = 0;
    for (const [len, turn] of segs) {
      if (s0 > this.zEnd) break;
      const sharp = Math.abs(turn) / len;
      if (sharp > 0.36 && s0 > 120) {
        const plate = sharp > 0.55 ? adv[25] : sharp > 0.42 ? adv[35] : null;
        add(s0 - 70, turn < 0 ? curveL : curveR, 1.2, 1.2, plate ? 2.5 : 2.2, 1, plate ? { rect: plate, w: 0.75, h: 0.9 } : null);
      }
      s0 += len;
    }
    // Falling rocks where the land side is a long rock face.
    const falling = textDiamond(['FALLING', 'ROCKS'], 44);
    let runStart = -1, placedFR = 0;
    for (let s = 100; s < this.zEnd && placedFR < 3; s += 5) {
      if (t.sideR[t.idx(s)] === 1) { if (runStart < 0) runStart = s; if (s - runStart > 90) { add(runStart - 40, falling, 1.2, 1.2, 2.4); placedFR++; runStart = 1e9; } }
      else if (runStart !== 1e9) runStart = -1;
      if (runStart === 1e9 && t.sideR[t.idx(s)] !== 1) runStart = -1;
    }
    // Speed limit, turnouts, vista, distances.
    const lim = atlas.add(200, 256, (g, w, h) => panel(g, w, h, '#f8f8f4', '#111', '#111', ['SPEED', 'LIMIT', '45'], [44, 44, 96]));
    add(230, lim, 0.9, 1.15, 2.1);
    const turnouts = atlas.add(512, 256, (g, w, h) => panel(g, w, h, '#f8f8f4', '#111', '#111', ['SLOWER TRAFFIC', 'USE TURNOUTS'], [60, 60]));
    add(1100, turnouts, 2.4, 1.2, 2.0);
    if (this.vista) {
      const v1 = atlas.add(512, 256, (g, w, h) => panel(g, w, h, '#5a3a22', '#f3ead8', '#f3ead8', ['VISTA POINT', '1/4 MILE'], [66, 54]));
      add(this.vista.s - 420, v1, 2.4, 1.2, 2.0);
      const v2 = atlas.add(512, 256, (g, w, h) => {
        panel(g, w, h, '#5a3a22', '#f3ead8', '#f3ead8', ['VISTA POINT'], [70]);
        g.fillStyle = '#f3ead8';
        g.beginPath(); g.moveTo(96, 200); g.lineTo(60, 178); g.lineTo(96, 156); g.closePath(); g.fill();
        g.fillRect(96, 172, 60, 12);
      });
      add(this.vista.s - 90, v2, 2.4, 1.2, 2.0);
    }
    if (this.lighthouse) {
      const lh = atlas.add(512, 256, (g, w, h) => panel(g, w, h, '#5a3a22', '#f3ead8', '#f3ead8', ['HOLLOW POINT', 'LIGHT STATION'], [58, 52]));
      add(this.lighthouse.s - 260, lh, 2.4, 1.2, 2.0);
    }
    const miles = (s) => Math.max(1, Math.round((this.zEnd - s) / 1609));
    const portS = t.tag('bridge')[0]?.s0 ?? this.zEnd + 3000;
    const dist = atlas.add(512, 256, (g, w, h) => {
      panel(g, w, h, '#0b6b3a', '#fff', '#fff', [], []);
      g.fillStyle = '#fff'; g.font = `bold 58px ${FONT}`; g.textBaseline = 'middle';
      g.textAlign = 'left'; g.fillText('Seabright', 36, 88); g.fillText('Port Meridian', 36, 172);
      g.textAlign = 'right'; g.fillText('' + miles(300), w - 36, 88); g.fillText('' + Math.max(2, Math.round((portS - 300) / 1609)), w - 36, 172);
    });
    add(300, dist, 3.2, 1.6, 2.2);
    const dist2 = atlas.add(512, 160, (g, w, h) => {
      panel(g, w, h, '#0b6b3a', '#fff', '#fff', [], []);
      g.fillStyle = '#fff'; g.font = `bold 60px ${FONT}`; g.textBaseline = 'middle';
      g.textAlign = 'left'; g.fillText('Seabright', 36, 82);
      g.textAlign = 'right'; g.fillText('' + miles(this.zEnd - 1800), w - 36, 82);
    });
    add(this.zEnd - 1800, dist2, 3.2, 1.0, 2.2);

    const tex = atlas.texture();
    const faceMat = new THREE.MeshStandardMaterial({ map: tex, roughness: 0.45, metalness: 0.1, emissive: 0xffffff, emissiveMap: tex, emissiveIntensity: 0.3, alphaTest: 0.5 });
    this.world.addNight(faceMat, 'emissiveIntensity', 0.04, 0.55);
    const metalMat = new THREE.MeshStandardMaterial({ color: 0x8d9197, metalness: 0.6, roughness: 0.45 });
    const faces = [], metal = [];
    const S = {}, H = {};
    const plate = (rect, w, h, x, y, z, yaw) => {
      const g = new THREE.PlaneGeometry(w, h);
      const uv = g.getAttribute('uv');
      for (let k = 0; k < uv.count; k++) uv.setXY(k, lerp(rect.u0, rect.u1, uv.getX(k)), lerp(rect.v0, rect.v1, uv.getY(k)));
      faces.push(placed(g, x, y, z, yaw));
      const back = rect.diamond ? new THREE.PlaneGeometry(w * 0.69, h * 0.69).rotateZ(Math.PI / 4) : new THREE.PlaneGeometry(w, h);
      back.rotateY(Math.PI);
      back.translate(0, 0, -0.02);
      metal.push(placed(back, x, y, z, yaw));
    };
    for (const sg of signs) {
      if (sg.s < 5 || sg.s > this.zEnd) continue;
      t.frame(sg.s, S);
      const i = t.idx(sg.s);
      const wall = sg.side > 0 ? t.wallR[i] : t.wallL[i];
      const kind = sg.side > 0 ? t.sideR[i] : t.sideL[i];
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
      if (sg.w > 2) {
        const off = sg.w * 0.32;
        const px2 = Math.cos(yaw) * off, pz2 = -Math.sin(yaw) * off;
        metal.push(placed(post, x + dx + px2, 0, z + dz + pz2, yaw), placed(post, x + dx - px2, 0, z + dz - pz2, yaw));
      } else metal.push(placed(post, x + dx, 0, z + dz, yaw));
    }
    const fm = mergedMesh(faces, faceMat);
    const mm = mergedMesh(metal, metalMat, { cast: true });
    if (fm) this.group.add(fm);
    if (mm) this.group.add(mm);
  }

  // ── Wooden utility poles on the land side, with sagging wires ──────
  buildPoles() {
    const t = this.track, S = {};
    const poles = [], wires = [];
    let prev = null;
    for (let s = 90; s < this.zEnd + 60; s += 46) {
      t.frame(s, S);
      const lat = S.wallR + 3.2;
      const x = S.x + S.rx * lat, z = S.z + S.rz * lat;
      const roadY = S.y - lat * S.bank;
      const g = this.surf.sample(x, z).h;
      if (g > roadY + 5 || g < roadY - 6 || !this.clearOfRoad(x, z, 0.3) || this.excluded(x, z)) { prev = null; continue; }
      const yaw = Math.atan2(S.fx, S.fz);
      poles.push({ x, y: g - 0.3, z, sx: 1, sy: 1, sz: 1, ry: yaw });
      const top = g - 0.3 + 9.4;
      const ends = [-0.95, 0.95].map((o) => ({ x: x + Math.cos(yaw) * o, y: top, z: z - Math.sin(yaw) * o }));
      if (prev) {
        for (let k = 0; k < 2; k++) {
          const a = prev[k], b = ends[k];
          const span = Math.hypot(b.x - a.x, b.z - a.z);
          const sag = span * 0.018;
          for (let j = 0; j < 8; j++) {
            const f0 = j / 8, f1 = (j + 1) / 8;
            const y0 = lerp(a.y, b.y, f0) - Math.sin(f0 * Math.PI) * sag, y1 = lerp(a.y, b.y, f1) - Math.sin(f1 * Math.PI) * sag;
            wires.push(lerp(a.x, b.x, f0), y0, lerp(a.z, b.z, f0), lerp(a.x, b.x, f1), y1, lerp(a.z, b.z, f1));
          }
        }
      }
      prev = ends;
    }
    if (!poles.length) return;
    const pole = new THREE.CylinderGeometry(0.12, 0.17, 10, 6);
    pole.translate(0, 5, 0);
    const arm = new THREE.BoxGeometry(2.3, 0.12, 0.12);
    arm.translate(0, 9.4, 0);
    const geo = mergeGeometries([pole.toNonIndexed(), arm.toNonIndexed()].map((g) => { g.deleteAttribute('uv'); return g; }), false);
    const mat = new THREE.MeshStandardMaterial({ color: 0x5a4432, roughness: 0.9 });
    this.group.add(instanced(geo, mat, poles, { cast: true }));
    const lg = new THREE.BufferGeometry();
    lg.setAttribute('position', new THREE.Float32BufferAttribute(wires, 3));
    const lines = new THREE.LineSegments(lg, new THREE.LineBasicMaterial({ color: 0x1b1d20 }));
    lines.matrixAutoUpdate = false;
    this.group.add(lines);
  }

  // ── Fishing boats offshore with running lights ─────────────────────
  buildBoats() {
    const t = this.track, T = this.terrain, rng = mulberry32(88);
    const boats = [];
    for (let k = 0; k < 9 && boats.length < 7; k++) {
      const s = lerp(200, this.zEnd - 200, (k + rng() * 0.6) / 9);
      const f = t.frame(s);
      const off = lerp(420, 1500, rng());
      const x = f.x - f.rx * off + f.fx * (rng() - 0.5) * 200, z = f.z - f.rz * off + f.fz * (rng() - 0.5) * 200;
      if (T.heightAt(x, z) > this.seaY - 6) continue;
      boats.push({ x, z, yaw: rng() * 6.28, ph: rng() * 6.28, col: [0xe8e4da, 0x2f5f8a, 0xb23a2e, 0xe8e4da][k % 4] });
    }
    if (!boats.length) return;
    // Hull (pointed bow), cabin, mast — one geometry with vertex colours.
    const shape = new THREE.Shape();
    shape.moveTo(-5.5, -1.6); shape.lineTo(3.4, -1.8); shape.lineTo(5.6, 0); shape.lineTo(3.4, 1.8); shape.lineTo(-5.5, 1.6); shape.closePath();
    const hull = new THREE.ExtrudeGeometry(shape, { depth: 1.7, bevelEnabled: false });
    hull.rotateX(-Math.PI / 2);
    hull.translate(0, -0.9, 0);
    const parts = [colorize(prep(hull), 0xffffff)];
    const cabin = prep(new THREE.BoxGeometry(3.2, 1.9, 2.5));
    cabin.translate(-1.2, 1.75, 0);
    parts.push(colorize(cabin, 0xe9e2d0));
    const mast = prep(new THREE.CylinderGeometry(0.07, 0.09, 6, 5));
    mast.translate(0.8, 3.8, 0);
    parts.push(colorize(mast, 0x555a60));
    const boom = prep(new THREE.CylinderGeometry(0.05, 0.05, 5, 5));
    boom.rotateZ(1.0);
    boom.translate(2.6, 3.2, 0);
    parts.push(colorize(boom, 0x555a60));
    const geo = mergeGeometries(parts, false);
    const mat = new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.6 });
    const im = new THREE.InstancedMesh(geo, mat, boats.length);
    const c = new THREE.Color();
    boats.forEach((b, k) => im.setColorAt(k, c.setHex(b.col)));
    im.instanceColor.needsUpdate = true;
    im.frustumCulled = false;
    this.group.add(im);
    // Running lights as fixed-size points so they read at a kilometre.
    const nl = boats.length * 3;
    const lpos = new Float32Array(nl * 3), lcol = new Float32Array(nl * 3);
    const LCOL = [[1.6, 1.5, 1.3], [1.8, 0.2, 0.15], [0.2, 1.7, 0.5]];
    for (let k = 0; k < nl; k++) lcol.set(LCOL[k % 3], k * 3);
    const lg = new THREE.BufferGeometry();
    lg.setAttribute('position', new THREE.BufferAttribute(lpos, 3));
    lg.setAttribute('color', new THREE.BufferAttribute(lcol, 3));
    const lmat = new THREE.PointsMaterial({ size: 5, sizeAttenuation: false, map: glowTexture(), vertexColors: true, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending });
    const pts = new THREE.Points(lg, lmat);
    pts.frustumCulled = false;
    this.group.add(pts);
    const m4 = new THREE.Matrix4(), q = new THREE.Quaternion(), e = new THREE.Euler(), p = new THREE.Vector3(), one = new THREE.Vector3(1, 1, 1), v = new THREE.Vector3();
    const LOCAL = [[0.8, 6.9, 0], [-1.2, 2.4, -1.3], [-1.2, 2.4, 1.3]];
    let time = 0;
    const update = (dt, night) => {
      time += dt;
      boats.forEach((b, k) => {
        const y = this.seaY + Math.sin(time * 0.8 + b.ph) * 0.22;
        e.set(Math.sin(time * 0.6 + b.ph) * 0.05, b.yaw + Math.sin(time * 0.1 + b.ph) * 0.05, Math.sin(time * 0.7 + b.ph * 2) * 0.03);
        q.setFromEuler(e);
        p.set(b.x, y, b.z);
        m4.compose(p, q, one);
        im.setMatrixAt(k, m4);
        for (let j = 0; j < 3; j++) {
          v.set(...LOCAL[j]).applyMatrix4(m4);
          lpos.set([v.x, v.y, v.z], (k * 3 + j) * 3);
        }
      });
      im.instanceMatrix.needsUpdate = true;
      lg.attributes.position.needsUpdate = true;
      lmat.opacity = 0.35 + 0.65 * night;
    };
    update(0, 0.8);
    this.world.updaters.push(update);
  }
}
