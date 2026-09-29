import * as THREE from 'three';
import { signTexture, glowTexture, concreteTexture, asphaltTexture } from '../textures.js';
import { GeoBuilder, staticMesh, instanced, trs, yawOf } from './geom.js';
import { adTexture, AD_COUNT, tunnelTileTexture, soundWallTexture, parkTexture, bannerTexture } from './cityTextures.js';
import { clamp, lerp, smoothstep } from '../../util/math.js';

// Everything that belongs to the Interstate itself: the westbound
// carriageway on the other side of the median, the road beyond the finish,
// lighting, sign gantries, sound walls, overpasses, the Meridian tunnel,
// billboards and the finish gantry.
//
// Lateral layout (metres from our centreline, + = right):
//   our barrier back   -10.52          median centre  -10.95
//   opposite barrier   -11.38 … -12.02 opposite centre -21.92
//   opposite outer     -31.82 … -32.46

export const MED_C = -10.95;
export const OPP_FACE_IN = -12.02;
export const OPP_C = -21.92;
export const OPP_FACE_OUT = -31.82;
export const OPP_BACK = -32.46;
export const RIGHT_CLEAR = 10.6;
const W_DECK = 13;   // anything to the right must be beyond this
// Westbound lane centres (lat), nearest the median first. Traffic on them
// drives toward -s; surface height is oppY(frame).
export const OPP_LANES = [-15.67, -19.57, -23.47, -27.37];
const HALF = 9.4;

// A path along the freeway: the track between sA and sB, and straight
// extensions beyond both ends so the westbound lanes and our lanes past the
// finish carry on into the distance.
export function makePath(track, sA, sB, back, fwd) {
  if (track.loop) {
    // A closed loop: the path is the track itself and s wraps.
    return {
      sA: 0, sB: track.length, u0: 0, u1: track.length, loop: true,
      frame(u, out = {}) { track.frame(u, out); out.ext = false; return out; },
    };
  }
  const fa = track.frame(sA, {});
  const fb = track.frame(sB - 0.01, {});
  return {
    sA, sB, u0: sA - back, u1: sB + fwd,
    frame(u, out = {}) {
      if (u >= sA && u <= sB) {
        track.frame(u, out);
        out.ext = false;
        return out;
      }
      const b = u < sA ? fa : fb;
      const du = u - (u < sA ? sA : sB);
      out.x = b.x + b.fx * du; out.z = b.z + b.fz * du; out.y = b.y;
      out.fx = b.fx; out.fz = b.fz; out.rx = b.rx; out.rz = b.rz;
      out.hw = HALF; out.bank = 0; out.grade = 0; out.kappa = 0;
      out.wallL = 9.9; out.wallR = 9.9; out.zone = 2; out.s = u;
      out.ext = true;
      return out;
    },
  };
}

// Height of the westbound carriageway: flat across, never below the ground
// the terrain flattened for our side.
export const oppY = (f) => f.y + Math.max(0.04, HALF * f.bank - 0.2);
export const ourY = (f, lat) => f.y - lat * f.bank;

// Sweep a cross-section along the path. profile: [{lat, y(f, lat)}] in
// order of increasing lat for upward faces. uv: 'road' → (lat/uS, s/vS),
// 'wall' → (s/uS, (y - f.y)/vS).
export function sweep(path, ranges, profile, { step = 4, uv = 'road', uS = 4, vS = 8, color = null } = {}) {
  const pos = [], uvs = [], idx = [], col = [];
  const f = {};
  const P = profile.length;
  for (const [a, b] of ranges) {
    if (b - a < 0.5) continue;
    const base = pos.length / 3;
    let rows = 0;
    for (let s = a; ; s += step) {
      const ss = Math.min(s, b);
      path.frame(ss, f);
      for (let p = 0; p < P; p++) {
        const pr = profile[p];
        const lat = typeof pr.lat === 'function' ? pr.lat(f) : pr.lat;
        const y = pr.y(f, lat);
        pos.push(f.x + f.rx * lat, y, f.z + f.rz * lat);
        if (uv === 'wall') uvs.push(ss / uS, (y - f.y) / vS);
        else uvs.push(lat / uS, ss / vS);
        if (color) col.push(color[0], color[1], color[2]);
      }
      rows++;
      if (ss >= b) break;
    }
    for (let r = 0; r < rows - 1; r++) {
      for (let p = 0; p < P - 1; p++) {
        if (profile[p].gap) continue;
        const i0 = base + r * P + p, i1 = i0 + 1, i2 = i0 + P, i3 = i2 + 1;
        idx.push(i0, i1, i2, i1, i3, i2);
      }
    }
  }
  const g = new THREE.BufferGeometry();
  g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
  g.setAttribute('uv', new THREE.Float32BufferAttribute(uvs, 2));
  if (color) g.setAttribute('color', new THREE.Float32BufferAttribute(col, 3));
  g.setIndex(idx);
  g.computeVertexNormals();
  g.computeBoundingSphere();
  return g;
}

export function chunked(ranges, size = 240) {
  const out = [];
  for (const [a, b] of ranges) for (let s = a; s < b; s += size) out.push([s, Math.min(b, s + size)]);
  return out;
}

// Jersey barrier offsets from its face: [outward, dy].
const JERSEY = [[-0.02, 0], [0.02, 0.25], [0.2, 0.36], [0.28, 1.0], [0.42, 1.0], [0.62, 0.25]];

export class Freeway {
  constructor(ctx) {
    this.ctx = ctx;
    this.group = ctx.group;
    this.reserved = []; // s-ranges where median poles must not stand
  }

  // Batch static geometry by material and ~700 m chunk; flushed at the end
  // of build() so the whole freeway costs a handful of draw calls.
  add(geo, mat, { cast = false, receive = true } = {}) {
    geo.computeBoundingSphere();
    const c = geo.boundingSphere.center;
    const CH = this.loop ? 1800 : 700;
    const key = mat.uuid + ':' + Math.floor(c.x / CH) + ':' + Math.floor(c.z / CH) + ':' + (cast ? 1 : 0);
    this.batches = this.batches || new Map();
    let b = this.batches.get(key);
    if (!b) this.batches.set(key, (b = { mat, cast, receive, geos: [] }));
    b.geos.push(geo);
  }

  flush() {
    for (const b of (this.batches || new Map()).values()) {
      const geo = mergeFlat(b.geos);
      this.group.add(staticMesh(geo, b.mat, { cast: b.cast, receive: b.receive }));
    }
    this.batches = new Map();
  }

  mat(key, make) {
    const m = this.ctx.mats;
    if (!m[key]) m[key] = make();
    return m[key];
  }

  get concrete() {
    return this.mat('concrete', () => new THREE.MeshStandardMaterial({ map: concreteTexture(), roughness: 0.9, color: 0xd8d5ce }));
  }

  build() {
    const { track, path, T } = this.ctx;
    this.loop = !!track.loop;
    this.tunnels = track.tag('tunnel');
    this.tunnel = this.tunnels[0];
    this.elevated = (u) => u >= path.sA && u <= path.sB && T.isElevated(track.idx(u));
    this.inTunnel = (u) => this.tunnels.some((t) => this.near(u, t.s0 - 1, t.s1 + 1));
    this.tunnelPools = [];
    this.parkTrees = [];
    this.parkLamps = [];

    if (this.loop) this.pickLoopSites(); else this.pickFeatureSites();
    this.buildOpposite();
    this.buildForwardExtension();
    this.buildSoundWalls();
    const names = this.loop ? ['CENTRAL TUNNEL', 'HARBOR TUNNEL', 'LOOP TUNNEL'] : ['MERIDIAN TUNNEL'];
    this.tunnels.forEach((t, i) => this.buildTunnel(t, names[i % names.length]));
    for (const o of this.overpasses) this.buildOverpass(o);
    // One instanced mesh each for every tunnel's and overpass's columns and
    // strip lights, however many there are.
    if (this.tunnelCols?.length) {
      const g = new THREE.BoxGeometry(1.6, 1, 0.6);
      g.translate(0, 0.5, 0);
      this.group.add(instanced(g, this.concrete, this.tunnelCols, { receive: true }));
      this.group.add(instanced(new THREE.BoxGeometry(1, 1, 1), this.sodium, this.tunnelStrips));
    }
    if (this.overpassCols?.length) {
      const g = new THREE.CylinderGeometry(0.38, 0.38, 1, 10);
      g.translate(0, 0.5, 0);
      this.group.add(instanced(g, this.concrete, this.overpassCols, { cast: true }));
    }
    this.buildGantries();
    if (Number.isFinite(track.finishS)) this.buildFinish(track.finishS, 'FINISH', true);
    else this.buildFinish(this.welcomeS, 'MERIDIAN LOOP', false);
    this.buildBillboards();
    this.buildLighting();
    this.flush();
  }

