import { clamp, lerp, smoothstep, makeNoise2D, fbm, ridged, hash2 } from '../util/math.js';
import { RUNOFF_FLAT } from '../track/Track.js';

// Heightfield terrain sculpted around the road.
//
// Two helper fields are rasterised from the track first:
//   far field  (32 m grid, whole map): distance to road, nearest road height,
//              nearest s and which side — drives the large landforms.
//   near field (4 m grid, sparse tiles hugging the road): how strongly the
//              road flattens the ground (K) and the road height to flatten to.
// Final height = lerp(zone landform, road height, K). Everything is
// continuous, so switchbacks stacked up a face turn into a smooth slope.

const FAR = 32;          // far-field cell size (m)
const NEAR = 4;          // near-field node spacing (m)
const NT = 64;           // near-field nodes per tile side
const TILE = NEAR * NT;  // 256 m
const FORM_FN = {
  mountain: 'formMountain', valley: 'formValley', city: 'formCity', coast: 'formCoast', beach: 'formBeach', harbor: 'formHarbor',
  streets: 'formStreets', canyon: 'formCanyon', desert: 'formDesert', playa: 'formPlaya',
  raceway: 'formRaceway',
};

// Per-landform near-road shaping: r0 = flat shoulder past the paved edge,
// band = distance over which ground returns to the landform, pow < 1 makes
// the ground climb straight away (rock walls). flat = level ground (city,
// port) where raised road sections stand on piers instead of embankments.
export const LANDFORMS = {
  mountain: { r0: 2.4, band: 38, pow: 0.6 },
  valley: { r0: 2.6, band: 60, pow: 1 },
  city: { r0: 1.5, band: 14, pow: 1, flat: true },
  coast: { r0: 2.2, band: 30, pow: 0.7 },
  beach: { r0: 3.0, band: 36, pow: 1 },
  harbor: { r0: 2.0, band: 18, pow: 1, flat: true },
  // Downtown Streets: the level supplies the ground (flat blocks and a
  // steep hill district); the road only trims it at the kerbs.
  streets: { r0: 1.0, band: 10, pow: 1 },
  canyon: { r0: 2.2, band: 20, pow: 0.7 },
  desert: { r0: 3.0, band: 40, pow: 1 },
  playa: { r0: 6.0, band: 60, pow: 1 },
  // Seaside Raceway: the level supplies surveyed ground. Inside the
  // barriers (the track's wallL/wallR) the run-off is graded flush with the
  // tarmac, so the car drives on what you see; outside it's the real land.
  raceway: { r0: 0.5, band: 7, pow: 1, corridor: true },
};

export class Terrain {
  constructor(track, level = track.level, opts = {}) {
    this.track = track;
    this.level = level;
    this.noise = makeNoise2D(opts.seed ?? 7);
    this.noise2 = makeNoise2D((opts.seed ?? 7) + 101);
    this.flattens = [];
    this.carves = [];
    this.margin = opts.margin ?? 2600;
    const b = track.bounds;
    this.minX = Math.floor((b.minX - this.margin) / TILE) * TILE;
    this.minZ = Math.floor((b.minZ - this.margin) / TILE) * TILE;
    this.maxX = Math.ceil((b.maxX + this.margin) / TILE) * TILE;
    this.maxZ = Math.ceil((b.maxZ + this.margin) / TILE) * TILE;
    this.seaY = level.sea ? level.sea.y : null;

    // Zones and their transition lines (by x — routes run broadly west→east).
    const t = track;
    this.forms = level.zones.map((z) => z.landform);
    this.lf = this.forms.map((k) => LANDFORMS[k]);
    this.edges = level.zones.slice(1).map((z, k) => ({
      x: t.px[t.zoneStart[k + 1]] + (z.blendOffset || 0),
      w: z.blend || 300,
    }));
    // Level ground for flat landforms: the lowest road not on a structure.
    this.flatY = this.forms.map((f, zi) => {
      if (!LANDFORMS[f].flat) return null;
      let lo = Infinity;
      for (let i = 0; i < t.n; i++) if (t.zone[i] === zi && !t.elevated[i]) lo = Math.min(lo, t.py[i]);
      return lo;
    });
    // Older scenery reads cityY: the first flat zone's ground.
    this.cityY = this.flatY.find((y) => y !== null) ?? 0;
    // Shipping channels: water under bridge spans (harbour).
    this.channels = t.tags.filter((g) => g.tag === 'bridge').map((g) => ({ s0: g.s0 + 50, s1: g.s1 - 50 }));
  }

