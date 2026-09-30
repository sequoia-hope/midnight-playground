import * as THREE from 'three';
import { clamp, lerp, smoothstep, mulberry32 } from '../util/math.js';
import { extrude, runs } from './Road.js';
import { concreteTexture } from './textures.js';
import { GeoBuilder, staticMesh } from './city/geom.js';
import { canopyGeometry } from './valley/flora.js';
import { SurfaceSampler } from './coast/kit.js';
import { kerbTexture, tyreTexture, fenceTexture, crowdTexture, bannerAtlas } from './raceway/textures.js';

// Seaside Raceway — Level 5: Laguna Seca, from survey data.
//
// The land, the racing line and the barrier positions are real (see
// tools/seaside/build.py and src/levels/seaside/). This dresses it: red and
// white kerbs through the corners, concrete walls with catch fencing where
// OpenStreetMap has walls and tyre walls on the outside of the fast
// corners, the start gantry and its lights, the grid, the pit lane, every
// building and grandstand from OpenStreetMap, the bridges over the track,
// the infield lake, and coast live oaks wherever the aerial photo shows a
// crown.

const tick = () => new Promise((r) => setTimeout(r, 0));
const WALL_H = 1.05;      // concrete wall height
const FENCE_H = 2.7;      // catch fence above the wall
const TYRE_W = 1.1;       // tyre wall depth
const CLEAR = 5.6;        // bridge deck underside above the road

export default class Raceway {
  label = 'Building the raceway';

  constructor({ zone = 0 } = {}) {
    this.zone = zone;
  }

  plan(world) {
    this.t = world.track;
    this.T = world.terrain;
    this.D = world.level.data;
    this.corners = this.findCorners();
  }

  // Corners: runs of road tighter than a 260 m radius, with the apex (the
  // tightest point) and which way they turn (+1 right, -1 left).
  findCorners() {
    const t = this.t, n = t.n, k = t.kSmooth;
    const out = [];
    let i = 0;
    while (i < n) {
      if (Math.abs(k[i]) < 1 / 260) { i++; continue; }
      const sign = Math.sign(k[i]);
      let j = i, apex = i;
      while (j < n && Math.sign(k[j]) === sign && Math.abs(k[j]) >= 1 / 260) {
        if (Math.abs(k[j]) > Math.abs(k[apex])) apex = j;
        j++;
      }
      const prev = out.at(-1);
      if (prev && prev.sign === sign && i - prev.s1 < 15) {
        prev.s1 = j;
        if (Math.abs(k[apex]) > prev.peak) { prev.apex = apex; prev.peak = Math.abs(k[apex]); }
      } else if (j - i > 8) out.push({ s0: i, s1: j, apex, sign, peak: Math.abs(k[apex]) });
      i = j;
    }
    return out;
  }