  // Is u within [a, b]? On a loop the range may straddle the seam.
  near(u, a, b) {
    if (!this.loop) return u > a && u < b;
    const L = this.ctx.track.length;
    const w = (x) => ((x % L) + L) % L;
    const uu = w(u), aa = w(a), bb = w(b);
    return aa <= bb ? uu > aa && uu < bb : uu > aa || uu < bb;
  }
  reservedAt(u, pad = 0) { return this.reserved.some(([a, b]) => this.near(u, a - pad, b + pad)); }

  // Loop: gantries, overpasses and billboards spread round the whole ring.
  pickLoopSites() {
    const { track, grid, cityY, T } = this.ctx;
    const L = track.length;
    const SIGNS = [
      [{ lines: ['LOOP', 'Downtown', 'Next 2 exits'], arrow: 'up', lane: -3.2 }, { lines: ['EXIT 7', 'Harbor Blvd', '1 MILE'], lane: 4.6 }],
      [{ lines: ['EXIT 8', 'Grand Ave'], arrow: 'right', lane: 4.6 }, { lines: ['LOOP', 'Central Tunnel'], arrow: 'up', lane: -3.2 }],
      [{ lines: ['LOOP', 'Next exit', 'Harbor Blvd'], lane: -3.4 }, { lines: ['EXIT 9', 'Waterfront'], arrow: 'right', lane: 4.6 }],
      [{ lines: ['I-9 WEST', 'Coast Hwy'], arrow: 'up', lane: -3.2 }, { lines: ['EXIT 10', 'Industrial Pkwy', '1/2 MILE'], lane: 4.6 }],
      [{ lines: ['LOOP', 'Midtown', 'Arena'], arrow: 'up', lane: -3.2 }, { lines: ['EXIT 11', 'Union Station'], lane: 4.6 }],
      [{ lines: ['EXIT 12', '5th Street', '1 MILE'], lane: 4.6 }, { lines: ['LOOP', 'City Center'], arrow: 'up', lane: -3.2 }],
      [{ lines: ['HARBOR TUNNEL', 'LIGHTS ON'], bg: '#f2c230', fg: '#111', border: '#111', lane: -3.4 }, { lines: ['EXIT 13', 'Pier 39'], arrow: 'right', lane: 4.6 }],
      [{ lines: ['EXIT 14', 'Airport Rd', '2 MILES'], lane: 4.6 }, { lines: ['LOOP', 'Downtown', 'Stay left'], arrow: 'up', lane: -3.2 }],
      [{ lines: ['LOOP', 'Meridian Park'], arrow: 'up', lane: -3.2 }, { lines: ['EXIT 15', 'University'], arrow: 'right', lane: 4.6 }],
      [{ lines: ['EXIT 16', 'Market St', '1/2 MILE'], lane: 4.6 }, { lines: ['LOOP', 'Harbor Blvd'], arrow: 'up', lane: -3.2 }],
      [{ lines: ['LOOP', 'Next exit', 'Old Town'], lane: -3.4 }, { lines: ['EXIT 17', 'Old Town'], arrow: 'right', lane: 4.6 }],
      [{ lines: ['EXIT 18', 'Stadium', '1 MILE'], lane: 4.6 }, { lines: ['LOOP', 'Downtown 3'], arrow: 'up', lane: -3.2 }],
    ];
    this.welcomeS = track.startS + 360;
    this.reserved.push([this.welcomeS - 10, this.welcomeS + 10]);
    for (const t of this.tunnels) this.reserved.push([t.s0 - 12, t.s1 + 12]);
    this.gantrySites = [];
    let k = 0;
    for (let s = this.welcomeS + 700; s < L + this.welcomeS - 600; s += 1050) {
      let ss = s;
      // Keep gantries out of tunnels and off the portals.
      for (let tries = 0; tries < 6 && this.tunnels.some((t) => this.near(ss, t.s0 - 120, t.s1 + 40)); tries++) ss += 90;
      // Tunnel-warning sign when a tunnel lies just ahead.
      const ahead = this.tunnels.find((t) => { const d = track.ds(ss, t.s0); return d > 60 && d < 600; });
      const signs = ahead
        ? [{ lines: [`${ahead === this.tunnels[0] ? 'CENTRAL' : 'HARBOR'} TUNNEL`, 'LIGHTS ON'], bg: '#f2c230', fg: '#111', border: '#111', lane: -3.4 }, SIGNS[k % SIGNS.length][1]]
        : SIGNS[k % SIGNS.length];
      this.gantrySites.push({ s: track.wrap(ss), signs });
      k++;
    }
    for (const g of this.gantrySites) this.reserved.push([g.s - 9, g.s + 9]);

    // Overpasses where either family of grid streets crosses the freeway at
    // ground level, roughly square-on.
    this.overpasses = [];
    const f = {};
    const cands = [];
    let pu = null, pv = null;
    for (let s = 0; s <= L; s += 1) {
      track.frame(s, f);
      const u = grid.toU(f.x, f.z), v = grid.toV(f.x, f.z);
      if (pu !== null) {
        const a0 = Math.floor(pu / grid.pu), a1 = Math.floor(u / grid.pu);
        if (a1 !== a0) cands.push({ s, fam: 'u', k: Math.max(a0, a1), rate: grid.vAxis[0] * f.rx + grid.vAxis[1] * f.rz });
        const b0 = Math.floor(pv / grid.pv), b1 = Math.floor(v / grid.pv);
        if (b1 !== b0) cands.push({ s, fam: 'v', k: Math.max(b0, b1), rate: grid.uAxis[0] * f.rx + grid.uAxis[1] * f.rz });
      }
      pu = u; pv = v;
    }
    const taken = [];
    for (const c of cands) {
      const s = c.s;
      if (Math.abs(c.rate) < 0.75) continue;
      const low = [-40, 0, 40].every((d) => !T.isElevated(track.idx(s + d)) && track.py[track.idx(s + d)] < cityY + 0.35);
      if (!low || this.reservedAt(s, 45) || this.tunnels.some((t) => this.near(s, t.s0 - 80, t.s1 + 80))) continue;
      if (taken.some((q) => Math.abs(track.ds(q, s)) < 520)) continue;
      this.overpasses.push({ s, k: c.k, fam: c.fam });
      taken.push(s);
    }
    for (const o of this.overpasses) this.reserved.push([o.s - 16, o.s + 16]);

    // Billboards, alternating sides, clear of everything else.
    this.billboardSites = [];
    let side = 1;
    for (let s = this.welcomeS + 250; s < L + this.welcomeS - 200; s += 430) {
      const ss = track.wrap(s + ((k * 97) % 120));
      k++;
      if (this.reservedAt(ss, 30) || this.tunnels.some((t) => this.near(ss, t.s0 - 60, t.s1 + 60))) continue;
      this.billboardSites.push({ s: ss, lat: side > 0 ? 19 + (k % 3) : -42 - (k % 2), h: 11 + (k % 4) * 1.5 });
      side = -side;
    }
  }

