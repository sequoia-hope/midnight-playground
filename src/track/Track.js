import { ROAD_TYPES, ROAD_KEYS } from './roadTypes.js';
import { clamp, smoothstep, DEG } from '../util/math.js';

// The track is sampled every metre. Everything else — road mesh, terrain,
// scenery placement, physics, AI — reads these arrays, so a level's route is
// the single source of truth for its shape.
//
// Two kinds of route:
//   point-to-point: a list of turtle segments (length, turn, climb)
//   loop:           a closed polyline, resampled to 1 m; s wraps around

function gaussianSmooth(src, sigma, out = new Float32Array(src.length), wrap = false) {
  const r = Math.ceil(sigma * 3);
  const w = new Float32Array(r * 2 + 1);
  let sum = 0;
  for (let k = -r; k <= r; k++) { w[k + r] = Math.exp(-(k * k) / (2 * sigma * sigma)); sum += w[k + r]; }
  for (let k = 0; k < w.length; k++) w[k] /= sum;
  const n = src.length;
  const tmp = out === src ? new Float32Array(n) : out;
  for (let i = 0; i < n; i++) {
    let acc = 0;
    for (let k = -r; k <= r; k++) {
      const j = wrap ? ((i + k) % n + n) % n : clamp(i + k, 0, n - 1);
      acc += src[j] * w[k + r];
    }
    tmp[i] = acc;
  }
  if (tmp !== out) out.set(tmp);
  return out;
}

// Circuits with surveyed run-off: metres past the tarmac edge before the
// ground leaves the road's plane (see surfaceY).
export const RUNOFF_FLAT = 1.5;

// Smoothed trapezoid in [0,1]: ramps over `r` at each end. Integral = 1 - r.
function trapezoid(t, r) {
  if (t < r) return smoothstep(0, r, t);
  if (t > 1 - r) return smoothstep(0, r, 1 - t);
  return 1;
}

export class Track {
  constructor(level) {
    this.level = level;
    this.loop = !!level.loop;
    this.build();
  }