  async build(world) {
    const root = (this.root = new THREE.Group());
    root.name = 'raceway';
    world.scene.add(root);
    this.world = world;
    this.S = new SurfaceSampler(this.T);
    this.mats = {
      concrete: new THREE.MeshStandardMaterial({ map: concreteTexture(), color: 0xe4e0d8, roughness: 0.9, side: THREE.DoubleSide }),
      verge: new THREE.MeshStandardMaterial({ map: concreteTexture(), color: 0x5a5854, roughness: 0.95 }),
      kerb: new THREE.MeshStandardMaterial({ map: kerbTexture(), roughness: 0.7, polygonOffset: true, polygonOffsetFactor: -2, polygonOffsetUnits: -2 }),
      tyre: new THREE.MeshStandardMaterial({ map: tyreTexture(), roughness: 0.85, side: THREE.DoubleSide }),
      fence: new THREE.MeshStandardMaterial({ map: fenceTexture(), alphaTest: 0.35, transparent: false, roughness: 0.6, metalness: 0.4, side: THREE.DoubleSide }),
      steel: new THREE.MeshStandardMaterial({ color: 0x8c9196, roughness: 0.5, metalness: 0.5 }),
      solid: new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.85 }),
      paint: new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.6, polygonOffset: true, polygonOffsetFactor: -3, polygonOffsetUnits: -3 }),
      banner: new THREE.MeshStandardMaterial({ map: bannerAtlas().texture, roughness: 0.7 }),
      crowd: new THREE.MeshStandardMaterial({ map: crowdTexture(), roughness: 0.95 }),
    };
    this.buildVerges();
    this.buildKerbs();
    await tick();
    this.buildBarriers();
    await tick();
    this.buildStart();
    this.buildPitLane();
    await tick();
    this.buildBuildings();
    this.buildGrandstands();
    await tick();
    this.buildBridges();
    this.buildLake();
    this.buildBoards();
    await tick();
    this.buildTrees();
  }

  add(mesh) { if (mesh) this.root.add(mesh); return mesh; }

  // Wall distance on a side at s.
  wall(f, side) { return side < 0 ? f.wallL : f.wallR; }

  // ── Verges: a strip of old tarmac past each white line, stepping down
  // to the run-off, so the road's edge never floats over the ground. ──
  buildVerges() {
    const t = this.t;
    for (const side of [-1, 1]) {
      // Heights from the run-off surface (it leaves the road's plane at the
      // edge), and the outer lip tucked under the ground.
      const at = (lat, dy) => ({ lat: (f) => side * (f.hw + lat), abs: true, dy: (f, s) => t.surfaceY(s, side * (f.hw + lat)) + dy });
      const prof = [{ ...at(-0.05, -0.004), u: 0 }, { ...at(1.2, -0.03), u: 0.5 }, { ...at(1.6, -0.45), u: 1 }];
      if (side < 0) prof.reverse();
      for (let s0 = 0; s0 < t.length; s0 += 600) {
        const g = extrude(t, [[s0, Math.min(t.length, s0 + 600) + (s0 + 600 >= t.length ? 1 : 0)]], prof, { vScale: 5 });
        const m = this.add(new THREE.Mesh(g, this.mats.verge));
        m.receiveShadow = true;
      }
    }
  }

  // ── Kerbs: on the inside through each apex, on the outside out of the
  // exit, and on the outside of the entry of the slow corners. ──
  buildKerbs() {
    const t = this.t;
    const spans = { '-1': [], '1': [] };
    for (const c of this.corners) {
      const inside = c.sign;               // a right-hander's inside is the right
      const len = c.s1 - c.s0, r = 1 / c.peak;
      const half = clamp(len * 0.3, 10, 40);
      spans[inside].push([c.apex - half, c.apex + half]);
      spans[-inside].push([c.apex, Math.min(c.s1 + 18, c.apex + len)]);
      if (r < 60) spans[-inside].push([c.s0 - 12, c.s0 + 10]);
    }
    this.kerbs = spans;
    for (const side of [-1, 1]) {
      // Merge overlaps.
      const list = spans[side].sort((a, b) => a[0] - b[0]);
      const merged = [];
      for (const r of list) {
        const last = merged.at(-1);
        if (last && r[0] <= last[1] + 6) last[1] = Math.max(last[1], r[1]);
        else merged.push([...r]);
      }
      // Whole blocks so the paint ends on a join.
      const ranges = merged.map(([a, b]) => [Math.round(a / 2) * 2, Math.round(b / 2) * 2]);
      spans[side] = ranges;
      const prof = [
        { lat: (f) => side * (f.hw - 0.25), dy: 0.004, u: 0 },
        { lat: (f) => side * (f.hw + 0.05), dy: 0.055, u: 0.25 },
        { lat: (f) => side * (f.hw + 0.95), dy: 0.035, u: 0.95 },
        { lat: (f) => side * (f.hw + 1.1), dy: -0.06, u: 1 },
      ];
      if (side < 0) prof.reverse();
      const g = extrude(t, ranges, prof, { step: 1, vScale: 2.4 });
      const m = this.add(new THREE.Mesh(g, this.mats.kerb));
      m.receiveShadow = true;
    }
  }

  // ── Barriers: concrete wall and catch fence along every surveyed wall;
  // tyre walls in front of it on the outside of corners with room to run
  // wide, and wherever the run-off is open (no wall in OpenStreetMap). ──
  buildBarriers() {
    const t = this.t;
    const outsideAt = (s, side) => this.corners.some((c) => c.sign === -side && s > c.s0 - 25 && s < c.s1 + 40);
    this.tyreRuns = {};
    for (const side of [-1, 1]) {
      const f = {};
      const tyre = (s) => {
        t.frame(s, f);
        const w = this.wall(f, side);
        return w > 33 || (outsideAt(s, side) && w - f.hw > 7);
      };
      // Tyre runs, cleaned of short flickers.
      let tr = runs(t, tyre, 2).filter(([a, b]) => b - a > 14);
      this.tyreRuns[side] = tr;
      const inTyre = (s) => tr.some(([a, b]) => s >= a && s <= b);
      const W = (f, s) => this.wall(f, side) + (inTyre(s) ? TYRE_W : 0);
      const gy = (s, lat) => t.surfaceY(s, lat);
      // Concrete wall (behind the tyres where there are some).
      const wallProf = [
        { lat: (f, s) => side * W(f, s), abs: true, dy: (f, s) => gy(s, side * W(f, s)) - 0.7, u: 0 },
        { lat: (f, s) => side * W(f, s), abs: true, dy: (f, s) => gy(s, side * W(f, s)) + WALL_H, u: 0.5 },
        { lat: (f, s) => side * (W(f, s) + 0.32), abs: true, dy: (f, s) => gy(s, side * W(f, s)) + WALL_H, u: 0.6 },
        { lat: (f, s) => side * (W(f, s) + 0.32), abs: true, dy: (f, s) => gy(s, side * W(f, s)) - 0.7, u: 1 },
      ];
      for (let s0 = 0; s0 < t.length; s0 += 500) {
        const r = [[s0, Math.min(t.length + 1, s0 + 500)]];
        const g = extrude(t, r, wallProf, { step: 2, vScale: 4 });
        const m = this.add(new THREE.Mesh(g, this.mats.concrete));
        m.castShadow = true; m.receiveShadow = true;
        // Catch fence on top.
        const fenceProf = [
          { lat: (f, s) => side * (W(f, s) + 0.12), abs: true, dy: (f, s) => gy(s, side * W(f, s)) + WALL_H - 0.05, u: 0 },
          { lat: (f, s) => side * (W(f, s) + 0.2), abs: true, dy: (f, s) => gy(s, side * W(f, s)) + WALL_H + FENCE_H, u: FENCE_H / 1.1 },
        ];
        this.add(new THREE.Mesh(extrude(t, r, fenceProf, { step: 4, vScale: 1.1 }), this.mats.fence));
      }
      // Tyre walls.
      if (tr.length) {
        const w0 = (f) => this.wall(f, side);
        const tyreProf = [
          { lat: (f, s) => side * w0(f), abs: true, dy: (f, s) => gy(s, side * w0(f)) - 0.3, u: 0 },
          { lat: (f, s) => side * w0(f), abs: true, dy: (f, s) => gy(s, side * w0(f)) + 1.0, u: 0.75 },
          { lat: (f, s) => side * (w0(f) + TYRE_W), abs: true, dy: (f, s) => gy(s, side * w0(f)) + 1.0, u: 1 },
        ];
        const m = this.add(new THREE.Mesh(extrude(t, tr, tyreProf, { step: 2, vScale: 3 }), this.mats.tyre));
        m.castShadow = true; m.receiveShadow = true;
      }
      // Fence posts.
      const posts = [];
      for (let s = 0; s < t.length; s += 4) {
        t.frame(s, f);
        const w = W(f, s) + 0.16;
        const p = t.pointAt(s, side * w);
        posts.push([p.x, gy(s, side * w) + WALL_H - 0.1, p.z]);
      }
      const post = new THREE.CylinderGeometry(0.05, 0.05, FENCE_H + 0.15, 5);
      post.translate(0, (FENCE_H + 0.15) / 2, 0);
      const im = new THREE.InstancedMesh(post, this.mats.steel, posts.length);
      const m4 = new THREE.Matrix4();
      posts.forEach(([x, y, z], k) => im.setMatrixAt(k, m4.makeTranslation(x, y, z)));
      im.computeBoundingSphere();
      this.add(im);
    }
    this.buildBanners();
  }

  // Sponsor banners on the fence along the concrete walls near the
  // grandstands and the famous corners.
  buildBanners() {
    const t = this.t, atlas = bannerAtlas();
    const names = ['seaside', 'midnight', 'vento', 'kestrel', 'ion', 'stiletto', 'brawler', 'tyres', 'oil'];
    const gb = new GeoBuilder();
    // A plain dark patch of the atlas for the backs of the banners.
    const tr = atlas.rect('tyres');
    const back = [tr[0] + 0.004, tr[1] + 0.004];
    const f = {};
    const rng = mulberry32(77);
    const zones = [[-240, 160], [440, 640], [2440, 2720], [3200, 3420]];
    for (const [a, b] of zones) {
      for (const side of [-1, 1]) {
        let s = a + rng() * 10;
        while (s < b) {
          const len = 6;
          const inTyre = (this.tyreRuns[side] || []).some(([p, q]) => s + len > p && s < q);
          t.frame(s, f);
          const w = this.wall(f, side) + (inTyre ? TYRE_W : 0) - 0.03;
          const y0 = t.surfaceY(s, side * w) + WALL_H + 0.15, y1 = y0 + 1.0;
          const [u0, v0, u1, v1] = atlas.rect(names[Math.floor(rng() * names.length)]);
          const P = (ss) => { const p = t.pointAt(ss, side * (this.wall(t.frame(ss), side) + (inTyre ? TYRE_W : 0) - 0.03)); return p; };
          const pa = P(s), pb = P(s + len);
          // Face the track: on the left side the panel runs forward as u grows.
          const A = [pa.x, y0, pa.z], B = [pb.x, y0, pb.z], C = [pb.x, y1, pb.z], Dd = [pa.x, y1, pa.z];
          if (side < 0) gb.quad(A, B, C, Dd, [[u0, v0], [u1, v0], [u1, v1], [u0, v1]]);
          else gb.quad(B, A, Dd, C, [[u0, v0], [u1, v0], [u1, v1], [u0, v1]]);
          // Plain back, seen from behind the fence.
          const bk = [[back[0], back[1]], [back[0], back[1]], [back[0], back[1]], [back[0], back[1]]];
          if (side < 0) gb.quad(B, A, Dd, C, bk); else gb.quad(A, B, C, Dd, bk);
          s += len + 0.4 + (rng() < 0.15 ? 12 : 0);
        }
      }
    }
    if (!gb.empty) this.add(staticMesh(gb.build(), this.mats.banner, { receive: true }));
  }

  // ── Start/finish: the gantry with its lights, the grid boxes. ──
  buildStart() {
    const t = this.t, f = t.frame(t.startS + 3);
    const gb = new GeoBuilder({ color: true });
    const grey = [0.55, 0.57, 0.6], dark = [0.08, 0.08, 0.09];
    const L = f.wallL + 0.9, R = f.wallR + 0.9;
    const top = f.y + 7.2;
    const yaw = Math.atan2(f.fz, f.fx);
    for (const lat of [-L, R]) {
      const x = f.x + f.rx * lat, z = f.z + f.rz * lat;
      const g = t.surfaceY(t.startS + 3, lat) - 0.5;
      gb.box(x, g, z, 0.8, top - g + 1.6, 0.8, yaw, { color: grey });
    }
    // Truss beam across (a box; the light box hangs from its middle).
    const cx = f.x + f.rx * (R - L) / 2, cz = f.z + f.rz * (R - L) / 2;
    gb.box(cx, top, cz, 1.2, 1.4, L + R + 0.8, yaw, { color: grey });
    gb.box(f.x, top - 1.3, f.z, 0.5, 1.3, 3.2, yaw, { color: dark });
    this.add(staticMesh(gb.build(), this.mats.solid, { cast: true }));
    // START · FINISH panels on both faces of the beam.
    const atlas = bannerAtlas();
    const [u0, v0, u1, v1] = atlas.rect('startfinish');
    const pb = new GeoBuilder();
    for (const dir of [-1, 1]) {
      const off = dir * 0.62;
      const px = cx + f.fx * off, pz = cz + f.fz * off;
      const hw = 9.5;
      const a = [px - f.rx * hw, top + 0.05, pz - f.rz * hw], b = [px + f.rx * hw, top + 0.05, pz + f.rz * hw];
      const c = [b[0], top + 1.35, b[2]], d = [a[0], top + 1.35, a[2]];
      if (dir < 0) pb.quad(a, b, c, d, [[u0, v0], [u1, v0], [u1, v1], [u0, v1]]);
      else pb.quad(b, a, d, c, [[u0, v0], [u1, v0], [u1, v1], [u0, v1]]);
    }
    this.add(staticMesh(pb.build(), this.mats.banner));

    // Start lights: five columns of two, facing the grid. They light one
    // column a second through the countdown and go out at GO.
    const lamp = new THREE.CircleGeometry(0.17, 12);
    this.lampMats = [];
    for (let col = 0; col < 5; col++) {
      const mat = new THREE.MeshStandardMaterial({ color: 0x220806, emissive: 0xff2010, emissiveIntensity: 0 });
      this.lampMats.push(mat);
      for (let row = 0; row < 2; row++) {
        const m = new THREE.Mesh(lamp, mat);
        const lat = (col - 2) * 0.55;
        m.position.set(f.x + f.rx * lat - f.fx * 0.27, top - 0.45 - row * 0.45, f.z + f.rz * lat - f.fz * 0.27);
        m.rotation.y = -yaw - Math.PI / 2;
        this.add(m);
      }
    }
    this.world.onCountdown = (cd, racing) => {
      // cd: seconds left (3.999 → 0) or -1 once racing.
      const lit = cd > 0 ? clamp(Math.ceil((4 - cd) * 5 / 4), 0, 5) : 0;
      this.lampMats.forEach((m, i) => { m.emissiveIntensity = i < lit ? 6 : 0; });
    };

    // Grid boxes: a white bracket ahead of each starting slot.
    const paint = new GeoBuilder({ color: true });
    const white = [0.92, 0.92, 0.9];
    const strip = (s0, s1, l0, l1) => {
      const p = (s, l) => { const q = t.pointAt(s, l); return [q.x, t.surfaceY(s, l) + 0.02, q.z]; };
      paint.quad(p(s0, l0), p(s0, l1), p(s1, l1), p(s1, l0), null, white);
    };
    for (let k = 0; k < 6; k++) {
      const row = Math.floor(k / 2), col = k % 2;
      const s = t.startS - 5 - row * 10 - col * 3 + 2.6;
      const lat = col ? 2.4 : -2.4;
      strip(s, s + 0.2, lat - 1.3, lat + 1.3);
      strip(s - 1.2, s, lat - 1.3, lat - 1.1);
      strip(s - 1.2, s, lat + 1.1, lat + 1.3);
    }
    this.add(staticMesh(paint.build(), this.mats.paint));
  }

  // ── Pit lane: a paved strip along OpenStreetMap's pit lane, where it
  // runs apart from the track. ──
  buildPitLane() {
    const t = this.t, T = this.T;
    const pts = resample(this.D.pitLane, 2);
    const gb = new GeoBuilder({ color: true });
    const col = [0.3, 0.3, 0.31], line = [0.85, 0.85, 0.82];
    const HW = 4.2;
    for (let i = 0; i < pts.length - 1; i++) {
      const a = pts[i], b = pts[i + 1];
      // Skip where it merges into the track itself.
      const da = t.distanceToRoad(a.x, a.z), db = t.distanceToRoad(b.x, b.z);
      if (da.d < t.hw[da.i] + HW + 0.5 || db.d < t.hw[db.i] + HW + 0.5) continue;
      const dx = b.x - a.x, dz = b.z - a.z, l = Math.hypot(dx, dz) || 1;
      const rx = -dz / l, rz = dx / l;
      const pa = pts[Math.max(0, i - 1)], pc = pts[Math.min(pts.length - 1, i + 2)];
      const ex = pc.x - pa.x, ez = pc.z - pa.z, el = Math.hypot(ex, ez) || 1;
      const nx = -ez / el, nz = ex / el;
      const P = (p, nx, nz, off, lift = 0.1) => [p.x + nx * off, this.S.sample(p.x + nx * off, p.z + nz * off).h + lift, p.z + nz * off];
      const na = [rx, rz], nb = [nx, nz];
      // (Wound so the face points up.)
      gb.quad(P(a, na[0], na[1], HW), P(b, nb[0], nb[1], HW), P(b, nb[0], nb[1], -HW), P(a, na[0], na[1], -HW), null, col);
      // Painted edge lines.
      for (const e of [-HW + 0.35, HW - 0.35]) {
        gb.quad(P(a, na[0], na[1], e + 0.08, 0.13), P(b, nb[0], nb[1], e + 0.08, 0.13), P(b, nb[0], nb[1], e - 0.08, 0.13), P(a, na[0], na[1], e - 0.08, 0.13), null, line);
      }
    }
    if (!gb.empty) {
      const m = this.add(staticMesh(gb.build(), this.mats.paint));
      m.receiveShadow = true;
    }
  }

  // ── Buildings from OpenStreetMap: plain boxes on their footprints,
  // walls pale, a darker band of windows or doors, flat grey roofs. ──
  buildBuildings() {
    const t = this.t;
    const gb = new GeoBuilder({ color: true });
    const walls = [[0.86, 0.84, 0.8], [0.93, 0.92, 0.88], [0.8, 0.76, 0.68], [0.74, 0.76, 0.78], [0.88, 0.82, 0.7]];
    const roof = [0.42, 0.42, 0.43], band = [0.18, 0.2, 0.23];
    const rng = mulberry32(12);
    for (const b of this.D.buildings) {
      const pts = b.pts.slice();
      if (pts.length > 2 && pts[0].x === pts.at(-1).x && pts[0].z === pts.at(-1).z) pts.pop();
      if (pts.length < 3) continue;
      const area = Math.abs(polyArea(pts));
      if (area < 12) continue;
      const cx = pts.reduce((a, p) => a + p.x, 0) / pts.length, cz = pts.reduce((a, p) => a + p.z, 0) / pts.length;
      // Nothing inside the barriers.
      const near = t.distanceToRoad(cx, cz, 60);
      if (near.i >= 0 && near.d < Math.max(t.wallL[near.i], t.wallR[near.i]) - 2) continue;
      let lo = Infinity, hi = -Infinity;
      for (const p of pts) { const h = this.S.sample(p.x, p.z).h; lo = Math.min(lo, h); hi = Math.max(hi, h); }
      const H = area > 2500 ? 8 : area > 700 ? 6 : area > 150 ? 4.6 : 3.2;
      const y0 = lo - 0.6, y1 = hi + H;
      const corners = pts.map((p) => [p.x, p.z]);
      const wc = walls[Math.floor(rng() * walls.length)];
      gb.prism(corners, y0, y1, { color: wc, roofColor: roof });
      // Window / door band.
      if (H >= 4) gb.prism(scalePoly(corners, 1.02), hi + H * 0.28, hi + H * 0.62, { color: band, roof: false });
    }
    if (!gb.empty) {
      const m = this.add(staticMesh(gb.build(), this.mats.solid, { cast: true }));
      m.castShadow = true;
    }
  }

  // ── Grandstands: tiers of seats full of people rising away from the
  // track on each OpenStreetMap footprint, the big ones roofed. ──
  buildGrandstands() {
    const t = this.t;
    const gb = new GeoBuilder({ color: true });
    const crowd = new GeoBuilder();
    const conc = [0.78, 0.77, 0.74], steelC = [0.6, 0.62, 0.66], roofC = [0.88, 0.9, 0.92];
    for (const poly of this.D.grandstands) {
      const pts = poly.slice();
      if (pts.length > 2 && pts[0].x === pts.at(-1).x && pts[0].z === pts.at(-1).z) pts.pop();
      if (pts.length < 3) continue;
      const box = minRect(pts);
      // Front: the long side nearer the track.
      const { c, u, v, hu, hv } = box; // u: unit along the long side, v: across
      const p1 = { x: c.x + v.x * hv, z: c.z + v.z * hv }, p2 = { x: c.x - v.x * hv, z: c.z - v.z * hv };
      const d1 = t.distanceToRoad(p1.x, p1.z, 200).d, d2 = t.distanceToRoad(p2.x, p2.z, 200).d;
      const back = d1 < d2 ? { x: -v.x, z: -v.z } : v; // from front to back
      const front = { x: c.x - back.x * hv, z: c.z - back.z * hv };
      let lo = Infinity;
      for (const p of pts) lo = Math.min(lo, this.S.sample(p.x, p.z).h);
      const depth = hv * 2, rows = Math.max(3, Math.floor(depth / 0.85)), rise = 0.42;
      const y0 = lo + 0.8, y1 = y0 + rows * rise;
      const P = (a, b, y) => [front.x + u.x * a + back.x * b, y, front.z + u.z * a + back.z * b];
      // Seating slope (crowd texture: 8 tiers per texture height).
      const vr = rows / 8, ur = (hu * 2) / 12;
      crowd.quad(P(-hu, 0, y0), P(hu, 0, y0), P(hu, depth, y1), P(-hu, depth, y1), [[0, 0], [ur, 0], [ur, vr], [0, vr]]);
      crowd.quad(P(hu, 0, y0), P(-hu, 0, y0), P(-hu, depth, y1), P(hu, depth, y1), [[0, 0], [ur, 0], [ur, vr], [0, vr]]);
      // Front wall, sides and back.
      const g0 = lo - 0.8;
      gb.quad(P(-hu, 0, g0), P(hu, 0, g0), P(hu, 0, y0), P(-hu, 0, y0), null, conc);
      for (const sgn of [-1, 1]) {
        gb.quad(P(sgn * hu, 0, g0), P(sgn * hu, depth, g0), P(sgn * hu, depth, y1 + 0.9), P(sgn * hu, 0, y0 + 0.9), null, conc);
      }
      gb.quad(P(hu, depth, g0), P(-hu, depth, g0), P(-hu, depth, y1 + 0.9), P(hu, depth, y1 + 0.9), null, conc);
      // Roof on the bigger stands: posts at the back, cantilevered forward.
      if (depth > 9 && hu > 12) {
        const ry = y1 + 4.2;
        gb.quad(P(-hu - 1, -2, ry - 0.8), P(hu + 1, -2, ry - 0.8), P(hu + 1, depth + 0.5, ry), P(-hu - 1, depth + 0.5, ry), null, roofC);
        gb.quad(P(hu + 1, -2, ry - 0.8), P(-hu - 1, -2, ry - 0.8), P(-hu - 1, depth + 0.5, ry), P(hu + 1, depth + 0.5, ry), null, roofC);
        for (let a = -hu; a <= hu + 0.1; a += 12) {
          const q = P(a, depth - 0.3, 0);
          gb.box(q[0], y1 - 1, q[2], 0.4, ry - y1 + 1, 0.4, 0, { color: steelC });
        }
      }
    }
    if (!gb.empty) this.add(staticMesh(gb.build(), this.mats.solid, { cast: true }));
    if (!crowd.empty) this.add(staticMesh(crowd.build(), this.mats.crowd));
  }

  // ── Bridges over the track (OpenStreetMap bridge ways that cross it):
  // steel footbridges and the perimeter road's concrete bridge. ──
  buildBridges() {
    const t = this.t;
    const gb = new GeoBuilder({ color: true });
    const blue = [0.2, 0.36, 0.62], white = [0.9, 0.9, 0.88], conc = [0.72, 0.7, 0.66], dark = [0.25, 0.25, 0.27];
    const atlas = bannerAtlas();
    const ban = new GeoBuilder();
    this.bridges = [];
    for (const br of this.D.bridges) {
      const hit = crossTrack(t, br.pts);
      if (!hit) continue;
      const { s, dir } = hit;
      const f = t.frame(s);
      // Span the corridor: out past both barriers.
      const sin = Math.abs(dir.x * f.fz - dir.z * f.fx) || 1;
      const reach = (Math.max(f.wallL, f.wallR) + 5) / Math.max(0.35, Math.abs(dir.x * f.rx + dir.z * f.rz));
      const road = br.kind !== 'footway' && br.kind !== 'path';
      const W = road ? 8.5 : 3.2;
      const deckY = f.y + CLEAR + 0.3;
      const nx = -dir.z, nz = dir.x;
      const P = (a, b, y) => [f.x + dir.x * a + nx * b, y, f.z + dir.z * a + nz * b];
      const yaw = Math.atan2(dir.z, dir.x);
      // Deck (a box).
      gb.box(f.x, deckY - 0.7, f.z, reach * 2, 0.7, W, yaw, { color: road ? conc : white, bottom: true });
      // Sides: parapets on the road bridge, blue truss girders on footbridges.
      for (const sd of [-1, 1]) {
        const cx = f.x + nx * sd * (W / 2 - 0.15), cz = f.z + nz * sd * (W / 2 - 0.15);
        if (road) gb.box(cx, deckY, cz, reach * 2, 1.0, 0.3, yaw, { color: conc });
        else {
          gb.box(cx, deckY + 1.9, cz, reach * 2, 0.3, 0.3, yaw, { color: blue });
          for (let a = -reach; a <= reach; a += 2.5) {
            const q = P(a, sd * (W / 2 - 0.15), 0);
            gb.box(q[0], deckY, q[2], 0.18, 1.9, 0.18, yaw, { color: blue });
          }
        }
      }
      if (!road) gb.box(f.x, deckY + 2.2, f.z, reach * 2, 0.12, W, yaw, { color: blue }); // canopy
      // Piers and abutments at each end, down to the ground.
      for (const a of [-reach, reach]) {
        const q = P(a, 0, 0);
        const g = this.S.sample(q[0], q[2]).h;
        gb.box(q[0], g - 0.5, q[2], 1.6, deckY - g - 0.2, W * 0.9, yaw, { color: conc });
      }
      // Banners along the girder, facing both ways.
      if (!road) {
        for (const face of [-1, 1]) {
          const [u0, v0, u1, v1] = atlas.rect(face > 0 ? 'seaside' : 'midnight');
          const b0 = P(-6, face * (W / 2 + 0.02), deckY + 0.3), b1 = P(6, face * (W / 2 + 0.02), deckY + 0.3);
          const c1 = [b1[0], deckY + 1.75, b1[2]], c0 = [b0[0], deckY + 1.75, b0[2]];
          if (face > 0) ban.quad(b1, b0, c0, c1, [[u0, v0], [u1, v0], [u1, v1], [u0, v1]]);
          else ban.quad(b0, b1, c1, c0, [[u0, v0], [u1, v0], [u1, v1], [u0, v1]]);
        }
      }
      this.bridges.push({ s, road, deckY });
    }
    if (!gb.empty) this.add(staticMesh(gb.build(), this.mats.solid, { cast: true }));
    if (!ban.empty) this.add(staticMesh(ban.build(), this.mats.banner));
  }

  // ── The infield lake and the ponds nearby. ──
  buildLake() {
    const mat = new THREE.MeshStandardMaterial({ color: 0x3c6462, roughness: 0.12, metalness: 0.2 });
    const ground = this.world.level.ground;
    for (const poly of this.D.water) {
      const pts = poly.slice();
      if (pts.length > 2 && pts[0].x === pts.at(-1).x && pts[0].z === pts.at(-1).z) pts.pop();
      if (pts.length < 3 || Math.abs(polyArea(pts)) < 150) continue;
      // The lidar's water surface: the lowest ground inside the shore.
      const cx = pts.reduce((a, p) => a + p.x, 0) / pts.length, cz = pts.reduce((a, p) => a + p.z, 0) / pts.length;
      let lo = Infinity;
      for (const p of pts) for (const k of [0.3, 0.6, 0.85]) lo = Math.min(lo, ground(lerp(p.x, cx, k), lerp(p.z, cz, k)));
      const contour = pts.map((p) => new THREE.Vector2(p.x, p.z));
      const tris = THREE.ShapeUtils.triangulateShape(contour, []);
      const pos = [];
      for (const p of pts) pos.push(p.x, lo + 0.25, p.z);
      const g = new THREE.BufferGeometry();
      g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
      const idx = [];
      for (const [a, b, c] of tris) {
        // Face up.
        const cr = (pts[b].x - pts[a].x) * (pts[c].z - pts[a].z) - (pts[b].z - pts[a].z) * (pts[c].x - pts[a].x);
        if (cr > 0) idx.push(a, b, c); else idx.push(a, c, b);
      }
      g.setIndex(idx);
      g.computeVertexNormals();
      this.add(staticMesh(g, mat));
    }
  }

  // ── Braking boards before the slow corners, and marshal posts. ──
  buildBoards() {
    const t = this.t, atlas = bannerAtlas();
    const gb = new GeoBuilder();
    const posts = new GeoBuilder({ color: true });
    const f = {};
    for (const c of this.corners) {
      // Only corners you really brake for.
      const drop = t.speedProfile[t.idx(c.s0 - 200)] - t.speedProfile[c.apex];
      if (drop < 14) continue;
      const side = -c.sign; // the outside of the corner
      // Braking starts where the speed profile starts falling.
      let sb = c.apex;
      while (sb > c.apex - 400 && t.speedProfile[t.idx(sb - 1)] > t.speedProfile[t.idx(sb)] + 0.001) sb--;
      for (const [d, name] of [[150, 'b150'], [100, 'b100'], [50, 'b50']]) {
        const s = sb - d + 30;
        t.frame(s, f);
        const w = this.wall(f, side) + ((this.tyreRuns[side] || []).some(([a, b]) => s > a && s < b) ? TYRE_W : 0);
        const lat = side * (w + 0.5);
        const p = t.pointAt(s, lat);
        const g = t.surfaceY(s, side * w);
        const [u0, v0, u1, v1] = atlas.rect(name);
        const y0 = g + WALL_H + 0.3, y1 = y0 + 0.9;
        const hx = f.rx * 1.2, hz = f.rz * 1.2; // half width across the road
        const a = [p.x - hx, y0, p.z - hz], b = [p.x + hx, y0, p.z + hz];
        // Facing oncoming cars (looking along +f): quad wound toward -f.
        gb.quad(b, a, [a[0], y1, a[2]], [b[0], y1, b[2]], [[u1, v0], [u0, v0], [u0, v1], [u1, v1]]);
        gb.quad(a, b, [b[0], y1, b[2]], [a[0], y1, a[2]], [[u0, v0], [u1, v0], [u1, v1], [u0, v1]]);
        posts.box(p.x, g, p.z, 0.12, WALL_H + 0.35, 0.12, 0, { color: [0.3, 0.3, 0.32] });
      }
      // Marshal post: a white hut behind the wall on the outside, with a
      // yellow flag on a pole.
      const s = c.apex + 10;
      t.frame(s, f);
      const w = this.wall(f, side) + 4;
      const p = t.pointAt(s, side * w);
      const g = this.S.sample(p.x, p.z).h;
      const yaw = Math.atan2(f.fz, f.fx);
      posts.box(p.x, g - 0.3, p.z, 2.2, 2.6, 2.0, yaw, { color: [0.92, 0.92, 0.9], roofColor: [0.7, 0.2, 0.18] });
      posts.box(p.x + f.fx * 1.5, g, p.z + f.fz * 1.5, 0.06, 4.2, 0.06, 0, { color: [0.7, 0.7, 0.72] });
      posts.box(p.x + f.fx * 1.5 + f.rx * side * 0.55, g + 3.4, p.z + f.fz * 1.5 + f.rz * side * 0.55, 1.0, 0.7, 0.02, yaw + Math.PI / 2, { color: [0.95, 0.8, 0.1] });
    }
    if (!gb.empty) this.add(staticMesh(gb.build(), this.mats.banner));
    if (!posts.empty) this.add(staticMesh(posts.build(), this.mats.solid, { cast: true }));
  }

  // ── Coast live oaks wherever the aerial photo shows a crown: 8 m cells
  // near the circuit, 32 m cells out to the hills. ──
  buildTrees() {
    const t = this.t, D = this.D;
    const rng = mulberry32(2024);
    const near = [], far = [];
    const inCorridor = (x, z) => {
      const r = t.distanceToRoad(x, z, 60);
      if (r.i < 0) return false;
      const w = r.lat < 0 ? t.wallL[r.i] : t.wallR[r.i];
      return r.d < w + 3;
    };
    // Detailed crowns only where you pass close by.
    const place = (x, z, r, list) => {
      const g = this.S.sample(x, z).h;
      list.push({ x, z, y: g, r, hi: list === near && t.distanceToRoad(x, z, 140).d < 140 });
    };
    // Near: the fine canopy grid, 2 × 2 cells (8 m) at a time.
    const tf = D.grids.treesFine;
    const cover = tf.values;
    for (let j = 0; j + 1 < tf.h; j += 2) {
      for (let i = 0; i + 1 < tf.w; i += 2) {
        const c = (cover[j * tf.w + i] + cover[j * tf.w + i + 1] + cover[(j + 1) * tf.w + i] + cover[(j + 1) * tf.w + i + 1]) / 4;
        if (c < 0.1 || rng() > c * 0.75) continue;
        const x = tf.x0 + (i + 0.5 + (rng() - 0.5) * 1.6) * tf.step, z = tf.z0 + (j + 0.5 + (rng() - 0.5) * 1.6) * tf.step;
        if (inCorridor(x, z)) continue;
        place(x, z, lerp(3.2, 5.6, rng()) * (0.85 + c * 0.35), near);
      }
    }
    // Far: the wide grid, outside the fine one, within 2.6 km.
    const tw = D.grids.treesWide;
    const fx0 = tf.x0, fx1 = tf.x0 + (tf.w - 1) * tf.step, fz0 = tf.z0, fz1 = tf.z0 + (tf.h - 1) * tf.step;
    const cx = (t.bounds.minX + t.bounds.maxX) / 2, cz = (t.bounds.minZ + t.bounds.maxZ) / 2;
    for (let j = 0; j < tw.h; j++) {
      for (let i = 0; i < tw.w; i++) {
        const c = tw.values[j * tw.w + i];
        if (c < 0.06) continue;
        const x0 = tw.x0 + i * tw.step, z0 = tw.z0 + j * tw.step;
        if (Math.hypot(x0 - cx, z0 - cz) > 2600) continue;
        if (x0 > fx0 && x0 < fx1 && z0 > fz0 && z0 < fz1) continue;
        // Fewer, bigger crowns in the distance: clumps read as woodland.
        const nTree = Math.floor(c * 1.3 + rng() * 0.8);
        for (let k = 0; k < nTree; k++) {
          place(x0 + (rng() - 0.5) * tw.step, z0 + (rng() - 0.5) * tw.step, lerp(5, 8.5, rng()), far);
        }
      }
    }
    this.treeCount = near.length + far.length;
    const mat = new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.95 });
    const trunkMat = new THREE.MeshStandardMaterial({ color: 0x4a3b2c, roughness: 0.95 });
    const crownHi = canopyGeometry('shade', 7, 0), crownLo = blobGeometry(7);
    const trunk = new THREE.CylinderGeometry(0.22, 0.34, 1, 5);
    trunk.translate(0, 0.5, 0);
    const col = new THREE.Color();
    const m4 = new THREE.Matrix4(), q = new THREE.Quaternion(), e = new THREE.Euler(), p = new THREE.Vector3(), sc = new THREE.Vector3();
    // Chunked so each piece can be frustum-culled.
    const CH = 420;
    const groups = new Map();
    const push = (key, item) => { if (!groups.has(key)) groups.set(key, []); groups.get(key).push(item); };
    for (const tr of near) push((tr.hi ? 'n:' : 'm:') + Math.floor(tr.x / CH) + ',' + Math.floor(tr.z / CH), tr);
    for (const tr of far) push('f:' + Math.floor(tr.x / (CH * 2)) + ',' + Math.floor(tr.z / (CH * 2)), tr);
    for (const [key, list] of groups) {
      const hi = key[0] === 'n';
      const crowns = new THREE.InstancedMesh(hi ? crownHi : crownLo, mat, list.length);
      const trunks = hi ? new THREE.InstancedMesh(trunk, trunkMat, list.length) : null;
      list.forEach((tr, k) => {
        const r = tr.r;
        const th = r * 0.42 + 0.6; // trunk height to the crown's underside
        e.set(0, rng() * Math.PI * 2, 0); q.setFromEuler(e);
        // Seen from further off, a low dome with no trunk showing.
        if (hi) m4.compose(p.set(tr.x, tr.y + th + r * 0.45, tr.z), q, sc.set(r * 0.95, r * 0.62, r * 0.95));
        else m4.compose(p.set(tr.x, tr.y + r * 0.42, tr.z), q, sc.set(r, r * 0.66, r));
        crowns.setMatrixAt(k, m4);
        // Coast live oak: dark olive, some sunnier.
        col.setRGB(lerp(0.075, 0.12, rng()), lerp(0.1, 0.15, rng()), lerp(0.035, 0.055, rng()));
        crowns.setColorAt(k, col);
        if (trunks) {
          m4.compose(p.set(tr.x, tr.y - 0.2, tr.z), q, sc.set(r * 0.28, th + r * 0.2, r * 0.28));
          trunks.setMatrixAt(k, m4);
        }
      });
      crowns.computeBoundingSphere();
      crowns.castShadow = hi;
      crowns.receiveShadow = true;
      this.add(crowns);
      if (trunks) { trunks.computeBoundingSphere(); this.add(trunks); }
    }
  }
}