  // Decide where gantries and overpasses go before anything is built, so
  // lighting can leave gaps for them.
  pickFeatureSites() {
    const { track, grid, cityY } = this.ctx;
    const t = this.tunnel;
    // Sites are anchored to the zone start, the tunnel and the finish so
    // they stay put when earlier parts of the route are edited.
    const z2 = track.zones[this.ctx.zone].s0;
    const tun = t ? t.s0 : z2 + 1940;
    const fin = track.finishS;
    this.gantrySites = [
      { s: z2 + 491, signs: [
        { lines: ['I-9 WEST', 'Downtown Meridian'], arrow: 'up', lane: 2.2 },
        { lines: ['EXIT 41', 'Airport Rd', '1 MILE'], lane: -4.6 } ] },
      { s: z2 + 841, signs: [
        { lines: ['EXIT 42', 'Main St', '1/2 MILE'], lane: 4.6 },
        { lines: ['I-9 WEST', 'Meridian Tunnel'], arrow: 'up', lane: -3.2 } ] },
      { s: z2 + 1401, signs: [
        { lines: ['DOWNTOWN', 'NEXT 3 EXITS'], lane: -3.4 },
        { lines: ['EXIT 42', 'Main St'], arrow: 'right', lane: 4.6 } ] },
      { s: tun - 80, signs: [
        { lines: ['MERIDIAN TUNNEL', 'LIGHTS ON'], bg: '#f2c230', fg: '#111', border: '#111', lane: -3.4 },
        { lines: ['I-9 WEST', 'Coast Hwy'], arrow: 'up', lane: 4.4 } ] },
      { s: fin - 560, signs: [
        { lines: ['EXIT 43', 'Harbor Blvd', '1 MILE'], lane: 4.6 },
        { lines: ['I-9 WEST', 'City Center'], arrow: 'up', lane: -3.2 } ] },
      { s: fin - 190, signs: [
        { lines: ['EXIT 44', 'Grand Ave'], arrow: 'right', lane: 4.6 },
        { lines: ['I-9 WEST', 'Ocean Beach 12'], arrow: 'up', lane: -3.2 } ] },
    ];
    for (const g of this.gantrySites) this.reserved.push([g.s - 9, g.s + 9]);
    if (t) this.reserved.push([t.s0 - 12, t.s1 + 12]);
    this.reserved.push([track.finishS - 10, track.finishS + 10]);

    // Overpasses where grid cross streets meet the freeway at ground level.
    this.overpasses = [];
    const f = {};
    let prevU = null;
    const cands = [];
    for (let s = z2 + 500; s < track.length - 20; s += 1) {
      track.frame(s, f);
      const u = grid.toU(f.x, f.z);
      if (prevU !== null) {
        const k0 = Math.floor(prevU / grid.pu), k1 = Math.floor(u / grid.pu);
        if (k1 !== k0) cands.push({ s, k: Math.max(k0, k1) });
      }
      prevU = u;
    }
    let lastS = -1e9;
    for (const c of cands) {
      const s = c.s;
      const low = [-40, 0, 40].every((d) => track.py[track.idx(s + d)] < cityY + 0.35);
      const clear = this.reserved.every(([a, b]) => s < a - 45 || s > b + 45);
      const inTunnel = t && s > t.s0 - 80 && s < t.s1 + 80;
      if (!low || !clear || inTunnel || s - lastS < 380) continue;
      this.overpasses.push({ s, k: c.k, fam: 'u' });
      lastS = s;
    }
    for (const o of this.overpasses) this.reserved.push([o.s - 16, o.s + 16]);
  }

  // ── Westbound carriageway ──────────────────────────────────────
  buildOpposite() {
    const { path } = this.ctx;
    const asphalt = this.mat('asphaltOpp', () => new THREE.MeshStandardMaterial({ map: asphaltTexture(2), roughness: 0.88 }));
    const conc = this.concrete;
    const all = [[path.u0, path.u1]];
    const ground = [], elev = [];
    // Split into ground/elevated runs.
    let cur = null, curE = null;
    for (let u = path.u0; u <= path.u1; u += 2) {
      const e = this.elevated(u);
      if (curE === null || e !== curE) {
        if (cur) (curE ? elev : ground).push([cur, u]);
        cur = u; curE = e;
      }
    }
    if (cur !== null) (curE ? elev : ground).push([cur, path.u1]);

    // Surface.
    for (const r of chunked(all, 260)) {
      const g = sweep(path, [r], [
        { lat: OPP_FACE_OUT, y: (f) => oppY(f) },
        { lat: OPP_C, y: (f) => oppY(f) },
        { lat: OPP_FACE_IN, y: (f) => oppY(f) },
      ], { step: 4, uS: 4.8, vS: 10 });
      this.add(g, asphalt, { receive: true });
    }
    // Median: opposite barrier + strip back to our barrier. Before sA there
    // is no eastbound road, so the barrier stands alone.
    const med = (a) => a.map(([o, dy]) => ({ lat: OPP_FACE_IN + o, y: (f) => oppY(f) + dy }));
    const medProfile = [
      ...JERSEY.map(([o, dy]) => ({ lat: OPP_FACE_IN + o, y: (f) => oppY(f) + dy })),
      { lat: -11.38, y: (f) => oppY(f) + 0.25 },
      { lat: -10.52, y: (f) => ourY(f, -10.52) + 0.25 },
    ];
    const medAlone = [
      { lat: OPP_FACE_IN, y: (f) => oppY(f) - 0.7 },
      ...med(JERSEY),
      { lat: -11.32, y: (f) => oppY(f) + 0.25 },
      { lat: -11.28, y: (f) => oppY(f) },
      { lat: -11.28, y: (f) => oppY(f) - 0.7 },
    ];
    const withEast = [[Math.max(path.u0, path.sA), path.u1]];
    const alone = path.u0 < path.sA ? [[path.u0, path.sA]] : [];
    for (const r of chunked(withEast, 300)) this.add(sweep(path, [r], medProfile, { step: 4 }), conc, { cast: true, receive: true });
    for (const r of alone) this.add(sweep(path, [r], medAlone, { step: 4 }), conc, { cast: true, receive: true });

    // Outer barrier (+ skirt on the ground, fascia when elevated).
    const outer = (skirt) => [
      ...(skirt
        ? [{ lat: OPP_BACK - 3.2, y: (f) => oppY(f) - 2.8 }, { lat: OPP_BACK, y: (f) => oppY(f) - 0.02 }]
        : [{ lat: OPP_BACK, y: (f) => oppY(f) - 1.8 }]),
      ...JERSEY.slice().reverse().map(([o, dy]) => ({ lat: OPP_FACE_OUT - o, y: (f) => oppY(f) + dy })),
    ];
    for (const r of chunked(ground, 300)) this.add(sweep(path, [r], outer(true), { step: 4 }), conc, { cast: true, receive: true });
    for (const r of chunked(elev, 300)) this.add(sweep(path, [r], outer(false), { step: 4 }), conc, { cast: true, receive: true });

    // Deck underside and piers where elevated.
    for (const r of chunked(elev, 300)) {
      const g = sweep(path, [r], [
        { lat: -10.52, y: (f) => ourY(f, -10.52) - 1.8 },
        { lat: OPP_BACK, y: (f) => oppY(f) - 1.8 },
      ], { step: 4 });
      this.add(g, conc);
    }
    this.buildOppositePiers();
    this.buildOppositeMarkings();
  }

  buildOppositePiers() {
    const { track, T } = this.ctx;
    const f = {};
    // Same spacing rule as Road.buildViaduct so piers line up in rows.
    const runs = [];
    let start = null;
    for (let s = 0; s <= track.length; s += 2) {
      const i = track.idx(s);
      const ok = track.zone[i] === this.ctx.zone && T.isElevated(i);
      if (ok && start === null) start = s;
      if (!ok && start !== null) { runs.push([start, s]); start = null; }
    }
    if (start !== null) runs.push([start, track.length]);
    const cols = [], caps = [];
    for (const [s0, s1] of runs) for (let s = s0 + 15; s < s1 - 5; s += 32) {
      track.frame(s, f);
      const top = oppY(f) - 2.8;
      const yaw = yawOf(f.fx, f.fz);
      for (const lat of [OPP_C - HALF * 0.55, OPP_C + HALF * 0.55]) {
        const x = f.x + f.rx * lat, z = f.z + f.rz * lat;
        const gy = T.heightAt(x, z) - 0.5;
        cols.push(trs(x, gy, z, yaw, 1, Math.max(0.5, top - gy), 1));
      }
      const cx = f.x + f.rx * OPP_C, cz = f.z + f.rz * OPP_C;
      caps.push(trs(cx, top + 0.4, cz, yaw, 1, 1, HALF * 2 + 2));
    }
    if (!cols.length) return;
    const colGeo = new THREE.CylinderGeometry(1.1, 1.3, 1, 10);
    colGeo.translate(0, 0.5, 0);
    const capGeo = new THREE.BoxGeometry(2.2, 1.2, 1);
    this.group.add(instanced(colGeo, this.concrete, cols, { cast: true, receive: true }));
    this.group.add(instanced(capGeo, this.concrete, caps, { cast: true, receive: true }));
  }

  buildOppositeMarkings() {
    const { path } = this.ctx;
    const white = [0.92, 0.92, 0.9], yellow = [0.95, 0.72, 0.12];
    const lines = [
      { lat: OPP_C + HALF - 1.2, w: 0.15, c: yellow },
      { lat: OPP_C - HALF + 2.0, w: 0.18, c: white },
    ];
    const lw = ((OPP_C + HALF - 1.2) - (OPP_C - HALF + 2.0)) / 4;
    for (let q = 1; q < 4; q++) lines.push({ lat: OPP_C + HALF - 1.2 - lw * q, w: 0.14, c: white, dash: [3, 12] });
    // On a loop run one metre past the end so the last quad closes the ring.
    this.addMarkings(lines, [[path.u0, path.u1 + (path.loop ? 1 : 0)]], (f) => oppY(f));
  }

