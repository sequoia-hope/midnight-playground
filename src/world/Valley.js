import * as THREE from 'three';
import { clamp, lerp, smoothstep, mulberry32, rrange, rpick, hash2, fbm, ridged, makeNoise2D } from '../util/math.js';
import { Builder, PaintBuilder } from './valley/Builder.js';
import { makeGround } from './valley/ground.js';
import {
  farmhouse, barn, silo, shed, windpumpTower, windpumpWheelGeometry, waterwheelGeometry,
  millHouse, generalStore, gasPump, mailbox, paddock, boardSign, cowGeometry, poleGeometry,
  towerMill, millSailsGeometry,
} from './valley/parts.js';
import { canopyGeometry, grassClumpGeometry, flowerGeometry, shrubGeometry, foliageMaterial } from './valley/flora.js';

// Old Mill Valley: farm country between the pass and the city. Weathered
// farmsteads set back from the road, orchards and windbreaks, hay in the
// fields, telephone poles along the verge, a creek (and the old mill it is
// named for) running under the road, and a general store at a crossroads.

const yawZ = (dx, dz) => Math.atan2(dx, dz);    // local +Z → (dx, dz)
const yawX = (dx, dz) => Math.atan2(-dz, dx);   // local +X → (dx, dz)

// Same parcel grid as TerrainMesh's field colours, so bales land in hay.
const FIELD_A = 0.38, FIELD_U = 110, FIELD_V = 160;
const toUV = (x, z) => [x * Math.cos(FIELD_A) - z * Math.sin(FIELD_A), x * Math.sin(FIELD_A) + z * Math.cos(FIELD_A)];
const fromUV = (u, v) => [u * Math.cos(FIELD_A) + v * Math.sin(FIELD_A), -u * Math.sin(FIELD_A) + v * Math.cos(FIELD_A)];

function segDist(px, pz, pts) {
  let best = Infinity, bi = 0, bt = 0;
  for (let i = 0; i < pts.length - 1; i++) {
    const a = pts[i], b = pts[i + 1];
    const abx = b.x - a.x, abz = b.z - a.z;
    const t = clamp(((px - a.x) * abx + (pz - a.z) * abz) / (abx * abx + abz * abz || 1), 0, 1);
    const dx = px - (a.x + abx * t), dz = pz - (a.z + abz * t);
    const d = dx * dx + dz * dz;
    if (d < best) { best = d; bi = i; bt = t; }
  }
  return { d: Math.sqrt(best), i: bi, t: bt };
}

function canvasTexture(w, h, draw) {
  const c = document.createElement('canvas');
  c.width = w; c.height = h;
  draw(c.getContext('2d'), w, h);
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = THREE.SRGBColorSpace;
  t.anisotropy = 8;
  return t;
}

function woodSign(lines, { w = 512, h = 192, bg = '#5a3f28', fg = '#f1e2c0', font = 'bold 64px Georgia, serif', sub = 'italic 30px Georgia, serif' } = {}) {
  return canvasTexture(w, h, (g) => {
    g.fillStyle = bg; g.fillRect(0, 0, w, h);
    // Planks
    for (let y = 0; y < h; y += h / 4) { g.fillStyle = 'rgba(0,0,0,0.18)'; g.fillRect(0, y, w, 3); }
    for (let k = 0; k < 60; k++) { g.fillStyle = `rgba(${Math.random() < 0.5 ? '255,230,200' : '0,0,0'},0.05)`; g.fillRect(Math.random() * w, Math.random() * h, 40 + Math.random() * 120, 2); }
    g.strokeStyle = fg; g.lineWidth = 5; g.strokeRect(12, 12, w - 24, h - 24);
    g.fillStyle = fg; g.textAlign = 'center'; g.textBaseline = 'middle';
    g.font = font; g.fillText(lines[0], w / 2, lines[1] ? h * 0.4 : h / 2);
    if (lines[1]) { g.font = sub; g.fillText(lines[1], w / 2, h * 0.72); }
  });
}

// Tileable ripple normals: a sum of integer-frequency waves wraps cleanly.
function waterNormalTexture() {
  const S = 128;
  const rng = mulberry32(77);
  const waves = [];
  for (let k = 0; k < 14; k++) {
    waves.push({ fx: Math.floor(rng() * 7) - 3, fy: Math.floor(rng() * 7) - 3 || 1, ph: rng() * Math.PI * 2, a: 0.6 / (1 + k * 0.25) });
  }
  const hgt = (x, y) => {
    let h = 0;
    for (const w of waves) h += w.a * Math.sin(((w.fx * x + w.fy * y) / S) * Math.PI * 2 + w.ph);
    return h;
  };
  return canvasTexture(S, S, (g) => {
    const img = g.createImageData(S, S);
    for (let y = 0; y < S; y++) for (let x = 0; x < S; x++) {
      const dx = hgt(x + 1, y) - hgt(x - 1, y);
      const dy = hgt(x, y + 1) - hgt(x, y - 1);
      const i = (y * S + x) * 4;
      img.data[i] = 128 + dx * 60; img.data[i + 1] = 128 + dy * 60; img.data[i + 2] = 255; img.data[i + 3] = 255;
    }
    g.putImageData(img, 0, 0);
  });
}

// World-space surface detail for the painted building buckets, so walls
// read as clapboard or barn boards and roofs as courses of shingles without
// a texture per building. Fades out where the pattern would alias.
function surfaceDetail(mat, mode) {
  mat.onBeforeCompile = (sh) => {
    sh.vertexShader = sh.vertexShader
      .replace('#include <common>', '#include <common>\nvarying vec3 vWP;\nvarying vec3 vWN;')
      .replace('#include <fog_vertex>', '#include <fog_vertex>\nvWP = (modelMatrix * vec4(transformed, 1.0)).xyz;\nvWN = normalize(mat3(modelMatrix) * objectNormal);');
    const body = {
      // Horizontal lap siding: a shadow line under each board's lip.
      siding: `float p_ = vWP.y / 0.23; float f_ = fract(p_);
        float a_ = (1.0 - smoothstep(0.25, 0.6, fwidth(p_))) * (1.0 - abs(vWN.y));
        diffuseColor.rgb *= 1.0 - a_ * (0.22 * smoothstep(0.78, 1.0, f_) - 0.05 * f_);`,
      // Vertical boards with grooves and a little plank-to-plank tone.
      boards: `float u_ = (abs(vWN.x) > abs(vWN.z) ? vWP.z : vWP.x) / 0.32; float f_ = fract(u_);
        float a_ = (1.0 - smoothstep(0.25, 0.6, fwidth(u_))) * (1.0 - abs(vWN.y));
        float h_ = fract(sin(floor(u_) * 91.7) * 4373.1);
        float g_ = smoothstep(0.0, 0.08, f_) * smoothstep(1.0, 0.92, f_);
        diffuseColor.rgb *= 1.0 - a_ * (0.3 * (1.0 - g_) + 0.12 * h_);`,
      // Shingle courses: a dark line per course and staggered tile tones.
      roof: `float p_ = vWP.y / 0.17; float f_ = fract(p_);
        float q_ = (vWP.x + vWP.z) / 0.45 + floor(p_) * 0.5;
        float a_ = (1.0 - smoothstep(0.25, 0.6, fwidth(p_))) * smoothstep(0.1, 0.4, abs(vWN.y));
        float h_ = fract(sin(floor(q_) * 12.9 + floor(p_) * 78.2) * 43758.5);
        diffuseColor.rgb *= 1.0 - a_ * (0.3 * smoothstep(0.75, 1.0, f_) + 0.14 * h_);`,
    }[mode];
    sh.fragmentShader = sh.fragmentShader
      .replace('#include <common>', '#include <common>\nvarying vec3 vWP;\nvarying vec3 vWN;')
      .replace('#include <color_fragment>', '#include <color_fragment>\n' + body);
  };
  mat.customProgramCacheKey = () => 'valley-' + mode;
  return mat;
}

export default class Valley {
  constructor() {
    this.label = 'Planting the valley';
    this.rng = mulberry32(4242);
  }

  // ── Planning (before terrain heights exist) ─────────────────────
  plan(world) {
    const tp = performance.now();
    const t = world.track, T = world.terrain;
    this.t = t; this.T = T;
    this.v0 = t.zoneStart[1];
    this.v1 = t.zoneStart[2];
    this.pads = [];
    this.planCreek();
    this.planFarms();
    this.planStore();
    this.planMill();
    this.planWindmill();
    for (const p of this.pads) T.addFlatten(p.x, p.z, p.r, p.falloff, p.y);
    // Open the roadside fence at driveway mouths and the crossroads.
    for (const f of this.farms) t.fenceGaps.push({ s0: f.s - 2.4, s1: f.s + 2.4, side: f.side });
    if (this.store) for (const side of [-1, 1]) t.fenceGaps.push({ s0: this.store.s - 3.6, s1: this.store.s + 3.6, side });
    T.addCarve(this.creek, this.creekWidth, this.creekDepth, { underRoad: true });
    this.planMs = Math.round(performance.now() - tp);
  }

  // Distance from a point to the nearest road, and whether that road is
  // the stretch around s (so we can tell the inside of a bend from open land).
  clearOfRoad(x, z, margin, s = null, window = 60) {
    const t = this.t;
    const k = t.nearest(x, z, margin + 20);
    if (k < 0) return true;
    const p = t.project(x, z, k, {}, 6);
    const wall = p.lat > 0 ? t.wallR[p.i] : t.wallL[p.i];
    if (Math.abs(p.lat) - wall < margin) return false;
    if (s !== null && Math.abs(p.s - s) > window && Math.abs(p.lat) < margin + wall + 10) return false;
    return true;
  }

  inValley(x, need = 0.6) {
    return this.T.zoneWeights(x)[1] >= need;
  }

  // Approximate valley landform, usable before the terrain fields exist
  // (mirrors Terrain.landform's valley branch). Used to route the creek
  // along low ground so its channel can actually hold water.
  approxLand(x, z) {
    const t = this.t, T = this.T;
    const k = t.nearest(x, z, 360);
    let d = 360, fy = this._fy ?? t.py[this.creekS ?? t.zoneStart[1]];
    if (k >= 0) { d = Math.hypot(x - t.px[k], z - t.pz[k]); fy = this._fy = t.py[k]; }
    const roll = fbm(T.noise, x / 380, z / 380, 3) * 7 * smoothstep(15, 90, d);
    const hills = smoothstep(260, 1500, d) * (40 + 380 * ridged(T.noise, x / 1700 + 9.1, z / 1700, 5));
    return fy - 1.2 + roll + hills;
  }