// ── Geometry helpers ─────────────────────────────────────────────────
// A distant crown: one lumpy icosahedron (20 triangles), lit like the
// detailed ones (normals from the centre, darker underneath).
function blobGeometry(seed) {
  const rng = mulberry32(seed);
  const g = new THREE.IcosahedronGeometry(1, 0).toNonIndexed();
  const p = g.getAttribute('position');
  const byKey = new Map();
  for (let i = 0; i < p.count; i++) {
    const key = p.getX(i).toFixed(3) + ',' + p.getY(i).toFixed(3) + ',' + p.getZ(i).toFixed(3);
    if (!byKey.has(key)) byKey.set(key, 0.85 + rng() * 0.3);
    const k = byKey.get(key);
    p.setXYZ(i, p.getX(i) * k, p.getY(i) * k, p.getZ(i) * k);
  }
  const nrm = new Float32Array(p.count * 3), col = new Float32Array(p.count * 3);
  for (let i = 0; i < p.count; i++) {
    const x = p.getX(i), y = p.getY(i), z = p.getZ(i), l = Math.hypot(x, y + 0.15, z) || 1;
    nrm[i * 3] = x / l; nrm[i * 3 + 1] = (y + 0.15) / l; nrm[i * 3 + 2] = z / l;
    const ao = lerp(0.62, 1.05, smoothstep(-0.9, 0.7, y));
    col[i * 3] = ao * 1.05; col[i * 3 + 1] = ao; col[i * 3 + 2] = ao * 0.85;
  }
  g.setAttribute('normal', new THREE.BufferAttribute(nrm, 3));
  g.setAttribute('color', new THREE.BufferAttribute(col, 3));
  g.deleteAttribute('uv');
  return g;
}