  build() {
    const L = this.level;
    const raw = this.loop ? this.buildLoop(L.loop) : this.walkSegments(L);
    const { n, px, pz, yRaw, kappa, zone, roadType, elevRaw } = raw;
    const W = this.loop;
    this.n = n;
    this.length = this.loop ? n : n - 1;
    // Laps of a circuit (a loop raced to a finish); 0 for everything else.
    this.laps = this.loop ? L.laps || 0 : 0;

    // Elevation: smooth crest/sag kinks.
    const py = gaussianSmooth(yRaw, L.elevationSmooth ?? L.loop?.elevationSmooth ?? 9, new Float32Array(n), W);
    const grade = new Float32Array(n);
    for (let k = 0; k < n; k++) {
      const a = W ? (k - 1 + n) % n : Math.max(0, k - 1), b = W ? (k + 1) % n : Math.min(n - 1, k + 1);
      grade[k] = (py[b] - py[a]) / (W || (k > 0 && k < n - 1) ? 2 : 1);
    }

    // Frames.
    const fx = new Float32Array(n), fz = new Float32Array(n);
    const rx = new Float32Array(n), rz = new Float32Array(n);
    for (let k = 0; k < n; k++) {
      const a = W ? (k - 1 + n) % n : Math.max(0, k - 1), b = W ? (k + 1) % n : Math.min(n - 1, k + 1);
      let dx = px[b] - px[a], dz = pz[b] - pz[a];
      const l = Math.hypot(dx, dz) || 1;
      dx /= l; dz /= l;
      fx[k] = dx; fz[k] = dz;
      rx[k] = -dz; rz[k] = dx;
    }
    // Loops get their curvature from the geometry.
    if (this.loop) {
      for (let k = 0; k < n; k++) {
        const a = (k - 1 + n) % n, b = (k + 1) % n;
        const h0 = Math.atan2(fz[a], fx[a]), h1 = Math.atan2(fz[b], fx[b]);
        let d = h1 - h0;
        while (d > Math.PI) d -= Math.PI * 2;
        while (d < -Math.PI) d += Math.PI * 2;
        kappa[k] = d / 2;
      }
    }

    // Width, barriers and banking.
    const hwRaw = new Float32Array(n), marginRaw = new Float32Array(n), bankK = new Float32Array(n);
    for (let k = 0; k < n; k++) {
      const rt = ROAD_TYPES[ROAD_KEYS[roadType[k]]];
      hwRaw[k] = rt.hw;
      marginRaw[k] = rt.margin;
      bankK[k] = rt.bank ?? 1; // kerbed streets stay level across
    }
    // A surveyed road brings its own width.
    const hw = raw.hw ? gaussianSmooth(raw.hw, 3, undefined, W) : gaussianSmooth(hwRaw, 40, undefined, W);
    const margin = gaussianSmooth(marginRaw, 40, undefined, W);
    const kSmooth = gaussianSmooth(kappa, 10, undefined, W);
    const bankRaw = new Float32Array(n);
    for (let k = 0; k < n; k++) bankRaw[k] = clamp(kSmooth[k] * 40, -0.085, 0.085) * bankK[k];
    // A surveyed road brings its own camber.
    const bank = raw.bank ? gaussianSmooth(raw.bank, 2, undefined, W) : gaussianSmooth(bankRaw, 12, undefined, W);

    Object.assign(this, { px, py, pz, kappa, kSmooth, grade, zone, roadType, fx, fz, rx, rz, hw, margin, bank });
    // Surveyed run-off (circuits): past the tarmac edge the ground leaves
    // the road's plane at its own grade, rise per metre outward (see
    // surfaceY). null elsewhere: the road's plane carries on.
    this.runL = raw.runL || null;
    this.runR = raw.runR || null;
    // How loose the ground is at (x, z) off the tarmac: 0 paved (asphalt
    // run-off, which drives like the road), 1 dirt or grass (slows the car).
    // null: all of it is loose.
    this.looseAt = L.looseGround ? (x, z) => L.looseGround(x, z) : null;
    this.elevated = elevRaw;

    // Collision limits either side of the centreline (positive distances).
    this.wallL = new Float32Array(n);
    this.wallR = new Float32Array(n);
    for (let k = 0; k < n; k++) {
      // Surveyed barriers (a circuit's walls), never closer than a metre
      // and a half off the tarmac.
      this.wallL[k] = raw.wallL ? Math.max(hw[k] + 1.5, raw.wallL[k]) : hw[k] + margin[k];
      this.wallR[k] = raw.wallR ? Math.max(hw[k] + 1.5, raw.wallR[k]) : hw[k] + margin[k];
    }

    // Zones.
    const Z = L.zones.length;
    this.zoneStart = new Array(Z).fill(0);
    for (let k = 1; k < n; k++) if (zone[k] !== zone[k - 1]) this.zoneStart[zone[k]] = k;
    this.zones = L.zones.map((z, idx) => ({
      ...z, id: idx,
      s0: this.zoneStart[idx],
      s1: idx < Z - 1 ? this.zoneStart[idx + 1] : this.length,
    }));
    this.finishS = this.loop ? Infinity : this.length - (L.finishRunoff ?? 180);
    this.startS = this.loop ? L.loop.startS ?? 120 : 60;
    // Metres of straight road past the last sample. Scenery that draws the
    // road carrying on beyond the end sets this in plan(); cars can drive it.
    this.runout = 0;
    // Openings in roadside fences (driveways, side roads): {s0, s1, side}.
    // Scenery registers these in plan(), before the road is built.
    this.fenceGaps = [];

    this.buildSpatialHash();
    this.buildRacingLine();
  }

