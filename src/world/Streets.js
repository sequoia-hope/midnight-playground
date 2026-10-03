import * as THREE from 'three';
import { GeoBuilder, staticMesh, instanced, trs } from './city/geom.js';
import { adTexture, AD_COUNT, bannerTexture } from './city/cityTextures.js';
import {
  streetAtlas, patchStreetAtlas, S_TILE, SHOP_A, SHOP_B, ROWHOUSE, STUCCO, SHUTTER, LOBBY, KONBINI, ROW_GROUND, AWNING_C,
  neonAtlas, H_SIGNS, V_SIGNS, NEON_COLORS, pavementTexture, barrierTexture, trainTexture,
} from './streets/textures.js';
import { mergeGeometries } from 'three/addons/utils/BufferGeometryUtils.js';
import { facadeMaterial, F, F_TILE, floorH } from './streets/facades.js';
import { ambientPatch, neonFlicker, emitData } from './streets/props.js';
import * as Props from './streets/props.js';
import { asphaltTexture, glowTexture } from './textures.js';
import { mulberry32, lerp } from '../util/math.js';

// Downtown Streets (Level 3): the city grid the race runs through.
//
// The level supplies the grid (crossing spacing, kerb and pavement widths,
// the ground function) and the route's straight legs. Every block is a
// kerbed pavement slab with buildings round its edge; blocks on the inside
// of the route's corners have their kerbs pushed out to follow the curve,
// so the kerb is always where the car's wall is. Where the wall crosses open
// road instead — side streets, the outside of corners, the ends — the race
// has closed the street with barriers.
//
// Districts by grid column: the Neon District (shop fronts, signs, lantern
// strings, an elevated railway), Nob Hill (painted rowhouses stepped up the
// ridge) and the Financial District (towers round the finish).

const tick = () => new Promise((r) => setTimeout(r, 0));
const NEON = [[3.2, 0.25, 2.6], [0.3, 2.6, 3.2], [3.4, 0.5, 1.0], [3.2, 1.6, 0.3], [0.5, 3.2, 1.2], [1.8, 0.5, 3.4], [3.0, 3.0, 3.2]];
// Painted-lady body colours for the rowhouses (tints on a light atlas cell).
const PAINTED = [[0.55, 0.74, 0.8], [0.95, 0.78, 0.48], [0.86, 0.55, 0.58], [0.62, 0.74, 0.52], [0.97, 0.93, 0.82], [0.62, 0.6, 0.8],
  [0.9, 0.62, 0.45], [0.45, 0.56, 0.7], [0.98, 0.84, 0.88], [0.72, 0.86, 0.78], [0.96, 0.9, 0.62]];
const AWNING = [[0.55, 0.08, 0.08], [0.08, 0.32, 0.18], [0.1, 0.14, 0.34], [0.36, 0.1, 0.24], [0.5, 0.3, 0.06]];

// Builders keyed by spatial chunk so far-off parts can be frustum culled.
class Chunks {
  constructor(size, opts) { this.size = size; this.opts = opts; this.map = new Map(); }
  at(x, z) {
    const key = Math.floor(x / this.size) + ',' + Math.floor(z / this.size);
    let b = this.map.get(key);
    if (!b) this.map.set(key, (b = new GeoBuilder(this.opts)));
    return b;
  }
  emit(group, mat, opts) {
    const out = [];
    for (const b of this.map.values()) if (!b.empty) { const m = staticMesh(b.build(), mat, opts); group.add(m); out.push(m); }
    return out;
  }
}

function pointInPoly(x, z, poly) {
  let inside = false;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const [xi, zi] = poly[i], [xj, zj] = poly[j];
    if ((zi > z) !== (zj > z) && x < ((xj - xi) * (z - zi)) / (zj - zi + 1e-12) + xi) inside = !inside;
  }
  return inside;
}

export default class Streets {
  label = 'Building downtown';

  constructor(opts = {}) {
    this.level = opts.level;
  }

  // ── Plan: road ends, and no lane paint through crossings ──────
  plan(world) {
    const t = world.track, G = this.level.grid;
    this.G = G;
    this.track = t;
    t.runout = 0;
    const { PX, PZ, HW } = G;
    const nm = [];
    for (const tg of t.tags) if (tg.tag === 'corner') nm.push({ s0: tg.s0 - 2, s1: tg.s1 + 2 });
    let st = null;
    for (let s = 0; s <= t.length; s++) {
      const x = t.px[s], z = t.pz[s];
      const dx = Math.abs(x - Math.round(x / PX) * PX), dz = Math.abs(z - Math.round(z / PZ) * PZ);
      const inBox = dx < HW + 5 && dz < HW + 5;
      if (inBox && st === null) st = s;
      if (!inBox && st !== null) { nm.push({ s0: st, s1: s }); st = null; }
    }
    t.noMarks = nm;
  }

  async build(world) {
    const t = world.track, T = world.terrain, G = this.G;
    this.world = world; this.T = T;
    this.ground = G.ground;
    this.rng = mulberry32(9090);
    this.group = new THREE.Group();
    this.group.name = 'streets';
    world.scene.add(this.group);

    this.analyseRoute();
    this.materials();
    this.planBlocks();
    await tick();
    this.buildStreets();
    await tick();
    this.buildBlocks();
    await tick();
    this.buildLamps();
    this.buildSignals();
    await tick();
    this.buildBarriers();
    this.buildGantries();
    this.buildCrowds();
    await tick();
    this.buildLanterns();
    Props.kerbFurniture(this);
    Props.frontProps(this);
    Props.busShelters(this);
    await Props.parkedCars(this);
    await tick();
    this.buildElevated();
    this.buildVents();
    this.buildReflections();
    Props.buildPuddlesAndSpill(this);
    Props.buildSteam(this);
    Props.buildWires(this);
    this.emitAll();
    this.group.traverse((o) => { if ((o.isMesh || o.isPoints) && !o.userData.dynamic) { o.matrixAutoUpdate = false; o.updateMatrix(); } });
  }

  // ── The route on the grid ─────────────────────────────────────
  analyseRoute() {
    const t = this.track, { PX, PZ, HW } = this.G;
    // Grid street segments the route runs straight along, end to end
    // (Road.js paints and paves those), and crossings it uses.
    this.fullSeg = new Set();
    this.crossUse = new Map(); // "i,j" → 'turn' | 'through'
    for (const leg of this.G.legs) {
      if (leg.axis === 'z') {
        const j = leg.line, a = Math.min(leg.x0, leg.x1), b = Math.max(leg.x0, leg.x1);
        for (let i = Math.floor(a / PX) - 1; i <= Math.ceil(b / PX); i++) {
          if (a <= i * PX + HW + 6 && b >= (i + 1) * PX - HW - 6) this.fullSeg.add(`h,${i},${j}`);
        }
      } else {
        const i = leg.line, a = Math.min(leg.z0, leg.z1), b = Math.max(leg.z0, leg.z1);
        for (let j = Math.floor(a / PZ) - 1; j <= Math.ceil(b / PZ); j++) {
          if (a <= j * PZ + HW + 6 && b >= (j + 1) * PZ - HW - 6) this.fullSeg.add(`v,${i},${j}`);
        }
      }
    }
    for (let s = 0; s <= t.length; s += 2) {
      const i = Math.round(t.px[s] / PX), j = Math.round(t.pz[s] / PZ);
      if (Math.abs(t.px[s] - i * PX) < HW && Math.abs(t.pz[s] - j * PZ) < HW) this.crossUse.set(`${i},${j}`, 'through');
    }
    for (const tg of t.tags) {
      if (tg.tag !== 'corner') continue;
      const m = Math.round((tg.s0 + tg.s1) / 2);
      this.crossUse.set(`${Math.round(t.px[m] / PX)},${Math.round(t.pz[m] / PZ)}`, 'turn');
    }
    // Route legs as axis-aligned segments for distance queries.
    this.legSegs = this.G.legs.map((l) => ({ x0: Math.min(l.x0, l.x1), x1: Math.max(l.x0, l.x1), z0: Math.min(l.z0, l.z1), z1: Math.max(l.z0, l.z1) }));
    const b = t.bounds;
    this.gi0 = Math.floor(b.minX / PX) - 12; this.gi1 = Math.ceil(b.maxX / PX) + 12;
    this.gj0 = Math.floor(b.minZ / PZ) - 13; this.gj1 = Math.ceil(b.maxZ / PZ) + 13;
    // The finish-area core for the tallest towers.
    const f = t.frame(t.finishS - 200);
    this.core = { x: f.x + 60, z: f.z - 150 };
  }

  // Height of whatever is underfoot: the race road's surface, else the ground.
  surfaceAt(x, z) {
    const r = this.track.distanceToRoad(x, z, 30);
    if (r.i >= 0 && r.d < this.G.HW + 0.5) return this.track.surfaceY(r.s, r.lat);
    return this.ground(x, z);
  }

  // Steam from manholes in the road and grates by the kerb.
  buildVents() {
    const t = this.track, { HW } = this.G;
    const f = {};
    const P = (x, z) => this.bPlain.at(x, z);
    for (let s = 60; s < t.length - 60; s += 90 + this.rng() * 140) {
      t.frame(s, f);
      if (f.zone === 1 && this.rng() < 0.7) continue;
      const lat = (this.rng() - 0.5) * (HW * 1.4);
      const x = f.x + f.rx * lat, z = f.z + f.rz * lat, y = t.surfaceY(s, lat);
      // Manhole cover: a dark disc flush with the road.
      Props.addGeo(P(x, z), new THREE.CylinderGeometry(0.4, 0.4, 0.03, 12).toNonIndexed(), new THREE.Matrix4().setPosition(x, y + 0.02, z), [0.12, 0.11, 0.1]);
      this.steam.push({ x, y: y + 0.1, z });
    }
  }

  routeDist(x, z) {
    let best = Infinity;
    for (const l of this.legSegs) {
      const dx = Math.max(l.x0 - x, 0, x - l.x1), dz = Math.max(l.z0 - z, 0, z - l.z1);
      best = Math.min(best, Math.hypot(dx, dz));
    }
    return best;
  }