  planCreek() {
    const t = this.t;
    const br = t.tag('bridge')[0];
    const sc = Math.round((br.s0 + br.s1) / 2);
    this.creekS = sc;
    const f = t.frame(sc);
    const STEP = 12;
    const along = (sgn) => {
      const out = [];
      let heading = Math.atan2(f.rz * sgn, f.rx * sgn); // leave perpendicular to the road
      let x = f.x, z = f.z;
      for (let k = 1; k < 70; k++) {
        const dist = k * STEP;
        if (dist > 72) {
          // Greedy: of the headings in a forward cone, take the lowest ground
          // (looking two steps ahead), with a mild penalty for turning.
          let best = heading, bestH = Infinity;
          for (const da of [-0.6, -0.4, -0.2, 0, 0.2, 0.4, 0.6]) {
            const h2 = heading + da;
            const x1 = x + Math.cos(h2) * STEP, z1 = z + Math.sin(h2) * STEP;
            const x2 = x1 + Math.cos(h2) * STEP, z2 = z1 + Math.sin(h2) * STEP;
            const v = this.approxLand(x1, z1) + 0.6 * this.approxLand(x2, z2) + Math.abs(da) * 0.8;
            if (v < bestH) { bestH = v; best = h2; }
          }
          heading = heading + (best - heading) * 0.7;
        }
        x += Math.cos(heading) * STEP; z += Math.sin(heading) * STEP;
        if (dist > 60) {
          const kk = t.nearest(x, z, 70);
          if (kk >= 0 && Math.abs(kk - sc) > 120) break;
        }
        if (!this.inValley(x, 0.75)) break;
        // Don't loop back on ourselves.
        if (out.slice(0, -4).some((p) => Math.hypot(p.x - x, p.z - z) < 36)) break;
        out.push({ x, z });
      }
      return out;
    };
    const a = along(-1).reverse(), b = along(1);
    this.creek = [...a, { x: f.x, z: f.z }, ...b];
    this.creekCross = a.length; // index of the crossing point
    this.creekWidth = 13;
    this.creekDepth = 3.4;
  }

  creekDist(x, z) { return segDist(x, z, this.creek).d; }

  padOK(x, z, r) {
    for (const p of this.pads) if (Math.hypot(x - p.x, z - p.z) < r + p.r + 12) return false;
    return this.creekDist(x, z) > r + 22 && this.inValley(x, 0.7);
  }

  planFarms() {
    const t = this.t, rng = this.rng;
    const farmTags = t.tag('farm').map((g) => Math.round((g.s0 + g.s1) / 2));
    const crest = t.tag('crest')[0];
    const extra = crest ? crest.s1 + 110 : null;
    const list = extra ? [farmTags[0], extra, ...farmTags.slice(1)] : farmTags;
    this.farms = [];
    let prefer = 1;
    for (const s of list) {
      let placed = null;
      for (const side of [prefer, -prefer]) {
        for (const lat of [60, 70, 82, 96, 112]) {
          const c = t.pointAt(s, side * lat);
          if (!this.clearOfRoad(c.x, c.z, lat - 12, s, 80)) continue;
          if (!this.padOK(c.x, c.z, 34)) continue;
          placed = { s, side, lat, x: c.x, z: c.z };
          break;
        }
        if (placed) break;
      }
      if (!placed) continue;
      const road = t.pointAt(s, 0);
      placed.yaw = yawZ(road.x - placed.x, road.z - placed.z);
      placed.y = t.surfaceY(s, 0) - 0.7;
      placed.seed = Math.floor(rng() * 1e9);
      this.pads.push({ x: placed.x, z: placed.z, r: 34, falloff: 26, y: placed.y });
      this.farms.push(placed);
      prefer = -side_of(placed);
    }
    function side_of(p) { return p.side; }
  }

  planStore() {
    const t = this.t;
    const g = t.tag('fields')[0];
    if (!g) return;
    const sx = Math.round((g.s0 + g.s1) / 2);
    for (const side of [1, -1]) {
      const lat = t.wallR[sx] + 17;
      const c = t.pointAt(sx + 18, side * lat);
      if (!this.padOK(c.x, c.z, 20)) continue;
      const road = t.pointAt(sx + 18, 0);
      this.store = { s: sx, side, x: c.x, z: c.z, yaw: yawZ(road.x - c.x, road.z - c.z), y: t.surfaceY(sx, 0) - 0.35 };
      this.pads.push({ x: c.x, z: c.z, r: 20, falloff: 16, y: this.store.y });
      break;
    }
    // The side road crosses the main road here, running out both ways.
    this.sideRoads = [];
    if (!this.store) return;
    const f = t.frame(sx);
    for (const sgn of [1, -1]) {
      const pts = [];
      for (let d = t.wallR[sx] + 0.7; d < 420; d += 6) {
        const x = f.x + f.rx * d * sgn, z = f.z + f.rz * d * sgn;
        if (d > 40 && !this.clearOfRoad(x, z, 30, sx, 40)) break;
        if (this.creekDist(x, z) < 14 || !this.inValley(x, 0.8)) break;
        pts.push({ x, z });
      }
      if (pts.length > 3) this.sideRoads.push(pts);
    }
  }

  planMill() {
    const pts = this.creek, c = this.creekCross;
    for (const dir of [1, -1]) {
      const i = c + dir * 8;
      if (i < 1 || i >= pts.length - 1) continue;
      const a = pts[i - 1], b = pts[i + 1];
      const dx = b.x - a.x, dz = b.z - a.z, l = Math.hypot(dx, dz);
      const nx = -dz / l, nz = dx / l;
      for (const side of [1, -1]) {
        const x = pts[i].x + nx * side * 13.5, z = pts[i].z + nz * side * 13.5;
        if (!this.clearOfRoad(x, z, 30)) continue;
        let bad = false;
        for (const p of this.pads) if (Math.hypot(x - p.x, z - p.z) < p.r + 24) bad = true;
        if (bad) continue;
        this.mill = { x, z, i, nx: nx * side, nz: nz * side, y: this.t.surfaceY(this.creekS, 0) - 1.2 };
        this.pads.push({ x, z, r: 7, falloff: 7, y: this.mill.y });
        return;
      }
    }
  }

  // The valley's windmill: on a rise near the road, where it stands out
  // against the sky as a landmark on the way down from the pass.
  planWindmill() {
    const t = this.t;
    const crest = t.tag('crest')[0];
    const cands = [];
    if (crest) for (let s = crest.s0; s <= crest.s1 + 120; s += 30) cands.push(s);
    for (let s = this.v0 + 350; s < this.v1 - 350; s += 40) cands.push(s);
    for (const s of cands) {
      for (const side of [-1, 1]) {
        for (const lat of [40, 48, 58]) {
          const c = t.pointAt(s, side * lat);
          if (!this.clearOfRoad(c.x, c.z, lat - 12, s, 80)) continue;
          if (!this.padOK(c.x, c.z, 10)) continue;
          const road = t.pointAt(s, 0);
          this.windmill = { s, side, x: c.x, z: c.z, y: t.surfaceY(s, 0) - 0.4, yaw: yawZ(road.x - c.x, road.z - c.z) };
          this.pads.push({ x: c.x, z: c.z, r: 9, falloff: 12, y: this.windmill.y });
          return;
        }
      }
    }
  }

  // ── Build ───────────────────────────────────────────────────────
  async build(world) {
    const t0 = performance.now();
    this.world = world;
    this.ground = makeGround(this.T);
    this.group = new THREE.Group();
    this.group.name = 'valley';
    this.makeMaterials();
    this.B = new PaintBuilder(this.paints);
    this.trees = [];
    this.bales = [];
    this.cows = [];
    this.wheels = [];
    this.avoid = []; // {x,z,r}: keep trees off buildings and lanes

    // Per-step timings end up in this.stats.steps (load-time budget).
    const steps = {};
    let tStep = performance.now();
    const lap = (name) => { const t = performance.now(); steps[name] = Math.round(t - tStep); tStep = t; };
    for (const f of this.farms) this.buildFarm(f);
    this.buildStore();
    this.buildCreek();
    this.buildMill();
    this.buildWindmill();
    this.buildPoles();
    this.buildEntrySign();
    lap('buildings');
    await new Promise((r) => setTimeout(r, 0));
    tStep = performance.now();
    this.scatterHedgerows(); lap('hedges');
    this.scatterCreekTrees();
    this.scatterHillTrees();
    this.scatterBales(); lap('trees');
    this.scatterCrops(); lap('crops');
    this.scatterVerge(); lap('verge');

    const M = this.M;
    for (const mesh of this.B.build(M, {
      castShadow: ['p:siding', 'p:boards', 'p:roof', 'p:metal', 'p:paint'],
    })) this.group.add(mesh);
    lap('merge');
    this.buildTrees();
    this.buildBalesMesh();
    this.buildCows();
    this.buildWheels();
    this.buildCropsMesh();
    this.buildVergeMesh();
    lap('meshes');
    world.scene.add(this.group);

    world.addNight(M.window, 'emissiveIntensity', 0.0, 2.4);
    world.addNight(M.lamp, 'emissiveIntensity', 0.3, 7.0);
    world.addNight(M.pumpGlobe, 'emissiveIntensity', 0.4, 4.0);
    world.addNight(M.neon, 'emissiveIntensity', 1.2, 5.0);
    world.addNight(M.signValley, 'emissiveIntensity', 0.0, 0.25);
    world.addNight(M.signStore, 'emissiveIntensity', 0.0, 0.35);

    const horizon = world.sky?.uniforms?.uHorizon?.value;
    world.updaters.push((dt, night) => {
      this.updateWheels(dt);
      if (this.waterMat) {
        this.waterMat.normalMap.offset.x += dt * 0.02;
        this.waterMat.normalMap.offset.y -= dt * 0.035;
        if (horizon) this.waterMat.emissive.copy(horizon).multiplyScalar(0.05).add(this._waterTint || (this._waterTint = new THREE.Color(0x06222c)));
      }
    });
    this.stats = {
      farms: this.farms.length, trees: this.trees.length, bales: this.bales.length, cows: this.cows.length,
      ms: Math.round(performance.now() - t0), planMs: this.planMs, meshes: this.group.children.length, steps,
    };
    world.valley = this;
  }

