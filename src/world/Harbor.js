import * as THREE from 'three';
import { mergeGeometries } from 'three/addons/utils/BufferGeometryUtils.js';
import { GeoBuilder, instanced, trs, yawOf } from './city/geom.js';
import { makePath, sweep, chunked, oppY, ourY, OPP_C, OPP_FACE_IN, OPP_FACE_OUT, OPP_BACK, MED_C, OPP_LANES } from './city/freeway.js';
import { bannerTexture } from './city/cityTextures.js';
import { signTexture, concreteTexture, asphaltTexture, gravelTexture, glowTexture } from './textures.js';
import { Batch, col, tint, quadOut, beam, frustum, ribbon, spanMatrix } from './harbor/build.js';
import { corrugatedTexture, pavingTexture, containerGeometry, containerMaterial } from './harbor/textures.js';
import { SignAtlas, panel } from './coast/kit.js';
import { mulberry32, clamp, lerp, smoothstep, makeNoise2D } from '../util/math.js';

// Port Meridian (Level 2, zone 2): the harbour bridge and the container docks.
//
// Lateral layout (m from our centreline, + = right, sea on the left):
//   our lanes ±9.4, barriers to ±10.52; median −10.52…−12.02;
//   westbound lanes −12.02…−31.82, barrier back −32.46;
//   bridge girder edges +13.4 / −35.4, tower legs +15.3 / −37.3;
//   container yard −45…−140, quay apron −150…−190 (sea beyond);
//   sheds and depot on the right from +40.

const FWD_EXT = 700;
const JERSEY = [[-0.02, 0], [0.02, 0.25], [0.2, 0.36], [0.28, 1.0], [0.42, 1.0], [0.62, 0.25]];
const GIRDER_R = 13.4, GIRDER_L = -35.4, GIRDER_D = 3.4;
const LEG_R = 15.3, LEG_L = -37.3;
const TOWER_TOP = 160;
const QUAY = -190;          // quay face
const APRON_IN = -148;      // start of the quay apron
const CONT = [12.19, 2.59, 2.44];

const tick = () => new Promise((r) => setTimeout(r, 0));

// Line liveries (weighted by repetition): navy, sky blue, rust red, orange,
// green, greys, reefer white, yellow, teal, brown, magenta, beige.
const CONTAINER_COLS = [0x1d4f8a, 0x2a6fb0, 0x5d9bc7, 0x5d9bc7, 0xb3342b, 0x9c3a2a, 0xd8612a, 0x2f7d3a, 0x2f6d4a, 0x8a8f96, 0x6f757c,
  0xd9d4c7, 0xe6e6e0, 0xe0b52b, 0x1f7f86, 0x6b3b2a, 0x7a4a32, 0xa8306a, 0xc9b98f, 0x36414d, 0x1d4f8a, 0xb3342b];
const RAIL_LATS = [22, 26.5];
const SHED_NAMES = [['MERIDIAN LOGISTICS', '#1d4f7a'], ['PIER 7 COLD STORE', '#2f6d4a'], ['NORDSTAR SHIPPING', '#8a2a24'], ['BAYSIDE CARGO', '#36414d'], ['WAREHOUSE 12', '#6b3b2a']];  // rail spur on the right, between the road and the sheds

export default class Harbor {
  label = 'Building the harbour';

  constructor({ zone = 2 } = {}) {
    this.zone = zone;
  }

  // ── Planning (before terrain fields) ─────────────────────────────
  plan(world) {
    const t = world.track, T = world.terrain;
    this.t = t;
    this.z0 = t.zoneStart[this.zone];
    this.span = t.tag('bridge')[0];
    this.up = t.tag('bridge-up')[0];
    this.down = t.tag('bridge-down')[0];
    // The westbound carriageway starts just before the climb; west of that
    // it peels away to the right (for its traffic) into the terminal gate.
    this.sWS = (this.up ? this.up.s0 : this.z0 + 200) - 40;
    t.runout = Math.max(t.runout, FWD_EXT); // buildForward draws it; drivable after the finish
    this.connector = this.makeConnector(T);
    // Embankment under the connector, kept clear of our viaduct.
    const P = {};
    for (let i = 0; i < this.connector.frames.length; i += 3) {
      const fr = this.connector.frames[i];
      t.project(fr.x, fr.z, this.sWS - 60, P, 60);
      const clear = Math.abs(P.lat) - 10.6;
      const r = clamp(clear - 10, 2, 11), fall = clamp(clear - r - 1, 1.5, 16);
      T.addFlatten(fr.x, fr.z, r, fall, fr.y - 0.35);
    }
    const g = this.connector.gate;
    T.addFlatten(g.x, g.z, 26, 18, g.y - 0.3);
  }

  // Bézier from the westbound carriageway's west end, curving (to its
  // traffic's right) toward the quay and levelling out at the gate plaza.
  makeConnector(T) {
    const t = this.t;
    const f0 = t.frame(this.sWS, {}), fe = t.frame(this.sWS - 150, {});
    const P0 = [f0.x + f0.rx * OPP_C, f0.z + f0.rz * OPP_C];
    const T0 = [-f0.fx, -f0.fz];
    const P3 = [fe.x + fe.rx * -84, fe.z + fe.rz * -84];
    let T3 = [-fe.fx * 0.5 - fe.rx * 0.87, -fe.fz * 0.5 - fe.rz * 0.87];
    const l3 = Math.hypot(T3[0], T3[1]); T3 = [T3[0] / l3, T3[1] / l3];
    const P1 = [P0[0] + T0[0] * 70, P0[1] + T0[1] * 70];
    const P2 = [P3[0] - T3[0] * 50, P3[1] - T3[1] * 50];
    const y0 = oppY(f0), y1 = (T.flatY[this.zone] ?? f0.y) + 0.12;
    const bez = (u) => {
      const v = 1 - u;
      return [
        v * v * v * P0[0] + 3 * v * v * u * P1[0] + 3 * v * u * u * P2[0] + u * u * u * P3[0],
        v * v * v * P0[1] + 3 * v * v * u * P1[1] + 3 * v * u * u * P2[1] + u * u * u * P3[1],
      ];
    };
    const pts = [];
    for (let i = 0; i <= 60; i++) pts.push(bez(i / 60));
    // Extend straight past P3 to the plaza.
    for (let k = 1; k <= 8; k++) pts.push([P3[0] + T3[0] * k * 6, P3[1] + T3[1] * k * 6]);
    let dist = 0;
    const cum = [0];
    for (let i = 1; i < pts.length; i++) { dist += Math.hypot(pts[i][0] - pts[i - 1][0], pts[i][1] - pts[i - 1][1]); cum.push(dist); }
    const bendLen = cum[60];
    const frames = pts.map((p, i) => {
      const a = pts[Math.max(0, i - 1)], b = pts[Math.min(pts.length - 1, i + 1)];
      let fx = b[0] - a[0], fz = b[1] - a[1];
      const l = Math.hypot(fx, fz) || 1;
      fx /= l; fz /= l;
      const y = lerp(y0, y1, smoothstep(0.08, 0.95, Math.min(1, cum[i] / bendLen)));
      return { x: p[0], z: p[1], fx, fz, y };
    });
    const last = frames[frames.length - 1];
    return { frames, P3, T3, gate: { x: P3[0] + T3[0] * 26, z: P3[1] + T3[1] * 26, y: y1, fx: T3[0], fz: T3[1] }, end: last };
  }

  // ── Build ────────────────────────────────────────────────────────
  async build(world) {
    const t = world.track, T = world.terrain;
    this.world = world; this.T = T;
    this.group = new THREE.Group();
    this.group.name = 'harbor';
    world.scene.add(this.group);
    this.B = new Batch(this.group);
    this.rng = mulberry32(8080);
    this.path = makePath(t, this.sWS, t.length, 0, FWD_EXT);
    this.elev = (u) => u >= this.path.sA && u <= this.path.sB && T.isElevated(t.idx(u));
    this.inSpan = (u) => this.span && u > this.span.s0 - 12 && u < this.span.s1 + 12;
    this.groundY = (T.flatY[this.zone] ?? 6) - 0.3;
    this.towers = this.span ? [this.span.s0 + 225, this.span.s1 - 225] : [];
    this.gantrySites = [
      { s: this.z0 + 95, signs: [
        { lines: ['I-9 EAST', 'Harbor Bridge'], arrow: 'up', lane: -3.2 },
        { lines: ['PORT MERIDIAN', 'Terminal 3', '2 MILES'], lane: 4.6 } ] },
      { s: (this.down ? this.down.s1 : this.z0 + 1940) + 110, signs: [
        { lines: ['PORT MERIDIAN', 'Terminal 3'], arrow: 'right', lane: 4.6 },
        { lines: ['DOWNTOWN', 'MERIDIAN 4'], arrow: 'up', lane: -3.2 } ] },
      { s: t.finishS - 320, signs: [
        { lines: ['EXIT 7', 'Terminal Rd'], arrow: 'right', lane: 4.6 },
        { lines: ['I-9 EAST', 'Downtown Meridian'], arrow: 'up', lane: -3.2 } ] },
    ];
    this.reserved = [
      ...this.gantrySites.map((g) => [g.s - 10, g.s + 10]),
      [t.finishS - 10, t.finishS + 10],
      ...this.towers.map((s) => [s - 14, s + 14]),
    ];
    this.mats = {};

    this.buildOpposite();
    this.buildConnector();
    this.buildForward();
    this.buildGirder();
    this.buildOppPiers();
    await tick();
    this.buildTowers();
    this.buildCables();
    this.buildLighting();
    this.buildGantries();
    this.buildFinish();
    await tick();
    this.marks = new GeoBuilder({ color: true }); // painted yard markings, flushed with the markings material
    this.buildPaving();
    this.buildQuay();
    this.buildYard();
    this.buildRTGs();
    this.buildCranes();
    this.buildShip();
    this.buildPortLights();
    await tick();
    this.buildRail();
    this.buildSheds();
    this.buildTrucks();
    if (!this.marks.empty) this.B.add(this.marks.build(), this.markingMat, { receive: true });
    this.buildBoats();
    this.buildBreakwaters();
    this.flushContainers();
    this.B.flush();
    this.group.traverse((o) => {
      if ((o.isMesh || o.isPoints) && !o.userData.animated) { o.matrixAutoUpdate = false; o.updateMatrix(); }
    });

    // For traffic: westbound lanes alongside our track (lat relative to it).
    const tmp = {};
    world.oppositeCarriageway = {
      s0: this.sWS + 40, s1: t.length, lanes: OPP_LANES.slice(), dir: -1,
      y: (s) => oppY(t.frame(s, tmp)),
    };
  }

  mat(key, make) {
    if (!this.mats[key]) this.mats[key] = make();
    return this.mats[key];
  }