  addMarkings(lines, ranges, yOf) {
    const { path } = this.ctx;
    const pos = [], col = [];
    const f0 = {}, f1 = {};
    for (const [a, b] of ranges) {
      for (let s = a; s < b - 1; s += 1) {
        path.frame(s, f0); path.frame(s + 1, f1);
        for (const ln of lines) {
          if (ln.dash && ((s + 0.5) % ln.dash[1] + ln.dash[1]) % ln.dash[1] > ln.dash[0]) continue;
          const q = [];
          for (const fr of [f0, f1]) for (const side of [-1, 1]) {
            const lat = ln.lat + side * ln.w * 0.5;
            q.push([fr.x + fr.rx * lat, yOf(fr, lat) + 0.02, fr.z + fr.rz * lat]);
          }
          // q: [f0-left, f0-right, f1-left, f1-right] → two tris facing up.
          for (const v of [q[0], q[1], q[2], q[1], q[3], q[2]]) { pos.push(...v); col.push(...ln.c); }
        }
      }
    }
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
    g.setAttribute('color', new THREE.Float32BufferAttribute(col, 3));
    g.computeVertexNormals();
    g.computeBoundingSphere();
    const m = this.mat('markings', () => {
      const mm = new THREE.MeshStandardMaterial({
        vertexColors: true, roughness: 0.55, emissive: 0xffffff, emissiveIntensity: 0.0,
        polygonOffset: true, polygonOffsetFactor: -2, polygonOffsetUnits: -2,
      });
      this.ctx.world.addNight(mm, 'emissiveIntensity', 0.0, 0.06);
      return mm;
    });
    this.add(g, m, { receive: true });
  }

  // ── Our lanes continuing past the end of the track ─────────────
  buildForwardExtension() {
    const { path } = this.ctx;
    if (path.u1 <= path.sB) return;
    const r = [[path.sB - 0.5, path.u1]];
    const asphalt = this.mat('asphaltOur', () => new THREE.MeshStandardMaterial({ map: asphaltTexture(2), roughness: 0.88 }));
    const g = sweep(path, r, [
      { lat: -10.25, y: (f) => f.y - 1.6 },
      { lat: -10.25, y: (f) => f.y },
      { lat: 0, y: (f) => f.y },
      { lat: 10.25, y: (f) => f.y },
      { lat: 10.25, y: (f) => f.y - 1.6 },
    ], { step: 6, uS: 4.8, vS: 10 });
    this.add(g, asphalt, { receive: true });
    const conc = this.concrete;
    const right = [...JERSEY.map(([o, dy]) => ({ lat: 9.9 + o, y: (f) => f.y + dy })), { lat: 10.52, y: (f) => f.y - 1.8 }];
    const left = [{ lat: -10.52, y: (f) => f.y - 1.8 }, ...JERSEY.slice().reverse().map(([o, dy]) => ({ lat: -9.9 - o, y: (f) => f.y + dy }))];
    this.add(sweep(path, r, right, { step: 6 }), conc, { cast: true, receive: true });
    this.add(sweep(path, r, left, { step: 6 }), conc, { cast: true, receive: true });
    const white = [0.92, 0.92, 0.9], yellow = [0.95, 0.72, 0.12];
    const L = -HALF + 1.2, R = HALF - 2.0, lw = (R - L) / 4;
    const lines = [{ lat: L, w: 0.15, c: yellow }, { lat: R, w: 0.18, c: white }];
    for (let q = 1; q < 4; q++) lines.push({ lat: L + lw * q, w: 0.14, c: white, dash: [3, 12] });
    this.addMarkings(lines, r, (f, lat) => f.y);
  }

  // ── Sound walls along the outskirts ───────────────────────────
  buildSoundWalls() {
    const { track, path, cityY } = this.ctx;
    // Level 1: the outskirts stretch. Loop: whatever the city asked for.
    let spans = this.ctx.soundWallSpans;
    if (!spans) {
      const out = track.tag('outskirts')[0], merge = track.tag('merge')[0];
      if (!out) return;
      spans = [[(merge ? merge.s0 + 140 : out.s0), out.s1 + 60]];
    }
    const ok = (s) => track.py[track.idx(s)] < cityY + 1.0 && !this.ctx.T.isElevated(track.idx(s));
    const ranges = [];
    for (const [s0, s1] of spans) {
      let st = null;
      for (let s = s0; s <= s1; s += 2) {
        const good = ok(s) && !this.reservedAt(s, 4) && !this.inTunnel(s);
        if (good && st === null) st = s;
        if (!good && st !== null) { ranges.push([st, s]); st = null; }
      }
      if (st !== null) ranges.push([st, s1]);
    }
    const mat = this.mat('soundwall', () => new THREE.MeshStandardMaterial({ map: soundWallTexture(), roughness: 0.95 }));
    const H = 5.2;
    const right = [
      { lat: 11.3, y: (f) => ourY(f, 10.5) - 0.8 },
      { lat: 11.3, y: (f) => ourY(f, 10.5) + H },
      { lat: 11.65, y: (f) => ourY(f, 10.5) + H },
      { lat: 11.65, y: (f) => ourY(f, 10.5) - 0.8 },
    ];
    const left = [
      { lat: OPP_BACK - 1.2, y: (f) => oppY(f) - 0.8 },
      { lat: OPP_BACK - 1.2, y: (f) => oppY(f) + H },
      { lat: OPP_BACK - 0.85, y: (f) => oppY(f) + H },
      { lat: OPP_BACK - 0.85, y: (f) => oppY(f) - 0.8 },
    ];
    for (const r of chunked(ranges, 200)) {
      for (const prof of [right, left]) {
        const g = sweep(path, [r], prof, { step: 4, uv: 'wall', uS: 6, vS: 6 });
        this.add(g, mat, { cast: true, receive: true });
      }
    }
  }