function polyArea(pts) {
  let a = 0;
  for (let i = 0; i < pts.length; i++) {
    const p = pts[i], q = pts[(i + 1) % pts.length];
    a += p.x * q.z - q.x * p.z;
  }
  return a / 2;
}

function scalePoly(corners, k) {
  const cx = corners.reduce((a, p) => a + p[0], 0) / corners.length, cz = corners.reduce((a, p) => a + p[1], 0) / corners.length;
  return corners.map(([x, z]) => [cx + (x - cx) * k, cz + (z - cz) * k]);
}

// Points every `step` metres along a polyline.
function resample(pts, step) {
  const out = [pts[0]];
  let carry = 0;
  for (let i = 0; i < pts.length - 1; i++) {
    const a = pts[i], b = pts[i + 1];
    const l = Math.hypot(b.x - a.x, b.z - a.z);
    let d = step - carry;
    while (d <= l) { out.push({ x: a.x + (b.x - a.x) * d / l, z: a.z + (b.z - a.z) * d / l }); d += step; }
    carry = l - (d - step);
  }
  out.push(pts.at(-1));
  return out;
}

// Minimum-area bounding rectangle (edges of a small polygon).
function minRect(pts) {
  let best = null;
  for (let i = 0; i < pts.length; i++) {
    const a = pts[i], b = pts[(i + 1) % pts.length];
    const l = Math.hypot(b.x - a.x, b.z - a.z);
    if (l < 0.5) continue;
    const u = { x: (b.x - a.x) / l, z: (b.z - a.z) / l }, v = { x: -u.z, z: u.x };
    let u0 = Infinity, u1 = -Infinity, v0 = Infinity, v1 = -Infinity;
    for (const p of pts) {
      const pu = p.x * u.x + p.z * u.z, pv = p.x * v.x + p.z * v.z;
      u0 = Math.min(u0, pu); u1 = Math.max(u1, pu); v0 = Math.min(v0, pv); v1 = Math.max(v1, pv);
    }
    const area = (u1 - u0) * (v1 - v0);
    if (!best || area < best.area) {
      const mu = (u0 + u1) / 2, mv = (v0 + v1) / 2;
      best = { area, u, v, hu: (u1 - u0) / 2, hv: (v1 - v0) / 2, c: { x: u.x * mu + v.x * mv, z: u.z * mu + v.z * mv } };
    }
  }
  // Long side along u.
  if (best.hv > best.hu) best = { ...best, u: best.v, v: { x: -best.u.x, z: -best.u.z }, hu: best.hv, hv: best.hu };
  return best;
}

