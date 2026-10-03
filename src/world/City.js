import * as THREE from 'three';
import { GeoBuilder, staticMesh, instanced, trs } from './city/geom.js';
import { cityFacadeAtlas, patchCityMaterial, cellSeed, adTexture, CELL_TILE, CELL_COLS, CELL_ROWS, ROOF_CELL, GLASS_CELL, WAREHOUSE_CELL, BRICK_CELL } from './city/cityTextures.js';
import { Freeway, makePath, sweep, atlasQuads, OPP_C, OPP_LANES, oppY } from './city/freeway.js';
import { glowTexture } from './textures.js';
import { mulberry32, clamp, lerp, smoothstep } from '../util/math.js';

// Meridian: a street grid laid out along the Interstate. On Level 1 it is
// the last zone — mid-rise outskirts climbing to a skyscraper core around the
// finish. On the endless cruise the whole level is a freeway loop through the
// city, with districts strung round it: two downtown cores, an industrial
// belt under the viaducts, a waterfront, brick residential and a midtown.
// The freeway furniture itself (westbound lanes, lights, gantries, tunnels,
// overpasses, finish/welcome gantry) lives in city/freeway.js.

const BACK_EXT = 700;   // westbound lanes carry on behind the on-ramp merge
const FWD_EXT = 900;    // and both directions carry on past the finish
const PU = 110, PV = 90, STREET = 14;

const NEON = [
  [3.2, 0.25, 2.6], [0.3, 2.6, 3.2], [3.4, 0.5, 1.0], [3.2, 1.6, 0.3], [0.5, 3.2, 1.2], [1.8, 0.5, 3.4], [3.0, 3.0, 3.2],
];

const tick = () => new Promise((r) => setTimeout(r, 0));

export default class City {
  label = 'Building the city';

  // World constructs scenery as new City({ zone, key, level }).
  constructor(opts = {}) {
    this.zone = opts.zone ?? 2;
    this.key = opts.key;
    this.level = opts.level;
  }

  plan(world) {
    const t = world.track;
    this.loop = !!t.loop;
    const Z = t.zones[this.zone];
    if (this.loop) {
      this.sA = 0;
      this.path = makePath(t, 0, t.length, 0, 0);
      this.planLoop(world);
      return;
    }
    this.sA = t.tag('merge')[0]?.s0 ?? Z.s0 + 220;
    this.path = makePath(t, this.sA, t.length, BACK_EXT, FWD_EXT);
    t.runout = Math.max(t.runout, FWD_EXT); // drivable after the finish
    // Level ground under the westbound lanes behind the merge, where the
    // terrain doesn't know about them.
    const f = {};
    for (let u = this.sA - BACK_EXT; u < this.sA - 15; u += 30) {
      this.path.frame(u, f);
      world.terrain.addFlatten(f.x + f.rx * OPP_C, f.z + f.rz * OPP_C, 16, 45, f.y - 0.04);
    }
  }

  // Loop: level the middle of the ring (the city landform raises hills far
  // from the road) and dig the waterfront basin outside it.
  planLoop(world) {
    const t = world.track, T = world.terrain;
    let cx = 0, cz = 0;
    for (let i = 0; i < t.n; i += 10) { cx += t.px[i]; cz += t.pz[i]; }
    cx /= Math.ceil(t.n / 10); cz /= Math.ceil(t.n / 10);
    this.centre = { x: cx, z: cz };
    // Which side of the road is the inside of the ring (+1 = right).
    let votes = 0;
    for (let i = 0; i < t.n; i += 97) votes += Math.sign((cx - t.px[i]) * t.rx[i] + (cz - t.pz[i]) * t.rz[i]);
    this.insideSign = votes >= 0 ? 1 : -1;
    const g = (T.flatY[this.zone] ?? T.cityY) - 0.25;
    T.addFlatten(cx, cz, 1350, 260, g);
    const L = t.length;
    const wf = { s0: Math.round(0.405 * L), s1: Math.round(0.495 * L), lat: -this.insideSign * 230, width: 170, depth: 6 };
    const pts = [];
    const f = {};
    for (let s = wf.s0 - 150; s <= wf.s1 + 150; s += 30) {
      t.frame(s, f);
      pts.push({ x: f.x + f.rx * wf.lat, z: f.z + f.rz * wf.lat });
    }
    T.addCarve(pts, wf.width, wf.depth);
    this.waterfront = wf;
  }

  setupGrid(t) {
    let ang, ox, oz;
    if (this.loop) {
      // One grid for the whole ring; the freeway crosses it at every angle.
      ang = 0.3;
      ox = this.centre.x; oz = this.centre.z;
    } else {
      const fa = t.frame(this.sA, {}), fe = t.frame(t.length - 1, {});
      ang = Math.atan2(fe.z - fa.z, fe.x - fa.x);
      const o = t.frame(t.zones[this.zone].s0 + 1400, {});
      ox = o.x; oz = o.z;
    }
    const c = Math.cos(ang), s = Math.sin(ang);
    this.grid = {
      pu: PU, pv: PV, street: STREET,
      uAxis: [c, s], vAxis: [-s, c],
      toU: (x, z) => (x - ox) * c + (z - oz) * s,
      toV: (x, z) => -(x - ox) * s + (z - oz) * c,
      toWorld: (u, v) => [ox + u * c - v * s, oz + u * s + v * c],
    };
  }

  // Signed lateral offsets of (x,z) from every nearby bit of freeway.
  freewayHits(x, z) {
    const t = this.track, out = [];
    // The far field is coarse (32 m) — only ask the track when plausibly close.
    if (this.T.far(x, z, this._farTmp).d < 175) {
      const r = t.distanceToRoad(x, z, 120);
      if (r.d < 1e8 && (this.loop || r.s >= t.zones[this.zone].s0 - 60)) out.push({ lat: r.lat, s: r.s, y: t.py[t.idx(r.s)] });
    }
    if (this.loop) return out;
    const f = this._fa, e = this._fe;
    const du = (x - f.x) * f.fx + (z - f.z) * f.fz;
    if (du <= 0 && du >= -BACK_EXT - 30) out.push({ lat: (x - f.x) * f.rx + (z - f.z) * f.rz, s: -1, y: f.y, back: true });
    const de = (x - e.x) * e.fx + (z - e.z) * e.fz;
    if (de >= 0 && de <= FWD_EXT + 30) out.push({ lat: (x - e.x) * e.rx + (z - e.z) * e.rz, s: -2, y: e.y });
    return out;
  }

  // Would a building here crowd the freeway? (20 m+ setback past barriers.)
  buildBlocked(x, z) {
    for (const h of this.freewayHits(x, z)) {
      if (h.back ? (h.lat > -56 && h.lat < -4) : (h.lat > -54 && h.lat < 31)) return true;
    }
    return false;
  }

  // Would a street here run through the freeway at grade?
  streetBlocked(x, z) {
    for (const h of this.freewayHits(x, z)) {
      const lo = h.back ? -37 : -37, hi = h.back ? -6 : 15.5;
      if (h.lat > lo && h.lat < hi && h.y < this.flatY + 3.5) return true;
      if (h.lat > lo && h.lat < hi && Math.abs(h.lat - (-10.95)) < 1.5) return true; // median piers
    }
    return false;
  }

  // Street lamps also keep out from under low viaduct ramps (loop).
  lampBlocked(x, z) {
    if (this.streetBlocked(x, z)) return true;
    if (!this.loop) return false;
    for (const h of this.freewayHits(x, z)) if (h.lat > -37 && h.lat < 15.5 && h.y < this.flatY + 10.5) return true;
    return false;
  }

  inCity(x, z) {
    const T = this.T;
    if (T.zoneWeights(x)[this.zone] < 0.9) return false;
    const F = T.far(x, z);
    if (this.loop) return F.d < 950 || this.sideAt(x, z) > 0;
    if (F.d > 1250) return false;
    if (F.s < this.track.zones[this.zone].s0 - 30 && F.d < 600) return false;
    return true;
  }

  // Nearest-cell far-field lookups (no interpolation, so s doesn't smear
  // across the loop's seam). side: +1 inside the ring, -1 outside.
  nearestS(x, z) {
    const T = this.T;
    const i = clamp(Math.round((x - T.minX) / 32), 0, T.fw - 1), j = clamp(Math.round((z - T.minZ) / 32), 0, T.fh - 1);
    return T.fs[j * T.fw + i];
  }
  sideAt(x, z) {
    const T = this.T;
    const i = clamp(Math.round((x - T.minX) / 32), 0, T.fw - 1), j = clamp(Math.round((z - T.minZ) / 32), 0, T.fh - 1);
    return (T.fl[j * T.fw + i] >= 0 ? 1 : -1) * (this.insideSign || 1);
  }

  // Loop districts by position along the ring.
  districtAt(s) {
    const t = this.track, L = t.length;
    const within = (a, b) => { const d = t.ds(a, s), w = t.ds(a, b); return d >= 0 && d <= w; };
    if (t.tag('downtown').some((g) => within(g.s0 - 260, g.s1 + 260))) return 'downtown';
    if (['viaduct', 'viaduct-up', 'viaduct-down'].some((n) => t.tag(n).some((g) => within(g.s0 - 120, g.s1 + 120)))) return 'industrial';
    const f = t.wrap(s) / L;
    if (f > 0.405 && f < 0.495) return 'waterfront';
    if (f > 0.86 || f < 0.07) return 'midrise';
    return 'residential';
  }

  flat(x, z) {
    return Math.abs(this.T.heightAt(x, z) - this.groundY) < 0.3;
  }

  async build(world) {
    const t0 = performance.now();
    const t = world.track, T = world.terrain;
    this.world = world; this.track = t; this.T = T;
    this.flatY = T.flatY?.[this.zone] ?? T.cityY;
    this.groundY = this.flatY - 0.25;
    this._farTmp = {};
    if (!this.loop) {
      this._fa = t.frame(this.sA, {});
      this._fe = t.frame(t.length - 0.01, {});
    }
    this.setupGrid(t);
    this.group = new THREE.Group();
    this.group.name = 'city';
    world.scene.add(this.group);
    const rng = mulberry32(4242);
    this.rng = rng;
    const ctx = {
      world, track: t, T, cityY: this.flatY, zone: this.zone, path: this.path, group: this.group, mats: {},
      rng, grid: this.grid, updaters: world.updaters,
    };
    if (this.loop) ctx.soundWallSpans = this.loopSoundWalls();
    // Skyscraper cores (also decide where lamps are white LED).
    this.DC = this.loop ? null : t.frame(t.finishS - 440, {});
    this.cores = this.loop
      ? t.tag('downtown').map((g) => { const c = t.frame((g.s0 + g.s1) / 2, {}); return { x: c.x, z: c.z, R: Math.max(450, (g.s1 - g.s0) * 0.42) }; })
      : [];
    const lf = {};
    ctx.lampTint = (u) => { this.path.frame(u, lf); return this.ledAt(lf.x, lf.z) ? [0.75, 0.86, 1.05] : [1.0, 0.72, 0.4]; };
    this.fw = new Freeway(ctx);
    this.fw.build();
    // For traffic: the westbound carriageway (lat relative to our centreline).
    // Level 1: from the merge to the end. Loop: all the way round.
    const tmp = {};
    world.oppositeCarriageway = {
      s0: this.sA, s1: t.length, lanes: OPP_LANES.slice(), dir: -1,
      y: (s) => oppY(t.frame(s, tmp)),
    };
    await tick();
    this.buildBlocks();
    await tick();
    if (this.loop) this.buildWaterfront();
    this.buildVerge();
    this.buildLamps();
    this.buildTrees();
    this.buildParkedCars();
    this.buildAircraftLights();
    this.buildGlows();
    this.buildTraffic();
    this.buildSkyGlow();
    this.group.traverse((o) => { if (o.isMesh || o.isPoints) { o.matrixAutoUpdate = false; o.updateMatrix(); } });
    console.info(`[City] ${this.stats.buildings} buildings (${this.stats.towers} towers), ${this.glows.length} lamps, ${this.parked.length} parked cars, ${(performance.now() - t0).toFixed(0)} ms`);
  }

  loopSoundWalls() {
    const t = this.track, spans = [];
    let st = null;
    for (let s = 0; s <= t.length; s += 20) {
      const ok = this.districtAt(s) === 'residential';
      if (ok && st === null) st = s;
      if (!ok && st !== null) { if (s - st > 150) spans.push([st, s]); st = null; }
    }
    if (st !== null && t.length - st > 150) spans.push([st, t.length]);
    return spans;
  }