  // ── Materials ─────────────────────────────────────────────────
  materials() {
    const w = this.world;
    // Upper floors: the Streets façade atlas (per-window lights, street bounce).
    this.facMat = facadeMaterial();
    const S = streetAtlas();
    this.streetMat = patchStreetAtlas(new THREE.MeshStandardMaterial({
      map: S.map, emissiveMap: S.emissive, emissive: 0xffffff, emissiveIntensity: 1.2, roughness: 0.7, metalness: 0.05, vertexColors: true,
    }));
    const pv = pavementTexture();
    // Pavements, props and plain dressing pick up a little of the street's
    // light (ambientPatch), so kerbs and walls don't sink to flat black.
    this.paveMat = ambientPatch(new THREE.MeshStandardMaterial({ map: pv, vertexColors: true, roughness: 0.78 }), [0.05, 0.042, 0.036], 'pave');
    const as = asphaltTexture(2).clone();
    as.needsUpdate = true;
    this.asphaltMat = new THREE.MeshStandardMaterial({ map: as, color: 0xb8b8b8, roughness: 0.55, metalness: 0.05, polygonOffset: true, polygonOffsetFactor: 1, polygonOffsetUnits: 2 });
    this.paintMat = new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.55, emissive: 0xffffff, emissiveIntensity: 0.05, polygonOffset: true, polygonOffsetFactor: -2, polygonOffsetUnits: -2 });
    this.plainMat = ambientPatch(new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.7, metalness: 0.15 }), [0.06, 0.05, 0.045], 'plain');
    this.glowMat = new THREE.MeshBasicMaterial({ vertexColors: true }); // HDR vertex colours (LED bands)
    const N = neonAtlas();
    // Single-sided: blade signs carry a face each way (see signQuad). Each
    // sign's vertices carry a seed and a mode, so a few buzz and blink.
    this.neonTime = { value: 0 };
    this.neonH = neonFlicker(new THREE.MeshBasicMaterial({ map: N.h, color: new THREE.Color(2.4, 2.4, 2.4) }), this.neonTime, 'h');
    this.neonV = neonFlicker(new THREE.MeshBasicMaterial({ map: N.v, color: new THREE.Color(2.4, 2.4, 2.4) }), this.neonTime, 'v');
    w.updaters.push((dt) => { this.neonTime.value += dt; });
    // Route road: a little wetter than a dry street.
    const rm = w.road.materials.asphalt2;
    if (rm) { rm.roughness = 0.62; rm.metalness = 0.05; rm.color.setScalar(0.72); }

    this.bFac = new Chunks(560, { cell: true, color: true });   // colour = fdata
    this.bStreet = new Chunks(560, { cell: true, color: true });
    this.bPave = new Chunks(560, { color: true });
    this.bAsphalt = new Chunks(560, {});
    this.bPaint = new Chunks(560, { color: true });
    this.bPlain = new Chunks(560, { color: true });
    this.bGlow = new Chunks(560, { color: true });
    this.bCar = new Chunks(560, { color: true });
    this.carMat = ambientPatch(new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.4, metalness: 0.4 }), [0.05, 0.045, 0.045], 'car');
    this.bNeonH = new Chunks(560, { color: true });           // colour = flicker data
    this.bNeonV = new Chunks(560, { color: true });
    this.aircraft = [];
    this.lampSpots = [];
    this.signLights = []; // {x, y, z, col} for wet-road reflections
    this.trees = [];
    this.spill = [];      // shop-window light on the pavement
    this.steam = [];      // steam emitters
    this.cables = [];     // overhead wire segments
    this.stalls = [];
    this.kerbProps = [];
    this.fronts = [];     // street fronts, for props against them
  }

  // A building's seed goes in the high bits of `cell` (see facades.js).
  seed() { return 1 + Math.floor(this.rng() * 900); }

  emitAll() {
    const g = this.group;
    for (const b of this.bFac.map.values()) if (!b.empty) g.add(staticMesh(emitData(b.build(), 'fdata'), this.facMat, { receive: true }));
    this.bStreet.emit(g, this.streetMat, { receive: true });
    this.bPave.emit(g, this.paveMat, { receive: true });
    this.bAsphalt.emit(g, this.asphaltMat, { receive: true });
    this.bPaint.emit(g, this.paintMat, { receive: true });
    this.bPlain.emit(g, this.plainMat, { receive: true });
    this.bGlow.emit(g, this.glowMat, { receive: false });
    this.bCar.emit(g, this.carMat, { receive: true });
    for (const [B, m] of [[this.bNeonH, this.neonH], [this.bNeonV, this.neonV]]) {
      for (const b of B.map.values()) if (!b.empty) g.add(staticMesh(emitData(b.build(), 'ndata'), m, { receive: false }));
    }
    this.buildTrees();
    this.buildAircraftLights();
  }

  // ── Blocks: which exist, how detailed, and their kerb outlines ─
  planBlocks() {
    const { PX, PZ, HW } = this.G;
    this.blocks = new Map();
    for (let i = this.gi0; i <= this.gi1; i++) {
      for (let j = this.gj0; j <= this.gj1; j++) {
        const cx = (i + 0.5) * PX, cz = (j + 0.5) * PZ;
        const d = this.routeDist(cx, cz);
        const tier = d < 70 ? 0 : d < 420 ? 1 : d < 1250 ? 2 : -1;
        if (tier < 0) continue;
        const b = { i, j, tier, d, district: this.G.district(i), cx, cz };
        b.poly = this.blockPoly(i, j, tier === 0);
        this.blocks.set(`${i},${j}`, b);
      }
    }
  }

  // Kerb line of block (i, j): a rounded rectangle, with any part inside the
  // route's paved width pushed out to the road edge (the inside of corners).
  blockPoly(i, j, nearRoute) {
    const { PX, PZ, HW } = this.G;
    const t = this.track;
    const x0 = i * PX + HW, x1 = (i + 1) * PX - HW, z0 = j * PZ + HW, z1 = (j + 1) * PZ - HW;
    const R = 4.5, pts = [];
    const corners = [[x1 - R, z0 + R, -Math.PI / 2], [x1 - R, z1 - R, 0], [x0 + R, z1 - R, Math.PI / 2], [x0 + R, z0 + R, Math.PI]];
    const step = nearRoute ? 1.5 : 1e9;
    corners.forEach(([cx, cz, a0], k) => {
      for (let q = 0; q <= 5; q++) { const a = a0 + (q / 5) * (Math.PI / 2); pts.push([cx + Math.cos(a) * R, cz + Math.sin(a) * R]); }
      // Straight edge to the next corner's start.
      const [nx, nz, na] = corners[(k + 1) % 4];
      const sx = nx + Math.cos(na) * R, sz = nz + Math.sin(na) * R;
      const ex = pts[pts.length - 1][0], ez = pts[pts.length - 1][1];
      const L = Math.hypot(sx - ex, sz - ez), n = Math.max(1, Math.floor(L / step));
      for (let q = 1; q < n; q++) pts.push([ex + ((sx - ex) * q) / n, ez + ((sz - ez) * q) / n]);
    });
    if (!nearRoute) return pts;
    const f = {};
    for (const p of pts) {
      const r = t.distanceToRoad(p[0], p[1], 48);
      if (r.i < 0 || r.d >= HW - 0.01) continue;
      if (!t.loop && (r.s < 0.5 || r.s > t.length - 0.5)) continue;
      t.frame(r.s, f);
      const side = r.lat >= 0 ? 1 : -1;
      p[0] = f.x + f.rx * side * HW;
      p[1] = f.z + f.rz * side * HW;
    }
    // Drop points that bunched up, then straight-line runs.
    const out = [];
    for (const p of pts) if (!out.length || Math.hypot(p[0] - out[out.length - 1][0], p[1] - out[out.length - 1][1]) > 0.35) out.push(p);
    const simp = [];
    for (let k = 0; k < out.length; k++) {
      const a = out[(k - 1 + out.length) % out.length], p = out[k], c = out[(k + 1) % out.length];
      const cross = (p[0] - a[0]) * (c[1] - a[1]) - (p[1] - a[1]) * (c[0] - a[0]);
      const L = Math.hypot(c[0] - a[0], c[1] - a[1]);
      if (Math.abs(cross) / (L || 1) > 0.02 || L > 6) simp.push(p);
    }
    return simp;
  }

  blockAt(x, z) {
    const { PX, PZ } = this.G;
    return this.blocks.get(`${Math.floor(x / PX)},${Math.floor(z / PZ)}`);
  }

  // Is (x, z) on a pavement (behind a kerb)?
  onKerb(x, z) {
    const b = this.blockAt(x, z);
    return !!b && pointInPoly(x, z, b.poly);
  }

  // ── Streets: asphalt, paint and pavements ─────────────────────
  buildStreets() {
    const { PX, PZ, HW } = this.G;
    const gy = this.ground;
    // Every street segment and crossing within the paved tiers.
    const paved = (i, j) => {
      const b = this.blocks.get(`${i},${j}`);
      return b && b.tier <= 1;
    };
    const asph = (x0, z0, x1, z1) => {
      // Axis-aligned rectangle, subdivided along its long side for hills.
      const B = this.bAsphalt.at((x0 + x1) / 2, (z0 + z1) / 2);
      const alongX = x1 - x0 > z1 - z0;
      const L = alongX ? x1 - x0 : z1 - z0, n = Math.max(1, Math.ceil(L / 6));
      for (let k = 0; k < n; k++) {
        const a = k / n, b = (k + 1) / n;
        const [ax0, ax1, az0, az1] = alongX ? [lerp(x0, x1, a), lerp(x0, x1, b), z0, z1] : [x0, x1, lerp(z0, z1, a), lerp(z0, z1, b)];
        const P = (x, z) => [x, gy(x, z) - 0.012, z];
        B.quad(P(ax0, az1), P(ax1, az1), P(ax1, az0), P(ax0, az0), [[ax0 / 9, az1 / 9], [ax1 / 9, az1 / 9], [ax1 / 9, az0 / 9], [ax0 / 9, az0 / 9]]);
      }
    };
    const seen = new Set();
    for (const b of this.blocks.values()) {
      if (b.tier > 1) continue;
      for (const [di, dj] of [[0, 0], [1, 0], [0, 1], [1, 1]]) {
        const i = b.i + di, j = b.j + dj;
        const key = `c${i},${j}`;
        if (seen.has(key)) continue;
        seen.add(key);
        asph(i * PX - HW, j * PZ - HW, i * PX + HW, j * PZ + HW);
        this.crosswalks(i, j);
      }
      // Its north (h) and west (v) street segments, plus south/east when
      // the neighbouring block isn't paved.
      const segs = [['h', b.i, b.j], ['v', b.i, b.j]];
      if (!paved(b.i, b.j + 1)) segs.push(['h', b.i, b.j + 1]);
      if (!paved(b.i + 1, b.j)) segs.push(['v', b.i + 1, b.j]);
      for (const [k, i, j] of segs) {
        const key = `${k},${i},${j}`;
        if (seen.has(key)) continue;
        seen.add(key);
        const full = this.fullSeg.has(key);
        if (k === 'h') {
          if (!full) asph(i * PX + HW, j * PZ - HW, (i + 1) * PX - HW, j * PZ + HW);
          this.lanePaint('h', i, j, full);
        } else {
          if (!full) asph(i * PX - HW, j * PZ + HW, i * PX + HW, (j + 1) * PZ - HW);
          this.lanePaint('v', i, j, full);
        }
      }
      this.pavement(b);
    }
    // Far blocks: no kerbs, just a slab so buildings don't float on the terrain.
    for (const b of this.blocks.values()) if (b.tier === 2) this.pavement(b, true);
  }

  // Zebra crossings on each arm of crossing (i, j).
  crosswalks(i, j) {
    const { PX, PZ, HW } = this.G;
    const cx = i * PX, cz = j * PZ;
    const B = this.bPaint.at(cx, cz);
    const W = [0.9, 0.9, 0.88];
    for (const [ax, az] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
      // Stripes run along the arm, spread across it.
      const d0 = HW + 1.2, d1 = HW + 4.2;
      for (let q = -HW + 0.6; q < HW - 0.5; q += 1.2) {
        const w = 0.55;
        const pts = ax ? [[cx + ax * d0, cz + q], [cx + ax * d1, cz + q], [cx + ax * d1, cz + q + w], [cx + ax * d0, cz + q + w]]
          : [[cx + q, cz + az * d0], [cx + q, cz + az * d1], [cx + q + w, cz + az * d1], [cx + q + w, cz + az * d0]];
        this.paintQuad(B, pts, W);
      }
    }
  }

  paintQuad(B, pts, col, lift = 0.03) {
    const gy = this.ground;
    const P = pts.map(([x, z]) => [x, gy(x, z) + lift, z]);
    // Face up whichever way the corners wind.
    const n = (P[1][2] - P[0][2]) * (P[2][0] - P[0][0]) - (P[1][0] - P[0][0]) * (P[2][2] - P[0][2]);
    if (n >= 0) B.quad(P[0], P[1], P[2], P[3], null, col);
    else B.quad(P[0], P[3], P[2], P[1], null, col);
  }

  // Lane lines on a street segment. On route segments Road.js has painted
  // the lanes; we only add them on the arms of corners it left blank.
  lanePaint(k, i, j, full) {
    const { PX, PZ, HW, setback } = this.G;
    if (full) return;
    const Y = [0.95, 0.72, 0.12], Wt = [0.92, 0.92, 0.9];
    const along = k === 'h';
    const a0 = along ? i * PX : j * PZ, a1 = along ? (i + 1) * PX : (j + 1) * PZ;
    const c = along ? j * PZ : i * PX;
    const B = this.bPaint.at(along ? (a0 + a1) / 2 : c, along ? c : (a0 + a1) / 2);
    // Where the route's corner already covers part of this segment, stop the
    // paint at the corner's start so it doesn't cross the curve.
    const endA = this.crossUse.get(`${i},${j}`), endB = this.crossUse.get(along ? `${i + 1},${j}` : `${i},${j + 1}`);
    const onRouteLine = this.segTouchesRoute(k, i, j);
    let s0 = a0 + HW + 4.6, s1 = a1 - HW - 4.6;
    if (onRouteLine && endA) s0 = Math.max(s0, a0 + setback + 1);
    if (onRouteLine && endB) s1 = Math.min(s1, a1 - setback - 1);
    if (onRouteLine && endA && endB) return;
    const strip = (lat, w, col, dash) => {
      if (s1 - s0 <= 0.5) return;
      const pieces = [];
      if (dash) for (let u = s0; u < s1; u += dash[1]) pieces.push([u, Math.min(s1, u + dash[0])]);
      else for (let u = s0; u < s1; u += 6) pieces.push([u, Math.min(s1, u + 6)]);
      for (const [u0, u1] of pieces) {
        const pts = along ? [[u0, c + lat - w / 2], [u1, c + lat - w / 2], [u1, c + lat + w / 2], [u0, c + lat + w / 2]]
          : [[c + lat - w / 2, u0], [c + lat - w / 2, u1], [c + lat + w / 2, u1], [c + lat + w / 2, u0]];
        this.paintQuad(B, pts, col);
      }
    };
    strip(-0.14, 0.12, Y); strip(0.14, 0.12, Y);
    strip(-HW / 2, 0.13, Wt, [3, 9]); strip(HW / 2, 0.13, Wt, [3, 9]);
    // Stop lines: traffic keeps right, so the approach half is on its right.
    const stop = (u, sign) => {
      const lat0 = sign > 0 ? 0.25 : -HW + 0.3, lat1 = sign > 0 ? HW - 0.3 : -0.25;
      const pts = along ? [[u - 0.2, c + lat0], [u + 0.2, c + lat0], [u + 0.2, c + lat1], [u - 0.2, c + lat1]]
        : [[c + lat0, u - 0.2], [c + lat0, u + 0.2], [c + lat1, u + 0.2], [c + lat1, u - 0.2]];
      this.paintQuad(B, pts, Wt);
    };
    // Heading +along, the right side is +lat for 'h' (right of east is south, +z)
    // and −lat for 'v' (right of south is west, −x).
    if (!(onRouteLine && endB)) stop(a1 - HW - 4.8, along ? 1 : -1);
    if (!(onRouteLine && endA)) stop(a0 + HW + 4.8, along ? -1 : 1);
  }

  // Does the route run along any part of this grid street segment?
  segTouchesRoute(k, i, j) {
    const { PX, PZ } = this.G;
    for (const leg of this.G.legs) {
      if (k === 'h' && leg.axis === 'z' && leg.line === j) {
        const a = Math.min(leg.x0, leg.x1), b = Math.max(leg.x0, leg.x1);
        if (b > i * PX - 40 && a < (i + 1) * PX + 40) return true;
      }
      if (k === 'v' && leg.axis === 'x' && leg.line === i) {
        const a = Math.min(leg.z0, leg.z1), b = Math.max(leg.z0, leg.z1);
        if (b > j * PZ - 40 && a < (j + 1) * PZ + 40) return true;
      }
    }
    return false;
  }

  // Pavement slab for a block: kerb face, kerb stones, pavement ring and
  // the lot inside, all following the ground.
  pavement(b, simple = false) {
    const { WALK } = this.G;
    const gy = this.ground;
    const poly = b.poly;
    const n = poly.length;
    let cx = 0, cz = 0;
    for (const p of poly) { cx += p[0]; cz += p[1]; }
    cx /= n; cz /= n;
    const B = this.bPave.at(cx, cz);
    const KERB = [0.66, 0.65, 0.62], WALKC = [0.5, 0.48, 0.46], LOT = [0.44, 0.42, 0.4];
    // Inward offsets along vertex normals (the outline is convex).
    const inset = (d) => poly.map((p, k) => {
      const a = poly[(k - 1 + n) % n], c = poly[(k + 1) % n];
      let nx = 0, nz = 0;
      for (const [u, v] of [[a, p], [p, c]]) {
        const ex = v[0] - u[0], ez = v[1] - u[1], L = Math.hypot(ex, ez) || 1;
        nx += -ez / L; nz += ex / L;
      }
      const L = Math.hypot(nx, nz) || 1;
      nx /= L; nz /= L;
      // Point the normal inward.
      if ((cx - p[0]) * nx + (cz - p[1]) * nz < 0) { nx = -nx; nz = -nz; }
      return [p[0] + nx * d, p[1] + nz * d];
    });
    const lift = 0.15;
    const V = (p, dy = lift) => [p[0], gy(p[0], p[1]) + dy, p[1]];
    const quadUp = (a, bb, c, d, col) => {
      const n1 = (bb[2] - a[2]) * (c[0] - a[0]) - (bb[0] - a[0]) * (c[2] - a[2]);
      const uv = [a, bb, c, d].map((q) => [q[0] / 3, q[2] / 3]);
      if (n1 >= 0) B.quad(a, bb, c, d, uv, col);
      else B.quad(a, d, c, bb, [uv[0], uv[3], uv[2], uv[1]], col);
    };
    if (simple) {
      const c = [cx, gy(cx, cz) + 0.05, cz];
      for (let k = 0; k < n; k++) {
        const a = V(poly[k], 0.05), d = V(poly[(k + 1) % n], 0.05);
        quadUp(a, d, c, c, LOT);
      }
      return;
    }
    const r1 = inset(0.35), r2 = inset(WALK);
    for (let k = 0; k < n; k++) {
      const k2 = (k + 1) % n;
      // Kerb face, facing out.
      const a = poly[k], c = poly[k2];
      const lo = -0.12;
      const A0 = V(a, lo), A1 = V(a), C0 = V(c, lo), C1 = V(c);
      const ex = c[0] - a[0], ez = c[1] - a[1];
      const outward = (a[0] - cx) * -ez + (a[1] - cz) * ex; // sign of the left normal · outward
      if (outward > 0) B.quad(A0, C0, C1, A1, null, KERB); else B.quad(C0, A0, A1, C1, null, KERB);
      quadUp(V(a), V(c), V(r1[k2]), V(r1[k]), KERB);
      quadUp(V(r1[k]), V(r1[k2]), V(r2[k2]), V(r2[k]), WALKC);
    }
    // The lot inside (mostly under buildings; open corners read as plazas).
    const C = [cx, gy(cx, cz) + lift, cz];
    for (let k = 0; k < n; k++) quadUp(V(r2[k]), V(r2[(k + 1) % n]), C, C, LOT);
  }

  // ── Buildings ─────────────────────────────────────────────────
  buildBlocks() {
    for (const b of this.blocks.values()) {
      if (b.tier === 2) { this.farBlock(b); continue; }
      this.block(b);
    }
  }

  // Lots round the edge of a block: [x0, z0, x1, z1, face] where face is the
  // outward direction of the street front: 'n' (−z), 's', 'w' (−x), 'e'.
  lots(b, frontage, depth) {
    const { PX, PZ, HW, WALK } = this.G;
    const rng = this.rng;
    const bx0 = b.i * PX + HW + WALK, bx1 = (b.i + 1) * PX - HW - WALK;
    const bz0 = b.j * PZ + HW + WALK, bz1 = (b.j + 1) * PZ - HW - WALK;
    const D = Math.min(depth, (bz1 - bz0) / 2, (bx1 - bx0) / 2);
    const out = [];
    const split = (a0, a1) => {
      const cuts = [a0];
      let u = a0;
      while (a1 - u > frontage[0] * 1.5) {
        u += frontage[0] + rng() * (frontage[1] - frontage[0]);
        if (a1 - u < frontage[0]) break;
        cuts.push(u);
      }
      cuts.push(a1);
      return cuts;
    };
    const xs = split(bx0, bx1);
    for (let k = 0; k < xs.length - 1; k++) {
      out.push([xs[k], bz0, xs[k + 1], bz0 + D, 'n']);
    }
    const xs2 = split(bx0, bx1);
    for (let k = 0; k < xs2.length - 1; k++) out.push([xs2[k], bz1 - D, xs2[k + 1], bz1, 's']);
    const zs = split(bz0 + D, bz1 - D);
    for (let k = 0; k < zs.length - 1; k++) out.push([bx0, zs[k], bx0 + D, zs[k + 1], 'w']);
    const zs2 = split(bz0 + D, bz1 - D);
    for (let k = 0; k < zs2.length - 1; k++) out.push([bx1 - D, zs2[k], bx1, zs2[k + 1], 'e']);
    return out;
  }

  // Would a lot crowd the race route (the inside of a corner)?
  lotBlocked(l) {
    const { HW, WALK } = this.G;
    const [x0, z0, x1, z1] = l;
    for (const [x, z] of [[x0, z0], [x1, z0], [x1, z1], [x0, z1], [(x0 + x1) / 2, z0], [(x0 + x1) / 2, z1], [x0, (z0 + z1) / 2], [x1, (z0 + z1) / 2]]) {
      const r = this.track.distanceToRoad(x, z, 40);
      if (r.i >= 0 && r.d < HW + WALK - 0.6) return true;
    }
    return false;
  }

  block(b) {
    const rng = this.rng;
    const D = b.district;
    const near = b.tier === 0;
    if (D === 2) {
      const r = Math.hypot(b.cx - this.core.x, b.cz - this.core.z);
      const core = Math.exp(-Math.pow(r / 520, 2));
      if (near && rng() < 0.12 && core < 0.8) { this.plaza(b); return; }
      if (rng() < 0.35 + core * 0.55) { this.towerBlock(b, core); return; }
      for (const l of this.lots(b, [16, 30], 30)) {
        if (near && this.lotBlocked(l)) continue;
        this.midrise(l, b, { floors: [6, 14 + Math.round(core * 10)], shop: rng() < 0.5 ? LOBBY : SHOP_A });
      }
      return;
    }
    if (D === 1) {
      for (const l of this.lots(b, [5.6, 7.6], 17)) {
        if (near && this.lotBlocked(l)) continue;
        if (rng() < 0.12) this.midrise(l, b, { floors: [3, 6], shop: rng() < 0.5 ? SHOP_B : SHOP_A, cells: [F.BRICK, F.HOTEL] });
        else this.rowhouse(l, b);
      }
      if (near) this.streetTrees(b, 0.5);
      return;
    }
    for (const l of this.lots(b, [8, 16], 24)) {
      if (near && this.lotBlocked(l)) continue;
      const r = rng();
      this.midrise(l, b, { floors: [2, 7], neon: near || rng() < 0.1, shop: r < 0.15 ? SHUTTER : r < 0.27 ? KONBINI : r < 0.62 ? SHOP_A : SHOP_B });
    }
  }

  // A lot's street-front wall and its other three walls, as [a, b] xz
  // pairs; wall() works out which way each one faces from the lot centre.
  lotWalls(x0, z0, x1, z1, face) {
    const walls = { n: [[x1, z0], [x0, z0]], s: [[x0, z1], [x1, z1]], w: [[x0, z0], [x0, z1]], e: [[x1, z1], [x1, z0]] };
    return { front: walls[face], others: Object.entries(walls).filter(([k]) => k !== face).map(([, w]) => w) };
  }

  // One wall quad with outward normal, atlas UVs in tile units.
  wall(B, a, c, y0, y1, cell, tile, vRef, col, cxz, uOff = 0) {
    let A = a, C = c;
    const ex = C[0] - A[0], ez = C[1] - A[1];
    // GeoBuilder.quad's normal is (−ez, 0, ex) for a→c; flip to face out.
    if ((-ez) * (A[0] - cxz[0]) + ex * (A[1] - cxz[1]) < 0) { A = c; C = a; }
    const L = Math.hypot(ex, ez);
    const u0 = uOff, u1 = uOff + L / tile[0];
    const v0 = (y0 - vRef) / tile[1], v1 = (y1 - vRef) / tile[1];
    B.quad([A[0], y0, A[1]], [C[0], y0, C[1]], [C[0], y1, C[1]], [A[0], y1, A[1]], [[u0, v0], [u1, v0], [u1, v1], [u0, v1]], col, cell);
  }

  roof(B, x0, z0, x1, z1, y, cell = F.ROOF, col = null) {
    B.triUV([x0, y, z0], [x1, y, z1], [x1, y, z0], [x0 / 16, z0 / 16], [x1 / 16, z1 / 16], [x1 / 16, z0 / 16], col, cell);
    B.triUV([x0, y, z0], [x0, y, z1], [x1, y, z1], [x0 / 16, z0 / 16], [x0 / 16, z1 / 16], [x1 / 16, z1 / 16], col, cell);
  }

  // Walls round a closed footprint on the façade atlas, u running on round
  // the corners so window indices (and the lights) stay continuous.
  ring(B, poly, y0, y1, cell, vRef, fd, cxz, uOff = 0) {
    const tile = F_TILE[cell % 16];
    let u = uOff;
    for (let k = 0; k < poly.length; k++) {
      const a = poly[k], c = poly[(k + 1) % poly.length];
      this.wall(B, a, c, y0, y1, cell, tile, vRef, fd, cxz, u);
      u += Math.hypot(c[0] - a[0], c[1] - a[1]) / tile[0];
    }
  }

  // Lowest ground under a footprint (buildings on the hill stand on it).
  lotGround(x0, z0, x1, z1) {
    const gy = this.ground;
    let lo = Infinity;
    for (const [x, z] of [[x0, z0], [x1, z0], [x1, z1], [x0, z1], [(x0 + x1) / 2, (z0 + z1) / 2]]) lo = Math.min(lo, gy(x, z));
    return lo;
  }

  // Front frame of a lot: its street wall, tangent and outward normal.
  frontFrame(fr, cx, cz) {
    const [fa, fb] = fr.front;
    const fmx = (fa[0] + fb[0]) / 2, fmz = (fa[1] + fb[1]) / 2;
    const ex = fb[0] - fa[0], ez = fb[1] - fa[1], L = Math.hypot(ex, ez);
    const tx = ex / L, tz = ez / L;
    let nx = -tz, nz = tx;
    if (nx * (fmx - cx) + nz * (fmz - cz) < 0) { nx = -nx; nz = -nz; }
    return { fa, fb, fmx, fmz, L, tx, tz, nx, nz };
  }

  // Shops at street level, façade floors above, signs on the front.
  midrise(l, b, o) {
    const rng = this.rng;
    const [x0, z0, x1, z1, face] = l;
    const inset = 0.25;
    const X0 = x0 + inset, X1 = x1 - inset, Z0 = z0 + inset, Z1 = z1 - inset;
    const cx = (X0 + X1) / 2, cz = (Z0 + Z1) / 2;
    const gy = this.ground;
    const fr = this.lotWalls(X0, Z0, X1, Z1, face);
    const { fa, fmx, fmz, L, tx, tz, nx, nz } = this.frontFrame(fr, cx, cz);
    const floorY = gy(fmx, fmz) + 0.15;            // street level at the front door
    const base = this.lotGround(X0, Z0, X1, Z1) - 1;
    const SH = 4.6;
    const D = b.district;
    const seed = this.seed();
    const pick = (a) => a[Math.floor(rng() * a.length)];
    const cellU = o.cells ? pick(o.cells) : D === 0 ? pick([F.NAPT, F.NTILE, F.BRICK, F.HOTEL, F.NAPT, F.RIBBON, F.NTILE]) : D === 1 ? pick([F.BRICK, F.HOTEL, F.BRICK]) : pick([F.STONE, F.RIBBON, F.BANDS, F.CURTAIN]);
    const fh = floorH(cellU);
    const floors = o.floors[0] + Math.floor(rng() * (o.floors[1] - o.floors[0] + 1));
    const top = floorY + SH + floors * fh;
    const hue = D === 0 ? 0.02 + rng() * 0.96 : 0;
    const fd = [floorY, D === 0 ? 1.0 : 0.75, hue];
    const Bs = this.bStreet.at(cx, cz), Bf = this.bFac.at(cx, cz);
    const shop = o.shop ?? SHOP_A;
    const wallCol = [0.75 + rng() * 0.2, 0.72 + rng() * 0.18, 0.68 + rng() * 0.2];
    // Street floor.
    this.wall(Bs, fr.front[0], fr.front[1], base, floorY + SH, shop + 16 * seed, S_TILE[shop], floorY, [1, 1, 1], [cx, cz], Math.floor(rng() * 4) / 4);
    // Side walls: stucco, unless it's a corner lot whose side faces the
    // cross street, which gets shop windows too.
    const { PX, PZ, HW, WALK } = this.G;
    const e0 = HW + WALK + 1.2;
    const onStreet = (m) => m[0] - b.i * PX < e0 || (b.i + 1) * PX - m[0] < e0 || m[1] - b.j * PZ < e0 || (b.j + 1) * PZ - m[1] < e0;
    for (const w of fr.others) {
      const m = [(w[0][0] + w[1][0]) / 2, (w[0][1] + w[1][1]) / 2];
      const side = b.tier === 0 && onStreet(m) ? (shop === KONBINI || shop === LOBBY ? shop : rng() < 0.5 ? SHOP_B : SHUTTER) : null;
      if (side !== null) this.wall(Bs, w[0], w[1], base, floorY + SH, side + 16 * seed, S_TILE[side], floorY, [1, 1, 1], [cx, cz], rng());
      else this.wall(Bs, w[0], w[1], base, floorY + SH, STUCCO + 16 * seed, S_TILE[STUCCO], floorY, wallCol, [cx, cz]);
    }
    // Floors above, a belt course over the shops and a cornice that caps
    // the roof.
    const cols = [4, 4, 4, 4, 8, 4, 6, 6][cellU] || 4;
    const poly = [[X0, Z0], [X1, Z0], [X1, Z1], [X0, Z1]];
    this.ring(Bf, poly, floorY + SH, top, cellU + 16 * seed, floorY + SH, fd, [cx, cz], Math.floor(rng() * cols) / cols);
    const yaw = Math.atan2(tz, tx);
    const pan = F.PANEL + 16 * seed;
    if (b.tier === 0) Bf.box(fmx + nx * 0.08, floorY + SH - 0.25, fmz + nz * 0.08, L + 0.1, 0.45, 0.4, yaw, { cell: pan, color: fd, tileW: 4, tileH: 4, roofCell: pan });
    Bf.box(cx, top - 0.55, cz, X1 - X0 + 0.5, 1.1, Z1 - Z0 + 0.5, 0, { cell: pan, color: fd, tileW: 4, tileH: 4, roofCell: F.ROOF + 16 * seed, roofTile: 16 });
    // Rooftop plant and, on older blocks, a water tank on legs.
    const P = this.bPlain.at(cx, cz);
    const pc = [0.3, 0.3, 0.32];
    for (let k = 0; k < (b.tier === 0 ? 1 + Math.floor(rng() * 3) : 0); k++) {
      const w = 1.5 + rng() * 3, d = 1.5 + rng() * 3;
      const px = lerp(X0 + w, X1 - w, rng()), pz = lerp(Z0 + d, Z1 - d, rng());
      P.box(px, top + 0.55, pz, w, 1 + rng() * 2, d, 0, { color: pc });
    }
    if (D !== 2 && rng() < 0.25 && b.tier === 0) {
      const px = lerp(X0 + 3, X1 - 3, rng()), pz = lerp(Z0 + 3, Z1 - 3, rng());
      for (const [lx, lz] of [[-1, -1], [1, -1], [1, 1], [-1, 1]]) P.box(px + lx, top + 0.55, pz + lz, 0.15, 2.9, 0.15, 0, { color: [0.2, 0.2, 0.2] });
      P.box(px, top + 3.4, pz, 3, 0.2, 3, 0, { color: [0.25, 0.22, 0.2] });
      Props.addGeo(P, new THREE.CylinderGeometry(1.4, 1.4, 3, 10).translate(0, 1.5, 0).toNonIndexed(), new THREE.Matrix4().setPosition(px, top + 3.6, pz), [0.42, 0.3, 0.22]);
      Props.addGeo(P, new THREE.ConeGeometry(1.5, 0.8, 10).translate(0, 0.4, 0).toNonIndexed(), new THREE.Matrix4().setPosition(px, top + 6.6, pz), [0.3, 0.24, 0.2]);
    }
    // Remember the front for pavement props and the light it spills.
    const lit = shop === SHUTTER ? null : shop === SHOP_B ? [0.5, 0.28, 0.1] : shop === KONBINI ? [0.36, 0.42, 0.5] : shop === LOBBY ? [0.4, 0.32, 0.2] : [0.3, 0.3, 0.38];
    this.fronts.push({ fa, tx, tz, nx, nz, L, y: floorY, district: D, tier: b.tier, shop, lit });
    if (shop !== SHUTTER && b.tier === 0 && rng() < (D === 0 ? 0.6 : 0.3)) {
      // Striped awning over the shop front, with a valance.
      const c = AWNING[Math.floor(rng() * AWNING.length)].map((v) => v * 1.6);
      const w0 = 0.5, w1 = L - 0.5, y0 = floorY + 3.55, y1 = floorY + 3.0, out = 1.6;
      const A = [fa[0] + tx * w0 + nx * 0.05, y0, fa[1] + tz * w0 + nz * 0.05];
      const Bq = [fa[0] + tx * w1 + nx * 0.05, y0, fa[1] + tz * w1 + nz * 0.05];
      const C = [Bq[0] + nx * out, y1, Bq[2] + nz * out];
      const Dq = [A[0] + nx * out, y1, A[2] + nz * out];
      const uL = (w1 - w0) / S_TILE[AWNING_C][0];
      const cell = AWNING_C + 16 * seed;
      const top4 = [[0, 1], [uL, 1], [uL, 0], [0, 0]];
      // Up-facing and down-facing sides (the underside darker).
      Bs.quad(A, Dq, C, Bq, [top4[0], top4[3], top4[2], top4[1]], c, cell);
      Bs.quad(A, Bq, C, Dq, top4, c.map((v) => v * 0.55), cell);
      const E = [C[0], y1 - 0.35, C[2]], G = [Dq[0], y1 - 0.35, Dq[2]];
      Bs.quad(G, E, C, Dq, [[0, 0], [uL, 0], [uL, 0.35], [0, 0.35]], c, cell);
      Bs.quad(E, G, Dq, C, [[0, 0], [uL, 0], [uL, 0.35], [0, 0.35]], c.map((v) => v * 0.55), cell);
    }
    const flick = () => { const r = rng(); return [rng(), r < 0.74 ? 0 : r < 0.86 ? 1 : r < 0.94 ? 3 : 2, 0]; };
    if (o.neon && floors >= 1) {
      // Fascia sign over the shop, blade signs sticking out, small signs in
      // upper windows: layers of neon up the street.
      if (rng() < 0.8) {
        const k = Math.floor(rng() * H_SIGNS);
        const w = Math.min(L - 1, 3 + rng() * 2.5), h = w / 2;
        const u = (L - w) / 2 + (rng() - 0.5) * (L - w - 0.4);
        const y0 = floorY + SH + 0.3;
        this.hSign(fa, tx, tz, nx, nz, u, w, h, y0, 0.12, k, flick());
      }
      if (floors >= 3 && rng() < 0.35) {
        const k = Math.floor(rng() * H_SIGNS);
        const w = 1.4 + rng() * 0.8, u = 0.8 + rng() * (L - w - 1.6);
        this.hSign(fa, tx, tz, nx, nz, u, w, w / 2, floorY + SH + fh * (1.3 + Math.floor(rng() * 2)), 0.06, k, flick());
      }
      const blades = floors >= 2 ? (rng() < 0.6 ? 1 : 0) + (floors >= 4 && rng() < 0.45 ? 1 : 0) : 0;
      for (let q = 0; q < blades; q++) {
        const k = Math.floor(rng() * V_SIGNS);
        const room = top - floorY - SH - 1.2;
        const h = Math.min(room, 3.5 + rng() * 4) * (q ? 0.7 : 1), w = h / 4;
        if (h < 2) continue;
        const u = q ? (rng() < 0.5 ? L * 0.35 : L * 0.65) : (rng() < 0.5 ? 0.6 : L - 0.6);
        const y0 = floorY + SH + 1 + (q ? Math.max(0, room - h) : 0);
        this.bladeSign(fa, tx, tz, nx, nz, u, w, h, y0, k, flick());
      }
    }
    // Overhead wires strung across Neon District streets.
    if (D === 0 && b.tier === 0 && rng() < 0.4) {
      const street = 2 * (this.G.HW + this.G.WALK) + 0.4;
      for (let q = 0; q < 1 + Math.floor(rng() * 3); q++) {
        const u = rng() * L, y = floorY + SH + 1.5 + rng() * 5;
        const a = [fa[0] + tx * u, y, fa[1] + tz * u], d = (rng() - 0.5) * 8;
        Props.wire(this, a, [a[0] + nx * street + tx * d, y + (rng() - 0.5) * 2, a[2] + nz * street + tz * d], 0.6 + rng() * 1.2);
      }
    }
    // A rooftop billboard now and then, facing the street.
    if (o.neon && b.tier === 0 && rng() < 0.06 && L > 9) {
      const w = Math.min(L - 1, 12), h = w * 0.375;
      const ox = fmx - tx * w / 2 - nx * 2, oz = fmz - tz * w / 2 - nz * 2, y0 = top + 1.5;
      (this.billboards ||= []).push({ ox, oz, y0, tx, tz, w, h, ad: Math.floor(rng() * AD_COUNT) });
    }
  }

  // Horizontal neon sign flat on a front, u metres along it.
  hSign(fa, tx, tz, nx, nz, u, w, h, y0, off, k, nd) {
    const ox = fa[0] + tx * u + nx * off, oz = fa[1] + tz * u + nz * off;
    const cu = (k % 4) / 4, cv = 1 - Math.floor(k / 4) / 4;
    // Read left to right from the street: start from whichever end is on
    // the viewer's left.
    const flip = -tz * nx + tx * nz < 0;
    const sx = flip ? ox + tx * w : ox, sz = flip ? oz + tz * w : oz;
    this.signQuad(this.bNeonH.at(ox, oz), [sx, y0, sz], flip ? -tx : tx, flip ? -tz : tz, w, h, nx, nz, [cu, cv - 0.25, cu + 0.25, cv], nd);
    // A dark backing box so the sign has depth.
    this.bPlain.at(ox, oz).box(ox + tx * w / 2 - nx * (off / 2), y0 - 0.05, oz + tz * w / 2 - nz * (off / 2), w + 0.1, h + 0.1, Math.max(0.04, off - 0.02), Math.atan2(tz, tx), { color: [0.08, 0.08, 0.09] });
    this.signLights.push({ x: ox + tx * w / 2, y: y0 + h / 2, z: oz + tz * w / 2, col: NEON_COLORS[k % NEON_COLORS.length] });
  }

  // Vertical blade sign on a bracket: a lit face each way along the street
  // on a dark box, sticking out from the front.
  bladeSign(fa, tx, tz, nx, nz, u, w, h, y0, k, nd) {
    const bx = fa[0] + tx * u + nx * 0.35, bz = fa[1] + tz * u + nz * 0.35;
    const cu = k / 8;
    const Bv = this.bNeonV.at(bx, bz);
    for (const d of [1, -1]) {
      const along = d * (-nz * tx + nx * tz) >= 0; // does +n run left-to-right seen from this side?
      const ox = bx + tx * d * 0.09, oz = bz + tz * d * 0.09;
      const p0 = along ? [ox, y0, oz] : [ox + nx * w, y0, oz + nz * w];
      this.signQuad(Bv, p0, along ? nx : -nx, along ? nz : -nz, w, h, tx * d, tz * d, [cu, 0, cu + 0.125, 1], nd);
    }
    const P = this.bPlain.at(bx, bz);
    const yaw = Math.atan2(nz, nx);
    P.box(bx + nx * w / 2, y0 - 0.06, bz + nz * w / 2, w + 0.1, h + 0.12, 0.16, yaw, { color: [0.07, 0.07, 0.08] });
    for (const yy of [y0 + 0.3, y0 + h - 0.4]) P.box(bx - nx * 0.18, yy, bz - nz * 0.18, 0.4, 0.06, 0.06, yaw, { color: [0.2, 0.2, 0.22] });
    this.signLights.push({ x: bx + nx * w / 2, y: y0 + h / 2, z: bz + nz * w / 2, col: NEON_COLORS[(k * 3 + 1) % NEON_COLORS.length] });
  }

  // Painted Victorian rowhouse: a garage storey with the entry up a stoop,
  // two or three floors with a canted bay window, floor bands, a bracketed
  // cornice and sometimes a gable.
  rowhouse(l, b) {
    const rng = this.rng;
    const [x0, z0, x1, z1, face] = l;
    const X0 = x0 + 0.1, X1 = x1 - 0.1, Z0 = z0 + 0.1, Z1 = z1 - 0.1;
    const cx = (X0 + X1) / 2, cz = (Z0 + Z1) / 2;
    const gy = this.ground;
    const fr = this.lotWalls(X0, Z0, X1, Z1, face);
    const { fa, fmx, fmz, L, tx, tz, nx, nz } = this.frontFrame(fr, cx, cz);
    const streetY = gy(fmx, fmz) + 0.15;
    const g1 = streetY + 3.5;                 // top of the garage storey
    const base = this.lotGround(X0, Z0, X1, Z1) - 1;
    const floors = 2 + (rng() < 0.35 ? 1 : 0);
    const top = g1 + floors * 3.4;
    const col = PAINTED[Math.floor(rng() * PAINTED.length)];
    const r = rng();
    const trim = r < 0.6 ? [0.97, 0.96, 0.93] : r < 0.72 ? [0.25, 0.34, 0.28] : r < 0.84 ? [0.5, 0.16, 0.16] : [0.95, 0.85, 0.5];
    const seed = this.seed();
    const full = b.tier === 0 && this.routeDist(fmx, fmz) < 26; // small details only where you pass them
    const Bs = this.bStreet.at(cx, cz), P = this.bPlain.at(cx, cz);
    const mirror = rng() < 0.5; // garage on the right, door on the left
    const uOff = mirror ? 1 : 0, tileSign = mirror ? -1 : 1;
    this.wall(Bs, fa, fr.front[1], base, g1, ROW_GROUND + 16 * seed, [tileSign * L, 3.5], streetY, col, [cx, cz], uOff);
    this.wall(Bs, fa, fr.front[1], g1, top, ROWHOUSE + 16 * seed, [tileSign * L, 3.4], g1, col, [cx, cz], uOff);
    for (const w of fr.others) this.wall(Bs, w[0], w[1], base, top, STUCCO + 16 * seed, S_TILE[STUCCO], streetY, col.map((v) => v * 0.85), [cx, cz]);
    this.roof(this.bFac.at(cx, cz), X0, Z0, X1, Z1, top + 0.01, F.ROOF + 16 * seed, [streetY, 0, 0]);
    // wall() maps u along the front from its (possibly swapped) start; find
    // where along the front texture u = 0.25 (the window over the garage) is.
    const flipped = (-(fr.front[1][1] - fa[1])) * (fa[0] - cx) + (fr.front[1][0] - fa[0]) * (fa[1] - cz) < 0;
    const uAt = (uu) => { const m = mirror ? 1 - uu : uu; return flipped ? L * (1 - m) : L * m; };
    const yaw = Math.atan2(tz, tx);
    // Canted bay window over the garage.
    const bu = uAt(0.25), bw = Math.min(L * 0.46, 3.2), dep = 0.7, fw = bw - 2 * dep;
    const bcx = fa[0] + tx * bu, bcz = fa[1] + tz * bu;
    const pt = (a, d) => [bcx + tx * a + nx * d, bcz + tz * a + nz * d];
    const bay = [pt(-bw / 2, 0.02), pt(-fw / 2, dep), pt(fw / 2, dep), pt(bw / 2, 0.02)];
    const yb0 = g1 - 0.1, yb1 = top - 0.55;
    const tileB = [tileSign * L, 3.4];
    for (let k = 0; k < 3; k++) {
      const a = bay[k], c = bay[k + 1];
      const segL = Math.hypot(c[0] - a[0], c[1] - a[1]);
      // Centre the texture's window on each face.
      const uc = 0.25, half = segL / (2 * L);
      this.wall(Bs, a, c, yb0, yb1, ROWHOUSE + 16 * seed, tileB, g1, col, [bcx - nx * 2, bcz - nz * 2], uc - tileSign * half);
    }
    // Bay soffit and cap, and the floor bands across the front.
    const bayPoly = [bay[0], bay[1], bay[2], bay[3]];
    if (b.tier === 0) P.prism(bayPoly.map((p) => [p[0] + nx * 0.06, p[1] + nz * 0.06]), yb1, yb1 + 0.28, { color: trim });
    if (full) P.prism(bayPoly.map((p) => [p[0] + nx * 0.06, p[1] + nz * 0.06]), yb0 - 0.3, yb0, { color: trim, roof: false });
    const soff = bayPoly.map((p) => [p[0], yb0 - 0.3, p[1]]);
    const ny = (soff[1][2] - soff[0][2]) * (soff[2][0] - soff[0][0]) - (soff[1][0] - soff[0][0]) * (soff[2][2] - soff[0][2]);
    if (full && ny < 0) P.quad(soff[0], soff[1], soff[2], soff[3], null, trim.map((v) => v * 0.7));
    else if (full) P.quad(soff[3], soff[2], soff[1], soff[0], null, trim.map((v) => v * 0.7));
    for (let q = 1; q < floors; q++) P.box(fmx + nx * 0.05, g1 + q * 3.4 - 0.1, fmz + nz * 0.05, L, 0.2, 0.12, yaw, { color: trim });
    if (b.tier === 0) P.box(fmx + nx * 0.05, g1 - 0.2, fmz + nz * 0.05, L, 0.25, 0.14, yaw, { color: trim });
    // Cornice with brackets, and a gable on some.
    P.box(fmx + nx * 0.35, top - 0.15, fmz + nz * 0.35, L + 0.1, 0.55, 0.8, yaw, { color: trim });
    if (full) for (let u = 0.35; u < L; u += 0.9) {
      const px = fa[0] + tx * u + nx * 0.2, pz = fa[1] + tz * u + nz * 0.2;
      P.box(px, top - 0.55, pz, 0.14, 0.4, 0.4, yaw, { color: trim });
    }
    // A gable or a parapet over the cornice (only near the route).
    if (b.tier === 0 && rng() < 0.35) {
      const gh = 1.4 + rng() * 0.8, gyb = top + 0.4;
      const A = [fa[0] + nx * 0.1, gyb, fa[1] + nz * 0.1], Bq = [fr.front[1][0] + nx * 0.1, gyb, fr.front[1][1] + nz * 0.1];
      const Cq = [fmx + nx * 0.1, gyb + gh, fmz + nz * 0.1];
      // Outward-facing triangle (either winding works for one of the two).
      const n1 = (Bq[1] - A[1]) * (Cq[2] - A[2]) - (Bq[2] - A[2]) * (Cq[1] - A[1]);
      const nxz = (Bq[0] - A[0]) * (Cq[1] - A[1]) - (Bq[1] - A[1]) * (Cq[0] - A[0]);
      const out = n1 * nx + nxz * nz >= 0;
      if (out) P.tri(A, Bq, Cq, col); else P.tri(Bq, A, Cq, col);
      P.box(fmx + nx * 0.2, gyb - 0.12, fmz + nz * 0.2, L, 0.15, 0.3, yaw, { color: trim });
    } else if (b.tier === 0) {
      P.box(fmx - nx * 0.1, top + 0.1, fmz - nz * 0.1, L, 0.6, 0.3, yaw, { color: col.map((v) => v * 0.9) });
    }
    // Stoop: steps up to the door (texture door at u 0.78, sill 0.8 m up).
    const du = uAt(0.78);
    const sx = fa[0] + tx * du, sz = fa[1] + tz * du;
    const steps = full ? 5 : b.tier === 0 ? 1 : 0;
    for (let k = 0; k < steps; k++) {
      const d = (steps - k) * 0.32;
      const px = sx + nx * d / 2, pz = sz + nz * d / 2;
      P.box(px, streetY - 0.2, pz, 1.3, 0.2 + (k + 1) * 0.16, d, yaw, { color: [0.6, 0.58, 0.55] });
    }
    if (full) for (const s of [-0.7, 0.7]) P.box(sx + tx * s + nx * 0.8, streetY + 0.1, sz + tz * s + nz * 0.8, 0.07, 1.4, 1.6, yaw, { color: trim });
    // Porch light by the door.
    if (rng() < 0.7) this.bGlow.at(sx, sz).box(sx + tx * 0.9 + nx * 0.1, streetY + 2.9, sz + tz * 0.9 + nz * 0.1, 0.16, 0.24, 0.16, yaw, { color: [3, 2.1, 1.1] });
    this.fronts.push({ fa, tx, tz, nx, nz, L, y: streetY, district: 1, tier: b.tier, shop: undefined, lit: null });
  }

  // Office towers: a lit lobby podium, a shaft with setbacks, a crown.
  towerBlock(b, core) {
    const rng = this.rng;
    const { PX, PZ, HW, WALK } = this.G;
    const bx0 = b.i * PX + HW + WALK, bx1 = (b.i + 1) * PX - HW - WALK;
    const bz0 = b.j * PZ + HW + WALK, bz1 = (b.j + 1) * PZ - HW - WALK;
    const lotsXZ = rng() < 0.5 ? [[bx0, bz0, bx1, bz1]] : [[bx0, bz0, (bx0 + bx1) / 2 - 2, bz1], [(bx0 + bx1) / 2 + 2, bz0, bx1, bz1]];
    for (const [x0, z0, x1, z1] of lotsXZ) {
      if (b.tier === 0 && this.lotBlocked([x0, z0, x1, z1])) {
        // Shrink away from the route's corner.
        const s = 0.72, cx = (x0 + x1) / 2, cz = (z0 + z1) / 2;
        const l2 = [cx - (cx - x0) * s, cz - (cz - z0) * s, cx + (x1 - cx) * s, cz + (z1 - cz) * s];
        if (this.lotBlocked(l2)) continue;
        this.tower(l2, core, b);
      } else this.tower([x0, z0, x1, z1], core, b);
    }
  }

  tower([x0, z0, x1, z1], core, b) {
    const rng = this.rng;
    const gy = this.ground;
    const cx = (x0 + x1) / 2, cz = (z0 + z1) / 2;
    const floorY = gy(cx, cz) + 0.15;
    const base = this.lotGround(x0, z0, x1, z1) - 1;
    const Bs = this.bStreet.at(cx, cz), Bf = this.bFac.at(cx, cz), P = this.bPlain.at(cx, cz);
    const H = 60 + core * (90 + rng() * 170) + rng() * 40;
    const seed = this.seed();
    const fd = [floorY, 0.8, 0];
    const rect = (X0, Z0, X1, Z1, ch = 0) => ch > 0
      ? [[X0 + ch, Z0], [X1 - ch, Z0], [X1, Z0 + ch], [X1, Z1 - ch], [X1 - ch, Z1], [X0 + ch, Z1], [X0, Z1 - ch], [X0, Z0 + ch]]
      : [[X0, Z0], [X1, Z0], [X1, Z1], [X0, Z1]];
    const pick = (a) => a[Math.floor(rng() * a.length)];
    // Lobby: double-height glass all round.
    const pr = rect(x0 + 0.3, z0 + 0.3, x1 - 0.3, z1 - 0.3);
    for (let k = 0; k < 4; k++) this.wall(Bs, pr[k], pr[(k + 1) % 4], base, floorY + 6, LOBBY + 16 * seed, S_TILE[LOBBY], floorY, [1, 1, 1], [cx, cz], k * 0.37);
    // Podium: a few floors of stone or strip windows, capped.
    const podCell = pick([F.STONE, F.RIBBON, F.STONE, F.BANDS]);
    const podTop = floorY + 6 + floorH(podCell) * (2 + Math.floor(rng() * 4));
    this.ring(Bf, pr, floorY + 6, podTop, podCell + 16 * seed, floorY + 6, fd, [cx, cz], Math.floor(rng() * 4) / 4);
    const pan = F.PANEL + 16 * seed;
    Bf.box(cx, podTop - 0.4, cz, x1 - x0 - 0.2, 0.9, z1 - z0 - 0.2, 0, { cell: pan, color: fd, tileW: 4, tileH: 4, roofCell: F.ROOF + 16 * seed, roofTile: 16 });
    // Entrance canopy on the side nearest the route, glowing underneath.
    {
      const sides = [[cx, z0 + 0.3, 0, -1], [cx, z1 - 0.3, 0, 1], [x0 + 0.3, cz, -1, 0], [x1 - 0.3, cz, 1, 0]];
      let best = sides[0], bd = Infinity;
      for (const s of sides) { const d = this.routeDist(s[0], s[1]); if (d < bd) { bd = d; best = s; } }
      const [ex, ez, nx, nz] = best;
      const yaw = Math.atan2(nx, -nz); // box length along the wall
      const w = Math.min(10, Math.abs(nx ? z1 - z0 : x1 - x0) * 0.4);
      P.box(ex + nx * 1.6, floorY + 4.3, ez + nz * 1.6, w, 0.35, 3.2, yaw, { color: [0.2, 0.2, 0.22] });
      this.bGlow.at(ex, ez).box(ex + nx * 1.6, floorY + 4.26, ez + nz * 1.6, w - 0.6, 0.04, 2.6, yaw, { color: [2.6, 2.3, 1.8] });
      this.spill.push({ x: ex + nx * 3.5, z: ez + nz * 3.5, r: w * 0.5, col: [0.45, 0.38, 0.26], tx: -nz, tz: nx, long: true });
      if (b) this.fronts.push({ fa: [ex + nz * w / 2, ez - nx * w / 2], tx: -nz, tz: nx, nx, nz, L: w, y: floorY, district: 2, tier: b.tier, shop: LOBBY, lit: null, tower: true });
    }
    // Shaft: maybe chamfered corners, maybe piers; then setbacks.
    const style = pick([F.CURTAIN, F.STONE, F.BANDS, F.FINS, F.CURTAIN, F.DARK, F.RIBBON]);
    const ins = 4 + rng() * 6;
    let X0 = x0 + ins, X1 = x1 - ins, Z0 = z0 + ins, Z1 = z1 - ins;
    const ch = rng() < 0.35 ? 2.5 + rng() * 2 : 0;
    const shaftTop = podTop + (H - (podTop - floorY)) * (0.62 + rng() * 0.2);
    const uOff = Math.floor(rng() * 8) / 8;
    const poly = rect(X0, Z0, X1, Z1, ch);
    this.ring(Bf, poly, podTop - 0.3, shaftTop, style + 16 * seed, podTop, fd, [cx, cz], uOff);
    this.roof(Bf, X0, Z0, X1, Z1, shaftTop, F.ROOF + 16 * seed, fd);
    if ((style === F.STONE || style === F.CURTAIN || style === F.RIBBON) && rng() < 0.65) this.piers(Bf, poly, podTop, shaftTop, pan, fd, [cx, cz], style === F.CURTAIN ? 3 : 3);
    let top = shaftTop;
    const tiers = rng() < 0.8 ? 1 + (rng() < 0.4 ? 1 : 0) : 0;
    let tierStyle = style;
    for (let q = 0; q < tiers; q++) {
      const s = 3 + rng() * 5;
      if (X1 - X0 - 2 * s < 10 || Z1 - Z0 - 2 * s < 10) break;
      X0 += s; X1 -= s; Z0 += s; Z1 -= s;
      const upper = q === tiers - 1 ? floorY + H : top + (floorY + H - top) * 0.55;
      // Setback terrace edge.
      Bf.box((X0 + X1) / 2, top - 0.3, (Z0 + Z1) / 2, X1 - X0 + 2 * s + 0.4, 0.8, Z1 - Z0 + 2 * s + 0.4, 0, { cell: pan, color: fd, tileW: 4, tileH: 4, roofCell: F.ROOF + 16 * seed });
      if (rng() < 0.3) tierStyle = F.DARK;
      this.ring(Bf, rect(X0, Z0, X1, Z1, ch * 0.6), top - 0.3, upper, tierStyle + 16 * seed, podTop, fd, [cx, cz], uOff);
      this.roof(Bf, X0, Z0, X1, Z1, upper, F.ROOF + 16 * seed, fd);
      top = upper;
    }
    // Crown.
    const cr = rng();
    const cp = rect(X0 - 0.2, Z0 - 0.2, X1 + 0.2, Z1 + 0.2, ch * 0.6);
    if (cr < 0.35) {
      // Lit louvred crown screen above the roof.
      this.ring(Bf, cp, top, top + 6, F.CROWN + 16 * seed, top, [0, 0, 0], [cx, cz]);
      top += 6;
    } else if (cr < 0.6) {
      // Glass pyramid.
      const apex = [(X0 + X1) / 2, top + Math.min(X1 - X0, Z1 - Z0) * 0.45, (Z0 + Z1) / 2];
      const r4 = rect(X0, Z0, X1, Z1);
      for (let k = 0; k < 4; k++) {
        const a = r4[k], c = r4[(k + 1) % 4];
        const A = [a[0], top, a[1]], C = [c[0], top, c[1]];
        // Face outward: test against the centre.
        const ex = C[0] - A[0], ez = C[2] - A[2];
        const outward = (-ez) * (A[0] - apex[0]) + ex * (A[2] - apex[2]) >= 0;
        const Lk = Math.hypot(ex, ez) / 12;
        if (outward) Bf.triUV(A, C, apex, [0, 0], [Lk, 0], [Lk / 2, 1.2], fd, F.DARK + 16 * seed);
        else Bf.triUV(C, A, apex, [0, 0], [Lk, 0], [Lk / 2, 1.2], fd, F.DARK + 16 * seed);
      }
      // Lit edges up the ridges.
      const G = this.bGlow.at(cx, cz);
      for (const a of r4) for (let q = 0; q < 6; q++) {
        const u = q / 6;
        G.box(lerp(a[0], apex[0], u), lerp(top, apex[1], u), lerp(a[1], apex[2], u), 0.35, 0.35, 0.35, 0, { color: [3, 2.6, 2] });
      }
      top = apex[1];
    } else if (cr < 0.85) {
      // LED band.
      const c = NEON[Math.floor(rng() * NEON.length)];
      const G = this.bGlow.at(cx, cz);
      for (let k = 0; k < cp.length; k++) this.glowWall(G, cp[k], cp[(k + 1) % cp.length], top - 3.5, top - 2.1, c, [cx, cz]);
      this.ring(Bf, rect(X0 + (X1 - X0) * 0.25, Z0 + (Z1 - Z0) * 0.25, X1 - (X1 - X0) * 0.25, Z1 - (Z1 - Z0) * 0.25), top, top + 4.5, F.MECH + 16 * seed, top, fd, [cx, cz]);
    } else {
      // Mechanical penthouse and a mast.
      const mx0 = X0 + (X1 - X0) * 0.2, mx1 = X1 - (X1 - X0) * 0.2, mz0 = Z0 + (Z1 - Z0) * 0.2, mz1 = Z1 - (Z1 - Z0) * 0.2;
      this.ring(Bf, rect(mx0, mz0, mx1, mz1), top, top + 5, F.MECH + 16 * seed, top, fd, [cx, cz]);
      this.roof(Bf, mx0, mz0, mx1, mz1, top + 5, F.ROOF + 16 * seed, fd);
      const mh = 18 + rng() * 25;
      P.box(cx, top + 5, cz, 0.5, mh, 0.5, 0, { color: [0.5, 0.5, 0.52] });
      top += 5 + mh;
    }
    if (H > 120 || cr >= 0.85) this.aircraft.push([cx, top + 1, cz]);
  }

  // Vertical piers proud of a façade every `pitch` metres: relief that
  // catches the light and breaks up the silhouette at grazing angles.
  piers(B, poly, y0, y1, cell, fd, cxz, pitch) {
    for (let k = 0; k < poly.length; k++) {
      const a = poly[k], c = poly[(k + 1) % poly.length];
      const ex = c[0] - a[0], ez = c[1] - a[1], L = Math.hypot(ex, ez);
      if (L < pitch * 2) continue;
      const tx = ex / L, tz = ez / L;
      let nx = -tz, nz = tx;
      const mx = (a[0] + c[0]) / 2, mz = (a[1] + c[1]) / 2;
      if (nx * (mx - cxz[0]) + nz * (mz - cxz[1]) < 0) { nx = -nx; nz = -nz; }
      const n = Math.round(L / pitch);
      for (let q = 1; q < n; q++) {
        const u = (q / n) * L;
        B.box(a[0] + tx * u + nx * 0.18, y0, a[1] + tz * u + nz * 0.18, 0.45, y1 - y0, 0.36, Math.atan2(tz, tx), { cell, color: fd, tileW: 4, tileH: 4, roofCell: cell });
      }
    }
  }

  // A sign quad at p0 spanning `w` along (ax, az) and `h` up, facing the
  // horizontal normal (nx, nz). uv: [u0, v0, u1, v1]. nd: flicker data.
  signQuad(B, p0, ax, az, w, h, nx, nz, [u0, v0, u1, v1], nd = [0, 0, 0]) {
    const [x, y, z] = p0;
    const a = [x, y, z], b = [x + ax * w, y, z + az * w], c = [x + ax * w, y + h, z + az * w], d = [x, y + h, z];
    // quad(a, b, c, …) faces (−az, ax); flip the winding (and u) if needed.
    if (-az * nx + ax * nz >= 0) B.quad(a, b, c, d, [[u0, v0], [u1, v0], [u1, v1], [u0, v1]], nd);
    else B.quad(b, a, d, c, [[u0, v0], [u1, v0], [u1, v1], [u0, v1]], nd);
  }

  glowWall(B, a, c, y0, y1, col, cxz) {
    let A = a, C = c;
    const ex = C[0] - A[0], ez = C[1] - A[1];
    if ((-ez) * (A[0] - cxz[0]) + ex * (A[1] - cxz[1]) < 0) { A = c; C = a; }
    B.quad([A[0], y0, A[1]], [C[0], y0, C[1]], [C[0], y1, C[1]], [A[0], y1, A[1]], null, col);
  }

  // A small plaza in place of a block: paving, trees in planters, benches,
  // lit bollards and a fountain.
  plaza(b) {
    const rng = this.rng;
    const P = this.bPlain.at(b.cx, b.cz);
    const y = this.ground(b.cx, b.cz) + 0.15;
    for (let k = 0; k < 12; k++) {
      const x = lerp(b.cx - 40, b.cx + 40, rng()), z = lerp(b.cz - 28, b.cz + 28, rng());
      if (Math.hypot(x - b.cx, z - b.cz) < 11) continue;
      const yy = this.ground(x, z) + 0.15;
      this.trees.push({ x, y: yy + 0.6, z, s: 0.9 + rng() * 0.4 });
      P.box(x, yy, z, 2.4, 0.6, 2.4, 0, { color: [0.45, 0.43, 0.4] });
      if (rng() < 0.6) Props.putKit(P, 'bench', new THREE.Matrix4().makeRotationY(rng() * 6.28).setPosition(x + 2.6, yy, z), null);
    }
    P.box(b.cx, y, b.cz, 12, 0.7, 12, 0, { color: [0.45, 0.44, 0.42] });
    this.bGlow.at(b.cx, b.cz).box(b.cx, y + 0.7, b.cz, 1.2, 3.5, 1.2, 0, { color: [0.6, 1.4, 2.2] });
    this.bGlow.at(b.cx, b.cz).box(b.cx, y + 0.72, b.cz, 10.5, 0.05, 10.5, 0, { color: [0.12, 0.35, 0.55] });
    for (let a = 0; a < 6.28; a += 0.52) {
      const x = b.cx + Math.cos(a) * 16, z = b.cz + Math.sin(a) * 16;
      const yy = this.ground(x, z) + 0.15;
      Props.putKit(P, 'bollard', new THREE.Matrix4().setPosition(x, yy, z), null);
      this.bGlow.at(x, z).box(x, yy + 0.72, z, 0.2, 0.12, 0.2, 0, { color: [2.6, 2.3, 1.8] });
    }
    this.spill.push({ x: b.cx, z: b.cz, r: 14, col: [0.12, 0.14, 0.16], tx: 1, tz: 0 });
  }

  // Far blocks: a few plain boxes, taller downtown.
  farBlock(b) {
    const rng = this.rng;
    const { PX, PZ, HW, WALK } = this.G;
    const bx0 = b.i * PX + HW + WALK, bx1 = (b.i + 1) * PX - HW - WALK;
    const bz0 = b.j * PZ + HW + WALK, bz1 = (b.j + 1) * PZ - HW - WALK;
    const r = Math.hypot(b.cx - this.core.x, b.cz - this.core.z);
    const core = b.district === 2 ? Math.exp(-Math.pow(r / 700, 2)) : 0;
    const halves = rng() < 0.5 ? [[bx0, bz0, bx1, (bz0 + bz1) / 2 - 1], [bx0, (bz0 + bz1) / 2 + 1, bx1, bz1]]
      : [[bx0, bz0, (bx0 + bx1) / 2 - 1, bz1], [(bx0 + bx1) / 2 + 1, bz0, bx1, bz1]];
    for (const [x0, z0, x1, z1] of halves) {
      if (rng() < 0.1) continue;
      const cx = (x0 + x1) / 2, cz = (z0 + z1) / 2;
      const B = this.bFac.at(cx, cz);
      const tall = core > 0.2 && rng() < 0.3 + core * 0.5;
      const cell = tall || (b.district === 2 && rng() < 0.5) ? F.FAR_OFF : F.FAR_RES;
      const fh = floorH(cell);
      const H = tall ? 60 + core * 160 * rng() + 40 * rng() : fh * (b.district === 1 ? 3 + Math.floor(rng() * 3) : 3 + Math.floor(rng() * 8));
      const base = this.lotGround(x0, z0, x1, z1) - 1, gy0 = this.ground(cx, cz);
      const seed = this.seed(), fd = [gy0, 0.35, 0];
      this.ring(B, [[x0, z0], [x1, z0], [x1, z1], [x0, z1]], base, gy0 + H, cell + 16 * seed, gy0, fd, [cx, cz], Math.floor(rng() * 8) / 8);
      this.roof(B, x0, z0, x1, z1, gy0 + H, F.ROOF + 16 * seed, fd);
      if (tall && rng() < 0.4) {
        const c = NEON[Math.floor(rng() * NEON.length)];
        const r4 = [[x0 - 0.2, z0 - 0.2], [x1 + 0.2, z0 - 0.2], [x1 + 0.2, z1 + 0.2], [x0 - 0.2, z1 + 0.2]];
        for (let k = 0; k < 4; k++) this.glowWall(this.bGlow.at(cx, cz), r4[k], r4[(k + 1) % 4], gy0 + H - 3, gy0 + H - 1.8, c, [cx, cz]);
      }
      if (tall && H > 150) this.aircraft.push([cx, gy0 + H + 1, cz]);
    }
  }

  streetTrees(b, p) {
    const { PX, PZ, HW } = this.G;
    const rng = this.rng;
    const edge = HW + 1.4;
    for (let x = b.i * PX + HW + 10; x < (b.i + 1) * PX - HW - 8; x += 11) {
      for (const [z, sgn] of [[b.j * PZ + edge, 1], [(b.j + 1) * PZ - edge, -1]]) {
        if (rng() > p || !this.onKerb(x, z)) continue;
        if (this.track.distanceToRoad(x, z, 20).d < HW + 1) continue;
        this.trees.push({ x, y: this.ground(x, z) + 0.15, z, s: 0.7 + rng() * 0.3 });
      }
    }
  }

  buildTrees() {
    if (!this.trees.length) return;
    const trunk = new THREE.CylinderGeometry(0.15, 0.22, 3.2, 5);
    trunk.translate(0, 1.6, 0);
    const crown = new THREE.IcosahedronGeometry(2.2, 0);
    crown.scale(1, 1.2, 1);
    crown.translate(0, 4.5, 0);
    const paint = (g0, c) => {
      const g = g0.index ? g0.toNonIndexed() : g0;
      const n = g.getAttribute('position').count;
      const a = new Float32Array(n * 3);
      for (let i = 0; i < n; i++) a.set(c, i * 3);
      g.setAttribute('color', new THREE.BufferAttribute(a, 3));
      return g;
    };
    const a = paint(trunk, [0.22, 0.15, 0.1]), c = paint(crown, [0.14, 0.25, 0.1]);
    const geo = new THREE.BufferGeometry();
    for (const n of ['position', 'normal', 'color']) {
      const A = a.getAttribute(n), C = c.getAttribute(n);
      const arr = new Float32Array(A.array.length + C.array.length);
      arr.set(A.array); arr.set(C.array, A.array.length);
      geo.setAttribute(n, new THREE.BufferAttribute(arr, 3));
    }
    geo.computeVertexNormals();
    const mat = ambientPatch(new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.95, flatShading: true }), [0.06, 0.07, 0.05], 'tree');
    const rng = this.rng;
    const im = instanced(geo, mat, this.trees.map((t) => trs(t.x, t.y, t.z, rng() * 6.28, t.s, t.s * (0.85 + rng() * 0.3), t.s)), { cast: true, receive: true });
    this.trees.forEach((t, i) => im.setColorAt(i, new THREE.Color().setHSL(0.24 + rng() * 0.08, 0.45, 0.4 + rng() * 0.2)));
    this.group.add(im);
  }

  buildAircraftLights() {
    if (!this.aircraft.length) return;
    const pos = new Float32Array(this.aircraft.flat());
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.BufferAttribute(pos, 3));
    g.computeBoundingSphere();
    const m = new THREE.PointsMaterial({ size: 3.5, sizeAttenuation: false, map: glowTexture(), color: new THREE.Color(6, 0.35, 0.25), transparent: true, depthWrite: false, blending: THREE.AdditiveBlending });
    const pts = new THREE.Points(g, m);
    pts.frustumCulled = false;
    this.group.add(pts);
    let time = 0;
    this.world.updaters.push((dt) => {
      time += dt;
      const on = Math.sin(time * Math.PI * 1.6) > 0.2 ? 1 : 0.08;
      m.color.setRGB(6 * on, 0.35 * on, 0.25 * on);
    });
    // Billboards collected while placing buildings.
    for (const bb of this.billboards || []) this.billboard(bb);
  }

  billboard({ ox, oz, y0, tx, tz, w, h, ad }) {
    const mat = (this.adMats ||= [])[ad] || (this.adMats[ad] = new THREE.MeshBasicMaterial({ map: adTexture(ad), color: new THREE.Color(1.5, 1.5, 1.5), side: THREE.DoubleSide }));
    const g = new THREE.BufferGeometry();
    const P = [[ox, y0, oz], [ox + tx * w, y0, oz + tz * w], [ox + tx * w, y0 + h, oz + tz * w], [ox, y0 + h, oz]];
    g.setAttribute('position', new THREE.Float32BufferAttribute([...P[0], ...P[1], ...P[2], ...P[0], ...P[2], ...P[3]], 3));
    g.setAttribute('uv', new THREE.Float32BufferAttribute([0, 0, 1, 0, 1, 1, 0, 0, 1, 1, 0, 1], 2));
    this.group.add(staticMesh(g, mat, { receive: false }));
    const P2 = this.bPlain.at(ox, oz);
    for (const u of [0.2, 0.8]) P2.box(ox + tx * w * u, y0 - 1.6, oz + tz * w * u, 0.25, 1.6, 0.25, 0, { color: [0.2, 0.2, 0.22] });
    this.signLights.push({ x: ox + tx * w / 2, y: y0 + h / 2, z: oz + tz * w / 2, col: '#ffffff', big: true });
  }

  // ── Street lamps ──────────────────────────────────────────────
  buildLamps() {
    const { PX, PZ, HW } = this.G;
    const spots = [];
    const seen = new Set();
    for (const b of this.blocks.values()) {
      if (b.tier > 1) continue;
      // Lamps along the block's four kerbs, pointing over the road.
      const x0 = b.i * PX + HW + 0.6, x1 = (b.i + 1) * PX - HW - 0.6, z0 = b.j * PZ + HW + 0.6, z1 = (b.j + 1) * PZ - HW - 0.6;
      const every = b.tier === 0 ? 30 : 45;
      const put = (x, z, dx, dz) => {
        const key = Math.round(x) + ',' + Math.round(z);
        if (seen.has(key) || !this.onKerb(x, z)) return;
        seen.add(key);
        spots.push({ x, y: this.ground(x, z) + 0.15, z, dx, dz, near: b.tier === 0 && this.track.distanceToRoad(x, z, 24).d < HW + 3 });
      };
      for (let x = x0 + 12; x < x1 - 8; x += every) { put(x, z0, 0, -1); put(x + every / 2, z1, 0, 1); }
      for (let z = z0 + 12; z < z1 - 8; z += every) { put(x0, z, -1, 0); put(x1, z + every / 2, 1, 0); }
    }
    const pole = new THREE.CylinderGeometry(0.1, 0.14, 1, 6);
    pole.translate(0, 0.5, 0);
    const poleM = [], armM = [], lensM = [], poolM = [];
    for (const s of spots) {
      const yaw = -Math.atan2(s.dz, s.dx), h = 8, arm = 2.2;
      poleM.push(trs(s.x, s.y, s.z, 0, 1, h, 1));
      armM.push(trs(s.x + s.dx * arm / 2, s.y + h - 0.1, s.z + s.dz * arm / 2, yaw, arm, 0.12, 0.12));
      lensM.push(trs(s.x + s.dx * arm, s.y + h - 0.25, s.z + s.dz * arm, yaw, 0.7, 0.12, 0.35));
      if (s.near) {
        const px = s.x + s.dx * (arm + 1.5), pz = s.z + s.dz * (arm + 1.5);
        poolM.push(trs(px, this.ground(px, pz) + 0.06, pz, 0, 12, 1, 12));
        // And a smaller pool on the pavement round the pole.
        poolM.push(trs(s.x - s.dx * 0.8, s.y + 0.05, s.z - s.dz * 0.8, 0, 7, 1, 7));
        this.signLights.push({ x: s.x + s.dx * arm, y: s.y + h, z: s.z + s.dz * arm, col: '#ffb870', lamp: true });
      }
    }
    const metal = new THREE.MeshStandardMaterial({ color: 0x4a4e54, metalness: 0.5, roughness: 0.6 });
    this.group.add(instanced(pole, metal, poleM), instanced(new THREE.BoxGeometry(1, 1, 1), metal, armM));
    this.group.add(instanced(new THREE.BoxGeometry(1, 1, 1), new THREE.MeshBasicMaterial({ color: new THREE.Color(4, 2.9, 1.7) }), lensM));
    const poolGeo = new THREE.PlaneGeometry(1, 1);
    poolGeo.rotateX(-Math.PI / 2);
    const poolMat = new THREE.MeshBasicMaterial({
      map: glowTexture(), color: new THREE.Color(0.12, 0.085, 0.05), transparent: true, depthWrite: false,
      blending: THREE.AdditiveBlending, polygonOffset: true, polygonOffsetFactor: -4, polygonOffsetUnits: -4,
    });
    if (poolM.length) { const p = instanced(poolGeo, poolMat, poolM); p.renderOrder = 2; this.group.add(p); }
  }

  // ── Traffic signals: flashing amber where the race runs ───────
  buildSignals() {
    const { PX, PZ, HW } = this.G;
    const poleM = [], armM = [], headM = [], lamps = [];
    for (const b of this.blocks.values()) {
      if (b.tier !== 0) continue;
      for (const [di, dj] of [[0, 0], [1, 1]]) {
        const i = b.i + di, j = b.j + dj;
        const use = this.crossUse.get(`${i},${j}`);
        // Two masts per crossing on opposite corners, arms over the road.
        for (const [sx, sz, ax, az] of [[1, 1, 0, -1], [-1, -1, 0, 1], [1, -1, -1, 0], [-1, 1, 1, 0]]) {
          if (this.rng() < 0.5) continue;
          const x = i * PX + sx * (HW + 1.2), z = j * PZ + sz * (HW + 1.2);
          if (!this.onKerb(x, z)) continue;
          const y = this.ground(x, z) + 0.15;
          poleM.push(trs(x, y, z, 0, 1, 6.2, 1));
          const L = HW * 0.9;
          const yaw = -Math.atan2(az, ax);
          armM.push(trs(x + ax * L / 2, y + 6.0, z + az * L / 2, yaw, L, 0.14, 0.14));
          const hx = x + ax * L, hz = z + az * L;
          headM.push(trs(hx, y + 5.1, hz, yaw, 0.4, 1.1, 0.4));
          // Lenses face both ways along the other street.
          for (const f of [-1, 1]) lamps.push({ x: hx + az * f * 0.22, y: y + (use ? 5.1 : 5.45), z: hz - ax * f * 0.22, flash: !!use, phase: Math.abs(ax) > 0 ? 0 : 1 });
        }
      }
    }
    if (!poleM.length) return;
    const metal = new THREE.MeshStandardMaterial({ color: 0x2c2f33, metalness: 0.5, roughness: 0.6 });
    const pole = new THREE.CylinderGeometry(0.11, 0.13, 1, 6);
    pole.translate(0, 0.5, 0);
    const head = new THREE.BoxGeometry(1, 1, 1);
    head.translate(0, 0.5, 0);
    this.group.add(instanced(pole, metal, poleM), instanced(new THREE.BoxGeometry(1, 1, 1), metal, armM), instanced(head, new THREE.MeshStandardMaterial({ color: 0x14161a, roughness: 0.7 }), headM));
    // Lenses as glowing points: amber flashing on the route, red/green elsewhere.
    const pos = new Float32Array(lamps.length * 3), col = new Float32Array(lamps.length * 3);
    lamps.forEach((l, k) => pos.set([l.x, l.y, l.z], k * 3));
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.BufferAttribute(pos, 3));
    g.setAttribute('color', new THREE.BufferAttribute(col, 3));
    g.computeBoundingSphere();
    const m = new THREE.PointsMaterial({ size: 0.9, map: glowTexture(), vertexColors: true, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending });
    const pts = new THREE.Points(g, m);
    this.group.add(pts);
    let time = 0, lastA = -1, lastP = -1;
    this.world.updaters.push((dt) => {
      time += dt;
      const amber = Math.floor(time * 1.4) % 2, phase = Math.floor(time / 9) % 2;
      if (amber === lastA && phase === lastP) return;
      lastA = amber; lastP = phase;
      lamps.forEach((l, k) => {
        let c;
        if (l.flash) c = amber ? [3.4, 1.7, 0.1] : [0.25, 0.12, 0.01];
        else c = l.phase === phase ? [0.2, 3.0, 1.2] : [3.4, 0.25, 0.15];
        col.set(c, k * 3);
      });
      g.attributes.color.needsUpdate = true;
    });
  }

  // ── Race dressing ─────────────────────────────────────────────
  // Barriers wherever the road's wall crosses open street.
  buildBarriers() {
    const t = this.track, { HW } = this.G;
    const f = {};
    const units = [];
    for (const side of [-1, 1]) {
      let run = [];
      const flush = () => {
        if (run.length >= 2) {
          const s0 = run[0], s1 = run[run.length - 1];
          const n = Math.max(1, Math.round((s1 - s0 + 1.6) / 2.05));
          for (let k = 0; k < n; k++) units.push({ s: s0 - 0.8 + ((s1 - s0 + 1.6) * (k + 0.5)) / n, side });
        }
        run = [];
      };
      for (let s = 0; s <= t.length; s += 0.5) {
        t.frame(s, f);
        const lat = side * (f.wallR + 0.6);
        const x = f.x + f.rx * lat, z = f.z + f.rz * lat;
        if (!this.onKerb(x, z)) run.push(s); else flush();
      }
      flush();
    }
    // Across the road at both ends.
    const across = [];
    for (const [s, dir] of [[0.6, 1], [t.length - 0.6, -1]]) {
      t.frame(s, f);
      for (let q = -HW - 0.3; q <= HW + 0.3; q += 2.05) across.push({ x: f.x + f.rx * q - f.fx * dir * 0.3, z: f.z + f.rz * q - f.fz * dir * 0.3, yaw: Math.atan2(f.rz, f.rx) });
    }
    const M = [], colors = [], blink = [];
    const c1 = new THREE.Color(1, 1, 1), c2 = new THREE.Color(1, 0.35, 0.3);
    let k = 0;
    for (const u of units) {
      t.frame(u.s, f);
      const lat = u.side * (f.wallR + 0.32);
      const x = f.x + f.rx * lat, z = f.z + f.rz * lat;
      const yaw = Math.atan2(f.fz, f.fx);
      M.push(trs(x, this.ground(x, z), z, -yaw, 1, 1, 1));
      colors.push(k++ % 2 ? c1 : c2);
      if (k % 4 === 0) blink.push([x, this.ground(x, z) + 1.0, z]);
    }
    for (const a of across) {
      M.push(trs(a.x, this.ground(a.x, a.z), a.z, -a.yaw, 1, 1, 1));
      colors.push(k++ % 2 ? c1 : c2);
      if (k % 3 === 0) blink.push([a.x, this.ground(a.x, a.z) + 1.0, a.z]);
    }
    // Water-filled barrier: tapered block, 2 m long.
    const shape = new THREE.Shape([new THREE.Vector2(-0.3, 0), new THREE.Vector2(0.3, 0), new THREE.Vector2(0.2, 0.82), new THREE.Vector2(-0.2, 0.82)]);
    const geo = new THREE.ExtrudeGeometry(shape, { depth: 2.0, bevelEnabled: false });
    geo.translate(0, 0, -1.0);
    geo.rotateY(Math.PI / 2); // length along local X
    // UVs: wrap the stripe texture along the length.
    const pos = geo.getAttribute('position'), uv = geo.getAttribute('uv');
    for (let q = 0; q < uv.count; q++) uv.setXY(q, (pos.getX(q) + 1) / 2, pos.getY(q) / 0.82);
    geo.computeVertexNormals();
    const mat = new THREE.MeshStandardMaterial({ map: barrierTexture(), roughness: 0.55 });
    const im = instanced(geo, mat, M, { cast: true, receive: true });
    colors.forEach((c, q) => im.setColorAt(q, c));
    this.group.add(im);
    this.barrierCount = M.length;
    // Amber flashers.
    if (blink.length) {
      const g = new THREE.BufferGeometry();
      g.setAttribute('position', new THREE.Float32BufferAttribute(blink.flat(), 3));
      g.computeBoundingSphere();
      const m = new THREE.PointsMaterial({ size: 0.7, map: glowTexture(), color: new THREE.Color(3.5, 1.8, 0.2), transparent: true, depthWrite: false, blending: THREE.AdditiveBlending });
      this.group.add(new THREE.Points(g, m));
      let time = 0;
      this.world.updaters.push((dt) => { time += dt; const on = Math.sin(time * 7) > 0 ? 1 : 0.15; m.color.setRGB(3.5 * on, 1.8 * on, 0.2 * on); });
    }
  }

  // Start and finish gantries over the street.
  buildGantries() {
    const t = this.track;
    const f = {};
    const truss = new THREE.MeshStandardMaterial({ color: 0x2a2c30, metalness: 0.6, roughness: 0.45 });
    const lights = [];
    const gantry = (s, text, checker) => {
      t.frame(s, f);
      const W = f.hw + 1.2, H = 6.4;
      const y = f.y;
      const yaw = Math.atan2(f.fz, f.fx);
      const P = new GeoBuilder({});
      for (const side of [-1, 1]) {
        const x = f.x + f.rx * side * W, z = f.z + f.rz * side * W;
        P.box(x, this.ground(x, z), z, 0.6, H + 1.2, 0.6, yaw);
      }
      P.box(f.x, y + H, f.z, 0.8, 1.4, W * 2 + 0.6, yaw);
      this.group.add(staticMesh(P.build(), truss, { cast: true }));
      // Banner on both faces.
      const tex = bannerTexture(text, { checker, bg: checker ? '#111' : '#0c0c12', fg: checker ? '#fff' : '#ff3c8a' });
      const bm = new THREE.MeshBasicMaterial({ map: tex, color: new THREE.Color(1.3, 1.3, 1.3) });
      const bg = new THREE.PlaneGeometry(W * 2 - 1, 1.3);
      // One face each way: the plane's normal is +Z, turned onto ±(fx, fz).
      for (const dir of [-1, 1]) {
        const mesh = new THREE.Mesh(bg, bm);
        mesh.position.set(f.x + f.fx * dir * 0.45, y + H + 0.02, f.z + f.fz * dir * 0.45);
        mesh.rotation.y = -yaw + Math.PI / 2 + (dir < 0 ? Math.PI : 0);
        this.group.add(mesh);
      }
      for (let q = -W + 0.6; q <= W - 0.6; q += 1.1) lights.push([f.x + f.rx * q, y + H - 0.85, f.z + f.rz * q]);
    };
    gantry(t.startS, 'DOWNTOWN NIGHT RUN', false);
    gantry(t.finishS, 'FINISH', true);
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.Float32BufferAttribute(lights.flat(), 3));
    g.computeBoundingSphere();
    this.group.add(new THREE.Points(g, new THREE.PointsMaterial({ size: 0.8, map: glowTexture(), color: new THREE.Color(3.2, 3, 2.6), transparent: true, depthWrite: false, blending: THREE.AdditiveBlending })));
  }

  // Spectators on the pavements at the start and the finish.
  buildCrowds() {
    const t = this.track, { HW, WALK } = this.G;
    const rng = this.rng;
    const f = {};
    const M = [], cols = [], flashes = [];
    for (const [c, len] of [[t.startS + 20, 150], [t.finishS - 40, 180]]) {
      for (let s = c - len / 2; s < c + len / 2; s += 0.9) {
        t.frame(s, f);
        for (const side of [-1, 1]) {
          for (let row = 0; row < 3; row++) {
            if (rng() < 0.3) continue;
            const lat = side * (HW + 0.9 + row * 0.9 + rng() * 0.5);
            const x = f.x + f.rx * lat + f.fx * (rng() - 0.5) * 0.6, z = f.z + f.rz * lat + f.fz * (rng() - 0.5) * 0.6;
            if (!this.onKerb(x, z) || this.track.distanceToRoad(x, z, 20).d < HW + 0.7) continue;
            const h = 0.85 + rng() * 0.2;
            M.push(trs(x, this.ground(x, z) + 0.15, z, rng() * 6.28, 1, h, 1));
            cols.push(new THREE.Color().setHSL(rng(), 0.25 + rng() * 0.4, 0.12 + rng() * 0.3));
            if (rng() < 0.08) flashes.push([x, this.ground(x, z) + 1.55, z]);
          }
        }
      }
    }
    if (!M.length) return;
    const body = new THREE.CapsuleGeometry(0.19, 0.85, 3, 6);
    body.scale(1.15, 1, 0.8);
    body.translate(0, 0.62, 0);
    const im = instanced(body, new THREE.MeshStandardMaterial({ roughness: 0.9 }), M, { cast: false, receive: true });
    cols.forEach((c, k) => im.setColorAt(k, c));
    const head = new THREE.SphereGeometry(0.13, 8, 6);
    head.translate(0, 1.33, 0);
    const heads = instanced(head, new THREE.MeshStandardMaterial({ roughness: 0.8 }), M);
    const skin = [0x3a2418, 0x6a4028, 0x9a6a48, 0xc89a78, 0x2a1a12];
    M.forEach((_, k) => heads.setColorAt(k, new THREE.Color(skin[k % skin.length])));
    this.group.add(im, heads);
    // Phone cameras twinkling in the crowd.
    const g = new THREE.BufferGeometry();
    const pos = new Float32Array(flashes.flat());
    g.setAttribute('position', new THREE.BufferAttribute(pos, 3));
    g.computeBoundingSphere();
    const m = new THREE.PointsMaterial({ size: 0.35, map: glowTexture(), color: new THREE.Color(3, 3, 3.3), transparent: true, depthWrite: false, blending: THREE.AdditiveBlending });
    const pts = new THREE.Points(g, m);
    pts.userData.dynamic = true;
    this.group.add(pts);
    let time = 0;
    this.world.updaters.push((dt) => { time += dt; m.size = 0.25 + 0.2 * Math.abs(Math.sin(time * 3.1)); });
  }

  // Strings of paper lanterns across the Neon District's streets.
  buildLanterns() {
    const t = this.track, { HW, WALK } = this.G;
    const f = {};
    const pos = [], line = [];
    const rng = this.rng;
    for (let s = 40; s < t.length; s += 16) {
      t.frame(s, f);
      if (f.zone !== 0 || Math.abs(f.kappa) > 0.004) continue;
      if (rng() < 0.35) continue;
      const W = HW + WALK - 0.4;
      const y0 = this.ground(f.x, f.z) + 7.5;
      const pts = [];
      for (let q = 0; q <= 12; q++) {
        const u = q / 12, lat = -W + 2 * W * u;
        const sag = 1.6 * 4 * u * (1 - u);
        pts.push([f.x + f.rx * lat, y0 - sag, f.z + f.rz * lat]);
      }
      for (let q = 0; q < pts.length - 1; q++) line.push(...pts[q], ...pts[q + 1]);
      for (let q = 1; q < pts.length - 1; q++) pos.push(pts[q][0], pts[q][1] - 0.35, pts[q][2]);
    }
    if (!pos.length) return;
    const lg = new THREE.BufferGeometry();
    lg.setAttribute('position', new THREE.Float32BufferAttribute(line, 3));
    this.group.add(new THREE.LineSegments(lg, new THREE.LineBasicMaterial({ color: 0x151515 })));
    const ball = new THREE.SphereGeometry(0.3, 8, 6);
    ball.scale(1, 1.25, 1);
    const M = [];
    for (let k = 0; k < pos.length; k += 3) M.push(trs(pos[k], pos[k + 1], pos[k + 2]));
    const im = instanced(ball, new THREE.MeshBasicMaterial({ color: new THREE.Color(2.6, 0.5, 0.25) }), M);
    const cc = [new THREE.Color(2.6, 0.45, 0.22), new THREE.Color(2.8, 1.5, 0.3), new THREE.Color(2.4, 0.35, 0.6)];
    for (let k = 0; k < M.length; k++) im.setColorAt(k, cc[k % 3]);
    this.group.add(im);
  }

  // The elevated railway over one of the Neon District's avenues, and a
  // train that crosses the race route as you pass under it.
  buildElevated() {
    const t = this.track, { PX, PZ, HW } = this.G;
    const el = t.tag('el')[0];
    if (!el) return;
    const ci = 8;
    const x = ci * PX;
    const j0 = -8, j1 = 12;
    const z0 = j0 * PZ, z1 = j1 * PZ;
    const deckY = (z) => this.ground(x, z) + 8.5;
    const steel = ambientPatch(new THREE.MeshStandardMaterial({ color: 0x4a5044, metalness: 0.55, roughness: 0.55 }), [0.05, 0.05, 0.045], 'steel');
    const B = new GeoBuilder({});
    const braceM = [];
    const step = 15;
    const G = this.bGlow;
    for (let z = z0; z < z1; z += step) {
      const y = deckY(z + step / 2);
      // Plate girders either side (with top and bottom flanges), the deck
      // between, and a parapet rail along each edge.
      // (GeoBuilder.box: sx runs along yaw, so π/2 puts the length on z.)
      for (const sx of [-1, 1]) {
        B.box(x + sx * 3.4, y - 1.2, z + step / 2, step, 1.6, 0.3, Math.PI / 2);
        B.box(x + sx * 3.4, y - 1.25, z + step / 2, step, 0.12, 0.8, Math.PI / 2);
        B.box(x + sx * 3.4, y + 0.28, z + step / 2, step, 0.12, 0.8, Math.PI / 2);
        B.box(x + sx * 3.55, y + 0.4, z + step / 2, step, 0.08, 0.08, Math.PI / 2);
        B.box(x + sx * 3.55, y + 1.1, z + step / 2, step, 0.1, 0.1, Math.PI / 2);
        // Stiffeners down the girder face, and rail posts.
        for (let q = 0; q < step; q += 5) B.box(x + sx * 3.58, y - 1.2, z + q, 0.08, 1.5, 0.12, 0);
      }
      B.box(x, y - 0.35, z + step / 2, step, 0.35, 6.4, Math.PI / 2);
      // Cross-frames under the deck, seen from the street.
      for (let q = 0; q < step; q += 5) B.box(x, y - 0.8, z + q, 6.6, 0.45, 0.25, 0);
      // X-bracing between the girders every bay.
      for (const d of [-1, 1]) braceM.push(trs(x, y - 1.1, z + step / 2, d * Math.atan2(6.4, step), 0.16, 0.16, Math.hypot(step, 6.4)));
      // Lamps under the deck light the road below.
      G.at(x, z).box(x + 2.2, y - 1.05, z + step / 2, 0.5, 0.08, 0.9, 0, { color: [3, 2.6, 1.9] });
      G.at(x, z).box(x - 2.2, y - 1.05, z + step / 2, 0.5, 0.08, 0.9, 0, { color: [3, 2.6, 1.9] });
      if (this.routeDist(x, z + step / 2) < 30) this.spill.push({ x, z: z + step / 2, r: 6, col: [0.3, 0.26, 0.18], tx: 0, tz: 1 });
      // Columns on the pavements every other bay, with a crossbeam, knee
      // braces and a catenary mast above.
      if (Math.round((z - z0) / step) % 2 === 0) {
        const nearCross = Math.abs(z - Math.round(z / PZ) * PZ) < HW + 4;
        if (!nearCross) {
          for (const sx of [-1, 1]) {
            const cx = x + sx * (HW + 1.2);
            const g0 = this.ground(cx, z);
            B.box(cx, g0, z, 0.6, y - 1.2 - g0, 0.6, 0);
            B.box(cx, g0, z, 0.9, 0.5, 0.9, 0);           // footing
            B.box(cx, y - 2.6, z, 0.9, 0.5, 0.9, 0);      // cap
            braceM.push(trs(cx - sx * 0.9, y - 2.4, z, 0, 0.14, 1.9, 0.14, 0, sx * 0.78));
          }
          B.box(x, y - 2.0, z, HW * 2 + 3.2, 0.8, 0.7, 0);
        }
        for (const sx of [-1, 1]) B.box(x + sx * 3.2, y + 0.3, z, 0.18, 5.2, 0.18, 0);
        B.box(x, y + 5.3, z, 6.6, 0.16, 0.16, 0);
      }
      // Catenary wire.
      for (const sx of [-1.5, 1.5]) Props.wire(this, [x + sx, y + 5.0, z], [x + sx, y + 5.0, z + step * 2], 0.25, 4);
    }
    this.group.add(staticMesh(B.build(), steel, { cast: true }));
    if (braceM.length) this.group.add(instanced(new THREE.BoxGeometry(1, 1, 1), steel, braceM));
    // Sleepers and rails.
    const R = new GeoBuilder({});
    for (const rx of [-2.1, -0.9, 0.9, 2.1]) R.box(x + rx, deckY(z0), (z0 + z1) / 2, z1 - z0, 0.15, 0.12, Math.PI / 2);
    this.group.add(staticMesh(R.build(), new THREE.MeshStandardMaterial({ color: 0x8a8a88, metalness: 0.8, roughness: 0.35 })));
    // The train: four cars in one mesh, textured with lit windows,
    // passengers, doors and a cab at each end.
    const tt = trainTexture();
    const body = new THREE.MeshStandardMaterial({ map: tt.map, emissiveMap: tt.e, emissive: new THREE.Color(2.2, 2.0, 1.7), metalness: 0.55, roughness: 0.35 });
    const carL = 15, gap = 0.8;
    const parts = [];
    const region = (g, face, u0, u1) => {
      // BoxGeometry face order: +x, −x, +y, −y, +z, −z (4 vertices each).
      const uv = g.getAttribute('uv');
      for (let v = face * 4; v < face * 4 + 4; v++) uv.setX(v, u0 + uv.getX(v) * (u1 - u0));
    };
    for (let k = 0; k < 4; k++) {
      const c = new THREE.BoxGeometry(2.9, 3.2, carL);
      // The box's ±x faces run their u along z: sides use the side strip.
      region(c, 0, 0, 0.625); region(c, 1, 0, 0.625);
      region(c, 2, 0.875, 1); region(c, 3, 0.875, 1);
      region(c, 4, 0.625, 0.875); region(c, 5, 0.625, 0.875);
      c.translate(0, 1.9, k * (carL + gap));
      parts.push(c);
      for (const bz of [-carL * 0.32, carL * 0.32]) {
        const bg = new THREE.BoxGeometry(2.4, 0.6, 2.6);
        const uv = bg.getAttribute('uv');
        for (let v = 0; v < uv.count; v++) uv.setXY(v, 0.3, 0.02);
        bg.translate(0, 0.35, k * (carL + gap) + bz);
        parts.push(bg);
      }
    }
    const trainGeo = mergeGeometries(parts);
    const train = new THREE.Mesh(trainGeo, body);
    train.castShadow = true;
    train.userData.dynamic = true;
    train.position.set(x - 1.5, 0, z0);
    this.group.add(train);
    const len = 4 * (carL + gap);
    // Where the route passes under the viaduct.
    let sCross = el.s0, bd = Infinity;
    for (let s = el.s0; s < el.s1; s++) { const d = Math.abs(t.px[s] - x); if (d < bd) { bd = d; sCross = s; } }
    const zc = t.pz[sCross];
    let zt = z0, free = true;
    this.world.updaters.push((dt, night, camera, s) => {
      // Scripted near the crossing: the train meets the player overhead.
      const ds = (s ?? 0) - sCross;
      if (ds > -380 && ds < 160) {
        zt = zc - len * 0.55 + ds * 0.85;
        free = false;
      } else {
        if (!free) free = true;
        zt += 16 * dt;
        if (zt > z1) zt = z0 - len;
      }
      train.position.z = zt;
      train.position.y = deckY(Math.min(z1, Math.max(z0, zt + len / 2))) - 0.2;
    });
  }

  // Wet road: soft coloured streaks on the asphalt under signs and lamps.
  buildReflections() {
    const t = this.track, { HW } = this.G;
    const pos = [], uv = [], col = [];
    const tmp = new THREE.Color();
    for (const L of this.signLights) {
      const r = t.distanceToRoad(L.x, L.z, 30);
      if (r.i < 0 || r.d > HW + 9) continue;
      const f = t.frame(r.s);
      const side = r.lat >= 0 ? 1 : -1;
      tmp.set(L.col);
      const k = L.lamp ? 0.12 : L.big ? 0.12 : 0.3;
      const c = [tmp.r * k, tmp.g * k, tmp.b * k];
      // A streak along the road under the light, as a wet surface smears it
      // toward the viewer.
      const lat = side * (HW - (L.lamp ? 2.6 : 1.6));
      const hw = L.lamp ? 1.0 : 0.8, len = L.lamp ? 4.5 : 6;
      const cx = f.x + f.rx * lat, cz = f.z + f.rz * lat;
      const P = [[cx - f.fx * len - f.rx * hw, cz - f.fz * len - f.rz * hw], [cx + f.fx * len - f.rx * hw, cz + f.fz * len - f.rz * hw],
        [cx + f.fx * len + f.rx * hw, cz + f.fz * len + f.rz * hw], [cx - f.fx * len + f.rx * hw, cz - f.fz * len + f.rz * hw]];
      const U = [[0, 0], [0, 1], [1, 1], [1, 0]];
      const Y = [r.s - len, r.s + len, r.s + len, r.s - len].map((ss) => t.surfaceY(ss, 0) + 0.035);
      const order = (P[1][1] - P[0][1]) * (P[2][0] - P[0][0]) - (P[1][0] - P[0][0]) * (P[2][1] - P[0][1]) >= 0 ? [0, 1, 2, 0, 2, 3] : [0, 2, 1, 0, 3, 2];
      for (const q of order) { pos.push(P[q][0], Y[q], P[q][1]); uv.push(...U[q]); col.push(...c); }
    }
    if (!pos.length) return;
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
    g.setAttribute('uv', new THREE.Float32BufferAttribute(uv, 2));
    g.setAttribute('color', new THREE.Float32BufferAttribute(col, 3));
    g.computeBoundingSphere();
    const m = new THREE.MeshBasicMaterial({ map: glowTexture(), vertexColors: true, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending, polygonOffset: true, polygonOffsetFactor: -4, polygonOffsetUnits: -4 });
    const mesh = staticMesh(g, m, { receive: false });
    mesh.renderOrder = 2;
    this.group.add(mesh);
  }
}