  // Zone boundary x positions (Level 1 scenery reads these by name).
  get x1() { return this.edges[0]?.x ?? Infinity; }
  get x2() { return this.edges[1]?.x ?? Infinity; }

  // Road on a bridge/viaduct at sample i: the ground isn't raised to it.
  isElevated(i) {
    const t = this.track;
    if (t.elevated[i]) return true;
    const fy = this.flatY[t.zone[i]];
    return fy !== null && fy !== undefined && t.py[i] > fy + 1.2;
  }

  // Flatten a disc to height y (or to the terrain's own height at its centre
  // when y is omitted). Must be registered before build().
  addFlatten(x, z, r, falloff = 20, y = null) {
    this.flattens.push({ x, z, r, falloff, y });
  }

  // Carve a channel along a polyline [{x,z}...].
  // opts.underRoad: apply after the road blend, so the channel passes under
  // the road deck (a bridge) instead of being filled in by the road.
  addCarve(points, width, depth, opts = {}) {
    let minX = Infinity, maxX = -Infinity, minZ = Infinity, maxZ = -Infinity;
    for (const p of points) {
      minX = Math.min(minX, p.x); maxX = Math.max(maxX, p.x);
      minZ = Math.min(minZ, p.z); maxZ = Math.max(maxZ, p.z);
    }
    this.carves.push({ points, width, depth, underRoad: !!opts.underRoad, minX: minX - width, maxX: maxX + width, minZ: minZ - width, maxZ: maxZ + width });
  }

  // Scripted views: the summit saddle looks down both sides and the
  // descent opens toward the valley.
  openBias(s, side) {
    const t = this.track;
    if (!this._open) {
      this._open = [];
      const vs = t.zoneStart[1] + 800;
      const vx = t.px[vs], vz = t.pz[vs];
      for (const tg of t.tags) {
        if (tg.tag === 'summit') this._open.push({ s0: tg.s0 - 40, s1: tg.s1 + 120, side: 0, v: 0.8 });
        if (tg.tag === 'descent') {
          const m = Math.round((tg.s0 + t.zoneStart[1]) / 2);
          const toward = (vx - t.px[m]) * t.rx[m] + (vz - t.pz[m]) * t.rz[m] > 0 ? 1 : -1;
          this._open.push({ s0: tg.s0, s1: t.zoneStart[1], side: toward, v: 0.9 });
        }
      }
    }
    let v = 0;
    for (const o of this._open) {
      const w = smoothstep(o.s0 - 80, o.s0, s) * (1 - smoothstep(o.s1, o.s1 + 80, s));
      const sideMatch = o.side === 0 ? 1 : clamp(0.5 + 0.5 * side * o.side, 0, 1);
      v = Math.max(v, o.v * w * sideMatch);
    }
    return v;
  }

  zoneWeights(x) {
    const Z = this.forms.length;
    const w = new Array(Z);
    let prev = 1;
    for (let z = 0; z < Z; z++) {
      const e = this.edges[z];
      const t = e ? smoothstep(e.x - e.w, e.x + e.w, x) : 0;
      w[z] = prev - t;
      prev = t;
    }
    return w;
  }