  // ── Blocks, lots and buildings ────────────────────────────────
  buildBlocks() {
    const t = this.track, G = this.grid, rng = this.rng;
    const g0 = this.groundY;
    // Skyscraper cores: around the finish on Level 1, round each 'downtown'
    // stretch on the loop.
    const DC = this.loop ? null : t.frame(t.finishS - 440, {});
    this.DC = DC;
    this.cores = this.loop
      ? t.tag('downtown').map((g) => { const c = t.frame((g.s0 + g.s1) / 2, {}); return { x: c.x, z: c.z, R: Math.max(450, (g.s1 - g.s0) * 0.42) }; })
      : [];
    // Grid extent from the freeway path.
    let umin = Infinity, umax = -Infinity, vmin = Infinity, vmax = -Infinity;
    const f = {};
    for (let u = this.path.u0; u <= this.path.u1; u += 40) {
      this.path.frame(u, f);
      const gu = G.toU(f.x, f.z), gv = G.toV(f.x, f.z);
      umin = Math.min(umin, gu); umax = Math.max(umax, gu);
      vmin = Math.min(vmin, gv); vmax = Math.max(vmax, gv);
    }
    const i0 = Math.floor((umin - 1300) / PU), i1 = Math.ceil((umax + 1300) / PU);
    const j0 = Math.floor((vmin - 1400) / PV), j1 = Math.ceil((vmax + 1400) / PV);

    const atlas = cityFacadeAtlas();
    const mat = patchCityMaterial(new THREE.MeshStandardMaterial({
      map: atlas.map, emissiveMap: atlas.emissive, emissive: 0xffffff, emissiveIntensity: 0.1,
      roughness: 0.72, metalness: 0.15,
    }), { mask: atlas.mask, ground: g0 + 0.15 });
    this.world.addNight(mat, 'emissiveIntensity', 0.06, 1.0);
    this.buildingMat = mat;

    this.chunks = new Map();
    const CH = this.loop ? 2000 : 600;
    const chunkOf = (x, z) => {
      const key = Math.floor(x / CH) + ',' + Math.floor(z / CH);
      let b = this.chunks.get(key);
      if (!b) this.chunks.set(key, (b = new GeoBuilder({ cell: true })));
      return b;
    };
    this.chunkOf = chunkOf;
    // Level 1: one ground mesh. Loop: the ring is ~8 × 6 km, so chunk it.
    const groundB = new ChunkedGeo(this.loop ? 2000 : Infinity, { color: true });
    this.neon = new GeoBuilder({ color: true });
    this.lampSpots = [];
    this.parkTrees = [];
    this.aircraft = [];
    this.padLights = [];
    this.streetCells = new Set();
    this.parked = [];
    this.sites = 0;
    this.glows = []; // [x, y, z, r, g, b, size]
    this.streetSegs = [];
    this.roofAds = [];
    this.stats = { blocks: 0, buildings: 0, towers: 0 };

    const W = (u, v) => G.toWorld(u, v);
    const P3 = (u, v, y) => { const p = W(u, v); return [p[0], y, p[1]]; };
    const ASPH = [0.07, 0.07, 0.075], WALK = [0.38, 0.37, 0.35], GRASS = [0.12, 0.24, 0.07], PLAZA = [0.46, 0.43, 0.38];
    const quadUV = (u0, v0, u1, v1, y, col) => {
      // Up-facing quad in grid space.
      const a = P3(u0, v0, y), b = P3(u1, v0, y), c = P3(u1, v1, y), d = P3(u0, v1, y);
      const n = normalY(a, b, c);
      if (n >= 0) groundB.quad(a, b, c, d, null, col); else groundB.quad(a, d, c, b, null, col);
    };
    const pad = (u0, v0, u1, v1, col, h = 0.15) => {
      quadUV(u0, v0, u1, v1, g0 + h, col);
      // Curb faces.
      const pts = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]].map(([u, v]) => W(u, v));
      groundB.prism(pts, g0 - 0.2, g0 + h, { roof: false, color: [col[0] * 0.8, col[1] * 0.8, col[2] * 0.8] });
    };

    for (let i = i0; i <= i1; i++) {
      for (let j = j0; j <= j1; j++) {
        const cu = (i + 0.5) * PU, cv = (j + 0.5) * PV;
        const cw = W(cu, cv);
        if (!this.inCity(cw[0], cw[1])) continue;
        let district = null, cellS = 0, inside = true, farD = 0;
        if (this.loop) {
          farD = this.T.far(cw[0], cw[1]).d;
          inside = this.sideAt(cw[0], cw[1]) > 0;
          cellS = this.nearestS(cw[0], cw[1]);
          district = farD > 700 && inside ? 'interior' : this.districtAt(cellS);
          // The promenade and basin own the waterfront's outer side.
          if (district === 'waterfront' && !inside && farD < 430) continue;
        }
        const corners = [[i * PU, j * PV], [(i + 1) * PU, j * PV], [(i + 1) * PU, (j + 1) * PV], [i * PU, (j + 1) * PV]];
        const pts = corners.map(([u, v]) => W(u, v)).concat([cw]);
        if (!pts.every((p) => this.flat(p[0], p[1]))) continue;
        // Streets: the whole cell in asphalt unless the freeway runs through.
        const streetOK = pts.every((p) => !this.streetBlocked(p[0], p[1])) &&
          [[0.25, 0.5], [0.75, 0.5], [0.5, 0.25], [0.5, 0.75]].every(([a, b]) => { const p = W((i + a) * PU, (j + b) * PV); return !this.streetBlocked(p[0], p[1]); });
        const bu0 = i * PU + STREET / 2, bu1 = (i + 1) * PU - STREET / 2;
        const bv0 = j * PV + STREET / 2, bv1 = (j + 1) * PV - STREET / 2;
        if (streetOK) {
          this.streetCells.add(i + ',' + j);
          quadUV(i * PU, j * PV, (i + 1) * PU, (j + 1) * PV, g0 + 0.06, ASPH);
          // Lamps on this cell's +V and +U street edges.
          for (const a of [0.28, 0.72]) this.lampSpots.push({ u: lerp(bu0, bu1, a), v: bv1 + 1.2, du: 0, dv: 1 });
          this.lampSpots.push({ u: bu1 + 1.2, v: lerp(bv0, bv1, 0.5), du: 1, dv: 0 });
          // Street centre lines on the same two edges, for the traffic lights.
          this.streetSegs.push([i * PU, (j + 1) * PV, (i + 1) * PU, (j + 1) * PV], [(i + 1) * PU, j * PV, (i + 1) * PU, (j + 1) * PV]);
        }
        const blockPts = [[bu0, bv0], [bu1, bv0], [bu1, bv1], [bu0, bv1], [(bu0 + bu1) / 2, (bv0 + bv1) / 2], [(bu0 + bu1) / 2, bv0], [(bu0 + bu1) / 2, bv1], [bu0, (bv0 + bv1) / 2], [bu1, (bv0 + bv1) / 2]]
          .map(([u, v]) => W(u, v));
        const blockClear = blockPts.every((p) => !this.buildBlocked(p[0], p[1]));
        if (this.loop) {
          this.stats.blocks++;
          this.loopBlock({ i, j, bu0, bu1, bv0, bv1, cw, district, inside, farD, blockClear, pad, quadUV, W, groundB });
          continue;
        }
        const r = Math.hypot(cw[0] - DC.x, cw[1] - DC.z);
        const core = Math.exp(-Math.pow(r / 640, 2));
        const ring = Math.exp(-Math.pow(r / 1500, 2));
        const roll = rng();
        this.stats.blocks++;
        if (blockClear && roll < 0.05 + core * 0.05) {
          // Park (or a plaza downtown).
          const plaza = core > 0.5;
          pad(bu0, bv0, bu1, bv1, plaza ? PLAZA : GRASS, 0.2);
          if (!plaza) {
            quadUV((bu0 + bu1) / 2 - 2, bv0, (bu0 + bu1) / 2 + 2, bv1, g0 + 0.23, PLAZA);
            quadUV(bu0, (bv0 + bv1) / 2 - 2, bu1, (bv0 + bv1) / 2 + 2, g0 + 0.23, PLAZA);
          }
          const n = plaza ? 8 : 26;
          for (let k = 0; k < n; k++) {
            const u = lerp(bu0 + 4, bu1 - 4, rng()), v = lerp(bv0 + 4, bv1 - 4, rng());
            if (!plaza && (Math.abs(u - (bu0 + bu1) / 2) < 4 || Math.abs(v - (bv0 + bv1) / 2) < 4)) continue;
            const p = W(u, v);
            this.parkTrees.push({ x: p[0], y: g0 + 0.2, z: p[1], s: 0.8 + rng() * 0.7 });
          }
          if (plaza) {
            const p = W((bu0 + bu1) / 2, (bv0 + bv1) / 2);
            // Fountain basin + glowing jet.
            this.chunkOf(p[0], p[1]).prism(circle(p[0], p[1], 7, 12), g0, g0 + 0.8, { cell: GLASS_CELL, tileW: 8, tileH: 8, roofCell: ROOF_CELL });
            this.neon.prism(circle(p[0], p[1], 0.6, 8), g0 + 0.8, g0 + 4.5, { color: [0.6, 1.4, 2.2], roofColor: [0.9, 1.8, 2.6] });
          }
          continue;
        }
        const tower = core > 0.25 && roll < 0.35 + core * 0.55;
        let lots;
        const BU = bu1 - bu0, BV = bv1 - bv0;
        if (tower) lots = rng() < 0.55 ? [[0, 0, 1, 1]] : [[0, 0, 0.5, 1], [0.5, 0, 1, 1]];
        else if (ring > 0.55 || rng() < 0.4) lots = [[0, 0, 0.5, 0.5], [0.5, 0, 1, 0.5], [0, 0.5, 0.5, 1], [0.5, 0.5, 1, 1]];
        else lots = [[0, 0, 1 / 3, 0.5], [1 / 3, 0, 2 / 3, 0.5], [2 / 3, 0, 1, 0.5], [0, 0.5, 1 / 3, 1], [1 / 3, 0.5, 2 / 3, 1], [2 / 3, 0.5, 1, 1]];
        if (blockClear) pad(bu0, bv0, bu1, bv1, WALK);
        for (const [a0, b0, a1, b1] of lots) {
          const lu0 = bu0 + BU * a0 + 2.5, lu1 = bu0 + BU * a1 - 2.5;
          const lv0 = bv0 + BV * b0 + 2.5, lv1 = bv0 + BV * b1 - 2.5;
          const lotPts = [[lu0, lv0], [lu1, lv0], [lu1, lv1], [lu0, lv1], [(lu0 + lu1) / 2, (lv0 + lv1) / 2]].map(([u, v]) => W(u, v));
          if (!blockClear) {
            if (!lotPts.every((p) => !this.buildBlocked(p[0], p[1]))) { this.parkingLot(lu0, lv0, lu1, lv1, pad); continue; }
            pad(lu0 - 2.5, lv0 - 2.5, lu1 + 2.5, lv1 + 2.5, WALK);
          }
          if (rng() < 0.06 && !tower) continue; // empty lot
          // Now and then a tower still going up, with its crane.
          if (tower && this.sites < 5 && rng() < 0.07) { this.site(lu0, lv0, lu1, lv1, core); continue; }
          this.building(lu0, lv0, lu1, lv1, { tower, core, ring, r });
        }
      }
    }

    // Emit building chunks.
    for (const b of this.chunks.values()) {
      if (b.empty) continue;
      this.group.add(staticMesh(b.build(), mat, { receive: true }));
    }
    const gm = new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.93, polygonOffset: true, polygonOffsetFactor: -2, polygonOffsetUnits: -4 });
    for (const b of groundB.builders()) {
      const m = staticMesh(b.build(), gm, { receive: true });
      this.group.add(m);
      if (this.loop) this.fadeable(m, 2600);
    }
    if (this.roofAds.length) {
      const ads = atlasQuads(this.roofAds, 1024, 384, 3, (m) => {
        m.roughness = 0.6;
        m.emissiveIntensity = 0.8;
        this.world.addNight(m, 'emissiveIntensity', 0.25, 1.2);
      });
      this.group.add(staticMesh(ads.geo, ads.material));
    }
    const nm = new THREE.MeshBasicMaterial({ vertexColors: true });
    this.world.updaters.push((dt, night) => nm.color.setScalar(0.3 + 0.7 * smoothstep(0.2, 0.8, night)));
    if (!this.neon.empty) this.group.add(staticMesh(this.neon.build(), nm, { receive: false }));
  }

  // ── Loop districts ────────────────────────────────────────────
  loopBlock(o) {
    const { bu0, bu1, bv0, bv1, cw, district, inside, blockClear, pad, quadUV, W, groundB } = o;
    const rng = this.rng, g0 = this.groundY, G = this.grid;
    const WALK = [0.38, 0.37, 0.35], GRASS = [0.12, 0.24, 0.07], PLAZA = [0.46, 0.43, 0.38], YARD = [0.1, 0.1, 0.11];
    const BU = bu1 - bu0, BV = bv1 - bv0;
    let core = 0;
    for (const c of this.cores) core = Math.max(core, Math.exp(-((Math.pow(cw[0] - c.x, 2) + Math.pow(cw[1] - c.z, 2)) / (c.R * c.R))));
    const dc = Math.hypot(cw[0] - this.centre.x, cw[1] - this.centre.z);
    const roll = rng();
    const tree = (u, v, sc = 1) => { const p = W(u, v); this.parkTrees.push({ x: p[0], y: g0 + 0.2, z: p[1], s: sc * (0.8 + rng() * 0.6) }); };
    const makePark = (plaza, lamps = false) => {
      pad(bu0, bv0, bu1, bv1, plaza ? PLAZA : GRASS, 0.2);
      const mu = (bu0 + bu1) / 2, mv = (bv0 + bv1) / 2;
      if (!plaza) {
        quadUV(mu - 2, bv0, mu + 2, bv1, g0 + 0.23, PLAZA);
        quadUV(bu0, mv - 2, bu1, mv + 2, g0 + 0.23, PLAZA);
      }
      const n = plaza ? 8 : 24;
      for (let k = 0; k < n; k++) {
        const u = lerp(bu0 + 4, bu1 - 4, rng()), v = lerp(bv0 + 4, bv1 - 4, rng());
        if (!plaza && (Math.abs(u - mu) < 4 || Math.abs(v - mv) < 4)) continue;
        tree(u, v);
      }
      if (lamps) for (const [du, dv] of [[-20, 3], [20, -3], [3, -18], [-3, 18]]) {
        const p = W(mu + du, mv + dv);
        (this.extraLamps ||= []).push({ x: p[0], y: g0 + 0.2, z: p[1], dx: 1, dz: 0, h: 4.2, arm: 0 });
      }
      if (plaza) {
        const p = W(mu, mv);
        this.chunkOf(p[0], p[1]).prism(circle(p[0], p[1], 7, 12), g0, g0 + 0.8, { cell: GLASS_CELL, tileW: 8, tileH: 8, roofCell: ROOF_CELL });
        this.neon.prism(circle(p[0], p[1], 0.6, 8), g0 + 0.8, g0 + 4.5, { color: [0.6, 1.4, 2.2], roofColor: [0.9, 1.8, 2.6] });
      }
    };
    // Run fn over lots; lots that crowd the freeway are dropped (with their
    // own pads when the block as a whole isn't clear).
    const lotsDo = (lots, fn, padCol = WALK) => {
      if (blockClear) pad(bu0, bv0, bu1, bv1, padCol);
      for (const [a0, b0, a1, b1] of lots) {
        const lu0 = bu0 + BU * a0 + 2.5, lu1 = bu0 + BU * a1 - 2.5;
        const lv0 = bv0 + BV * b0 + 2.5, lv1 = bv0 + BV * b1 - 2.5;
        const lotPts = [[lu0, lv0], [lu1, lv0], [lu1, lv1], [lu0, lv1], [(lu0 + lu1) / 2, (lv0 + lv1) / 2]].map(([u, v]) => W(u, v));
        if (!blockClear) {
          if (!lotPts.every((p) => !this.buildBlocked(p[0], p[1]))) { this.parkingLot(lu0, lv0, lu1, lv1, pad); continue; }
          pad(lu0 - 2.5, lv0 - 2.5, lu1 + 2.5, lv1 + 2.5, padCol);
        }
        fn(lu0, lv0, lu1, lv1);
      }
    };
    const Q4 = [[0, 0, 0.5, 0.5], [0.5, 0, 1, 0.5], [0, 0.5, 0.5, 1], [0.5, 0.5, 1, 1]];
    const Q6 = [[0, 0, 1 / 3, 0.5], [1 / 3, 0, 2 / 3, 0.5], [2 / 3, 0, 1, 0.5], [0, 0.5, 1 / 3, 1], [1 / 3, 0.5, 2 / 3, 1], [2 / 3, 0.5, 1, 1]];
    const Q2 = [[0, 0, 0.5, 1], [0.5, 0, 1, 1]];
    const streetTrees = () => {
      if (!blockClear) return;
      if (rng() < 0.4) return;
      for (let u = bu0 + 6; u < bu1 - 4; u += 18) tree(u, bv1 - 1.6, 0.8);
    };

    // Meridian Park fills the middle of the ring.
    if (district === 'interior' && dc < 330) {
      if (blockClear) makePark(dc < 60, true);
      return;
    }
    if (district === 'downtown' || (district === 'interior' && core > 0.3)) {
      if (blockClear && roll < 0.05 + core * 0.05) { makePark(core > 0.5); return; }
      const tower = core > 0.2 && roll < 0.35 + core * 0.55;
      const lots = tower ? (rng() < 0.55 ? [[0, 0, 1, 1]] : Q2) : Q4;
      lotsDo(lots, (a, b, c, d) => {
        if (!tower && rng() < 0.06) return;
        if (tower && this.sites < 8 && rng() < 0.06) { this.site(a, b, c, d, core); return; }
        this.building(a, b, c, d, { tower, core: Math.max(core, 0.35), ring: 1, r: 0 });
      });
      return;
    }
    if (district === 'industrial') {
      if (roll < 0.2) { this.containerYard(o, core); return; }
      const lots = rng() < 0.5 ? [[0, 0, 1, 1]] : Q2;
      lotsDo(lots, (a, b, c, d) => this.warehouse(a, b, c, d), YARD);
      return;
    }
    if (district === 'midrise') {
      if (blockClear && roll < 0.06) { makePark(false, true); return; }
      lotsDo(Q4, (a, b, c, d) => {
        if (rng() < 0.05) return;
        const brick = rng() < 0.25;
        this.building(a, b, c, d, { ring: 1, r: 0, cell: brick ? BRICK_CELL : undefined, floors: brick ? [5, 10] : [7, 22] });
      });
      return;
    }
    // Residential (and the waterfront's inner side / far shore, and the
    // rest of the interior): brick walk-ups, low-rise and parks.
    if (blockClear && roll < (district === 'waterfront' ? 0.14 : 0.08)) { makePark(false, true); return; }
    lotsDo(rng() < 0.5 ? Q6 : Q4, (a, b, c, d) => {
      if (rng() < 0.05) return;
      const k = rng();
      if (k < 0.55) this.building(a, b, c, d, { ring: 0, r: 0, cell: BRICK_CELL, floors: [3, 7] });
      else if (k < 0.85) this.building(a, b, c, d, { ring: 0, r: 0 });
      else this.building(a, b, c, d, { ring: 1, r: 0, floors: [6, 12] });
    });
    streetTrees();
  }

  // Warehouse with a flat roof, maybe a chimney or tanks beside it.
  warehouse(lu0, lv0, lu1, lv1) {
    const rng = this.rng, G = this.grid, g0 = this.groundY + 0.15;
    const W = (u, v) => G.toWorld(u, v);
    const cu = (lu0 + lu1) / 2, cv = (lv0 + lv1) / 2;
    const cw = W(cu, cv);
    const B = this.chunkOf(cw[0], cw[1]);
    const wu = (lu1 - lu0) - 3 - rng() * 6, wv = (lv1 - lv0) - 3 - rng() * 6;
    const H = 7 + rng() * 8;
    const rect = [[cu - wu / 2, cv - wv / 2], [cu + wu / 2, cv - wv / 2], [cu + wu / 2, cv + wv / 2], [cu - wu / 2, cv + wv / 2]].map(([u, v]) => W(u, v));
    B.prism(rect, g0 - 1, g0 + H, {
      cell: WAREHOUSE_CELL, tileW: CELL_TILE[WAREHOUSE_CELL][0], tileH: CELL_TILE[WAREHOUSE_CELL][1], vRef: g0,
      uOff: Math.floor(rng() * 4) / 4, vOff: 0, roofCell: ROOF_CELL, roofTile: 16,
    });
    this.stats.buildings++;
    const yaw = Math.atan2(G.uAxis[1], G.uAxis[0]);
    for (let k = 0; k < 2 + Math.floor(rng() * 3); k++) {
      const p = W(cu + (rng() - 0.5) * wu * 0.7, cv + (rng() - 0.5) * wv * 0.7);
      B.box(p[0], g0 + H - 0.1, p[1], 2 + rng() * 4, 1.2 + rng() * 2, 2 + rng() * 3, yaw, { cell: ROOF_CELL, tileW: 6, tileH: 6 });
    }
    const extra = rng();
    if (extra < 0.07) {
      // Chimney stack with an aircraft light.
      const p = W(lu1 - 3, lv1 - 3);
      const h = 35 + rng() * 30;
      B.prism(circle(p[0], p[1], 1.6, 10), g0 - 1, g0 + h, { cell: ROOF_CELL, tileW: 6, tileH: 6 });
      this.neon.prism(circle(p[0], p[1], 1.65, 10), g0 + h - 5, g0 + h - 4.2, { roof: false, color: [3.2, 0.3, 0.2] });
      this.aircraft.push([p[0], g0 + h + 0.6, p[1]]);
    } else if (extra < 0.3) {
      // Storage tanks.
      for (const k of [0, 1]) {
        const p = W(lu0 + 7 + k * 13, lv1 - 7);
        const r = 5 + rng() * 1.5;
        B.prism(circle(p[0], p[1], r, 14), g0 - 1, g0 + 8 + rng() * 6, { cell: WAREHOUSE_CELL, tileW: 24, tileH: 48, vRef: g0 + 20, roofCell: ROOF_CELL });
      }
    }
  }

  // Stacked shipping containers under floodlights.
  containerYard(o, core) {
    const { bu0, bu1, bv0, bv1, blockClear, pad, W, groundB } = o;
    if (!blockClear) return;
    const rng = this.rng, G = this.grid, g0 = this.groundY;
    pad(bu0, bv0, bu1, bv1, [0.1, 0.1, 0.11], 0.12);
    const yaw = Math.atan2(G.uAxis[1], G.uAxis[0]);
    const COLS = [[0.45, 0.12, 0.08], [0.08, 0.2, 0.42], [0.1, 0.32, 0.16], [0.6, 0.32, 0.06], [0.35, 0.36, 0.38], [0.5, 0.45, 0.1], [0.12, 0.3, 0.36]];
    // Rows of stacks in pairs, with aisles between.
    for (let v = bv0 + 6; v < bv1 - 4; v += 8.5) {
      for (let u = bu0 + 8; u < bu1 - 8; u += 13) {
        for (const dv of [0, 2.55]) {
          if (rng() < 0.15) continue;
          const stack = 1 + Math.floor(rng() * 3);
          const p = W(u + 6.1, v + dv);
          const c = COLS[Math.floor(rng() * COLS.length)];
          groundB.box(p[0], g0 + 0.12, p[1], 12.2, 2.6 * stack - 0.05, 2.45, yaw, { color: c, roofColor: c.map((x) => x * 1.15) });
        }
      }
    }
    for (const [a, b] of [[0.1, 0.1], [0.9, 0.9], [0.1, 0.9], [0.9, 0.1]]) {
      this.lampSpots.push({ u: lerp(bu0, bu1, a), v: lerp(bv0, bv1, b), du: a < 0.5 ? 1 : -1, dv: 0, h: 14 });
    }
  }

  // Promenade, quay, water and the ferris wheel along the waterfront.
  buildWaterfront() {
    const wf = this.waterfront, t = this.track, g0 = this.groundY, rng = this.rng;
    if (!wf) return;
    const out = -this.insideSign;
    const path = this.path;
    const ranges = [[wf.s0 - 150, wf.s1 + 150]];
    const lats = (a, b) => (out < 0 ? [out * b, out * a] : [out * a, out * b]);
    // Water: dark and glossy; the basin's banks hide its edges.
    const [w0, w1] = lats(80, 385);
    const water = new THREE.MeshStandardMaterial({ color: 0x0a1726, roughness: 0.06, metalness: 0.85, envMapIntensity: 1.4 });
    this.group.add(staticMesh(sweep(path, ranges, [{ lat: w0, y: () => g0 - 1.6 }, { lat: w1, y: () => g0 - 1.6 }], { step: 6 }), water, { receive: true }));
    // Reflections: long soft streaks running across the water from the
    // lamps on the quay and the lit far shore.
    {
      const pos = [], uv = [], col = [];
      const f = {};
      const streak = (s, lat0, len, w, c) => {
        path.frame(s, f);
        const a = [f.x + f.rx * lat0, f.z + f.rz * lat0];
        const d = [f.rx * out * len, f.rz * out * len], sw = [f.fx * w / 2, f.fz * w / 2];
        const y = g0 - 1.55;
        const P = [[a[0] - sw[0], a[1] - sw[1]], [a[0] + sw[0], a[1] + sw[1]], [a[0] + sw[0] + d[0], a[1] + sw[1] + d[1]], [a[0] - sw[0] + d[0], a[1] - sw[1] + d[1]]];
        const U = [[0, 0.5], [1, 0.5], [1, 1], [0, 1]];
        // Two triangles facing up whichever way `out` points.
        const order = out < 0 ? [0, 1, 2, 0, 2, 3] : [0, 2, 1, 0, 3, 2];
        for (const k of order) { pos.push(P[k][0], y, P[k][1]); uv.push(U[k][0], U[k][1]); col.push(...c); }
      };
      for (let s = wf.s0 - 30; s < wf.s1 + 30; s += 24) streak(s + 1, out * 80, 70 + rng() * 40, 3, [0.5, 0.36, 0.2]);
      const tints = [[0.5, 0.4, 0.25], [0.35, 0.4, 0.5], [0.55, 0.15, 0.45], [0.15, 0.45, 0.5]];
      for (let s = wf.s0 - 60; s < wf.s1 + 60; s += 9) {
        if (rng() < 0.35) continue;
        streak(s, out * (360 - rng() * 20), -(60 + rng() * 110), 1.5 + rng() * 3, tints[Math.floor(rng() * (rng() < 0.8 ? 2 : 4))]);
      }
      const g = new THREE.BufferGeometry();
      g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
      g.setAttribute('uv', new THREE.Float32BufferAttribute(uv, 2));
      g.setAttribute('color', new THREE.Float32BufferAttribute(col, 3));
      g.computeBoundingSphere();
      const m = new THREE.MeshBasicMaterial({ map: glowTexture(), vertexColors: true, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending });
      const mesh = staticMesh(g, m);
      mesh.renderOrder = 2;
      this.group.add(mesh);
    }
    // Promenade deck and quay wall.
    const pr = [[wf.s0 - 40, wf.s1 + 40]];
    const [p0, p1] = lats(36, 79);
    const deck = new THREE.MeshStandardMaterial({ color: 0x8f877a, roughness: 0.85 });
    this.group.add(staticMesh(sweep(path, pr, [{ lat: p0, y: () => g0 + 0.12 }, { lat: p1, y: () => g0 + 0.12 }], { step: 5, uS: 3, vS: 3 }), deck, { receive: true }));
    const quay = out < 0
      ? [{ lat: -79, y: () => g0 - 2.6 }, { lat: -79, y: () => g0 + 0.12 }]
      : [{ lat: 79, y: () => g0 + 0.12 }, { lat: 79, y: () => g0 - 2.6 }];
    const conc = new THREE.MeshStandardMaterial({ color: 0x6f6a62, roughness: 0.9 });
    this.group.add(staticMesh(sweep(path, pr, quay, { step: 5, uv: 'wall' }), conc, { receive: true }));
    const rail = out < 0
      ? [{ lat: -78.6, y: () => g0 + 0.9 }, { lat: -78.6, y: () => g0 + 1.05 }]
      : [{ lat: 78.6, y: () => g0 + 1.05 }, { lat: 78.6, y: () => g0 + 0.9 }];
    const railMat = new THREE.MeshStandardMaterial({ color: 0xb8bcc2, metalness: 0.7, roughness: 0.4, side: THREE.DoubleSide });
    this.group.add(staticMesh(sweep(path, pr, rail, { step: 5 }), railMat));
    // Lamps and trees along the promenade.
    const f = {};
    this.extraLamps ||= [];
    for (let s = wf.s0 - 30; s < wf.s1 + 30; s += 24) {
      path.frame(s, f);
      const lat = out * 76;
      this.extraLamps.push({ x: f.x + f.rx * lat, y: g0 + 0.12, z: f.z + f.rz * lat, dx: -f.rx * out, dz: -f.rz * out, h: 5.5, arm: 0.8 });
      for (const tl of [48, 60]) {
        if (rng() < 0.3) continue;
        const tt = out * (tl + rng() * 6);
        this.parkTrees.push({ x: f.x + f.rx * tt, y: g0 + 0.12, z: f.z + f.rz * tt, s: 0.9 + rng() * 0.5 });
      }
    }
    // The ferris wheel, two-thirds of the way along.
    const sw = wf.s0 + (wf.s1 - wf.s0) * 0.62;
    path.frame(sw, f);
    const lat = out * 64;
    this.buildFerrisWheel(f.x + f.rx * lat, g0 + 0.12, f.z + f.rz * lat, f.fx, f.fz);
    this.fw.reserved.push([sw - 40, sw + 40]);
  }

  buildFerrisWheel(x, y0, z, fx, fz) {
    const R = 27, hub = y0 + R + 6;
    const root = new THREE.Group();
    root.position.set(x, hub, z);
    root.rotation.y = Math.atan2(-fz, fx); // local X along the road
    const rotor = new THREE.Group();
    root.add(rotor);
    const metal = new THREE.MeshStandardMaterial({ color: 0xd8dce2, metalness: 0.6, roughness: 0.35, emissive: 0x223044, emissiveIntensity: 0.6 });
    const parts = [];
    for (const zz of [-1.4, 1.4]) {
      const rim = new THREE.TorusGeometry(R, 0.28, 6, 72);
      rim.translate(0, 0, zz);
      parts.push(rim);
    }
    for (let k = 0; k < 24; k++) {
      const a = (k / 24) * Math.PI * 2;
      for (const zz of [-1.4, 1.4]) {
        const sp = new THREE.CylinderGeometry(0.08, 0.08, R, 4);
        sp.translate(0, R / 2, 0);
        sp.rotateZ(a - Math.PI / 2);
        sp.translate(0, 0, zz);
        parts.push(sp);
      }
    }
    const hubG = new THREE.CylinderGeometry(1.4, 1.4, 3.6, 12);
    hubG.rotateX(Math.PI / 2);
    parts.push(hubG);
    rotor.add(staticMesh(mergeFlatGeos(parts), metal));
    // Rim lights in rainbow order, and gondolas.
    const lp = [], lc = [];
    const c = new THREE.Color();
    for (let k = 0; k < 96; k++) {
      const a = (k / 96) * Math.PI * 2;
      c.setHSL(k / 96, 0.9, 0.55);
      for (const zz of [-1.7, 1.7]) {
        lp.push(Math.cos(a) * R, Math.sin(a) * R, zz);
        lc.push(c.r * 3.2, c.g * 3.2, c.b * 3.2);
      }
    }
    const lg = new THREE.BufferGeometry();
    lg.setAttribute('position', new THREE.Float32BufferAttribute(lp, 3));
    lg.setAttribute('color', new THREE.Float32BufferAttribute(lc, 3));
    const lm = new THREE.PointsMaterial({ size: 0.9, vertexColors: true, map: glowTexture(), transparent: true, depthWrite: false, blending: THREE.AdditiveBlending });
    const lights = new THREE.Points(lg, lm);
    rotor.add(lights);
    const gond = new GeoBuilder({ color: true });
    for (let k = 0; k < 16; k++) {
      const a = (k / 16) * Math.PI * 2;
      const gx = Math.cos(a) * (R + 1.8), gy = Math.sin(a) * (R + 1.8);
      c.setHSL(k / 16, 0.7, 0.5);
      gond.box(gx, gy - 1.4, 0, 2.2, 2.4, 2.2, 0, { color: [c.r * 0.8, c.g * 0.8, c.b * 0.8] });
    }
    rotor.add(staticMesh(gond.build(), new THREE.MeshBasicMaterial({ vertexColors: true })));
    // A-frame legs.
    const legs = [];
    for (const zz of [-3.2, 3.2]) for (const xx of [-15, 15]) {
      const len = Math.hypot(xx, hub - y0);
      const g = new THREE.CylinderGeometry(0.45, 0.6, len, 8);
      g.translate(0, -len / 2, 0);
      g.rotateZ(Math.atan2(xx, hub - y0));
      g.translate(0, 0, zz);
      legs.push(g);
    }
    root.add(staticMesh(mergeFlatGeos(legs), metal));
    this.group.add(root);
    this.world.updaters.push((dt) => { rotor.rotation.z += dt * 0.06; });
  }

  // One building on a lot [lu0,lu1]×[lv0,lv1] in grid space.
  building(lu0, lv0, lu1, lv1, o) {
    const rng = this.rng, G = this.grid, g0 = this.groundY + 0.15;
    const W = (u, v) => G.toWorld(u, v);
    const cu = (lu0 + lu1) / 2, cv = (lv0 + lv1) / 2;
    const cw = W(cu, cv);
    const B = this.chunkOf(cw[0], cw[1]);
    const LU = lu1 - lu0, LV = lv1 - lv0;
    const pick = (arr) => arr[Math.floor(rng() * arr.length)];
    this.stats.buildings++;
    // Per-building seed for the window shader, from position so the layout
    // rng stream is untouched.
    const seed = hash2(cw[0], cw[1]);
    // Distance to the freeway for neon/visibility decisions.
    const hits = this.freewayHits(cw[0], cw[1]);
    const nearFw = hits.length ? Math.min(...hits.map((h) => Math.abs(h.lat))) : 999;
    // Roof clutter and trim only where it will be seen up close: along the
    // freeway. Further out, the silhouette is what counts.
    const detail = nearFw < 420;
    const yaw = Math.atan2(G.uAxis[1], G.uAxis[0]);

    // Snap a footprint to the chosen cell's window pitch.
    const foot = (cell, inset, jitter = 0) => {
      const pw = CELL_TILE[cell][0] / CELL_COLS[cell];
      let wu = Math.max(pw * 2, Math.floor((LU - inset * 2 - rng() * jitter) / pw) * pw);
      let wv = Math.max(pw * 2, Math.floor((LV - inset * 2 - rng() * jitter) / pw) * pw);
      return { wu, wv };
    };
    const rect = (wu, wv, du = 0, dv = 0) => [[cu + du - wu / 2, cv + dv - wv / 2], [cu + du + wu / 2, cv + dv - wv / 2], [cu + du + wu / 2, cv + dv + wv / 2], [cu + du - wu / 2, cv + dv + wv / 2]].map(([u, v]) => W(u, v));
    const floorH = (cell) => CELL_TILE[cell][1] / CELL_ROWS[cell];
    const pitch = (cell) => CELL_TILE[cell][0] / CELL_COLS[cell];
    const box = (cell, wu, wv, y0, y1, extra = {}) => {
      const cols = CELL_COLS[cell], rows = CELL_ROWS[cell];
      const r = rect(wu, wv, extra.du || 0, extra.dv || 0);
      B.prism(r, y0, y1, {
        cell: cellSeed(cell, seed), tileW: CELL_TILE[cell][0], tileH: CELL_TILE[cell][1], vRef: g0,
        uOff: Math.floor(rng() * cols) / cols, vOff: Math.floor(rng() * rows) / rows,
        roofCell: ROOF_CELL, roofTile: 16, roof: extra.roof ?? true,
      });
      return r;
    };
    const centreOf = (r) => [(r[0][0] + r[2][0]) / 2, (r[0][1] + r[2][1]) / 2];
    // Roof plant: AC units and vents.
    const plant = (wu, wv, top, n, du = 0, dv = 0) => {
      for (let k = 0; k < n; k++) {
        const p = W(cu + du + (rng() - 0.5) * wu * 0.5, cv + dv + (rng() - 0.5) * wv * 0.5);
        B.box(p[0], top - 0.1, p[1], 2 + rng() * 5, 1.5 + rng() * 2.5, 2 + rng() * 4, yaw, { cell: ROOF_CELL, tileW: 6, tileH: 6 });
      }
    };
    // Timber-and-steel water tank on a stand, New York style.
    const tank = (du, dv, top) => {
      const p = W(cu + du, cv + dv);
      const r = 1.5 + rng() * 0.8, h = 3 + rng() * 1.5, leg = 2.2 + rng();
      B.box(p[0], top - 0.1, p[1], r * 2, leg, 0.25, yaw, { cell: ROOF_CELL, tileW: 6, tileH: 6, roof: false });
      B.box(p[0], top - 0.1, p[1], 0.25, leg, r * 2, yaw, { cell: ROOF_CELL, tileW: 6, tileH: 6, roof: false });
      const ring = circle(p[0], p[1], r, 8);
      const y0 = top + leg, y1 = y0 + h;
      B.prism(ring, y0, y1, { cell: WAREHOUSE_CELL, tileW: 24, tileH: 48, vRef: y0 - 22, roof: false });
      // Underside (seen from the street), as a fan of quads facing down.
      for (const k of [1, 3, 5]) upQuad(B, ...[ring[0], ring[k], ring[k + 1], ring[k + 2]].map((q) => [q[0], y0, q[1]]), ROOF_CELL, null, true);
      const apex = [p[0], y1 + r * 0.7, p[1]];
      for (let k = 0; k < 8; k++) {
        const a = ring[k], b = ring[(k + 1) % 8];
        B.tri([a[0], y1, a[1]], apex, [b[0], y1, b[1]], null, ROOF_CELL);
      }
    };
    // Parapet ring round a roof: outer face, inner face and coping.
    const parapet = (r, top, h = 1.1, t = 0.35) => {
      const c = centreOf(r);
      const inner = growRect(r, c, -t);
      B.prism(r, top - 0.05, top + h, { cell: ROOF_CELL, tileW: 8, tileH: 8, roof: false });
      wallsInward(B, inner, top - 0.05, top + h, ROOF_CELL);
      for (let k = 0; k < 4; k++) {
        const a = r[k], b = r[(k + 1) % 4], ai = inner[k], bi = inner[(k + 1) % 4];
        upQuad(B, [a[0], top + h, a[1]], [b[0], top + h, b[1]], [bi[0], top + h, bi[1]], [ai[0], top + h, ai[1]], ROOF_CELL);
      }
    };
    const antenna = (x, z, top, h) => {
      B.box(x, top - 0.3, z, 0.5, h, 0.5, yaw, { cell: ROOF_CELL, tileW: 4, tileH: 4 });
      B.box(x, top + h * 0.55, z, 2.4, 0.2, 0.2, yaw, { cell: ROOF_CELL, tileW: 4, tileH: 4, roof: false });
      this.aircraft.push([x, top + h + 0.3, z]);
    };
    // LED strips up the corners of a volume.
    const cornerLeds = (r, y0, y1, col) => {
      const c = centreOf(r);
      for (const p of growRect(r, c, 0.12)) this.neon.box(p[0], y0, p[1], 0.3, y1 - y0, 0.3, yaw, { color: col, roof: false });
    };

    if (o.tower) {
      this.stats.towers++;
      const H = 70 + o.core * (60 + rng() * 210) + rng() * 30;
      const cell = pick([2, 4, 4, 0, 2, 1]);
      const podCell = pick([1, 5, 3, 0]);
      const fh = floorH(podCell);
      const podH = fh * (3 + Math.floor(rng() * 3));
      const pf = foot(podCell, 1.0);
      const podR = box(podCell, pf.wu, pf.wv, g0 - 1, g0 + podH);
      if (detail && rng() < 0.5) parapet(podR, g0 + podH, 0.9, 0.3);
      const pw = pitch(cell);
      const sf = foot(cell, 3 + rng() * 6, 4);
      // Massing: a straight shaft with a slimmer top tier, a stepped
      // "wedding cake", or an off-centre slab with a taller slim twin.
      const form = rng();
      let tw = sf.wu, tv = sf.wv, tdu = 0, tdv = 0, topRect, top;
      if (form < 0.3) {
        const steps = 3 + (rng() < 0.4 ? 1 : 0);
        const fr = [0.5, 0.25, 0.15, 0.1];
        let y = g0 + podH - 0.5;
        for (let k = 0; k < steps; k++) {
          if (k > 0) {
            tw = Math.max(pw * 3, tw - pw * 2 * (1 + Math.floor(rng() * 2)));
            tv = Math.max(pw * 3, tv - pw * 2 * (1 + Math.floor(rng() * 2)));
          }
          const y1 = k === steps - 1 ? g0 + H : y + (H - podH) * fr[k] * (steps === 3 ? 1.15 : 1);
          topRect = box(k === steps - 1 && rng() < 0.3 ? GLASS_CELL : cell, tw, tv, y, y1);
          if (k < steps - 1 && detail) plant(tw, tv, y1, 1);
          // Setback terraces get a lit band along their edge.
          if (k < steps - 1 && rng() < 0.4) this.neon.prism(growRect(topRect, cw, 0.25), y1 - 1.6, y1 - 0.9, { roof: false, color: pick(NEON).map((x) => x * 0.6) });
          y = y1 - 0.3;
        }
        top = g0 + H;
      } else if (form < 0.75) {
        const shaftTop = g0 + H * (0.7 + rng() * 0.1);
        topRect = box(cell, sf.wu, sf.wv, g0 + podH - 0.5, shaftTop);
        top = shaftTop;
        if (rng() < 0.8) {
          tw = Math.max(pw * 3, sf.wu - pw * 2 * (1 + Math.floor(rng() * 3)));
          tv = Math.max(pw * 3, sf.wv - pw * 2 * (1 + Math.floor(rng() * 3)));
          if (detail) plant(sf.wu, sf.wv, shaftTop, 2);
          topRect = box(rng() < 0.3 ? GLASS_CELL : cell, tw, tv, shaftTop - 0.3, g0 + H);
          top = g0 + H;
        }
      } else {
        // Slab on one side of the lot, a slimmer tower rising past it.
        const main = { wu: sf.wu, wv: Math.max(pw * 3, Math.floor(sf.wv * 0.55 / pw) * pw) };
        const side = rng() < 0.5 ? -1 : 1;
        const dvMain = side * (sf.wv - main.wv) / 2;
        const slabTop = g0 + H * (0.5 + rng() * 0.15);
        const slab = box(cell, main.wu, main.wv, g0 + podH - 0.5, slabTop, { dv: dvMain });
        parapet(slab, slabTop, 1.2, 0.3);
        if (detail) plant(main.wu, main.wv, slabTop, 2, 0, dvMain);
        tw = Math.max(pw * 3, Math.floor(sf.wu * 0.6 / pw) * pw);
        tv = Math.max(pw * 3, sf.wv - main.wv);
        tdv = -side * (sf.wv - tv) / 2;
        tdu = (rng() - 0.5) * (sf.wu - tw);
        topRect = box(rng() < 0.4 ? GLASS_CELL : cell, tw, tv, g0 + podH - 0.5, g0 + H, { du: tdu, dv: tdv });
        top = g0 + H;
      }
      const tierTop = top;
      const tc = centreOf(topRect);
      // Crown.
      const style = rng();
      if (style < 0.25) {
        // Glass cap, maybe with a spire.
        const ch = 6 + rng() * 10;
        box(GLASS_CELL, tw * 0.6, tv * 0.6, top - 0.2, top + ch, { du: tdu, dv: tdv });
        top += ch;
      } else if (style < 0.45) {
        // Pyramid.
        const apex = [tc[0], top + Math.min(tw, tv) * 0.7, tc[1]];
        const r = topRect;
        for (let k = 0; k < 4; k++) {
          const a = r[k], b = r[(k + 1) % 4];
          B.tri([a[0], top, a[1]], [b[0], top, b[1]], apex, null, GLASS_CELL);
          B.tri([a[0], top, a[1]], apex, [b[0], top, b[1]], null, GLASS_CELL);
        }
        top = apex[1];
      } else if (style < 0.6) {
        // Stepped crown: three shrinking glass tiers, each edged in light.
        const col = pick(NEON);
        let cw2 = tw, cv2 = tv;
        for (let k = 0; k < 3; k++) {
          cw2 *= 0.78; cv2 *= 0.78;
          const h = 4 + rng() * 3;
          const r = box(GLASS_CELL, cw2, cv2, top - 0.2, top + h, { du: tdu, dv: tdv });
          this.neon.prism(growRect(r, tc, 0.2), top + h - 0.8, top + h - 0.2, { roof: false, color: col.map((x) => x * 0.8) });
          top += h;
        }
      } else if (style < 0.75) {
        // Spire on a plant room.
        box(ROOF_CELL, tw * 0.45, tv * 0.45, top - 0.2, top + 5, { du: tdu, dv: tdv });
        top += 5;
        const sh = 18 + rng() * 40, sr = Math.min(tw, tv) * 0.12 + 0.6;
        const base = circle(tc[0], tc[1], sr, 6);
        const apex = [tc[0], top + sh, tc[1]];
        for (let k = 0; k < 6; k++) {
          const a = base[k], b = base[(k + 1) % 6];
          B.tri([a[0], top, a[1]], apex, [b[0], top, b[1]], null, GLASS_CELL);
        }
        top = apex[1];
      } else if (style < 0.87 && tw > 18 && tv > 18) {
        // Helipad: a raised deck with edge lights and a painted H.
        parapet(topRect, top, 1.1, 0.35);
        const s = Math.min(tw, tv) * 0.7;
        const deckY = top + 1.4;
        box(ROOF_CELL, s, s, top - 0.2, deckY, { du: tdu, dv: tdv });
        this.helipad(tc[0], deckY + 0.03, tc[1], s, yaw);
      } else {
        // Flat roof: parapet, plant, maybe LED corners.
        parapet(topRect, top, 1.4, 0.35);
        if (detail) plant(tw, tv, top, 2, tdu, tdv);
        if (rng() < 0.5) cornerLeds(topRect, top - Math.min(40, (top - g0) * 0.3), top + 1.4, pick(NEON).map((x) => x * 0.7));
      }
      // LED crown band.
      if (rng() < 0.55) {
        const col = pick(NEON);
        const band = growRect(topRect, tc, 0.3);
        const by = tierTop - 3.2;
        this.neon.prism(band, by, by + 1.4, { roof: false, color: col });
      }
      // Antennas and aircraft lights.
      if (H > 150 && rng() < 0.6) {
        const ah = 15 + rng() * 30;
        B.box(tc[0], top - 0.5, tc[1], 1.0, ah, 1.0, 0, { cell: ROOF_CELL, tileW: 4, tileH: 4 });
        this.aircraft.push([tc[0], top + ah + 0.4, tc[1]]);
      } else if (H > 90) {
        this.aircraft.push([tc[0], top + 0.6, tc[1]]);
      }
      if (H > 200 && rng() < 0.7) for (const p of topRect) this.aircraft.push([p[0], tierTop + 0.8, p[1]]);
      return;
    }

    // Mid- and low-rise.
    const mid = o.ring > 0.45 && rng() < 0.75;
    const cell = o.cell ?? (mid ? pick([0, 1, 3, 1]) : pick([5, 5, 3, 1]));
    const brick = cell === BRICK_CELL;
    const fh = floorH(cell);
    let floors = o.floors ? o.floors[0] + Math.floor(rng() * (o.floors[1] - o.floors[0] + 1))
      : mid ? 4 + Math.floor(rng() * (6 + o.ring * 12)) : 2 + Math.floor(rng() * 4);
    if (rng() < 0.07 + o.ring * 0.08) floors = Math.round(floors * (1.8 + rng() * 1.4)); // the odd taller block
    const H = floors * fh;
    const f = foot(cell, 0.5 + rng() * 3, 3);
    let r, roofR, wu = f.wu, wv = f.wv, du = 0, dv = 0;
    if (!brick && floors >= 8 && rng() < 0.35) {
      // Setback: the top few floors step in on one or two sides.
      const hs = g0 + fh * Math.round(floors * (0.6 + rng() * 0.2));
      r = box(cell, f.wu, f.wv, g0 - 1, hs);
      const pw = pitch(cell);
      wu = Math.max(pw * 2, f.wu - pw * (1 + Math.floor(rng() * 2)));
      wv = Math.max(pw * 2, f.wv - pw * (1 + Math.floor(rng() * 2)));
      du = (rng() < 0.5 ? -1 : 1) * (f.wu - wu) / 2;
      dv = (rng() < 0.5 ? -1 : 1) * (f.wv - wv) / 2;
      if (detail) parapet(r, hs, 0.9, 0.3);
      roofR = box(cell, wu, wv, hs - 0.3, g0 + H, { du, dv });
    } else {
      r = roofR = box(cell, f.wu, f.wv, g0 - 1, g0 + H);
    }
    const top = g0 + H;
    const rc = centreOf(roofR);
    if (detail) {
      if (brick) {
        // Cornice and parapet.
        const cr = growRect(roofR, rc, 0.45);
        B.prism(cr, top - 0.9, top - 0.1, { cell: ROOF_CELL, tileW: 8, tileH: 8, roof: false });
        for (let k = 0; k < 4; k++) {
          const a = cr[k], b = cr[(k + 1) % 4], ai = roofR[k], bi = roofR[(k + 1) % 4];
          upQuad(B, [a[0], top - 0.9, a[1]], [b[0], top - 0.9, b[1]], [bi[0], top - 0.9, bi[1]], [ai[0], top - 0.9, ai[1]], ROOF_CELL, null, true);
        }
        parapet(roofR, top, 0.8, 0.3);
      } else if (rng() < 0.65) parapet(roofR, top, 0.9 + rng() * 0.6, 0.3);
      plant(wu, wv, top, 1 + Math.floor(rng() * 3), du, dv);
      if (rng() < (brick ? 0.45 : 0.18) && wu > 9 && wv > 9) tank((rng() - 0.5) * wu * 0.4 + du, (rng() - 0.5) * wv * 0.4 + dv, top);
      if (H > 30 && rng() < 0.2) antenna(rc[0] + (rng() - 0.5) * 4, rc[1] + (rng() - 0.5) * 4, top, 8 + rng() * 12);
      if (nearFw < 260 && H < 50 && rng() < 0.14) this.roofBillboard(roofR, top, cw);
      if (!brick && rng() < 0.08) this.neon.prism(growRect(roofR, rc, 0.2), top - 0.6, top - 0.2, { roof: false, color: pick(NEON).map((x) => x * 0.6) });
    } else {
      plant(wu, wv, top, 1, du, dv);
    }
    // Neon signage on buildings that face the freeway.
    if (nearFw < 220 && rng() < 0.4) {
      const col = pick(NEON);
      // Face whose outward normal points most toward the freeway.
      const tr = this.track.distanceToRoad(cw[0], cw[1], 400);
      let best = 0, bestDot = -Infinity;
      for (let k = 0; k < 4; k++) {
        const a = r[k], b = r[(k + 1) % 4];
        const mx = (a[0] + b[0]) / 2, mz = (a[1] + b[1]) / 2;
        const nx = mx - cw[0], nz = mz - cw[1];
        const tf = tr.i >= 0 ? this.track.frame(tr.s, {}) : this.DC;
        const dx = tf.x - mx, dz = tf.z - mz;
        const d = (nx * dx + nz * dz) / (Math.hypot(nx, nz) * Math.hypot(dx, dz) + 1e-6);
        if (d > bestDot) { bestDot = d; best = k; }
      }
      const a = r[best], b = r[(best + 1) % 4];
      const mx = (a[0] + b[0]) / 2, mz = (a[1] + b[1]) / 2;
      const nx = mx - cw[0], nz = mz - cw[1];
      const nl = Math.hypot(nx, nz);
      const ox = (nx / nl) * 0.35, oz = (nz / nl) * 0.35;
      const tx = (b[0] - a[0]), tz = (b[1] - a[1]);
      const tl = Math.hypot(tx, tz);
      const along = (rng() - 0.5) * 0.6;
      const px = mx + (tx / tl) * along * tl + ox, pz = mz + (tz / tl) * along * tl + oz;
      const w = 1.6 + rng() * 1.2, hh = Math.min(H - 6, 6 + rng() * 10);
      const y0 = Math.max(g0 + 5, g0 + H - hh - 1.5);
      const ux = (tx / tl) * w / 2, uz = (tz / tl) * w / 2;
      // Double-sided vertical sign.
      const A = [px - ux, y0, pz - uz], Bp = [px + ux, y0, pz + uz], C = [px + ux, y0 + hh, pz + uz], D = [px - ux, y0 + hh, pz - uz];
      this.neon.quad(A, Bp, C, D, null, col);
      this.neon.quad(Bp, A, D, C, null, col);
    }
  }

  // The setback strips either side of the freeway: landscaped verges with
  // trees and lamps, so the ground between the barriers and the first
  // buildings isn't a black void at night.
  buildVerge() {
    const t = this.track, T = this.T, path = this.path, G = this.grid, fw = this.fw;
    const rng = mulberry32(31), f = {};
    this.extraLamps ||= [];
    let k = 0;
    for (let u = path.u0 + 10; u < path.u1 - 10; u += 9, k++) {
      if (fw.reservedAt(u, 45) || fw.inTunnel(u)) continue;
      if (u >= path.sA && u <= path.sB && T.isElevated(t.idx(u))) continue;
      path.frame(u, f);
      const sides = [[-52, -41, -1]];
      if (!(f.ext && u < path.sA)) sides.push([17.5, 29, 1]);
      for (const [a, b, side] of sides) {
        const lamp = k % 5 === (side > 0 ? 0 : 2);
        if (!lamp && rng() < 0.4) continue;
        const lat = lamp ? (side > 0 ? 17.5 : -41) : lerp(a, b, rng());
        const x = f.x + f.rx * lat, z = f.z + f.rz * lat;
        const gy = T.heightAt(x, z);
        if (Math.abs(gy - this.groundY) > 1.2) continue;
        if (this.streetCells.has(Math.floor(G.toU(x, z) / PU) + ',' + Math.floor(G.toV(x, z) / PV))) continue;
        if (this.freewayHits(x, z).some((h) => h.lat > -40 && h.lat < 16)) continue;
        if (lamp) this.extraLamps.push({ x, y: gy, z, dx: f.rx * side, dz: f.rz * side, h: 7, arm: 1.4 });
        else this.parkTrees.push({ x, y: gy, z, s: 0.8 + rng() * 0.6 });
      }
    }
  }

  // Lots too close to the freeway for a building become car parks: asphalt,
  // tall lamps and rows of parked cars instead of dead black ground.
  parkingLot(lu0, lv0, lu1, lv1, pad) {
    const G = this.grid, rng = this.rng;
    const W = (u, v) => G.toWorld(u, v);
    for (let a = 0; a <= 4; a++) for (let b = 0; b <= 4; b++) {
      const p = W(lerp(lu0 - 2.5, lu1 + 2.5, a / 4), lerp(lv0 - 2.5, lv1 + 2.5, b / 4));
      if (!this.flat(p[0], p[1])) return;
      for (const h of this.freewayHits(p[0], p[1])) if (h.lat > -39 && h.lat < 15) return;
    }
    pad(lu0 - 2.5, lv0 - 2.5, lu1 + 2.5, lv1 + 2.5, [0.1, 0.1, 0.105], 0.12);
    const y = this.groundY + 0.27;
    const yaw = Math.atan2(G.uAxis[1], G.uAxis[0]);
    let row = 0;
    for (let v = lv0 + 3; v + 2.7 < lv1; v += 6.2, row++) {
      if (row % 2 === 1) continue; // aisle
      for (let u = lu0 + 1.5; u + 1.4 < lu1; u += 2.8) {
        if (rng() < 0.4) continue;
        const p = W(u + 1.4, v + 1.5 + (rng() - 0.5) * 0.4);
        this.parked.push([p[0], y, p[1], yaw + Math.PI / 2 + (rng() < 0.5 ? Math.PI : 0) + (rng() - 0.5) * 0.08]);
      }
    }
    for (let u = lu0 + 8; u < lu1 - 4; u += 24) for (let v = lv0 + 8; v < lv1 - 4; v += 24) {
      this.lampSpots.push({ u, v, du: 1, dv: 0, h: 9.5, arm: 0.5 });
    }
  }

  buildParkedCars() {
    if (!this.parked.length) return;
    const body = new THREE.BoxGeometry(4.3, 0.72, 1.78).toNonIndexed();
    body.translate(0, 0.62, 0);
    const cab = new THREE.BoxGeometry(2.3, 0.56, 1.58).toNonIndexed();
    cab.translate(-0.25, 1.26, 0);
    const paint = (g, c) => {
      const n = g.getAttribute('position').count, a = new Float32Array(n * 3);
      for (let i = 0; i < n; i++) a.set(c, i * 3);
      g.setAttribute('color', new THREE.BufferAttribute(a, 3));
      return g;
    };
    const geo = mergeTwo(paint(body, [1, 1, 1]), paint(cab, [0.25, 0.27, 0.3]));
    const mat = new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.35, metalness: 0.55 });
    const rng = mulberry32(7);
    const PAINT = [[0.55, 0.56, 0.58], [0.06, 0.06, 0.07], [0.7, 0.7, 0.72], [0.35, 0.05, 0.05], [0.08, 0.14, 0.32], [0.3, 0.3, 0.32], [0.9, 0.9, 0.9], [0.15, 0.25, 0.18]];
    const ms = this.parked.map((p) => trs(p[0], p[1], p[2], p[3]));
    const cols = this.parked.map(() => new THREE.Color(...PAINT[Math.floor(rng() * PAINT.length)]));
    this.emitInstanced(geo, mat, ms, { receive: true }, 1300, cols);
  }

  // A tower under construction: bare concrete core and floor slabs, work
  // lights, and a tower crane with aircraft lights on the mast and jib.
  site(lu0, lv0, lu1, lv1, core) {
    this.sites++;
    const rng = this.rng, G = this.grid, g0 = this.groundY + 0.15;
    const cu = (lu0 + lu1) / 2, cv = (lv0 + lv1) / 2;
    const cw = G.toWorld(cu, cv);
    const B = this.chunkOf(cw[0], cw[1]);
    const yaw = Math.atan2(G.uAxis[1], G.uAxis[0]);
    const o = { cell: ROOF_CELL, tileW: 8, tileH: 8 };
    const S = Math.min(lu1 - lu0, lv1 - lv0) * 0.7;
    const Hs = 25 + rng() * (30 + core * 60);
    B.box(cw[0], g0 - 1, cw[1], S * 0.35, Hs + 5, S * 0.35, yaw, o);
    // Slabs every floor on thin columns.
    for (let y = g0 + 4; y < g0 + Hs; y += 4) B.box(cw[0], y, cw[1], S, 0.35, S, yaw, { ...o, bottom: true });
    for (const [a, b] of [[-1, -1], [1, -1], [1, 1], [-1, 1]]) {
      const p = G.toWorld(cu + a * S * 0.45, cv + b * S * 0.45);
      B.box(p[0], g0 - 1, p[1], 0.6, Hs + 1, 0.6, yaw, o);
      if (rng() < 0.6) this.glows.push([p[0], g0 + 4 + Math.floor(rng() * Hs / 4) * 4 - 0.4, p[1], 1.2, 1.2, 1.1, 1.2]);
    }
    // Crane beside it.
    const mp = G.toWorld(cu + S * 0.65, cv - S * 0.3);
    const mh = Hs + 25 + rng() * 25;
    B.box(mp[0], g0 - 0.5, mp[1], 2.0, mh, 2.0, yaw, o);
    const jy = yaw + rng() * Math.PI * 2;
    const jl = 40 + rng() * 20, cl = 14;
    const jx = Math.cos(jy), jz = Math.sin(jy);
    B.box(mp[0] + jx * (jl - cl) / 2, g0 + mh, mp[1] + jz * (jl - cl) / 2, jl + cl, 1.4, 1.4, jy, { ...o, bottom: true });
    B.box(mp[0], g0 + mh + 1.4, mp[1], 1.4, 7, 1.4, jy, o);
    B.box(mp[0] + jx * 1.8, g0 + mh - 3, mp[1] + jz * 1.8, 2.4, 3, 2.4, jy, { ...o, bottom: true });
    B.box(mp[0] - jx * (cl - 2), g0 + mh - 2.5, mp[1] - jz * (cl - 2), 3.5, 2.5, 3, jy, { ...o, bottom: true });
    // Cab light, then the aviation lights.
    this.glows.push([mp[0] + jx * 1.8, g0 + mh - 1.5, mp[1] + jz * 1.8, 1.3, 1.1, 0.8, 0.7]);
    this.aircraft.push([mp[0], g0 + mh + 8.8, mp[1]], [mp[0] + jx * jl, g0 + mh + 1.8, mp[1] + jz * jl], [mp[0] - jx * cl, g0 + mh + 1.6, mp[1] - jz * cl]);
  }

  // A rooftop helipad: painted H and a ring of green edge lights.
  helipad(x, y, z, s, yaw) {
    const c = Math.cos(yaw), sn = Math.sin(yaw);
    const P = (a, b) => [x + a * c - b * sn, y, z + a * sn + b * c];
    const q = (a0, b0, a1, b1) => upQuad(this.neon, P(a0, b0), P(a1, b0), P(a1, b1), P(a0, b1), 0, [0.35, 0.35, 0.32]);
    const h = s * 0.18;
    q(-h, -h * 1.3, -h * 0.65, h * 1.3);
    q(h * 0.65, -h * 1.3, h, h * 1.3);
    q(-h * 0.65, -h * 0.18, h * 0.65, h * 0.18);
    for (let k = 0; k < 12; k++) {
      const a = (k / 12) * Math.PI * 2;
      this.padLights.push([x + Math.cos(a) * s * 0.42, y + 0.2, z + Math.sin(a) * s * 0.42]);
    }
  }

  // A billboard standing on a roof, facing the nearest bit of freeway.
  roofBillboard(r, top, cw) {
    const tr = this.track.distanceToRoad(cw[0], cw[1], 400);
    if (tr.i < 0) return;
    const f = this.track.frame(tr.s, {});
    const c = [(r[0][0] + r[2][0]) / 2, (r[0][1] + r[2][1]) / 2];
    let nx = f.x - c[0], nz = f.z - c[1];
    const nl = Math.hypot(nx, nz) || 1;
    nx /= nl; nz /= nl;
    const Wb = 12, Hb = Wb * 384 / 1024, lift = 2.5;
    const ax = [nz, -nx];
    const B = this.chunkOf(cw[0], cw[1]);
    const yawA = Math.atan2(ax[1], ax[0]);
    for (const k of [-0.3, 0.3]) B.box(c[0] - nx * 0.6 + ax[0] * Wb * k, top - 0.1, c[1] - nz * 0.6 + ax[1] * Wb * k, 0.35, lift + Hb * 0.6, 0.35, yawA, { cell: ROOF_CELL, tileW: 4, tileH: 4 });
    B.box(c[0] - nx * 0.25, top + lift - 0.2, c[1] - nz * 0.25, Wb + 0.4, Hb + 0.4, 0.3, yawA, { cell: ROOF_CELL, tileW: 4, tileH: 4 });
    // Nudge a hair toward the viewer so the ad sits in front of its frame.
    this.roofAds.push({ image: adTexture(this.roofAds.length * 5 + 3).image, c: [c[0] + nx * 0.02, top + lift + Hb / 2, c[1] + nz * 0.02], rx: ax[0], rz: ax[1], w: Wb, h: Hb });
  }

  // ── Street lamps (grid streets, overpasses, lid park) ─────────
  buildLamps() {
    const G = this.grid, g0 = this.groundY;
    this._thin = 0;
    const spots = [];
    const overs = this.fw.overpasses.filter((o) => o.v0 !== undefined);
    for (const l of this.lampSpots) {
      const onBridge = overs.some((o) => (o.fam === 'v'
        ? Math.abs(l.v - o.U) < 10 && l.u > o.v0 - 5 && l.u < o.v1 + 5
        : Math.abs(l.u - o.U) < 10 && l.v > o.v0 - 5 && l.v < o.v1 + 5));
      if (onBridge) continue;
      const p = G.toWorld(l.u, l.v);
      if (this.lampBlocked(p[0], p[1])) continue;
      if (this.loop && (++this._thin % 2) && this.T.far(p[0], p[1], this._farTmp).d > 600) continue;
      const d = [G.uAxis[0] * l.du + G.vAxis[0] * l.dv, G.uAxis[1] * l.du + G.vAxis[1] * l.dv];
      spots.push({ x: p[0], y: g0 + 0.15, z: p[1], dx: d[0], dz: d[1], h: l.h ?? 7.5, arm: l.arm, led: this.ledAt(p[0], p[1]) });
    }
    for (const o of overs) for (const l of o.lamps) spots.push({ x: l.x, y: l.y, z: l.z, dx: l.dx, dz: l.dz, h: 6.5 });
    for (const l of this.fw.parkLamps || []) spots.push({ x: l.x, y: l.y, z: l.z, dx: 1, dz: 0, h: 4.2, arm: 0 });
    for (const l of this.extraLamps || []) spots.push(l);
    if (!spots.length) return;
    // Pole with an arm along local +X.
    const pole = new THREE.CylinderGeometry(0.1, 0.14, 1, 6);
    pole.translate(0, 0.5, 0);
    const poleM = [], armM = [], lensM = [], poolM = [], tints = [];
    // Downtown streets have white LED heads, the rest orange sodium.
    const SOD = new THREE.Color(1, 0.72, 0.42), LED = new THREE.Color(0.75, 0.86, 1.05);
    for (const s of spots) {
      const tint = s.led ? LED : SOD;
      tints.push(tint);
      const yaw = -Math.atan2(s.dz, s.dx);
      poleM.push(trs(s.x, s.y, s.z, 0, 1, s.h, 1));
      const arm = s.arm ?? 1.8;
      if (arm > 0) armM.push(trs(s.x + s.dx * arm / 2, s.y + s.h - 0.1, s.z + s.dz * arm / 2, yaw, arm, 0.12, 0.12));
      lensM.push(trs(s.x + s.dx * arm, s.y + s.h - 0.25, s.z + s.dz * arm, yaw, 0.7, 0.12, 0.35));
      this.glows.push([s.x + s.dx * arm, s.y + s.h - 0.35, s.z + s.dz * arm, tint.r, tint.g, tint.b, 1]);
      poolM.push(trs(s.x + s.dx * (arm + 1.5), s.y + 0.09, s.z + s.dz * (arm + 1.5), 0, 13, 1, 13));
    }
    const metal = new THREE.MeshStandardMaterial({ color: 0x5c6066, metalness: 0.5, roughness: 0.6 });
    this.emitInstanced(pole, metal, poleM, {}, 1900);
    this.emitInstanced(new THREE.BoxGeometry(1, 1, 1), metal, armM, {}, 1900);
    const lens = new THREE.MeshBasicMaterial({ color: new THREE.Color(4, 2.9, 1.7) });
    this.emitInstanced(new THREE.BoxGeometry(1, 1, 1), lens, lensM, {}, 2600, tints.map((c) => c.clone()));
    const poolGeo = new THREE.PlaneGeometry(1, 1);
    poolGeo.rotateX(-Math.PI / 2);
    const poolMat = new THREE.MeshBasicMaterial({
      map: glowTexture(), color: new THREE.Color(1, 0.72, 0.42), transparent: true, depthWrite: false,
      blending: THREE.AdditiveBlending, polygonOffset: true, polygonOffsetFactor: -4, polygonOffsetUnits: -4,
    });
    for (const pools of this.emitInstanced(poolGeo, poolMat, poolM, {}, 2200, tints)) pools.renderOrder = 2;
    this.world.updaters.push((dt, night) => {
      const k = smoothstep(0.2, 0.8, night);
      poolMat.color.setRGB(0.26 * k, 0.25 * k, 0.24 * k);
      const L = 0.3 + 0.7 * k;
      lens.color.setRGB(4 * L, 4 * L, 4 * L);
    });
  }

  // White LED street lights downtown, sodium elsewhere.
  ledAt(x, z) {
    if (!this.loop) return !!this.DC && Math.hypot(x - this.DC.x, z - this.DC.z) < 750;
    for (const c of this.cores) if (Math.hypot(x - c.x, z - c.z) < c.R * 0.9) return true;
    return this.districtAt(this.nearestS(x, z)) === 'midrise';
  }

  // Halo sprites on every lamp head (street, freeway, helipads). Their
  // on-screen size never drops below a couple of pixels, so the street grid
  // and the freeway stay traced in light all the way to the horizon instead
  // of fading to black once the lamp meshes are too small to see.
  buildGlows() {
    const list = this.glows.slice();
    for (const h of this.fw.lampHeads || []) list.push([h.x, h.y - 0.3, h.z, h.c[0], h.c[1], h.c[2], 1.1]);
    for (const p of this.padLights) list.push([p[0], p[1], p[2], 0.3, 1.4, 0.5, 0.5]);
    for (const r of this.fw.reflectors || []) list.push(r[3] ? [r[0], r[1], r[2], 0.5, 0.3, 0.05, 0.1] : [r[0], r[1], r[2], 0.35, 0.35, 0.35, 0.1]);
    if (!list.length) return;
    const n = list.length;
    const pos = new Float32Array(n * 3), col = new Float32Array(n * 3), size = new Float32Array(n);
    list.forEach((g, i) => {
      pos.set([g[0], g[1], g[2]], i * 3);
      col.set([g[3], g[4], g[5]], i * 3);
      size[i] = g[6];
    });
    const geo = new THREE.BufferGeometry();
    geo.setAttribute('position', new THREE.BufferAttribute(pos, 3));
    geo.setAttribute('color', new THREE.BufferAttribute(col, 3));
    geo.setAttribute('gsize', new THREE.BufferAttribute(size, 1));
    geo.computeBoundingSphere();
    const mat = glowPointsMaterial(this.world, 2.0, 1.6);
    const pts = new THREE.Points(geo, mat);
    pts.frustumCulled = false;
    pts.renderOrder = 3;
    this.group.add(pts);
  }

  // Cars on the grid streets, as pairs of head- and tail-light sprites that
  // slide along each block in the vertex shader: one draw call brings the
  // whole city grid to life when seen from the freeway or the skyline.
  buildTraffic() {
    const segs = this.streetSegs, G = this.grid, rng = mulberry32(99);
    if (!segs?.length) return;
    const start = [], dir = [], par = [];
    const y = this.groundY + 0.75;
    for (const [u0, v0, u1, v1] of segs) {
      if (rng() < 0.25) continue; // quiet streets
      const a = G.toWorld(u0, v0), b = G.toWorld(u1, v1);
      const dx = b[0] - a[0], dz = b[1] - a[1], len = Math.hypot(dx, dz);
      const nx = -dz / len, nz = dx / len;
      for (const lane of [-1, 1]) {
        const cars = Math.floor(len / 55 + rng() * 2);
        // Lane on the right-hand side of travel; lane -1 runs backwards.
        const sx = lane > 0 ? a[0] : b[0], sz = lane > 0 ? a[1] : b[1];
        const ox = nx * 2.6 * -lane, oz = nz * 2.6 * -lane;
        const ddx = dx * lane, ddz = dz * lane;
        const speed = (7 + rng() * 7) / len;
        const ph0 = rng();
        for (let c = 0; c < cars; c++) {
          const ph = (ph0 + c / cars + rng() * 0.15) % 1;
          for (const tail of [0, 1]) {
            const back = tail ? -4.2 / len : 0;
            start.push(sx + ox + ddx * back, y, sz + oz + ddz * back);
            dir.push(ddx, 0, ddz);
            par.push(ph, speed, tail);
          }
        }
      }
    }
    if (!start.length) return;
    const geo = new THREE.BufferGeometry();
    geo.setAttribute('position', new THREE.Float32BufferAttribute(start, 3));
    geo.setAttribute('aDir', new THREE.Float32BufferAttribute(dir, 3));
    geo.setAttribute('aPar', new THREE.Float32BufferAttribute(par, 3));
    geo.boundingSphere = new THREE.Sphere(new THREE.Vector3(), 1e5);
    const u = { uTime: { value: 0 }, uNight: { value: 1 }, uFogK: { value: 0 }, uHalfH: { value: 360 } };
    const mat = new THREE.ShaderMaterial({
      uniforms: u, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending,
      vertexShader: `
        uniform float uTime, uFogK, uHalfH;
        attribute vec3 aDir;
        attribute vec3 aPar;
        varying vec3 vCol;
        varying float vA;
        void main() {
          float t = fract(aPar.x + uTime * aPar.y);
          vec4 mv = modelViewMatrix * vec4(position + aDir * t, 1.0);
          gl_Position = projectionMatrix * mv;
          float d = max(-mv.z, 0.1);
          float raw = 1.5 * projectionMatrix[1][1] * uHalfH / d;
          gl_PointSize = max(raw, 1.5);
          // Fade in/out at the block ends (junctions) and close to the camera.
          vA = sqrt(min(1.0, raw / 1.5)) * exp(-d * uFogK) * smoothstep(25.0, 70.0, d)
            * smoothstep(0.0, 0.06, t) * smoothstep(1.0, 0.94, t);
          vCol = aPar.z > 0.5 ? vec3(1.0, 0.1, 0.04) : vec3(1.0, 0.88, 0.66);
        }`,
      fragmentShader: `
        uniform float uNight;
        varying vec3 vCol;
        varying float vA;
        void main() {
          vec2 q = gl_PointCoord - 0.5;
          float a = exp(-dot(q, q) * 14.0) * vA * uNight;
          gl_FragColor = vec4(vCol * 2.2, a);
        }`,
    });
    const pts = new THREE.Points(geo, mat);
    pts.frustumCulled = false;
    pts.renderOrder = 3;
    this.group.add(pts);
    this.world.updaters.push((dt, night) => {
      u.uTime.value += dt;
      u.uNight.value = smoothstep(0.2, 0.7, night);
      u.uFogK.value = (this.world.scene.fog?.density ?? 0) * 0.4;
      u.uHalfH.value = this.world.renderer.domElement.height / 2;
    });
  }

  // Light pollution: a flattened dome over the city, seen from inside as a
  // warm haze low on the horizon and from outside as a glow hanging over the
  // skyline, so the gaps between buildings aren't dead black.
  buildSkyGlow() {
    const c = this.loop ? this.centre : this.DC;
    if (!c) return;
    let R = 0;
    if (this.loop) for (let i = 0; i < this.track.n; i += 50) R = Math.max(R, Math.hypot(this.track.px[i] - c.x, this.track.pz[i] - c.z));
    else R = 1400;
    R += 900;
    const geo = new THREE.SphereGeometry(1, 48, 12, 0, Math.PI * 2, 0, Math.PI / 2);
    geo.scale(R, R * 0.14, R);
    const u = { uK: { value: 0 }, uGround: { value: this.groundY }, uH: { value: R * 0.05 } };
    const mat = new THREE.ShaderMaterial({
      uniforms: u, side: THREE.BackSide, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending, fog: false,
      vertexShader: `
        varying float vY;
        void main() {
          vec4 w = modelMatrix * vec4(position, 1.0);
          vY = w.y;
          gl_Position = projectionMatrix * viewMatrix * w;
        }`,
      fragmentShader: `
        uniform float uK, uGround, uH;
        varying float vY;
        void main() {
          float h = max(vY - uGround, 0.0);
          float g = exp(-h / uH);
          vec3 col = mix(vec3(0.5, 0.26, 0.16), vec3(0.22, 0.14, 0.3), smoothstep(0.0, 2.5 * uH, h));
          gl_FragColor = vec4(col * g * uK, 1.0);
        }`,
    });
    const m = new THREE.Mesh(geo, mat);
    m.position.set(c.x, this.groundY - 10, c.z);
    m.frustumCulled = false;
    m.renderOrder = -1;
    this.group.add(m);
    this.world.updaters.push((dt, night) => { u.uK.value = 0.2 * smoothstep(0.3, 0.8, night); });
  }

  // Level 1: one InstancedMesh as before. Loop: split into spatial chunks
  // (so frustum culling works) that also switch off beyond `far` metres.
  emitInstanced(geo, mat, matrices, opts = {}, far = 2000, colors = null) {
    const out = [];
    if (!matrices.length) return out;
    if (!this.loop) {
      const im = instanced(geo, mat, matrices, opts);
      if (colors) colors.forEach((c, i) => im.setColorAt(i, c));
      this.group.add(im);
      out.push(im);
      return out;
    }
    const CH = 2000, groups = new Map();
    const e = new THREE.Vector3();
    matrices.forEach((m, i) => {
      e.setFromMatrixPosition(m);
      const key = Math.floor(e.x / CH) + ',' + Math.floor(e.z / CH);
      if (!groups.has(key)) groups.set(key, []);
      groups.get(key).push(i);
    });
    for (const idx of groups.values()) {
      const im = instanced(geo, mat, idx.map((i) => matrices[i]), opts);
      if (colors) idx.forEach((i, k) => im.setColorAt(k, colors[i]));
      this.group.add(im);
      this.fadeable(im, far);
      out.push(im);
    }
    return out;
  }

  // Hide a mesh when the camera is further than `far` from its bounds.
  fadeable(mesh, far) {
    if (!this._fade) {
      this._fade = [];
      const tmp = new THREE.Vector3();
      this.world.updaters.push((dt, night, camera) => {
        if (!camera) return;
        for (const f of this._fade) {
          const d = tmp.copy(f.c).distanceTo(camera.position) - f.r;
          f.m.visible = d < f.far;
        }
      });
    }
    const bs = mesh.geometry.boundingSphere || (mesh.geometry.computeBoundingSphere(), mesh.geometry.boundingSphere);
    const sph = mesh.isInstancedMesh ? (mesh.computeBoundingSphere(), mesh.boundingSphere) : bs;
    this._fade.push({ m: mesh, c: sph.center.clone(), r: sph.radius, far });
  }

  buildTrees() {
    const trees = this.parkTrees.concat(this.fw.parkTrees || []);
    if (!trees.length) return;
    const trunk = new THREE.CylinderGeometry(0.18, 0.26, 3.2, 5);
    trunk.translate(0, 1.6, 0);
    const crown = new THREE.IcosahedronGeometry(2.4, 0);
    crown.scale(1, 1.15, 1);
    crown.translate(0, 4.6, 0);
    const paint = (g0, c) => {
      const g = g0.index ? g0.toNonIndexed() : g0;
      const n = g.getAttribute('position').count;
      const a = new Float32Array(n * 3);
      for (let i = 0; i < n; i++) { a[i * 3] = c[0]; a[i * 3 + 1] = c[1]; a[i * 3 + 2] = c[2]; }
      g.setAttribute('color', new THREE.BufferAttribute(a, 3));
      return g;
    };
    const g1 = paint(trunk, [0.22, 0.14, 0.08]);
    const g2 = paint(crown, [0.13, 0.26, 0.09]);
    const geo = mergeTwo(g1, g2);
    geo.computeVertexNormals();
    const mat = new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.95, flatShading: true });
    const rng = this.rng;
    const ms = trees.map((t) => trs(t.x, t.y, t.z, rng() * 6.28, t.s, t.s * (0.85 + rng() * 0.3), t.s));
    const cols = ms.map(() => new THREE.Color().setHSL(0.25 + rng() * 0.08, 0.5, 0.35 + rng() * 0.25));
    this.emitInstanced(geo, mat, ms, { cast: true, receive: true }, 1700, cols);
  }

  buildAircraftLights() {
    if (!this.aircraft.length) return;
    const pos = new Float32Array(this.aircraft.length * 3);
    this.aircraft.forEach((p, i) => { pos[i * 3] = p[0]; pos[i * 3 + 1] = p[1]; pos[i * 3 + 2] = p[2]; });
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.BufferAttribute(pos, 3));
    g.computeBoundingSphere();
    const m = new THREE.PointsMaterial({
      size: 3.5, sizeAttenuation: false, map: glowTexture(), color: new THREE.Color(6, 0.35, 0.25),
      transparent: true, depthWrite: false, blending: THREE.AdditiveBlending,
    });
    const pts = new THREE.Points(g, m);
    pts.frustumCulled = false;
    this.group.add(pts);
    let time = 0;
    this.world.updaters.push((dt, night) => {
      time += dt;
      const on = (Math.sin(time * Math.PI * 1.6) > 0.2 ? 1 : 0.08) * (0.3 + 0.7 * night);
      m.color.setRGB(6 * on, 0.35 * on, 0.25 * on);
    });
  }
}

