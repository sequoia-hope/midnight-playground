import * as THREE from 'three';
import { mergeGeometries } from 'three/addons/utils/BufferGeometryUtils.js';
import { clamp, lerp, smoothstep, mulberry32, rrange, rpick } from '../util/math.js';
import { Builder } from './valley/Builder.js';
import { ColorBuilder } from './beach/ColorBuilder.js';
import { makeGround } from './valley/ground.js';
import { glowTexture } from './textures.js';
import { buildVehicle } from '../vehicles/CarModel.js';
import { SignAtlas, paintedSign, neonSign, signGeometry } from './beach/atlas.js';
import {
  WALLS, shop, surfShop, diner, tacoStand, motel, gasStation, beachHouse, hotel,
  bench, promLamp, streetLight, trashCan, lifeguardTower, umbrellaSet, volleyballNet,
  surfboardsInSand, sailboat, motorboat, picnicTable,
} from './beach/parts.js';

// Seabright: the beach town on Level 2. The boulevard runs along the sand
// with the sea on the left: a boardwalk promenade, lifeguard towers and a
// long pier with a Ferris wheel and a little coaster on the ocean side;
// pastel motels, surf shops, a diner and a gas station on the town side,
// with a grid of beach houses behind; palms everywhere; a marina with
// sailboats at the far end. It's sunrise here, so lamps and neon fade out
// as the sun comes up.

const yawZ = (dx, dz) => Math.atan2(dx, dz); // local +Z → (dx, dz)

// Plain paints: merged into one vertex-coloured mesh per channel.
const PALETTE = {
  wPink: 0xe9aea8, wMint: 0xa9d8c1, wYellow: 0xf1d690, wBlue: 0x9fc3de, wWhite: 0xefebe1,
  wPeach: 0xf1bf95, wLilac: 0xc6b6de, wTeal: 0x6db8b1, wSand: 0xdcc9a2,
  trim: 0xf6f3ea, concrete: 0xc4bdb0, white: 0xf2f2ee, wood: 0x9c7651, woodDark: 0x5a3f2b,
  roofTar: 0x55524d, roofTile: 0xbf6a4c, black: 0x1b1b1d, boardB: 0xff8a3c, paintRed: 0xc83a34,
  lawn: 0x6f9a45, hedge: 0x3f6a32, sailBlue: 0x2b4f86, hullWhite: 0xf1f2ee,
};

function canvasTex(w, h, draw, { repeat = true, srgb = true } = {}) {
  const c = document.createElement('canvas');
  c.width = w; c.height = h;
  draw(c.getContext('2d'), w, h);
  const t = new THREE.CanvasTexture(c);
  if (repeat) t.wrapS = t.wrapT = THREE.RepeatWrapping;
  t.colorSpace = srgb ? THREE.SRGBColorSpace : THREE.NoColorSpace;
  t.anisotropy = 8;
  return t;
}

// The plain-painted surfaces (walls, trim, wood) share one vertex-coloured
// material. A world-space triplanar grain gives the stucco and boards some
// tooth, and a broad mottling breaks up the colour, so big pastel walls
// don't read as flat fills.
function stuccoMaterial() {
  const noise = canvasTex(128, 128, (g, w, h) => {
    const img = g.createImageData(w, h);
    const rng = mulberry32(19);
    const lat = new Float32Array(16 * 16).map(() => rng());
    for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
      const fx = x / 8, fy = y / 8, x0 = Math.floor(fx), y0 = Math.floor(fy), tx = fx - x0, ty = fy - y0;
      const at = (i, j) => lat[(j & 15) * 16 + (i & 15)];
      const broad = (at(x0, y0) * (1 - tx) + at(x0 + 1, y0) * tx) * (1 - ty) + (at(x0, y0 + 1) * (1 - tx) + at(x0 + 1, y0 + 1) * tx) * ty;
      const v = 150 + broad * 60 + (rng() - 0.5) * 50;
      const i = (y * w + x) * 4;
      img.data[i] = img.data[i + 1] = img.data[i + 2] = v; img.data[i + 3] = 255;
    }
    g.putImageData(img, 0, 0);
  }, { srgb: false });
  const m = new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.85 });
  m.userData.kind = 'Stucco'; // MaterialKind for the scene export
  m.onBeforeCompile = (sh) => {
    sh.uniforms.tGrain = { value: noise };
    sh.vertexShader = sh.vertexShader
      .replace('#include <common>', '#include <common>\nvarying vec3 vGP;\nvarying vec3 vGN;')
      .replace('#include <fog_vertex>', '#include <fog_vertex>\nvGP = (modelMatrix * vec4(transformed, 1.0)).xyz;\nvGN = normalize(mat3(modelMatrix) * objectNormal);');
    sh.fragmentShader = sh.fragmentShader
      .replace('#include <common>', '#include <common>\nvarying vec3 vGP;\nvarying vec3 vGN;\nuniform sampler2D tGrain;')
      .replace('#include <color_fragment>', `#include <color_fragment>
        vec3 w_ = abs(normalize(vGN)); w_ /= (w_.x + w_.y + w_.z);
        float g_ = texture2D(tGrain, vGP.zy * 0.45).r * w_.x + texture2D(tGrain, vGP.xz * 0.45).r * w_.y + texture2D(tGrain, vGP.xy * 0.45).r * w_.z;
        float f_ = texture2D(tGrain, vGP.xz * 0.06 + vGP.y * 0.02).r;
        diffuseColor.rgb *= (0.8 + 0.34 * g_) * (0.9 + 0.16 * f_);`);
  };
  m.customProgramCacheKey = () => 'beach-stucco';
  return m;
}

export default class Beach {
  constructor({ zone = 1 } = {}) {
    this.zone = zone;
    this.label = 'Building Seabright';
  }

  // ── Planning: terraces for the houses on the hill at the town entry ──
  plan(world) {
    const t = (this.t = world.track);
    this.T = world.terrain;
    this.setupRanges();
    const rng = mulberry32(8812);
    this.hillHouses = [];
    for (let s = this.zs0 + 40; s < this.townS0 - 40; s += rrange(rng, 55, 80)) {
      const f = t.frame(s);
      const lat = f.wallR + rrange(rng, 32, 60);
      const p = t.pointAt(s, lat);
      this.hillHouses.push({ s, lat, x: p.x, z: p.z, yaw: yawZ(-f.rx, -f.rz), seed: Math.floor(rng() * 1e9) });
      world.terrain.addFlatten(p.x, p.z, 10, 9, null);
    }
  }

  setupRanges() {
    const t = this.t, z = this.zone;
    this.zs0 = t.zoneStart[z];
    this.zs1 = z + 1 < t.zoneStart.length ? t.zoneStart[z + 1] : t.length;
    const tag = (n) => t.tags.find((g) => g.tag === n && g.s0 >= this.zs0 - 5 && g.s0 < this.zs1);
    const mid = (g, d) => (g ? Math.round((g.s0 + g.s1) / 2) : d);
    this.pierS = mid(tag('pier'), this.zs0 + 700);
    this.marinaS = mid(tag('marina'), this.zs1 - 300);
    this.townS0 = (tag('promenade')?.s0 ?? this.zs0 + 420) + 50;
    this.townS1 = this.zs1 - 70;
    const cs = [];
    for (let k = -1; k < 6; k++) {
      const s = this.pierS + k * 257;
      if (s > this.townS0 + 30 && s < this.townS1 - 25) cs.push(s);
    }
    this.crossS = cs;
    // Open the kerb railing where each cross street meets the boulevard.
    if (this.t?.fenceGaps) for (const c of cs) this.t.fenceGaps.push({ s0: c - 5.5, s1: c + 5.5, side: 1 });
  }

  // ── Build ────────────────────────────────────────────────────────
  async build(world) {
    const t0 = performance.now();
    this.world = world;
    this.t = world.track; this.T = world.terrain;
    if (this.zs0 === undefined) this.setupRanges();
    this.ground = makeGround(world.terrain);
    this.group = new THREE.Group();
    this.group.name = 'beach';
    this.B = new ColorBuilder(PALETTE);
    this.ribbons = new Map(); // material → geometries, merged at the end
    this.rng = mulberry32(4210);
    this.occ = [];          // occupied discs {x, z, r}
    this.palms = [];        // {x, y, z, h, yaw, s}
    this.cars = [];         // parked cars
    this.pools = [];        // road light pools {x, y, z}
    this.makeMaterials();
    this.makeSigns();

    this.reserveStreets();
    this.buildPromenade();
    this.buildFrontage();
    this.B.channel = 'far';       // houses behind main street: no shadow casting
    this.buildResidential();
    this.buildHillHouses();
    this.B.channel = 'near';
    this.buildStreetFurniture();
    this.buildSidewalkProps();
    this.buildStreetParking();
    this.buildBeachProps();
    this.buildPier();
    this.buildMarina();
    this.buildTrafficLights();
    this.buildTownSign();
    this.buildFoam();

    const M = this.M;
    for (const mesh of this.B.build(M, { castShadow: ['metal', 'awnRed', 'awnBlue', 'awnYellow', 'awnGreen'] })) this.group.add(mesh);
    this.mergeRibbons();
    this.buildPalms();
    this.buildCars();
    this.buildPools();
    world.scene.add(this.group);

    world.addNight(M.glassLit, 'emissiveIntensity', 0.0, 1.5);
    world.addNight(M.lampGlow, 'emissiveIntensity', 0.15, 4.5);
    world.addNight(M.neon, 'emissiveIntensity', 0.55, 3.2);
    world.addNight(M.signs, 'emissiveIntensity', 0.0, 0.3);
    world.addNight(M.canopyLight, 'emissiveIntensity', 0.4, 2.6);
    world.addNight(this.bulbMat, 'emissiveIntensity', 0.6, 5.0);
    world.addNight(M.pool, 'emissiveIntensity', 0.05, 0.6);
    world.updaters.push((dt, night) => this.animate(dt, night));
    this.stats = { ms: Math.round(performance.now() - t0), meshes: this.group.children.length, palms: this.palms.length, cars: this.cars.length };
    world.beach = this;
  }

  // ── Helpers ──────────────────────────────────────────────────────
  gy(x, z) { return this.ground.height(x, z); }

  free(x, z, r) {
    for (const o of this.occ) {
      const dx = x - o.x, dz = z - o.z;
      if (dx * dx + dz * dz < (r + o.r) * (r + o.r)) return false;
    }
    return this.clearOfRoad(x, z, r);
  }

  take(x, z, r) { this.occ.push({ x, z, r }); }

  // Keep everything outside the driving corridor (with room to spare).
  clearOfRoad(x, z, r) {
    const t = this.t;
    const k = t.nearest(x, z, r + 40);
    if (k < 0) return true;
    const p = t.project(x, z, k, {}, 8);
    const wall = p.lat > 0 ? t.wallR[p.i] : t.wallL[p.i];
    return Math.abs(p.lat) - r > wall + 0.6;
  }

  // Frame at (s, lat): origin on the rendered ground, +Z facing `dir`
  // ('road' or 'away'), X along the road.
  frameAt(s, lat, dir = 'road') {
    const f = this.t.frame(s);
    const p = this.t.pointAt(s, lat);
    const side = Math.sign(lat) || 1;
    const k = dir === 'road' ? -side : side;
    return { x: p.x, z: p.z, y: this.gy(p.x, p.z), yaw: yawZ(k * f.rx, k * f.rz), f };
  }