  // ── Field construction ─────────────────────────────────────────
  buildFields() {
    const t = this.track;
    // Far field.
    const fw = Math.round((this.maxX - this.minX) / FAR) + 1;
    const fh = Math.round((this.maxZ - this.minZ) / FAR) + 1;
    this.fw = fw; this.fh = fh;
    const fd = new Float32Array(fw * fh).fill(1e9);
    const fy = new Float32Array(fw * fh);
    const fs = new Float32Array(fw * fh);
    const fl = new Float32Array(fw * fh);
    const flat = new Float32Array(fw * fh); // signed lateral distance
    const R = 3000;
    for (let i = 0; i < t.n; i += 12) {
      const x = t.px[i], z = t.pz[i];
      const i0 = Math.max(0, Math.floor((x - R - this.minX) / FAR));
      const i1 = Math.min(fw - 1, Math.ceil((x + R - this.minX) / FAR));
      const j0 = Math.max(0, Math.floor((z - R - this.minZ) / FAR));
      const j1 = Math.min(fh - 1, Math.ceil((z + R - this.minZ) / FAR));
      for (let j = j0; j <= j1; j++) {
        const wz = this.minZ + j * FAR - z;
        for (let ii = i0; ii <= i1; ii++) {
          const wx = this.minX + ii * FAR - x;
          const d2 = wx * wx + wz * wz;
          const k = j * fw + ii;
          if (d2 < fd[k]) {
            fd[k] = d2;
            fy[k] = t.py[i];
            fs[k] = i;
            const lat = wx * t.rx[i] + wz * t.rz[i];
            fl[k] = lat >= 0 ? 1 : -1;
            flat[k] = lat;
          }
        }
      }
    }
    for (let k = 0; k < fd.length; k++) fd[k] = Math.sqrt(fd[k]);
    Object.assign(this, { fd, fy, fs, fl, flat });
    if (this.forms.includes('mountain')) this.openField();

    // Near field: sparse 256 m tiles of 4 m nodes.
    this.nearTiles = new Map();
    // Circuits: the flat run-off reaches the barriers on each side.
    const corridor = this.lf.every((f) => f.corridor);
    for (let i = 0; i < t.n; i += 2) {
      const zb = t.zoneBlend(i, 150);
      const hw = t.hw[i];
      let r0 = hw, band = 0, kPow = 0;
      zb.forEach((w, z) => { r0 += w * this.lf[z].r0; band += w * this.lf[z].band; kPow += w * this.lf[z].pow; });
      const rL = corridor ? t.wallL[i] + r0 - hw : r0, rR = corridor ? t.wallR[i] + r0 - hw : r0;
      const r1 = Math.max(rL, rR) + band;
      // Circuits keep the run-off nearly flush; in a sag (the foot of the
      // Corkscrew) a straight 4 m chord of ground would cut above the
      // curving road, so it sits a little lower there.
      const sag = corridor ? Math.max(0, t.py[t.idx(i - 3)] + t.py[t.idx(i + 3)] - 2 * t.py[i]) / 9 : 0;
      const drop = corridor ? 0.1 + sag * 6 : 0.3;
      // Bridges and viaducts: no embankment, the deck stands on piers.
      if (this.isElevated(i)) continue;
      const x = t.px[i], z = t.pz[i], y = t.py[i];
      const rx = t.rx[i], rz = t.rz[i], bank = t.bank[i];
      const n0x = Math.floor((x - r1) / NEAR), n1x = Math.ceil((x + r1) / NEAR);
      const n0z = Math.floor((z - r1) / NEAR), n1z = Math.ceil((z + r1) / NEAR);
      for (let nz = n0z; nz <= n1z; nz++) {
        const dz = nz * NEAR - z;
        for (let nx = n0x; nx <= n1x; nx++) {
          const dx = nx * NEAR - x;
          const d = Math.sqrt(dx * dx + dz * dz);
          if (d > r1) continue;
          const lat = dx * rx + dz * rz;
          const rs = lat >= 0 ? rR : rL;
          if (corridor && d > rs + band) continue;
          const tile = this.nearTileFor(nx, nz, true);
          const li = (nz - tile.oz) * (NT + 1) + (nx - tile.ox);
          const k = 1 - Math.pow(smoothstep(rs, rs + band, d), kPow);
          // Run-off: the track's own surface out to the barriers. Where the
          // ground folds up from a banked road (concave), the 4 m chord
          // would stand proud of the road's edge, so drop it by the fold.
          const fold = corridor ? Math.max(0, lat >= 0 ? t.runR[i] + bank : t.runL[i] - bank) * 0.5
            * (1 - smoothstep(hw + RUNOFF_FLAT + 2, hw + RUNOFF_FLAT + 6, Math.abs(lat))) : 0;
          if (k > tile.K[li]) tile.K[li] = k;
          if (corridor) {
            // The run-off under a car is the surface of the nearest bit of
            // road (physics projects onto it), so take that one alone: an
            // average over the stretches round a tight corner on a steep
            // drop (the Corkscrew) rides high. Offset along the road too,
            // since samples are 2 m apart.
            if (d < tile.d[li]) {
              const along = dx * t.fx[i] + dz * t.fz[i];
              tile.swh[li] = t.surfaceY(i + along, lat) - drop - fold;
              tile.sw[li] = 1;
            }
          } else {
            const h = y - clamp(lat, -hw, hw) * bank - drop;
            const w = (k * k) / ((d * d + 1) * (d * d + 1));
            tile.sw[li] += w;
            tile.swh[li] += w * h;
          }
          if (d < tile.d[li]) { tile.d[li] = d; tile.s[li] = i; }
        }
      }
    }
  }