  // ── The Meridian tunnel (cut-and-cover with a park on the lid) ──
  buildTunnel(t, name = 'MERIDIAN TUNNEL') {
    const { path, world, rng } = this.ctx;
    if (!t) return;
    const r = [[t.s0, t.s1]];
    const tiles = this.mat('tunnelTiles', () => {
      const tex = tunnelTileTexture();
      const m = new THREE.MeshStandardMaterial({ map: tex, roughness: 0.3, metalness: 0.0, color: 0xf4f0e6, emissive: 0xffa860, emissiveMap: tex, emissiveIntensity: 0.12 });
      return m;
    });
    const conc = this.concrete;
    const CEIL = 7.0, TOP = 8.4;
    // Heights from the high edge of the (possibly banked) roadway.
    const yb = (f) => f.y + Math.abs(f.bank) * 10.5;
    // Right wall: inner face at +11.4. Profile ascending lat, going up
    // at the inner face (normal faces the road).
    this.add(sweep(path, r, [
      { lat: 11.4, y: (f) => f.y - 1.0 },
      { lat: 11.4, y: (f) => yb(f) + CEIL },
    ], { step: 4, uv: 'wall', uS: 4, vS: 4 }), tiles, { receive: true });
    // Left wall inner face at -33.0 faces +lat: profile going down.
    this.add(sweep(path, r, [
      { lat: -33.0, y: (f) => yb(f) + CEIL },
      { lat: -33.0, y: (f) => f.y - 1.0 },
    ], { step: 4, uv: 'wall', uS: 4, vS: 4 }), tiles, { receive: true });
    // Ceiling underside (faces down → decreasing lat).
    const ceilMat = this.mat('tunnelCeil', () => new THREE.MeshStandardMaterial({ color: 0x3a3632, roughness: 0.9, emissive: 0x2a1606, emissiveIntensity: 1 }));
    // The ceiling faces down, so seen from the moon it shows its back face —
    // which is the side three.js renders into the shadow map. It shades the
    // tunnel (the lid's park trees would otherwise cast through).
    this.add(sweep(path, r, [
      { lat: 11.4, y: (f) => yb(f) + CEIL },
      { lat: -33.0, y: (f) => yb(f) + CEIL },
    ], { step: 4 }), ceilMat, { cast: true });
    // Lid: outer walls + park top.
    const park = this.mat('park', () => new THREE.MeshStandardMaterial({ map: parkTexture(), roughness: 1 }));
    // The lid casts the moon's shadow into the tunnel (and hides the park
    // trees' shadows from it).
    this.add(sweep(path, r, [
      { lat: -36, y: (f) => yb(f) + TOP },
      { lat: 14.5, y: (f) => yb(f) + TOP },
    ], { step: 4, uS: 6, vS: 6 }), park, { cast: true, receive: true });
    // Outer faces of the lid box (right: normal +lat → profile going down; left: going up).
    this.add(sweep(path, r, [
      { lat: 14.5, y: (f) => yb(f) + TOP + 1.1 },
      { lat: 14.5, y: (f) => f.y - 1.2 },
    ], { step: 4, uv: 'wall', uS: 5, vS: 5 }), conc, { cast: true, receive: true });
    this.add(sweep(path, r, [
      { lat: -36, y: (f) => f.y - 1.2 },
      { lat: -36, y: (f) => yb(f) + TOP + 1.1 },
    ], { step: 4, uv: 'wall', uS: 5, vS: 5 }), conc, { cast: true, receive: true });
    // Parapets on the lid edges (inner faces).
    this.add(sweep(path, r, [
      { lat: 14.1, y: (f) => yb(f) + TOP },
      { lat: 14.1, y: (f) => yb(f) + TOP + 1.1 },
      { lat: 14.5, y: (f) => yb(f) + TOP + 1.1 },
    ], { step: 4, uv: 'wall', uS: 5, vS: 5 }), conc);
    this.add(sweep(path, r, [
      { lat: -36, y: (f) => yb(f) + TOP + 1.1 },
      { lat: -35.6, y: (f) => yb(f) + TOP + 1.1 },
      { lat: -35.6, y: (f) => yb(f) + TOP },
    ], { step: 4, uv: 'wall', uS: 5, vS: 5 }), conc);

    // Portals: a band from the ceiling to above the lid, both ends, with
    // wing walls down to the ground at the outer walls.
    const f = {};
    const geo = new GeoBuilder();
    for (const [s, dir] of [[t.s0, -1], [t.s1, 1]]) {
      path.frame(s, f);
      const P = (lat, dy, along = 0) => [f.x + f.rx * lat + f.fx * along, f.y + dy, f.z + f.rz * lat + f.fz * along];
      const d = dir * 0.02;
      // Face points along dir*F. Quad CCW seen from outside.
      // Seen from outside, +lat is on the viewer's right at the entrance
      // (dir < 0) and on the left at the exit — start bottom-left, go CCW.
      const A = P(dir > 0 ? 14.5 : -36, CEIL, d), B = P(dir > 0 ? -36 : 14.5, CEIL, d);
      const C = P(dir > 0 ? -36 : 14.5, TOP + 1.1, d), D = P(dir > 0 ? 14.5 : -36, TOP + 1.1, d);
      geo.quad(A, B, C, D, [[0, 0], [12, 0], [12, 0.6], [0, 0.6]]);
      // Wing columns at both sides (the lid ends are also the outer faces).
      for (const [l0, l1] of [[11.4, 14.5], [-36, -33.0]]) {
        const a = P(dir > 0 ? l1 : l0, -1, d), b = P(dir > 0 ? l0 : l1, -1, d);
        const c = P(dir > 0 ? l0 : l1, CEIL, d), dd = P(dir > 0 ? l1 : l0, CEIL, d);
        geo.quad(a, b, c, dd, [[0, 0], [1, 0], [1, 2], [0, 2]]);
      }
      // Median columns at the portal.
    }
    this.add(geo.build(), conc, { cast: true, receive: true });

    // Portal name sign above our lanes at the entrance.
    path.frame(t.s0, f);
    const signTex = bannerTexture(name, { w: 1024, h: 160, bg: '#0d4f2e', fg: '#ffffff', font: 'bold 96px "Arial Narrow", Arial, sans-serif' });
    const signMat = new THREE.MeshStandardMaterial({ map: signTex, emissive: 0xffffff, emissiveMap: signTex, emissiveIntensity: 0.3, roughness: 0.6 });
    world.addNight(signMat, 'emissiveIntensity', 0.1, 0.45);
    const sign = new THREE.Mesh(new THREE.PlaneGeometry(14, 2.2), signMat);
    sign.position.set(f.x - f.fx * 0.08 + f.rx * 0, f.y + CEIL + 0.75, f.z - f.fz * 0.08 + f.rz * 0);
    sign.rotation.y = Math.atan2(-f.fx, -f.fz);
    sign.updateMatrix(); sign.matrixAutoUpdate = false;
    this.group.add(sign);

    // Median columns.
    const cols = [];
    for (let s = t.s0 + 4; s < t.s1 - 2; s += 9) {
      path.frame(s, f);
      const x = f.x + f.rx * MED_C, z = f.z + f.rz * MED_C;
      cols.push(trs(x, f.y - 0.3, z, yawOf(f.fx, f.fz), 1, yb(f) + CEIL - f.y + 0.3, 1));
    }
    (this.tunnelCols ||= []).push(...cols);

    // Sodium strip lights along the ceiling and warm pools on the road.
    const lamp = this.mat('sodium', () => new THREE.MeshBasicMaterial({ color: new THREE.Color(4.0, 1.9, 0.55) }));
    const strips = [];
    for (let s = t.s0 + 2; s < t.s1 - 2; s += 3.6) {
      path.frame(s, f);
      for (const lat of [-4.6, 4.2, OPP_C - 4.6, OPP_C + 4.6]) {
        strips.push(trs(f.x + f.rx * lat, yb(f) + CEIL - 0.12, f.z + f.rz * lat, yawOf(f.fx, f.fz), 2.3, 0.12, 0.4));
      }
    }
    // Lamps along the underside of both portal lintels.
    for (const [s, dir] of [[t.s0, -1], [t.s1, 1]]) {
      path.frame(s, f);
      for (let lat = -32; lat <= 10.5; lat += 2.6) {
        if (Math.abs(lat - MED_C) < 1.2) continue;
        strips.push(trs(f.x + f.rx * lat + f.fx * dir * 0.35, yb(f) + CEIL - 0.1, f.z + f.rz * lat + f.fz * dir * 0.35, yawOf(f.fx, f.fz), 0.5, 0.14, 1.4));
      }
    }
    (this.tunnelStrips ||= []).push(...strips);
    this.sodium = lamp;
    for (let s = t.s0 + 3; s < t.s1 - 3; s += 7) {
      this.tunnelPools.push({ s, lat: 0, rx: 12, rz: 7, y: 'our', c: [1.0, 0.55, 0.18], k: 0.2 });
      this.tunnelPools.push({ s, lat: OPP_C, rx: 12, rz: 7, y: 'opp', c: [1.0, 0.55, 0.18], k: 0.2 });
    }

    // Park on the lid: trees, paths and lamps.
    for (let s = t.s0 + 8; s < t.s1 - 8; s += 7) {
      for (let k = 0; k < 2; k++) {
        const lat = lerp(-33, 12, rng());
        if (Math.abs(lat - (-10)) < 3) continue; // a path down the middle
        path.frame(s + (rng() - 0.5) * 6, f);
        this.parkTrees.push({ x: f.x + f.rx * lat, y: f.y + TOP, z: f.z + f.rz * lat, s: 0.8 + rng() * 0.6 });
      }
    }
    const pathMat = this.mat('parkPath', () => new THREE.MeshStandardMaterial({ color: 0xa89f8e, roughness: 1, polygonOffset: true, polygonOffsetFactor: -1 }));
    this.add(sweep(path, r, [
      { lat: -12.2, y: (ff) => ff.y + TOP + 0.02 },
      { lat: -8.2, y: (ff) => ff.y + TOP + 0.02 },
    ], { step: 6 }), pathMat, { receive: true });
    for (let s = t.s0 + 10; s < t.s1 - 5; s += 26) {
      for (const lat of [-13, -7.4]) {
        path.frame(s, f);
        this.parkLamps.push({ x: f.x + f.rx * lat, y: f.y + TOP, z: f.z + f.rz * lat });
      }
    }
  }