  toWorld(fr, lx, lz) {
    const c = Math.cos(fr.yaw), s = Math.sin(fr.yaw);
    return [fr.x + lx * c + lz * s, fr.z - lx * s + lz * c];
  }

  // Ribbon following the road between two lateral offsets.
  stripAlong(s0, s1, lat0, lat1, mat, { step = 4, lift = 0.12, uScale = 4, vScale = 4, name = 'strip' } = {}) {
    const t = this.t, pos = [], uv = [], idx = [];
    const lats = [lat0, (lat0 + lat1) / 2, lat1];
    let rows = 0;
    for (let s = s0; ; s += step) {
      const ss = Math.min(s, s1);
      for (const lat of lats) {
        const p = t.pointAt(ss, lat);
        pos.push(p.x, this.gy(p.x, p.z) + lift, p.z);
        uv.push((lat - lat0) / uScale, ss / vScale);
      }
      rows++;
      if (ss >= s1) break;
    }
    for (let r = 0; r < rows - 1; r++) for (let c = 0; c < 2; c++) {
      const a = r * 3 + c, b = a + 1, d = a + 3, e = d + 1;
      idx.push(a, b, d, b, e, d); // counter-clockwise from above
    }
    return this.addRibbonMesh(pos, uv, idx, mat, name);
  }