// Additive halo sprites with a per-point colour and size (metres) that
// never shrink below minPx on screen; dimmer when clamped so far lamps
// don't outshine near ones. Fog is replaced by a gentler fade: lights carry
// much further through haze than the geometry they sit on.
function glowPointsMaterial(world, size, minPx) {
  const m = new THREE.PointsMaterial({
    size, sizeAttenuation: true, map: glowTexture(), vertexColors: true, transparent: true,
    depthWrite: false, blending: THREE.AdditiveBlending, fog: false,
  });
  const u = { uMinPx: { value: minPx }, uFogK: { value: 0 } };
  m.onBeforeCompile = (sh) => {
    Object.assign(sh.uniforms, u);
    sh.vertexShader = sh.vertexShader
      .replace('#include <common>', '#include <common>\nattribute float gsize;\nuniform float uMinPx;\nuniform float uFogK;\nvarying float vGA;')
      .replace('gl_PointSize = size;', 'gl_PointSize = size * gsize;')
      .replace('#include <logdepthbuf_vertex>', `float gRaw = gl_PointSize;
gl_PointSize = max(gRaw, uMinPx);
vGA = sqrt(gRaw / gl_PointSize) * exp(-max(-mvPosition.z, 0.0) * uFogK);
#include <logdepthbuf_vertex>`);
    sh.fragmentShader = sh.fragmentShader
      .replace('#include <common>', '#include <common>\nvarying float vGA;')
      .replace('#include <premultiplied_alpha_fragment>', 'gl_FragColor.a *= vGA;\n#include <premultiplied_alpha_fragment>');
  };
  m.customProgramCacheKey = () => 'city-glow';
  world.updaters.push((dt, night) => {
    m.color.setScalar(1.6 * smoothstep(0.2, 0.7, night));
    u.uFogK.value = (world.scene.fog?.density ?? 0) * 0.4;
  });
  return m;
}