  walkSegments(L) {
    const SEG = L.segments;
    const totalLen = Math.round(SEG.reduce((a, s) => a + s[0], 0));
    const n = totalLen + 1;
    const px = new Float32Array(n), pz = new Float32Array(n);
    const kappa = new Float32Array(n), gradeRaw = new Float32Array(n);
    const zone = new Uint8Array(n), roadType = new Uint8Array(n), elevRaw = new Uint8Array(n);
    this.tags = [];
    let x = L.startX ?? 0, z = L.startZ ?? 0, h = (L.startHeading || 0) * DEG;
    let curZone = 0, curRoad = SEG[0][3]?.road || 'mountain';
    let i = 0;
    px[0] = x; pz[0] = z;
    for (const seg of SEG) {
      const [len, turnDeg, rise, extra = {}] = seg;
      if (extra.zone !== undefined) curZone = extra.zone;
      if (extra.road) curRoad = extra.road;
      const s0 = i;
      if (extra.tag) this.tags.push({ tag: extra.tag, s0, s1: s0 + len, turn: turnDeg });
      const r = Math.abs(turnDeg) > 120 ? 0.22 : 0.32;
      const kPeak = (turnDeg * DEG) / (len * (1 - r));
      for (let k = 0; k < len; k++) {
        const t = (k + 0.5) / len;
        const kap = kPeak * trapezoid(t, r);
        kappa[i] = kap;
        gradeRaw[i] = rise / len;
        zone[i] = curZone;
        roadType[i] = ROAD_KEYS.indexOf(curRoad);
        elevRaw[i] = extra.elevated ? 1 : 0;
        h += kap;
        x += Math.cos(h - kap * 0.5);
        z += Math.sin(h - kap * 0.5);
        i++;
        px[i] = x; pz[i] = z;
      }
    }
    kappa[n - 1] = 0;
    gradeRaw[n - 1] = gradeRaw[n - 2];
    zone[n - 1] = zone[n - 2];
    roadType[n - 1] = roadType[n - 2];
    elevRaw[n - 1] = elevRaw[n - 2];
    const yRaw = new Float32Array(n);
    if (L.elevation) {
      // The level owns the ground (Downtown Streets): the road sits on it.
      for (let k = 0; k < n; k++) yRaw[k] = L.elevation(px[k], pz[k]);
    } else {
      yRaw[0] = L.startHeight ?? 100;
      for (let k = 1; k < n; k++) yRaw[k] = yRaw[k - 1] + gradeRaw[k - 1];
    }
    return { n, px, pz, yRaw, kappa, zone, roadType, elevRaw };
  }