  makeMaterials() {
    const S = (color, o = {}) => new THREE.MeshStandardMaterial({ color, roughness: 0.9, metalness: 0, ...o });
    // Plain paints are baked into vertex colours and share a few
    // material-class buckets (see PaintBuilder): [bucket, colour].
    this.paints = {
      wallCream: ['p:siding', 0xd6caa9], wallWhite: ['p:siding', 0xe2ddd0], wallYellow: ['p:siding', 0xcdb27a], wallBlue: ['p:siding', 0x8e9ea3],
      wallGreen: ['p:siding', 0x9aa88a],
      woodGray: ['p:boards', 0x857a6b], woodDark: ['p:boards', 0x4d3c2d], barnRed: ['p:boards', 0x8e3327], barnRed2: ['p:boards', 0x6d3a2c], barnDoor: ['p:boards', 0x5a2921],
      barnWhite: ['p:boards', 0xd8d2c4],
      roofShingle: ['p:roof', 0x3e3a38], roofRust: ['p:roof', 0x7c4a2f], roofSlate: ['p:roof', 0x4a4e56], roofGreen: ['p:roof', 0x3f5a48],
      roofMetal: ['p:metal', 0x8b8f91], siloMetal: ['p:metal', 0xb6babd], siloDome: ['p:metal', 0x9ea4a8], siloBand: ['p:metal', 0x6c6f72],
      steelOld: ['p:metal', 0x6c6158], mailbox: ['p:metal', 0x3c4146], pumpRed: ['p:metal', 0xb2271d],
      trim: ['p:paint', 0xefe9dc], stone: ['p:paint', 0x77716a], stoneLight: ['p:paint', 0x9a9387], brick: ['p:paint', 0x7b4636],
      black: ['p:paint', 0x141414], hay: ['p:paint', 0xd2ae62], concrete: ['p:paint', 0xb7b0a2], vane: ['p:paint', 0x8c3024],
      shutterGreen: ['p:paint', 0x2f4a36], shutterBlack: ['p:paint', 0x222426], shutterRed: ['p:paint', 0x6e2a22], shutterBlue: ['p:paint', 0x34506a],
      sail: ['p:paint', 0xd9d0bc],
    };
    const V = (o = {}) => S(0xffffff, { vertexColors: true, ...o });
    const M = {
      'p:siding': surfaceDetail(V({ roughness: 0.85 }), 'siding'),
      'p:boards': surfaceDetail(V({ roughness: 0.92 }), 'boards'),
      'p:roof': surfaceDetail(V({ roughness: 0.85, metalness: 0.12 }), 'roof'),
      'p:metal': V({ metalness: 0.5, roughness: 0.48, side: THREE.DoubleSide }),
      'p:paint': V({ side: THREE.DoubleSide }),
      window: S(0x2b2721, { emissive: 0xffbb66, emissiveIntensity: 0, roughness: 0.3 }),
      windowDark: S(0x1c2126, { roughness: 0.25, metalness: 0.4 }),
      lamp: S(0xfff1d0, { emissive: 0xffd49a, emissiveIntensity: 0.3 }),
      pumpGlobe: S(0xffffff, { emissive: 0xfff0d8, emissiveIntensity: 0.4 }),
      dirt: S(0x94806a, { roughness: 1, polygonOffset: true, polygonOffsetFactor: -1, polygonOffsetUnits: -2 }),
      gravelRoad: S(0x9b9384, { roughness: 1, polygonOffset: true, polygonOffsetFactor: -1, polygonOffsetUnits: -2 }),
      hay: S(0xd2ae62, { roughness: 1 }),
    };
    const vs = woodSign(['OLD MILL VALLEY', 'Pop. 312  ·  Est. 1887']);
    M.signValley = S(0xffffff, { map: vs, emissive: 0xffffff, emissiveMap: vs, emissiveIntensity: 0 });
    const st = woodSign(['GENERAL STORE', 'MILL VALLEY  ·  FEED · SEED · GAS'], { w: 768, h: 160, bg: '#7a2e22', font: 'bold 74px Georgia, serif', sub: 'bold 30px Georgia, serif' });
    M.signStore = S(0xffffff, { map: st, emissive: 0xffffff, emissiveMap: st, emissiveIntensity: 0 });
    const neon = canvasTexture(256, 96, (g, w, h) => {
      g.fillStyle = '#000'; g.fillRect(0, 0, w, h);
      g.font = 'bold 70px "Arial Black", Arial, sans-serif'; g.textAlign = 'center'; g.textBaseline = 'middle';
      g.shadowColor = '#ff3040'; g.shadowBlur = 18; g.strokeStyle = '#ff5060'; g.lineWidth = 6; g.strokeText('OPEN', w / 2, h / 2);
      g.shadowBlur = 0; g.lineWidth = 2; g.strokeStyle = '#ffd0d0'; g.strokeText('OPEN', w / 2, h / 2);
    });
    M.neon = new THREE.MeshStandardMaterial({ color: 0x000000, emissive: 0xffffff, emissiveMap: neon, emissiveIntensity: 1.2, transparent: true, alphaMap: neon, depthWrite: false });
    this.M = M;
  }

  gy(x, z) { return this.ground.height(x, z); }

  // ── Farmsteads ─────────────────────────────────────────────────
  buildFarm(f) {
    const B = this.B, t = this.t;
    const rng = mulberry32(f.seed);
    const y = f.y;
    const toWorld = (lx, lz) => {
      const c = Math.cos(f.yaw), s = Math.sin(f.yaw);
      return [f.x + lx * c + lz * s, f.z - lx * s + lz * c];
    };
    B.setFrame(f.x, y, f.z, f.yaw);

    // Farmhouse, left-front of the yard.
    const hx = -11 + rrange(rng, -2, 2), hz = 7;
    B.pushFrame(hx, 0, hz, rrange(rng, -0.08, 0.08));
    const house = farmhouse(B, rng);
    B.popFrame();
    this.avoid.push({ p: toWorld(hx, hz), r: 12 });

    // Barn, right-back; sometimes turned side-on.
    const bx = 13 + rrange(rng, -2, 2), bz = -7;
    const turned = rng() < 0.4;
    B.pushFrame(bx, 0, bz, turned ? -Math.PI / 2 : rrange(rng, -0.1, 0.1));
    const b = barn(B, rng);
    B.popFrame();
    this.avoid.push({ p: toWorld(bx, bz), r: 15 });

    // Silos beside the barn.
    const nSilo = rng() < 0.25 ? 0 : rng() < 0.6 ? 1 : 2;
    for (let k = 0; k < nSilo; k++) {
      const sx = bx + (turned ? 6 + k * 6.5 : b.w / 2 + 4 + k * 6.2), sz = bz + (turned ? -b.w / 2 - 4 : -4);
      silo(B, rng, sx, sz, { metal: k === 0 ? rng() < 0.5 : true });
      this.avoid.push({ p: toWorld(sx, sz), r: 4 });
    }
    // Shed and a stack of square bales.
    B.pushFrame(-18, 0, -13, rrange(rng, -0.3, 0.3));
    shed(B, rng);
    B.popFrame();
    for (let r = 0; r < 3; r++) for (let c = 0; c < 4 - r; c++) {
      B.box('hay', 1.2, 0.5, 0.9, 1 + c * 1.25 + r * 0.6, r * 0.5, -18.5, 0);
    }
    // Windpump near the house.
    if (rng() < 0.8) {
      const wx = -26, wz = 15;
      B.pushFrame(wx, 0, wz, 0);
      // Tower in the frame; wheel faces the prevailing wind (+Z world-ish).
      const hub = windpumpTower(B, 10 + rng() * 2);
      B.popFrame();
      const [ex, ez] = toWorld(wx + hub.x, wz + hub.z);
      this.wheels.push({ x: ex, y: y + hub.y, z: ez, yaw: f.yaw, speed: 1.6 + rng() * 1.4, a: rng() * 6 });
      this.avoid.push({ p: toWorld(wx, wz), r: 4 });
    }
    // Yard light pole.
    B.box('woodDark', 0.18, 6.5, 0.18, 2, 0, 12);
    B.cbox('lamp', 0.5, 0.25, 0.35, 2, 6.4, 12.35);
    // Paddock with cows behind the barn.
    const px = 8, pz = -27, pw = 30, pd = 16;
    B.pushFrame(px, 0, pz, 0);
    paddock(B, pw, pd, 3.5);
    B.popFrame();
    const nCows = 3 + Math.floor(rng() * 5);
    for (let k = 0; k < nCows; k++) {
      const [cx, cz] = toWorld(px + rrange(rng, -pw / 2 + 2, pw / 2 - 2), pz + rrange(rng, -pd / 2 + 2, pd / 2 - 2));
      this.cows.push({ x: cx, z: cz, yaw: rng() * Math.PI * 2, c: rng() });
    }
    this.avoid.push({ p: toWorld(px, pz), r: 17 });

    // Driveway from the road edge to the porch.
    const hw = t.frame(f.s);
    const wall = f.side > 0 ? hw.wallR : hw.wallL;
    const a = t.pointAt(f.s, f.side * (wall + 0.7));
    const a2 = t.pointAt(f.s, f.side * (wall + 12));
    const [dx, dz] = toWorld(hx, hz + house.d / 2 + 3.2);
    const [mx, mz] = toWorld(hx + 4, hz + house.d / 2 + 14);
    const path = [];
    // Quadratic from road edge, leaving straight out from the road.
    const P0 = [a.x, a.z], P1 = [a2.x, a2.z], P2 = [mx, mz], P3 = [dx, dz];
    const L = Math.hypot(P3[0] - P0[0], P3[1] - P0[1]);
    const n = Math.max(6, Math.round(L / 3));
    for (let i = 0; i <= n; i++) {
      const u = i / n, v = 1 - u;
      path.push({
        x: v * v * v * P0[0] + 3 * v * v * u * P1[0] + 3 * v * u * u * P2[0] + u * u * u * P3[0],
        z: v * v * v * P0[1] + 3 * v * v * u * P1[1] + 3 * v * u * u * P2[1] + u * u * u * P3[1],
      });
    }
    this.ribbon(path, 2.8, 'dirt');
    this.avoidPath(path, 5);
    f.drive = path;
    // Mailbox beside the driveway mouth, just outside the fence line.
    const mb = t.pointAt(f.s + 3.2, f.side * (wall + 1.6));
    B.setFrame(mb.x, this.gy(mb.x, mb.z), mb.z, yawZ(-hw.rx * f.side, -hw.rz * f.side));
    mailbox(B, 0, 0, 0);

    // Big shade trees around the house.
    const nt = 2 + Math.floor(rng() * 3);
    for (let k = 0; k < nt; k++) {
      const [tx, tz] = toWorld(hx + rrange(rng, -16, 8), hz + rrange(rng, 10, 20) * (rng() < 0.5 ? 1 : -0.9));
      this.addTree(tx, tz, 'shade', rng);
    }
    // Orchard behind the house.
    if (rng() < 0.75) {
      const ox = -36 + rrange(rng, -4, 4), oz = -30;
      const cols = 6 + Math.floor(rng() * 3), rows = 6 + Math.floor(rng() * 4);
      for (let i = 0; i < cols; i++) for (let j = 0; j < rows; j++) {
        const [tx, tz] = toWorld(ox - i * 6.5, oz - j * 6.5 + 20);
        this.addTree(tx, tz, 'orchard', rng, 1.5);
      }
    }
    // Windbreak row of poplars along one side.
    const wbSide = rng() < 0.5 ? -1 : 1;
    for (let k = 0; k < 14; k++) {
      const [tx, tz] = toWorld(wbSide * 44, 20 - k * 5.2);
      this.addTree(tx, tz, 'poplar', rng, 1.5);
    }
  }