  // ── Overpasses carrying cross streets over the freeway ────────
  buildOverpass(o) {
    const { track, grid, cityY, world } = this.ctx;
    const f = track.frame(o.s, {});
    // The street is a grid line: constant U (runs along V) or constant V
    // (runs along U). `v` below is the coordinate along the street.
    const fam = o.fam || 'u';
    const U = fam === 'u' ? o.k * grid.pu : o.k * grid.pv;
    const v0 = fam === 'u' ? grid.toV(f.x, f.z) : grid.toU(f.x, f.z);
    const P = fam === 'u' ? (v) => grid.toWorld(U, v) : (v) => grid.toWorld(v, U);
    // lat along the line changes at this rate per metre of v.
    const p0 = P(v0), p1 = P(v0 + 1);
    const latAt = (v) => {
      const p = P(v);
      return (p[0] - f.x) * f.rx + (p[1] - f.z) * f.rz;
    };
    const rate = latAt(v0 + 1) - latAt(v0);
    if (Math.abs(rate) < 0.5) return; // too oblique
    const vForLat = (lat) => v0 + (lat - latAt(v0)) / rate;
    // On the loop streets cross at an angle, so the deck's corners reach
    // further across than its centreline: push the abutments out to match.
    const skew = this.loop ? (W_DECK / 2) * Math.abs((fam === 'u' ? grid.uAxis : grid.vAxis)[0] * f.rx + (fam === 'u' ? grid.uAxis : grid.vAxis)[1] * f.rz) : 0;
    const vR = vForLat(13.2 + skew), vL = vForLat(OPP_BACK - 2.2 - skew);
    const vA = Math.min(vR, vL), vB = Math.max(vR, vL);
    const ROAD = f.y + Math.max(0.6, Math.abs(f.bank) * 10.5 + 0.1);
    const bottom = ROAD + 6.9, top = bottom + 1.3;
    const ground = cityY - 0.25;
    const rampLen = (top - ground) / 0.07;
    const W = 13; // deck width
    const dirU = fam === 'u' ? grid.uAxis : grid.vAxis; // across the deck
    const geo = new GeoBuilder({ color: true });
    const deck = [0.23, 0.23, 0.25], side = [0.72, 0.7, 0.66], under = [0.45, 0.44, 0.42];
    // Height profile along v: ramp up, span, ramp down.
    const H = (v) => {
      if (v < vA) return lerp(ground, top, clamp(1 - (vA - v) / rampLen, 0, 1));
      if (v > vB) return lerp(ground, top, clamp(1 - (v - vB) / rampLen, 0, 1));
      return top;
    };
    const vs = [];
    for (let v = vA - rampLen; v < vA; v += rampLen / 6) vs.push(v);
    vs.push(vA);
    for (let v = vA + 6; v < vB; v += 6) vs.push(v);
    vs.push(vB);
    for (let v = vB + rampLen / 6; v <= vB + rampLen + 0.01; v += rampLen / 6) vs.push(v);
    const edge = (v, s) => { const p = P(v); return [p[0] + dirU[0] * s * W / 2, p[1] + dirU[1] * s * W / 2]; };
    for (let i = 0; i < vs.length - 1; i++) {
      const va = vs[i], vb = vs[i + 1];
      const ha = H(va), hb = H(vb);
      const la = edge(va, -1), ra = edge(va, 1), lb = edge(vb, -1), rb = edge(vb, 1);
      const span = va >= vA - 0.01 && vb <= vB + 0.01;
      // Top surface (normal up). Winding decided by GeoBuilder normal; fix by testing.
      const q = [[la[0], ha, la[1]], [ra[0], ha, ra[1]], [rb[0], hb, rb[1]], [lb[0], hb, lb[1]]];
      pushUp(geo, q, deck);
      // Sides.
      const lowA = span ? bottom : ground - 0.5, lowB = span ? bottom : ground - 0.5;
      pushSide(geo, [la[0], lowA, la[1]], [lb[0], lowB, lb[1]], [lb[0], hb + 1.0, lb[1]], [la[0], ha + 1.0, la[1]], side, P(va));
      pushSide(geo, [ra[0], lowA, ra[1]], [rb[0], lowB, rb[1]], [rb[0], hb + 1.0, rb[1]], [ra[0], ha + 1.0, ra[1]], side, P(va));
      if (span) {
        const u = [[la[0], bottom, la[1]], [ra[0], bottom, ra[1]], [rb[0], bottom, rb[1]], [lb[0], bottom, lb[1]]];
        pushDown(geo, u, under);
      }
    }
    // Abutment faces at the ends of the span (facing the freeway).
    for (const [v, dir] of [[vA, 1], [vB, -1]]) {
      const l = edge(v, -1), r = edge(v, 1);
      const quad = [[l[0], ground - 0.5, l[1]], [r[0], ground - 0.5, r[1]], [r[0], bottom, r[1]], [l[0], bottom, l[1]]];
      const c = P(v + dir * 5);
      pushFacing(geo, quad, side, [c[0], (ground + bottom) / 2, c[1]]);
    }
    const mat = this.mat('overpass', () => new THREE.MeshStandardMaterial({ vertexColors: true, map: concreteTexture(), roughness: 0.9 }));
    this.add(geo.build(), mat, { cast: true, receive: true });

    // Median columns under the span.
    const cols = [];
    const vm = vForLat(MED_C);
    for (const s of [-3.5, 3.5]) {
      const p = P(vm);
      // Along the median (loop) so skewed crossings keep them out of the lanes.
      const ax = this.loop ? f.fx : dirU[0], az = this.loop ? f.fz : dirU[1];
      const x = p[0] + ax * s, z = p[1] + az * s;
      const cr = this.loop ? 0.88 : 1;
      cols.push(trs(x, ROAD - 1.5, z, 0, cr, bottom - ROAD + 1.5, cr));
    }
    (this.overpassCols ||= []).push(...cols);
    // Street lamps on the bridge (collected with the city street lamps).
    o.lamps = [];
    for (let v = vA - rampLen * 0.6; v <= vB + rampLen * 0.6; v += 28) {
      const e = edge(v, -1);
      o.lamps.push({ x: e[0] + dirU[0] * 0.5, y: H(v) + 1.0, z: e[1] + dirU[1] * 0.5, dx: dirU[0], dz: dirU[1] });
    }
    o.v0 = vA - rampLen; o.v1 = vB + rampLen; o.U = U;
  }

  // ── Overhead sign gantries ────────────────────────────────────
  buildGantries() {
    const { track, world } = this.ctx;
    const steel = this.mat('gantrySteel', () => new THREE.MeshStandardMaterial({ color: 0x9aa0a6, metalness: 0.6, roughness: 0.45 }));
    const geo = new GeoBuilder();
    const f = {};
    const signQuads = [];
    for (const gs of this.gantrySites) {
      track.frame(gs.s, f);
      const yaw = Math.atan2(f.fz, f.fx);
      const acrossYaw = Math.atan2(f.rz, f.rx);
      const P = (lat, along = 0) => [f.x + f.rx * lat + f.fx * along, f.z + f.rz * lat + f.fz * along];
      // Highest point of the banked carriageway under the gantry.
      const road = f.y + Math.abs(f.bank) * 10.5;
      // Posts.
      for (const lat of [11.15, MED_C]) {
        const p = P(lat);
        const base = ourY(f, Math.max(-10.5, Math.min(10.5, lat))) - 0.2;
        geo.box(p[0], base, p[1], 0.5, 9.9 - (base - road), 0.5, yaw, { roof: true });
      }
      // Truss chords and verticals.
      const mid = P((11.15 + MED_C) / 2);
      const span = 11.15 - MED_C + 0.6;
      for (const [dy, along] of [[7.6, -0.5], [7.6, 0.5], [9.5, -0.5], [9.5, 0.5]]) {
        const p = P((11.15 + MED_C) / 2, along);
        geo.box(p[0], road + dy, p[1], span, 0.16, 0.16, acrossYaw);
      }
      for (let lat = MED_C + 1; lat < 11; lat += 1.6) {
        for (const along of [-0.5, 0.5]) {
          const p = P(lat, along);
          geo.box(p[0], road + 7.6, p[1], 0.1, 1.9, 0.1, acrossYaw);
        }
      }
      // Signs hanging on the approach side.
      for (const sg of gs.signs) {
        const { texture, aspect } = signTexture(sg.lines, {
          bg: sg.bg || '#0b6b3a', fg: sg.fg || '#fff', border: sg.border || '#fff',
          w: 512, h: 256, arrow: sg.arrow || null,
          font: `bold ${sg.lines.length > 2 ? 54 : 64}px "Arial Narrow", Arial, sans-serif`,
        });
        const h = 3.0, w = h * aspect;
        // Faces oncoming traffic (normal -F); its +u runs along the road's right.
        const p = P(sg.lane, -0.75);
        const cy = road + 6.8 + h / 2;
        signQuads.push({ image: texture.image, c: [p[0], cy, p[1]], rx: f.rx, rz: f.rz, w, h });
        // Backing plate.
        const b = P(sg.lane, -0.65);
        geo.box(b[0], road + 6.75, b[1], w + 0.1, h + 0.1, 0.12, acrossYaw);
        // Sign lights: small lamp arms under the sign.
        this.signLamps = this.signLamps || [];
        for (let k = -1; k <= 1; k += 2) {
          const q = P(sg.lane + k * w * 0.25, -1.4);
          this.signLamps.push({ x: q[0], y: road + 6.68, z: q[1] });
        }
      }
    }
    this.add(geo.build(), steel, { cast: true });
    // All sign faces share one atlas texture, so they cost one draw call.
    const mat = atlasQuads(signQuads, 512, 256, 4, (m) => {
      m.emissiveIntensity = 0.2;
      world.addNight(m, 'emissiveIntensity', 0.05, 0.42);
    });
    if (mat) this.group.add(staticMesh(mat.geo, mat.material));
  }