  nearTileFor(nx, nz, create = false) {
    const tx = Math.floor(nx / NT), tz = Math.floor(nz / NT);
    const key = tx + ',' + tz;
    let tile = this.nearTiles.get(key);
    if (!tile && create) {
      const N = (NT + 1) * (NT + 1);
      tile = {
        tx, tz, ox: tx * NT, oz: tz * NT,
        K: new Float32Array(N), sw: new Float32Array(N), swh: new Float32Array(N),
        d: new Float32Array(N).fill(1e9), s: new Float32Array(N),
      };
      this.nearTiles.set(key, tile);
    }
    return tile;
  }

  // Near-field node values; returns null when no road influence.
  nearNode(nx, nz) {
    const tile = this.nearTileFor(nx, nz, false);
    if (!tile) return null;
    const li = (nz - tile.oz) * (NT + 1) + (nx - tile.ox);
    return { tile, li };
  }

  // Bilinear far-field sample → {d, y, s, side}
  far(x, z, out = {}) {
    const gx = clamp((x - this.minX) / FAR, 0, this.fw - 1.001);
    const gz = clamp((z - this.minZ) / FAR, 0, this.fh - 1.001);
    const i = Math.floor(gx), j = Math.floor(gz);
    const tx = gx - i, tz = gz - j;
    const k00 = j * this.fw + i, k10 = k00 + 1, k01 = k00 + this.fw, k11 = k01 + 1;
    const B = (a) => lerp(lerp(a[k00], a[k10], tx), lerp(a[k01], a[k11], tx), tz);
    out.d = B(this.fd); out.y = B(this.fy); out.s = B(this.fs); out.side = B(this.fl); out.lat = B(this.flat);
    out.open = this.fopen ? B(this.fopen) : 0;
    return out;
  }

  // ── Landforms per zone ─────────────────────────────────────────
  landform(x, z, F = this.far(x, z, this._farTmp || (this._farTmp = {}))) {
    const w = this.zoneWeights(x);
    const detail = fbm(this.noise2, x / 90, z / 90, 3) * 3;
    let h = 0;
    for (let k = 0; k < w.length; k++) {
      if (w[k] > 0.001) h += w[k] * this[FORM_FN[this.forms[k]]](x, z, F, detail, k);
    }
    return h;
  }

  // Mountain lookouts: per far-field cell, how far the ground falls away
  // instead of rising (0 wall .. 1 drop), from that cell's own nearest road
  // s and side. Bilinear sampling of this is smooth even where neighbouring
  // cells belong to different road branches.
  openField() {
    const n = this.noise;
    const fopen = new Float32Array(this.fd.length);
    for (let k = 0; k < fopen.length; k++) {
      const s = this.fs[k], side = this.fl[k];
      fopen[k] = Math.max(smoothstep(0.15, 0.55, n(s / 520, side * 3.1 + 11.7)), this.openBias(s, side));
    }
    this.fopen = fopen;
  }

  formMountain(x, z, F, detail) {
    const n = this.noise, n2 = this.noise2, d = F.d;
    // Terrain rises on both sides of the pass, except where a stretch of
    // road opens up onto a drop (a lookout) on one side.
    // How open each side is comes precomputed per far-field cell (see
    // openField): computing it from the interpolated nearest-road s and side
    // raced through noise cells and lookout windows wherever neighbouring
    // cells were nearest to different road branches (hairpins), leaving
    // sawtooth spikes on the walls beside the road.
    const open = F.open;
    // Soft-edged modulation: sharp ridged noise here turns into needles.
    const rocky = 0.5 + 0.5 * fbm(n, x / 320, z / 320, 3);
    const wall = (18 + 150 * (1 - Math.exp(-d / 90))) * (0.62 + 0.7 * rocky);
    const drop = -130 * (1 - Math.exp(-d / 160)) * (0.7 + 0.5 * rocky);
    const local = lerp(wall, drop, open);
    const rp = ridged(n, x / 1900 + 3.3, z / 1900, 5, 2.0, 0.45);
    const peaks = smoothstep(250, 1600, d) * (180 + 420 * rp * rp * 1.6 + 120 * fbm(n2, x / 2500, z / 2500, 3));
    return F.y + local * (1 - smoothstep(600, 1400, d)) + peaks + detail * 2;
  }