  avoidPath(path, r) {
    for (let i = 0; i < path.length; i += 2) this.avoid.push({ p: [path[i].x, path[i].z], r });
  }

  // Flat ribbon draped on the rendered ground.
  ribbon(path, width, matKey, lift = 0.09) {
    const pos = [], uv = [], idx = [];
    let along = 0;
    for (let i = 0; i < path.length; i++) {
      const p = path[i];
      const a = path[Math.max(0, i - 1)], b = path[Math.min(path.length - 1, i + 1)];
      let dx = b.x - a.x, dz = b.z - a.z;
      const l = Math.hypot(dx, dz) || 1;
      dx /= l; dz /= l;
      if (i > 0) along += Math.hypot(p.x - path[i - 1].x, p.z - path[i - 1].z);
      for (const sgn of [-1, 1]) {
        const x = p.x - dz * sgn * width / 2, z = p.z + dx * sgn * width / 2;
        pos.push(x, this.gy(x, z) + lift, z);
        uv.push(sgn * 0.5 + 0.5, along / 4);
      }
      if (i > 0) {
        const k = i * 2;
        idx.push(k - 2, k, k - 1, k - 1, k, k + 1);
      }
    }
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
    g.setAttribute('uv', new THREE.Float32BufferAttribute(uv, 2));
    g.setIndex(idx);
    g.computeVertexNormals();
    // Make sure normals point up whatever the winding.
    const nrm = g.getAttribute('normal');
    for (let i = 0; i < nrm.count; i++) if (nrm.getY(i) < 0) nrm.setXYZ(i, -nrm.getX(i), -nrm.getY(i), -nrm.getZ(i));
    const m = new THREE.Mesh(g, this.M[matKey]);
    m.material.side = THREE.DoubleSide;
    m.receiveShadow = true;
    m.matrixAutoUpdate = false;
    this.group.add(m);
    return m;
  }

  // ── Crossroads store ───────────────────────────────────────────
  buildStore() {
    const st = this.store;
    if (!st) return;
    const B = this.B, t = this.t;
    for (const path of this.sideRoads) {
      this.ribbon(path, 5.5, 'gravelRoad', 0.1);
      this.avoidPath(path, 7);
    }
    const rng = mulberry32(99);
    B.setFrame(st.x, st.y, st.z, st.yaw);
    const s = generalStore(B, rng);
    // Sign board on the false front.
    B.put('signStore', new THREE.PlaneGeometry(12, 2.5), s.sign.x, s.sign.y, s.sign.z + 0.02);
    B.put('neon', new THREE.PlaneGeometry(1.6, 0.6), s.neon.x, s.neon.y - 0.95, s.neon.z + 0.02);
    // Pumps on a gravel apron between the store and the road.
    for (const px of [-2.2, 2.2]) gasPump(B, px, 12.5);
    B.box('roofRust', 8, 0.2, 4.5, 0, 4.6, 12.5);
    for (const px of [-3.6, 3.6]) B.box('woodDark', 0.2, 4.6, 0.2, px, 0, 12.5);
    this.avoid.push({ p: [st.x, st.z], r: 22 });
    // Stop signs at the side-road mouths.
    const f = t.frame(st.s);
    for (const sgn of [1, -1]) {
      const wall = sgn > 0 ? f.wallR : f.wallL;
      const p = t.pointAt(st.s - 4 * sgn, sgn * (wall + 1.8));
      const face = yawZ(f.rx * sgn, f.rz * sgn);
      B.setFrame(p.x, this.gy(p.x, p.z), p.z, face);
      B.box('steelOld', 0.07, 2.2, 0.07, 0, 0, 0);
      B.put('pumpRed', new THREE.CylinderGeometry(0.38, 0.38, 0.04, 8), 0, 2.4, 0.05, Math.PI / 2, 0, Math.PI / 8);
    }
  }

  // ── Mill and creek ─────────────────────────────────────────────
  buildMill() {
    const m = this.mill;
    if (!m) return;
    const B = this.B;
    // Building's +X (wheel side) faces the creek.
    const yaw = yawX(-m.nx, -m.nz);
    B.setFrame(m.x, m.y, m.z, yaw);
    const h = millHouse(B, mulberry32(7));
    const c = Math.cos(yaw), s = Math.sin(yaw);
    const wx = m.x + h.wheel.x * c + h.wheel.z * s, wz = m.z - h.wheel.x * s + h.wheel.z * c;
    const water = this.waterLevelAt(wx, wz);
    // Paddles dip ~0.7 m into the stream.
    this.millWheel = { x: wx, y: water + 3.2 - 0.7, z: wz, yaw };
    this.avoid.push({ p: [m.x, m.z], r: 13 });
    // A sign by the road pointing at it.
    const t = this.t, sc = this.creekS;
    const f = t.frame(sc - 30);
    const side = (m.x - f.x) * f.rx + (m.z - f.z) * f.rz > 0 ? 1 : -1;
    const wall = side > 0 ? f.wallR : f.wallL;
    const p = t.pointAt(sc - 30, side * (wall + 2.2));
    const tex = woodSign(['THE OLD MILL', 'Grist since 1887'], { w: 512, h: 192 });
    this.M.signMill = new THREE.MeshStandardMaterial({ map: tex, roughness: 0.9 });
    B.setFrame(p.x, this.gy(p.x, p.z), p.z, yawZ(-f.fx, -f.fz));
    boardSign(B, 'signMill', 2.4, 0.9, 0, 0, 0, 0.9);
  }