  buildLoop(spec) {
    // path() gives the closed centreline, and optionally surveyed heights,
    // camber, half-widths and barrier distances at the same points.
    const P = spec.path();
    const { x: X, z: Zs } = P;
    const m = X.length;
    // Cumulative arc length around the closed polyline.
    const cum = new Float64Array(m + 1);
    for (let k = 0; k < m; k++) {
      const j = (k + 1) % m;
      cum[k + 1] = cum[k] + Math.hypot(X[j] - X[k], Zs[j] - Zs[k]);
    }
    const total = cum[m];
    const n = Math.round(total);
    const step = total / n;
    const px = new Float32Array(n), pz = new Float32Array(n);
    const extra = ['y', 'bank', 'hw', 'wallL', 'wallR', 'runL', 'runR'].filter((k) => P[k]);
    const ex = Object.fromEntries(extra.map((k) => [k, new Float32Array(n)]));
    let seg = 0;
    for (let i = 0; i < n; i++) {
      const d = i * step;
      while (cum[seg + 1] < d) seg++;
      const t = (d - cum[seg]) / (cum[seg + 1] - cum[seg]);
      const j = (seg + 1) % m;
      px[i] = X[seg] + (X[j] - X[seg]) * t;
      pz[i] = Zs[seg] + (Zs[j] - Zs[seg]) * t;
      for (const k of extra) ex[k][i] = P[k][seg] + (P[k][j] - P[k][seg]) * t;
    }
    // Tags by fraction (f0/f1) or metres (s0/s1); elevated ones lift the
    // road with long ramps.
    this.tags = [];
    const yRaw = ex.y || new Float32Array(n).fill(spec.baseY ?? 0);
    const elevRaw = new Uint8Array(n);
    for (const tg of spec.tags || []) {
      const s0 = Math.round(tg.s0 ?? tg.f0 * n), s1 = Math.round(tg.s1 ?? tg.f1 * n);
      this.tags.push({ tag: tg.tag, s0, s1, turn: 0 });
      if (tg.elevated) {
        const ramp = 260;
        this.tags.push({ tag: tg.tag + '-up', s0: s0 - ramp, s1: s0 });
        this.tags.push({ tag: tg.tag + '-down', s0: s1, s1: s1 + ramp });
        for (let s = s0 - ramp; s <= s1 + ramp; s++) {
          const k = ((s % n) + n) % n;
          const up = smoothstep(s0 - ramp, s0, s), down = 1 - smoothstep(s1, s1 + ramp, s);
          yRaw[k] += tg.elevated * Math.min(up, down);
          if (yRaw[k] > (spec.baseY ?? 0) + 1.4) elevRaw[k] = 1;
        }
      }
    }
    const kappa = new Float32Array(n);
    // Zones start at the given metres into the lap (zone 0 from s = 0).
    const zone = new Uint8Array(n);
    const starts = spec.zones || [0];
    for (let i = 0; i < n; i++) {
      let z = 0;
      while (z + 1 < starts.length && i >= starts[z + 1]) z++;
      zone[i] = z;
    }
    const roadType = new Uint8Array(n).fill(ROAD_KEYS.indexOf(spec.road || 'freeway'));
    for (const r of spec.roads || []) {
      for (let s = r.s0; s < r.s1; s++) roadType[((s % n) + n) % n] = ROAD_KEYS.indexOf(r.road);
    }
    return { n, px, pz, yRaw, kappa, zone, roadType, elevRaw, bank: ex.bank, hw: ex.hw, wallL: ex.wallL, wallR: ex.wallR, runL: ex.runL, runR: ex.runR };
  }

  tag(name) { return this.tags.filter((t) => t.tag === name); }

  // How far along a point-to-point road a car can go, runout included.
  get roadEnd() { return this.loop ? Infinity : this.length + this.runout; }

  // ── Queries ─────────────────────────────────────────────────────
  wrap(s) {
    if (!this.loop) return s;
    const n = this.n;
    return ((s % n) + n) % n;
  }

  // Signed shortest distance from a to b along the road.
  ds(a, b) {
    let d = b - a;
    if (this.loop) {
      const n = this.n;
      d = ((d % n) + n) % n;
      if (d > n / 2) d -= n;
    }
    return d;
  }

  idx(s) {
    if (this.loop) { const n = this.n; return ((Math.round(s) % n) + n) % n; }
    return clamp(Math.round(s), 0, this.n - 1);
  }

  // Integer sample i and fraction t for arc length s.
  locate(s) {
    const n = this.n;
    if (this.loop) {
      s = ((s % n) + n) % n;
      const i = Math.floor(s);
      return [i, s - i, (i + 1) % n, s];
    }
    s = clamp(s, 0, n - 1.001);
    const i = Math.floor(s);
    return [i, s - i, i + 1, s];
  }