  // ── Finish gantry ─────────────────────────────────────────────
  // The finish gantry (Level 1) or, on the endless loop, a welcome arch.
  buildFinish(sAt, text, checker) {
    const { track, world, updaters } = this.ctx;
    const f = track.frame(sAt, {});
    const acrossYaw = Math.atan2(f.rz, f.rx);
    const yaw = Math.atan2(f.fz, f.fx);
    const P = (lat, along = 0) => [f.x + f.rx * lat + f.fx * along, f.z + f.rz * lat + f.fz * along];
    const geo = new GeoBuilder();
    const road = f.y + Math.abs(f.bank) * 10.5;
    for (const lat of [11.4, MED_C]) {
      const p = P(lat);
      geo.box(p[0], road - 0.3, p[1], 0.8, 11.2, 0.8, yaw);
    }
    const mid = P((11.4 + MED_C) / 2);
    const span = 11.4 - MED_C + 0.8;
    geo.box(mid[0], road + 9.8, mid[1], span, 0.5, 1.2, acrossYaw);
    geo.box(mid[0], road + 6.9, mid[1], span, 0.35, 1.0, acrossYaw);
    const mat = new THREE.MeshStandardMaterial({ color: 0x202226, metalness: 0.6, roughness: 0.4 });
    this.add(geo.build(), mat, { cast: true });
    // Banner, front and back.
    const tex = checker
      ? bannerTexture(text, { w: 1024, h: 256, checker: true, font: 'italic 900 150px "Arial Narrow", Arial, sans-serif' })
      : bannerTexture(text, { w: 1024, h: 256, bg: '#12082c', fg: '#7cf6ff', font: 'italic 900 132px "Arial Narrow", Arial, sans-serif' });
    const bm = new THREE.MeshStandardMaterial({ map: tex, emissive: 0xffffff, emissiveMap: tex, emissiveIntensity: 0.35, roughness: 0.6, side: THREE.DoubleSide });
    world.addNight(bm, 'emissiveIntensity', 0.15, 0.6);
    const w = span - 0.9, h = w / 4 * 0.62;
    const banner = new THREE.Mesh(new THREE.PlaneGeometry(w, 2.6), bm);
    banner.position.set(mid[0] - f.fx * 0.62, road + 8.4, mid[1] - f.fz * 0.62);
    banner.rotation.y = Math.atan2(-f.fx, -f.fz);
    banner.updateMatrix(); banner.matrixAutoUpdate = false;
    this.group.add(banner);
    // Chase lights along the top and bottom beams.
    const bulbs = [];
    for (let lat = MED_C + 0.4; lat <= 11.0; lat += 0.7) {
      for (const dy of [7.0, 9.8]) {
        const p = P(lat, -0.65);
        bulbs.push(trs(p[0], road + dy, p[1], 0, 0.18, 0.18, 0.18));
      }
    }
    const bulbMat = new THREE.MeshBasicMaterial({ color: new THREE.Color(5, 4, 2.4) });
    const bulbMesh = instanced(new THREE.SphereGeometry(1, 8, 6), bulbMat, bulbs);
    // Per-instance colour for a chase pattern.
    const c = new THREE.Color();
    for (let i = 0; i < bulbs.length; i++) bulbMesh.setColorAt(i, c.setRGB(1, 1, 1));
    this.group.add(bulbMesh);
    let time = 0;
    updaters.push((dt) => {
      time += dt;
      const ph = Math.floor(time * 8);
      for (let i = 0; i < bulbs.length; i++) {
        const on = ((i >> 1) + ph) % 3 === 0;
        bulbMesh.setColorAt(i, c.setScalar(on ? 1.0 : 0.12));
      }
      bulbMesh.instanceColor.needsUpdate = true;
    });
  }

  // ── Billboards ───────────────────────────────────────────────
  buildBillboards() {
    const { track, world, path } = this.ctx;
    const z2 = track.zones[this.ctx.zone].s0, fin = track.finishS;
    const sites = this.billboardSites || [
      { s: z2 + 661, lat: 19, h: 11 }, { s: z2 + 1081, lat: -42, h: 14 }, { s: z2 + 1281, lat: 21, h: 16 },
      { s: z2 + 1781, lat: -43, h: 13 }, { s: fin - 680, lat: 20, h: 12 }, { s: fin - 400, lat: -42, h: 12 },
      { s: fin - 80, lat: 20, h: 13 }, { s: fin + 300, lat: -42, h: 12 },
    ];
    const frame = new GeoBuilder();
    const f = {};
    const boards = [];
    sites.forEach((st, i) => {
      if (st.s > path.u1 - 50) return;
      path.frame(st.s, f);
      const tex = adTexture(i);
      const W = 17, H = W * 384 / 1024;
      const x = f.x + f.rx * st.lat, z = f.z + f.rz * st.lat;
      const gy = this.ctx.T.heightAt(x, z);
      const baseY = Math.max(gy, f.y) + st.h;
      // Face oncoming traffic, toed in 18° toward the road.
      const toe = st.lat > 0 ? 0.32 : -0.32;
      const fx = -f.fx, fz = -f.fz;
      const c = Math.cos(toe), s = Math.sin(toe);
      const nx = fx * c - fz * s, nz = fx * s + fz * c;
      // Frame and poles (plane x axis is perpendicular to the normal).
      const ax = [nz, -nx];
      boards.push({ image: tex.image, c: [x, baseY + H / 2, z], rx: ax[0], rz: ax[1], w: W, h: H });
      const yawAcross = Math.atan2(ax[1], ax[0]);
      frame.box(x - nx * 0.25, baseY - 0.3, z - nz * 0.25, W + 0.6, H + 0.6, 0.4, yawAcross);
      for (const k of [-0.28, 0.28]) {
        const px = x - nx * 0.8 + ax[0] * W * k, pz = z - nz * 0.8 + ax[1] * W * k;
        frame.box(px, gy - 0.5, pz, 0.7, baseY - gy + 0.6, 0.7, yawAcross);
      }
      // Catwalk.
      frame.box(x + nx * 0.5, baseY - 0.5, z + nz * 0.5, W, 0.15, 1.2, yawAcross);
    });
    const mat = this.mat('billboardFrame', () => new THREE.MeshStandardMaterial({ color: 0x3a3c40, metalness: 0.5, roughness: 0.6 }));
    this.add(frame.build(), mat, { cast: true });
    const ads = atlasQuads(boards, 1024, 384, 2, (m) => {
      m.roughness = 0.6;
      m.emissiveIntensity = 0.9;
      world.addNight(m, 'emissiveIntensity', 0.25, 1.35);
    });
    if (ads) this.group.add(staticMesh(ads.geo, ads.material));
  }

  // ── Freeway lighting: median poles with twin arms, glow pools ─
  buildLighting() {
    const { path, world, track, updaters } = this.ctx;
    const f = {};
    const poles = [], heads = [];
    const pools = this.tunnelPools ? this.tunnelPools.slice() : [];
    const blocked = (u) => this.reservedAt(u) || this.inTunnel(u);
    for (let u = path.u0 + 20; u < path.u1 - 10; u += 48) {
      if (blocked(u)) continue;
      path.frame(u, f);
      const x = f.x + f.rx * MED_C, z = f.z + f.rz * MED_C;
      const base = f.ext && u < path.sA ? oppY(f) - 0.2 : Math.min(ourY(f, -10.5), oppY(f)) - 0.2;
      const top = base + 12.2;
      const yaw = yawOf(f.fx, f.fz);
      poles.push({ x, z, base, top, yaw, rx: f.rx, rz: f.rz });
      for (const [lat, yRoad, side] of [[-4.6, 'our', 1], [OPP_C + 5.4, 'opp', -1]]) {
        if (side === 1 && f.ext && u < path.sA) continue; // no eastbound lanes back there
        const c = this.ctx.lampTint?.(u) ?? [1.0, 0.72, 0.4];
        heads.push({ x: f.x + f.rx * (lat + side * 0.8), z: f.z + f.rz * (lat + side * 0.8), y: top - 0.25, yaw, c });
        pools.push({ s: u, lat, rx: 12, rz: 12, y: yRoad, c, k: 0.17 });
      }
    }
    // Retro-reflectors on the barrier faces (amber on the median side, white
    // on the outside), collected for the city's glow sprites.
    this.reflectors = [];
    for (let u = path.u0 + 8; u < path.u1 - 8; u += 16) {
      if (this.inTunnel(u)) continue;
      path.frame(u, f);
      const P = (lat, y, c) => this.reflectors.push([f.x + f.rx * lat, y, f.z + f.rz * lat, c]);
      if (!(f.ext && u < path.sA)) {
        P(9.84, ourY(f, 9.84) + 0.78, 0);
        P(-9.84, ourY(f, -9.84) + 0.78, 1);
      }
      P(OPP_FACE_IN - 0.06, oppY(f) + 0.78, 1);
      P(OPP_FACE_OUT + 0.06, oppY(f) + 0.78, 0);
    }
    // Pole geometry: mast + twin arms, built per pole into one merged mesh.
    const geo = new GeoBuilder();
    for (const p of poles) {
      geo.box(p.x, p.base, p.z, 0.36, p.top - p.base, 0.36, Math.atan2(p.rz, p.rx));
      // Arms reach 6.3 m each way across the road.
      const armYaw = Math.atan2(p.rz, p.rx);
      geo.box(p.x + p.rx * 0.0, p.top - 0.35, p.z + p.rz * 0.0, 13.4, 0.18, 0.18, armYaw);
    }
    for (const h of heads) geo.box(h.x, h.y - 0.1, h.z, 1.3, 0.22, 0.45, h.yaw + Math.PI / 2);
    const poleMat = this.mat('pole', () => new THREE.MeshStandardMaterial({ color: 0x80868c, metalness: 0.6, roughness: 0.5 }));
    this.add(geo.build(), poleMat, { cast: false });
    // Lamp lenses.
    // Lens colour per head (sodium, or white LED where the city asks for it).
    const lens = new THREE.MeshBasicMaterial({ color: new THREE.Color(4.5, 4.5, 4.5) });
    const lensM = heads.map((h) => trs(h.x, h.y - 0.14, h.z, 0, 1.1, 0.06, 0.4));
    const lensC = heads.map((h) => new THREE.Color(...h.c));
    if (this.signLamps) for (const l of this.signLamps) { lensM.push(trs(l.x, l.y, l.z, 0, 0.5, 0.12, 0.3)); lensC.push(new THREE.Color(1, 0.71, 0.4)); }
    const lensMesh = instanced(new THREE.BoxGeometry(1, 1, 1), lens, lensM);
    lensC.forEach((c, i) => lensMesh.setColorAt(i, c));
    this.group.add(lensMesh);
    this.lampHeads = heads;
    world.updaters.push((dt, night) => {
      const k = 0.35 + 0.65 * smoothstep(0.2, 0.7, night);
      lens.color.setScalar(4.5 * k);
    });

    this.buildPools(pools);
  }