  // Ribbon along an arbitrary path [{x,z}].
  ribbon(path, width, mat, { lift = 0.1, name = 'ribbon', yFn = null } = {}) {
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
        pos.push(x, (yFn ? yFn(i, x, z) : this.gy(x, z)) + lift, z);
        uv.push(sgn * 0.5 + 0.5, along / 4);
      }
      if (i > 0) { const k = i * 2; idx.push(k - 2, k - 1, k, k - 1, k + 1, k); }
    }
    return this.addRibbonMesh(pos, uv, idx, mat, name);
  }

  addRibbonMesh(pos, uv, idx, mat, name) {
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
    g.setAttribute('uv', new THREE.Float32BufferAttribute(uv, 2));
    g.setIndex(idx);
    g.computeVertexNormals();
    const n = g.getAttribute('normal');
    for (let i = 0; i < n.count; i++) if (n.getY(i) < 0) n.setXYZ(i, -n.getX(i), -n.getY(i), -n.getZ(i));
    if (!this.ribbons.has(mat)) this.ribbons.set(mat, { name, geos: [] });
    this.ribbons.get(mat).geos.push(g);
    return g;
  }

  // One mesh per ribbon material (streets, sidewalk, boardwalk, foam).
  mergeRibbons() {
    for (const [mat, { name, geos }] of this.ribbons) {
      const g = geos.length === 1 ? geos[0] : this.mergeList(geos.map((x) => x.toNonIndexed()));
      g.computeBoundingSphere();
      const m = new THREE.Mesh(g, mat);
      m.name = 'beach:' + name;
      m.receiveShadow = true;
      m.matrixAutoUpdate = false;
      if (mat.transparent) m.renderOrder = 3;
      this.group.add(m);
    }
    this.ribbons.clear();
  }

  // ── Materials and signage ────────────────────────────────────────
  makeMaterials() {
    const S = (color, o = {}) => new THREE.MeshStandardMaterial({ color, roughness: 0.85, metalness: 0, ...o });
    const stripes = (a, b) => canvasTex(64, 64, (g, w, h) => {
      for (let i = 0; i < 8; i++) { g.fillStyle = i % 2 ? b : a; g.fillRect(i * w / 8, 0, w / 8, h); }
    });
    const planks = canvasTex(256, 256, (g, w, h) => {
      g.fillStyle = '#9a7650'; g.fillRect(0, 0, w, h);
      const rng = mulberry32(3);
      for (let y = 0; y < h; y += 16) {
        g.fillStyle = `rgb(${130 + rng() * 40},${98 + rng() * 30},${64 + rng() * 22})`;
        g.fillRect(0, y, w, 14);
        g.fillStyle = 'rgba(40,25,15,0.55)'; g.fillRect(0, y + 14, w, 2);
        g.fillStyle = 'rgba(40,25,15,0.35)'; g.fillRect(Math.floor(rng() * w), y, 2, 14);
      }
    });
    const paving = canvasTex(128, 128, (g, w, h) => {
      g.fillStyle = '#c9c3b6'; g.fillRect(0, 0, w, h);
      g.strokeStyle = 'rgba(80,70,60,0.35)'; g.lineWidth = 2;
      for (let k = 0; k <= w; k += 32) { g.beginPath(); g.moveTo(k, 0); g.lineTo(k, h); g.stroke(); g.beginPath(); g.moveTo(0, k); g.lineTo(w, k); g.stroke(); }
      const rng = mulberry32(9);
      for (let i = 0; i < 400; i++) { g.fillStyle = `rgba(${rng() < 0.5 ? '255,255,255' : '0,0,0'},0.05)`; g.fillRect(rng() * w, rng() * h, 2, 2); }
    });
    const asphalt = canvasTex(128, 128, (g, w, h) => {
      g.fillStyle = '#3f4044'; g.fillRect(0, 0, w, h);
      const rng = mulberry32(12);
      for (let i = 0; i < 1500; i++) { const v = 50 + rng() * 40; g.fillStyle = `rgb(${v},${v},${v + 3})`; g.fillRect(rng() * w, rng() * h, 1.5, 1.5); }
      g.fillStyle = 'rgba(235,190,60,0.9)'; g.fillRect(w / 2 - 2, 0, 4, h * 0.55);
    });
    // Window panes: a painted frame and glazing bars around a sky
    // reflection, so every glass box reads as a window. The lit variant
    // glows only through the panes.
    const pane = (lit) => canvasTex(64, 64, (g, w, h) => {
      if (lit) { g.fillStyle = '#000'; g.fillRect(0, 0, w, h); }
      else {
        const grd = g.createLinearGradient(0, 0, w * 0.4, h);
        grd.addColorStop(0, '#9fb8c8'); grd.addColorStop(0.45, '#5f7888'); grd.addColorStop(0.5, '#7d97a8'); grd.addColorStop(1, '#3a4a58');
        g.fillStyle = grd; g.fillRect(0, 0, w, h);
      }
      const fr = lit ? '#000' : '#eeeae0';
      g.fillStyle = lit ? '#ffd9a0' : 'rgba(0,0,0,0)';
      if (lit) g.fillRect(5, 5, w - 10, h - 10);
      g.fillStyle = fr;
      g.fillRect(0, 0, w, 5); g.fillRect(0, h - 5, w, 5); g.fillRect(0, 0, 5, h); g.fillRect(w - 5, 0, 5, h);
      g.fillRect(w / 2 - 2, 0, 4, h); g.fillRect(0, h * 0.45 - 2, w, 4);
    }, { repeat: false });
    const M = {
      wPink: S(0xe9aea8), wMint: S(0xa9d8c1), wYellow: S(0xf1d690), wBlue: S(0x9fc3de), wWhite: S(0xefebe1),
      wPeach: S(0xf1bf95), wLilac: S(0xc6b6de), wTeal: S(0x6db8b1), wSand: S(0xdcc9a2),
      trim: S(0xf6f3ea), concrete: S(0xc4bdb0), white: S(0xf2f2ee, { roughness: 0.6 }),
      glass: S(0xffffff, { map: pane(false), roughness: 0.2, metalness: 0.1 }),
      glassLit: S(0x8a8478, { map: pane(false), roughness: 0.2, metalness: 0.1, emissive: 0xffc98a, emissiveMap: pane(true), emissiveIntensity: 0 }),
      balGlass: S(0x9fc6d4, { roughness: 0.1, metalness: 0.3, transparent: true, opacity: 0.45 }),
      wood: S(0x9c7651), woodDark: S(0x5a3f2b),
      awnRed: S(0xffffff, { map: stripes('#d8433f', '#f6f1e6') }),
      awnBlue: S(0xffffff, { map: stripes('#2f78b8', '#f6f1e6') }),
      awnYellow: S(0xffffff, { map: stripes('#f2c233', '#f6f1e6') }),
      awnGreen: S(0xffffff, { map: stripes('#3f9e6a', '#f6f1e6') }),
      roofTar: S(0x55524d), roofTile: S(0xbf6a4c),
      metal: S(0x9a9fa4, { metalness: 0.7, roughness: 0.35 }),
      black: S(0x1b1b1d, { roughness: 0.6 }),
      boardB: S(0xff8a3c, { roughness: 0.35 }),
      lawn: S(0x6f9a45, { roughness: 1 }), hedge: S(0x3f6a32, { roughness: 1 }),
      paintRed: S(0xc83a34, { roughness: 0.5 }), paintWhite: S(0xf0f0ea, { roughness: 0.7, polygonOffset: true, polygonOffsetFactor: -2, polygonOffsetUnits: -2 }),
      pool: S(0x3fc7d6, { roughness: 0.1, emissive: 0x2aa6c0, emissiveIntensity: 0.05 }),
      asphaltLot: S(0x4a4b4f, { roughness: 0.95, polygonOffset: true, polygonOffsetFactor: -1, polygonOffsetUnits: -1 }),
      canopyLight: S(0xffffff, { emissive: 0xf4f8ff, emissiveIntensity: 0.4 }),
      lampGlow: S(0xfff4dc, { emissive: 0xffd9a0, emissiveIntensity: 0.15 }),
      hullWhite: S(0xf1f2ee, { roughness: 0.4 }), sailBlue: S(0x2b4f86),
      sigRed: S(0x300808, { emissive: 0xff2a1a, emissiveIntensity: 0 }),
      sigAmber: S(0x302008, { emissive: 0xffa020, emissiveIntensity: 0 }),
      sigGreen: S(0x08301a, { emissive: 0x30ff90, emissiveIntensity: 0 }),
    };
    this.planks = planks;
    M.solid = stuccoMaterial();
    M.deckPlank = S(0xffffff, { map: planks, roughness: 0.9 });
    M.paving = S(0xffffff, { map: paving, roughness: 0.95, polygonOffset: true, polygonOffsetFactor: -1, polygonOffsetUnits: -2 });
    M.street = S(0xffffff, { map: asphalt, roughness: 0.95, polygonOffset: true, polygonOffsetFactor: -2, polygonOffsetUnits: -3 });
    this.M = M;
  }

  makeSigns() {
    const A = new SignAtlas(2048, '#ffffff');
    const N = new SignAtlas(1024, '#0b0a10');
    const P = (text, o) => A.add(512, 128, paintedSign(text, o));
    this.sg = {
      surf: P('SURF SHOP', { bg: '#1f7f8c', fg: '#fff4d8', font: 'bold 84px "Arial Black", Arial', stripe: '#f2c233' }),
      icecream: P('ICE CREAM', { bg: '#f7d6e3', fg: '#c2346c', font: 'bold 80px "Brush Script MT", cursive' }),
      bikes: P('BIKE RENTALS', { bg: '#f6efe0', fg: '#2f6aa8', border: '#2f6aa8' }),
      cafe: P('BOARDWALK CAFE', { bg: '#3b2a20', fg: '#f3dfb8', font: 'bold 70px Georgia, serif' }),
      swim: P('SWIMWEAR', { bg: '#ffffff', fg: '#e24d6c', font: 'bold 84px "Arial Black", Arial' }),
      pizza: P('SLICE OF LIFE PIZZA', { bg: '#1c6b3a', fg: '#fff', stripe: '#c83a34' }),
      pharmacy: P('PHARMACY', { bg: '#ffffff', fg: '#1b7a3a', border: '#1b7a3a' }),
      supply: P('MARINA SUPPLY', { bg: '#1d3f75', fg: '#fff', stripe: '#e8e6df' }),
      bait: P('BAIT & TACKLE', { bg: '#e8d9a8', fg: '#23466e', font: 'bold 74px Georgia, serif' }),
      records: P('WAVE RECORDS', { bg: '#141414', fg: '#ffcf4d' }),
      tacos: A.add(384, 128, paintedSign('TACOS', { bg: '#f2c233', fg: '#c83a34', font: 'bold 96px "Arial Black", Arial', sub: null })),
      motelBoard: P('SEA BREEZE MOTEL', { bg: '#f6efe0', fg: '#1f7f8c', font: 'bold 64px Georgia, serif' }),
      gas: P('SEABRIGHT FUEL', { bg: '#c83a34', fg: '#fff' }),
      price: A.add(256, 256, (g, w, h) => {
        g.fillStyle = '#fff'; g.fillRect(0, 0, w, h);
        g.fillStyle = '#c83a34'; g.fillRect(0, 0, w, 64);
        g.fillStyle = '#fff'; g.font = 'bold 44px Arial'; g.textAlign = 'center'; g.textBaseline = 'middle'; g.fillText('FUEL', w / 2, 32);
        g.fillStyle = '#111'; g.font = 'bold 40px "Courier New", monospace';
        ['REG 4.39', 'PLS 4.59', 'DSL 4.79'].forEach((l, i) => g.fillText(l, w / 2, 100 + i * 56));
      }),
      pier: A.add(1024, 160, paintedSign('SEABRIGHT PIER', { bg: '#f6f1e6', fg: '#1f5f8b', font: 'bold 110px "Arial Black", Arial', stripe: '#d8433f' })),
      welcome: A.add(768, 384, (g, w, h) => {
        const grd = g.createLinearGradient(0, 0, 0, h);
        grd.addColorStop(0, '#ffb86b'); grd.addColorStop(0.55, '#ff7a8a'); grd.addColorStop(1, '#3fb6c8');
        g.fillStyle = grd; g.fillRect(0, 0, w, h);
        g.fillStyle = 'rgba(255,240,200,0.9)'; g.beginPath(); g.arc(w * 0.78, h * 0.46, 70, 0, Math.PI * 2); g.fill();
        g.strokeStyle = '#fff'; g.lineWidth = 10; g.strokeRect(10, 10, w - 20, h - 20);
        g.fillStyle = '#fff'; g.textAlign = 'center'; g.textBaseline = 'middle';
        g.font = 'bold 42px Arial'; g.fillText('WELCOME TO', w / 2, 70);
        g.font = 'bold 120px "Brush Script MT", "Segoe Script", cursive'; g.shadowColor = 'rgba(0,0,0,0.35)'; g.shadowBlur = 8;
        g.fillText('Seabright', w / 2, 180);
        g.shadowBlur = 0; g.font = 'bold 40px Arial'; g.fillText('pop. 4,210  ·  est. 1912', w / 2, 300);
      }),
      tower: [1, 2, 3, 4, 5, 6, 7].map((n) => A.add(128, 128, (g, w, h) => {
        g.fillStyle = '#c83a34'; g.fillRect(0, 0, w, h);
        g.fillStyle = '#fff'; g.font = 'bold 96px Arial'; g.textAlign = 'center'; g.textBaseline = 'middle'; g.fillText(String(n), w / 2, h / 2 + 4);
      })),
      marina: P('SEABRIGHT MARINA', { bg: '#f6f1e6', fg: '#1d3f75', stripe: '#1d3f75' }),
      // Projecting blade signs: a word stacked letter by letter, or an icon.
      blades: [
        ['OPEN', '#c83a34', '#fff'], ['SURF', '#1f7f8c', '#fff4d8'], ['EAT', '#f2c233', '#1b1b1d'], ['BAR', '#1d3f75', '#ffcf4d'],
        ['COFFEE', '#3b2a20', '#f3dfb8'], ['GIFTS', '#f7d6e3', '#c2346c'], ['CAFE', '#2f6d4a', '#fff'], ['TOYS', '#8f6ad8', '#fff'],
      ].map(([txt, bg, fg]) => A.add(128, 224, (g, w, h) => {
        g.fillStyle = bg; g.fillRect(0, 0, w, h);
        g.strokeStyle = fg; g.lineWidth = 6; g.strokeRect(8, 8, w - 16, h - 16);
        g.fillStyle = fg; g.textAlign = 'center'; g.textBaseline = 'middle';
        const chars = txt.length > 1 && /^[A-Z]+$/.test(txt) ? txt.split('') : [txt];
        const size = chars.length > 1 ? Math.min(52, 180 / chars.length) : 80;
        g.font = `bold ${size}px "Arial Black", Arial, sans-serif`;
        chars.forEach((c, i) => g.fillText(c, w / 2, h / 2 + (i - (chars.length - 1) / 2) * size * 1.05));
      })),
    };
    this.nn = {
      vacancy: N.add(512, 160, neonSign('Vacancy', { color: '#ff3b6b', sub: 'MOTEL · COLOR TV', subColor: '#5ff2ff' })),
      diner: N.add(512, 170, neonSign('Starlite Diner', { color: '#ff5fd2', sub: 'OPEN 24 HOURS', subColor: '#ffd84d' })),
      inn: N.add(1024, 180, neonSign('Seabright Inn', { color: '#5ff2ff', font: 'bold 120px "Brush Script MT", "Segoe Script", cursive', sub: null, frame: false })),
      arcade: N.add(512, 150, neonSign('ARCADE', { color: '#ffd84d', font: 'bold 104px "Arial Black", Arial', sub: 'SKEE-BALL · PINBALL', subColor: '#ff5fd2' })),
    };
    const at = A.texture(), nt = N.texture();
    this.M.signs = new THREE.MeshStandardMaterial({ map: at, roughness: 0.6, emissive: 0xffffff, emissiveMap: at, emissiveIntensity: 0 });
    this.M.neon = new THREE.MeshStandardMaterial({ map: nt, color: 0xffffff, roughness: 0.5, emissive: 0xffffff, emissiveMap: nt, emissiveIntensity: 0.6 });
  }

  // Streets are laid first so nothing is built across them.
  reserveStreets() {
    const t = this.t;
    this.streets = [];
    for (const s of this.crossS) {
      const f = t.frame(s);
      const pts = [];
      for (let d = f.wallR + 0.8; d < 330; d += 5) {
        if (d > 300) break;
        const x = f.x + f.rx * d, z = f.z + f.rz * d;
        pts.push({ x, z });
        this.take(x, z, 7.5);
      }
      this.streets.push({ s, pts });
    }
    // Back streets parallel to the boulevard.
    this.backLats = [95, 187, 279];
    for (const lat of this.backLats) {
      for (let s = this.townS0 - 20; s < this.townS1 + 20; s += 5) {
        const p = t.pointAt(s, lat);
        this.take(p.x, p.z, 5.5);
      }
    }
  }

  // ── Sea side: boardwalk promenade ────────────────────────────────
  buildPromenade() {
    const t = this.t, B = this.B, rng = this.rng;
    const w0 = (s) => t.frame(s).wallL;
    const W = w0(this.pierS);
    this.promIn = W + 0.45;
    this.promOut = W + 6.8;
    this.stripAlong(this.townS0 - 30, this.townS1 + 30, -this.promOut, -this.promIn, this.M.deckPlank, { lift: 0.22, uScale: 6.35, vScale: 3, name: 'promenade' });
    // Sand-side edge beam and a low timber rail, open at the beach stairs.
    for (let s = this.townS0 - 30; s < this.townS1 + 30; s += 4) {
      const fr = this.frameAt(s, -this.promOut - 0.1, 'road');
      B.setFrame(fr.x, fr.y, fr.z, fr.yaw);
      B.box('woodDark', 4.05, 0.35, 0.2, 0, -0.1, 0);
      if (Math.abs(s - this.pierS) < 10 || Math.abs(s - this.marinaS) < 8 || ((s - this.townS0) % 120 + 120) % 120 < 4) continue;
      B.box('wood', 0.14, 1.0, 0.14, -2, 0.22, 0.02);
      B.box('wood', 4.05, 0.1, 0.16, 0, 1.12, 0.02);
      B.box('wood', 4.05, 0.08, 0.1, 0, 0.7, 0.02);
    }
    // Lamps, benches and bins along the boardwalk.
    for (let s = this.townS0; s < this.townS1; s += 28) {
      if (Math.abs(s - this.pierS) < 12) continue;
      let fr = this.frameAt(s, -(this.promOut - 0.6), 'away');
      B.setFrame(fr.x, fr.y + 0.22, fr.z, fr.yaw);
      promLamp(B, 0, 0);
      fr = this.frameAt(s + 14, -(this.promOut - 1.2), 'away');
      B.setFrame(fr.x, fr.y + 0.22, fr.z, fr.yaw);
      bench(B, 0, 0, 0);
      if (rng() < 0.5) trashCan(B, 1.6, 0.2);
    }
  }

  // ── Town side: storefronts along the boulevard ───────────────────
  buildFrontage() {
    const t = this.t, rng = this.rng, sg = this.sg, nn = this.nn;
    const f0 = t.frame(this.pierS);
    const W = f0.wallR;
    // Sidewalk.
    this.stripAlong(this.townS0 - 40, this.townS1 + 40, W + 0.4, W + 5.9, this.M.paving, { lift: 0.14, uScale: 4, vScale: 4, name: 'sidewalk' });
    this.frontLat = W + 6.2;

    const L = {
      shop: (rect, w = rrange(rng, 11, 14)) => ({ w, d: 16, build: (B) => shop(B, rng, { w: w - 1, d: 14, rect, blade: rng() < 0.6 ? rpick(rng, sg.blades) : null }) }),
      surf: () => ({ w: 16, d: 15, build: (B) => surfShop(B, rng, sg.surf) }),
      taco: () => ({ w: 12, d: 8, build: (B) => tacoStand(B, rng, sg.tacos) }),
      diner: () => ({ w: 26, d: 26, build: (B, fr) => { B.box('asphaltLot', 25, 0.08, 7.5, 0, 0, -3.9); B.pushFrame(0, 0, -8); diner(B, rng, nn.diner); B.popFrame(); this.lotCars(fr, -9, 9, -3.9, 4); } }),
      motel: () => ({ w: 46, d: 34, build: (B, fr) => { motel(B, rng, { w: 42, signRect: sg.motelBoard, neonRect: nn.vacancy }); this.lotCars(fr, -6, 17, -13, 5); } }),
      gas: () => ({ w: 38, d: 30, build: (B, fr) => { gasStation(B, rng, { signRect: sg.gas, priceRect: sg.price }); this.lotCars(fr, -2.4, 2.4, -8, 2, true); } }),
      hotel: () => ({ w: 40, d: 18, build: (B) => hotel(B, rng, { w: 36, rect: nn.inn }) }),
      parking: () => ({ w: 26, d: 24, build: (B, fr) => {
        B.box('asphaltLot', 24, 0.08, 22, 0, 0, -11);
        for (let x = -10.5; x <= 10.5; x += 3) B.box('paintWhite', 0.12, 0.02, 5, x, 0.09, -17);
        for (let x = -10.5; x <= 10.5; x += 3) B.box('paintWhite', 0.12, 0.02, 5, x, 0.09, -5.5);
        this.lotCars(fr, -9, 9, -17, 5); this.lotCars(fr, -9, 9, -5.5, 3);
      } }),
      house: () => ({ w: rrange(rng, 13, 16), d: 16, build: (B) => beachHouse(B, rng, { w: rrange(rng, 10, 12) }) }),
    };
    const blocks = [
      [L.surf(), L.shop(sg.icecream), L.taco(), L.shop(sg.bikes), L.shop(sg.cafe, 14), L.parking(), L.shop(sg.records), L.house()],
      [L.hotel(), L.shop(sg.swim), L.shop(sg.pizza, 15), L.parking(), L.house(), L.house()],
      [L.motel(), L.diner(), L.house(), L.shop(sg.icecream)],
      [L.gas(), L.shop(sg.pharmacy, 15), L.house(), L.house(), L.shop(sg.cafe)],
      [L.shop(sg.supply, 16), L.shop(sg.bait), L.house(), L.house(), L.house()],
      [L.house(), L.house(), L.house(), L.house()],
    ];
    const edges = [this.townS0, ...this.crossS, this.townS1];
    for (let b = 0; b < edges.length - 1; b++) {
      const a = edges[b] + (b === 0 ? 0 : 9), z = edges[b + 1] - (b === edges.length - 2 ? 0 : 9);
      const list = blocks[Math.min(b, blocks.length - 1)];
      let s = a, k = 0;
      while (true) {
        const lot = list[k] || L.house();
        k++;
        if (s + lot.w > z) break;
        this.placeLot(s + lot.w / 2, this.frontLat, lot);
        s += lot.w + rrange(rng, 0.5, 2.5);
      }
    }
  }

  placeLot(s, lat, lot) {
    const fr = this.frameAt(s, lat, 'road');
    const [cx, cz] = this.toWorld(fr, 0, -lot.d / 2);
    const r = Math.min(lot.w, lot.d) * 0.45;
    if (!this.clearOfRoad(cx, cz, r)) return false;
    this.take(cx, cz, Math.max(lot.w, lot.d) * 0.5);
    // Sit on the lowest corner so nothing floats.
    let y = Infinity;
    for (const [lx, lz] of [[-lot.w / 2, 0], [lot.w / 2, 0], [-lot.w / 2, -lot.d], [lot.w / 2, -lot.d]]) {
      const [x, z] = this.toWorld(fr, lx, lz);
      y = Math.min(y, this.gy(x, z));
    }
    fr.y = y;
    this.B.setFrame(fr.x, y, fr.z, fr.yaw);
    lot.build(this.B, fr);
    return true;
  }

  // Parked cars in a row inside a lot frame (local x0..x1 at local z).
  lotCars(fr, x0, x1, z, n, atPumps = false) {
    const rng = this.rng;
    const kinds = ['sedan', 'hatch', 'pickup', 'van', 'sedan', 'hatch'];
    const colors = [0xe8e6df, 0x2b2f36, 0x8a1c1c, 0x2e6d8e, 0xc9ccd1, 0xd9b45a, 0x4d8a7a, 0x6b4a2e];
    for (let i = 0; i < n; i++) {
      if (!atPumps && rng() < 0.3) continue;
      const lx = n === 1 ? (x0 + x1) / 2 : x0 + (x1 - x0) * (i / (n - 1));
      const [x, zw] = this.toWorld(fr, lx, z);
      const yaw = fr.yaw + (atPumps ? 0 : rng() < 0.5 ? 0 : Math.PI) + rrange(rng, -0.05, 0.05);
      this.cars.push({ x, z: zw, y: this.gy(x, zw), yaw, kind: rpick(rng, kinds), color: rpick(rng, colors), seed: Math.floor(rng() * 999) });
    }
  }

  // ── Houses behind the main street ────────────────────────────────
  buildResidential() {
    const t = this.t, rng = this.rng, B = this.B;
    // Street surfaces.
    for (const st of this.streets) this.ribbon(st.pts, 9, this.M.street, { lift: 0.14, name: 'crossStreet' });
    for (const lat of this.backLats) this.stripAlong(this.townS0 - 20, this.townS1 + 20, lat - 4, lat + 4, this.M.street, { lift: 0.13, uScale: 8, vScale: 4, name: 'backStreet' });
    const rows = [
      { lat: this.backLats[0] - 7, dir: 1, dens: 0.9 }, { lat: this.backLats[0] + 7, dir: -1, dens: 0.9 },
      { lat: this.backLats[1] - 7, dir: 1, dens: 0.8 }, { lat: this.backLats[1] + 7, dir: -1, dens: 0.75 },
      { lat: this.backLats[2] - 7, dir: 1, dens: 0.65 }, { lat: this.backLats[2] + 7, dir: -1, dens: 0.55 },
    ];
    for (const row of rows) {
      for (let s = this.townS0 + rrange(rng, 0, 8); s < this.townS1; s += rrange(rng, 14, 19)) {
        if (rng() > row.dens) continue;
        const f = t.frame(s);
        const p = t.pointAt(s, row.lat);
        // +Z (house front) toward the back street.
        const k = row.dir;
        const fr = { x: p.x, z: p.z, yaw: yawZ(k * f.rx, k * f.rz), f };
        const w = rrange(rng, 9, 12), d = rrange(rng, 10, 13);
        const [cx, cz] = this.toWorld(fr, 0, -d / 2);
        if (!this.free(cx, cz, Math.max(w, d) * 0.55)) continue;
        this.take(cx, cz, Math.max(w, d) * 0.55);
        fr.y = Math.min(this.gy(cx, cz), this.gy(p.x, p.z));
        B.setFrame(fr.x, fr.y, fr.z, fr.yaw);
        beachHouse(B, rng, { w, d, floors: rng() < 0.7 ? 2 : 3 });
        // Front lawn and hedge, sometimes a pool out back.
        B.box('lawn', w * 0.55, 0.05, 3.6, w * 0.2, 0.02, 2.0);
        B.box('concrete', w * 0.4, 0.05, 3.6, -w * 0.25, 0.02, 2.0);
        if (rng() < 0.6) B.box('hedge', w * 0.5, rrange(rng, 0.7, 1.1), 0.7, w * 0.22, 0, 4.1);
        if (rng() < 0.25) { B.box('concrete', 5.5, 0.12, 4, 0, 0, -d - 3.2); B.box('pool', 4.5, 0.04, 3, 0, 0.1, -d - 3.2); }
        // Driveway car and a palm or two in the yard.
        if (rng() < 0.16) {
          const [x, z] = this.toWorld(fr, -w * 0.22, 3.2);
          this.cars.push({ x, z, y: this.gy(x, z), yaw: fr.yaw + (rng() < 0.5 ? 0 : Math.PI), kind: rpick(rng, ['sedan', 'hatch', 'pickup', 'van']), color: rpick(rng, [0xe8e6df, 0x2b2f36, 0x8a1c1c, 0x2e6d8e, 0x9aa3ad, 0xb5a27a]), seed: Math.floor(rng() * 999) });
        }
        if (rng() < 0.55) {
          const [x, z] = this.toWorld(fr, w / 2 + 1.4, rrange(rng, -d, 1));
          this.addPalm(x, z, rng, true);
        }
      }
    }
  }

  buildHillHouses() {
    for (const h of this.hillHouses || []) {
      const rng = mulberry32(h.seed);
      const y = this.gy(h.x, h.z);
      this.B.setFrame(h.x, y, h.z, h.yaw);
      this.B.box('concrete', 14, 1.2, 15, 0, -1.1, -6);
      beachHouse(this.B, rng, { w: rrange(rng, 10, 12), d: 11, floors: 2, tile: rng() < 0.6 });
      this.take(h.x, h.z, 9);
      const [x, z] = this.toWorld({ x: h.x, z: h.z, yaw: h.yaw }, 8, -2);
      this.addPalm(x, z, rng);
    }
  }

  // ── Palms, street lights, light pools ────────────────────────────
  addPalm(x, z, rng, force = false) {
    if (!force && !this.free(x, z, 1.2)) return;
    this.take(x, z, 1.0);
    this.palms.push({ x, z, y: this.gy(x, z), h: rrange(rng, 9, 15), yaw: rng() * Math.PI * 2, lean: rrange(rng, 0.6, 1.4), s: rrange(rng, 0.85, 1.2), c: rng() });
  }

  buildStreetFurniture() {
    const t = this.t, B = this.B, rng = this.rng;
    const nearCross = (s, d) => this.crossS.some((c) => Math.abs(s - c) < d);
    for (let s = this.townS0 - 40; s < this.townS1 + 30; s += 46) {
      for (const side of [1, -1]) {
        const ss = side > 0 ? s : s + 23;
        if (nearCross(ss, 10) || (side < 0 && Math.abs(ss - this.pierS) < 12)) continue;
        const f = t.frame(ss);
        const wall = side > 0 ? f.wallR : f.wallL;
        const fr = this.frameAt(ss, side * (wall + 1.0), 'road');
        B.setFrame(fr.x, fr.y + (side < 0 ? 0.22 : 0.14), fr.z, fr.yaw);
        streetLight(B, 0, 0, 0, 3.4);
        const p = t.pointAt(ss, side * (wall + 1.0 - 3.7));
        this.pools.push({ x: p.x, z: p.z, y: p.y + 0.04 });
      }
    }
    // Palms: along the town sidewalk, the sand edge and the boulevard's
    // approach down the hill.
    for (let s = this.townS0 - 20; s < this.townS1 + 20; s += 23) {
      if (nearCross(s, 9)) continue;
      const p = t.pointAt(s + 11, t.frame(s).wallR + 3.4);
      this.addPalm(p.x, p.z, rng, true);
    }
    for (let s = this.townS0 - 20; s < this.townS1 + 20; s += 17) {
      if (Math.abs(s - this.pierS) < 14 || Math.abs(s - this.marinaS) < 10) continue;
      const p = t.pointAt(s, -(this.promOut + 2.5 + rng() * 2));
      this.addPalm(p.x, p.z, rng, true);
    }
    for (let s = this.zs0 + 60; s < this.townS0 - 20; s += 30) {
      const f = t.frame(s);
      const p = t.pointAt(s, f.wallR + rrange(rng, 4, 9));
      this.addPalm(p.x, p.z, rng);
    }
  }

  // Hydrants, bike racks, newspaper boxes and planters on the town
  // sidewalk, kept clear of the lamp posts, palms and cross streets.
  buildSidewalkProps() {
    const t = this.t, B = this.B, rng = this.rng;
    const W = t.frame(this.pierS).wallR;
    const hydrant = new THREE.CylinderGeometry(0.16, 0.2, 0.75, 8);
    const cap = new THREE.SphereGeometry(0.17, 8, 5, 0, Math.PI * 2, 0, Math.PI / 2);
    for (let s = this.townS0 + 7; s < this.townS1 - 5; s += rrange(rng, 9, 16)) {
      if (this.crossS.some((c) => Math.abs(s - c) < 9)) continue;
      const k = Math.floor(rng() * 5);
      const fr = this.frameAt(s, W + 1.3, 'road');
      B.setFrame(fr.x, fr.y + 0.14, fr.z, fr.yaw + Math.PI);
      if (k === 0) {
        B.put('paintRed', hydrant, 0, 0.37, 0);
        B.put('paintRed', cap, 0, 0.74, 0);
        B.cbox('paintRed', 0.5, 0.1, 0.1, 0, 0.5, 0);
      } else if (k === 1) {
        for (let i = 0; i < 3; i++) {
          B.box('metal', 0.05, 0.8, 0.05, -0.8 + i * 0.8 - 0.3, 0, 0);
          B.box('metal', 0.05, 0.8, 0.05, -0.8 + i * 0.8 + 0.3, 0, 0);
          B.box('metal', 0.65, 0.05, 0.05, -0.8 + i * 0.8, 0.8, 0);
        }
      } else if (k === 2) {
        B.box(rpick(rng, ['wBlue', 'paintRed', 'wYellow', 'white']), 0.5, 1.05, 0.45, -0.3, 0, 0);
        B.box(rpick(rng, ['wBlue', 'paintRed', 'wYellow', 'white']), 0.5, 1.05, 0.45, 0.3, 0, 0);
      } else if (k === 3) {
        B.box('concrete', 1.6, 0.55, 0.8, 0, 0, 0);
        B.box('hedge', 1.45, 0.45, 0.65, 0, 0.55, 0);
      } else {
        bench(B, 0, 0, 0);
      }
    }
  }

  // Cars parked along the first stretch of each cross street, where they
  // show from the boulevard. (Each car costs a couple of thousand triangles,
  // so the far ends of the streets stay empty.)
  buildStreetParking() {
    const t = this.t, rng = this.rng;
    const kinds = ['sedan', 'hatch', 'pickup', 'van', 'sedan', 'hatch'];
    const colors = [0xe8e6df, 0x2b2f36, 0x8a1c1c, 0x2e6d8e, 0xc9ccd1, 0xd9b45a, 0x4d8a7a, 0x6b4a2e, 0x9fd8cf, 0xf2c14e];
    for (const st of this.streets) {
      const f = t.frame(st.s);
      const yaw = yawZ(f.rx, f.rz);
      for (let d = f.wallR + 16; d < 42; d += 6.2) {
        for (const side of [-1, 1]) {
          if (rng() < 0.45) continue;
          const x = f.x + f.rx * d + f.fx * side * 3.4, z = f.z + f.rz * d + f.fz * side * 3.4;
          this.cars.push({ x, z, y: this.gy(x, z), yaw: yaw + (side > 0 ? 0 : Math.PI) + rrange(rng, -0.03, 0.03), kind: rpick(rng, kinds), color: rpick(rng, colors), seed: Math.floor(rng() * 999) });
        }
      }
    }
  }

  buildPalms() {
    const list = this.palms;
    if (!list.length) return;
    // Trunk: tapered, ringed, bending to +X by 1.4 m over its height.
    const trunk = new THREE.CylinderGeometry(0.17, 0.28, 1, 7, 10, true);
    trunk.translate(0, 0.5, 0);
    const p = trunk.getAttribute('position');
    for (let i = 0; i < p.count; i++) {
      const y = p.getY(i);
      const bulge = 1 + 0.07 * Math.max(0, Math.sin(y * 24 * Math.PI * 2));
      p.setXYZ(i, p.getX(i) * bulge + 1.4 * y * y, y, p.getZ(i) * bulge);
    }
    trunk.computeVertexNormals();
    const bark = canvasTex(64, 256, (g, w, h) => {
      g.fillStyle = '#8a6f52'; g.fillRect(0, 0, w, h);
      for (let y = 0; y < h; y += h / 24) {
        g.fillStyle = 'rgba(60,40,25,0.55)'; g.fillRect(0, y, w, 3);
        g.fillStyle = 'rgba(200,170,130,0.25)'; g.fillRect(0, y + 3, w, 2);
      }
    });
    const trunkMat = new THREE.MeshStandardMaterial({ map: bark, roughness: 0.95 });
    // Crown: arching fronds with feathered leaflets that hang from the
    // midrib, plus a few dead fronds drooping below as a brown skirt.
    const leaf = canvasTex(64, 256, (g, w, h) => {
      g.clearRect(0, 0, w, h);
      const rng = mulberry32(5);
      for (let y = 4; y < h - 6; y += 3) {
        const t = y / h;
        const span = (w / 2 - 1) * Math.sin(t * Math.PI * 0.92 + 0.12) * (0.8 + rng() * 0.2);
        const droop = 10 + t * 8;
        const c = 110 + rng() * 60;
        g.strokeStyle = `rgb(${c * 0.45},${c},${c * 0.32})`; g.lineWidth = 1.8;
        for (const sd of [-1, 1]) {
          if (rng() < 0.08) continue; // a torn leaflet here and there
          g.beginPath(); g.moveTo(w / 2, y); g.quadraticCurveTo(w / 2 + sd * span * 0.6, y + droop * 0.2, w / 2 + sd * span, y + droop); g.stroke();
        }
      }
      g.strokeStyle = '#6a7a3a'; g.lineWidth = 3;
      g.beginPath(); g.moveTo(w / 2, 0); g.lineTo(w / 2, h); g.stroke();
    }, { repeat: false });
    const fronds = [];
    const N = 16;
    const dead = new THREE.Color(0x9a7a4a), green = new THREE.Color(0xffffff);
    for (let i = 0; i < N; i++) {
      const isDead = i >= 13;
      const a = isDead ? (i - 13) * 2.1 + 0.4 : (i / 13) * Math.PI * 2 + (i % 2) * 0.24;
      const len = isDead ? 2.8 : i % 3 === 0 ? 3.4 : 4.4;
      const up = isDead ? -1.6 : i % 3 === 0 ? 1.3 : i % 3 === 1 ? 0.7 : 0.35;
      const g = new THREE.PlaneGeometry(1.5, 1, 1, 7);
      const q = g.getAttribute('position');
      const c = isDead ? dead : green;
      const cols = new Float32Array(q.count * 3);
      for (let k = 0; k < q.count; k++) {
        const tt = q.getY(k) + 0.5;          // 0 at base → 1 at tip
        const across = q.getX(k) * (1 - tt * 0.45);
        const r = tt * len;
        const h = up * tt * 2.2 - (isDead ? 0.6 : 2.6) * tt * tt;
        const fold = -Math.abs(across) * 0.55;  // leaflets hang below the midrib
        const ca = Math.cos(a), sa = Math.sin(a);
        const tw = across * Math.cos(tt * 1.2);
        q.setXYZ(k, ca * r - sa * tw, h + fold, sa * r + ca * tw);
        cols[k * 3] = c.r; cols[k * 3 + 1] = c.g; cols[k * 3 + 2] = c.b;
      }
      g.setAttribute('color', new THREE.BufferAttribute(cols, 3));
      g.computeVertexNormals();
      fronds.push(g);
    }
    const crown = mergeGeometries(fronds.map((g) => g.toNonIndexed()), false);
    const crownMat = new THREE.MeshStandardMaterial({ map: leaf, vertexColors: true, alphaTest: 0.4, side: THREE.DoubleSide, roughness: 0.85, color: 0xffffff });
    const nutMat = new THREE.MeshStandardMaterial({ color: 0x6a4a28, roughness: 0.8 });
    const nut = new THREE.IcosahedronGeometry(0.22, 0);
    const imT = new THREE.InstancedMesh(trunk, trunkMat, list.length);
    const imC = new THREE.InstancedMesh(crown, crownMat, list.length);
    const imN = new THREE.InstancedMesh(nut, nutMat, list.length * 3);
    const m4 = new THREE.Matrix4(), q = new THREE.Quaternion(), e = new THREE.Euler(), v = new THREE.Vector3(), sc = new THREE.Vector3(), col = new THREE.Color();
    list.forEach((pm, k) => {
      e.set(0, pm.yaw, 0); q.setFromEuler(e);
      v.set(pm.x, pm.y - 0.2, pm.z);
      sc.set(pm.s, pm.h, pm.s);
      m4.compose(v, q, sc);
      imT.setMatrixAt(k, m4);
      // Crown at the bent trunk's tip.
      const bx = 1.4 * pm.s;
      const tx = pm.x + Math.cos(pm.yaw) * bx, tz = pm.z - Math.sin(pm.yaw) * bx, ty = pm.y - 0.2 + pm.h;
      e.set(0.1, pm.yaw * 1.7, 0.05); q.setFromEuler(e);
      v.set(tx, ty, tz);
      const cs = pm.s * lerp(0.85, 1.15, pm.c);
      sc.set(cs, cs, cs);
      m4.compose(v, q, sc);
      imC.setMatrixAt(k, m4);
      col.setHSL(0.26 + pm.c * 0.06, 0.35, 0.75 + pm.c * 0.2);
      imC.setColorAt(k, col);
      for (let j = 0; j < 3; j++) {
        const a = j * 2.1 + pm.yaw;
        v.set(tx + Math.cos(a) * 0.3, ty - 0.35, tz + Math.sin(a) * 0.3);
        m4.compose(v, q.identity(), sc.set(1, 1, 1));
        imN.setMatrixAt(k * 3 + j, m4);
      }
    });
    for (const im of [imT, imC, imN]) {
      im.castShadow = im !== imN;
      im.receiveShadow = true;
      im.computeBoundingSphere();
      im.name = 'beach:palms';
      this.group.add(im);
    }
  }

  mergeList(geos) {
    const B = new Builder();
    for (const g of geos) B.add('x', g);
    return B.mergeAll();
  }

  // Additive glow on the road under each street light.
  buildPools() {
    if (!this.pools.length) return;
    const pos = [], uv = [], idx = [];
    const R = 7;
    this.pools.forEach((p, k) => {
      const b = k * 4;
      pos.push(p.x - R, p.y, p.z - R, p.x + R, p.y, p.z - R, p.x - R, p.y, p.z + R, p.x + R, p.y, p.z + R);
      uv.push(0, 0, 1, 0, 0, 1, 1, 1);
      idx.push(b, b + 2, b + 1, b + 1, b + 2, b + 3);
    });
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
    g.setAttribute('uv', new THREE.Float32BufferAttribute(uv, 2));
    g.setIndex(idx);
    g.computeBoundingSphere();
    this.poolMat = new THREE.MeshBasicMaterial({ map: glowTexture(), color: 0xffd9a0, transparent: true, opacity: 0, blending: THREE.AdditiveBlending, depthWrite: false, polygonOffset: true, polygonOffsetFactor: -6, polygonOffsetUnits: -6 });
    const m = new THREE.Mesh(g, this.poolMat);
    m.name = 'beach:lightPools';
    m.renderOrder = 2;
    this.group.add(m);
  }

  // Parked cars: built from the vehicle kit, then merged by material so the
  // whole town's cars cost a dozen or so draw calls.
  buildCars() {
    if (!this.cars.length) return;
    const B = new Builder();
    const mats = {};
    const I = new THREE.Matrix4();
    for (const c of this.cars) {
      const m = buildVehicle(c.kind, { color: c.color, seed: c.seed, lod: 'low' });
      m.root.position.set(c.x, c.y, c.z);
      m.root.rotation.set(0, c.yaw, 0);
      m.root.updateMatrixWorld(true);
      m.root.traverse((o) => {
        if (!o.isMesh) return;
        const mt = o.material;
        const sig = [mt.type, mt.color?.getHexString(), mt.emissive?.getHexString(), mt.emissiveIntensity, mt.roughness, mt.metalness, mt.map?.uuid, mt.transparent, mt.opacity].join('|');
        if (!mats[sig]) mats[sig] = mt;
        B.setFrame(0, 0, 0, 0);
        B.add(sig, o.geometry, o.matrixWorld);
      });
    }
    for (const mesh of B.build(mats, { castShadow: true })) {
      mesh.name = 'beach:cars';
      this.group.add(mesh);
    }
  }

  // ── The beach ───────────────────────────────────────────────────
  waterlineLat(s) {
    // Search seaward for where the rendered ground meets the sea.
    const t = this.t;
    let lo = -(this.promOut + 5), hi = -260;
    let p = t.pointAt(s, hi);
    if (this.gy(p.x, p.z) > 0.1) return null;
    for (let k = 0; k < 18; k++) {
      const m = (lo + hi) / 2;
      p = t.pointAt(s, m);
      if (this.gy(p.x, p.z) > 0.1) lo = m; else hi = m;
    }
    return (lo + hi) / 2;
  }

  buildBeachProps() {
    const t = this.t, B = this.B, rng = this.rng;
    const busy = (s) => Math.abs(s - this.pierS) < 22 || Math.abs(s - this.marinaS) < 75;
    // Lifeguard towers.
    let n = 0;
    for (let s = this.townS0 + 60; s < this.townS1 - 40; s += 215) {
      if (busy(s)) s += 50;
      const wl = this.waterlineLat(s) ?? -100;
      const fr = this.frameAt(s, lerp(-this.promOut, wl, 0.55), 'away');
      if (!this.free(fr.x, fr.z, 4)) continue;
      this.take(fr.x, fr.z, 5);
      B.setFrame(fr.x, fr.y, fr.z, fr.yaw);
      lifeguardTower(B, rng, this.sg.tower[n++ % 7]);
      B.pushFrame(3.5, 0, -1.5, 0.3);
      surfboardsInSand(B, rng, 2 + Math.floor(rng() * 3));
      B.popFrame();
    }
    // Umbrella clusters.
    const fabrics = ['awnRed', 'awnBlue', 'awnYellow', 'awnGreen'];
    for (let s = this.townS0 + 20; s < this.townS1 - 20; s += rrange(rng, 35, 70)) {
      if (busy(s)) continue;
      const wl = this.waterlineLat(s) ?? -100;
      const cols = 2 + Math.floor(rng() * 3), rows = 1 + Math.floor(rng() * 2);
      const lat0 = lerp(-this.promOut, wl, rrange(rng, 0.2, 0.35));
      const fabric = rpick(rng, fabrics);
      for (let i = 0; i < cols; i++) for (let j = 0; j < rows; j++) {
        const fr = this.frameAt(s + i * 5.5, lat0 - j * 6, 'away');
        if (!this.free(fr.x, fr.z, 2)) continue;
        this.take(fr.x, fr.z, 2.2);
        B.setFrame(fr.x, fr.y, fr.z, fr.yaw);
        umbrellaSet(B, rng, rng() < 0.7 ? fabric : rpick(rng, fabrics));
      }
    }
    // Volleyball courts.
    for (const s of [this.townS0 + 150, this.pierS + 140, this.pierS + 400, this.marinaS - 170]) {
      if (s > this.townS1 || busy(s)) continue;
      const wl = this.waterlineLat(s) ?? -100;
      const fr = this.frameAt(s, lerp(-this.promOut, wl, 0.4), 'away');
      if (!this.free(fr.x, fr.z, 6)) continue;
      this.take(fr.x, fr.z, 7);
      // Net parallel to the shore → rotate so X runs along the beach.
      B.setFrame(fr.x, fr.y, fr.z, fr.yaw + Math.PI / 2);
      volleyballNet(B);
    }
  }

  // ── The pier ────────────────────────────────────────────────────
  buildPier() {
    const t = this.t, B = this.B, rng = this.rng;
    const s = this.pierS;
    const fr = this.frameAt(s, -this.promOut, 'away');
    const deckY = t.surfaceY(s, 0) + 0.25;
    fr.y = deckY;
    const L = 240, W = 12, PW = 46, PL = 90;
    this.pier = { fr, L, W };
    // Keep the beach clear of the pier.
    for (let z = 0; z < L; z += 8) { const [x, zz] = this.toWorld(fr, 0, z); this.take(x, zz, z > L - PL ? 26 : 9); }
    B.setFrame(fr.x, deckY, fr.z, fr.yaw);
    // Deck: main walk + the wide platform at the end.
    B.box('wood', W, 0.35, L - PL, 0, -0.35, (L - PL) / 2);
    B.box('wood', PW, 0.35, PL, 0, -0.35, L - PL / 2);
    B.box('woodDark', W + 0.3, 0.6, L - PL, 0, -0.9, (L - PL) / 2);
    B.box('woodDark', PW + 0.3, 0.6, PL, 0, -0.9, L - PL / 2);
    // Piles down to the sea floor.
    const pile = new THREE.CylinderGeometry(0.28, 0.32, 1, 8);
    const pileAt = (lx, lz) => {
      const [x, z] = this.toWorld(fr, lx, lz);
      const g = Math.min(this.gy(x, z), 0) - 1;
      const h = deckY - 0.9 - g;
      if (h > 0.3) B.put('woodDark', pile, lx, -0.9 - h / 2, lz, 0, 0, 0, 1, h, 1);
    };
    for (let z = 6; z < L - PL; z += 8) for (const x of [-W / 2 + 0.6, 0, W / 2 - 0.6]) pileAt(x, z);
    // Cross bracing between the piles, down to just above the water.
    for (let z = 6; z < L - PL; z += 8) {
      const [x, zz] = this.toWorld(fr, 0, z);
      const drop = Math.min(4.5, deckY - 0.9 - Math.max(0.6, Math.min(this.gy(x, zz), 0) + 0.6));
      if (drop < 1) continue;
      B.beam('woodDark', new THREE.Vector3(-W / 2 + 0.6, -0.9 - drop, z), new THREE.Vector3(0, -1.0, z), 0.16);
      B.beam('woodDark', new THREE.Vector3(W / 2 - 0.6, -0.9 - drop, z), new THREE.Vector3(0, -1.0, z), 0.16);
      B.cbox('woodDark', W - 1, 0.2, 0.14, 0, -0.9 - drop * 0.6, z);
    }
    for (let z = L - PL + 4; z < L; z += 9) for (let x = -PW / 2 + 1; x <= PW / 2 - 1; x += 9) pileAt(x, z);
    // Railings.
    const rail = (x0, z0, x1, z1) => {
      const len = Math.hypot(x1 - x0, z1 - z0);
      const ry = Math.atan2(x1 - x0, z1 - z0);
      B.cbox('white', 0.1, 0.1, len, (x0 + x1) / 2, 1.05, (z0 + z1) / 2, 0, ry, 0);
      B.cbox('white', 0.06, 0.06, len, (x0 + x1) / 2, 0.55, (z0 + z1) / 2, 0, ry, 0);
      const n = Math.floor(len / 2.4);
      for (let i = 0; i <= n; i++) {
        const u = i / n;
        B.box('white', 0.1, 1.05, 0.1, x0 + (x1 - x0) * u, 0, z0 + (z1 - z0) * u);
      }
    };
    rail(-W / 2, 0, -W / 2, L - PL); rail(W / 2, 0, W / 2, L - PL);
    rail(-PW / 2, L - PL, -W / 2, L - PL); rail(W / 2, L - PL, PW / 2, L - PL);
    rail(-PW / 2, L - PL, -PW / 2, L); rail(PW / 2, L - PL, PW / 2, L); rail(-PW / 2, L, PW / 2, L);
    // Lamps and benches along the walk.
    for (let z = 12; z < L - 4; z += 22) {
      for (const x of [-W / 2 + 0.5, W / 2 - 0.5]) {
        if (z > L - PL && Math.abs(x) < PW / 2 - 2) continue;
        promLamp(B, x, z);
      }
      if (z < L - PL - 10) { bench(B, -W / 2 + 1.3, z + 8, Math.PI / 2); bench(B, W / 2 - 1.3, z + 8, -Math.PI / 2); }
    }
    // Entrance arch over the start of the pier.
    for (const x of [-W / 2 - 0.2, W / 2 + 0.2]) B.box('white', 0.8, 7.2, 0.8, x, 0, 1.2);
    B.box('white', W + 2.4, 1.9, 0.5, 0, 6.2, 1.2);
    B.put('signs', signGeometry(this.sg.pier, W + 1.6, 1.6), 0, 7.15, 0.92, 0, Math.PI, 0);
    B.put('signs', signGeometry(this.sg.pier, W + 1.6, 1.6), 0, 7.15, 1.48, 0, 0, 0);
    for (let k = 0; k < 9; k++) B.put('lampGlow', new THREE.SphereGeometry(0.14, 6, 5), -W / 2 - 0.2 + k * (W + 0.4) / 8, 8.25, 1.2);
    // Bait shop halfway out, with rods leaning on the rail.
    B.box('wBlue', 3.2, 2.7, 6, -W / 2 + 2.2, 0, 70);
    B.cbox('roofTar', 3.8, 0.16, 6.6, -W / 2 + 2.2, 2.85, 70, 0, 0, -0.08);
    B.cbox('glass', 0.08, 1.1, 2.4, -W / 2 + 3.82, 1.6, 70);
    B.put('signs', signGeometry(this.sg.bait, 3.6, 0.9), -W / 2 + 3.86, 2.35, 70, 0, Math.PI / 2, 0);
    for (let k = 0; k < 4; k++) B.beam('black', new THREE.Vector3(W / 2 - 0.3, 0.1, 40 + k * 23), new THREE.Vector3(W / 2 + 1.6, 3.2, 40 + k * 23 + 0.6), 0.03);
    // Arcade near the end of the walk.
    B.box('wLilac', 5.5, 4.2, 16, W / 2 - 3, 0, L - PL - 10);
    B.box('trim', 5.9, 0.4, 16.4, W / 2 - 3, 4.2, L - PL - 10);
    B.cbox('glassLit', 0.1, 2, 12, W / 2 - 5.78, 1.6, L - PL - 10);
    B.box('black', 0.3, 1.9, 6.4, W / 2 - 5.9, 4.4, L - PL - 10);
    B.put('neon', signGeometry(this.nn.arcade, 6, 1.75), W / 2 - 6.08, 5.35, L - PL - 10, 0, -Math.PI / 2, 0);
    // Snack kiosk and tables on the platform.
    B.box('wYellow', 5, 3, 4, 3, 0, L - 9);
    B.box('awnRed', 5.6, 0.3, 4.6, 3, 3, L - 9);
    for (const [x, z] of [[-1.5, L - 16], [4, L - 18], [9.5, L - 15], [9, L - 6]]) picnicTable(B, x, z);

    this.buildFerris(fr, deckY, 12, L - 45);
    this.buildCoaster(fr, deckY, L, PL);
  }

  buildFerris(fr, deckY, lx, lz) {
    const B = new Builder();
    const R = 15, AX = 20;
    // Rotating part in its own frame: axle along local X, wheel in the YZ plane.
    const ring = new THREE.TorusGeometry(R, 0.22, 6, 64);
    ring.rotateY(Math.PI / 2);
    for (const x of [-1.4, 1.4]) B.put('w', ring, x, 0, 0);
    const ring2 = new THREE.TorusGeometry(R * 0.55, 0.12, 5, 40);
    ring2.rotateY(Math.PI / 2);
    for (const x of [-1.4, 1.4]) B.put('w', ring2, x, 0, 0);
    const spokes = 16;
    for (let i = 0; i < spokes; i++) {
      const a = (i / spokes) * Math.PI * 2;
      const y = Math.sin(a) * R, z = Math.cos(a) * R;
      for (const x of [-1.4, 1.4]) B.beam('w', new THREE.Vector3(x * 0.4, 0, 0), new THREE.Vector3(x, y, z), 0.1);
      B.beam('w', new THREE.Vector3(-1.4, y, z), new THREE.Vector3(1.4, y, z), 0.12);
    }
    B.put('w', new THREE.CylinderGeometry(0.6, 0.6, 3.4, 12), 0, 0, 0, 0, 0, Math.PI / 2);
    const wheelGeo = B.mergeAll();
    const wheelMat = new THREE.MeshStandardMaterial({ color: 0xf4f1ea, roughness: 0.5, metalness: 0.3 });
    const pivot = new THREE.Group();
    pivot.position.set(fr.x, deckY + AX, fr.z);
    pivot.rotation.y = fr.yaw;
    const [wx, wz] = this.toWorld(fr, lx, lz);
    pivot.position.set(wx, deckY + AX, wz);
    const wheel = new THREE.Group();
    const wm = new THREE.Mesh(wheelGeo, wheelMat);
    wm.castShadow = true;
    wheel.add(wm);
    // Bulbs on both rims.
    this.bulbMat = new THREE.MeshStandardMaterial({ color: 0xfff2d0, emissive: 0xffc870, emissiveIntensity: 0.6 });
    const nb = 48;
    const bulbs = new THREE.InstancedMesh(new THREE.SphereGeometry(0.2, 6, 4), this.bulbMat, nb * 2);
    const m4 = new THREE.Matrix4();
    for (let i = 0; i < nb; i++) {
      const a = (i / nb) * Math.PI * 2;
      for (const [j, x] of [[0, -1.5], [1, 1.5]]) {
        m4.makeTranslation(x, Math.sin(a) * (R + 0.35), Math.cos(a) * (R + 0.35));
        bulbs.setMatrixAt(i * 2 + j, m4);
      }
    }
    bulbs.computeBoundingSphere();
    wheel.add(bulbs);
    pivot.add(wheel);
    // Static A-frame legs.
    const legs = new Builder();
    for (const x of [-3.2, 3.2]) for (const z of [-7.5, 7.5]) legs.beam('l', new THREE.Vector3(x, -AX, z), new THREE.Vector3(x * 0.5, 0, 0), 0.45);
    legs.beam('l', new THREE.Vector3(-3.2, -AX + 0.2, -7.5), new THREE.Vector3(-3.2, -AX + 0.2, 7.5), 0.35);
    legs.beam('l', new THREE.Vector3(3.2, -AX + 0.2, -7.5), new THREE.Vector3(3.2, -AX + 0.2, 7.5), 0.35);
    const legMesh = new THREE.Mesh(legs.mergeAll(), wheelMat);
    legMesh.castShadow = true;
    pivot.add(legMesh);
    // Gondolas hang level: an instanced mesh updated each frame.
    const gb = new Builder();
    gb.cbox('g', 1.7, 1.3, 1.5, 0, -1.9, 0);
    gb.cbox('g', 1.9, 0.12, 1.7, 0, -1.2, 0);
    gb.beam('g', new THREE.Vector3(0, -1.2, 0), new THREE.Vector3(0, 0, 0), 0.08);
    const gMat = new THREE.MeshStandardMaterial({ color: 0xffffff, roughness: 0.5 });
    const gondolas = new THREE.InstancedMesh(gb.mergeAll(), gMat, spokes);
    const col = new THREE.Color();
    const pal = [0xe24d6c, 0x2fa7c9, 0xf2c233, 0x3f9e6a, 0xff8a3c, 0x8f6ad8];
    for (let i = 0; i < spokes; i++) gondolas.setColorAt(i, col.setHex(pal[i % pal.length]));
    gondolas.frustumCulled = false;
    pivot.add(gondolas);
    pivot.name = 'beach:ferris';
    this.group.add(pivot);
    this.ferris = { wheel, gondolas, R, n: spokes, angle: 0 };
    this.updateGondolas();
  }

  updateGondolas() {
    const F = this.ferris, m4 = new THREE.Matrix4();
    for (let i = 0; i < F.n; i++) {
      const a = (i / F.n) * Math.PI * 2 + F.angle;
      m4.makeTranslation(0, Math.sin(a) * F.R, Math.cos(a) * F.R);
      F.gondolas.setMatrixAt(i, m4);
    }
    F.gondolas.instanceMatrix.needsUpdate = true;
  }

  // A small wild-mouse coaster at the end of the pier with a train running.
  buildCoaster(fr, deckY, L, PL) {
    const base = [
      [-20, 1.5, L - 84], [-8, 3, L - 86], [-5, 9, L - 78], [-6, 13, L - 66], [-10, 13.5, L - 56],
      [-19, 9, L - 50], [-20, 4, L - 40], [-12, 6, L - 30], [-6, 10, L - 20], [-12, 12, L - 10],
      [-20, 7, L - 12], [-21, 3, L - 26], [-21, 2, L - 60], [-21, 1.6, L - 76],
    ];
    const pts = base.map(([x, y, z]) => new THREE.Vector3(x, y, z));
    const curve = new THREE.CatmullRomCurve3(pts, true, 'centripetal');
    const N = 200;
    const left = [], right = [], sleepers = new Builder(), supports = new Builder();
    for (let i = 0; i < N; i++) {
      const u = i / N;
      const p = curve.getPointAt(u), tg = curve.getTangentAt(u);
      const side = new THREE.Vector3(-tg.z, 0, tg.x).normalize();
      left.push(p.clone().addScaledVector(side, 0.55));
      right.push(p.clone().addScaledVector(side, -0.55));
      if (i % 2 === 0) {
        const q = new THREE.Quaternion().setFromUnitVectors(new THREE.Vector3(1, 0, 0), side);
        const m = new THREE.Matrix4().compose(p.clone().setY(p.y - 0.12), q, new THREE.Vector3(1.4, 0.1, 0.2));
        sleepers.add('s', new THREE.BoxGeometry(1, 1, 1), m);
      }
      if (i % 6 === 0 && p.y > 1) supports.beam('s', new THREE.Vector3(p.x, 0, p.z), new THREE.Vector3(p.x, p.y - 0.15, p.z), 0.22);
    }
    const railGeo = new Builder();
    for (const arr of [left, right]) railGeo.add('r', new THREE.TubeGeometry(new THREE.CatmullRomCurve3(arr, true), 400, 0.09, 5, true));
    const grp = new THREE.Group();
    grp.position.set(fr.x, deckY, fr.z);
    grp.rotation.y = fr.yaw;
    const railMat = new THREE.MeshStandardMaterial({ color: 0xd8433f, metalness: 0.5, roughness: 0.4 });
    const supMat = new THREE.MeshStandardMaterial({ color: 0xf2f2ee, roughness: 0.6 });
    const rails = new THREE.Mesh(railGeo.mergeAll(), railMat);
    const sup = new THREE.Mesh(this.mergeList([sleepers.mergeAll(), supports.mergeAll()]), supMat);
    rails.castShadow = sup.castShadow = true;
    grp.add(rails, sup);
    // Train of three cars.
    const cb = new Builder();
    cb.cbox('c', 1.2, 0.7, 1.6, 0, 0.45, 0);
    cb.cbox('c', 1.25, 0.25, 0.3, 0, 0.9, 0.6);
    const train = new THREE.InstancedMesh(cb.mergeAll(), new THREE.MeshStandardMaterial({ color: 0x2fa7c9, metalness: 0.3, roughness: 0.4 }), 3);
    train.frustumCulled = false;
    grp.add(train);
    grp.name = 'beach:coaster';
    this.group.add(grp);
    this.coaster = { curve, train, u: 0, v: 0.02, len: curve.getLength() };
  }

  // ── Marina ──────────────────────────────────────────────────────
  buildMarina() {
    const t = this.t, B = this.B, rng = this.rng;
    const s = this.marinaS;
    const wl = this.waterlineLat(s) ?? -110;
    const fr = this.frameAt(s, wl + 2, 'away');
    fr.y = 0;
    const DY = 0.75, L = 110;
    this.marina = fr;
    for (let z = -4; z < L + 20; z += 8) { const [x, zz] = this.toWorld(fr, 0, z); this.take(x, zz, 30); }
    B.setFrame(fr.x, 0, fr.z, fr.yaw);
    // Main dock and finger piers on floats.
    B.box('wood', 3.2, 0.3, L, 0, DY - 0.3, L / 2 - 4);
    B.box('white', 3.0, 0.35, L, 0, DY - 0.65, L / 2 - 4);
    const boatsB = this.B;
    let k = 0;
    for (let z = 10; z < L - 4; z += 11) {
      for (const side of [-1, 1]) {
        B.box('wood', 12, 0.25, 1.4, side * 7.6, DY - 0.25, z);
        B.box('white', 11.8, 0.3, 1.2, side * 7.6, DY - 0.55, z);
        for (const px of [3.5, 13]) B.box('woodDark', 0.3, 2.2, 0.3, side * px, DY - 1.4, z + 0.8);
        // A boat in most slips, bow toward the main dock.
        if (rng() < 0.8) {
          const bx = side * 8, bz = z + 5.5;
          B.pushFrame(bx, 0.05, bz, side > 0 ? -Math.PI / 2 : Math.PI / 2);
          if (rng() < 0.7) sailboat(boatsB, rng); else motorboat(boatsB, rng);
          B.popFrame();
          k++;
        }
      }
    }
    // Gangway down from the promenade across the sand.
    const p0 = t.pointAt(s, -this.promOut + 0.3);
    const [ex, ez] = this.toWorld(fr, 0, -3);
    const y0 = this.gy(p0.x, p0.z) + 0.25;
    const path = [];
    for (let i = 0; i <= 20; i++) { const u = i / 20; path.push({ x: lerp(p0.x, ex, u), z: lerp(p0.z, ez, u) }); }
    this.ribbon(path, 2.6, this.M.deckPlank, { lift: 0, name: 'gangway', yFn: (i) => lerp(y0, DY, i / 20) });
    // Harbour-master hut at the dock head, and a sign.
    B.setFrame(fr.x, 0, fr.z, fr.yaw);
    B.box('wBlue', 4, 2.8, 3.5, 5.5, DY, 3);
    B.box('white', 4.6, 0.3, 4.1, 5.5, DY + 2.8, 3);
    B.box('white', 0.25, 4.2, 0.25, -3, DY - 0.2, 0);
    B.box('white', 0.25, 4.2, 0.25, 3, DY - 0.2, 0);
    B.box('white', 6.6, 1.4, 0.25, 0, DY + 3.9, 0);
    B.put('signs', signGeometry(this.sg.marina, 6.2, 1.2), 0, DY + 4.6, -0.14, 0, Math.PI, 0);
    // Breakwater of rocks enclosing the slips.
    const rocks = [];
    const addRock = (lx, lz) => {
      const [x, z] = this.toWorld(fr, lx, lz);
      rocks.push({ x, z, y: Math.min(this.gy(x, z), 0) + rrange(rng, -0.6, 0.2), s: rrange(rng, 1.6, 2.8), r: rng() * 6 });
    };
    for (let z = 20; z < L + 16; z += 2.6) { addRock(-30, z); addRock(30, z); }
    for (let x = -30; x < -8; x += 2.6) addRock(x, L + 16);
    for (let x = 8; x <= 30; x += 2.6) addRock(x, L + 16);
    const rg = new THREE.IcosahedronGeometry(1, 0);
    const im = new THREE.InstancedMesh(rg, new THREE.MeshStandardMaterial({ color: 0x7c756c, roughness: 0.95, flatShading: true }), rocks.length);
    const m4 = new THREE.Matrix4(), q = new THREE.Quaternion(), e = new THREE.Euler();
    rocks.forEach((r, i) => {
      e.set(r.r, r.r * 1.3, 0); q.setFromEuler(e);
      m4.compose(new THREE.Vector3(r.x, r.y, r.z), q, new THREE.Vector3(r.s, r.s * 0.75, r.s));
      im.setMatrixAt(i, m4);
    });
    im.computeBoundingSphere();
    im.castShadow = true;
    im.name = 'beach:breakwater';
    this.group.add(im);
    // Marina parking on the promenade side.
    this.boatCount = k;
  }

  // ── Traffic lights and crosswalk at the pier intersection ────────
  buildTrafficLights() {
    const t = this.t, B = this.B;
    const s = this.pierS;
    // Crosswalk stripes on both sides of the intersection.
    for (const ds of [-7, 7]) {
      const f = t.frame(s + ds);
      for (let lat = -f.hw + 0.6; lat < f.hw - 0.3; lat += 1.2) {
        const p = t.pointAt(s + ds, lat);
        B.setFrame(p.x, p.y + 0.02, p.z, yawZ(f.fx, f.fz));
        B.box('paintWhite', 0.6, 0.02, 3, 0, 0, 0);
      }
      // Stop line.
      const sl = s + ds * 1.55;
      const g = t.frame(sl);
      const lat0 = ds < 0 ? 0.3 : -g.hw + 0.3, lat1 = ds < 0 ? g.hw - 0.3 : -0.3;
      const p = t.pointAt(sl, (lat0 + lat1) / 2);
      B.setFrame(p.x, p.y + 0.02, p.z, yawZ(g.fx, g.fz));
      B.box('paintWhite', lat1 - lat0, 0.02, 0.45, 0, 0, 0);
    }
    // Mast arms: ours on the far right corner, oncoming on their far right.
    const mast = (ss, side) => {
      const f = t.frame(ss);
      const wall = side > 0 ? f.wallR : f.wallL;
      const fr = this.frameAt(ss, side * (wall + 1.1), 'road');
      B.setFrame(fr.x, fr.y + 0.2, fr.z, fr.yaw);
      const reach = wall + 1.1 - 2.2;
      B.box('metal', 0.36, 8.3, 0.36, 0, 0, 0);
      B.cbox('metal', 0.22, 0.22, reach, 0, 8.0, reach / 2);
      // Signal heads face traffic coming toward them (−s for ours, +s for oncoming).
      const faceYaw = side > 0 ? Math.PI / 2 : -Math.PI / 2;
      for (const z of [reach * 0.45, reach * 0.95]) {
        B.pushFrame(0, 0, z, 0);
        B.box('black', 0.45, 1.25, 0.4, 0, 6.6, 0);
        B.box('metal', 0.08, 0.35, 0.08, 0, 7.85, 0);
        const lens = new THREE.CylinderGeometry(0.13, 0.13, 0.06, 10);
        lens.rotateX(Math.PI / 2);
        const dx = 0.23;
        B.put('sigRed', lens, dx, 7.55, 0, 0, faceYaw, 0);
        B.put('sigAmber', lens, dx, 7.22, 0, 0, faceYaw, 0);
        B.put('sigGreen', lens, dx, 6.89, 0, 0, faceYaw, 0);
        B.popFrame();
      }
    };
    mast(s + 11, 1);
    mast(s - 11, -1);
    this.signalT = 0;
  }

  buildTownSign() {
    const t = this.t, B = this.B;
    const s = Math.max(this.zs0 + 80, this.townS0 - 110);
    const f = t.frame(s);
    const p = t.pointAt(s, f.wallR + 5);
    // Face drivers arriving down the hill (−s direction).
    const y = this.gy(p.x, p.z);
    B.setFrame(p.x, y, p.z, yawZ(-f.fx, -f.fz));
    B.box('concrete', 9, 0.6, 1.6, 0, -0.3, 0);
    for (const x of [-3.6, 3.6]) B.box('woodDark', 0.35, 3.2, 0.35, x, 0, 0);
    B.box('white', 7.6, 3.9, 0.3, 0, 2.6, 0);
    B.put('signs', signGeometry(this.sg.welcome, 7.2, 3.6), 0, 4.55, 0.16);
    // Surfboard leaning on the sign.
    B.put('boardB', new THREE.CylinderGeometry(1, 1, 1, 14), 4.6, 2.2, 0.4, 0, 0, -0.18, 0.3, 4.6, 0.05);
    this.take(p.x, p.z, 5);
    for (const dx of [-7, 7]) {
      const q = t.pointAt(s + dx, f.wallR + 6);
      this.addPalm(q.x, q.z, this.rng, true);
    }
  }

  // ── Surf: foam lines washing up the beach ────────────────────────
  buildFoam() {
    const t = this.t;
    const foam = canvasTex(256, 64, (g, w, h) => {
      g.clearRect(0, 0, w, h);
      const rng = mulberry32(77);
      for (let i = 0; i < 260; i++) {
        const x = rng() * w, y = (0.25 + 0.5 * Math.pow(rng(), 2)) * h;
        g.fillStyle = `rgba(255,255,255,${0.25 + rng() * 0.5})`;
        g.beginPath(); g.ellipse(x, y, 4 + rng() * 14, 1.5 + rng() * 3, 0, 0, 7); g.fill();
      }
      const grd = g.createLinearGradient(0, 0, 0, h);
      grd.addColorStop(0, 'rgba(255,255,255,0)'); grd.addColorStop(0.35, 'rgba(255,255,255,0.8)'); grd.addColorStop(0.5, 'rgba(255,255,255,0.35)'); grd.addColorStop(1, 'rgba(255,255,255,0)');
      g.fillStyle = grd; g.fillRect(0, 0, w, h);
    });
    const mats = [];
    for (let layer = 0; layer < 2; layer++) {
      const pos = [], uv = [], idx = [];
      let rows = 0;
      for (let s = this.townS0 - 60; s < this.townS1 + 60; s += 6) {
        if (Math.abs(s - this.marinaS) < 60) { if (rows > 1) this.foamMesh(pos, uv, idx, rows, foam, mats, layer); pos.length = uv.length = idx.length = 0; rows = 0; continue; }
        const wl = this.waterlineLat(s);
        if (wl === null) continue;
        const off = layer * 9;
        for (const [lat, v] of [[wl + 3 - off, 0], [wl - 7 - off, 1]]) {
          const p = t.pointAt(s, lat);
          pos.push(p.x, 0.05 + layer * 0.01, p.z);
          uv.push(s / 40, v);
        }
        if (rows > 0) { const a = (rows - 1) * 2; idx.push(a, a + 2, a + 1, a + 1, a + 2, a + 3); }
        rows++;
      }
      if (rows > 1) this.foamMesh(pos, uv, idx, rows, foam, mats, layer);
    }
    this.foamMats = mats;
  }

  foamMesh(pos, uv, idx, rows, tex, mats, layer) {
    let m = mats[layer];
    if (!m) {
      const t2 = tex.clone();
      t2.needsUpdate = true;
      t2.wrapS = t2.wrapT = THREE.RepeatWrapping;
      m = mats[layer] = new THREE.MeshLambertMaterial({ map: t2, transparent: true, depthWrite: false, opacity: layer ? 0.55 : 0.85, polygonOffset: true, polygonOffsetFactor: -3, polygonOffsetUnits: -3 });
    }
    this.addRibbonMesh(pos.slice(), uv.slice(), idx.slice(), m, 'foam');
  }

  // ── Per-frame animation ─────────────────────────────────────────
  animate(dt, night) {
    this.time = (this.time || 0) + dt;
    if (this.ferris) {
      this.ferris.angle += dt * 0.06;
      this.ferris.wheel.rotation.x = this.ferris.angle;
      this.updateGondolas();
    }
    if (this.coaster) {
      const C = this.coaster;
      // Speed from height: slow up the lift, fast down the drops.
      const p = C.curve.getPointAt(C.u);
      const v = Math.max(3, Math.sqrt(Math.max(0, 2 * 9.81 * (14.5 - p.y))) + 3);
      C.u = (C.u + (v * dt) / C.len) % 1;
      const m4 = new THREE.Matrix4(), q = new THREE.Quaternion(), zAxis = new THREE.Vector3(0, 0, 1);
      for (let i = 0; i < 3; i++) {
        const u = (C.u - i * (2.0 / C.len) + 1) % 1;
        const pos = C.curve.getPointAt(u), tg = C.curve.getTangentAt(u);
        q.setFromUnitVectors(zAxis, tg);
        m4.compose(pos, q, new THREE.Vector3(1, 1, 1));
        C.train.setMatrixAt(i, m4);
      }
      C.train.instanceMatrix.needsUpdate = true;
    }
    // Waves washing in and out.
    if (this.foamMats) {
      this.foamMats.forEach((m, i) => {
        const ph = this.time * (0.35 + i * 0.1) + i * 1.7;
        m.map.offset.set(this.time * 0.01 * (i ? -1 : 1), Math.sin(ph) * 0.18);
        m.opacity = (i ? 0.45 : 0.75) + 0.2 * Math.sin(ph + 1);
      });
    }
    if (this.poolMat) this.poolMat.opacity = 0.42 * smoothstep(0.08, 0.5, night);
    // Traffic lights: green 9 s, amber 2.5 s, red 7 s (visual only).
    this.signalT = (this.signalT + dt) % 18.5;
    const ph = this.signalT;
    const on = 2.4 + night * 1.5;
    this.M.sigGreen.emissiveIntensity = ph < 9 ? on : 0.02;
    this.M.sigAmber.emissiveIntensity = ph >= 9 && ph < 11.5 ? on : 0.02;
    this.M.sigRed.emissiveIntensity = ph >= 11.5 ? on : 0.02;
  }
}