  // Interpolated centreline frame at arc length s.
  frame(s, out = {}) {
    const [i, t, j, sw] = this.locate(s);
    const L = (a) => a[i] + (a[j] - a[i]) * t;
    out.x = L(this.px); out.y = L(this.py); out.z = L(this.pz);
    const fx = L(this.fx), fz = L(this.fz);
    const l = Math.hypot(fx, fz) || 1;
    out.fx = fx / l; out.fz = fz / l;
    out.rx = -out.fz; out.rz = out.fx;
    out.hw = L(this.hw);
    out.bank = L(this.bank);
    out.grade = L(this.grade);
    out.kappa = L(this.kSmooth);
    out.wallL = L(this.wallL);
    out.wallR = L(this.wallR);
    out.zone = this.zone[i];
    out.s = sw;
    // Past the last sample the road carries straight on (the runout).
    if (!this.loop && s > sw + 0.01) {
      const d = s - sw;
      out.x += out.fx * d; out.z += out.fz * d;
      out.s = s;
    }
    return out;
  }

  // World position of a point at (s, lateral offset) on the road surface.
  pointAt(s, lat, out = {}) {
    const f = this.frame(s, out);
    const x = f.x + f.rx * lat, z = f.z + f.rz * lat;
    out.x = x; out.z = z;
    out.y = this.runL && Math.abs(lat) > f.hw + RUNOFF_FLAT ? this.surfaceY(s, lat) : f.y - lat * f.bank;
    return out;
  }

  surfaceY(s, lat) {
    const [i, t, j] = this.locate(s);
    const y = this.py[i] + (this.py[j] - this.py[i]) * t;
    const b = this.bank[i] + (this.bank[j] - this.bank[i]) * t;
    if (this.runL) {
      // The road's plane carries on RUNOFF_FLAT past the edge (the verge),
      // then the ground takes its own grade.
      const hw = this.hw[i] + (this.hw[j] - this.hw[i]) * t + RUNOFF_FLAT, a = Math.abs(lat);
      if (a > hw) {
        const r = lat > 0 ? this.runR : this.runL;
        const e = lat > 0 ? hw : -hw;
        return y - e * b + (a - hw) * (r[i] + (r[j] - r[i]) * t);
      }
    }
    return y - lat * b;
  }

  // Local projection of (x,z) onto the centreline, searching near `hint`.
  // Returns {s, lat, i}.
  project(x, z, hint, out = {}, window = 12) {
    const n = this.n;
    let best = -1, bd = Infinity, edge = false;
    const h = this.loop ? Math.floor(hint) : clamp(Math.floor(hint), 0, n - 1);
    if (this.loop) {
      for (let o = -window; o <= window; o++) {
        const k = (((h + o) % n) + n) % n;
        const dx = x - this.px[k], dz = z - this.pz[k];
        const d = dx * dx + dz * dz;
        if (d < bd) { bd = d; best = k; edge = Math.abs(o) === window; }
      }
    } else {
      const lo = Math.max(0, h - window), hi = Math.min(n - 1, h + window);
      for (let k = lo; k <= hi; k++) {
        const dx = x - this.px[k], dz = z - this.pz[k];
        const d = dx * dx + dz * dz;
        if (d < bd) { bd = d; best = k; }
      }
      edge = (best === lo && lo > 0) || (best === hi && hi < n - 1);
    }
    // If the best is at the window's edge we may be far off; widen.
    if (edge && window < 200) return this.project(x, z, best, out, window * 4);
    const dx = x - this.px[best], dz = z - this.pz[best];
    const along = dx * this.fx[best] + dz * this.fz[best];
    out.s = this.loop ? this.wrap(best + along) : clamp(best + along, 0, n - 1 + this.runout);
    out.lat = dx * this.rx[best] + dz * this.rz[best];
    out.i = best;
    return out;
  }

  buildSpatialHash() {
    this.cell = 32;
    this.hash = new Map();
    for (let k = 0; k < this.n; k += 2) {
      const key = Math.floor(this.px[k] / this.cell) + ',' + Math.floor(this.pz[k] / this.cell);
      let a = this.hash.get(key);
      if (!a) this.hash.set(key, (a = []));
      a.push(k);
    }
    let minX = Infinity, maxX = -Infinity, minZ = Infinity, maxZ = -Infinity;
    for (let k = 0; k < this.n; k++) {
      minX = Math.min(minX, this.px[k]); maxX = Math.max(maxX, this.px[k]);
      minZ = Math.min(minZ, this.pz[k]); maxZ = Math.max(maxZ, this.pz[k]);
    }
    this.bounds = { minX, maxX, minZ, maxZ };
  }