  // Additive light pools draped on the road surface.
  buildPools(pools) {
    const { path, world } = this.ctx;
    const pos = [], uv = [], col = [];
    const f = {};
    for (const p of pools) {
      const yOf = p.y === 'opp' ? (fr, lat) => oppY(fr) : (fr, lat) => ourY(fr, lat);
      const N = 2;
      const grid = [];
      for (let a = 0; a <= N; a++) {
        const row = [];
        const u = p.s + (a / N - 0.5) * 2 * p.rz;
        path.frame(u, f);
        for (let b = 0; b <= N; b++) {
          const lat = p.lat + (b / N - 0.5) * 2 * p.rx;
          row.push([f.x + f.rx * lat, yOf(f, lat) + 0.05, f.z + f.rz * lat, b / N, a / N]);
        }
        grid.push(row);
      }
      for (let a = 0; a < N; a++) for (let b = 0; b < N; b++) {
        const A = grid[a][b], B = grid[a][b + 1], C = grid[a + 1][b], D = grid[a + 1][b + 1];
        for (const v of [A, B, C, B, D, C]) {
          pos.push(v[0], v[1], v[2]); uv.push(v[3], v[4]);
          col.push(p.c[0] * p.k, p.c[1] * p.k, p.c[2] * p.k);
        }
      }
    }
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
    g.setAttribute('uv', new THREE.Float32BufferAttribute(uv, 2));
    g.setAttribute('color', new THREE.Float32BufferAttribute(col, 3));
    g.computeBoundingSphere();
    const m = new THREE.MeshBasicMaterial({
      map: glowTexture(), vertexColors: true, transparent: true, depthWrite: false,
      blending: THREE.AdditiveBlending, polygonOffset: true, polygonOffsetFactor: -4, polygonOffsetUnits: -4,
      color: 0xffffff,
    });
    world.updaters.push((dt, night) => m.color.setScalar(smoothstep(0.25, 0.8, night)));
    const mesh = staticMesh(g, m, { receive: false });
    mesh.renderOrder = 2;
    this.group.add(mesh);
  }
}

// Helpers that make a quad face a known direction regardless of how its
// corners were listed.
function pushUp(geo, q, c) {
  const n = normalOf(q);
  if (n[1] < 0) q = [q[0], q[3], q[2], q[1]];
  geo.quad(q[0], q[1], q[2], q[3], [[0, 0], [2, 0], [2, 2], [0, 2]], c);
}
function pushDown(geo, q, c) {
  const n = normalOf(q);
  if (n[1] > 0) q = [q[0], q[3], q[2], q[1]];
  geo.quad(q[0], q[1], q[2], q[3], [[0, 0], [2, 0], [2, 2], [0, 2]], c);
}
// Side wall facing away from centre point `mid` ([x, z]).
function pushSide(geo, a, b, c, d, col, mid) {
  const q = [a, b, c, d];
  const n = normalOf(q);
  const cx = (a[0] + b[0]) / 2 - mid[0], cz = (a[2] + b[2]) / 2 - mid[1];
  if (n[0] * cx + n[2] * cz < 0) { geo.quad(a, d, c, b, [[0, 0], [0, 1], [2, 1], [2, 0]], col); return; }
  geo.quad(a, b, c, d, [[0, 0], [2, 0], [2, 1], [0, 1]], col);
}
function pushFacing(geo, q, col, toward) {
  const n = normalOf(q);
  const cx = (q[0][0] + q[2][0]) / 2, cz = (q[0][2] + q[2][2]) / 2;
  const dx = toward[0] - cx, dz = toward[2] - cz;
  if (n[0] * dx + n[2] * dz < 0) q = [q[0], q[3], q[2], q[1]];
  geo.quad(q[0], q[1], q[2], q[3], [[0, 0], [2, 0], [2, 1], [0, 1]], col);
}
function normalOf(q) {
  const ab = [q[1][0] - q[0][0], q[1][1] - q[0][1], q[1][2] - q[0][2]];
  const ac = [q[2][0] - q[0][0], q[2][1] - q[0][1], q[2][2] - q[0][2]];
  return [ab[1] * ac[2] - ab[2] * ac[1], ab[2] * ac[0] - ab[0] * ac[2], ab[0] * ac[1] - ab[1] * ac[0]];
}

// Concatenate geometries (indexed or not) into one non-indexed geometry with
// the attributes they all share.
function mergeFlat(geos) {
  const flat = geos.map((g) => (g.index ? g.toNonIndexed() : g));
  const names = ['position', 'normal', 'uv', 'color'].filter((n) => flat.every((g) => g.getAttribute(n)));
  const out = new THREE.BufferGeometry();
  for (const n of names) {
    const size = flat[0].getAttribute(n).itemSize;
    const total = flat.reduce((a, g) => a + g.getAttribute(n).count * size, 0);
    const arr = new Float32Array(total);
    let o = 0;
    for (const g of flat) { const a = g.getAttribute(n).array; arr.set(a, o); o += a.length; }
    out.setAttribute(n, new THREE.BufferAttribute(arr, size));
  }
  out.computeBoundingSphere();
  return out;
}

// Pack distinct canvases into one atlas and emit a single mesh of textured
// quads. quads: [{image, c:[x,y,z], rx, rz, w, h}] — each quad faces the
// side whose right-hand vector is (rx, rz).
export function atlasQuads(quads, cw, chh, cols, setup) {
  if (!quads.length) return null;
  const images = [];
  for (const q of quads) if (!images.includes(q.image)) images.push(q.image);
  const rows = Math.ceil(images.length / cols);
  const cv = document.createElement('canvas');
  cv.width = cw * Math.min(cols, images.length);
  cv.height = chh * rows;
  const g = cv.getContext('2d');
  images.forEach((im, k) => g.drawImage(im, (k % cols) * cw, Math.floor(k / cols) * chh, cw, chh));
  const tex = new THREE.CanvasTexture(cv);
  tex.colorSpace = THREE.SRGBColorSpace;
  tex.anisotropy = 8;
  const geo = new GeoBuilder();
  for (const q of quads) {
    const k = images.indexOf(q.image);
    const u0 = ((k % cols) * cw) / cv.width, u1 = ((k % cols + 1) * cw) / cv.width;
    const v1 = 1 - (Math.floor(k / cols) * chh) / cv.height, v0 = 1 - ((Math.floor(k / cols) + 1) * chh) / cv.height;
    const hx = q.rx * q.w / 2, hz = q.rz * q.w / 2, hy = q.h / 2;
    const [x, y, z] = q.c;
    geo.quad([x - hx, y - hy, z - hz], [x + hx, y - hy, z + hz], [x + hx, y + hy, z + hz], [x - hx, y + hy, z - hz],
      [[u0, v0], [u1, v0], [u1, v1], [u0, v1]]);
  }
  const material = new THREE.MeshStandardMaterial({ map: tex, emissive: 0xffffff, emissiveMap: tex, roughness: 0.5 });
  setup(material);
  return { geo: geo.build(), material };
}