  formValley(x, z, F, detail) {
    const n = this.noise, d = F.d;
    const roll = fbm(n, x / 380, z / 380, 3) * 7 * smoothstep(15, 90, d);
    const hills = smoothstep(260, 1500, d) * (40 + 380 * ridged(n, x / 1700 + 9.1, z / 1700, 5));
    return F.y - 1.2 + roll + hills + detail * 0.4;
  }

  formCity(x, z, F, detail, k) {
    const n = this.noise, d = F.d;
    const hills = smoothstep(1300, 2600, d) * (90 + 320 * ridged(n, x / 1500 + 4.2, z / 1500, 4));
    return this.flatY[k] - 0.25 + hills;
  }

  // Sea on the left of the road, land on the right. Sharp right beside the
  // road; the split softens with distance so that where the nearest bit of
  // road changes (behind the start, past the end) the land ramps into the
  // sea instead of standing as a straight wall.
  seaSide(F) {
    const R = 4 + 0.35 * F.d;
    return smoothstep(R, -R, F.lat);
  }

  // Cliff road: sheer drop to the sea on the left, steep hills on the right.
  formCoast(x, z, F, detail) {
    const n = this.noise, n2 = this.noise2, d = F.d;
    const sea = this.seaY ?? 0;
    // The cliff edge wanders in and out so the coastline isn't a copy of the road.
    const wob = 0.7 + 0.6 * (0.5 + 0.5 * fbm(n, x / 170, z / 170, 3));
    const cliff = Math.pow(smoothstep(7, 75, d * wob), 0.6);
    const floor = sea - 14 - 12 * smoothstep(200, 900, d);
    // (Sea stacks and arches are placed as meshes by Coast.js.)
    const hs = lerp(F.y - 1, floor, cliff) + detail * 1.5 * cliff;
    const rocky = 0.5 + 0.5 * fbm(n, x / 300, z / 300, 3);
    const hl = F.y + (10 + 150 * (1 - Math.exp(-d / 140))) * (0.55 + 0.8 * rocky)
      + smoothstep(400, 1800, d) * (120 + 260 * ridged(n, x / 1500 + 2.2, z / 1500, 5)) + detail * 2;
    return lerp(hl, hs, this.seaSide(F));
  }

  // Beach town: promenade and a wide sand beach on the left, a flat town
  // backed by hills on the right.
  formBeach(x, z, F, detail) {
    const n = this.noise, n2 = this.noise2, d = F.d;
    const sea = this.seaY ?? 0;
    const sand = smoothstep(16, 150, d);
    const hs = lerp(F.y - 1.6, sea - 2.5, sand) - 9 * smoothstep(160, 520, d) + fbm(n2, x / 40, z / 40, 2) * 0.35 * sand;
    const hl = F.y - 0.4 + smoothstep(420, 1600, d) * (60 + 240 * ridged(n, x / 1400 + 5.1, z / 1400, 5)) + detail * 0.3 * smoothstep(300, 500, d);
    return lerp(hl, hs, this.seaSide(F));
  }

  // Harbour: flat quays, a quay wall to the sea on the left, a shipping
  // channel under the bridge, hills far inland.
  formHarbor(x, z, F, detail, k) {
    const n = this.noise, d = F.d;
    const sea = this.seaY ?? 0;
    const water = sea - 14;
    let h = this.flatY[k] - 0.3;
    h += (1 - this.seaSide(F)) * smoothstep(900, 2200, d) * (100 + 220 * ridged(n, x / 1600 + 8.8, z / 1600, 5));
    h = lerp(h, water, smoothstep(175, 195, d) * this.seaSide(F));
    for (const c of this.channels) {
      const m = smoothstep(c.s0, c.s0 + 90, F.s) * (1 - smoothstep(c.s1 - 90, c.s1, F.s));
      h = lerp(h, water, m);
    }
    return h;
  }