  // Nearest centreline sample anywhere (radius-limited). Returns index or -1.
  nearest(x, z, radius = 96) {
    const c = this.cell, rc = Math.ceil(radius / c);
    const cx = Math.floor(x / c), cz = Math.floor(z / c);
    let best = -1, bd = radius * radius;
    for (let a = -rc; a <= rc; a++) for (let b = -rc; b <= rc; b++) {
      const arr = this.hash.get((cx + a) + ',' + (cz + b));
      if (!arr) continue;
      for (const k of arr) {
        const dx = x - this.px[k], dz = z - this.pz[k];
        const d = dx * dx + dz * dz;
        if (d < bd) { bd = d; best = k; }
      }
    }
    return best;
  }

  // Distance from (x,z) to the nearest bit of road, and which sample.
  distanceToRoad(x, z, radius = 96) {
    const k = this.nearest(x, z, radius);
    if (k < 0) return { d: Infinity, i: -1, lat: 0 };
    const p = this.project(x, z, k, {}, 4);
    return { d: Math.abs(p.lat), i: p.i, lat: p.lat, s: p.s };
  }

  // ── AI helpers: racing line and target-speed profile ───────────
  buildRacingLine() {
    const n = this.n, W = this.loop;
    // Aim for the inside of corners: offset proportional to smoothed
    // curvature, looked-ahead a little so the apex is early-ish.
    const k2 = gaussianSmooth(this.kappa, 35, undefined, W);
    const line = new Float32Array(n);
    for (let i = 0; i < n; i++) {
      const j = W ? (i + 12) % n : Math.min(n - 1, i + 12);
      const lim = this.hw[i] - 1.6;
      line[i] = clamp(k2[j] * 420, -lim, lim);
    }
    this.racingLine = gaussianSmooth(line, 18, undefined, W);
    // Smoothing can carry the line past a narrowing (circuit straights
    // are wider than their corners): hold it on the tarmac.
    for (let i = 0; i < n; i++) this.racingLine[i] = clamp(this.racingLine[i], -(this.hw[i] - 1.6), this.hw[i] - 1.6);

    // Speed a well-driven car can carry through each metre.
    const vmax = new Float32Array(n);
    const aLat = 15.5, vTop = 69.5;
    for (let i = 0; i < n; i++) {
      const k = Math.abs(this.kSmooth[i]) + 1e-5;
      vmax[i] = Math.min(vTop, Math.sqrt(aLat / k));
    }
    // Backward pass: you have to brake before a corner (twice round a loop).
    const brake = 11;
    const passes = W ? 2 : 1;
    for (let p = 0; p < passes; p++) {
      for (let i = n - 2 + (W ? 1 : 0); i >= 0; i--) {
        const j = W ? (i + 1) % n : i + 1;
        vmax[i] = Math.min(vmax[i], Math.sqrt(vmax[j] * vmax[j] + 2 * brake));
      }
    }
    this.speedProfile = vmax;
  }

  // Blend weights across zones at s (one per zone, summing to 1).
  zoneBlend(s, width = 250) {
    const Z = this.zoneStart.length;
    const w = new Array(Z).fill(0);
    if (Z === 1) { w[0] = 1; return w; }
    let prev = 1;
    for (let z = 0; z < Z; z++) {
      const t = z + 1 < Z ? smoothstep(this.zoneStart[z + 1] - width, this.zoneStart[z + 1] + width, s) : 0;
      w[z] = prev - t;
      prev = t;
    }
    return w;
  }
}