// Where a polyline crosses the track centreline: {s, dir (unit, along the
// polyline)}, or null.
function crossTrack(t, pts) {
  for (let i = 0; i < pts.length - 1; i++) {
    const a = pts[i], b = pts[i + 1];
    const mx = (a.x + b.x) / 2, mz = (a.z + b.z) / 2;
    const half = Math.hypot(b.x - a.x, b.z - a.z) / 2 + 4;
    const k0 = t.nearest(mx, mz, half + 30);
    if (k0 < 0) continue;
    for (let o = -60; o <= 60; o++) {
      const k = t.idx(k0 + o), k1 = t.idx(k0 + o + 1);
      const p = { x: t.px[k], z: t.pz[k] }, q = { x: t.px[k1], z: t.pz[k1] };
      const d1x = b.x - a.x, d1z = b.z - a.z, d2x = q.x - p.x, d2z = q.z - p.z;
      const den = d1x * d2z - d1z * d2x;
      if (Math.abs(den) < 1e-9) continue;
      const ta = ((p.x - a.x) * d2z - (p.z - a.z) * d2x) / den;
      const tb = ((p.x - a.x) * d1z - (p.z - a.z) * d1x) / den;
      if (ta >= 0 && ta <= 1 && tb >= 0 && tb <= 1) {
        const l = Math.hypot(d1x, d1z);
        return { s: k0 + o + tb, dir: { x: d1x / l, z: d1z / l } };
      }
    }
  }
  return null;
}