  // Downtown Streets: the level's own ground function (level.ground(x, z)).
  formStreets(x, z, F) {
    const g = this.level.ground;
    const d = F.d;
    const hills = smoothstep(1200, 2400, d) * (60 + 260 * ridged(this.noise, x / 1500 + 6.1, z / 1500, 4));
    return (g ? g(x, z) : F.y) - 0.3 + hills;
  }

  // Seaside Raceway: the surveyed ground (level.ground), as is.
  formRaceway(x, z) {
    return this.level.ground(x, z);
  }

  // ── Desert Run (Level 4) ───────────────────────────────────────
  // Red sandstone: horizontal strata, each a soft slope topped by a hard
  // cliff band. Applied in absolute height so layers stay level while the
  // road descends through them.
  strata(y) {
    const B = 9;
    const w = y + 1.6 * this.noise2(y * 0.05, 3.7);
    const k = Math.floor(w / B), f = w / B - k;
    const g = f < 0.55 ? (f / 0.55) * 0.22 : 0.22 + ((f - 0.55) / 0.45) * 0.78;
    return y + ((k + g) * B - w);
  }

  // Distance from the road centreline to the foot of each canyon wall,
  // every 10 m of road: narrows, an amphitheatre of hoodoos, the arch, and
  // the mouth opening onto the basin. Smoothed so the walls wander.
  canyonWidth(s, side) {
    if (!this._cw) {
      const t = this.track, n = Math.ceil(t.length / 10) + 1;
      const L = new Float32Array(n), R = new Float32Array(n);
      const within = (tag, sv, pad = 0) => t.tags.some((g) => g.tag === tag && sv > g.s0 - pad && sv < g.s1 + pad);
      const mouth = t.tags.filter((g) => g.tag === 'mouth');
      const m0 = mouth.length ? Math.min(...mouth.map((g) => g.s0)) : t.zoneStart[1] - 300;
      for (let i = 0; i < n; i++) {
        const sv = i * 10;
        for (const [arr, sd] of [[L, -1], [R, 1]]) {
          let w = 24 + 22 * (0.5 + 0.5 * this.noise(sv / 260, sd * 4.1 + 1.3));
          // Side canyons: the wall steps back for a while.
          w += 55 * smoothstep(0.45, 0.7, this.noise2(sv / 140, sd * 7.7 + 2.2));
          if (within('narrows', sv, 20)) w = 13.5 + 2 * (0.5 + 0.5 * this.noise(sv / 60, sd));
          if (within('arch', sv, 10)) w = Math.min(w, 17);
          if (within('fin', sv)) w = Math.min(w, 16);
          if (within('hoodoos', sv, 30) && sd > 0) w = 150;
          if (sv < 220) w = Math.max(w, 34); // room round the start
          w += smoothstep(m0, t.zoneStart[1] + 250, sv) * 520;
          arr[i] = w;
        }
      }
      const sm = (a) => {
        const out = new Float32Array(a.length);
        for (let i = 0; i < a.length; i++) {
          let acc = 0, wt = 0;
          for (let k = -4; k <= 4; k++) { const j = clamp(i + k, 0, a.length - 1); const q = Math.exp(-(k * k) / 8); acc += a[j] * q; wt += q; }
          out[i] = acc / wt;
        }
        return out;
      };
      this._cw = { L: sm(L), R: sm(R), n };
    }
    const i = clamp(s / 10, 0, this._cw.n - 1.001), i0 = Math.floor(i), f = i - i0;
    const a = side < 0 ? this._cw.L : this._cw.R;
    return a[i0] + (a[i0 + 1] - a[i0]) * f;
  }