  buildWindmill() {
    const w = this.windmill;
    if (!w) return;
    const B = this.B;
    B.setFrame(w.x, w.y, w.z, w.yaw);
    const hub = towerMill(B, mulberry32(31));
    B.box('stone', 1.8, 0.3, 1.2, 0, -0.1, 4.1);  // doorstep
    const geo = millSailsGeometry(new PaintBuilder({ wood: ['a', 0x4a3a2c], cloth: ['a', 0xd9d0bc] }));
    const m = new THREE.Mesh(geo, new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.9, side: THREE.DoubleSide }));
    const c = Math.cos(w.yaw), s = Math.sin(w.yaw);
    m.position.set(w.x + hub.x * c + hub.z * s, w.y + hub.y, w.z - hub.x * s + hub.z * c);
    m.rotation.order = 'YXZ';
    m.rotation.y = w.yaw;
    m.rotation.x = -0.08; // windshaft tilted up, as on real mills
    m.castShadow = true;
    this.sails = m;
    this.group.add(m);
    this.avoid.push({ p: [w.x, w.z], r: 13 });
  }

  waterLevelAt(x, z) {
    if (!this.water) return this.T.heightAt(x, z);
    const { i, t } = segDist(x, z, this.creek);
    return lerp(this.water[i], this.water[Math.min(this.water.length - 1, i + 1)], t);
  }

  buildCreek() {
    const pts = this.creek, T = this.T, t = this.t;
    // Bed height along the creek, ignoring where the road embankment sits.
    const bed = pts.map((p) => T.heightAt(p.x, p.z));
    const valid = pts.map((p) => { const k = t.nearest(p.x, p.z, 40); return k < 0; });
    // Fill invalid spans by interpolation.
    let last = -1;
    for (let i = 0; i < pts.length; i++) {
      if (!valid[i]) continue;
      if (last >= 0 && i - last > 1) for (let k = last + 1; k < i; k++) bed[k] = lerp(bed[last], bed[i], (k - last) / (i - last));
      if (last < 0) for (let k = 0; k < i; k++) bed[k] = bed[i];
      last = i;
    }
    if (last >= 0) for (let k = last + 1; k < pts.length; k++) bed[k] = bed[last];
    // Water sits ~1 m above the bed, but always well below the banks: near
    // the road the channel is filled by the embankment, and there the
    // water must stay hidden underneath it.
    const bank = pts.map((p, i) => {
      const a = pts[Math.max(0, i - 1)], b = pts[Math.min(pts.length - 1, i + 1)];
      let dx = b.x - a.x, dz = b.z - a.z;
      const l = Math.hypot(dx, dz) || 1;
      dx /= l; dz /= l;
      let m = Infinity;
      for (const off of [-14, -11, 11, 14]) m = Math.min(m, this.ground.height(p.x - dz * off, p.z + dx * off), T.heightAt(p.x - dz * off, p.z + dx * off));
      return m;
    });
    const water = bed.map((b, i) => Math.min(b + 1.1, bank[i] - 0.4));
    for (let pass = 0; pass < 3; pass++) {
      for (let i = 1; i < water.length - 1; i++) water[i] = Math.min(bank[i] - 0.4, (water[i - 1] + water[i] + water[i + 1]) / 3);
    }
    this.water = water;

    const pos = [], uv = [], idx = [];
    const W = 9;
    let along = 0;
    for (let i = 0; i < pts.length; i++) {
      const a = pts[Math.max(0, i - 1)], b = pts[Math.min(pts.length - 1, i + 1)];
      let dx = b.x - a.x, dz = b.z - a.z;
      const l = Math.hypot(dx, dz) || 1;
      dx /= l; dz /= l;
      if (i) along += Math.hypot(pts[i].x - pts[i - 1].x, pts[i].z - pts[i - 1].z);
      for (const sgn of [-1, 1]) {
        pos.push(pts[i].x - dz * sgn * W, water[i], pts[i].z + dx * sgn * W);
        uv.push((sgn * W) / 10, along / 10);
      }
      if (i) { const k = i * 2; idx.push(k - 2, k - 1, k, k - 1, k + 1, k); }
    }
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
    g.setAttribute('uv', new THREE.Float32BufferAttribute(uv, 2));
    g.setIndex(idx);
    g.computeVertexNormals();
    const nrm = g.getAttribute('normal');
    for (let i = 0; i < nrm.count; i++) nrm.setXYZ(i, 0, 1, 0);
    const nm = waterNormalTexture();
    nm.wrapS = nm.wrapT = THREE.RepeatWrapping;
    nm.colorSpace = THREE.NoColorSpace;
    this.waterMat = new THREE.MeshStandardMaterial({
      color: 0x1b4a5c, roughness: 0.05, metalness: 0.0, normalMap: nm, normalScale: new THREE.Vector2(0.4, 0.4),
      transparent: true, opacity: 0.86, emissive: 0x000000, side: THREE.DoubleSide, envMapIntensity: 0.8,
    });
    const mesh = new THREE.Mesh(g, this.waterMat);
    mesh.receiveShadow = true;
    mesh.matrixAutoUpdate = false;
    this.group.add(mesh);
    for (const p of pts) this.avoid.push({ p: [p.x, p.z], r: 11, creek: true });

    this.buildCrossing();
  }

  // Where the creek meets the road: stone parapets, and either a proper
  // bridge (if the terrain dips under the deck) or culvert headwalls.
  buildCrossing() {
    const t = this.t, T = this.T, B = this.B;
    const sc = this.creekS;
    const f = t.frame(sc);
    const roadY = t.surfaceY(sc, 0);
    const gap = roadY - T.heightAt(f.x, f.z);
    const span = 12;
    for (const side of [-1, 1]) {
      const wall = side > 0 ? f.wallR : f.wallL;
      // Parapet: a stone wall with a capping, just outside the fence.
      for (let s = sc - span; s < sc + span; s += 2) {
        const p = t.pointAt(s + 1, side * (wall + 1.1));
        const fr = t.frame(s + 1);
        B.setFrame(p.x, t.surfaceY(s + 1, side * (wall + 1.1)) - 0.2, p.z, yawZ(fr.fx, fr.fz));
        B.box('stoneLight', 0.5, 1.15, 2.05, 0, 0, 0);
        B.box('stone', 0.62, 0.14, 2.05, 0, 1.15, 0);
      }
      for (const e of [-1, 1]) {
        const s = sc + e * (span + 0.6);
        const p = t.pointAt(s, side * (wall + 1.1));
        const fr = t.frame(s);
        B.setFrame(p.x, t.surfaceY(s, side * (wall + 1.1)) - 0.2, p.z, yawZ(fr.fx, fr.fz));
        B.box('stone', 0.8, 1.6, 0.8, 0, 0, 0);
        B.box('stoneLight', 0.9, 0.2, 0.9, 0, 1.6, 0);
      }
    }
    // Creek direction at the crossing.
    const i = this.creekCross;
    const a = this.creek[Math.max(0, i - 1)], b = this.creek[Math.min(this.creek.length - 1, i + 1)];
    const cdx = b.x - a.x, cdz = b.z - a.z, cl = Math.hypot(cdx, cdz);
    const ux = cdx / cl, uz = cdz / cl;
    if (gap > 2) {
      // Open bridge: abutments at the banks and beams under the deck.
      for (const e of [-1, 1]) {
        const s = sc + e * (span - 1);
        const fr = t.frame(s);
        B.setFrame(fr.x, T.heightAt(fr.x, fr.z) - 1, fr.z, yawZ(fr.fx, fr.fz));
        B.box('stone', fr.hw * 2 + 4, roadY - T.heightAt(fr.x, fr.z) + 0.6, 2.2, 0, 0, 0);
      }
      for (const lat of [-4, 0, 4]) {
        const p0 = t.pointAt(sc - span, lat), p1 = t.pointAt(sc + span, lat);
        B.setFrame(0, 0, 0, 0);
        B.beam('steelOld', new THREE.Vector3(p0.x, p0.y - 0.9, p0.z), new THREE.Vector3(p1.x, p1.y - 0.9, p1.z), 0.8);
      }
    } else {
      // Culvert headwalls where the embankment meets the water.
      for (const dir of [-1, 1]) {
        // Walk out along the creek until the water surfaces above the bed;
        // the culvert mouth sits just road-side of that.
        let hx = f.x, hz = f.z, hdx = ux * dir, hdz = uz * dir, found = false;
        const pts = this.creek;
        for (let k = 1; k < 12; k++) {
          const j = i + dir * k;
          if (j < 0 || j >= pts.length) break;
          if (this.water[j] - T.heightAt(pts[j].x, pts[j].z) > 0.25) {
            const pj = pts[j], pp = pts[j - dir];
            const back = 3 / Math.hypot(pj.x - pp.x, pj.z - pp.z);
            hx = pj.x + (pp.x - pj.x) * back; hz = pj.z + (pp.z - pj.z) * back;
            const l = Math.hypot(pj.x - pp.x, pj.z - pp.z);
            hdx = (pj.x - pp.x) / l; hdz = (pj.z - pp.z) / l;
            found = true;
            break;
          }
        }
        if (!found) continue;
        const wl = this.waterLevelAt(hx, hz);
        const base = wl - 1.4;
        const top = Math.max(wl + 2.4, T.heightAt(hx, hz) + 0.6);
        B.setFrame(hx, base, hz, yawZ(hdx, hdz));
        B.box('stoneLight', 12, top - base, 0.9, 0, 0, 0);
        B.box('stone', 12.4, 0.25, 1.1, 0, top - base, 0);
        // Arch opening: dark half-disc on the outer face.
        B.put('black', new THREE.CircleGeometry(1.9, 16, 0, Math.PI), 0, wl - base - 0.3, 0.47);
        B.put('stone', new THREE.TorusGeometry(2.0, 0.22, 4, 16, Math.PI), 0, wl - base - 0.3, 0.5);
      }
    }
  }

  // ── Telephone poles along the verge ─────────────────────────────
  buildPoles() {
    const t = this.t;
    const skip = [];
    for (const f of this.farms) skip.push([f.s - 8, f.s + 12]);
    if (this.store) skip.push([this.store.s - 10, this.store.s + 10]);
    skip.push([this.creekS - 22, this.creekS + 22]);
    const side = 1;
    const poles = [];
    let s = this.v0 + 60;
    while (s < this.v1 - 40) {
      const k = Math.abs(t.kSmooth[t.idx(s)]);
      const spacing = k > 0.004 ? 30 : 42;
      if (!skip.some(([a, b]) => s > a && s < b)) {
        const f = t.frame(s);
        const lat = side * (f.wallR + 4.2);
        const p = t.pointAt(s, lat);
        if (this.inValley(p.x, 0.5) && this.clearOfRoad(p.x, p.z, 3.5)) {
          poles.push({ x: p.x, y: this.gy(p.x, p.z) - 0.3, z: p.z, rx: f.rx, rz: f.rz, s });
        }
      }
      s += spacing;
    }
    if (!poles.length) return;
    const geo = poleGeometry(new Builder());
    const mat = new THREE.MeshStandardMaterial({ color: 0x5a4a3b, roughness: 0.95 });
    const im = new THREE.InstancedMesh(geo, mat, poles.length);
    const m4 = new THREE.Matrix4(), q = new THREE.Quaternion(), e = new THREE.Euler();
    poles.forEach((p, k) => {
      p.yaw = yawX(p.rx, p.rz) + (hash2(k, 3) - 0.5) * 0.06;
      e.set((hash2(k, 5) - 0.5) * 0.04, p.yaw, (hash2(k, 7) - 0.5) * 0.04);
      q.setFromEuler(e);
      m4.compose(new THREE.Vector3(p.x, p.y, p.z), q, new THREE.Vector3(1, 1, 1));
      im.setMatrixAt(k, m4);
    });
    im.castShadow = true;
    im.receiveShadow = true;
    im.computeBoundingSphere();
    this.group.add(im);
    // Wires with a catenary sag between neighbouring poles.
    const wpos = [];
    for (let k = 0; k < poles.length - 1; k++) {
      const a = poles[k], b = poles[k + 1];
      const span = Math.hypot(b.x - a.x, b.z - a.z);
      if (span > 80) continue;
      const sag = 0.45 + span * 0.01;
      for (const off of [-0.95, 0, 0.95]) {
        const ax = a.x + Math.cos(a.yaw) * off, az = a.z - Math.sin(a.yaw) * off, ay = a.y + 8.85;
        const bx = b.x + Math.cos(b.yaw) * off, bz = b.z - Math.sin(b.yaw) * off, by = b.y + 8.85;
        const N = 10;
        for (let i = 0; i < N; i++) {
          for (const u of [i / N, (i + 1) / N]) wpos.push(lerp(ax, bx, u), lerp(ay, by, u) - sag * 4 * u * (1 - u), lerp(az, bz, u));
        }
      }
    }
    const wg = new THREE.BufferGeometry();
    wg.setAttribute('position', new THREE.Float32BufferAttribute(wpos, 3));
    const wires = new THREE.LineSegments(wg, new THREE.LineBasicMaterial({ color: 0x1a1816 }));
    wires.matrixAutoUpdate = false;
    this.group.add(wires);
    this.poles = poles;
  }

  buildEntrySign() {
    const t = this.t, B = this.B;
    const s = this.v0 + 70;
    const f = t.frame(s);
    const p = t.pointAt(s, f.wallR + 3.2);
    B.setFrame(p.x, this.gy(p.x, p.z), p.z, yawZ(-f.fx, -f.fz) + 0.25);
    boardSign(B, 'signValley', 3.6, 1.35, 0, 0, 0, 1.0);
  }

  // ── Trees ──────────────────────────────────────────────────────
  treeOK(x, z, r = 2) {
    if (!this.clearOfRoad(x, z, r + 3)) return false;
    for (const a of this.avoid) {
      const dx = x - a.p[0], dz = z - a.p[1];
      if (dx * dx + dz * dz < (a.r + r) * (a.r + r)) return false;
    }
    return true;
  }

  addTree(x, z, kind, rng, r0 = 0) {
    if (!this.treeOK(x, z, r0 || 2.5)) return false;
    if (!this.inValley(x, 0.55)) return false;
    const y = this.gy(x, z);
    const tr = { x, y, z, kind };
    const h = hash2(Math.floor(x * 3), Math.floor(z * 3), 9);
    if (kind === 'poplar') { tr.r = rrange(rng, 1.3, 1.8); tr.sy = rrange(rng, 3.2, 4.3); tr.th = 1.4; tr.col = [0.19, 0.3, 0.12]; }
    else if (kind === 'orchard') { tr.r = rrange(rng, 1.6, 2.2); tr.sy = 0.85; tr.th = 1.1; tr.col = [0.3, 0.42, 0.16]; }
    else if (kind === 'willow') { tr.r = rrange(rng, 3.5, 5.2); tr.sy = 0.8; tr.th = 1.8; tr.col = [0.33, 0.42, 0.18]; }
    else if (kind === 'hill') { tr.r = rrange(rng, 3.0, 5.0); tr.sy = rrange(rng, 1.0, 1.5); tr.th = 1.5; tr.col = [0.16, 0.24, 0.1]; }
    else { tr.r = rrange(rng, 2.8, 4.8); tr.sy = rrange(rng, 0.9, 1.2); tr.th = rrange(rng, 1.8, 3.0); tr.col = [0.24, 0.35, 0.13]; }
    // Late-summer variety: some yellowing, some darker.
    const v = rng();
    const k = 0.8 + h * 0.4;
    tr.col = tr.col.map((c) => c * k);
    if (v < 0.15 && kind !== 'poplar') { tr.col[0] *= 1.5; tr.col[1] *= 1.15; }
    tr.yaw = rng() * Math.PI * 2;
    this.trees.push(tr);
    return true;
  }

  scatterHedgerows() {
    const rng = mulberry32(1717), T = this.T, t = this.t;
    const n = makeNoise2D(313);
    // Bounds of the valley road, padded.
    let minX = Infinity, maxX = -Infinity, minZ = Infinity, maxZ = -Infinity;
    for (let i = this.v0; i < this.v1; i += 10) {
      minX = Math.min(minX, t.px[i]); maxX = Math.max(maxX, t.px[i]);
      minZ = Math.min(minZ, t.pz[i]); maxZ = Math.max(maxZ, t.pz[i]);
    }
    minX -= 700; maxX += 700; minZ -= 800; maxZ += 800;
    const corners = [[minX, minZ], [maxX, minZ], [minX, maxZ], [maxX, maxZ]].map(([x, z]) => toUV(x, z));
    const u0 = Math.min(...corners.map((c) => c[0])), u1 = Math.max(...corners.map((c) => c[0]));
    const v0 = Math.min(...corners.map((c) => c[1])), v1 = Math.max(...corners.map((c) => c[1]));
    const tryAt = (u, v) => {
      const [x, z] = fromUV(u, v);
      if (x < minX || x > maxX || z < minZ || z > maxZ) return;
      if (n(x / 90, z / 90) < 0.05) return;
      if (rng() > 0.6) return;
      if (!this.inValley(x, 0.8)) return;
      const info = T.roadInfo(x, z);
      if (info.d < 22 || info.d > 820) return;
      if (T.slopeAt(x, z, 3) > 0.22) return;
      this.addTree(x + rrange(rng, -1.5, 1.5), z + rrange(rng, -1.5, 1.5), 'shade', rng);
    };
    for (let u = Math.ceil(u0 / FIELD_U) * FIELD_U; u < u1; u += FIELD_U) for (let v = v0; v < v1; v += 8.5) tryAt(u, v);
    for (let v = Math.ceil(v0 / FIELD_V) * FIELD_V; v < v1; v += FIELD_V) for (let u = u0; u < u1; u += 8.5) tryAt(u, v);
    // Continuous hedge (hawthorn, bramble) along the same boundaries as the
    // trees, so field edges read as lines of green rather than dotted trees.
    this.hedges = [];
    const hedgeAt = (u, v) => {
      const [x, z] = fromUV(u + rrange(rng, -0.8, 0.8), v + rrange(rng, -0.8, 0.8));
      if (x < minX || x > maxX || z < minZ || z > maxZ) return;
      if (n(x / 90, z / 90) < 0.0) return;
      if (!this.inValley(x, 0.8)) return;
      const info = T.roadInfo(x, z);
      if (info.d < 16 || info.d > 450) return;
      if (!this.treeOK(x, z, 1)) return;
      const k = rrange(rng, 1.1, 1.9);
      this.hedges.push({ x, z, y: this.gy(x, z), sx: k * rrange(rng, 1.2, 1.7), sy: k * rrange(rng, 0.8, 1.2), sz: k * rrange(rng, 1.2, 1.7), yaw: rng() * 6.28, c: rng() });
    };
    for (let u = Math.ceil(u0 / FIELD_U) * FIELD_U; u < u1; u += FIELD_U) for (let v = v0; v < v1; v += 3.2) hedgeAt(u, v);
    for (let v = Math.ceil(v0 / FIELD_V) * FIELD_V; v < v1; v += FIELD_V) for (let u = u0; u < u1; u += 3.2) hedgeAt(u, v);
  }

  scatterCreekTrees() {
    const rng = mulberry32(2323);
    const pts = this.creek;
    for (let i = 1; i < pts.length - 1; i++) {
      const a = pts[i - 1], b = pts[i + 1];
      const dx = b.x - a.x, dz = b.z - a.z, l = Math.hypot(dx, dz);
      for (const side of [-1, 1]) {
        if (rng() > 0.55) continue;
        const off = rrange(rng, 12.5, 18);
        const x = pts[i].x - (dz / l) * off * side + rrange(rng, -3, 3), z = pts[i].z + (dx / l) * off * side + rrange(rng, -3, 3);
        // Creek banks are in the avoid list; allow these by testing road/buildings only.
        if (!this.clearOfRoad(x, z, 6)) continue;
        if (!this.inValley(x, 0.6)) continue;
        this.trees.push(this.makeTreeRecord(x, z, rng() < 0.5 ? 'willow' : 'shade', rng));
      }
    }
  }

  makeTreeRecord(x, z, kind, rng) {
    const saveLen = this.trees.length;
    const saveAvoid = this.avoid;
    this.avoid = saveAvoid.filter((a) => !a.creek);
    this.addTree(x, z, kind, rng);
    this.avoid = saveAvoid;
    const tr = this.trees.length > saveLen ? this.trees.pop() : { x, z, y: this.gy(x, z), kind, r: 3, sy: 1, th: 2, col: [0.25, 0.35, 0.13], yaw: 0 };
    return tr;
  }

  scatterHillTrees() {
    const rng = mulberry32(5151), T = this.T, t = this.t;
    let minX = Infinity, maxX = -Infinity, minZ = Infinity, maxZ = -Infinity;
    for (let i = this.v0; i < this.v1; i += 10) {
      minX = Math.min(minX, t.px[i]); maxX = Math.max(maxX, t.px[i]);
      minZ = Math.min(minZ, t.pz[i]); maxZ = Math.max(maxZ, t.pz[i]);
    }
    let placed = 0;
    for (let k = 0; k < 9000 && placed < 520; k++) {
      const x = rrange(rng, minX - 1300, maxX + 1300), z = rrange(rng, minZ - 1300, maxZ + 1300);
      if (!this.inValley(x, 0.85)) continue;
      const info = T.roadInfo(x, z);
      if (info.d < 480 || info.d > 1500) continue;
      const mask = smoothstep(0.05, 0.3, fbm(T.noise2, x / 300 + 3, z / 300, 3)) * smoothstep(500, 900, info.d);
      if (mask < 0.55) continue;
      // Clusters: plant a few around the seed point.
      const nc = 3 + Math.floor(rng() * 4);
      for (let c = 0; c < nc; c++) {
        const cx = x + rrange(rng, -14, 14), cz = z + rrange(rng, -14, 14);
        if (T.slopeAt(cx, cz, 4) > 0.7) continue;
        if (this.addTree(cx, cz, 'hill', rng)) placed++;
      }
    }
  }

  // ── Hay bales in the hay/wheat parcels ─────────────────────────
  scatterBales() {
    const rng = mulberry32(8080), T = this.T, t = this.t;
    let minX = Infinity, maxX = -Infinity, minZ = Infinity, maxZ = -Infinity;
    for (let i = this.v0; i < this.v1; i += 10) {
      minX = Math.min(minX, t.px[i]); maxX = Math.max(maxX, t.px[i]);
      minZ = Math.min(minZ, t.pz[i]); maxZ = Math.max(maxZ, t.pz[i]);
    }
    minX -= 600; maxX += 600; minZ -= 600; maxZ += 600;
    const corners = [[minX, minZ], [maxX, minZ], [minX, maxZ], [maxX, maxZ]].map(([x, z]) => toUV(x, z));
    const fu0 = Math.floor(Math.min(...corners.map((c) => c[0])) / FIELD_U), fu1 = Math.ceil(Math.max(...corners.map((c) => c[0])) / FIELD_U);
    const fv0 = Math.floor(Math.min(...corners.map((c) => c[1])) / FIELD_V), fv1 = Math.ceil(Math.max(...corners.map((c) => c[1])) / FIELD_V);
    for (let fu = fu0; fu <= fu1; fu++) for (let fv = fv0; fv <= fv1; fv++) {
      const h = hash2(fu, fv, 3);
      const col = Math.floor(h * 8);
      if (col !== 3 && col !== 0 && col !== 6) continue;
      if (rng() < (col === 3 ? 0.2 : 0.6)) continue;
      const rows = 2 + Math.floor(rng() * 3);
      for (let r = 0; r < rows; r++) {
        const vv = (fv + 0.2 + (r / Math.max(1, rows - 1)) * 0.6) * FIELD_V;
        for (let q = 0; q < 6; q++) {
          if (rng() < 0.3) continue;
          const uu = (fu + 0.15 + q * 0.13 + rrange(rng, -0.03, 0.03)) * FIELD_U;
          const [x, z] = fromUV(uu, vv + rrange(rng, -3, 3));
          if (x < minX || x > maxX || z < minZ || z > maxZ) continue;
          if (!this.inValley(x, 0.85)) continue;
          const info = T.roadInfo(x, z);
          if (info.d < 30 || info.d > 650) continue;
          if (T.slopeAt(x, z, 2) > 0.06) continue;
          if (!this.treeOK(x, z, 1)) continue;
          this.bales.push({ x, z, y: this.gy(x, z), yaw: rng() * Math.PI, k: 0.85 + rng() * 0.25 });
        }
      }
    }
  }

  // ── Crop rows ──────────────────────────────────────────────────
  // Parcels the terrain paints as a green crop get rows of corn, and the
  // lavender parcels get low mounded rows, following the painted furrows.
  // Only parcels near the road: that's where rows read as rows.
  scatterCrops() {
    const T = this.T, t = this.t, rng = mulberry32(4545);
    this.cropRuns = [];
    // Slope on a coarse 10 m lattice is plenty to keep rows off banks, and
    // far cheaper than sampling the terrain at every plant.
    const slopes = new Map();
    const slopeAt = (x, z) => {
      const k = Math.round(x / 10) * 100003 + Math.round(z / 10);
      let v = slopes.get(k);
      if (v === undefined) { v = T.slopeAt(Math.round(x / 10) * 10, Math.round(z / 10) * 10, 3); slopes.set(k, v); }
      return v;
    };
    let minX = Infinity, maxX = -Infinity, minZ = Infinity, maxZ = -Infinity;
    for (let i = this.v0; i < this.v1; i += 10) {
      minX = Math.min(minX, t.px[i]); maxX = Math.max(maxX, t.px[i]);
      minZ = Math.min(minZ, t.pz[i]); maxZ = Math.max(maxZ, t.pz[i]);
    }
    minX -= 400; maxX += 400; minZ -= 400; maxZ += 400;
    const corners = [[minX, minZ], [maxX, minZ], [minX, maxZ], [maxX, maxZ]].map(([x, z]) => toUV(x, z));
    const fu0 = Math.floor(Math.min(...corners.map((c) => c[0])) / FIELD_U), fu1 = Math.ceil(Math.max(...corners.map((c) => c[0])) / FIELD_U);
    const fv0 = Math.floor(Math.min(...corners.map((c) => c[1])) / FIELD_V), fv1 = Math.ceil(Math.max(...corners.map((c) => c[1])) / FIELD_V);
    for (let fu = fu0; fu <= fu1; fu++) for (let fv = fv0; fv <= fv1; fv++) {
      const h = hash2(fu, fv, 3);
      const col = Math.floor(h * 8);
      const kind = col === 2 || col === 5 ? 'corn' : col === 7 ? 'lav' : null;
      if (!kind) continue;
      const [cx, cz] = fromUV((fu + 0.5) * FIELD_U, (fv + 0.5) * FIELD_V);
      if (!this.inValley(cx, 0.6) || T.roadInfo(cx, cz).d > 260) continue;
      // Obstacles near this parcel only.
      const avoid = this.avoid.filter((a) => !a.creek && Math.hypot(a.p[0] - cx, a.p[1] - cz) < 120 + a.r);
      const nearCreek = this.creekDist(cx, cz) < 120;
      const alongV = h > 0.5; // matches the painted furrow direction
      const [A, Bn] = alongV ? [FIELD_U, FIELD_V] : [FIELD_V, FIELD_U];
      const fa = alongV ? fu : fv, fb = alongV ? fv : fu;
      const gap = kind === 'corn' ? 2.0 : 2.2;
      for (let a = fa * A + 5 + 1.1; a < (fa + 1) * A - 5; a += gap) {
        let run = [];
        const flush = () => { if (run.length > 2) this.cropRuns.push({ kind, pts: run }); run = []; };
        for (let b = fb * Bn + 5; b < (fb + 1) * Bn - 5; b += kind === 'corn' ? 3 : 3.5) {
          const [x, z] = alongV ? fromUV(a, b) : fromUV(b, a);
          let ok = this.inValley(x, 0.85);
          if (ok) { const d = T.roadInfo(x, z).d; ok = d > 15 && d < 200; }
          if (ok) for (const o of avoid) if (Math.pow(x - o.p[0], 2) + Math.pow(z - o.p[1], 2) < Math.pow(o.r + 1.5, 2)) { ok = false; break; }
          if (ok && nearCreek) ok = this.creekDist(x, z) > 14;
          if (ok) ok = slopeAt(x, z) < 0.2;
          if (!ok) { flush(); continue; }
          run.push({ x, z, y: this.gy(x, z), r: rng() });
        }
        flush();
      }
    }
  }

  // Grass tussocks and wildflowers on the verge between the road and the
  // fence line, chunked along the road.
  scatterVerge() {
    const t = this.t, rng = mulberry32(6262);
    const n = makeNoise2D(99);
    this.verge = { grass: [], flowers: [] };
    const gaps = t.fenceGaps || [];
    for (let s = this.v0 - 200; s < this.v1 + 100; s += 2.2) {
      const f = t.frame(s);
      for (const side of [-1, 1]) {
        if (gaps.some((g) => g.side === side && s > g.s0 - 2 && s < g.s1 + 2)) continue;
        const wall = side > 0 ? f.wallR : f.wallL;
        for (let k = 0; k < 3; k++) {
          const lat = side * (wall + 0.9 + rng() * rng() * 7);
          const x = f.x + f.rx * lat + f.fx * (rng() - 0.5) * 2, z = f.z + f.rz * lat + f.fz * (rng() - 0.5) * 2;
          if (!this.inValley(x, 0.4) || !this.clearOfRoad(x, z, 0.4)) continue;
          const y = this.gy(x, z);
          const lush = n(x / 50, z / 50);
          const sc = rrange(rng, 0.65, 1.15);
          this.verge.grass.push({ s, x, y: y - 0.05, z, sx: sc, sy: sc * rrange(rng, 0.8, 1.4), sz: sc, yaw: rng() * 6.28, lush, b: rrange(rng, 0.8, 1.1) });
          if (lush > -0.1 && rng() < 0.3) {
            const hue = Math.floor((n(x / 18 + 7, z / 18) * 0.5 + 0.5) * 6.99);
            this.verge.flowers.push({ s, x: x + rrange(rng, -0.5, 0.5), y: y - 0.02, z: z + rrange(rng, -0.5, 0.5), sx: 1, sy: rrange(rng, 0.8, 1.3), sz: 1, yaw: rng() * 6.28, hue });
          }
        }
      }
    }
  }

  // ── Instanced meshes ───────────────────────────────────────────
  // One InstancedMesh from records {x,y,z,sx,sy,sz,yaw,col?}.
  instances(geo, mat, list, { cast = false, receive = true } = {}) {
    if (!list.length) return null;
    const im = new THREE.InstancedMesh(geo, mat, list.length);
    const m4 = new THREE.Matrix4(), q = new THREE.Quaternion(), e = new THREE.Euler(), v = new THREE.Vector3(), sc = new THREE.Vector3();
    list.forEach((r, k) => {
      e.set(r.rx || 0, r.yaw || 0, r.rz || 0); q.setFromEuler(e);
      m4.compose(v.set(r.x, r.y, r.z), q, sc.set(r.sx ?? 1, r.sy ?? 1, r.sz ?? 1));
      im.setMatrixAt(k, m4);
      if (r.col) im.setColorAt(k, r.col);
    });
    im.castShadow = cast;
    im.receiveShadow = receive;
    im.computeBoundingSphere();
    this.group.add(im);
    return im;
  }

  buildTrees() {
    const trees = this.trees;
    if (!trees.length) return;
    // A crown shape per kind: round orchard heads, columnar poplars, broad
    // lobed shade trees and drooping willows. Vertex colours bake the
    // occlusion; the instance colour is the tree's own green.
    const geos = {
      orchard: canopyGeometry('orchard', 5), poplar: canopyGeometry('poplar', 6),
      shade: canopyGeometry('shade', 7), willow: canopyGeometry('willow', 8), shadeFar: canopyGeometry('shade', 7, 1),
    };
    const canopyMat = foliageMaterial({ side: THREE.FrontSide, roughness: 0.95 });
    const trunkGeo = new THREE.CylinderGeometry(0.6, 1, 1, 6, 1, true);
    trunkGeo.translate(0, 0.5, 0);
    const trunkMat = new THREE.MeshStandardMaterial({ color: 0x4e3b2a, roughness: 1 });
    const groups = new Map();
    for (const tr of trees) {
      const kind = tr.kind === 'hill' ? 'shade' : tr.kind;
      // Shade trees are the most numerous: only those near the road cast shadows.
      const key = kind === 'shade' ? (this.T.roadInfo(tr.x, tr.z).d < 70 ? 'shade:near' : 'shade:far') : kind;
      if (!groups.has(key)) groups.set(key, []);
      const col = new THREE.Color().setRGB(tr.col[0], tr.col[1], tr.col[2]);
      groups.get(key).push({ x: tr.x, y: tr.y + tr.th + tr.r * tr.sy * 0.8, z: tr.z, sx: tr.r, sy: tr.r * tr.sy, sz: tr.r, yaw: tr.yaw, col });
    }
    for (const [key, list] of groups) {
      const kind = key === 'shade:far' ? 'shadeFar' : key.split(':')[0];
      this.instances(geos[kind], canopyMat, list, { cast: key !== 'shade:far' });
    }
    const tim = new THREE.InstancedMesh(trunkGeo, trunkMat, trees.length);
    const m4 = new THREE.Matrix4(), q = new THREE.Quaternion(), e = new THREE.Euler(), v = new THREE.Vector3(), sc = new THREE.Vector3();
    trees.forEach((tr, k) => {
      e.set(0, tr.yaw, 0); q.setFromEuler(e);
      v.set(tr.x, tr.y - 0.3, tr.z);
      const tw = 0.12 + tr.r * 0.045;
      sc.set(tw, tr.th + tr.r * tr.sy * 0.6 + 0.3, tw);
      m4.compose(v, q, sc);
      tim.setMatrixAt(k, m4);
    });
    tim.castShadow = true;
    tim.computeBoundingSphere();
    this.group.add(tim);

    // Hedges, in a few spatial cells so off-screen ones are culled.
    if (this.hedges?.length) {
      const hedgeGeo = shrubGeometry(44, 0);
      const cells = new Map();
      const tints = [[0.16, 0.22, 0.08], [0.2, 0.25, 0.09], [0.14, 0.2, 0.09], [0.24, 0.24, 0.1]];
      for (const h of this.hedges) {
        const key = Math.floor(h.x / 900) + ',' + Math.floor(h.z / 900);
        if (!cells.has(key)) cells.set(key, []);
        const tc = tints[Math.floor(h.c * tints.length)];
        cells.get(key).push({ ...h, col: new THREE.Color().setRGB(tc[0], tc[1], tc[2]) });
      }
      for (const list of cells.values()) this.instances(hedgeGeo, canopyMat, list, { cast: false });
    }
  }

  // Round bales: a lathed drum with rounded shoulders, the rolled-up
  // spiral on the ends painted as alternating rings in vertex colour.
  // Some are wrapped in white or green plastic (silage).
  buildBalesMesh() {
    if (!this.bales.length) return;
    const R = 0.78, L = 1.25, prof = [];
    for (let k = 0; k <= 4; k++) prof.push(new THREE.Vector2((k / 4) * (R - 0.12), L / 2));
    for (let k = 1; k <= 2; k++) { const a = (k / 2) * Math.PI / 2; prof.push(new THREE.Vector2(R - 0.12 + Math.sin(a) * 0.12, L / 2 - 0.12 + Math.cos(a) * 0.12)); }
    const n0 = prof.length;
    for (let k = n0 - 1; k >= 0; k--) prof.push(new THREE.Vector2(prof[k].x, -prof[k].y));
    const g = new THREE.LatheGeometry(prof, 11);
    g.deleteAttribute('uv');
    const p = g.getAttribute('position');
    const col = new Float32Array(p.count * 3);
    const rng = mulberry32(3);
    for (let i = 0; i < p.count; i++) {
      const r = Math.hypot(p.getX(i), p.getZ(i)), y = p.getY(i);
      let v;
      if (Math.abs(y) > L / 2 - 0.02 && r < R - 0.12) v = 0.72 + 0.3 * (Math.round(r / ((R - 0.12) / 4)) % 2); // spiral rings
      else v = 0.85 + rng() * 0.25;                                                                         // straw streaks
      col[i * 3] = v; col[i * 3 + 1] = v * 0.97; col[i * 3 + 2] = v * 0.9;
    }
    g.setAttribute('color', new THREE.BufferAttribute(col, 3));
    g.computeVertexNormals();
    g.rotateZ(Math.PI / 2);
    g.translate(0, R - 0.04, 0);
    const mat = new THREE.MeshStandardMaterial({ color: 0xffffff, vertexColors: true, roughness: 0.95, side: THREE.DoubleSide });
    const hay = new THREE.Color(0xd2ae62), wrapW = new THREE.Color(0xeeeeea), wrapG = new THREE.Color(0x4f6a3c);
    const list = this.bales.map((b) => {
      const w = hash2(Math.floor(b.x), Math.floor(b.z), 11);
      const col = w < 0.12 ? wrapW.clone() : w < 0.18 ? wrapG.clone() : hay.clone().multiplyScalar(b.k).multiply(new THREE.Color(1, 0.95, 0.85));
      return { x: b.x, y: b.y - 0.05, z: b.z, yaw: b.yaw, col };
    });
    this.instances(g, mat, list, { cast: true });
  }

  // Crop rows, draped on the ground; one merged mesh per 600 m cell and
  // crop so they cull with the camera. Corn is a pair of back-to-back
  // cut-out strips painted with stalks, leaves and tassels (a ragged,
  // see-through silhouette for two triangles a side); lavender is a low
  // mounded ridge, purple on top.
  buildCropsMesh() {
    if (!this.cropRuns?.length) return;
    const cells = new Map();
    const cell = (key) => {
      if (!cells.has(key)) cells.set(key, { pos: [], nrm: [], col: [], uv: [], idx: [], kind: key.split(':')[0] });
      return cells.get(key);
    };
    for (const run of this.cropRuns) {
      const key = run.kind + ':' + Math.floor(run.pts[0].x / 600) + ',' + Math.floor(run.pts[0].z / 600);
      const G = cell(key);
      const pts = run.pts;
      let along = 0;
      for (let i = 0; i < pts.length; i++) {
        const a = pts[Math.max(0, i - 1)], b = pts[Math.min(pts.length - 1, i + 1)];
        let dx = b.x - a.x, dz = b.z - a.z;
        const l = Math.hypot(dx, dz) || 1;
        dx /= l; dz /= l;
        const nx = -dz, nz = dx;
        const p = pts[i];
        if (i) along += Math.hypot(p.x - pts[i - 1].x, p.z - pts[i - 1].z);
        const v0 = G.pos.length / 3;
        if (run.kind === 'corn') {
          const h = lerp(2.0, 2.5, p.r) * (i === 0 || i === pts.length - 1 ? 0.8 : 1);
          // Two coincident sheets, wound opposite ways, each lit as if it
          // faced up-and-out so neither side goes black against the sun.
          for (const sd of [1, -1]) {
            G.pos.push(p.x, p.y - 0.15, p.z, p.x, p.y + h, p.z);
            G.nrm.push(nx * sd * 0.5, 0.85, nz * sd * 0.5, nx * sd * 0.5, 0.85, nz * sd * 0.5);
            G.col.push(0.55, 0.55, 0.5, 1, 1, 1);
            G.uv.push(along / 2.6, 0, along / 2.6, 1);
          }
          if (i > 0) {
            const u0 = v0 - 4;
            G.idx.push(u0, v0, u0 + 1, u0 + 1, v0, v0 + 1);
            G.idx.push(u0 + 2, u0 + 3, v0 + 2, u0 + 3, v0 + 3, v0 + 2);
          }
        } else {
          const w = 1.2, h = end(i, pts) ? 0.35 : lerp(0.5, 0.75, p.r);
          G.pos.push(p.x - nx * w / 2, p.y - 0.1, p.z - nz * w / 2, p.x, p.y + h, p.z, p.x + nx * w / 2, p.y - 0.1, p.z + nz * w / 2);
          G.nrm.push(-nx * 0.8, 0.6, -nz * 0.8, 0, 1, 0, nx * 0.8, 0.6, nz * 0.8);
          const top = p.r < 0.5 ? [0.2, 0.12, 0.38] : [0.26, 0.16, 0.42];
          G.col.push(0.06, 0.08, 0.05, ...top, 0.06, 0.08, 0.05);
          G.uv.push(0, 0, 0, 0, 0, 0);
          if (i > 0) {
            const u0 = v0 - 3;
            G.idx.push(u0, v0, u0 + 1, u0 + 1, v0, v0 + 1);
            G.idx.push(u0 + 1, v0 + 1, u0 + 2, u0 + 2, v0 + 1, v0 + 2);
          }
        }
      }
    }
    function end(i, pts) { return i === 0 || i === pts.length - 1; }
    const cornTex = canvasTexture(256, 128, (g, w, h) => {
      const rng = mulberry32(12);
      g.clearRect(0, 0, w, h);
      for (let k = 0; k < 9; k++) {
        const x = (k + 0.5) * (w / 9) + (rng() - 0.5) * 8;
        const top = 6 + rng() * 16, lean = (rng() - 0.5) * 8;
        const green = ['#6f8e30', '#7d9a36', '#8aa23c', '#9aa448'][Math.floor(rng() * 4)];
        g.strokeStyle = green; g.lineWidth = 3;
        g.beginPath(); g.moveTo(x, h); g.quadraticCurveTo(x, h * 0.5, x + lean, top); g.stroke();
        // Arching leaves in pairs up the stalk.
        for (let y = h - 14; y > top + 16; y -= 13 + rng() * 6) {
          for (const sd of [-1, 1]) {
            const len = 14 + rng() * 16;
            g.strokeStyle = rng() < 0.2 ? '#b0a654' : green; g.lineWidth = 3.5;
            g.beginPath(); g.moveTo(x, y); g.quadraticCurveTo(x + sd * len * 0.7, y - 12, x + sd * len, y + 4 + rng() * 6); g.stroke();
          }
        }
        // Tassel.
        g.strokeStyle = '#c8b46a'; g.lineWidth = 2;
        for (let j = -1; j <= 1; j++) { g.beginPath(); g.moveTo(x + lean, top + 4); g.lineTo(x + lean + j * 5, top - 5); g.stroke(); }
      }
    });
    cornTex.wrapS = THREE.RepeatWrapping;
    const cornMat = new THREE.MeshStandardMaterial({ map: cornTex, vertexColors: true, alphaTest: 0.45, roughness: 0.9, color: 0xffffff });
    const lavMat = foliageMaterial({ roughness: 0.95 });
    for (const G of cells.values()) {
      const g = new THREE.BufferGeometry();
      g.setAttribute('position', new THREE.Float32BufferAttribute(G.pos, 3));
      g.setAttribute('normal', new THREE.Float32BufferAttribute(G.nrm, 3));
      g.setAttribute('color', new THREE.Float32BufferAttribute(G.col, 3));
      g.setAttribute('uv', new THREE.Float32BufferAttribute(G.uv, 2));
      g.setIndex(G.idx);
      g.computeBoundingSphere();
      const m = new THREE.Mesh(g, G.kind === 'corn' ? cornMat : lavMat);
      m.receiveShadow = true;
      m.matrixAutoUpdate = false;
      this.group.add(m);
    }
  }

  buildVergeMesh() {
    const V = this.verge;
    if (!V) return;
    const mat = foliageMaterial();
    const gGeo = grassClumpGeometry(11, 15), fGeo = flowerGeometry(6, 19);
    const green = [new THREE.Color(0x9cc070), new THREE.Color(0xb8cc80), new THREE.Color(0x86ad5e)];
    const straw = [new THREE.Color(0xffffff), new THREE.Color(0xf4e2b8), new THREE.Color(0xe8d8a8)];
    const bloom = [0xff4a2a, 0xffd23a, 0xfff4e0, 0x8a6ee0, 0xff8a2a, 0x6e8ef0, 0xe86aa0].map((h) => new THREE.Color(h));
    const CH = 600;
    const chunk = (list, fn) => {
      const out = new Map();
      for (const r of list) {
        const k = Math.floor(r.s / CH);
        if (!out.has(k)) out.set(k, []);
        out.get(k).push(fn(r));
      }
      return out.values();
    };
    for (const l of chunk(V.grass, (r) => ({ ...r, col: (r.lush > -0.2 ? green : straw)[Math.floor(r.b * 10) % 3].clone().multiplyScalar(r.b) }))) this.instances(gGeo, mat, l);
    for (const l of chunk(V.flowers, (r) => ({ ...r, col: bloom[r.hue % bloom.length] }))) this.instances(fGeo, mat, l);
  }

  buildCows() {
    if (!this.cows.length) return;
    const geo = cowGeometry(new Builder());
    const im = new THREE.InstancedMesh(geo, new THREE.MeshStandardMaterial({ color: 0xffffff, roughness: 0.9 }), this.cows.length);
    const palette = [0x5b3a26, 0x1d1b1a, 0xd9d2c4, 0x8a5a36, 0x2a2624];
    const m4 = new THREE.Matrix4(), q = new THREE.Quaternion(), e = new THREE.Euler(), col = new THREE.Color();
    this.cows.forEach((c, k) => {
      e.set(0, c.yaw, 0); q.setFromEuler(e);
      m4.compose(new THREE.Vector3(c.x, this.gy(c.x, c.z), c.z), q, new THREE.Vector3(1, 1, 1));
      im.setMatrixAt(k, m4);
      im.setColorAt(k, col.setHex(palette[Math.floor(c.c * palette.length)]));
    });
    im.castShadow = true;
    im.computeBoundingSphere();
    this.group.add(im);
  }

  buildWheels() {
    if (this.wheels.length) {
      const geo = windpumpWheelGeometry(new Builder());
      const mat = new THREE.MeshStandardMaterial({ color: 0x8f8a82, metalness: 0.5, roughness: 0.55, side: THREE.DoubleSide });
      this.wheelMesh = new THREE.InstancedMesh(geo, mat, this.wheels.length);
      this.wheelMesh.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
      this.wheelMesh.castShadow = true;
      this.group.add(this.wheelMesh);
      this.updateWheels(0);
      this.wheelMesh.computeBoundingSphere();
    }
    if (this.millWheel) {
      const geo = waterwheelGeometry(new Builder());
      const mat = new THREE.MeshStandardMaterial({ color: 0x5c4634, roughness: 0.95 });
      const m = new THREE.Mesh(geo, mat);
      m.position.set(this.millWheel.x, this.millWheel.y, this.millWheel.z);
      m.rotation.order = 'YXZ';
      m.rotation.y = this.millWheel.yaw;
      m.castShadow = true;
      this.waterwheel = m;
      this.group.add(m);
    }
  }

  updateWheels(dt) {
    if (this.wheelMesh) {
      const m4 = this._m4 || (this._m4 = new THREE.Matrix4());
      const q = this._q || (this._q = new THREE.Quaternion());
      const e = this._e || (this._e = new THREE.Euler(0, 0, 0, 'YXZ'));
      const v = this._v || (this._v = new THREE.Vector3());
      const one = this._one || (this._one = new THREE.Vector3(1, 1, 1));
      this.wheels.forEach((w, k) => {
        w.a += dt * w.speed;
        e.set(0, w.yaw, w.a);
        q.setFromEuler(e);
        m4.compose(v.set(w.x, w.y, w.z), q, one);
        this.wheelMesh.setMatrixAt(k, m4);
      });
      this.wheelMesh.instanceMatrix.needsUpdate = true;
    }
    if (this.waterwheel) this.waterwheel.rotation.x -= dt * 0.6;
    if (this.sails) this.sails.rotation.z -= dt * 0.5;
  }
}