function hash2(x, z) {
  const v = Math.sin(x * 12.9898 + z * 78.233) * 43758.5453;
  return v - Math.floor(v);
}

function normalY(a, b, c) {
  const abx = b[0] - a[0], abz = b[2] - a[2], acx = c[0] - a[0], acz = c[2] - a[2];
  return abz * acx - abx * acz;
}

function circle(x, z, r, n) {
  const out = [];
  for (let k = 0; k < n; k++) { const a = (k / n) * Math.PI * 2; out.push([x + Math.cos(a) * r, z + Math.sin(a) * r]); }
  return out;
}

// Walls facing into a closed footprint (the inside of a parapet).
function wallsInward(B, pts, y0, y1, cell) {
  let area = 0;
  for (let i = 0; i < pts.length; i++) { const p = pts[i], q = pts[(i + 1) % pts.length]; area += p[0] * q[1] - q[0] * p[1]; }
  const C = area > 0 ? pts.slice().reverse() : pts;
  for (let i = 0; i < C.length; i++) {
    const p = C[i], q = C[(i + 1) % C.length];
    B.quad([q[0], y0, q[1]], [p[0], y0, p[1]], [p[0], y1, p[1]], [q[0], y1, q[1]], [[0, 0], [1, 0], [1, 0.2], [0, 0.2]], null, cell);
  }
}