  formCanyon(x, z, F, detail) {
    const n = this.noise, n2 = this.noise2, d = F.d;
    const side = F.lat >= 0 ? 1 : -1;
    // Buttresses and alcoves: the foot of the wall wanders in and out.
    const w = this.canyonWidth(F.s, side) * (0.88 + 0.24 * (0.5 + 0.5 * fbm(n, x / 120, z / 120, 2)))
      + 9 * fbm(n2, x / 38 + 5.5, z / 38, 2);
    const u = d - w; // metres past the foot of the wall
    // Wall height and how far back it climbs before the rim.
    const rim = 46 + 62 * (0.5 + 0.5 * fbm(n, x / 650 + 3.1, z / 650, 2));
    const reach = 7 + 16 * (0.5 + 0.5 * n2(x / 260, z / 260));
    let wall = rim * smoothstep(-3, reach, u) + 5 * smoothstep(-14, 2, u);
    // Slickrock domes and fins along the rim.
    wall += 26 * smoothstep(0.15, 0.6, fbm(n, x / 230 + 1.7, z / 230 - 2.3, 2)) * smoothstep(reach, reach + 60, u);
    // Beyond the rim: benchland rising in steps toward the far ranges.
    wall += smoothstep(200, 900, d) * (30 + 70 * (0.5 + 0.5 * fbm(n2, x / 900, z / 900, 3)));
    wall += smoothstep(1300, 2600, d) * (120 + 380 * ridged(n, x / 1700 + 6.6, z / 1700, 5));
    const y = F.y + wall + detail * 0.6;
    // Terrace the rock; the canyon floor stays smooth sand.
    const rock = smoothstep(3, 12, wall);
    return lerp(y, this.strata(y), rock) + (1 - rock) * fbm(n2, x / 50, z / 50, 2) * 0.6 * smoothstep(8, 20, d);
  }

  // Open basin: gentle swells, washes, flat-topped mesas standing off in
  // the distance and a jagged range on the horizon. Desert.plan() can lay
  // a level bed for the railway beside the road (this.desertRail).
  formDesert(x, z, F, detail) {
    const n = this.noise, n2 = this.noise2, d = F.d;
    let h = F.y - 0.8 + fbm(n, x / 380, z / 380, 3) * 4.5 * smoothstep(25, 140, d) + detail * 0.35 * smoothstep(20, 60, d);
    // Mesas: a thresholded noise field gives flat tops and sheer terraced sides.
    const m = fbm(n2, x / 1100 + 17.3, z / 1100 - 4.2, 3);
    const mesa = smoothstep(0.2, 0.3, m) * smoothstep(500, 800, d);
    if (mesa > 0) {
      const top = F.y + 70 + 90 * (0.5 + 0.5 * n(x / 2300, z / 2300));
      const y = lerp(h, top, mesa);
      h = lerp(y, this.strata(y), smoothstep(0.05, 0.4, mesa) * (1 - smoothstep(0.9, 1, mesa)));
    }
    h += smoothstep(1500, 2800, d) * (140 + 420 * ridged(n, x / 1600 + 2.7, z / 1600, 5));
    const r = this.desertRail;
    if (r && F.s > r.s0 && F.s < r.s1) {
      const k = 1 - smoothstep(r.half, r.half + 22, Math.abs(F.lat - r.lat));
      h = lerp(h, F.y - r.drop, k);
    }
    return h;
  }

  // Dry lake: dead level out to a wandering shoreline, alluvial fans, then
  // the mountains that ring the basin.
  formPlaya(x, z, F) {
    const n = this.noise, d = F.d;
    const shore = 700 + 500 * (0.5 + 0.5 * fbm(n, x / 1400 + 9.1, z / 1400, 2));
    const fan = smoothstep(shore, shore + 900, d);
    return F.y - 0.35 + fan * fan * 60 + smoothstep(shore + 500, shore + 2200, d) * (160 + 420 * ridged(n, x / 1500 + 1.9, z / 1500, 5));
  }