  get concrete() {
    return this.mat('concrete', () => new THREE.MeshStandardMaterial({ map: concreteTexture(), roughness: 0.9, color: 0xdcd8d0 }));
  }
  get paint() { // vertex-coloured painted steel
    return this.mat('paint', () => new THREE.MeshStandardMaterial({ vertexColors: true, metalness: 0.35, roughness: 0.5 }));
  }
  get matte() { // vertex-coloured matte surfaces
    return this.mat('matte', () => new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.85 }));
  }

  ranges(test, a, b, step = 2) {
    const out = [];
    let st = null;
    for (let u = a; u <= b; u += step) {
      const ok = test(u);
      if (ok && st === null) st = u;
      if (!ok && st !== null) { out.push([st, u]); st = null; }
    }
    if (st !== null) out.push([st, b]);
    return out;
  }

  // ── Westbound carriageway ────────────────────────────────────────
  buildOpposite() {
    const path = this.path, B = this.B, conc = this.concrete;
    const asphalt = this.mat('asphalt', () => new THREE.MeshStandardMaterial({ map: asphaltTexture(2), roughness: 0.88 }));
    const all = [[path.u0, path.u1]];
    const elev = this.ranges((u) => this.elev(u), path.u0, path.u1);
    const ground = this.ranges((u) => !this.elev(u), path.u0, path.u1);
    for (const r of chunked(all, 300)) {
      B.add(sweep(path, [r], [
        { lat: OPP_FACE_OUT, y: (f) => oppY(f) },
        { lat: OPP_C, y: (f) => oppY(f) },
        { lat: OPP_FACE_IN, y: (f) => oppY(f) },
      ], { step: 4, uS: 4.8, vS: 10 }), asphalt, { receive: true });
    }
    // Median: westbound barrier and the strip back to ours.
    const medProfile = [
      ...JERSEY.map(([o, dy]) => ({ lat: OPP_FACE_IN + o, y: (f) => oppY(f) + dy })),
      { lat: -11.38, y: (f) => oppY(f) + 0.25 },
      { lat: -10.52, y: (f) => ourY(f, -10.52) + 0.25 },
    ];
    for (const r of chunked(all, 300)) B.add(sweep(path, [r], medProfile, { step: 4 }), conc, { cast: true });
    // Outer barrier, with a skirt on the ground and a fascia when elevated.
    const outer = (skirt) => [
      ...(skirt
        ? [{ lat: OPP_BACK - 3.2, y: (f) => oppY(f) - 2.8 }, { lat: OPP_BACK, y: (f) => oppY(f) - 0.02 }]
        : [{ lat: OPP_BACK, y: (f) => oppY(f) - 1.8 }]),
      ...JERSEY.slice().reverse().map(([o, dy]) => ({ lat: OPP_FACE_OUT - o, y: (f) => oppY(f) + dy })),
    ];
    for (const r of chunked(ground, 300)) B.add(sweep(path, [r], outer(true), { step: 4 }), conc, { cast: true });
    for (const r of chunked(elev, 300)) B.add(sweep(path, [r], outer(false), { step: 4 }), conc, { cast: true });
    // Deck underside where elevated (the deep girder hides it on the bridge).
    for (const r of chunked(elev, 300)) {
      B.add(sweep(path, [r], [
        { lat: -10.52, y: (f) => ourY(f, -10.52) - 1.8 },
        { lat: OPP_BACK, y: (f) => oppY(f) - 1.8 },
      ], { step: 4 }), conc);
    }
    // Markings.
    const white = [0.92, 0.92, 0.9], yellow = [0.95, 0.72, 0.12];
    const L = OPP_C + 9.4 - 1.2, R = OPP_C - 9.4 + 2.0, lw = (L - R) / 4;
    const lines = [{ lat: L, w: 0.15, c: yellow }, { lat: R, w: 0.18, c: white }];
    for (let q = 1; q < 4; q++) lines.push({ lat: L - lw * q, w: 0.14, c: white, dash: [3, 12] });
    this.addMarkings(lines, all, (f) => oppY(f));
  }

  get markingMat() {
    return this.mat('markings', () => {
      const m = new THREE.MeshStandardMaterial({
        vertexColors: true, roughness: 0.55, emissive: 0xffffff, emissiveIntensity: 0.0,
        polygonOffset: true, polygonOffsetFactor: -2, polygonOffsetUnits: -2,
      });
      this.world.addNight(m, 'emissiveIntensity', 0.0, 0.06);
      return m;
    });
  }

  addMarkings(lines, ranges, yOf, frameAt = (u, out) => this.path.frame(u, out)) {
    const pos = [], colr = [];
    const f0 = {}, f1 = {};
    for (const [a, b] of ranges) {
      for (let s = a; s < b - 1; s += 1) {
        frameAt(s, f0); frameAt(s + 1, f1);
        for (const ln of lines) {
          if (ln.dash && ((s + 0.5) % ln.dash[1] + ln.dash[1]) % ln.dash[1] > ln.dash[0]) continue;
          const q = [];
          for (const fr of [f0, f1]) for (const side of [-1, 1]) {
            const lat = ln.lat + side * ln.w * 0.5;
            q.push([fr.x + fr.rx * lat, yOf(fr, lat) + 0.02, fr.z + fr.rz * lat]);
          }
          for (const v of [q[0], q[1], q[2], q[1], q[3], q[2]]) { pos.push(...v); colr.push(...ln.c); }
        }
      }
    }
    if (!pos.length) return;
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
    g.setAttribute('color', new THREE.Float32BufferAttribute(colr, 3));
    g.computeVertexNormals();
    this.B.add(g, this.markingMat, { receive: true });
  }

  // ── Westbound connector to the terminal gate ────────────────────
  buildConnector() {
    const C = this.connector, B = this.B, conc = this.concrete;
    const frames = C.frames;
    const asphalt = this.mat('asphalt', () => new THREE.MeshStandardMaterial({ map: asphaltTexture(2), roughness: 0.88 }));
    const W = 9.9;
    B.add(ribbon(frames, [
      { o: -W, y: (fr) => fr.y }, { o: 0, y: (fr) => fr.y }, { o: W, y: (fr) => fr.y },
    ], { uS: 4.8, vS: 10 }), asphalt);
    // Barriers both sides, with retaining walls down into the ground.
    const right = [...JERSEY.map(([o, dy]) => ({ o: W + o, y: (fr) => fr.y + dy })), { o: W + 0.62, y: (fr) => fr.y - 4 }];
    const left = [{ o: -W - 0.62, y: (fr) => fr.y - 4 }, ...JERSEY.slice().reverse().map(([o, dy]) => ({ o: -W - o, y: (fr) => fr.y + dy }))];
    B.add(ribbon(frames, right), conc, { cast: true });
    B.add(ribbon(frames, left), conc, { cast: true });
    // Lane lines.
    const white = [0.92, 0.92, 0.9], yellow = [0.95, 0.72, 0.12];
    const L = -W + 1.2, R = W - 2.0, lw = (R - L) / 4;
    const lines = [{ lat: L, w: 0.15, c: yellow }, { lat: R, w: 0.18, c: white }];
    for (let q = 1; q < 4; q++) lines.push({ lat: L + lw * q, w: 0.14, c: white, dash: [3, 12] });
    const cum = [0];
    for (let i = 1; i < frames.length; i++) cum.push(cum[i - 1] + Math.hypot(frames[i].x - frames[i - 1].x, frames[i].z - frames[i - 1].z));
    const frameAt = (d, out) => {
      let i = 0;
      while (i < cum.length - 2 && cum[i + 1] < d) i++;
      const a = frames[i], b = frames[i + 1], k = clamp((d - cum[i]) / (cum[i + 1] - cum[i] || 1), 0, 1);
      out.x = lerp(a.x, b.x, k); out.z = lerp(a.z, b.z, k); out.y = lerp(a.y, b.y, k);
      const fx = lerp(a.fx, b.fx, k), fz = lerp(a.fz, b.fz, k), l = Math.hypot(fx, fz) || 1;
      out.fx = fx / l; out.fz = fz / l; out.rx = -out.fz; out.rz = out.fx;
      return out;
    };
    this.addMarkings(lines, [[0, cum[cum.length - 1] - 30]], (fr) => fr.y, frameAt);
    this.buildGatePlaza();
  }

  buildGatePlaza() {
    const g = this.connector.gate, geo = new GeoBuilder({ color: true });
    const fx = g.fx, fz = g.fz, rx = -fz, rz = fx;
    const P = (o, a = 0) => [g.x + rx * o + fx * a, g.z + rz * o + fz * a];
    const y = g.y;
    const across = Math.atan2(rz, rx), along = Math.atan2(fz, fx);
    // Canopy on five posts, booths between the lanes.
    const white = col(0xe8e8e4), blue = col(0x1d4f7a), grey = col(0x8a8f96);
    for (const o of [-10.4, -5.2, 0, 5.2, 10.4]) {
      const p = P(o);
      geo.box(p[0], y, p[1], 0.7, 7.2, 0.7, along, { color: grey });
      if (Math.abs(o) < 10) {
        const b = P(o, -2);
        geo.box(b[0], y, b[1], 3.6, 2.8, 1.6, along, { color: white, roofColor: blue });
      }
    }
    const c = P(0);
    geo.box(c[0], y + 7.2, c[1], 12, 1.4, 23, along, { color: white, roofColor: grey });
    geo.box(c[0], y + 8.6, c[1], 13, 0.25, 24, along, { color: blue });
    // Plaza slab beyond.
    const s = P(0, 16);
    geo.box(s[0], y - 0.3, s[1], 30, 0.32, 28, along, { color: col(0x55575b) });
    this.B.add(geo.build(), this.matte, { cast: true });
    // Name board on the canopy facing arriving traffic.
    const { texture, aspect } = signTexture(['PORT MERIDIAN', 'TERMINAL GATE'], { bg: '#123a52', fg: '#ffffff', border: '#ffffff', w: 512, h: 160, font: 'bold 54px "Arial Narrow", Arial, sans-serif' });
    const m = new THREE.MeshStandardMaterial({ map: texture, roughness: 0.6 });
    const h = 1.3, w = h * aspect;
    const board = new THREE.Mesh(new THREE.PlaneGeometry(w, h), m);
    const bp = P(0, -6.2);
    board.position.set(bp[0], y + 7.9, bp[1]);
    board.rotation.y = Math.atan2(-fx, -fz);
    this.group.add(board);
  }

  // Our lanes and barriers carrying on past the end of the track.
  buildForward() {
    const path = this.path, B = this.B, conc = this.concrete;
    if (path.u1 <= path.sB) return;
    const r = [[path.sB - 0.5, path.u1]];
    const asphalt = this.mat('asphalt', () => new THREE.MeshStandardMaterial({ map: asphaltTexture(2), roughness: 0.88 }));
    B.add(sweep(path, r, [
      { lat: -10.25, y: (f) => f.y - 1.6 }, { lat: -10.25, y: (f) => f.y }, { lat: 0, y: (f) => f.y },
      { lat: 10.25, y: (f) => f.y }, { lat: 10.25, y: (f) => f.y - 1.6 },
    ], { step: 6, uS: 4.8, vS: 10 }), asphalt);
    const right = [...JERSEY.map(([o, dy]) => ({ lat: 9.9 + o, y: (f) => f.y + dy })), { lat: 10.52, y: (f) => f.y - 1.8 }];
    B.add(sweep(path, r, right, { step: 6 }), conc, { cast: true });
    const white = [0.92, 0.92, 0.9], yellow = [0.95, 0.72, 0.12];
    const L = -9.4 + 1.2, R = 9.4 - 2.0, lw = (R - L) / 4;
    const lines = [{ lat: L, w: 0.15, c: yellow }, { lat: R, w: 0.18, c: white }];
    for (let q = 1; q < 4; q++) lines.push({ lat: L + lw * q, w: 0.14, c: white, dash: [3, 12] });
    this.addMarkings(lines, r, (f) => f.y);
  }

  // ── Bridge ───────────────────────────────────────────────────────
  // A painted steel box girder under both carriageways wherever the deck
  // is high: cantilevered edge beams carry the stay-cable anchors.
  buildGirder() {
    const t = this.t, path = this.path;
    if (!this.up || !this.down) return;
    const a = this.up.s0 + 60, b = this.down.s1 - 60;
    const top = (f) => ourY(f, 10.52) - 0.06;
    const topO = (f) => oppY(f) - 0.06;
    const prof = [
      { lat: 10.52, y: top }, { lat: GIRDER_R, y: top },
      { lat: GIRDER_R, y: (f) => f.y - 2.2 }, { lat: GIRDER_R - 1.6, y: (f) => f.y - GIRDER_D },
      { lat: GIRDER_L + 1.6, y: (f) => f.y - GIRDER_D }, { lat: GIRDER_L, y: (f) => f.y - 2.2 },
      { lat: GIRDER_L, y: topO }, { lat: OPP_BACK, y: topO },
    ];
    // The sweep wants increasing lat for upward faces; this profile walks
    // right → down → left → up, which gives outward normals throughout.
    const steel = this.mat('girder', () => new THREE.MeshStandardMaterial({ color: 0x356f8c, metalness: 0.45, roughness: 0.45 }));
    for (const r of chunked([[a, b]], 280)) this.B.add(sweep(path, [r], prof, { step: 4, uv: 'wall', uS: 6, vS: 4 }), steel, { cast: true });
    // End faces.
    const geo = new GeoBuilder();
    for (const [s, dir] of [[a, -1], [b, 1]]) {
      const f = t.frame(s, {});
      const q = (lat, y) => [f.x + f.rx * lat, y, f.z + f.rz * lat];
      quadOut(geo, q(GIRDER_R, f.y - 0.06), q(GIRDER_L, f.y - 0.06), q(GIRDER_L, f.y - GIRDER_D), q(GIRDER_R, f.y - GIRDER_D), [f.fx * dir, 0, f.fz * dir]);
    }
    this.B.add(geo.build(), steel);
  }

  buildOppPiers() {
    const t = this.t, T = this.T;
    const runs = this.ranges((u) => this.elev(u), this.sWS, t.length);
    const cols = [], caps = [];
    const f = {};
    for (const [s0, s1] of runs) for (let s = s0 + 15; s < s1 - 5; s += 32) {
      if (this.inSpan(s)) continue;
      t.frame(s, f);
      const top = oppY(f) - 2.8;
      const yaw = yawOf(f.fx, f.fz);
      for (const lat of [OPP_C - 9.4 * 0.55, OPP_C + 9.4 * 0.55]) {
        const x = f.x + f.rx * lat, z = f.z + f.rz * lat;
        const gy = T.heightAt(x, z) - 0.5;
        cols.push(trs(x, gy, z, yaw, 1, Math.max(0.5, top - gy), 1));
      }
      const cx = f.x + f.rx * OPP_C, cz = f.z + f.rz * OPP_C;
      caps.push(trs(cx, top + 0.4, cz, yaw, 1, 1, 9.4 * 2 + 2));
    }
    if (!cols.length) return;
    const colGeo = new THREE.CylinderGeometry(1.1, 1.3, 1, 10);
    colGeo.translate(0, 0.5, 0);
    const capGeo = new THREE.BoxGeometry(2.2, 1.2, 1);
    this.group.add(instanced(colGeo, this.concrete, cols, { cast: true, receive: true }));
    this.group.add(instanced(capGeo, this.concrete, caps, { cast: true, receive: true }));
  }

  // Two H-frame towers: legs outside both carriageways on caissons, a
  // crossbeam carrying the deck and a portal at the top.
  buildTowers() {
    const t = this.t, geo = new GeoBuilder();
    this.warn = [];
    for (const sT of this.towers) {
      const f = t.frame(sT, {});
      const deck = f.y;
      const P = (lat, a = 0) => [f.x + f.rx * lat + f.fx * a, f.z + f.rz * lat + f.fz * a];
      for (const lat of [LEG_R, LEG_L]) {
        const [x, z] = P(lat);
        // Caisson (pile cap) at the waterline.
        const cais = new THREE.CylinderGeometry(7.5, 8.5, 20, 20);
        cais.translate(x, -16 + 10, z);
        this.B.add(cais, this.concrete, { cast: true });
        // Leg: tapered, slightly chamfered look from two stacked frustums.
        frustum(geo, x, z, 4, deck - GIRDER_D - 4, f.fx, f.fz, 11, 6, 9, 5);
        frustum(geo, x, z, deck - GIRDER_D - 4, TOWER_TOP - 6, f.fx, f.fz, 9, 5, 6.2, 3.8);
        frustum(geo, x, z, TOWER_TOP - 6, TOWER_TOP, f.fx, f.fz, 6.2, 3.8, 5.2, 3.2);
        // Pilasters proud of the faces: the cross section becomes a cross,
        // so each face shows a pair of crisp vertical shadow lines.
        frustum(geo, x, z, 6, deck - GIRDER_D - 4, f.fx, f.fz, 6.6, 6.9, 5.6, 5.8);
        frustum(geo, x, z, deck - GIRDER_D - 4, TOWER_TOP - 8, f.fx, f.fz, 5.6, 5.8, 3.4, 4.6);
        frustum(geo, x, z, TOWER_TOP, TOWER_TOP + 1.2, f.fx, f.fz, 5.8, 3.8, 5.8, 3.8);
        this.warn.push([x, TOWER_TOP + 0.6, z]);
      }
      // Crossbeam under the deck and portal beams above it.
      const beamAt = (y, h, d) => {
        const a = P(LEG_L), b = P(LEG_R);
        beam(geo, [a[0], y, a[1]], [b[0], y, b[1]], d, h);
      };
      beamAt(deck - GIRDER_D - 2.2, 4.2, 7);
      beamAt(TOWER_TOP - 10, 5, 4.4);
      beamAt(deck + 50, 3.6, 3.6);
      const mid = P((LEG_L + LEG_R) / 2);
      this.warn.push([mid[0], TOWER_TOP - 7.2, mid[1]]);
    }
    this.B.add(geo.build(), this.concrete, { cast: true, chunk: 2000 });
    // Aircraft warning lights.
    const lamp = new THREE.MeshBasicMaterial({ color: new THREE.Color(6, 0.4, 0.3) });
    const lampMesh = instanced(new THREE.SphereGeometry(0.7, 8, 6), lamp, this.warn.map((p) => trs(p[0], p[1], p[2])));
    this.group.add(lampMesh);
    let time = 0;
    this.world.updaters.push((dt) => {
      time += dt;
      const on = (time % 1.6) < 0.5;
      lamp.color.setRGB(on ? 6 : 0.5, on ? 0.4 : 0.05, on ? 0.3 : 0.04);
    });
  }

  // Semi-fan stay cables in two vertical planes per tower.
  buildCables() {
    const t = this.t, mats = [], anchors = new GeoBuilder({ color: true });
    for (const sT of this.towers) {
      const ft = t.frame(sT, {});
      for (const [legLat, anchorLat, opp] of [[LEG_R, 13.0, false], [LEG_L, -35.0, true]]) {
        for (const dir of [-1, 1]) {
          for (let k = 0; k < 11; k++) {
            const sa = sT + dir * (28 + 18.4 * k);
            const fa = t.frame(sa, {});
            const deckY = (opp ? oppY(fa) : ourY(fa, anchorLat)) + 0.2;
            const A = [fa.x + fa.rx * anchorLat, deckY, fa.z + fa.rz * anchorLat];
            const hy = 108 + 4.4 * k;
            // Each stay is a pair of strands side by side, with a damper
            // sleeve where it meets the deck and an anchor block.
            for (const off of [-0.32, 0.32]) {
              const a2 = [A[0] + fa.rx * off, A[1], A[2] + fa.rz * off];
              const b2 = [ft.x + ft.rx * (legLat + off) + ft.fx * dir * 2.4, hy, ft.z + ft.rz * (legLat + off) + ft.fz * dir * 2.4];
              mats.push(spanMatrix(a2, b2, 0.14));
              const d = [b2[0] - a2[0], b2[1] - a2[1], b2[2] - a2[2]], l = Math.hypot(...d);
              const e = [a2[0] + d[0] / l * 5, a2[1] + d[1] / l * 5, a2[2] + d[2] / l * 5];
              mats.push(spanMatrix(a2, e, 0.3));
            }
            anchors.box(A[0], A[1] - 0.55, A[2], 1.6, 0.7, 1.4, Math.atan2(fa.fz, fa.fx), { color: col(0x356f8c) });
          }
        }
      }
    }
    if (!mats.length) return;
    const m = new THREE.MeshStandardMaterial({ color: 0xf0f0ec, metalness: 0.3, roughness: 0.35 });
    this.group.add(instanced(new THREE.CylinderGeometry(1, 1, 1, 6, 1, true), m, mats));
    this.B.add(anchors.build(), this.paint, { cast: true, chunk: 2000 });
  }

  // ── Road furniture ───────────────────────────────────────────────
  reservedAt(u) { return this.reserved.some(([a, b]) => u > a && u < b); }

  buildLighting() {
    const path = this.path, geo = new GeoBuilder();
    const heads = [];
    const f = {};
    for (let u = path.u0 + 20; u < path.u1 - 10; u += 48) {
      if (this.reservedAt(u)) continue;
      path.frame(u, f);
      const x = f.x + f.rx * MED_C, z = f.z + f.rz * MED_C;
      const base = Math.min(ourY(f, -10.5), oppY(f)) - 0.2;
      const top = base + 12.2;
      const armYaw = Math.atan2(f.rz, f.rx);
      geo.box(x, base, z, 0.36, top - base, 0.36, armYaw);
      geo.box(x, top - 0.35, z, 13.4, 0.18, 0.18, armYaw);
      for (const lat of [-4.6 + 0.8, OPP_C + 5.4 - 0.8]) {
        const hx = f.x + f.rx * lat, hz = f.z + f.rz * lat;
        geo.box(hx, top - 0.35, hz, 1.3, 0.22, 0.45, Math.atan2(f.fz, f.fx));
        heads.push(trs(hx, top - 0.39, hz, 0, 1.1, 0.06, 0.4));
      }
    }
    const poleMat = this.mat('pole', () => new THREE.MeshStandardMaterial({ color: 0x80868c, metalness: 0.6, roughness: 0.5 }));
    this.B.add(geo.build(), poleMat);
    const lens = new THREE.MeshBasicMaterial({ color: new THREE.Color(1.2, 1.1, 0.9) });
    this.group.add(instanced(new THREE.BoxGeometry(1, 1, 1), lens, heads));
    this.world.updaters.push((dt, night) => {
      const k = 0.3 + 0.7 * smoothstep(0.2, 0.7, night);
      lens.color.setRGB(4.5 * k, 3.2 * k, 1.8 * k);
    });
  }

  buildGantries() {
    const t = this.t, world = this.world;
    const steel = this.mat('gantrySteel', () => new THREE.MeshStandardMaterial({ color: 0x9aa0a6, metalness: 0.6, roughness: 0.45 }));
    const geo = new GeoBuilder();
    const f = {};
    for (const gs of this.gantrySites) {
      t.frame(gs.s, f);
      const yaw = Math.atan2(f.fz, f.fx);
      const acrossYaw = Math.atan2(f.rz, f.rx);
      const P = (lat, along = 0) => [f.x + f.rx * lat + f.fx * along, f.z + f.rz * lat + f.fz * along];
      const road = f.y + Math.abs(f.bank) * 10.5;
      for (const lat of [11.15, MED_C]) {
        const p = P(lat);
        const base = Math.min(ourY(f, Math.max(-10.5, Math.min(10.5, lat))), this.T.heightAt(p[0], p[1])) - 0.2;
        geo.box(p[0], base, p[1], 0.5, road + 9.9 - base, 0.5, yaw, { roof: true });
      }
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
      for (const sg of gs.signs) {
        const { texture, aspect } = signTexture(sg.lines, {
          bg: '#0b6b3a', fg: '#fff', border: '#fff', w: 512, h: 256, arrow: sg.arrow || null,
          font: `bold ${sg.lines.length > 2 ? 54 : 64}px "Arial Narrow", Arial, sans-serif`,
        });
        const h = 3.0, w = h * aspect;
        const m = new THREE.MeshStandardMaterial({ map: texture, emissive: 0xffffff, emissiveMap: texture, emissiveIntensity: 0.1, roughness: 0.5 });
        world.addNight(m, 'emissiveIntensity', 0.05, 0.42);
        const mesh = new THREE.Mesh(new THREE.PlaneGeometry(w, h), m);
        const p = P(sg.lane, -0.75);
        mesh.position.set(p[0], road + 6.8 + h / 2, p[1]);
        mesh.rotation.y = Math.atan2(-f.fx, -f.fz);
        this.group.add(mesh);
        const b = P(sg.lane, -0.65);
        geo.box(b[0], road + 6.75, b[1], w + 0.1, h + 0.1, 0.12, acrossYaw);
      }
    }
    this.B.add(geo.build(), steel, { cast: true });
  }

  buildFinish() {
    const t = this.t, world = this.world;
    const f = t.frame(t.finishS, {});
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
    this.B.add(geo.build(), mat, { cast: true });
    const tex = bannerTexture('FINISH', { w: 1024, h: 256, checker: true, font: 'italic 900 150px "Arial Narrow", Arial, sans-serif' });
    const bm = new THREE.MeshStandardMaterial({ map: tex, emissive: 0xffffff, emissiveMap: tex, emissiveIntensity: 0.3, roughness: 0.6, side: THREE.DoubleSide });
    world.addNight(bm, 'emissiveIntensity', 0.15, 0.6);
    const w = span - 0.9;
    const banner = new THREE.Mesh(new THREE.PlaneGeometry(w, 2.6), bm);
    banner.position.set(mid[0] - f.fx * 0.62, road + 8.4, mid[1] - f.fz * 0.62);
    banner.rotation.y = Math.atan2(-f.fx, -f.fz);
    this.group.add(banner);
    const bulbs = [];
    for (let lat = MED_C + 0.4; lat <= 11.0; lat += 0.7) {
      for (const dy of [7.0, 9.8]) {
        const p = P(lat, -0.65);
        bulbs.push(trs(p[0], road + dy, p[1], 0, 0.18, 0.18, 0.18));
      }
    }
    const bulbMat = new THREE.MeshBasicMaterial({ color: new THREE.Color(5, 4, 2.4) });
    const bulbMesh = instanced(new THREE.SphereGeometry(1, 8, 6), bulbMat, bulbs);
    const c = new THREE.Color();
    for (let i = 0; i < bulbs.length; i++) bulbMesh.setColorAt(i, c.setRGB(1, 1, 1));
    this.group.add(bulbMesh);
    let time = 0;
    this.world.updaters.push((dt) => {
      time += dt;
      const ph = Math.floor(time * 8);
      for (let i = 0; i < bulbs.length; i++) bulbMesh.setColorAt(i, c.setScalar(((i >> 1) + ph) % 3 === 0 ? 1.0 : 0.12));
      bulbMesh.instanceColor.needsUpdate = true;
    });
  }

  // ── Container terminal ───────────────────────────────────────────
  get portStart() { return (this.down ? this.down.s1 : this.z0 + 1940) + 40; }

  // Painted mark on the yard: a quad across the path at u, from lat0 to
  // lat1, du0..du1 along it.
  mark(u, lat0, lat1, du0, du1, c, y = this.groundY + 0.07) {
    const f = this.path.frame(u, this._mf || (this._mf = {}));
    const P = (lat, d) => [f.x + f.rx * lat + f.fx * d, y, f.z + f.rz * lat + f.fz * d];
    quadOut(this.marks, P(lat0, du0), P(lat1, du0), P(lat1, du1), P(lat0, du1), [0, 1, 0], c);
  }

  // The yard is paved: concrete slabs from the westbound barrier out to the
  // quay apron, and on the right out past the sheds. Vertex colours carry a
  // broad wash of wear so the 16 m tile doesn't read as a repeat.
  buildPaving() {
    const path = this.path, y = this.groundY + 0.04;
    const a = this.portStart - 40, b = path.u1 - 5;
    const mat = this.mat('paving', () => new THREE.MeshStandardMaterial({
      map: pavingTexture(), vertexColors: true, roughness: 0.92,
      polygonOffset: true, polygonOffsetFactor: -1, polygonOffsetUnits: -1,
    }));
    const n = makeNoise2D(606);
    const bands = [[APRON_IN + 0.3, -35.8], [11.2, 236]];
    // Next to our road the ground rises and falls with its embankment: the
    // paving follows the terrain there so neither shows through the other.
    const T = this.T;
    const yAt = (fr, lat) => (lat > 0 && lat < 20 ? Math.max(y, T.heightAt(fr.x + fr.rx * lat, fr.z + fr.rz * lat) + 0.05) : y);
    for (const [l0, l1] of bands) {
      const lats = l0 > 0 ? [l0, 12.5, 14, 16, 18] : [];
      for (let l = l0 > 0 ? 20 : l0; l < l1; l += 10) lats.push(l);
      lats.push(l1);
      const prof = lats.map((lat) => ({ lat, y: yAt }));
      for (const r of chunked([[a, b]], 300)) {
        const g = sweep(path, [r], prof, { step: 8, uS: 16, vS: 16 });
        const p = g.getAttribute('position'), c = new Float32Array(p.count * 3);
        for (let i = 0; i < p.count; i++) {
          const x = p.getX(i), z = p.getZ(i);
          const v = 0.82 + 0.14 * n(x / 60, z / 60) + 0.06 * n(x / 13 + 7, z / 13);
          c[i * 3] = v; c[i * 3 + 1] = v * 0.99; c[i * 3 + 2] = v * 0.97;
        }
        g.setAttribute('color', new THREE.BufferAttribute(c, 3));
        this.B.add(g, mat, { receive: true, chunk: 1500 });
      }
    }
  }

  // Concrete quay apron with a crisp face to the sea, bollards and fenders,
  // and the crane rails the ship-to-shore cranes run on.
  buildQuay() {
    const path = this.path, g0 = this.groundY + 0.15;
    const a = this.portStart, b = path.u1 - 20;
    for (const r of chunked([[a, b]], 300)) {
      this.B.add(sweep(path, [r], [
        { lat: QUAY, y: () => -2.5 }, { lat: QUAY, y: () => g0 },
        { lat: APRON_IN, y: () => g0 }, { lat: APRON_IN + 0.5, y: () => g0 - 0.6 },
      ], { step: 6, uv: 'road', uS: 8, vS: 8 }), this.concrete, { receive: true });
      // Coping: a dark capping strip along the quay edge.
      this.B.add(sweep(path, [r], [
        { lat: QUAY - 0.05, y: () => g0 - 0.6 }, { lat: QUAY - 0.05, y: () => g0 + 0.18 },
        { lat: QUAY + 0.6, y: () => g0 + 0.18 }, { lat: QUAY + 0.6, y: () => g0 },
      ], { step: 6, color: col(0x4a4c50) }), this.matte, { receive: true });
    }
    const geo = new GeoBuilder({ color: true });
    const f = {};
    const bollards = [];
    for (let u = a + 4; u < b; u += 14) {
      path.frame(u, f);
      bollards.push(trs(f.x + f.rx * (QUAY + 1.3), g0 + 0.18, f.z + f.rz * (QUAY + 1.3), yawOf(f.fx, f.fz)));
      // Cylindrical rubber fender hanging on the face, with its chains.
      const fe = [f.x + f.rx * (QUAY - 0.7), f.z + f.rz * (QUAY - 0.7)];
      geo.box(fe[0], g0 - 3.6, fe[1], 3.2, 1.3, 1.3, Math.atan2(f.fz, f.fx), { color: col(0x1a1a1a) });
      geo.box(fe[0], g0 - 2.3, fe[1], 0.12, 2.4, 0.12, 0, { color: col(0x3a3a3a) });
    }
    this.B.add(geo.build(), this.matte, { cast: false });
    // Bollards: a waisted post with a mushroom head.
    const post = new THREE.CylinderGeometry(0.3, 0.36, 0.62, 10);
    post.translate(0, 0.31, 0);
    const head = new THREE.CylinderGeometry(0.46, 0.34, 0.2, 10);
    head.translate(0, 0.66, 0);
    const bg = tint(mergeGeometries([post.toNonIndexed(), head.toNonIndexed()], false), col(0x2c2d30));
    this.B.add(mergeGeometries(bollards.map((m) => bg.clone().applyMatrix4(m)), false), this.paint, { cast: true });
    this.bollards = bollards.map((m) => new THREE.Vector3().setFromMatrixPosition(m));
    // Crane rails under the crane legs, with yellow safety lines either side,
    // and a hatched strip along the quay edge.
    const yellow = col(0xe0b52b), white = col(0xe8e8e2), steel = col(0x55585c);
    const lines = [];
    for (const lat of [QUAY + 6, QUAY + 30]) {
      lines.push({ lat, w: 0.18, c: steel }, { lat: lat - 1.4, w: 0.15, c: yellow }, { lat: lat + 1.4, w: 0.15, c: yellow });
    }
    lines.push({ lat: QUAY + 2.6, w: 0.2, c: yellow }, { lat: APRON_IN - 1.5, w: 0.2, c: white });
    lines.push({ lat: QUAY + 18, w: 0.15, c: white, dash: [4, 10] });
    this.addMarkings(lines, [[a, b]], () => g0);
  }

  // Stacks of 40 ft boxes in the yard (left) and a depot behind the sheds
  // (right). All containers — yard, ship, trucks and trains — are one
  // InstancedMesh.
  addContainer(x, y, z, yaw, rng) {
    this.containers.push({ x, y, z, yaw, c: CONTAINER_COLS[Math.floor(rng() * CONTAINER_COLS.length)], v: Math.floor(rng() * 4) });
  }

  buildYard() {
    const path = this.path, rng = this.rng, g0 = this.groundY;
    this.containers = [];
    const f = {};
    const a = this.portStart + 30, b = path.u1 - 60;
    const groups = [-48, -73, -98, -123];
    const yellow = col(0xe0b52b), white = col(0xe8e8e2);
    this.yardBlocks = [];
    for (let u = a; u < b; u += 10 * 12.6 + 22) {
      const bays = Math.min(10, Math.floor((b - u) / 12.6) + 1);
      this.yardBlocks.push({ u0: u - 6.3, u1: u + (bays - 1) * 12.6 + 6.3 });
      for (let bay = 0; bay < bays; bay++) {
        const uc = u + bay * 12.6;
        path.frame(uc, f);
        const yaw = yawOf(f.fx, f.fz);
        // Stacks step down toward the block ends and are uneven within it.
        const endBias = Math.min(bay, bays - 1 - bay) < 1 ? -1 : 0;
        for (const g of groups) {
          const blockBias = Math.floor(rng() * 3) - 1;
          for (let row = 0; row < 6; row++) {
            const lat = g - row * 2.6;
            const r = rng();
            let tiers = r < 0.08 ? 0 : r < 0.2 ? 1 : r < 0.38 ? 2 : r < 0.68 ? 3 : r < 0.9 ? 4 : 5;
            tiers = clamp(tiers + endBias + (rng() < 0.5 ? blockBias : 0), 0, 5);
            for (let k = 0; k < tiers; k++) {
              // Boxes never sit perfectly square on each other.
              const jl = (rng() - 0.5) * 0.12, ja = (rng() - 0.5) * 0.3;
              this.addContainer(f.x + f.rx * (lat + jl) + f.fx * ja, g0 + k * CONT[1], f.z + f.rz * (lat + jl) + f.fz * ja, yaw + (rng() - 0.5) * 0.008, rng);
            }
          }
          // Painted slot ends between bays.
          if (bay > 0) this.mark(uc - 6.3, g - 14.4, g + 1.4, -0.08, 0.08, white);
        }
      }
      // Block outlines and aisle centre lines.
      for (const g of groups) {
        const lines = [{ lat: g + 1.5, w: 0.16, c: yellow }, { lat: g - 14.5, w: 0.16, c: yellow }];
        this.addMarkings(lines, [[u - 6.3, u + (bays - 1) * 12.6 + 6.3]], () => g0 + 0.05);
      }
    }
    const aisle = [];
    for (const lat of [-40.5, -67, -92, -117, -141]) aisle.push({ lat, w: 0.14, c: white, dash: [3, 6] });
    this.addMarkings(aisle, [[a - 20, b + 20]], () => g0 + 0.05);
    // Depot on the right, behind the sheds.
    for (let u = this.portStart + 200; u < b; u += 8 * 12.6 + 30) {
      for (let bay = 0; bay < 8; bay++) {
        const uc = u + bay * 12.6;
        if (uc > b) break;
        path.frame(uc, f);
        const yaw = yawOf(f.fx, f.fz);
        for (let row = 0; row < 8; row++) {
          const lat = 132 + row * 2.6;
          const tiers = Math.floor(rng() * 4.6);
          for (let k = 0; k < tiers; k++) this.addContainer(f.x + f.rx * lat, g0 + k * CONT[1], f.z + f.rz * lat, yaw, rng);
        }
      }
    }
    // Straddle carriers parked in the aisles, and high-mast lights.
    const geo = new GeoBuilder({ color: true });
    const yel = col(0xe8b020), dk = col(0x2b2d31), glass = col(0x9ab8c8);
    const masts = [], crowns = [];
    this.straddleU = [];
    let i = 0;
    for (let u = a + 60; u < b; u += 95, i++) {
      path.frame(u, f);
      const along = [f.fx, f.fz], ac = [f.rx, f.rz];
      const lat = [-67, -92, -117][i % 3]; // aisle centres between stack groups
      this.straddleU.push(u);
      const c = [f.x + ac[0] * lat, f.z + ac[1] * lat];
      const ya = Math.atan2(along[1], along[0]);
      for (const [p, q] of [[-4, -2.6], [4, -2.6], [-4, 2.6], [4, 2.6]]) {
        const x = c[0] + along[0] * p + ac[0] * q, z = c[1] + along[1] * p + ac[1] * q;
        geo.box(x, g0 + 1.1, z, 0.7, 11.9, 0.7, 0, { color: yel });
        geo.box(x, g0, z, 1.3, 1.1, 0.7, ya, { color: dk }); // wheel
      }
      for (const q of [-2.6, 2.6]) {
        const x = c[0] + ac[0] * q, z = c[1] + ac[1] * q;
        geo.box(x, g0 + 12, z, 9, 1.2, 0.8, ya, { color: yel });
        geo.box(x, g0 + 1.4, z, 8.6, 0.5, 0.5, ya, { color: yel });
      }
      geo.box(c[0] + ac[0] * 2.6 + along[0] * 4.3, g0 + 9.5, c[1] + ac[1] * 2.6 + along[1] * 4.3, 1.6, 2.2, 1.6, 0, { color: dk });
      geo.box(c[0] + ac[0] * 2.6 + along[0] * 4.3, g0 + 10.2, c[1] + ac[1] * 2.6 + along[1] * 4.3, 1.66, 0.8, 1.66, 0, { color: glass, roof: false });
      geo.box(c[0], g0 + 8.2, c[1], 7.2, 0.35, 2.6, ya, { color: dk }); // spreader
      if (i % 2 === 0) {
        // High mast further along the same aisle.
        path.frame(u + 47, f);
        const mx = f.x + f.rx * lat, mz = f.z + f.rz * lat;
        masts.push(trs(mx, g0, mz, 0, 0.5, 34, 0.5));
        crowns.push(trs(mx, g0 + 34, mz, yawOf(f.fx, f.fz)));
      }
    }
    this.B.add(geo.build(), this.paint, { cast: true });
    const poleMat = this.mat('pole', () => new THREE.MeshStandardMaterial({ color: 0x80868c, metalness: 0.6, roughness: 0.5 }));
    const mastGeo = new THREE.CylinderGeometry(0.6, 1, 1, 8); mastGeo.translate(0, 0.5, 0);
    this.group.add(instanced(mastGeo, poleMat, masts, { cast: true }));
    // Crown: a ring frame carrying six floodlights, lamp faces tilted down.
    const cg = new GeoBuilder();
    cg.box(0, -0.2, 0, 3.6, 0.3, 3.6, 0);
    for (let k = 0; k < 6; k++) {
      const an = (k / 6) * Math.PI * 2;
      cg.box(Math.cos(an) * 1.7, -0.9, Math.sin(an) * 1.7, 1.0, 0.7, 0.8, an);
    }
    const crownMat = new THREE.MeshStandardMaterial({ color: 0x9aa0a6, emissive: 0xfff0d0, emissiveIntensity: 0.1, metalness: 0.5, roughness: 0.4 });
    this.world.addNight(crownMat, 'emissiveIntensity', 0.1, 2.5);
    this.group.add(instanced(cg.build(), crownMat, crowns));
  }

  flushContainers() {
    const list = this.containers || [];
    if (!list.length) return;
    const geo = containerGeometry();
    const im = instanced(geo, containerMaterial(), list.map((c) => trs(c.x, c.y, c.z, c.yaw, CONT[0], CONT[1] - 0.04, CONT[2])), { cast: true, receive: true });
    const c = new THREE.Color();
    const rng = mulberry32(515);
    // A little fade and dirt per box so a row of one colour isn't uniform.
    list.forEach((k, i) => im.setColorAt(i, c.setHex(k.c).multiplyScalar(0.85 + rng() * 0.22)));
    im.instanceColor.needsUpdate = true;
    geo.setAttribute('aVar', new THREE.InstancedBufferAttribute(new Float32Array(list.map((k) => k.v ?? 0)), 1));
    this.group.add(im);
  }

  // Rubber-tyred gantry cranes straddling the yard blocks: six rows and a
  // truck lane under each.
  buildRTGs() {
    const path = this.path, g0 = this.groundY, f = {};
    const geo = new GeoBuilder({ color: true });
    const white = col(0xecebe6), yel = col(0xe8b020), dk = col(0x2b2d31), blue = col(0x1d4f7a), glass = col(0x9ab8c8);
    const groups = [-48, -73, -98, -123];
    const lamps = [];
    let k = 0;
    for (const blk of this.yardBlocks || []) {
      for (const g of groups) {
        k++;
        if (k % 3 === 0) continue;
        const u = lerp(blk.u0 + 12, blk.u1 - 12, ((k * 0.618) % 1));
        if (this.straddleU.some((s) => Math.abs(s - u) < 14)) continue;
        path.frame(u, f);
        const ya = Math.atan2(f.fz, f.fx);
        const latA = g + 3.2, latB = g - 20.2, H = 19;
        const P = (lat, d, y) => [f.x + f.rx * lat + f.fx * d, y, f.z + f.rz * lat + f.fz * d];
        for (const lat of [latA, latB]) {
          for (const d of [-3.6, 3.6]) {
            beam(geo, P(lat, d, g0 + 1.2), P(lat, d * 0.8, g0 + H), 0.8, 0.8, white);
            const w = P(lat, d, 0);
            geo.box(w[0], g0, w[2], 1.6, 1.3, 0.9, ya, { color: dk }); // tyres
          }
          beam(geo, P(lat, -4.4, g0 + 1.6), P(lat, 4.4, g0 + 1.6), 0.9, 1.0, white); // sill beam
          beam(geo, P(lat, -3.2, g0 + H), P(lat, 3.2, g0 + H), 0.8, 0.8, white);
          const eh = P(lat, -4.6, 0);
          geo.box(eh[0], g0 + 1.2, eh[2], 1.6, 2.2, 2.0, ya, { color: yel }); // engine house
        }
        for (const d of [-2.2, 2.2]) beam(geo, P(latA + 1, d, g0 + H + 0.8), P(latB - 1, d, g0 + H + 0.8), 1.1, 1.6, white);
        // Trolley with its cab, hoist ropes and spreader over the stacks.
        const tl = lerp(latA, latB, 0.3 + ((k * 0.37) % 0.5));
        const tr = P(tl, 0, 0);
        geo.box(tr[0], g0 + H + 1.6, tr[2], 5.2, 1.6, 4.4, ya, { color: yel });
        const cab = P(tl, 2.6, 0);
        geo.box(cab[0], g0 + H - 1.6, cab[2], 1.8, 2.0, 1.8, ya, { color: glass, roofColor: white });
        const sy = g0 + 5 * CONT[1] + 2 + ((k * 3) % 5);
        for (const [d, o] of [[-1.5, -0.8], [1.5, -0.8], [-1.5, 0.8], [1.5, 0.8]]) beam(geo, P(tl + o, d, sy + 0.3), P(tl + o, d, g0 + H + 1.6), 0.07, 0.07, dk, false);
        const sp = P(tl, 0, 0);
        geo.box(sp[0], sy, sp[2], 12.2, 0.4, 2.5, ya, { color: yel });
        // Name band on the girder.
        for (const d of [-2.2, 2.2]) beam(geo, P(latA + 0.5, d * 1.26, g0 + H + 0.9), P(latA + 8, d * 1.26, g0 + H + 0.9), 0.05, 1.0, blue, false);
        lamps.push(P(latA, 0, g0 + H + 2.7), P(latB, 0, g0 + H + 2.7));
      }
    }
    if (geo.empty) return;
    this.B.add(geo.build(), this.paint, { cast: true, chunk: 1500 });
    this.amberBeacons = (this.amberBeacons || []).concat(lamps);
  }

  // Ship-to-shore cranes along the quay, booms reaching over the water.
  buildCranes() {
    const path = this.path, g0 = this.groundY + 0.15;
    const geo = new GeoBuilder({ color: true });
    const red = col(0xc23a2e), white = col(0xe9e9e4), dk = col(0x2b2d31), blue = col(0x1d4f7a), glass = col(0x8fb0c4), grey = col(0x7a7e84);
    const f = {};
    const sites = this.craneSites();
    const beacons = [], floods = [];
    sites.forEach((u, ci) => {
      path.frame(u, f);
      const al = [f.fx, 0, f.fz], out = [-f.rx, 0, -f.rz]; // "out" = toward the sea
      const O = [f.x + f.rx * (QUAY + 18), g0, f.z + f.rz * (QUAY + 18)];
      const at = (o, a, y) => [O[0] + out[0] * o + al[0] * a, y, O[2] + out[2] * o + al[2] * a];
      const yawOut = Math.atan2(out[2], out[0]), yawAl = Math.atan2(al[2], al[0]);
      // Portal legs (water side o=+12, land side o=-12), 18 m apart along the quay.
      for (const o of [-12, 12]) for (const a of [-9, 9]) {
        beam(geo, at(o, a, g0 + 2.4), at(o, a, g0 + 46), 1.6, 1.6, white);
        // Bogies on the rail: an equaliser beam over two wheel trucks.
        const p = at(o, a, 0);
        geo.box(p[0], g0 + 1.4, p[2], 5.2, 1.0, 1.4, yawAl, { color: red });
        for (const d of [-1.6, 1.6]) { const q = at(o, a + d, 0); geo.box(q[0], g0, q[2], 1.8, 1.4, 1.1, yawAl, { color: dk }); }
      }
      for (const a of [-9, 9]) beam(geo, at(-12, a, g0 + 45), at(12, a, g0 + 45), 1.8, 2.2, white);
      for (const o of [-12, 12]) beam(geo, at(o, -9, g0 + 2.4), at(o, 9, g0 + 2.4), 1.4, 1.4, white);
      for (const o of [-12, 12]) beam(geo, at(o, -9, g0 + 44), at(o, 9, g0 + 44), 1.4, 1.4, white);
      // Portal ties at mid height and diagonal bracing on the ends.
      for (const a of [-9, 9]) {
        beam(geo, at(-12, a, g0 + 4), at(12, a, g0 + 40), 0.8, 0.8, white);
        beam(geo, at(-12, a, g0 + 24), at(12, a, g0 + 24), 0.9, 0.9, white);
      }
      for (const o of [-12, 12]) {
        beam(geo, at(o, -9, g0 + 30), at(o, 0, g0 + 44), 0.6, 0.6, white);
        beam(geo, at(o, 9, g0 + 30), at(o, 0, g0 + 44), 0.6, 0.6, white);
      }
      // Stair tower up the land-side leg.
      for (let y = g0 + 3; y < g0 + 44; y += 3.2) {
        const s0 = at(-13.4, -9, 0), s1 = at(-13.4, -6.6, 0);
        beam(geo, [s0[0], y, s0[2]], [s1[0], y + 1.6, s1[2]], 0.9, 0.12, grey, false);
        beam(geo, [s1[0], y + 1.6, s1[2]], [s0[0], y + 3.2, s0[2]], 0.9, 0.12, grey, false);
      }
      // A-frame over the land-side legs.
      for (const a of [-7, 7]) {
        beam(geo, at(-12, a, g0 + 46), at(-6, a, g0 + 82), 1.2, 1.2, red);
        beam(geo, at(8, a, g0 + 46), at(-6, a, g0 + 82), 1.2, 1.2, red);
        beam(geo, at(-9, a, g0 + 64), at(1, a, g0 + 64), 0.7, 0.7, red);
      }
      beam(geo, at(-6, -7, g0 + 82), at(-6, 7, g0 + 82), 1.2, 1.2, red);
      beam(geo, at(-6, -7, g0 + 64), at(-6, 7, g0 + 64), 0.7, 0.7, red);
      // Boom (two box girders) from back-reach to far over the water, with a
      // lacing of diagonals between the cross members and a walkway.
      for (const a of [-3.2, 3.2]) beam(geo, at(-34, a, g0 + 52), at(66, a, g0 + 52), 1.4, 2.6, red);
      for (let o = -30; o <= 62; o += 8) {
        beam(geo, at(o, -3.2, g0 + 51.2), at(o, 3.2, g0 + 51.2), 0.5, 0.5, red, false);
        if (o < 62) beam(geo, at(o, -3.2, g0 + 51.1), at(o + 8, 3.2, g0 + 51.1), 0.35, 0.35, red, false);
      }
      for (const a of [-4.4, 4.4]) {
        beam(geo, at(-34, a, g0 + 51.0), at(64, a, g0 + 51.0), 0.9, 0.1, grey, false);
        beam(geo, at(-34, a * 1.1, g0 + 52.1), at(64, a * 1.1, g0 + 52.1), 0.06, 0.06, grey, false);
      }
      // Forestays and backstays.
      for (const a of [-3.2, 3.2]) {
        beam(geo, at(-6, a * 1.8, g0 + 82), at(64, a, g0 + 53.4), 0.35, 0.35, dk, false);
        beam(geo, at(-6, a * 1.8, g0 + 82), at(28, a, g0 + 53.4), 0.35, 0.35, dk, false);
        beam(geo, at(-6, a * 1.8, g0 + 82), at(-32, a, g0 + 53.4), 0.35, 0.35, dk, false);
      }
      // Machinery house with a louvred band, trolley, cab, and the spreader
      // hanging on its ropes.
      const mh = at(-26, 0, 0);
      geo.box(mh[0], g0 + 53.4, mh[2], 11, 6.5, 8, yawOut, { color: white, roofColor: blue });
      geo.box(mh[0], g0 + 57.2, mh[2], 11.1, 1.0, 8.1, yawOut, { color: blue, roof: false });
      const to = 14 + ((u * 0.37) % 34);
      const tr = at(to, 0, 0);
      geo.box(tr[0], g0 + 49.2, tr[2], 5, 3.4, 6.5, yawOut, { color: blue });
      geo.box(tr[0], g0 + 46.2, tr[2], 2.2, 2.6, 2.2, yawOut, { color: white });
      const cw = at(to + 1.15, 0, 0);
      geo.box(cw[0], g0 + 46.6, cw[2], 0.1, 1.8, 2.0, yawOut, { color: glass, roof: false });
      const sy = g0 + 14 + ((ci * 11) % 22);
      for (const [d, a] of [[-1.2, -1.1], [1.2, -1.1], [-1.2, 1.1], [1.2, 1.1]]) beam(geo, at(to + d, a, sy + 0.4), at(to + d, a, g0 + 49.2), 0.08, 0.08, dk, false);
      const sp = at(to, 0, 0);
      geo.box(sp[0], sy, sp[2], 2.6, 0.5, 12.2, yawOut, { color: col(0xe8b020) });
      beacons.push(at(66, 0, g0 + 54.2), at(-6, 0, g0 + 83.4));
      for (let o = 2; o <= 58; o += 14) floods.push(at(o, 0, g0 + 50.4));
    });
    this.B.add(geo.build(), this.paint, { cast: true, chunk: 2000 });
    this.redBeacons = beacons;
    this.craneFloods = floods;
  }

  // Lamps on the cranes and gantries: red aircraft beacons that blink, amber
  // gantry beacons, and floodlights under the booms — fixed-size glow points
  // so they read from a kilometre away, plus lamp faces that glow at night.
  buildPortLights() {
    const red = this.redBeacons || [], amber = this.amberBeacons || [], flood = this.craneFloods || [];
    const all = [...red.map((p) => [p, [2.2, 0.15, 0.1]]), ...amber.map((p) => [p, [1.9, 1.0, 0.2]]), ...flood.map((p) => [p, [1.6, 1.45, 1.2]]),
      ...(this.shipLamps || []).map((p) => [p, [1.7, 1.6, 1.4]])];
    if (!all.length) return;
    const pos = new Float32Array(all.length * 3), colr = new Float32Array(all.length * 3), base = new Float32Array(all.length * 3);
    all.forEach(([p, c], i) => { pos.set(p, i * 3); base.set(c, i * 3); colr.set(c, i * 3); });
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.BufferAttribute(pos, 3));
    g.setAttribute('color', new THREE.BufferAttribute(colr, 3));
    const m = new THREE.PointsMaterial({ size: 7, sizeAttenuation: false, map: glowTexture(), vertexColors: true, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending });
    const pts = new THREE.Points(g, m);
    pts.frustumCulled = false;
    this.group.add(pts);
    let time = 0;
    const nR = red.length, nA = amber.length;
    this.world.updaters.push((dt, night) => {
      time += dt;
      const blink = (time % 1.6) < 0.5 ? 1 : 0.12;
      const lit = 0.25 + 0.75 * smoothstep(0.1, 0.6, night);
      for (let i = 0; i < all.length; i++) {
        const k = i < nR ? blink : i < nR + nA ? (((time + i * 0.37) % 1.2) < 0.6 ? 1 : 0.15) * lit : lit * 0.9;
        colr[i * 3] = base[i * 3] * k; colr[i * 3 + 1] = base[i * 3 + 1] * k; colr[i * 3 + 2] = base[i * 3 + 2] * k;
      }
      g.attributes.color.needsUpdate = true;
    });
  }

  craneSites() {
    if (!this._cranes) {
      const mid = this.shipMidU();
      this._cranes = [-110, -55, 0, 55, 110].map((d) => mid + d);
    }
    return this._cranes;
  }

  shipMidU() { return Math.min(this.t.finishS - 60, this.portStart + 480); }

  // A container ship moored along the quay: hull, deck stacks, accommodation
  // block and funnel. Placed along the chord of the (slightly curved) quay.
  buildShip() {
    const path = this.path, f = {};
    const mid = this.shipMidU(), Lship = 270, Bm = 38;
    const qa = path.frame(mid - Lship / 2, {}), qb = path.frame(mid + Lship / 2, {});
    const A = [qa.x + qa.rx * QUAY, qa.z + qa.rz * QUAY], Bq = [qb.x + qb.rx * QUAY, qb.z + qb.rz * QUAY];
    let dx = Bq[0] - A[0], dz = Bq[1] - A[1];
    const len = Math.hypot(dx, dz); dx /= len; dz /= len;
    // Seaward normal: the side of the chord away from the road.
    let nx = -dz, nz = dx;
    path.frame(mid, f);
    if ((nx * -f.rx + nz * -f.rz) < 0) { nx = -nx; nz = -nz; }
    // How far the quay bulges past the chord.
    let bulge = 0;
    for (let u = mid - Lship / 2; u <= mid + Lship / 2; u += 10) {
      path.frame(u, f);
      const qx = f.x + f.rx * QUAY, qz = f.z + f.rz * QUAY;
      bulge = Math.max(bulge, (qx - A[0]) * nx + (qz - A[1]) * nz);
    }
    const cx = (A[0] + Bq[0]) / 2 + nx * (bulge + 3 + Bm / 2), cz = (A[1] + Bq[1]) / 2 + nz * (bulge + 3 + Bm / 2);
    const P = (x, y, z) => [cx + dx * x + nx * z, y, cz + dz * x + nz * z]; // x along ship, z across (toward sea)
    const hullC = col(0x1f3550), boot = col(0x9c2b24), deckC = col(0x6b6e70), whiteC = col(0xeeeeea), dk = col(0x23262b);
    const geo = new GeoBuilder({ color: true });
    const half = (x) => {
      // Beam along the hull: full amidships, fine bow, slightly narrower stern.
      const t = (x + Lship / 2) / Lship;
      if (t > 0.82) return Bm / 2 * Math.sqrt(Math.max(0.0004, 1 - ((t - 0.82) / 0.18) ** 2));
      if (t < 0.06) return Bm / 2 * (0.86 + 0.14 * t / 0.06);
      return Bm / 2;
    };
    const N = 40;
    const xs = Array.from({ length: N + 1 }, (_, i) => -Lship / 2 + (Lship * i) / N);
    const bands = [[-11, -1.8, boot], [-1.8, 0.9, boot], [0.9, 13, hullC]];
    for (let i = 0; i < N; i++) {
      const x0 = xs[i], x1 = xs[i + 1], h0 = half(x0), h1 = half(x1);
      for (const side of [-1, 1]) {
        for (const [y0, y1, c] of bands) {
          quadOut(geo, P(x0, y0, side * h0), P(x1, y0, side * h1), P(x1, y1, side * h1), P(x0, y1, side * h0), [nx * side, 0, nz * side], c);
        }
      }
      // Deck.
      quadOut(geo, P(x0, 13, -h0), P(x1, 13, -h1), P(x1, 13, h1), P(x0, 13, h0), [0, 1, 0], deckC);
    }
    // Transom.
    const hs = half(-Lship / 2);
    quadOut(geo, P(-Lship / 2, -11, -hs), P(-Lship / 2, -11, hs), P(-Lship / 2, 13, hs), P(-Lship / 2, 13, -hs), [-dx, 0, -dz], hullC);
    // Accommodation block near the stern, bridge wings, funnel.
    const yawShip = Math.atan2(dz, dx);
    const sb = P(-Lship / 2 + 30, 13, 0);
    geo.box(sb[0], 13, sb[2], 16, 24, Bm * 0.82, yawShip, { color: whiteC, roofColor: col(0xb0b2b4) });
    geo.box(sb[0], 34, sb[2], 8, 3, Bm + 3, yawShip, { color: whiteC });
    for (let k = 0; k < 7; k++) {
      // Window bands on the forward face.
      const p = P(-Lship / 2 + 38.05, 0, 0);
      geo.box(p[0], 15 + k * 3.2, p[2], 0.1, 1.2, Bm * 0.78, yawShip, { color: dk, roof: false });
    }
    const fn = P(-Lship / 2 + 14, 0, 0);
    geo.box(fn[0], 13, fn[2], 8, 30, 7, yawShip, { color: col(0xb3342b) });
    geo.box(fn[0], 43, fn[2], 8.2, 2.5, 7.2, yawShip, { color: dk });
    // Sheer stripe under the deck edge, draft marks at bow and stern.
    for (let i = 0; i < N; i++) {
      const x0 = xs[i], x1 = xs[i + 1], h0 = half(x0), h1 = half(x1);
      for (const side of [-1, 1]) {
        const o = side * 0.04;
        quadOut(geo, P(x0, 11.6, side * h0 + o), P(x1, 11.6, side * h1 + o), P(x1, 12.1, side * h1 + o), P(x0, 12.1, side * h0 + o), [nx * side, 0, nz * side], whiteC);
        // Deck railing: posts every segment and a top rail.
        const pp = P(x0, 13, side * (h0 - 0.3));
        geo.box(pp[0], 13, pp[2], 0.08, 1.1, 0.08, 0, { color: whiteC, roof: false });
        beam(geo, P(x0, 14.1, side * (h0 - 0.3)), P(x1, 14.1, side * (h1 - 0.3)), 0.07, 0.07, whiteC, false);
      }
    }
    for (const xm of [Lship / 2 - 14, -Lship / 2 + 4]) {
      for (let k = 0; k < 8; k++) {
        for (const side of [-1, 1]) {
          const p = P(xm, 0, side * (half(xm) + 0.06));
          geo.box(p[0], -1.5 + k * 1.1, p[2], 0.6, 0.12, 0.12, yawShip, { color: whiteC, roof: false });
        }
      }
    }
    // Forecastle with a breakwater, and a foremast carrying a lamp.
    const fc = P(Lship / 2 - 16, 0, 0);
    geo.box(fc[0], 13, fc[2], 22, 2.6, Bm * 0.7, yawShip, { color: hullC, roofColor: deckC });
    const bw = P(Lship / 2 - 28, 0, 0);
    geo.box(bw[0], 13, bw[2], 0.6, 4.2, Bm * 0.8, yawShip, { color: whiteC });
    const fm = P(Lship / 2 - 12, 0, 0);
    geo.box(fm[0], 15.6, fm[2], 0.6, 11, 0.6, yawShip, { color: col(0xd8a020) });
    // Wheelhouse windows wrapping the top of the accommodation, a radar mast
    // above, and orange lifeboats in davits on both sides.
    const acc = (x, y, z) => P(-Lship / 2 + 30 + x, y, z);
    for (const side of [-1, 1]) {
      const wv = acc(0, 0, side * (Bm * 0.41 + 0.05));
      geo.box(wv[0], 31.6, wv[2], 15, 1.6, 0.1, yawShip, { color: dk, roof: false });
      const lb = acc(-3, 0, side * (Bm * 0.41 + 2.4));
      geo.box(lb[0], 21.5, lb[2], 8, 2.2, 2.6, yawShip, { color: col(0xf07a1a), roofColor: col(0xf29a40) });
      for (const d of [-3, 3]) {
        const dv = acc(-3 + d, 0, side * (Bm * 0.41 + 0.6));
        const dv2 = acc(-3 + d, 0, side * (Bm * 0.41 + 2.6));
        beam(geo, [dv[0], 21, dv[2]], [dv[0], 25.5, dv[2]], 0.3, 0.3, whiteC);
        beam(geo, [dv[0], 25.5, dv[2]], [dv2[0], 24.2, dv2[2]], 0.25, 0.25, whiteC);
      }
    }
    const fw = acc(8.05, 0, 0);
    geo.box(fw[0], 31.6, fw[2], 0.1, 1.6, Bm * 0.8, yawShip, { color: dk, roof: false });
    const rm = acc(0, 0, 0);
    beam(geo, [rm[0], 37, rm[2]], [rm[0], 45, rm[2]], 0.5, 0.5, whiteC);
    const rb = acc(0, 0, 0);
    geo.box(rb[0], 42, rb[2], 0.5, 0.3, 7, yawShip, { color: whiteC });
    geo.box(rb[0], 44.2, rb[2], 4.4, 0.25, 0.4, yawShip + 0.6, { color: dk });
    // Funnel: company band.
    geo.box(fn[0], 33, fn[2], 8.15, 4, 7.15, yawShip, { color: whiteC, roof: false });
    this.shipLamps = [acc(0, 45.4, 0), P(Lship / 2 - 12, 26.8, 0), P(-Lship / 2 + 2, 15, 0)];
    // Deck containers: bays forward of the accommodation, each on a hatch
    // cover, with lashing bridges between bays.
    const rng = mulberry32(99);
    const yawC = -yawShip;
    const lash = col(0x9a9ea3);
    for (let x = -Lship / 2 + 46; x < Lship / 2 - 42; x += 13.2) {
      const rows = Math.floor((half(x + 6) * 2 - 2) / 2.5);
      const tiers = 3 + Math.floor(rng() * 4);
      const hc = P(x + 6.1, 0, 0);
      geo.box(hc[0], 13, hc[2], 12.8, 0.9, rows * 2.5 + 0.6, yawShip, { color: col(0x55616b) });
      if (x > -Lship / 2 + 46) {
        const lz = (rows * 2.5) / 2 + 0.3;
        for (let q = -1; q <= 1; q += 2 / 3) {
          const lp = P(x - 0.4, 0, q * lz);
          geo.box(lp[0], 13.9, lp[2], 0.4, 5.4, 0.4, yawShip, { color: lash });
        }
        beam(geo, P(x - 0.4, 19.2, -lz), P(x - 0.4, 19.2, lz), 0.5, 0.35, lash);
        beam(geo, P(x - 0.4, 16.6, -lz), P(x - 0.4, 16.6, lz), 0.3, 0.25, lash, false);
      }
      for (let r = 0; r < rows; r++) {
        const z = -((rows - 1) * 2.5) / 2 + r * 2.5;
        const tt = Math.max(1, tiers - (Math.abs(z) > Bm * 0.4 ? 1 : 0) - (rng() < 0.12 ? 1 : 0));
        for (let k = 0; k < tt; k++) {
          const p = P(x + 6.1, 13.9 + k * CONT[1], z);
          this.addContainer(p[0], p[1], p[2], yawC, rng);
        }
      }
    }
    this.B.add(geo.build(), this.paint, { cast: true, chunk: 3000 });
    // Name on the bow.
    const { texture, aspect } = signTexture(['MERIDIAN STAR'], { bg: '#1f3550', fg: '#ffffff', border: null, w: 512, h: 96, font: 'bold 64px Arial, sans-serif' });
    const nm = new THREE.MeshStandardMaterial({ map: texture, roughness: 0.6 });
    for (const side of [-1, 1]) {
      const h = 3.2, w = h * aspect;
      const p = P(Lship / 2 - 58, 0, side * (half(Lship / 2 - 58) + 0.05));
      const m = new THREE.Mesh(new THREE.PlaneGeometry(w, h), nm);
      m.position.set(p[0], 9.5, p[2]);
      m.rotation.y = Math.atan2(nx * side, nz * side);
      if (side < 0) m.scale.x = -1;
      m.material.side = THREE.DoubleSide;
      this.group.add(m);
    }
    this.ship = { cx, cz, dx, dz, nx, nz, Lship, Bm };
    // Name and port of registry across the stern.
    {
      const { texture, aspect } = signTexture(['MERIDIAN STAR', 'PORT MERIDIAN'], { bg: '#1f3550', fg: '#ffffff', border: null, w: 512, h: 160, font: 'bold 56px Arial, sans-serif' });
      const m = new THREE.MeshStandardMaterial({ map: texture, roughness: 0.6 });
      const h = 4.2, w = h * aspect;
      const p = P(-Lship / 2 - 0.08, 0, 0);
      const sm = new THREE.Mesh(new THREE.PlaneGeometry(w, h), m);
      sm.position.set(p[0], 8.5, p[2]);
      sm.rotation.y = Math.atan2(-dx, -dz);
      this.group.add(sm);
    }
    // Mooring lines: head, breast and stern lines down to the quay bollards.
    if (this.bollards?.length) {
      const lines = new GeoBuilder({ color: true });
      const rope = col(0xd8cfb8);
      for (const xs0 of [Lship / 2 - 8, Lship / 2 - 30, -Lship / 2 + 6, -Lship / 2 + 26]) {
        const A = P(xs0, 14, -half(xs0) + 0.5);
        let best = null, bd = Infinity;
        for (const bp of this.bollards) {
          const d = Math.hypot(bp.x - A[0], bp.z - A[2]);
          const want = Math.abs(xs0) > Lship / 2 - 12 ? 40 : 18; // head and stern lines run out further
          const sc = Math.abs(d - want);
          if (sc < bd) { bd = sc; best = bp; }
        }
        if (!best) continue;
        // Sagging rope in a few straight pieces.
        const Bp = [best.x, best.y + 0.6, best.z];
        let prev = A;
        for (let k = 1; k <= 4; k++) {
          const t2 = k / 4;
          const q = [lerp(A[0], Bp[0], t2), lerp(A[1], Bp[1], t2) - Math.sin(t2 * Math.PI) * 1.2, lerp(A[2], Bp[2], t2)];
          beam(lines, prev, q, 0.1, 0.1, rope, false);
          prev = q;
        }
      }
      this.B.add(lines.build(), this.paint, { cast: true, chunk: 3000 });
    }
  }

  // ── Rail spur on the right: two tracks on ballast, with a container
  // train standing on one of them ───────────────────────────────────
  buildRail() {
    const path = this.path, g0 = this.groundY + 0.04, rng = mulberry32(2323);
    const a = this.portStart - 10, b = path.u1 - 40;
    const gravel = gravelTexture().clone();
    gravel.wrapS = gravel.wrapT = THREE.RepeatWrapping;
    gravel.needsUpdate = true;
    const ballast = this.mat('ballast', () => new THREE.MeshStandardMaterial({ map: gravel, color: 0x8f877e, roughness: 1 }));
    const ties = new GeoBuilder({ color: true }), tieC = col(0x6a655e);
    const f = {};
    for (const c of RAIL_LATS) {
      for (const r of chunked([[a, b]], 300)) {
        this.B.add(sweep(path, [r], [
          { lat: c - 2.0, y: () => g0 }, { lat: c - 1.5, y: () => g0 + 0.28 },
          { lat: c + 1.5, y: () => g0 + 0.28 }, { lat: c + 2.0, y: () => g0 },
        ], { step: 6, uS: 3, vS: 3 }), ballast, { receive: true, chunk: 1500 });
        for (const rl of [c - 0.75, c + 0.75]) {
          this.B.add(sweep(path, [r], [
            { lat: rl - 0.04, y: () => g0 + 0.36 }, { lat: rl - 0.04, y: () => g0 + 0.5 },
            { lat: rl + 0.04, y: () => g0 + 0.5 }, { lat: rl + 0.04, y: () => g0 + 0.36 },
          ], { step: 6, color: col(0x6d6a66) }), this.paint, { cast: true });
        }
      }
      for (let u = a + 0.4; u < b; u += 0.72) {
        path.frame(u, f);
        ties.box(f.x + f.rx * c, g0 + 0.28, f.z + f.rz * c, 2.6, 0.16, 0.26, Math.atan2(f.rz, f.rx), { color: tieC });
      }
    }
    this.B.add(ties.build(), this.matte, { receive: true });
    // A string of well wagons carrying boxes, and a locomotive at its head.
    const geo = new GeoBuilder({ color: true });
    const wag = col(0x5a3a2c), dk = col(0x26282b), loco = col(0x1d4f7a), yel = col(0xe0b52b);
    const c = RAIL_LATS[1], deckY = g0 + 0.5 + 1.1;
    const u0 = a + 120;
    let u = u0;
    for (let k = 0; k < 14; k++) {
      path.frame(u + 7.6, f);
      const yaw = yawOf(f.fx, f.fz), ya = Math.atan2(f.fz, f.fx);
      const x = f.x + f.rx * c, z = f.z + f.rz * c;
      geo.box(x, deckY - 0.5, z, 15, 0.5, 2.7, ya, { color: wag });
      for (const d of [-5.8, 5.8]) {
        const bx = x + f.fx * d, bz = z + f.fz * d;
        geo.box(bx, g0 + 0.5, bz, 2.6, 0.7, 2.3, ya, { color: dk });
      }
      if (rng() < 0.85) {
        this.addContainer(x, deckY, z, yaw, rng);
        if (rng() < 0.5) this.addContainer(x, deckY + CONT[1], z, yaw, rng);
      }
      u += 15.6;
    }
    path.frame(u + 10, f);
    const ya = Math.atan2(f.fz, f.fx);
    const x = f.x + f.rx * c, z = f.z + f.rz * c;
    geo.box(x, g0 + 1.2, z, 20, 3.4, 3.0, ya, { color: loco, roofColor: col(0x8a9096) });
    const cab = [x + f.fx * 8.2, z + f.fz * 8.2];
    geo.box(cab[0], g0 + 4.6, cab[1], 3.6, 0.9, 3.02, ya, { color: col(0x2a3440), roof: false });
    geo.box(x, g0 + 1.2, z, 20.1, 0.3, 3.02, ya, { color: yel, roof: false });
    for (const d of [-6.5, 6.5]) geo.box(x + f.fx * d, g0 + 0.5, z + f.fz * d, 3.4, 0.7, 2.4, ya, { color: dk });
    this.B.add(geo.build(), this.paint, { cast: true });
  }

  // Container trucks: under the quay cranes waiting to be loaded, in the
  // yard lanes, and backed up to the shed doors.
  truck(geo, x, z, fx, fz, rng, loaded = true) {
    const g0 = this.groundY + 0.04;
    const ya = Math.atan2(fz, fx), yaw = yawOf(fx, fz);
    const cabs = [0xb3342b, 0xe9e9e4, 0x1d4f8a, 0x2f7d3a, 0xe0b52b, 0x2b2d31];
    const cc = col(cabs[Math.floor(rng() * cabs.length)]), dk = col(0x1c1d20), steel = col(0x55585c), glass = col(0x3a4a58);
    const at = (d) => [x + fx * d, z + fz * d];
    // Chassis (trailer) and wheels.
    let p = at(-1.2);
    geo.box(p[0], g0 + 1.05, p[1], 12.4, 0.3, 2.4, ya, { color: steel });
    for (const d of [-6, -4.8, 3.4]) { p = at(d); geo.box(p[0], g0, p[1], 1.0, 1.0, 2.5, ya, { color: dk }); }
    // Tractor: cab over the front axle, sleeper, fuel tank.
    p = at(6.9);
    geo.box(p[0], g0 + 1.0, p[1], 2.3, 2.6, 2.5, ya, { color: cc });
    p = at(7.95);
    geo.box(p[0], g0 + 2.2, p[1], 0.1, 1.1, 2.2, ya, { color: glass, roof: false });
    p = at(5.5);
    geo.box(p[0], g0 + 0.5, p[1], 3.0, 0.6, 2.2, ya, { color: dk });
    for (const d of [5.2, 7.2]) { p = at(d); geo.box(p[0], g0, p[1], 1.0, 1.0, 2.5, ya, { color: dk }); }
    if (loaded) {
      p = at(-1.2);
      this.addContainer(p[0], g0 + 1.35, p[1], yaw, rng);
    }
  }

  buildTrucks() {
    const path = this.path, rng = mulberry32(777), f = {};
    const geo = new GeoBuilder({ color: true });
    // Under the cranes, facing along the quay.
    this.craneSites().forEach((u, i) => {
      if (i % 2) return;
      path.frame(u + 2, f);
      const lat = QUAY + 18 + (i % 4 ? 3.5 : -3.5);
      this.truck(geo, f.x + f.rx * lat, f.z + f.rz * lat, f.fx, f.fz, rng, rng() < 0.5);
    });
    // In the yard truck lanes.
    for (const blk of this.yardBlocks || []) {
      for (const g of [-48, -98]) {
        if (rng() < 0.4) continue;
        const u = lerp(blk.u0 + 15, blk.u1 - 15, rng());
        path.frame(u, f);
        const lat = g - 17.2;
        this.truck(geo, f.x + f.rx * lat, f.z + f.rz * lat, f.fx, f.fz, rng, rng() < 0.7);
      }
    }
    // Backed up to the shed doors.
    for (const d of this.dockDoors || []) {
      if (rng() < 0.55) continue;
      path.frame(d.u, f);
      const lat = d.lat - 9.4;
      this.truck(geo, f.x + f.rx * lat, f.z + f.rz * lat, -f.rx, -f.rz, rng, rng() < 0.6);
    }
    if (!geo.empty) this.B.add(geo.build(), this.paint, { cast: true });
  }

  // ── Sheds and offices on the right ──────────────────────────────
  buildSheds() {
    const path = this.path, rng = this.rng, g0 = this.groundY;
    const walls = new GeoBuilder({ color: true }), roofs = new GeoBuilder({ color: true });
    const f = {};
    const tints = [0xb9c3cc, 0xd6cfbd, 0x9fb3a0, 0xb07d62, 0xc9ccd1, 0x8fa6b8];
    const doors = col(0x2f3236), stripe = col(0x1d4f7a);
    const rollerC = [col(0x9aa3ab), col(0x7d8f9e), col(0xb9b3a4)];
    this.dockDoors = [];
    this.shedCount = 0;
    this.shedSigns = [];
    this.shedAtlas = new SignAtlas();
    let u = this.portStart - 30;
    const end = path.u1 - 40;
    let first = true;
    while (u < end) {
      const L = 90 + rng() * 60, D = 34 + rng() * 22, H = 10 + rng() * 5;
      const uc = u + L / 2;
      path.frame(Math.min(uc, path.u1 - 1), f);
      const front = 46 + rng() * 10;
      const latC = front + D / 2;
      const cx = f.x + f.rx * latC, cz = f.z + f.rz * latC;
      const yaw = Math.atan2(f.fz, f.fx);
      const c = col(tints[Math.floor(rng() * tints.length)]);
      walls.box(cx, g0, cz, L, H, D, yaw, { tileW: 12, tileH: 12, color: c, roof: false });
      // Low gable roof.
      const ax = Math.cos(yaw), az = Math.sin(yaw), rx = -az, rz = ax;
      const pt = (a, o, y) => [cx + ax * a + rx * o, y, cz + az * a + rz * o];
      const rc = col(0x8d9296);
      for (const side of [-1, 1]) {
        quadOut(roofs, pt(-L / 2 - 0.5, side * (D / 2 + 0.5), H + g0), pt(L / 2 + 0.5, side * (D / 2 + 0.5), H + g0),
          pt(L / 2 + 0.5, 0, H + g0 + 2.4), pt(-L / 2 - 0.5, 0, H + g0 + 2.4), [rx * side, 1, rz * side], rc);
      }
      for (const e of [-1, 1]) {
        roofs.tri(pt(e * L / 2, -D / 2, H + g0), pt(e * L / 2, D / 2, H + g0), pt(e * L / 2, 0, H + g0 + 2.4), c);
        roofs.tri(pt(e * L / 2, D / 2, H + g0), pt(e * L / 2, -D / 2, H + g0), pt(e * L / 2, 0, H + g0 + 2.4), c);
      }
      // Roller doors (ribs run across: the wall texture turned 90°) in dark
      // frames, rubber dock bumpers, and a canopy over the dock.
      const rot = [[0, 0], [0, 1.2], [3, 1.2], [3, 0]];
      for (let a = -L / 2 + 8; a < L / 2 - 6; a += 9) {
        quadOut(walls, pt(a - 0.3, -D / 2 - 0.04, g0), pt(a + 5.3, -D / 2 - 0.04, g0), pt(a + 5.3, -D / 2 - 0.04, g0 + 5.8), pt(a - 0.3, -D / 2 - 0.04, g0 + 5.8), [-rx, 0, -rz], doors);
        quadOut(walls, pt(a, -D / 2 - 0.08, g0 + 1.2), pt(a + 5, -D / 2 - 0.08, g0 + 1.2), pt(a + 5, -D / 2 - 0.08, g0 + 5.5), pt(a, -D / 2 - 0.08, g0 + 5.5), [-rx, 0, -rz], rollerC[Math.abs(Math.round(a / 9 + u)) % 3], rot);
        for (const e of [0.3, 4.7]) { const q = pt(a + e, -D / 2 - 0.3, 0); walls.box(q[0], g0 + 0.5, q[2], 0.4, 0.7, 0.5, yaw, { color: doors }); }
        this.dockDoors.push({ u: uc + a + 2.5, lat: front });
      }
      const dockF = pt(0, -D / 2 - 0.8, 0);
      walls.box(dockF[0], g0, dockF[2], L - 8, 1.2, 1.6, yaw, { color: col(0x9a978f) });
      const can = pt(0, -D / 2 - 1.5, 0);
      roofs.box(can[0], g0 + 6.3, can[2], L - 6, 0.3, 3.0, yaw, { color: col(0x6d7378) });
      // Skylight strips and vents along the roof.
      for (const side of [-1, 1]) {
        for (let a = -L / 2 + 6; a < L / 2 - 6; a += 12) {
          const o0 = side * D * 0.14, o1 = side * D * 0.3;
          const y0 = H + g0 + 2.4 * (1 - Math.abs(o0) / (D / 2)) + 0.05, y1 = H + g0 + 2.4 * (1 - Math.abs(o1) / (D / 2)) + 0.05;
          quadOut(roofs, pt(a, o0, y0), pt(a + 6, o0, y0), pt(a + 6, o1, y1), pt(a, o1, y1), [0, 1, 0], col(0xc9dde6));
        }
      }
      for (let a = -L / 2 + 12; a < L / 2 - 8; a += 24) { const q = pt(a, 0, 0); roofs.box(q[0], g0 + H + 2.3, q[2], 1.4, 1.2, 1.4, yaw, { color: col(0x9a9ea3) }); }
      // Company name on the wall facing the road.
      const nm = SHED_NAMES[this.shedCount++ % SHED_NAMES.length];
      this.shedSigns.push({ rect: this.shedAtlas.add(640, 96, (g, w, h) => panel(g, w, h, nm[1], '#ffffff', nm[1], [nm[0]], [50])), at: pt(0, -D / 2 - 0.12, g0 + H - 2.2), yaw: Math.atan2(-rx, -rz), w: Math.min(L * 0.4, 20) });
      quadOut(walls, pt(-L / 2, -D / 2 - 0.06, g0 + H - 2.2), pt(L / 2, -D / 2 - 0.06, g0 + H - 2.2), pt(L / 2, -D / 2 - 0.06, g0 + H - 1.2), pt(-L / 2, -D / 2 - 0.06, g0 + H - 1.2), [-rx, 0, -rz], stripe);
      if (first) {
        // Rooftop sign facing arriving traffic.
        const { texture, aspect } = signTexture(['PORT MERIDIAN'], { bg: '#123a52', fg: '#ffffff', border: '#ffffff', w: 1024, h: 180, font: 'bold 110px "Arial Narrow", Arial, sans-serif' });
        const m = new THREE.MeshStandardMaterial({ map: texture, emissive: 0xffffff, emissiveMap: texture, emissiveIntensity: 0.1, roughness: 0.5, side: THREE.DoubleSide });
        this.world.addNight(m, 'emissiveIntensity', 0.1, 0.8);
        const h = 6, w = h * aspect;
        const sign = new THREE.Mesh(new THREE.PlaneGeometry(w, h), m);
        const p = pt(-L / 2 + w / 2 + 4, -D / 4, 0);
        sign.position.set(p[0], g0 + H + 2.4 + h / 2 + 1, p[2]);
        sign.rotation.y = Math.atan2(-ax * 0.6 - rx * 0.8, -az * 0.6 - rz * 0.8);
        this.group.add(sign);
        for (const k of [-0.35, 0.35]) {
          const q = pt(-L / 2 + w / 2 + 4 + k * w, -D / 4 + 0.6, 0);
          walls.box(q[0], g0 + H, q[2], 0.4, 3.8, 0.4, 0, { color: col(0x55585c) });
        }
        first = false;
      }
      u += L + 20 + rng() * 25;
    }
    // Wall names: one atlas, one mesh.
    const sg = [];
    for (const n of this.shedSigns) {
      const h = n.w * 96 / 640;
      const g = new THREE.PlaneGeometry(n.w, h);
      const uv = g.getAttribute('uv');
      for (let k = 0; k < uv.count; k++) uv.setXY(k, lerp(n.rect.u0, n.rect.u1, uv.getX(k)), lerp(n.rect.v0, n.rect.v1, uv.getY(k)));
      g.rotateY(n.yaw);
      g.translate(n.at[0], n.at[1], n.at[2]);
      sg.push(g);
    }
    if (sg.length) {
      const m = new THREE.MeshStandardMaterial({ map: this.shedAtlas.texture(), roughness: 0.6, polygonOffset: true, polygonOffsetFactor: -2, polygonOffsetUnits: -2 });
      this.B.add(mergeGeometries(sg, false), m);
    }
    const wallMat = this.mat('shedWall', () => new THREE.MeshStandardMaterial({ map: corrugatedTexture(), vertexColors: true, roughness: 0.75, metalness: 0.2 }));
    this.B.add(walls.build(), wallMat, { cast: true });
    this.B.add(roofs.build(), this.paint, { cast: true });
    // Terminal office: a glassy block by the gate end of the port.
    const og = new GeoBuilder({ color: true });
    path.frame(this.portStart + 20, f);
    const ox = f.x + f.rx * 40, oz = f.z + f.rz * 40;
    og.box(ox, g0, oz, 26, 22, 18, Math.atan2(f.fz, f.fx), { color: col(0x6f8fa6), roofColor: col(0x9aa0a6) });
    for (let k = 0; k < 6; k++) og.box(ox, g0 + 2.5 + k * 3.5, oz, 26.2, 0.5, 18.2, Math.atan2(f.fz, f.fx), { color: col(0xdedede), roof: false });
    this.B.add(og.build(), this.paint, { cast: true });
  }

  // ── On the water ─────────────────────────────────────────────────
  buildBoats() {
    const t = this.t, world = this.world;
    const mk = (build) => {
      const geo = new GeoBuilder({ color: true });
      build(geo);
      const m = new THREE.Mesh(geo.build(), this.paint);
      m.castShadow = true;
      m.userData.animated = true;
      this.group.add(m);
      return m;
    };
    const hull = (geo, L, B, H, c, deckC) => {
      // Boat hull along +x with a pointed bow, bottom at y=-1.
      const pts = [[-L / 2, -B / 2 * 0.85], [L * 0.2, -B / 2], [L / 2, 0], [L * 0.2, B / 2], [-L / 2, B / 2 * 0.85]];
      for (let i = 0; i < pts.length; i++) {
        const a = pts[i], b = pts[(i + 1) % pts.length];
        const mx = (a[0] + b[0]) / 2, mz = (a[1] + b[1]) / 2;
        quadOut(geo, [a[0], -1, a[1]], [b[0], -1, b[1]], [b[0], H, b[1]], [a[0], H, a[1]], [mx, 0, mz], c);
      }
      // Deck (wound to face up).
      geo.tri([pts[0][0], H, pts[0][1]], [pts[2][0], H, pts[2][1]], [pts[1][0], H, pts[1][1]], deckC);
      geo.tri([pts[0][0], H, pts[0][1]], [pts[3][0], H, pts[3][1]], [pts[2][0], H, pts[2][1]], deckC);
      geo.tri([pts[0][0], H, pts[0][1]], [pts[4][0], H, pts[4][1]], [pts[3][0], H, pts[3][1]], deckC);
    };
    const boats = [];
    // Tugs by the ship's bow.
    if (this.ship) {
      const S = this.ship;
      for (const [k, off] of [[0, 1], [1, 0.75]]) {
        const tug = mk((g) => {
          hull(g, 28, 10, 3, col(0x1d1f24), col(0x8a3b2a));
          g.box(-2, 3, 0, 8, 3.2, 6.5, 0, { color: col(0xe9e9e4), roofColor: col(0xb3342b) });
          g.box(-1, 6.2, 0, 5, 2.4, 5.5, 0, { color: col(0xe9e9e4), roofColor: col(0x2b2d31) });
          g.box(-7, 3, 0, 1.4, 7, 1.4, 0, { color: col(0xb3342b) });
        });
        const x = S.cx + S.dx * (S.Lship / 2 + 30 + k * 40) + S.nx * (off * 30);
        const z = S.cz + S.dz * (S.Lship / 2 + 30 + k * 40) + S.nz * (off * 30);
        boats.push({ m: tug, x, z, yaw: -Math.atan2(S.dz, S.dx) + k * 0.6, ph: k * 1.7, drift: 0 });
      }
    }
    // A sailboat and a fishing boat in the shipping channel.
    if (this.span) {
      const sm = (this.span.s0 + this.span.s1) / 2;
      const f = t.frame(sm, {});
      const sail = mk((g) => {
        hull(g, 11, 3.6, 1.2, col(0xf2f2ee), col(0xb89a72));
        g.box(0, 1.2, 0, 0.25, 14, 0.25, 0, { color: col(0xd0d0d0) });
        g.tri([0.3, 2.5, 0], [0.3, 15, 0], [5, 2.5, 0], col(0xfafaf6));
        g.tri([0.3, 2.5, 0], [5, 2.5, 0], [0.3, 15, 0], col(0xfafaf6));
        g.tri([-0.3, 3, 0], [-4.5, 3, 0], [-0.3, 12, 0], col(0xe8e2d0));
        g.tri([-0.3, 3, 0], [-0.3, 12, 0], [-4.5, 3, 0], col(0xe8e2d0));
      });
      boats.push({ m: sail, x: f.x - f.rx * 180, z: f.z - f.rz * 180, yaw: 0, ph: 0.4, drift: 1, ax: -f.rx, az: -f.rz, range: 520, speed: 3.2, base: [f.x - f.rx * 60, f.z - f.rz * 60] });
      const fish = mk((g) => {
        hull(g, 16, 5, 2, col(0x2e6d8e), col(0x9a8f7c));
        g.box(-3, 2, 0, 5, 3, 4, 0, { color: col(0xe9e9e4), roofColor: col(0x2e6d8e) });
        g.box(3, 2, 0, 0.3, 8, 0.3, 0, { color: col(0x55585c) });
      });
      boats.push({ m: fish, x: f.x + f.rx * 260 + f.fx * 120, z: f.z + f.rz * 260 + f.fz * 120, yaw: 1.2, ph: 2.1, drift: 0 });
    }
    let time = 0;
    world.updaters.push((dt) => {
      time += dt;
      for (const b of boats) {
        let x = b.x, z = b.z, yaw = b.yaw;
        if (b.drift) {
          const d = ((time * b.speed) % (b.range * 2));
          const k = d < b.range ? d : b.range * 2 - d;
          x = b.base[0] + b.ax * (k - b.range * 0.5);
          z = b.base[1] + b.az * (k - b.range * 0.5);
          yaw = -Math.atan2(b.az * (d < b.range ? 1 : -1), b.ax * (d < b.range ? 1 : -1));
        }
        b.m.position.set(x, 0.15 + Math.sin(time * 1.3 + b.ph) * 0.25, z);
        b.m.rotation.set(Math.sin(time * 0.9 + b.ph) * 0.03, yaw, Math.sin(time * 1.1 + b.ph * 2) * 0.05, 'YXZ');
      }
    });
    // Channel buoys.
    if (this.span) {
      const mats = [], colors = [];
      const f = {};
      for (const [s, c] of [[this.span.s0 + 160, 0x2f9a45], [this.span.s1 - 160, 0xc0302a]]) {
        t.frame(s, f);
        for (const lat of [-260, -400, -540, 150, 300]) {
          mats.push(trs(f.x + f.rx * lat, -0.5, f.z + f.rz * lat, 0, 1.2, 3.2, 1.2));
          colors.push(c);
        }
      }
      const g = new THREE.CylinderGeometry(0.6, 1, 1, 10); g.translate(0, 0.5, 0);
      const im = instanced(g, new THREE.MeshStandardMaterial({ roughness: 0.6 }), mats);
      const cc = new THREE.Color();
      colors.forEach((c, i) => im.setColorAt(i, cc.setHex(c)));
      im.instanceColor.needsUpdate = true;
      this.group.add(im);
    }
  }

  // Rubble breakwaters either side of the harbour mouth, each with a small
  // lighthouse at its tip.
  buildBreakwaters() {
    const t = this.t;
    if (!this.span) return;
    const rng = mulberry32(31);
    const rocks = [];
    const arms = [
      { s0: this.span.s0 + 40, s1: this.span.s0 + 170, lat0: -196, lat1: -470, light: 0x2fd05a },
      { s0: this.span.s1 - 40, s1: this.span.s1 - 170, lat0: -196, lat1: -470, light: 0xff3a2a },
    ];
    const geo = new GeoBuilder({ color: true });
    const lamps = [];
    const f0 = {}, f1 = {};
    for (const a of arms) {
      t.frame(a.s0, f0); t.frame(a.s1, f1);
      const A = [f0.x + f0.rx * a.lat0, f0.z + f0.rz * a.lat0], Bp = [f1.x + f1.rx * a.lat1, f1.z + f1.rz * a.lat1];
      const L = Math.hypot(Bp[0] - A[0], Bp[1] - A[1]);
      const dx = (Bp[0] - A[0]) / L, dz = (Bp[1] - A[1]) / L, nx = -dz, nz = dx;
      for (let d = 0; d < L; d += 3.2) {
        for (const o of [-7, -3.5, 0, 3.5, 7]) {
          const x = A[0] + dx * d + nx * (o + (rng() - 0.5) * 2), z = A[1] + dz * d + nz * (o + (rng() - 0.5) * 2);
          const top = Math.abs(o) < 4 ? 3.2 : 0.8;
          const s = 2.2 + rng() * 2.4;
          rocks.push(trs(x, top - s * 0.5, z, rng() * 6, s * (0.9 + rng() * 0.4), s * (0.7 + rng() * 0.4), s * (0.9 + rng() * 0.4), rng() * 0.6, rng() * 0.6));
        }
      }
      // Lighthouse: tapered white tower with a coloured band and lantern.
      const lx = Bp[0], lz = Bp[1];
      frustum(geo, lx, lz, 3, 16, dx, dz, 3.4, 3.4, 2.4, 2.4, col(0xf0efe8), 4);
      frustum(geo, lx, lz, 9, 11.5, dx, dz, 2.95, 2.95, 2.8, 2.8, col(a.light === 0xff3a2a ? 0xc0302a : 0x2f9a45), 4);
      frustum(geo, lx, lz, 16, 16.4, dx, dz, 3.4, 3.4, 3.4, 3.4, col(0x2b2d31), 4);
      frustum(geo, lx, lz, 18.6, 19.4, dx, dz, 2.2, 2.2, 0.6, 0.6, col(0x2b2d31), 4);
      lamps.push({ p: [lx, 17.5, lz], c: a.light });
    }
    const rockGeo = new THREE.DodecahedronGeometry(0.62, 0);
    const rockMat = new THREE.MeshStandardMaterial({ color: 0x8a8580, roughness: 0.95, flatShading: true });
    this.group.add(instanced(rockGeo, rockMat, rocks, { cast: false, receive: true }));
    this.B.add(geo.build(), this.paint, { cast: true, chunk: 3000 });
    const lampMat = new THREE.MeshBasicMaterial({ color: 0xffffff });
    const im = instanced(new THREE.SphereGeometry(1, 10, 8), lampMat, lamps.map((l) => trs(l.p[0], l.p[1], l.p[2])));
    const c = new THREE.Color();
    lamps.forEach((l, i) => im.setColorAt(i, c.setHex(l.c).multiplyScalar(3)));
    im.instanceColor.needsUpdate = true;
    this.group.add(im);
    let time = 0;
    this.world.updaters.push((dt) => {
      time += dt;
      lamps.forEach((l, i) => im.setColorAt(i, c.setHex(l.c).multiplyScalar(((time + i * 1.3) % 4) < 1 ? 4 : 0.35)));
      im.instanceColor.needsUpdate = true;
    });
  }
}