// Horizontal quad forced to face up (or down).
function upQuad(B, a, b, c, d, cell = 0, col = null, down = false) {
  if ((normalY(a, b, c) >= 0) !== down) B.quad(a, b, c, d, null, col, cell);
  else B.quad(a, d, c, b, null, col, cell);
}

// Push each corner outward from the centre by `d` metres.
function growRect(rect, c, d) {
  return rect.map((p) => {
    const dx = p[0] - c[0], dz = p[1] - c[1], l = Math.hypot(dx, dz) || 1;
    return [p[0] + (dx / l) * d * 1.41, p[1] + (dz / l) * d * 1.41];
  });
}

function mergeTwo(a, b) {
  const g = new THREE.BufferGeometry();
  for (const name of ['position', 'normal', 'color']) {
    const A = a.getAttribute(name), Bb = b.getAttribute(name);
    if (!A || !Bb) continue;
    const arr = new Float32Array(A.array.length + Bb.array.length);
    arr.set(A.array, 0);
    arr.set(Bb.array, A.array.length);
    g.setAttribute(name, new THREE.BufferAttribute(arr, A.itemSize));
  }
  return g;
}

// Merge indexed/non-indexed geometries into one non-indexed geometry
// (position + normal only).
function mergeFlatGeos(geos) {
  const flat = geos.map((g) => (g.index ? g.toNonIndexed() : g));
  const out = new THREE.BufferGeometry();
  for (const n of ['position', 'normal']) {
    const total = flat.reduce((a, g) => a + g.getAttribute(n).array.length, 0);
    const arr = new Float32Array(total);
    let o = 0;
    for (const g of flat) { arr.set(g.getAttribute(n).array, o); o += g.getAttribute(n).array.length; }
    out.setAttribute(n, new THREE.BufferAttribute(arr, 3));
  }
  out.computeBoundingSphere();
  return out;
}

// A set of GeoBuilders keyed by spatial chunk; call quad/prism/box as on a
// single builder and each primitive lands in the chunk of its first vertex.
class ChunkedGeo {
  constructor(size, opts) { this.size = size; this.opts = opts; this.map = new Map(); }
  of(x, z) {
    const key = Math.floor(x / this.size) + ',' + Math.floor(z / this.size);
    let b = this.map.get(key);
    if (!b) this.map.set(key, (b = new GeoBuilder(this.opts)));
    return b;
  }
  quad(a, ...rest) { this.of(a[0], a[2]).quad(a, ...rest); }
  tri(a, ...rest) { this.of(a[0], a[2]).tri(a, ...rest); }
  prism(c, ...rest) { this.of(c[0][0], c[0][1]).prism(c, ...rest); }
  box(x, y, z, ...rest) { this.of(x, z).box(x, y, z, ...rest); }
  builders() { return [...this.map.values()].filter((b) => !b.empty); }
}