  // Full terrain height at a world point.
  heightAt(x, z) {
    let h = this.landform(x, z);
    // Modifiers (farm pads, creek) act on the landform only.
    for (const f of this.flattens) {
      const d = Math.hypot(x - f.x, z - f.z);
      if (d < f.r + f.falloff) {
        const target = f.y ?? f.yResolved;
        h = lerp(target, h, smoothstep(f.r, f.r + f.falloff, d));
      }
    }
    for (const c of this.carves) if (!c.underRoad) h -= this.carveDepth(c, x, z);

    // Road influence (bilinear over near-field nodes).
    const gx = x / NEAR, gz = z / NEAR;
    const nx = Math.floor(gx), nz = Math.floor(gz);
    const tx = gx - nx, tz = gz - nz;
    let K = 0, sw = 0, swh = 0;
    for (let c = 0; c < 4; c++) {
      const ax = nx + (c & 1), az = nz + (c >> 1);
      const node = this.nearNode(ax, az);
      if (!node) continue;
      const wgt = ((c & 1) ? tx : 1 - tx) * ((c >> 1) ? tz : 1 - tz);
      K += node.tile.K[node.li] * wgt;
      if (node.tile.sw[node.li] > 0) {
        const rh = node.tile.swh[node.li] / node.tile.sw[node.li];
        sw += wgt; swh += wgt * rh;
      }
    }
    if (K > 0 && sw > 0) h = lerp(h, swh / sw, K);
    for (const c of this.carves) if (c.underRoad) h -= this.carveDepth(c, x, z);
    return h;
  }

  // Resolve flatten targets that were given without a height.
  resolveFlattens() {
    for (const f of this.flattens) if (f.y === null) f.yResolved = this.landform(f.x, f.z);
  }

  carveDepth(c, x, z) {
    if (x < c.minX || x > c.maxX || z < c.minZ || z > c.maxZ) return 0;
    let best = Infinity;
    const pts = c.points;
    for (let i = 0; i < pts.length - 1; i++) {
      const a = pts[i], b = pts[i + 1];
      const abx = b.x - a.x, abz = b.z - a.z;
      const t = clamp(((x - a.x) * abx + (z - a.z) * abz) / (abx * abx + abz * abz), 0, 1);
      const dx = x - (a.x + abx * t), dz = z - (a.z + abz * t);
      const d = dx * dx + dz * dz;
      if (d < best) best = d;
    }
    best = Math.sqrt(best);
    return c.depth * (1 - smoothstep(c.width * 0.35, c.width, best));
  }

  // Road proximity info for scenery placement.
  roadInfo(x, z) {
    const node = this.nearNode(Math.round(x / NEAR), Math.round(z / NEAR));
    if (node && node.tile.d[node.li] < 1e8) {
      return { d: node.tile.d[node.li], s: node.tile.s[node.li], near: true };
    }
    const F = this.far(x, z);
    return { d: F.d, s: F.s, near: false };
  }

  slopeAt(x, z, e = 2) {
    const hx = this.heightAt(x + e, z) - this.heightAt(x - e, z);
    const hz = this.heightAt(x, z + e) - this.heightAt(x, z - e);
    return Math.hypot(hx, hz) / (2 * e);
  }

  // ── Mesh ───────────────────────────────────────────────────────
  // Returns plain arrays per tile so the builder can run anywhere; the
  // three.js side (TerrainMesh.js) turns them into geometry.
  tileList() {
    const tiles = [];
    const nx = Math.round((this.maxX - this.minX) / TILE);
    const nz = Math.round((this.maxZ - this.minZ) / TILE);
    for (let j = 0; j < nz; j++) {
      for (let i = 0; i < nx; i++) {
        const x0 = this.minX + i * TILE, z0 = this.minZ + j * TILE;
        // Closest approach of the road to this tile, from the far field.
        let dmin = Infinity;
        for (let a = 0; a <= 8; a += 2) for (let b = 0; b <= 8; b += 2) {
          dmin = Math.min(dmin, this.far(x0 + a * 32, z0 + b * 32).d);
        }
        const hasNear = this.nearTiles.has(Math.floor(x0 / TILE) + ',' + Math.floor(z0 / TILE));
        const step = hasNear || dmin < 60 ? 4 : dmin < 700 ? 16 : 32;
        tiles.push({ x0, z0, size: TILE, step, dmin, i, j });
      }
    }
    const grid = new Map(tiles.map((t) => [t.i + ',' + t.j, t]));
    for (const t of tiles) {
      // Neighbour resolution per edge: z0 (j-1), z1 (j+1), x0 (i-1), x1 (i+1).
      t.nsteps = [[0, -1], [0, 1], [-1, 0], [1, 0]].map(([a, b]) => grid.get((t.i + a) + ',' + (t.j + b))?.step ?? t.step);
    }
    return tiles;
  }
}

export const TERRAIN_TILE = TILE;
