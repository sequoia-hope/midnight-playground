import * as THREE from 'three';
import { mergeGeometries } from 'three/addons/utils/BufferGeometryUtils.js';
import { clamp, lerp, smoothstep, mulberry32, rrange, rpick, hash2 } from '../util/math.js';
import { glowTexture } from './textures.js';
import { bannerTexture } from './city/cityTextures.js';
import { makeGround } from './valley/ground.js';
import { Builder } from './valley/Builder.js';
import { poleGeometry } from './valley/parts.js';
import { ColorBuilder } from './beach/ColorBuilder.js';
import { SignAtlas, paintedSign, neonSign, signGeometry, roundRect } from './beach/atlas.js';
import { motel, gasStation, diner } from './beach/parts.js';
import { buildVehicle } from '../vehicles/CarModel.js';
import {
  paint, prep, sandstoneMaterial, joshuaGeometry, saguaroGeometry,
  bushGeometry, juniperGeometry, yuccaGeometry, palmGeometry, coneGeometry,
  locomotiveGeometry, boxcarGeometry, tankGeometry, hopperGeometry, stackGeometry,
  gondolaGeometry, autorackGeometry, lumberGeometry,
} from './desert/parts.js';
import {
  rockGeometry, hoodooGeometry, archGeometry, butteGeometry, pinyonGeometry, sageGeometry,
  chollaGeometry, ocotilloGeometry, tumbleweedGeometry, delineatorGeometry, beamGeometry,
  motorhome, domeTentGeometry, campSetGeometry, personGeometry, crackDecalTexture,
} from './desert/props.js';
import { flickerPoints, flickerPools, glowTime } from './desert/glow.js';

// Desert Run — Level 4, all three zones.
//
// Red Rock Canyon: the road threads a sandstone canyon (the walls and their
// strata are terrain, see Terrain.formCanyon) past boulder falls, junipers
// and an amphitheatre of hoodoos, and under a natural arch.
// Route 66: the canyon opens onto a basin. A ranch fence lines the highway,
// telephone poles march along the right, a railway runs parallel on the
// left with a freight train the player catches and passes, and the Oasis —
// motel, diner and gas station under palms and neon — sits halfway.
// Silver Lake: the course crosses a dry lake bed marked out with cones,
// flags and flares, past a spectators' camp, to a floodlit finish.

const CHUNK = 500;          // metres of road per instancing chunk
const RAIL_LAT = -64;       // railway centreline, left of the road
const tick = () => new Promise((r) => setTimeout(r, 0));
const yawZ = (dx, dz) => Math.atan2(dx, dz);   // yaw that turns local +Z onto (dx, dz)
const yawX = (dx, dz) => Math.atan2(-dz, dx);  // yaw that turns local +X onto (dx, dz)

const PALETTE = {
  wPeach: 0xe8b48a, wTeal: 0x5fb0a8, wWhite: 0xefe9dc, wYellow: 0xf0cf7a, wSand: 0xd8c098, wPink: 0xe4a49a, wMint: 0xa9d8c1,
  trim: 0xf4efe4, concrete: 0xc2b8a8, white: 0xf2f0ea, roofTile: 0xb85c3c, roofTar: 0x4e4a46,
  black: 0x1b1b1d, paintRed: 0xc23a2e, wood: 0x8a6a4a, woodDark: 0x4e3a2a, steel: 0x6c7076, rust: 0x7a4a30,
  hay: 0xc8a860, tire: 0x1a1a1a, canvasW: 0xe8e2d4, canvasR: 0xc84a3a, canvasB: 0x3a6aa8,
};

export default class Desert {
  label = 'Painting the desert';

  constructor({ zone = 0 } = {}) {
    this.zone = zone;
  }

  // ── Planning (before terrain fields) ──────────────────────────────
  plan(world) {
    const t = (this.t = world.track), T = (this.T = world.terrain);
    this.setupRanges();
    // Level bed for the railway beside the highway (see Terrain.formDesert).
    T.desertRail = { lat: RAIL_LAT, half: 6, drop: 0.7, s0: this.Z[1].s0 - 260, s1: this.Z[2].s0 + 600 };
    // The Oasis: a gravel lot on the right, level with the road.
    const oa = t.tag('oasis')[0];
    if (oa) {
      const s = Math.round((oa.s0 + oa.s1) / 2);
      this.oasis = { s };
      for (let k = -2; k <= 2; k++) {
        const ss = s + k * 30;
        const f = t.frame(ss);
        const p = t.pointAt(ss, f.wallR + 34);
        T.addFlatten(p.x, p.z, 30, 22, f.y - 0.25);
      }
    }
    // Start: a pull-out on the right for the marshals' trucks.
    const f0 = t.frame(t.startS + 30);
    const pp = t.pointAt(t.startS + 30, f0.wallR + 12);
    this.pullout = { s: t.startS + 30, x: pp.x, z: pp.z, y: f0.y - 0.15 };
    T.addFlatten(pp.x, pp.z, 11, 12, this.pullout.y);
  }

  setupRanges() {
    const t = this.t;
    this.Z = t.zones.map((z) => ({ s0: z.s0, s1: z.s1 }));
  }

  // ── Build ─────────────────────────────────────────────────────────
  async build(world) {
    const t0 = performance.now();
    this.world = world;
    this.t = world.track; this.T = world.terrain;
    if (!this.Z) this.setupRanges();
    this.ground = makeGround(world.terrain);
    this.group = new THREE.Group();
    this.group.name = 'desert';
    this.rng = mulberry32(7727);
    this.occ = [];
    this.nChunks = Math.ceil((this.t.length + 400) / CHUNK) + 1;
    this.makeMaterials();
    this.makeSigns();
    this.B = new ColorBuilder(PALETTE);

    this.buildStart();
    this.buildArch();
    this.buildRocks();
    await tick();
    this.buildTalus();
    this.buildHorizon();
    await tick();
    this.buildVegetation();
    await tick();
    this.buildFence();
    this.buildPoles();
    this.buildRailway();
    await tick();
    this.buildTrain();
    this.buildOasis();
    this.buildRoadSigns();
    this.buildRoadside();
    await tick();
    this.buildLakebed();
    this.buildPlaya();
    this.buildCourse();
    this.buildCamp();
    this.buildFinish();

    for (const mesh of this.B.build(this.M, { castShadow: ['metal', 'neonFrame'] })) this.group.add(mesh);
    this.buildCars();
    this.buildGlows();
    world.scene.add(this.group);

    const M = this.M;
    world.addNight(M.glassLit, 'emissiveIntensity', 0.05, 1.6);
    world.addNight(M.neon, 'emissiveIntensity', 0.5, 3.4);
    world.addNight(M.signs, 'emissiveIntensity', 0.0, 0.25);
    world.addNight(M.canopyLight, 'emissiveIntensity', 0.3, 2.8);
    world.addNight(M.lamp, 'emissiveIntensity', 0.2, 5.0);
    world.addNight(M.pool, 'emissiveIntensity', 0.05, 0.7);
    world.addNight(M.signs2, 'emissiveIntensity', 0.0, 0.3);
    world.addNight(M.rvGlass, 'emissiveIntensity', 0.0, 1.2);
    world.updaters.push((dt, night, camera, s) => this.animate(dt, night, camera, s));

    let calls = 0, tris = 0;
    this.group.traverse((o) => {
      if (!o.isMesh && !o.isPoints) return;
      calls++;
      const g = o.geometry;
      if (o.isMesh) tris += (g.index ? g.index.count : g.getAttribute('position').count) / 3 * (o.isInstancedMesh ? o.count : 1);
    });
    this.stats = { calls, tris };
    world.desert = this;
    console.info(`[Desert] ${calls} draws, ${(tris / 1000).toFixed(0)}k tris, ${(performance.now() - t0).toFixed(0)} ms`);
  }

  makeMaterials() {
    const S = (color, o = {}) => new THREE.MeshStandardMaterial({ color, roughness: 0.85, metalness: 0, ...o });
    this.M = {
      solid: new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.85 }),
      veg: new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.95 }),
      bush: new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 1 }),
      rock: sandstoneMaterial('desert-rock'),
      glass: S(0x2a3440, { roughness: 0.15, metalness: 0.5 }),
      glassLit: S(0x3a3632, { roughness: 0.2, metalness: 0.2, emissive: 0xffc98a, emissiveIntensity: 0 }),
      pool: S(0x3fc7d6, { roughness: 0.1, emissive: 0x2aa6c0, emissiveIntensity: 0.05 }),
      paintWhite: S(0xf0f0ea, { roughness: 0.7, polygonOffset: true, polygonOffsetFactor: -2, polygonOffsetUnits: -2 }),
      asphaltLot: S(0x55504a, { roughness: 0.95, polygonOffset: true, polygonOffsetFactor: -1, polygonOffsetUnits: -1 }),
      canopyLight: S(0xffffff, { emissive: 0xf4f8ff, emissiveIntensity: 0.3 }),
      lamp: S(0xfff4dc, { emissive: 0xffd9a0, emissiveIntensity: 0.2 }),
      metal: S(0x9a9fa4, { metalness: 0.7, roughness: 0.35 }),
      rail: S(0x8a8680, { metalness: 0.8, roughness: 0.35 }),
      ballast: S(0x8a7a68, { roughness: 1 }),
      rvGlass: S(0x2a2e34, { roughness: 0.2, metalness: 0.3, emissive: 0xffb06a, emissiveIntensity: 0 }),
      rust: new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.9, metalness: 0.15 }),
      paintV: new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.5, metalness: 0.3 }),
    };
  }

  makeSigns() {
    const A = new SignAtlas(2048, '#ffffff');
    const N = new SignAtlas(1024, '#0b0a10');
    const P = (w, h, text, o) => A.add(w, h, paintedSign(text, o));
    const diamond = (text, size = 60) => A.add(192, 192, (g, w, h) => {
      g.clearRect(0, 0, w, h);
      g.save(); g.translate(w / 2, h / 2); g.rotate(Math.PI / 4);
      const s = w * 0.69;
      roundRect(g, -s / 2, -s / 2, s, s, 12); g.fillStyle = '#111'; g.fill();
      roundRect(g, -s / 2 + 7, -s / 2 + 7, s - 14, s - 14, 9); g.fillStyle = '#f5c518'; g.fill();
      g.restore();
      g.fillStyle = '#111'; g.textAlign = 'center'; g.textBaseline = 'middle';
      g.font = `bold ${size}px "Arial Narrow", Arial, sans-serif`;
      const lines = text.split('\n');
      lines.forEach((l, i) => g.fillText(l, w / 2, h / 2 + (i - (lines.length - 1) / 2) * size * 0.9));
    });
    this.sg = {
      dip: diamond('DIP', 64),
      rocks: diamond('FALLING\nROCKS', 36),
      flood: diamond('FLASH\nFLOOD', 38),
      byway: P(768, 256, 'RED ROCK CANYON', { bg: '#6b3a24', fg: '#f6e4c4', font: 'bold 92px Georgia, serif', sub: 'SCENIC BYWAY', subFont: 'bold 46px Georgia, serif', border: '#f6e4c4' }),
      route: A.add(256, 256, (g, w, h) => {
        g.fillStyle = '#fff'; g.fillRect(0, 0, w, h);
        g.fillStyle = '#111';
        g.beginPath();
        g.moveTo(20, 30); g.lineTo(w - 20, 30); g.lineTo(w - 26, 120); g.quadraticCurveTo(w - 40, 200, w / 2, h - 16);
        g.quadraticCurveTo(40, 200, 26, 120); g.closePath(); g.fill();
        g.fillStyle = '#fff';
        g.beginPath();
        g.moveTo(34, 44); g.lineTo(w - 34, 44); g.lineTo(w - 38, 122); g.quadraticCurveTo(w - 52, 188, w / 2, h - 32);
        g.quadraticCurveTo(52, 188, 38, 122); g.closePath(); g.fill();
        g.fillStyle = '#111'; g.textAlign = 'center'; g.textBaseline = 'middle';
        g.font = 'bold 34px Arial'; g.fillText('U S', w / 2, 72);
        g.font = 'bold 30px Arial'; g.fillText('ROUTE', w / 2, 104);
        g.font = 'bold 92px "Arial Narrow", Arial'; g.fillText('66', w / 2, 170);
      }),
      bbOasis: P(1016, 380, 'OASIS MOTEL', { bg: '#1f6f7a', fg: '#fff6e0', font: 'bold 150px "Arial Black", Arial', sub: 'POOL · COLOR TV · DINER — 2 MILES', subFont: 'bold 52px Arial', stripe: '#f2b233' }),
      bbGas: P(1016, 380, 'LAST GAS', { bg: '#f2e8d0', fg: '#b8322a', font: 'bold 170px "Arial Black", Arial', sub: 'NEXT SERVICES 98 MILES', subFont: 'bold 58px Arial', stripe: '#b8322a' }),
      bbTrials: P(1016, 380, 'SILVER LAKE', { bg: '#111418', fg: '#ffd84d', font: 'bold 150px "Arial Black", Arial', sub: 'SPEED TRIALS TONIGHT →', subFont: 'bold 62px Arial', stripe: '#e84a2a' }),
      bbJerky: P(1016, 380, 'JERKY · FIREWORKS', { bg: '#e8c86a', fg: '#4a2a18', font: 'bold 110px "Arial Black", Arial', sub: 'TRADING POST — NEXT EXIT', subFont: 'bold 58px Arial', stripe: '#4a2a18' }),
      gas: P(512, 128, 'LAST CHANCE GAS', { bg: '#c23a2e', fg: '#fff' }),
      price: A.add(256, 256, (g, w, h) => {
        g.fillStyle = '#fff'; g.fillRect(0, 0, w, h);
        g.fillStyle = '#c23a2e'; g.fillRect(0, 0, w, 64);
        g.fillStyle = '#fff'; g.font = 'bold 44px Arial'; g.textAlign = 'center'; g.textBaseline = 'middle'; g.fillText('GAS', w / 2, 32);
        g.fillStyle = '#111'; g.font = 'bold 40px "Courier New", monospace';
        ['REG 5.89', 'PLS 6.19', 'DSL 6.49'].forEach((l, i) => g.fillText(l, w / 2, 100 + i * 56));
      }),
      mile: ['1 MILE', '1/2 MILE', '1/4 MILE'].map((m) => A.add(384, 192, paintedSign(m, { bg: '#111', fg: '#fff', font: 'bold 96px "Arial Narrow", Arial', border: '#ffd84d' }))),
      closed: P(512, 192, 'ROAD CLOSED', { bg: '#f2f2f2', fg: '#111', font: 'bold 84px "Arial Narrow", Arial', border: '#111' }),
      end: P(768, 256, 'END OF COURSE', { bg: '#c23a2e', fg: '#fff', font: 'bold 110px "Arial Narrow", Arial', border: '#fff' }),
      lake: P(768, 256, 'SILVER LAKE', { bg: '#1c2a3a', fg: '#e8eef4', font: 'bold 110px Georgia, serif', sub: 'DRY LAKE · SPEED TRIALS', subFont: 'bold 44px Georgia, serif', border: '#e8eef4' }),
      timing: P(512, 128, 'TIMING', { bg: '#f2f2f2', fg: '#111', font: 'bold 90px "Arial Black", Arial' }),
    };
    this.nn = {
      oasis: N.add(1024, 300, neonSign('Oasis', { color: '#5ff2ff', font: 'bold 200px "Brush Script MT", "Segoe Script", cursive', sub: 'MOTEL · CAFE · GAS', subColor: '#ff5fd2', subFont: 'bold 58px Arial, sans-serif' })),
      vacancy: N.add(512, 160, neonSign('Vacancy', { color: '#ff3b6b', sub: 'COLOR TV · POOL', subColor: '#5ff2ff' })),
      diner: N.add(512, 170, neonSign('Sidewinder Cafe', { color: '#ffb24d', sub: 'OPEN ALL NIGHT', subColor: '#62f0ff' })),
      eat: N.add(256, 128, neonSign('EAT', { color: '#ff4040', font: 'bold 96px "Arial Black", Arial', frame: false })),
    };
    // Second atlas: the extra billboards, mile markers and roadside
    // lettering added with the Route 66 dressing.
    const A2 = new SignAtlas(2048, '#ffffff');
    const P2 = (w, h, text, o) => A2.add(w, h, paintedSign(text, o));
    const weathered = (draw) => (g, w, h) => {
      draw(g, w, h);
      // Sun-faded, peeling: pale blotches and a few dark streaks.
      const r = mulberry32(w * 7 + h);
      for (let i = 0; i < 40; i++) { g.fillStyle = `rgba(255,248,230,${0.05 + r() * 0.12})`; g.beginPath(); g.arc(r() * w, r() * h, 6 + r() * 40, 0, 7); g.fill(); }
      for (let i = 0; i < 12; i++) { g.fillStyle = `rgba(60,40,20,${0.08 + r() * 0.1})`; g.fillRect(r() * w, r() * h * 0.5, 2 + r() * 4, h * (0.2 + r() * 0.5)); }
    };
    const WP = (w, h, text, o) => A2.add(w, h, weathered(paintedSign(text, o)));
    Object.assign(this.sg, {
      bbSnakes: WP(1016, 380, 'SEE LIVE RATTLERS!', { bg: '#f2d64a', fg: '#1a1a1a', font: 'bold 104px "Arial Black", Arial', sub: 'GILA MONSTERS · TARANTULAS — 5 MI', subFont: 'bold 50px Arial', stripe: '#b8322a' }),
      bbPie: WP(1016, 380, 'HOME MADE PIE', { bg: '#f4ece0', fg: '#2e5a8a', font: 'italic bold 128px Georgia, serif', sub: 'SIDEWINDER CAFE · EXIT NOW', subFont: 'bold 56px Arial', stripe: '#2e5a8a' }),
      bbMotor: WP(1016, 380, 'ROUTE 66 MOTOR CO.', { bg: '#1d3f75', fg: '#ffd84d', font: 'bold 96px "Arial Black", Arial', sub: 'TIRES · TOWING · COLD BEER', subFont: 'bold 58px Arial', stripe: '#ffd84d' }),
      bbDino: WP(1016, 380, 'DINOSAUR PARK', { bg: '#3a7a3a', fg: '#fff6d8', font: 'bold 130px "Arial Black", Arial', sub: 'LIFE SIZE! KIDS FREE — NEXT RIGHT', subFont: 'bold 50px Arial', stripe: '#e8a030' }),
      bbFaded: WP(1016, 380, 'ICE COLD WATER', { bg: '#d8c8a8', fg: '#8a4a3a', font: 'bold 120px "Arial Black", Arial', sub: 'FREE · 20 MI · FREE', subFont: 'bold 62px Arial', stripe: '#8a4a3a' }),
      mm: [0, 1, 2, 3, 4].map((k) => A2.add(96, 256, (g, w, h) => {
        g.fillStyle = '#0b6b3a'; g.fillRect(0, 0, w, h);
        g.strokeStyle = '#fff'; g.lineWidth = 5; g.strokeRect(5, 5, w - 10, h - 10);
        g.fillStyle = '#fff'; g.textAlign = 'center'; g.textBaseline = 'middle';
        g.font = 'bold 26px Arial'; g.fillText('MILE', w / 2, 40);
        g.font = 'bold 62px "Arial Narrow", Arial';
        String(141 + k).split('').forEach((d, i) => g.fillText(d, w / 2, 96 + i * 56));
      })),
      tower: A2.add(512, 160, (g, w, h) => {
        g.clearRect(0, 0, w, h); g.fillStyle = '#d8d4cc'; g.fillRect(0, 0, w, h);
        g.fillStyle = '#b8322a'; g.textAlign = 'center'; g.textBaseline = 'middle';
        g.font = 'italic bold 110px Georgia, serif'; g.fillText('Oasis', w / 2, h / 2 + 6);
      }),
      soda: A2.add(128, 256, (g, w, h) => {
        g.fillStyle = '#c8202a'; g.fillRect(0, 0, w, h);
        g.fillStyle = '#fff'; g.font = 'italic bold 30px Georgia, serif'; g.textAlign = 'center';
        g.save(); g.translate(w / 2, h * 0.35); g.rotate(-Math.PI / 2); g.fillText('Ice Cold', 0, 10); g.restore();
        g.fillStyle = '#222'; g.fillRect(16, h * 0.62, w - 32, h * 0.3);
        g.fillStyle = '#e8e8e8'; for (let i = 0; i < 4; i++) g.fillRect(24 + i * 22, h * 0.66, 14, 22);
      }),
      stage: P2(768, 192, 'SILVER LAKE SPEED TRIALS', { bg: '#111418', fg: '#ffd84d', font: 'bold 64px "Arial Black", Arial', stripe: '#e84a2a' }),
    });
    const at2 = A2.texture();
    this.M.signs2 = new THREE.MeshStandardMaterial({ map: at2, roughness: 0.6, emissive: 0xffffff, emissiveMap: at2, emissiveIntensity: 0, transparent: true, alphaTest: 0.5 });
    Object.assign(this.nn, {
      motelArrow: N.add(512, 256, (g, w, h) => {
        g.fillStyle = '#0b0a10'; g.fillRect(0, 0, w, h);
        g.lineJoin = 'round'; g.lineCap = 'round';
        const arrow = () => { g.beginPath(); g.moveTo(24, 70); g.lineTo(380, 70); g.lineTo(380, 30); g.lineTo(490, 128); g.lineTo(380, 226); g.lineTo(380, 186); g.lineTo(24, 186); g.closePath(); };
        g.shadowColor = '#ff3b6b'; g.shadowBlur = 24; g.strokeStyle = '#ff3b6b'; g.lineWidth = 10; arrow(); g.stroke();
        g.shadowBlur = 0; g.strokeStyle = '#ffd2e0'; g.lineWidth = 3; arrow(); g.stroke();
        g.font = 'bold 90px "Arial Black", Arial'; g.textAlign = 'center'; g.textBaseline = 'middle';
        g.shadowColor = '#5ff2ff'; g.shadowBlur = 22; g.fillStyle = '#5ff2ff'; g.fillText('MOTEL', 210, 132);
        g.shadowBlur = 0; g.fillStyle = '#e8ffff'; g.font = 'bold 86px "Arial Black", Arial'; g.fillText('MOTEL', 210, 132);
      }),
    });
    const at = A.texture(), nt = N.texture();
    this.M.signs = new THREE.MeshStandardMaterial({ map: at, roughness: 0.6, emissive: 0xffffff, emissiveMap: at, emissiveIntensity: 0, transparent: true, alphaTest: 0.5 });
    this.M.neon = new THREE.MeshStandardMaterial({ map: nt, color: 0xffffff, roughness: 0.5, emissive: 0xffffff, emissiveMap: nt, emissiveIntensity: 0.6 });
  }

  // ── Helpers ───────────────────────────────────────────────────────
  gy(x, z) { return this.ground.height(x, z); }
  zoneAt(s) { return this.t.zone[this.t.idx(s)]; }
  take(x, z, r) { this.occ.push({ x, z, r }); }
  free(x, z, r) {
    for (const o of this.occ) if (Math.pow(x - o.x, 2) + Math.pow(z - o.z, 2) < Math.pow(r + o.r, 2)) return false;
    return true;
  }
  // Clear of every stretch of road, walls included.
  clearOfRoad(x, z, r) {
    const info = this.T.roadInfo(x, z);
    if (!info.near) return info.d > r + 14;
    const i = this.t.idx(info.s);
    return info.d - r >= Math.max(this.t.wallL[i], this.t.wallR[i]) + 0.8;
  }
  chunkOf(s) { return clamp(Math.floor(s / CHUNK), 0, this.nChunks - 1); }
  // Pose at (s, lat): local +Z faces the road (dir 'road') or along it.
  frameAt(s, lat, dir = 'road') {
    const f = this.t.frame(s);
    const x = f.x + f.rx * lat, z = f.z + f.rz * lat;
    const side = lat >= 0 ? 1 : -1;
    const yaw = dir === 'road' ? yawZ(-f.rx * side, -f.rz * side) : yawZ(f.fx, f.fz);
    return { x, z, y: this.gy(x, z), yaw, f };
  }

  // Instanced mesh per chunk from item lists [{x,y,z,sx,sy,sz,ry,col}].
  addInstanced(geo, mat, chunks, { cast = false, receive = true } = {}) {
    const m4 = new THREE.Matrix4(), q = new THREE.Quaternion(), e = new THREE.Euler(), p = new THREE.Vector3(), sc = new THREE.Vector3(), c = new THREE.Color();
    for (const items of chunks) {
      if (!items.length) continue;
      const im = new THREE.InstancedMesh(geo, mat, items.length);
      items.forEach((it, k) => {
        if (it.q) q.copy(it.q); else { e.set(it.rx || 0, it.ry || 0, it.rz || 0); q.setFromEuler(e); }
        m4.compose(p.set(it.x, it.y, it.z), q, sc.set(it.sx, it.sy ?? it.sx, it.sz ?? it.sx));
        im.setMatrixAt(k, m4);
        if (it.col !== undefined) im.setColorAt(k, c.setHex(it.col).multiplyScalar(it.b ?? 1));
      });
      im.instanceMatrix.needsUpdate = true;
      if (im.instanceColor) im.instanceColor.needsUpdate = true;
      im.computeBoundingSphere();
      im.castShadow = cast;
      im.receiveShadow = receive;
      im.matrixAutoUpdate = false;
      im.updateMatrix();
      this.group.add(im);
    }
  }
  chunks() { return Array.from({ length: this.nChunks }, () => []); }

  // A flat sign on a post (or two), facing back down the road.
  roadSign(s, side, rect, w, h, { lat = null, posts = 1, y0 = 1.6, face = -1, key = 'signs' } = {}) {
    const f = this.t.frame(s);
    const L = lat ?? side * ((side > 0 ? f.wallR : f.wallL) + 0.9);
    const x = f.x + f.rx * L, z = f.z + f.rz * L;
    const y = this.gy(x, z);
    const B = this.B;
    B.setFrame(x, y, z, yawZ(f.fx * face, f.fz * face));
    const px = posts === 1 ? [0] : [-w * 0.35, w * 0.35];
    for (const p of px) B.box('steel', 0.09, y0 + h * 0.5, 0.09, p, 0, -0.06);
    B.put(key, signGeometry(rect, w, h), 0, y0 + h / 2, 0);
    B.put(key, signGeometry(rect, w, h), 0, y0 + h / 2, -0.04, 0, Math.PI, 0);
  }

  // ── Start gantry and marshals' pull-out ───────────────────────────
  buildStart() {
    const t = this.t, S = t.frame(t.startS);
    const wl = S.wallL, wr = S.wallR;
    const yaw = Math.atan2(S.fx, S.fz);
    const B = this.B;
    const H = 7.2;
    const place = (lat) => ({ x: S.x + S.rx * lat, z: S.z + S.rz * lat, y: S.y });
    for (const lat of [-(wl + 1.3), wr + 1.3]) {
      const p = place(lat);
      B.setFrame(p.x, p.y, p.z, yaw);
      for (const off of [-0.35, 0.35]) B.box('steel', 0.16, H + 1.2, 0.16, 0, -1, off);
      for (let k = 0; k < 7; k++) B.box('steel', 0.08, 0.08, 0.7, 0, 0.5 + k, 0);
      B.box('concrete', 1.1, 0.5, 1.4, 0, -0.4, 0);
    }
    const span = wl + wr + 2.6;
    const c = place((wr - wl) / 2);
    B.setFrame(c.x, c.y, c.z, yaw);
    for (const dy of [H, H - 0.9]) B.box('steel', span, 0.18, 0.18, 0, dy - 0.09, 0);
    const bannerTex = (front) => {
      const cv = document.createElement('canvas');
      cv.width = 1024; cv.height = 160;
      const g = cv.getContext('2d');
      g.fillStyle = '#1a0e0a'; g.fillRect(0, 0, 1024, 160);
      const sq = 20;
      for (let y = 0; y < 160; y += sq) for (let x = 0; x < 120; x += sq) {
        g.fillStyle = ((x + y) / sq) % 2 ? '#f2f2f2' : '#111';
        g.fillRect(x, y, sq, sq); g.fillRect(1024 - 120 + x, y, sq, sq);
      }
      const grd = g.createLinearGradient(120, 0, 904, 0);
      grd.addColorStop(0, '#ff9a3c'); grd.addColorStop(1, '#c8431e');
      g.fillStyle = grd; g.fillRect(120, 0, 784, 8); g.fillRect(120, 152, 784, 8);
      g.fillStyle = '#fff'; g.textAlign = 'center'; g.textBaseline = 'middle';
      g.font = 'italic 900 84px "Arial Narrow", Arial, sans-serif';
      g.fillText(front ? 'RED ROCK CANYON' : 'START', 512, 70);
      g.font = 'bold 26px "Arial Narrow", Arial, sans-serif'; g.fillStyle = '#ffc89a';
      g.fillText(front ? 'STAGE 4 · DESERT RUN' : 'RED ROCK CANYON', 512, 132);
      const tx = new THREE.CanvasTexture(cv);
      tx.colorSpace = THREE.SRGBColorSpace; tx.anisotropy = 8;
      return tx;
    };
    const bw = span - 1.6, bh = bw * 160 / 1024;
    for (const front of [true, false]) {
      const tex = bannerTex(front);
      const mat = new THREE.MeshStandardMaterial({ map: tex, roughness: 0.7, emissive: 0xffffff, emissiveMap: tex, emissiveIntensity: 0.2 });
      this.world.addNight(mat, 'emissiveIntensity', 0.15, 0.7);
      const g = new THREE.PlaneGeometry(bw, bh);
      g.rotateY(front ? Math.PI : 0);
      g.translate(0, S.y + H + 0.1 - bh / 2, front ? -0.05 : 0.05);
      g.rotateY(yaw);
      g.translate(c.x, 0, c.z);
      const mesh = new THREE.Mesh(g, mat);
      mesh.matrixAutoUpdate = false;
      this.group.add(mesh);
    }
    // Marshals' pull-out: gravel pad, a tow truck and a pickup.
    const d = this.pullout, f = t.frame(d.s);
    const ry = yawZ(f.fx, f.fz);
    B.setFrame(d.x, d.y, d.z, ry);
    B.box('asphaltLot', 18, 0.06, 16, 0, 0.02, 0);
    this.cars = this.cars || [];
    this.cars.push({ kind: 'pickup', color: 0xd8d2c4, seed: 3, x: d.x - f.rx * 2 + f.fx * 4, y: d.y, z: d.z - f.rz * 2 + f.fz * 4, yaw: Math.atan2(f.fx, f.fz) + 0.3 });
    this.cars.push({ kind: 'van', color: 0x2b2f36, seed: 5, x: d.x + f.rx * 3 - f.fx * 4, y: d.y, z: d.z + f.rz * 3 - f.fz * 4, yaw: Math.atan2(f.fx, f.fz) - 0.2 });
    this.take(d.x, d.z, 14);
    this.roadSign(t.startS + 150, 1, this.sg.byway, 4.2, 1.4, { posts: 2, y0: 1.2 });
    // Behind the grid the road is closed: striped barricades and a sign.
    const fb = t.frame(1.5);
    const by = yawZ(fb.fx, fb.fz);
    for (let lat = -fb.hw - 0.5; lat <= fb.hw + 0.6; lat += 2.4) {
      const x = fb.x + fb.rx * lat, z = fb.z + fb.rz * lat;
      B.setFrame(x, fb.y, z, by);
      for (const px of [-0.9, 0.9]) B.box('woodDark', 0.08, 1.1, 0.5, px, 0, 0, 0, 0.35, 0);
      B.box('paintRed', 2.2, 0.22, 0.06, 0, 0.72, 0.1);
      B.box('white', 2.2, 0.22, 0.06, 0, 0.42, 0.1);
    }
    this.roadSign(1.2, 1, this.sg.closed, 2.6, 1.0, { lat: 0, posts: 2, y0: 1.3, face: 1 });
    this.take(t.pointAt(t.startS + 150, 9).x, t.pointAt(t.startS + 150, 9).z, 4);
  }

  // ── Natural arch spanning the road ────────────────────────────────
  buildArch() {
    const tag = this.t.tag('arch')[0];
    if (!tag) return;
    const t = this.t;
    const s = Math.round((tag.s0 + tag.s1) / 2);
    const f = t.frame(s);
    const R = 21;
    // Legs stand on whatever the canyon floor is doing either side.
    const gL = this.gy(f.x - f.rx * R, f.z - f.rz * R) - f.y;
    const gR = this.gy(f.x + f.rx * R, f.z + f.rz * R) - f.y;
    const merged = archGeometry(17, { R, H: 31, groundL: gL, groundR: gR, base: f.y });
    const yaw = yawX(f.rx, f.rz);
    merged.rotateY(yaw);
    merged.translate(f.x, f.y, f.z);
    merged.computeBoundingSphere();
    const mesh = new THREE.Mesh(merged, this.M.rock);
    mesh.castShadow = true;
    mesh.receiveShadow = true;
    mesh.matrixAutoUpdate = false;
    this.group.add(mesh);
    this.take(f.x + f.rx * R, f.z + f.rz * R, 12);
    this.take(f.x - f.rx * R, f.z - f.rz * R, 12);
    this.archS = s;
  }

  // ── Hoodoos, boulder falls and slickrock outcrops ─────────────────
  buildRocks() {
    const t = this.t, rng = mulberry32(311);
    const hoodoos = [0, 1, 2, 3].map(() => this.chunks());
    // Rockfall comes in three characters: weathered boulders, fresh broken
    // blocks and fallen slabs of ledge. The strata and varnish are in the
    // vertex colours, so instance tints only nudge brightness and hue.
    const KINDS = ['round', 'block', 'slab'];
    const boulders = KINDS.map(() => this.chunks());
    const PAL = [0xffffff, 0xfff0e4, 0xf4e4d8, 0xffe8d0, 0xe8d8cc];
    const kindOf = () => { const r = rng(); return r < 0.4 ? 0 : r < 0.8 ? 1 : 2; };
    const zEnd = this.Z[1].s0 + 250;
    const S = {};
    // Hoodoo amphitheatre: the wide bay on the right.
    for (const tag of t.tag('hoodoos')) {
      for (let s = tag.s0 - 20; s < tag.s1 + 20; s += 3) {
        t.frame(s, S);
        for (let k = 0; k < 3; k++) {
          const lat = S.wallR + rrange(rng, 6, 120);
          const x = S.x + S.rx * lat + (rng() - 0.5) * 4, z = S.z + S.rz * lat + (rng() - 0.5) * 4;
          const h = lerp(6, 24, Math.pow(rng(), 1.3)) * lerp(0.7, 1.2, smoothstep(10, 100, lat - S.wallR));
          const r = h * 0.2;
          if (!this.clearOfRoad(x, z, r + 1) || !this.free(x, z, r * 0.8)) continue;
          if (this.T.slopeAt(x, z, 3) > 0.35) continue;
          this.take(x, z, r * 0.6);
          const y = this.gy(x, z);
          hoodoos[Math.floor(rng() * 4)][this.chunkOf(s)].push({ x, y: y - 0.4, z, sx: h * lerp(0.9, 1.3, rng()), sy: h, sz: h * lerp(0.9, 1.3, rng()), ry: rng() * 6.3, col: 0xffffff, b: lerp(0.85, 1.05, rng()) });
          // Spalled pieces round the pedestal.
          for (let j = 0; j < 3; j++) {
            const a = rng() * 6.3, d = r * lerp(0.9, 2.2, rng());
            const px = x + Math.cos(a) * d, pz = z + Math.sin(a) * d, sz = h * lerp(0.03, 0.07, rng());
            if (!this.clearOfRoad(px, pz, sz)) continue;
            boulders[kindOf()][this.chunkOf(s)].push({ x: px, y: this.gy(px, pz) - sz * 0.2, z: pz, sx: sz * lerp(0.8, 1.4, rng()), sy: sz * lerp(0.6, 1, rng()), sz, ry: rng() * 6.3, col: rpick(rng, PAL), b: lerp(0.85, 1.05, rng()) });
          }
        }
      }
    }
    // Along the canyon: boulders fallen from the walls, the odd hoodoo on a
    // ledge, rock at the wall foot.
    for (let s = 20; s < zEnd; s += 4) {
      t.frame(s, S);
      for (const side of [-1, 1]) {
        if (rng() < 0.45) continue;
        const wall = side > 0 ? S.wallR : S.wallL;
        const lat = side * (wall + rrange(rng, 2, 45) * (rng() < 0.3 ? 2.5 : 1));
        const x = S.x + S.rx * lat + (rng() - 0.5) * 3, z = S.z + S.rz * lat + (rng() - 0.5) * 3;
        const big = rng() < 0.18;
        const size = big ? rrange(rng, 2.5, 6) : rrange(rng, 0.5, 1.8);
        if (!this.clearOfRoad(x, z, size) || !this.free(x, z, size * 0.7)) continue;
        const slope = this.T.slopeAt(x, z, 2);
        if (slope > 0.6) continue;
        boulders[kindOf()][this.chunkOf(s)].push({ x, y: this.gy(x, z) - size * 0.25, z, sx: size * lerp(0.8, 1.4, rng()), sy: size * lerp(0.6, 1.0, rng()), sz: size, ry: rng() * 6.3, col: rpick(rng, PAL), b: lerp(0.8, 1.05, rng()) });
        if (big) this.take(x, z, size * 0.8);
        if (rng() < 0.05 && slope < 0.4) {
          const h = rrange(rng, 5, 14);
          hoodoos[Math.floor(rng() * 4)][this.chunkOf(s)].push({ x, y: this.gy(x, z) - 0.4, z, sx: h * 1.1, sy: h, sz: h * 1.1, ry: rng() * 6.3, col: 0xffffff, b: 0.95 });
        }
      }
    }
    // Scattered rock on the basin and lake shore: duller and browner than
    // the canyon's red sandstone.
    const DARK = [0xc8b0a4, 0xb8a094, 0xd0bcb0, 0xa89488];
    for (let s = this.Z[1].s0; s < t.length; s += 9) {
      t.frame(s, S);
      for (const side of [-1, 1]) {
        if (rng() < 0.55) continue;
        const lat = side * rrange(rng, 12, 260);
        const x = S.x + S.rx * lat, z = S.z + S.rz * lat;
        if (this.zoneAt(s) === 2 && Math.abs(lat) < 400) continue;
        const size = rrange(rng, 0.4, 1.6);
        if (!this.clearOfRoad(x, z, size) || Math.abs(lat - RAIL_LAT) < 10) continue;
        boulders[0][this.chunkOf(s)].push({ x, y: this.gy(x, z) - size * 0.3, z, sx: size * 1.2, sy: size * 0.7, sz: size, ry: rng() * 6.3, col: rpick(rng, DARK), b: 1 });
      }
    }
    const hg = [11, 23, 37, 51].map((sd) => hoodooGeometry(sd));
    hoodoos.forEach((ch, i) => this.addInstanced(hg[i], this.M.rock, ch, { cast: true }));
    KINDS.forEach((k, i) => this.addInstanced(rockGeometry(91 + i * 17, k), this.M.rock, this.coarse(boulders[i], 2), { cast: true }));
  }

  // Merge every k neighbouring chunks: sparse props don't earn a draw call
  // per 500 m.
  coarse(chunks, k) {
    const out = [];
    for (let i = 0; i < chunks.length; i += k) out.push(chunks.slice(i, i + k).flat());
    return out;
  }

  // ── Talus: scree aprons at the foot of the canyon walls ───────────
  buildTalus() {
    const t = this.t, T = this.T, rng = mulberry32(5150);
    const scree = this.chunks(), blocks = this.chunks();
    const PAL = [0xffffff, 0xf8e8dc, 0xecd8c8, 0xfff2e0];
    const zEnd = this.Z[1].s0 + 150;
    const S = {};
    for (let s = 10; s < zEnd; s += 3) {
      t.frame(s, S);
      for (const side of [-1, 1]) {
        const wall = side > 0 ? S.wallR : S.wallL;
        // Walk outward to where the wall starts to climb: that's the foot.
        let foot = -1, prevY = this.gy(S.x + S.rx * side * (wall + 1), S.z + S.rz * side * (wall + 1));
        for (let d = wall + 3; d < wall + 130; d += 2.5) {
          const x = S.x + S.rx * side * d, z = S.z + S.rz * side * d;
          const y = this.gy(x, z);
          if (y - prevY > 1.1) { foot = d; break; }
          prevY = y;
        }
        if (foot < 0) continue;
        // A fan of chips spilling out from the foot, thinning with distance;
        // a few blocks lodged higher up the slope.
        const n = 5 + Math.floor(rng() * 5);
        for (let k = 0; k < n; k++) {
          const u = -Math.pow(rng(), 1.8) * 10 + rng() * 2;  // metres past the foot (negative = out on the floor)
          const d = foot + u;
          const x = S.x + S.rx * side * d + S.fx * (rng() - 0.5) * 3, z = S.z + S.rz * side * d + S.fz * (rng() - 0.5) * 3;
          if (!this.clearOfRoad(x, z, 1) || this.T.slopeAt(x, z, 1.5) > 0.7) continue;
          const sz = lerp(0.3, 1.2, Math.pow(rng(), 2)) * (u > -2 ? 1.3 : 1);
          scree[this.chunkOf(s)].push({ x, y: this.gy(x, z) - sz * 0.15, z, sx: sz * lerp(0.8, 1.5, rng()), sy: sz * lerp(0.5, 0.9, rng()), sz, ry: rng() * 6.3, rx: (rng() - 0.5) * 0.5, col: rpick(rng, PAL), b: lerp(0.75, 1.05, rng()) });
        }
        if (rng() < 0.35) {
          const d = foot + rrange(rng, -5, 1.5);
          const x = S.x + S.rx * side * d, z = S.z + S.rz * side * d;
          const sz = rrange(rng, 1.2, 3.2);
          if (this.clearOfRoad(x, z, sz) && this.free(x, z, sz * 0.6) && this.T.slopeAt(x, z, 2) < 0.8) {
            blocks[this.chunkOf(s)].push({ x, y: this.gy(x, z) - sz * 0.3, z, sx: sz * lerp(0.9, 1.4, rng()), sy: sz * lerp(0.6, 1, rng()), sz, ry: rng() * 6.3, rz: (rng() - 0.5) * 0.4, col: rpick(rng, PAL), b: lerp(0.8, 1, rng()) });
          }
        }
      }
    }
    this.addInstanced(rockGeometry(401, 'scree'), this.M.rock, this.coarse(scree, 3));
    this.addInstanced(rockGeometry(433, 'block'), this.M.rock, this.coarse(blocks, 3), { cast: true });
  }

  // ── Buttes and spires on the horizon ──────────────────────────────
  // Monument-style silhouettes standing off across the basin, kilometres
  // away: a single instanced mesh each for the two shapes.
  buildHorizon() {
    const t = this.t, rng = mulberry32(1966), S = {};
    const buttes = [], spires = [];
    const s0 = this.Z[0].s1 - 500, s1 = this.Z[2].s0 + 900;
    for (let s = s0; s < s1; s += 60) {
      t.frame(s, S);
      const side = rng() < 0.5 ? -1 : 1;
      const lat = side * rrange(rng, 850, 2600);
      const x = S.x + S.rx * lat, z = S.z + S.rz * lat;
      // Skip where the terrain already stands tall (its own mesas and the
      // far range) — a butte there would just be buried.
      if (this.T.roadInfo(x, z).d < 700) continue;
      const y = this.gy(x, z);
      if (y - S.y > 25) continue;
      if (rng() < 0.3) {
        const h = rrange(rng, 90, 170);
        spires.push({ x, y: y - 6, z, sx: h * rrange(rng, 0.14, 0.22), sy: h, sz: h * rrange(rng, 0.14, 0.22), ry: rng() * 6.3, col: 0xffffff, b: lerp(0.9, 1.05, rng()) });
      } else {
        const h = rrange(rng, 60, 140), r = h * rrange(rng, 0.6, 1.6);
        buttes.push({ x, y: y - 8, z, sx: r, sy: h, sz: r * rrange(rng, 0.6, 1.1), ry: rng() * 6.3, col: 0xffffff, b: lerp(0.9, 1.05, rng()) });
      }
    }
    this.addInstanced(butteGeometry(7), this.M.rock, [buttes], { receive: false });
    this.addInstanced(butteGeometry(19, true), this.M.rock, [spires], { receive: false });
  }

  // ── Plants ────────────────────────────────────────────────────────
  buildVegetation() {
    const t = this.t, rng = mulberry32(4401);
    const bushes = this.chunks(), junipers = this.chunks(), yuccas = this.chunks();
    const pinyons = this.chunks(), sage = this.chunks(), cholla = this.chunks(), ocotillo = this.chunks();
    const joshua = [0, 1, 2].map(() => this.chunks()), saguaro = [0, 1].map(() => this.chunks());
    const weeds = [];
    const S = {};
    // Creosote olive, brittlebush grey-green, dead-brown; sage silver-blue.
    const BUSH = [0x646440, 0x70683f, 0x585a38, 0x7a6e4c, 0x6e5e40, 0x7c7650, 0x867a58];
    const SAGE = [0x7e8c70, 0x72806a, 0x8a9678, 0x6a7a62, 0x94a080];
    const zc = this.Z[1].s0, zp = this.Z[2].s0;
    this.mouthS = t.tag('mouth')[0]?.s0 ?? zc - 300;
    for (let s = 0; s < t.length; s += 2.2) {
      t.frame(s, S);
      const zone = this.zoneAt(s);
      const inCanyon = s < zc + 150;
      const onLake = s > zp + 250;
      for (const side of [-1, 1]) {
        const wall = side > 0 ? S.wallR : S.wallL;
        // Distance out: dense near, thinning with distance.
        const reach = inCanyon ? 70 : onLake ? 60 : 420;
        const lat = side * (wall + 1.5 + reach * Math.pow(rng(), 1.6));
        const x = S.x + S.rx * lat + (rng() - 0.5) * 2, z = S.z + S.rz * lat + (rng() - 0.5) * 2;
        if (onLake && Math.abs(lat) < 900) {
          // Out on the lake bed: nothing grows until the shore.
          continue;
        }
        const r = rng();
        const slope = this.T.slopeAt(x, z, 2);
        if (slope > 0.45) continue;
        if (Math.abs(lat - RAIL_LAT) < 8 && zone >= 1) continue;
        if (!this.clearOfRoad(x, z, 1.2) || !this.free(x, z, 0.6)) continue;
        const y = this.gy(x, z);
        const ch = this.chunkOf(s);
        const bush = (list, pal, lo, hi) => list[ch].push({ x, y: y - 0.05, z, sx: rrange(rng, lo, hi), sy: rrange(rng, lo * 1.1, hi * 1.1), sz: rrange(rng, lo, hi), ry: rng() * 6.3, col: rpick(rng, pal), b: lerp(0.8, 1.1, rng()) });
        if (inCanyon) {
          if (r < 0.42) bush(bushes, BUSH, 0.5, 1.05);
          else if (r < 0.62) bush(sage, SAGE, 0.6, 1.1);
          else if (r < 0.68 && s < this.mouthS) { junipers[ch].push({ x, y: y - 0.2, z, sx: rrange(rng, 0.8, 1.4), ry: rng() * 6.3 }); this.take(x, z, 1.5); }
          else if (r < 0.72 && s < this.mouthS) { pinyons[ch].push({ x, y: y - 0.2, z, sx: rrange(rng, 0.7, 1.2), ry: rng() * 6.3 }); this.take(x, z, 1.5); }
          else if (r < 0.79) yuccas[ch].push({ x, y: y - 0.05, z, sx: rrange(rng, 0.7, 1.2), ry: rng() * 6.3 });
        } else {
          const near = Math.abs(lat) - wall < 60;
          if (r < 0.5) bush(bushes, BUSH, 0.5, 1.1);
          else if (r < 0.66) bush(sage, SAGE, 0.5, 1.0);
          else if (r < (near ? 0.715 : 0.69)) { joshua[Math.floor(rng() * 3)][ch].push({ x, y: y - 0.15, z, sx: rrange(rng, 0.8, 1.35), ry: rng() * 6.3 }); this.take(x, z, 2); }
          else if (r < (near ? 0.728 : 0.70)) { saguaro[Math.floor(rng() * 2)][ch].push({ x, y: y - 0.2, z, sx: rrange(rng, 0.85, 1.2), ry: rng() * 6.3 }); this.take(x, z, 1.2); }
          else if (r < 0.75) cholla[ch].push({ x, y: y - 0.05, z, sx: rrange(rng, 0.8, 1.4), ry: rng() * 6.3, col: 0xffffff, b: lerp(0.9, 1.15, rng()) });
          else if (r < 0.765) ocotillo[ch].push({ x, y: y - 0.05, z, sx: rrange(rng, 0.8, 1.2), ry: rng() * 6.3 });
          else if (r < 0.8) yuccas[ch].push({ x, y: y - 0.05, z, sx: rrange(rng, 0.6, 1.1), ry: rng() * 6.3 });
          else if (r < 0.81) weeds.push({ x, y: y + 0.35, z, sx: rrange(rng, 0.45, 0.7), ry: rng() * 6.3, rx: rng() * 3 });
        }
      }
    }
    // Route 66 roadside: a denser band of the showy species within ~45 m
    // of the fence, where the camera actually looks — the open basin
    // beyond stays sparse.
    for (let s = zc + 40; s < zp + 120; s += 5) {
      if (this.oasis && Math.abs(s - this.oasis.s) < 95) continue;
      t.frame(s, S);
      for (const side of [-1, 1]) {
        const wall = side > 0 ? S.wallR : S.wallL;
        const lat = side * (wall + 2.5 + 42 * Math.pow(rng(), 1.3));
        if (Math.abs(lat - RAIL_LAT) < 9) continue;
        const x = S.x + S.rx * lat + (rng() - 0.5) * 3, z = S.z + S.rz * lat + (rng() - 0.5) * 3;
        if (!this.clearOfRoad(x, z, 1.2) || !this.free(x, z, 1)) continue;
        const y = this.gy(x, z), ch = this.chunkOf(s), r = rng();
        if (r < 0.12) { joshua[Math.floor(rng() * 3)][ch].push({ x, y: y - 0.15, z, sx: rrange(rng, 0.75, 1.3), ry: rng() * 6.3 }); this.take(x, z, 2); }
        else if (r < 0.16) { saguaro[Math.floor(rng() * 2)][ch].push({ x, y: y - 0.2, z, sx: rrange(rng, 0.8, 1.15), ry: rng() * 6.3 }); this.take(x, z, 1.2); }
        else if (r < 0.34) cholla[ch].push({ x, y: y - 0.05, z, sx: rrange(rng, 0.8, 1.5), ry: rng() * 6.3, col: 0xffffff, b: lerp(0.9, 1.15, rng()) });
        else if (r < 0.40) ocotillo[ch].push({ x, y: y - 0.05, z, sx: rrange(rng, 0.8, 1.2), ry: rng() * 6.3 });
        else if (r < 0.56) yuccas[ch].push({ x, y: y - 0.05, z, sx: rrange(rng, 0.7, 1.2), ry: rng() * 6.3 });
        else if (r < 0.64) weeds.push({ x, y: y + 0.35, z, sx: rrange(rng, 0.45, 0.75), ry: rng() * 6.3, rx: rng() * 3 });
        else bushes[ch].push({ x, y: y - 0.05, z, sx: rrange(rng, 0.6, 1.2), sy: rrange(rng, 0.6, 1.2), sz: rrange(rng, 0.6, 1.2), ry: rng() * 6.3, col: rpick(rng, BUSH), b: lerp(0.8, 1.1, rng()) });
      }
    }
    this.addInstanced(bushGeometry(12), this.M.bush, this.coarse(bushes, 2));
    this.addInstanced(sageGeometry(21), this.M.bush, this.coarse(sage, 2));
    this.addInstanced(juniperGeometry(5), this.M.veg, junipers, { cast: true });
    this.addInstanced(pinyonGeometry(8), this.M.veg, this.coarse(pinyons, 3), { cast: true });
    this.addInstanced(yuccaGeometry(9), this.M.veg, yuccas);
    this.addInstanced(chollaGeometry(31), this.M.veg, this.coarse(cholla, 2));
    this.addInstanced(ocotilloGeometry(13), this.M.veg, this.coarse(ocotillo, 3));
    [3, 17, 29].forEach((sd, i) => this.addInstanced(joshuaGeometry(sd), this.M.veg, this.coarse(joshua[i], 2), { cast: true }));
    [7, 41].forEach((sd, i) => this.addInstanced(saguaroGeometry(sd), this.M.veg, this.coarse(saguaro[i], 2), { cast: true }));
    this.weeds = weeds;
  }

  // ── Ranch fence along the highway (the wall) ─────────────────────
  buildFence() {
    const t = this.t, S = {};
    const s0 = this.Z[1].s0 - 120, s1 = this.Z[2].s0 + 200;
    const posts = [], wires = [], refl = [];
    const gaps = this.fenceGaps = [];
    if (this.oasis) gaps.push({ s0: this.oasis.s - 70, s1: this.oasis.s + 70, side: 1 });
    for (const side of [-1, 1]) {
      let prev = null, k = 0;
      for (let s = s0; s <= s1; s += 4, k++) {
        t.frame(s, S);
        const lat = side * ((side > 0 ? S.wallR : S.wallL) + 0.25);
        const x = S.x + S.rx * lat, z = S.z + S.rz * lat, y = this.gy(x, z);
        const gap = gaps.some((g) => g.side === side && s > g.s0 && s < g.s1);
        if (gap) {
          // In front of the Oasis: a kerb and painted bollards instead.
          prev = null;
          const B = this.B;
          B.setFrame(x, S.y, z, yawZ(S.fx, S.fz));
          B.box('concrete', 0.5, 0.25, 4.05, 0, -0.05, 0);
          for (const dz of [-1, 1]) {
            B.box('white', 0.22, 0.95, 0.22, 0, 0, dz);
            B.box('paintRed', 0.24, 0.16, 0.24, 0, 0.62, dz);
          }
          continue;
        }
        const fade = smoothstep(s0, s0 + 80, s) * (1 - smoothstep(s1 - 80, s1, s));
        const tall = k % 4 === 0 ? 1.35 : 1.2;
        posts.push({ x, y: y - 0.3, z, sx: 1, sy: tall * lerp(0.4, 1, fade), sz: 1, ry: hash2(k, side) * 3, col: k % 4 === 0 ? 0x6a5846 : 0x7a6a56 });
        if (k % 10 === 0) refl.push({ x: x - S.rx * side * 0.08, y: y + 0.85, z: z - S.rz * side * 0.08, side });
        const cur = { x, y, z };
        if (prev && fade > 0.9) {
          for (const h of [0.45, 0.72, 0.98]) wires.push(prev.x, prev.y + h, prev.z, x, y + h, z);
          // Now and then a tumbleweed has blown up against the wire.
          if (hash2(k, side, 9) < 0.035) (this.fenceWeeds ||= []).push({ x: x - S.rx * side * 0.7, y: y + 0.45, z: z - S.rz * side * 0.7, sx: 0.5 + hash2(k, side, 3) * 0.3, ry: k, rx: k * 0.7 });
        }
        prev = cur;
      }
    }
    const post = paint(prep(new THREE.CylinderGeometry(0.055, 0.07, 1.3, 5)), 0xffffff);
    post.translate(0, 0.65, 0);
    const ch = [posts];
    this.addInstanced(post, this.M.bush, ch, { cast: false });
    const wg = new THREE.BufferGeometry();
    wg.setAttribute('position', new THREE.Float32BufferAttribute(wires, 3));
    wg.computeBoundingSphere();
    const lines = new THREE.LineSegments(wg, new THREE.LineBasicMaterial({ color: 0x3a3632, transparent: true, opacity: 0.8 }));
    lines.matrixAutoUpdate = false;
    this.group.add(lines);
    this.reflectors = refl;
  }

  // ── Telephone poles on the right ──────────────────────────────────
  buildPoles() {
    const t = this.t, S = {};
    const poles = [];
    for (let s = this.Z[1].s0 - 300; s < this.Z[2].s0 + 900; s += 46) {
      t.frame(s, S);
      if (this.oasis && Math.abs(s - this.oasis.s) < 80) continue;
      const lat = S.wallR + 7;
      const x = S.x + S.rx * lat, z = S.z + S.rz * lat;
      if (!this.clearOfRoad(x, z, 1.5)) continue;
      poles.push({ x, y: this.gy(x, z) - 0.3, z, rx: S.rx, rz: S.rz });
    }
    if (!poles.length) return;
    const geo = poleGeometry(new Builder());
    const items = poles.map((p, k) => {
      p.yaw = yawX(p.rx, p.rz) + (hash2(k, 3) - 0.5) * 0.06;
      return { x: p.x, y: p.y, z: p.z, sx: 1, ry: p.yaw, rx: (hash2(k, 5) - 0.5) * 0.05, rz: (hash2(k, 7) - 0.5) * 0.05 };
    });
    const mat = new THREE.MeshStandardMaterial({ color: 0x5e4c3a, roughness: 0.95 });
    this.addInstanced(geo, mat, [items], { cast: true });
    const wpos = [];
    for (let k = 0; k < poles.length - 1; k++) {
      const a = poles[k], b = poles[k + 1];
      const span = Math.hypot(b.x - a.x, b.z - a.z);
      if (span > 120) continue;
      const sag = 0.5 + span * 0.012;
      for (const off of [-0.95, 0, 0.95]) {
        const ax = a.x + Math.cos(a.yaw) * off, az = a.z - Math.sin(a.yaw) * off, ay = a.y + 8.85;
        const bx = b.x + Math.cos(b.yaw) * off, bz = b.z - Math.sin(b.yaw) * off, by = b.y + 8.85;
        const N = 10;
        for (let i = 0; i < N; i++) for (const u of [i / N, (i + 1) / N]) wpos.push(lerp(ax, bx, u), lerp(ay, by, u) - sag * 4 * u * (1 - u), lerp(az, bz, u));
      }
    }
    const wg = new THREE.BufferGeometry();
    wg.setAttribute('position', new THREE.Float32BufferAttribute(wpos, 3));
    wg.computeBoundingSphere();
    const wires = new THREE.LineSegments(wg, new THREE.LineBasicMaterial({ color: 0x1a1816 }));
    wires.matrixAutoUpdate = false;
    this.group.add(wires);
  }

  // ── Railway: path, ballast, ties and rails ────────────────────────
  railPath() {
    const t = this.t, S = {};
    const sA = this.Z[1].s0 - 200, sB = Math.min(t.finishS - 300, this.Z[2].s0 + 1400);
    const pts = [];
    for (let s = sA; s <= sB; s += 5) {
      t.frame(s, S);
      // Beyond the highway the line drifts off along the lake shore.
      const lat = RAIL_LAT - 120 * Math.pow(smoothstep(this.Z[2].s0 + 200, sB, s), 1.5);
      pts.push({ x: S.x + S.rx * lat, z: S.z + S.rz * lat });
    }
    // Extend: bend away to the left behind, carry straight on ahead.
    const ext = (a, b, n, bend) => {
      const out = [];
      let dx = b.x - a.x, dz = b.z - a.z;
      const l = Math.hypot(dx, dz); dx /= l; dz /= l;
      let x = b.x, z = b.z, h = Math.atan2(dz, dx);
      for (let k = 0; k < n; k++) {
        h += bend * smoothstep(0, 60, k) * (1 - smoothstep(100, 160, k));
        x += Math.cos(h) * 5; z += Math.sin(h) * 5;
        out.push({ x, z });
      }
      return out;
    };
    const back = ext(pts[1], pts[0], 400, 0.0022).reverse();
    const fwd = ext(pts[pts.length - 2], pts[pts.length - 1], 500, 0);
    const all = back.concat(pts, fwd);
    // Heights: the rendered ground, smoothed along the line.
    const raw = all.map((p) => this.gy(p.x, p.z));
    const ys = raw.map((_, i) => {
      let a = 0, w = 0;
      for (let k = -10; k <= 10; k++) { const j = clamp(i + k, 0, raw.length - 1); const q = Math.exp(-(k * k) / 30); a += raw[j] * q; w += q; }
      return a / w;
    });
    let u = 0;
    all.forEach((p, i) => {
      if (i) u += Math.hypot(p.x - all[i - 1].x, p.z - all[i - 1].z);
      p.u = u; p.y = ys[i] + 0.55;
    });
    for (let i = 0; i < all.length; i++) {
      const a = all[Math.max(0, i - 1)], b = all[Math.min(all.length - 1, i + 1)];
      const dx = b.x - a.x, dz = b.z - a.z, l = Math.hypot(dx, dz) || 1;
      all[i].fx = dx / l; all[i].fz = dz / l;
    }
    this.rail = { pts: all, len: u, back: back.length * 5 };
  }

  railAt(u, out = {}) {
    const P = this.rail.pts;
    const i = clamp(Math.floor(u / 5), 0, P.length - 2);
    const f = clamp((u - P[i].u) / Math.max(1e-3, P[i + 1].u - P[i].u), 0, 1);
    const a = P[i], b = P[i + 1];
    out.x = lerp(a.x, b.x, f); out.y = lerp(a.y, b.y, f); out.z = lerp(a.z, b.z, f);
    out.fx = lerp(a.fx, b.fx, f); out.fz = lerp(a.fz, b.fz, f);
    return out;
  }

  buildRailway() {
    this.railPath();
    const P = this.rail.pts;
    // Profile across: [lat, dy]. Ballast shoulders run down into the ground.
    const sweep = (prof, mat, { uS = 1, vS = 4 } = {}) => {
      const pos = [], uv = [], idx = [];
      const n = prof.length;
      P.forEach((p, i) => {
        const rx = -p.fz, rz = p.fx;
        for (let k = 0; k < n; k++) {
          const [lat, dy, uu] = prof[k];
          pos.push(p.x + rx * lat, p.y + dy, p.z + rz * lat);
          uv.push(uu ?? lat / uS, p.u / vS);
        }
        if (i < P.length - 1) for (let k = 0; k < n - 1; k++) {
          const a = i * n + k, b = a + 1, c = a + n, d = c + 1;
          idx.push(a, c, b, b, c, d);
        }
      });
      const g = new THREE.BufferGeometry();
      g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
      g.setAttribute('uv', new THREE.Float32BufferAttribute(uv, 2));
      g.setIndex(idx);
      g.computeVertexNormals();
      g.computeBoundingSphere();
      const m = new THREE.Mesh(g, mat);
      m.receiveShadow = true;
      m.matrixAutoUpdate = false;
      this.group.add(m);
      return m;
    };
    // Ties texture: dark sleepers across pale ballast.
    const cv = document.createElement('canvas');
    cv.width = 64; cv.height = 256;
    const g = cv.getContext('2d');
    g.fillStyle = '#8c7c68'; g.fillRect(0, 0, 64, 256);
    const rng = mulberry32(5);
    for (let i = 0; i < 900; i++) { const v = 90 + rng() * 70; g.fillStyle = `rgb(${v},${v * 0.9},${v * 0.78})`; g.fillRect(rng() * 64, rng() * 256, 2, 2); }
    for (let y = 0; y < 256; y += 16) { g.fillStyle = '#3a2e26'; g.fillRect(6, y + 2, 52, 9); g.fillStyle = 'rgba(0,0,0,0.25)'; g.fillRect(6, y + 10, 52, 2); }
    const tex = new THREE.CanvasTexture(cv);
    tex.wrapS = tex.wrapT = THREE.RepeatWrapping;
    tex.colorSpace = THREE.SRGBColorSpace;
    tex.anisotropy = 8;
    const bed = new THREE.MeshStandardMaterial({ map: tex, roughness: 1 });
    sweep([[2.6, 0.02, 0], [-2.6, 0.02, 1]], bed, { vS: 10.4 });
    sweep([[5.6, -2.5], [2.6, 0.0], [-2.6, 0.0], [-5.6, -2.5]].reverse().map(([a, b]) => [a, b - 0.02]), this.M.ballast, { uS: 3, vS: 3 });
    for (const lat of [0.72, -0.72]) {
      sweep([[lat + 0.05, 0.2], [lat + 0.05, 0.05], [lat - 0.05, 0.05], [lat - 0.05, 0.2], [lat + 0.05, 0.2]].reverse(), this.M.rail);
    }
  }

  // ── Freight train pacing the player ───────────────────────────────
  buildTrain() {
    if (!this.rail) return;
    const cars = [];
    const add = (spec) => cars.push(spec);
    // Two units up front in the road's colours, a third in another
    // railroad's paint (a borrowed unit, as real consists often have).
    const loco = locomotiveGeometry(0), loco2 = locomotiveGeometry(1);
    add({ ...loco, key: 'loco' }); add({ ...loco, key: 'loco' }); add({ ...loco2, key: 'loco2' });
    const types = [
      boxcarGeometry(0x7a3a28, 1), boxcarGeometry(0x4a4e54, 2), boxcarGeometry(0x8a6a3a, 3), boxcarGeometry(0x2f5a7a, 14),
      tankGeometry(0x1e1e20, 4), tankGeometry(0xe0ddd4, 5), hopperGeometry(0x8a8e92, 6), hopperGeometry(0xb8b0a0, 7),
      stackGeometry([0x1d4f8a, 0xb3342b, 0xd9d4c7], 8), stackGeometry([0x2f7d3a, 0xe0b52b, 0x8a8f96], 9), stackGeometry([0xd8612a, 0x36414d, 0x1f7f86], 10),
      gondolaGeometry(0x5a3a2a, 11, 'scrap'), gondolaGeometry(0x3a3a3e, 15, 'ballast'), autorackGeometry(0xb8b4ac, 12), lumberGeometry(13),
    ];
    const rng = mulberry32(2718);
    // Cars run in short blocks of one type, like a real manifest freight.
    for (let i = 0; i < 28;) {
      const k = Math.floor(rng() * types.length);
      for (let n = 1 + Math.floor(rng() * 3); n > 0 && i < 28; n--, i++) add({ ...types[k], key: 'c' + k });
    }
    // One InstancedMesh per car type.
    const groups = new Map();
    let off = 0;
    this.trainCars = cars.map((c) => {
      if (!groups.has(c.key)) groups.set(c.key, { geo: c.geo, list: [] });
      const e = { off: off + c.L / 2, L: c.L, key: c.key, idx: groups.get(c.key).list.length, flip: false };
      groups.get(c.key).list.push(e);
      off += c.L + 1.2;
      return e;
    });
    this.trainLen = off;
    this.trainMeshes = new Map();
    for (const [key, gr] of groups) {
      const im = new THREE.InstancedMesh(gr.geo, this.M.solid, gr.list.length);
      im.castShadow = true;
      im.receiveShadow = true;
      im.frustumCulled = false;
      this.group.add(im);
      this.trainMeshes.set(key, im);
    }
    // Headlight glow sprite and ditch lights on the lead unit.
    const gm = new THREE.SpriteMaterial({ map: glowTexture(), color: 0xfff0c8, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending });
    this.trainGlow = [0, 1, 2].map(() => { const sp = new THREE.Sprite(gm); sp.scale.setScalar(4); this.group.add(sp); return sp; });
    this.trainGlowMat = gm;
    const beam = new THREE.SpotLight(0xfff0d0, 0, 220, 0.3, 0.6, 1.3);
    beam.castShadow = false;
    this.group.add(beam, beam.target);
    this.trainBeam = beam;
    // Where the train is: parked out ahead until the player nears the
    // highway, then rolling at a steady 27 m/s.
    this.trainU = null;
    this.trainSpeed = 27;
  }

  // Road s → rail u (by projection on the nearest rail point).
  railUFor(s) {
    const f = this.t.frame(s);
    const x = f.x + f.rx * RAIL_LAT, z = f.z + f.rz * RAIL_LAT;
    const P = this.rail.pts;
    let best = 0, bd = Infinity;
    for (let i = 0; i < P.length; i += 4) { const d = Math.pow(P[i].x - x, 2) + Math.pow(P[i].z - z, 2); if (d < bd) { bd = d; best = i; } }
    return P[best].u;
  }

  updateTrain(dt, night, s) {
    if (!this.trainCars) return;
    if (this._uCache === undefined || Math.abs(s - this._sCache) > 20) { this._uCache = this.railUFor(s); this._sCache = s; }
    const pu = this._uCache + (s - this._sCache);
    const start = this.Z[1].s0 - 500;
    if (s < start || this.trainU === null) {
      // Waiting: the whole train sits ahead of the player, tail 150 m ahead.
      this.trainU = pu + this.trainLen + 150;
      if (s < start) this._rolling = false;
    }
    if (s >= start) { this._rolling = true; }
    if (this._rolling) this.trainU += this.trainSpeed * dt;
    const head = this.trainU;
    const m4 = new THREE.Matrix4(), q = new THREE.Quaternion(), p = new THREE.Vector3(), one = new THREE.Vector3(1, 1, 1);
    const A = {}, Bp = {};
    const upd = new Set();
    for (const c of this.trainCars) {
      const u = head - c.off;
      const im = this.trainMeshes.get(c.key);
      if (u < c.L || u > this.rail.len - c.L) {
        m4.makeScale(0, 0, 0);
        im.setMatrixAt(c.idx, m4);
      } else {
        this.railAt(u + c.L * 0.36, A);
        this.railAt(u - c.L * 0.36, Bp);
        const dx = A.x - Bp.x, dy = A.y - Bp.y, dz = A.z - Bp.z;
        const yaw = Math.atan2(-dz, dx), pitch = Math.atan2(dy, Math.hypot(dx, dz));
        q.setFromEuler(new THREE.Euler(0, yaw, pitch, 'YZX'));
        p.set((A.x + Bp.x) / 2, (A.y + Bp.y) / 2 + 0.25, (A.z + Bp.z) / 2);
        m4.compose(p, q, one);
        im.setMatrixAt(c.idx, m4);
      }
      upd.add(im);
    }
    for (const im of upd) im.instanceMatrix.needsUpdate = true;
    // Lights on the lead locomotive.
    const lead = this.trainCars[0];
    const lu = head - lead.off + lead.L / 2;
    const F = this.railAt(lu, {});
    const rx = -F.fz, rz = F.fx;
    const k = 0.25 + 0.75 * smoothstep(0.15, 0.6, night);
    this.trainGlow[0].position.set(F.x + F.fx * 0.4, F.y + 3.45, F.z + F.fz * 0.4);
    this.trainGlow[1].position.set(F.x + F.fx * 0.3 + rx * 1.0, F.y + 1.6, F.z + F.fz * 0.3 + rz * 1.0);
    this.trainGlow[2].position.set(F.x + F.fx * 0.3 - rx * 1.0, F.y + 1.6, F.z + F.fz * 0.3 - rz * 1.0);
    const blink = Math.sin(this.time * 6) > 0;
    this.trainGlow[0].scale.setScalar(3 + 4 * k);
    this.trainGlow[1].scale.setScalar((blink ? 2.5 : 1.2) * (0.5 + k));
    this.trainGlow[2].scale.setScalar((!blink ? 2.5 : 1.2) * (0.5 + k));
    this.trainGlowMat.opacity = 0.5 + 0.5 * k;
    this.trainBeam.position.set(F.x + F.fx * 0.5, F.y + 4.2, F.z + F.fz * 0.5);
    this.trainBeam.target.position.set(F.x + F.fx * 40, F.y, F.z + F.fz * 40);
    this.trainBeam.target.updateMatrixWorld();
    this.trainBeam.intensity = 90 * smoothstep(0.3, 0.8, night);
  }

  // ── The Oasis: motel, diner and gas station ───────────────────────
  buildOasis() {
    if (!this.oasis) return;
    const t = this.t, B = this.B, rng = mulberry32(66);
    const s = this.oasis.s;
    const f = t.frame(s);
    const yaw = yawZ(-f.rx, -f.rz);          // buildings' fronts face the road
    const along = (ds, lat) => {
      const fr = t.frame(s + ds);
      const x = fr.x + fr.rx * lat, z = fr.z + fr.rz * lat;
      return { x, z, y: f.y - 0.25, yaw };
    };
    const lot = along(0, f.wallR + 34);
    // Gravel lot and an apron from the road.
    B.setFrame(lot.x, lot.y, lot.z, yaw);
    B.box('asphaltLot', 150, 0.08, 52, 0, 0.0, 0);
    // Gas station nearest the approach, the diner in the middle, motel on.
    const gp = along(-48, f.wallR + 20);
    B.setFrame(gp.x, gp.y + 0.05, gp.z, yaw);
    gasStation(B, rng, { signRect: this.sg.gas, priceRect: this.sg.price });
    const dp = along(-2, f.wallR + 24);
    B.setFrame(dp.x, dp.y + 0.05, dp.z, yaw);
    diner(B, rng, this.nn.diner);
    const mp = along(52, f.wallR + 18);
    B.setFrame(mp.x, mp.y + 0.05, mp.z, yaw);
    motel(B, rng, { w: 44, neonRect: this.nn.vacancy, wall: 'wPeach' });
    // The big sign: a tall pole carrying the OASIS board and an arrow of
    // chaser bulbs pointing at the lot.
    const sp = along(-18, f.wallR + 6);
    B.setFrame(sp.x, sp.y, sp.z, yawZ(f.fx * -1, f.fz * -1) + 0.5);
    B.box('steel', 0.5, 13, 0.5, -1.5, 0, 0);
    B.box('steel', 0.5, 13, 0.5, 1.5, 0, 0);
    B.box('black', 9.4, 3.2, 0.4, 0, 9.6, 0);
    B.put('neon', signGeometry(this.nn.oasis, 9.0, 2.9), 0, 11.2, 0.22);
    B.put('neon', signGeometry(this.nn.oasis, 9.0, 2.9), 0, 11.2, -0.22, 0, Math.PI, 0);
    B.box('black', 7, 1.1, 0.35, -0.5, 7.6, 0);
    B.put('neon', signGeometry(this.nn.eat, 2.1, 1.0), -2.5, 8.15, 0.2);
    this.oasis.sign = { x: sp.x, y: sp.y, z: sp.z, yaw: yawZ(f.fx * -1, f.fz * -1) + 0.5 };
    // Palms round the lot and along the frontage.
    this.palms = [];
    for (let k = 0; k < 16; k++) {
      const p = along(-70 + k * 9.5 + (rng() - 0.5) * 3, f.wallR + 7 + (k % 2) * 2.5 + (k % 5 === 0 ? 38 : 0));
      this.palms.push({ x: p.x, y: p.y, z: p.z, sx: rrange(rng, 0.85, 1.15), ry: rng() * 6.3 });
    }
    for (let k = 0; k < 6; k++) {
      const p = along(20 + k * 12, f.wallR + 48);
      this.palms.push({ x: p.x, y: p.y, z: p.z, sx: rrange(rng, 0.9, 1.2), ry: rng() * 6.3 });
    }
    this.addInstanced(palmGeometry(5), this.M.veg, [this.palms.filter((_, i) => i % 2 === 0)], { cast: true });
    this.addInstanced(palmGeometry(9), this.M.veg, [this.palms.filter((_, i) => i % 2 === 1)], { cast: true });
    // Cars on the lot, big rigs parked by the diner.
    this.cars = this.cars || [];
    const kinds = ['sedan', 'pickup', 'van', 'hatch', 'pickup', 'sedan'];
    const COLS = [0xc9ccd1, 0x8a1c1c, 0x2e6d8e, 0xe8e6df, 0x4d5a3a, 0x9aa3ad, 0xb5a27a];
    for (let k = 0; k < 9; k++) {
      const p = along(30 + k * 4.1, f.wallR + 26);
      this.cars.push({ kind: kinds[k % kinds.length], color: rpick(rng, COLS), seed: k, x: p.x, y: p.y + 0.05, z: p.z, yaw: yaw + Math.PI });
    }
    for (let k = 0; k < 3; k++) {
      const p = along(-20 + k * 7, f.wallR + 38);
      this.cars.push({ kind: 'boxtruck', color: rpick(rng, [0xe8e6df, 0x8a1c1c, 0x1d3f75]), seed: 20 + k, x: p.x, y: p.y + 0.05, z: p.z, yaw: yawZ(f.fx, f.fz) + 1.2 });
    }
    // Lot lights: pools of warm light on the gravel.
    this.pools = this.pools || [];
    for (let k = 0; k < 6; k++) {
      const p = along(-60 + k * 24, f.wallR + 14);
      B.setFrame(p.x, p.y, p.z, yaw);
      B.box('steel', 0.2, 7, 0.2, 0, 0, 0);
      B.box('steel', 1.4, 0.15, 0.2, 0.6, 6.9, 0);
      B.box('lamp', 0.8, 0.12, 0.4, 1.1, 6.75, 0);
      this.pools.push({ x: p.x, y: p.y + 0.12, z: p.z, r: 12, c: [0.5, 0.36, 0.2] });
    }
    // Under the canopy.
    const cp = along(-48, f.wallR + 13);
    this.pools.push({ x: cp.x, y: cp.y + 0.14, z: cp.z, r: 16, c: [0.5, 0.55, 0.6] });
    this.oasisExtras(along, yaw, f, rng);
  }

  // Dressing round the Oasis: water tower, the MOTEL arrow, vending
  // machines, vintage pumps, a shade ramada, string lights and chaser bulbs.
  oasisExtras(along, yaw, f, rng) {
    const B = this.B, t = this.t;
    const W = (p, ry, lx, ly, lz) => ({ x: p.x + lx * Math.cos(ry) + lz * Math.sin(ry), y: p.y + ly, z: p.z - lx * Math.sin(ry) + lz * Math.cos(ry) });
    this.bulbs = this.bulbs || [];
    this.strings = this.strings || [];
    // Water tower behind the diner, the town name on the tank.
    const wt = along(8, f.wallR + 66);
    B.setFrame(wt.x, wt.y, wt.z, yaw);
    const TH = 15;
    for (const [lx, lz] of [[-2.6, -2.6], [2.6, -2.6], [-2.6, 2.6], [2.6, 2.6]]) B.beam('steel', new THREE.Vector3(lx * 1.25, 0, lz * 1.25), new THREE.Vector3(lx, TH, lz), 0.28);
    for (const h of [4, 9]) for (const [a, b] of [[[-1, -1], [1, -1]], [[1, -1], [1, 1]], [[1, 1], [-1, 1]], [[-1, 1], [-1, -1]]]) {
      const k = 1.25 - 0.25 * (h / TH);
      B.beam('steel', new THREE.Vector3(a[0] * 2.6 * k, h, a[1] * 2.6 * k), new THREE.Vector3(b[0] * 2.6 * k, h + 4, b[1] * 2.6 * k), 0.1);
    }
    B.beam('steel', new THREE.Vector3(0, 0, 0), new THREE.Vector3(0, TH, 0), 0.5);
    B.put('white', new THREE.CylinderGeometry(3.8, 3.8, 5.5, 20), 0, TH + 2.75, 0);
    B.put('roofTar', new THREE.ConeGeometry(4.0, 2.0, 20), 0, TH + 6.5, 0);
    B.put('white', new THREE.CylinderGeometry(4.2, 4.2, 0.2, 20), 0, TH + 0.05, 0);
    B.put('signs2', signGeometry(this.sg.tower, 5.2, 1.62), 0, TH + 2.9, 3.86);
    // MOTEL arrow on its own pole at the motel end of the frontage.
    const ma = along(78, f.wallR + 7);
    const may = yawZ(-f.fx, -f.fz) + 0.6;
    B.setFrame(ma.x, ma.y, ma.z, may);
    B.box('steel', 0.3, 7.2, 0.3, 0, 0, 0);
    B.box('black', 5.2, 2.7, 0.3, 0, 6.2, 0);
    B.put('neon', signGeometry(this.nn.motelArrow, 5.0, 2.5), 0, 7.55, 0.17);
    B.put('neon', signGeometry(this.nn.motelArrow, 5.0, 2.5), 0, 7.55, -0.17, 0, Math.PI, 0);
    // Chaser bulbs round the big OASIS board and the arrow.
    const sg = this.oasis.sign;
    const ring = (p, ry, cx, cy, w, h, step, face) => {
      let i = 0;
      const per = 2 * (w + h), n = Math.floor(per / step);
      for (let k = 0; k < n; k++) {
        let d = k * step, lx, ly;
        if (d < w) { lx = -w / 2 + d; ly = h / 2; } else if ((d -= w) < h) { lx = w / 2; ly = h / 2 - d; } else if ((d -= h) < w) { lx = w / 2 - d; ly = -h / 2; } else { d -= w; lx = -w / 2; ly = -h / 2 + d; }
        for (const fz of face) this.bulbs.push({ ...W(p, ry, cx + lx, cy + ly, fz), ph: i });
        i++;
      }
    };
    ring(sg, sg.yaw, 0, 11.2, 9.6, 3.4, 0.42, [0.26, -0.26]);
    ring(sg, sg.yaw, -0.5, 8.15, 7.2, 1.3, 0.42, [0.24]);
    ring(ma, may, 0, 7.55, 5.4, 2.9, 0.4, [0.2, -0.2]);
    // Vending and ice machines by the motel office and the gas kiosk.
    for (const [ds, lat, kind] of [[36, f.wallR + 11, 'soda'], [37.4, f.wallR + 11, 'soda'], [38.9, f.wallR + 11, 'ice'], [-40, f.wallR + 37, 'soda'], [-41.4, f.wallR + 37, 'ice']]) {
      const p = along(ds, lat);
      B.setFrame(p.x, p.y + 0.05, p.z, yaw);
      if (kind === 'soda') {
        B.box('paintRed', 1.0, 1.9, 0.8, 0, 0, 0);
        B.put('signs2', signGeometry(this.sg.soda, 0.9, 1.75), 0, 0.97, 0.41);
      } else {
        B.box('white', 1.3, 1.8, 0.9, 0, 0, 0);
        B.cbox('canvasB', 1.32, 0.35, 0.92, 0, 1.3, 0);
        B.cbox('glassLit', 0.9, 0.3, 0.05, 0, 1.3, 0.46);
      }
    }
    // Vintage visible-register pumps kept out front of the diner.
    for (const k of [-1, 1]) {
      const p = along(-2 + k * 3.5, f.wallR + 16.5);
      B.setFrame(p.x, p.y + 0.05, p.z, yaw);
      B.box('concrete', 1.1, 0.25, 1.1, 0, 0, 0);
      B.box('paintRed', 0.6, 2.2, 0.5, 0, 0.25, 0);
      B.cbox('white', 0.62, 0.35, 0.52, 0, 1.6, 0);
      B.put('lamp', new THREE.SphereGeometry(0.28, 10, 6), 0, 2.75, 0);
      B.box('black', 0.05, 0.9, 0.05, 0.33, 0.9, 0.1);
    }
    // Shade ramada with picnic tables on the diner's far side.
    const rp = along(22, f.wallR + 30);
    B.setFrame(rp.x, rp.y + 0.05, rp.z, yaw);
    for (const [px, pz] of [[-4, -2.5], [4, -2.5], [-4, 2.5], [4, 2.5]]) B.box('woodDark', 0.25, 2.9, 0.25, px, 0, pz);
    for (let k = 0; k < 11; k++) B.box('wood', 9.4, 0.08, 0.28, 0, 2.95, -2.8 + k * 0.56);
    for (const [px, pz] of [[-2.2, 0], [2.2, 0]]) {
      B.box('wood', 1.8, 0.08, 0.8, px, 0.75, pz);
      for (const dz of [-0.65, 0.65]) B.box('wood', 1.8, 0.06, 0.3, px, 0.45, pz + dz);
      for (const sx of [-0.7, 0.7]) B.box('woodDark', 0.1, 0.75, 1.5, px + sx, 0, pz);
    }
    // Stack of old tyres and a trash barrel by the kiosk.
    const tp = along(-58, f.wallR + 30);
    for (let k = 0; k < 5; k++) {
      B.setFrame(tp.x, tp.y + k * 0.28, tp.z, k * 0.4);
      B.put('tire', new THREE.TorusGeometry(0.34, 0.14, 5, 10), 0, 0.14, 0, Math.PI / 2, 0, 0);
    }
    // Festoon lights zig-zagging over the diner forecourt.
    const posts = [];
    for (let k = 0; k < 5; k++) {
      const p = along(-14 + k * 7, f.wallR + (k % 2 ? 11 : 17));
      B.setFrame(p.x, p.y, p.z, yaw);
      B.box('woodDark', 0.18, 5, 0.18, 0, 0, 0);
      posts.push({ x: p.x, y: p.y + 4.9, z: p.z });
    }
    for (let k = 0; k < posts.length - 1; k++) {
      const a = posts[k], b = posts[k + 1];
      const n = 12;
      for (let i = 1; i < n; i++) {
        const u = i / n;
        this.strings.push({ x: lerp(a.x, b.x, u), y: lerp(a.y, b.y, u) - 0.9 * 4 * u * (1 - u), z: lerp(a.z, b.z, u), ph: i + k * 5 });
      }
    }
  }

  // ── Road signs, billboards, DIP warnings ──────────────────────────
  buildRoadSigns() {
    const t = this.t;
    for (const tag of t.tag('dip')) this.roadSign(tag.s0 - 110, 1, this.sg.dip, 1.1, 1.1, { y0: 1.4 });
    this.roadSign(260, 1, this.sg.rocks, 1.1, 1.1, { y0: 1.4 });
    this.roadSign(1500, 1, this.sg.flood, 1.1, 1.1, { y0: 1.4 });
    this.roadSign(this.Z[1].s0 + 60, 1, this.sg.route, 0.9, 0.9, { y0: 1.5 });
    // Billboards on wooden legs, angled toward oncoming drivers.
    const B = this.B;
    const boards = [
      [this.Z[1].s0 + 140, this.sg.bbGas, 1, 30],
      [this.oasis ? this.oasis.s - 420 : this.Z[1].s0 + 600, this.sg.bbOasis, 1, 26],
      [this.Z[1].s0 + 1450, this.sg.bbJerky, -1, 28],
      [this.Z[2].s0 - 380, this.sg.bbTrials, 1, 24],
      [this.Z[1].s0 + 420, this.sg.bbSnakes, -1, 26, 'signs2'],
      [this.Z[1].s0 + 1150, this.sg.bbPie, 1, 22, 'signs2'],
      [this.Z[1].s0 + 1800, this.sg.bbMotor, 1, 32, 'signs2'],
      [this.Z[1].s0 + 2150, this.sg.bbDino, -1, 24, 'signs2'],
      [this.Z[1].s0 + 950, this.sg.bbFaded, -1, 40, 'signs2', true],
    ];
    this.lampPools = [];
    boards.forEach(([s, rect, side, off, key = 'signs', wreck = false], i) => {
      const f = t.frame(s);
      let lat = side * ((side > 0 ? f.wallR : f.wallL) + off);
      // The railway runs at RAIL_LAT on the left: stand boards clear of it.
      if (side < 0 && Math.abs(lat - RAIL_LAT) < 14) lat = RAIL_LAT + 16;
      const x = f.x + f.rx * lat, z = f.z + f.rz * lat, y = this.gy(x, z);
      const ry = yawZ(-f.fx, -f.fz) + side * 0.35;
      B.setFrame(x, y, z, ry);
      for (const px of [-4, 0, 4]) B.box('wood', 0.3, 5.2, 0.3, px, -0.3, -0.4);
      B.box('woodDark', 11, 0.2, 1.2, 0, 3.7, -0.3);
      // Catwalk and diagonal bracing behind the face.
      for (const px of [-4, 4]) B.box('woodDark', 0.14, 4.4, 0.14, px, -0.2, -2.3, 0, 0.42, 0);
      if (wreck) {
        // An abandoned board: the face has slumped on its legs.
        B.put(key, signGeometry(rect, 10.6, 4.0), 0.3, 6.3, 0, 0, 0, -0.08);
        B.box('woodDark', 10.8, 4.2, 0.12, 0.3, 4.2, -0.1, 0, 0, -0.08);
      } else {
        B.put(key, signGeometry(rect, 10.6, 4.0), 0, 6.8, 0);
        B.box('woodDark', 10.8, 4.2, 0.12, 0, 4.7, -0.1);
        // Gooseneck lamps along the top: they light the face at night.
        for (const px of [-3.6, 0, 3.6]) {
          B.box('steel', 0.08, 0.08, 1.0, px, 8.95, 0.35);
          B.box('lamp', 0.5, 0.14, 0.3, px, 8.85, 0.85);
          const lx = x + Math.cos(ry) * px + Math.sin(ry) * 0.9, lz = z - Math.sin(ry) * px + Math.cos(ry) * 0.9;
          this.lampPools.push({ x: lx, y: y + 8.6, z: lz, ph: i * 3 + px });
        }
      }
      this.take(x, z, 7);
    });
  }

  // ── Roadside life along Route 66 ──────────────────────────────────
  // Delineators and mile markers, abandoned wrecks out in the scrub, and
  // tumbleweeds — a few caught on the fence, a few blowing across the road.
  buildRoadside() {
    const t = this.t, rng = mulberry32(6606), S = {};
    const posts = [];
    this.reflectors = this.reflectors || [];
    const s0 = this.Z[1].s0 - 60, s1 = this.Z[2].s0 + 150;
    for (let s = s0, k = 0; s < s1; s += 64, k++) {
      if (this.oasis && Math.abs(s - this.oasis.s) < 80) continue;
      t.frame(s, S);
      for (const side of [-1, 1]) {
        const lat = side * ((side > 0 ? S.wallR : S.wallL) - 0.35);
        const x = S.x + S.rx * lat, z = S.z + S.rz * lat;
        posts.push({ x, y: this.gy(x, z) - 0.05, z, sx: 1, ry: yawZ(-S.fx, -S.fz) });
        this.reflectors.push({ x: x - S.fx * 0.05, y: this.gy(x, z) + 1.0, z: z - S.fz * 0.05 });
      }
    }
    this.addInstanced(delineatorGeometry(), this.M.veg, [posts]);
    // Mile markers on the right, one a mile, all the way to the lake.
    for (let k = 0; k < 5; k++) {
      const s = 700 + k * 1609;
      if (s > this.t.finishS - 200) break;
      const f = t.frame(s);
      this.roadSign(s, 1, this.sg.mm[k], 0.36, 0.96, { lat: f.wallR + 1.2, y0: 0.9, key: 'signs2' });
    }
    // Wrecks: rusted-out cars abandoned in the desert, sunk to the sills.
    this.cars = this.cars || [];
    const RUST = [0x7a4a30, 0x6a4030, 0x8a5a3a, 0x5a4a3e, 0x7a6a50, 0x6e5a4a];
    const wrecks = [
      [this.Z[1].s0 + 330, 1, 28, 'sedan'], [this.Z[1].s0 + 700, -1, 22, 'pickup'], [this.Z[1].s0 + 1320, 1, 60, 'hatch'],
      [this.Z[1].s0 + 1620, 1, 18, 'van'], [this.Z[1].s0 + 2050, -1, 34, 'sedan'], [this.Z[2].s0 - 120, 1, 40, 'pickup'],
      [(this.oasis?.s ?? 3500) + 34, 1, 70, 'sedan'], [(this.oasis?.s ?? 3500) + 40, 1, 74, 'hatch'],
    ];
    for (const [s, side, off, kind] of wrecks) {
      const f = t.frame(s);
      let lat = side * ((side > 0 ? f.wallR : f.wallL) + off);
      if (Math.abs(lat - RAIL_LAT) < 12) lat -= 16;
      const x = f.x + f.rx * lat, z = f.z + f.rz * lat;
      if (!this.clearOfRoad(x, z, 3) || !this.free(x, z, 2.5)) continue;
      this.take(x, z, 3);
      this.cars.push({ kind, color: rpick(rng, RUST), seed: 100 + Math.floor(rng() * 50), x, y: this.gy(x, z) - 0.25, z, yaw: rng() * 6.3, rust: true, pitch: (rng() - 0.5) * 0.12, roll: (rng() - 0.5) * 0.14 });
    }
    // Tumbleweeds at rest: out in the scrub and against the fence.
    const still = (this.weeds || []).concat(this.fenceWeeds || []);
    const tg = tumbleweedGeometry(3);
    this.addInstanced(tg, this.M.bush, [still.map((w) => ({ ...w, col: 0xffffff }))]);
    // Rolling ones, recycled round the player (see updateWeeds).
    const N = 7;
    const im = new THREE.InstancedMesh(tg, this.M.bush, N);
    im.frustumCulled = false;
    im.castShadow = true;
    const m4 = new THREE.Matrix4().makeScale(0, 0, 0);
    for (let i = 0; i < N; i++) im.setMatrixAt(i, m4);
    this.group.add(im);
    this.rollers = { im, list: Array.from({ length: N }, (_, i) => ({ live: false, i, q: new THREE.Quaternion() })) };
  }

  // ── The lake bed: a pale surface replaces the asphalt ────────────
  buildLakebed() {
    const t = this.t;
    const road = this.world.road?.group;
    if (!road) return;
    // Cracked mud: a canvas of polygon cracks.
    const S = 512;
    const cv = document.createElement('canvas');
    cv.width = cv.height = S;
    const g = cv.getContext('2d');
    g.fillStyle = '#d6cfc0'; g.fillRect(0, 0, S, S);
    const rng = mulberry32(81);
    for (let i = 0; i < 6000; i++) { const v = 190 + rng() * 40; g.fillStyle = `rgba(${v},${v * 0.96},${v * 0.9},0.35)`; g.fillRect(rng() * S, rng() * S, 2, 2); }
    // Voronoi-ish cracks from jittered cells (tileable by wrapping).
    const cells = [];
    for (let j = 0; j < 12; j++) for (let i = 0; i < 12; i++) cells.push([(i + 0.1 + rng() * 0.8) * S / 12, (j + 0.1 + rng() * 0.8) * S / 12]);
    const img = g.getImageData(0, 0, S, S);
    for (let y = 0; y < S; y++) for (let x = 0; x < S; x++) {
      let d1 = 1e9, d2 = 1e9;
      for (const [cx, cy] of cells) {
        let dx = Math.abs(x - cx), dy = Math.abs(y - cy);
        dx = Math.min(dx, S - dx); dy = Math.min(dy, S - dy);
        const d = dx * dx + dy * dy;
        if (d < d1) { d2 = d1; d1 = d; } else if (d < d2) d2 = d;
      }
      const e = Math.sqrt(d2) - Math.sqrt(d1);
      if (e < 1.8) {
        const k = (y * S + x) * 4, a = 1 - e / 1.8;
        img.data[k] -= 48 * a; img.data[k + 1] -= 50 * a; img.data[k + 2] -= 52 * a;
      }
    }
    g.putImageData(img, 0, 0);
    const tex = new THREE.CanvasTexture(cv);
    tex.wrapS = tex.wrapT = THREE.RepeatWrapping;
    tex.colorSpace = THREE.SRGBColorSpace;
    tex.anisotropy = 8;
    tex.repeat.set(4.4, 2.5);
    const mat = new THREE.MeshStandardMaterial({ map: tex, roughness: 0.95, color: 0xffffff });
    this.lakeMat = mat;
    const P = {};
    const lakeFrom = this.Z[2].s0 + 120;
    for (const m of road.children) {
      if (!m.isMesh || !m.material?.map || !/asphalt/.test(Object.keys(this.world.road.materials).find((k) => this.world.road.materials[k] === m.material) || '')) continue;
      m.geometry.computeBoundingSphere();
      const c = m.geometry.boundingSphere.center;
      const hint = t.nearest(c.x, c.z, 400);
      if (hint < 0) continue;
      t.project(c.x, c.z, hint, P, 30);
      if (P.s > lakeFrom) m.material = mat;
    }
    // Shoulders on the lake: the same mud (they're tinted gravel elsewhere).
    const shoulder = this.world.road.materials.shoulder;
    for (const m of road.children) {
      if (!m.isMesh || m.material !== shoulder) continue;
      m.geometry.computeBoundingSphere();
      const c = m.geometry.boundingSphere.center;
      const hint = t.nearest(c.x, c.z, 400);
      if (hint < 0) continue;
      t.project(c.x, c.z, hint, P, 30);
      if (P.s > lakeFrom) m.material = mat;
    }
  }

  // ── Course marking on the lake: cones, flags, flares, mile boards ─
  buildCourse() {
    const t = this.t, S = {};
    const s0 = this.Z[2].s0 + 180, s1 = t.roadEnd - 2;
    const cones = [], flagPoles = [], flags = [];
    this.flares = [];
    for (const side of [-1, 1]) {
      for (let s = s0, k = 0; s < s1; s += 9, k++) {
        t.frame(s, S);
        const lat = side * ((side > 0 ? S.wallR : S.wallL) + 0.3);
        const x = S.x + S.rx * lat, z = S.z + S.rz * lat, y = S.y - 0.04;
        cones.push({ x, y, z, sx: 1, ry: k * 0.7 });
        if (k % 6 === 0) {
          const fl = side * ((side > 0 ? S.wallR : S.wallL) + 1.2);
          const fx = S.x + S.rx * fl, fz = S.z + S.rz * fl;
          flagPoles.push({ x: fx, y: y - 0.05, z: fz, sx: 1 });
          flags.push({ x: fx, y: y + 3.2, z: fz, sx: 1, ry: yawX(-S.fx, -S.fz), col: (k / 6) % 2 ? 0xff5a14 : 0xf2f2f2 });
        }
        if (k % 3 === 1) this.flares.push({ x: x + S.fx * 4, y: y + 0.12, z: z + S.fz * 4, ph: hash2(k, side) * 10 });
      }
    }
    this.addInstanced(coneGeometry(), this.M.veg, [cones]);
    const pole = paint(prep(new THREE.CylinderGeometry(0.03, 0.04, 3.6, 5)), 0xdddddd);
    pole.translate(0, 1.8, 0);
    this.addInstanced(pole, this.M.veg, [flagPoles]);
    const flagG = new THREE.BufferGeometry();
    flagG.setAttribute('position', new THREE.Float32BufferAttribute([0, 0.35, 0, 0, -0.35, 0, 1.2, 0.05, 0], 3));
    flagG.computeVertexNormals();
    paint(flagG, 0xffffff);
    const flagMat = new THREE.MeshStandardMaterial({ vertexColors: true, side: THREE.DoubleSide, roughness: 0.8 });
    this.addInstanced(flagG, flagMat, [flags]);
    // Distance boards before the finish, on the right.
    const marks = [1609, 805, 402];
    marks.forEach((d, i) => {
      const s = t.finishS - d;
      if (s > s0) this.roadSign(s, 1, this.sg.mile[i], 2.4, 1.2, { posts: 2, y0: 1.0, lat: t.frame(s).wallR + 3 });
    });
    this.roadSign(this.Z[2].s0 + 120, 1, this.sg.lake, 4.2, 1.4, { posts: 2, y0: 1.2, lat: t.frame(this.Z[2].s0 + 120).wallR + 4 });
    // End of the course: hay bales and a tyre wall across the lake bed.
    const B = this.B, fe = t.frame(t.roadEnd - 1);
    const yaw = yawZ(-fe.fx, -fe.fz);
    for (let lat = -fe.wallL - 3; lat <= fe.wallR + 3; lat += 1.3) {
      const x = fe.x + fe.rx * lat + fe.fx * 1.5, z = fe.z + fe.rz * lat + fe.fz * 1.5;
      B.setFrame(x, fe.y - 0.05, z, yaw);
      B.box('hay', 1.2, 0.9, 0.9, 0, 0, 0);
      if (Math.round(lat / 1.3) % 2 === 0) B.box('hay', 1.2, 0.9, 0.9, 0, 0.9, 0.05);
      B.box('tire', 1.1, 1.1, 0.8, 0, 0, 1.3);
    }
    const ep = { x: fe.x + fe.fx * 3, z: fe.z + fe.fz * 3 };
    B.setFrame(ep.x, fe.y, ep.z, yaw);
    for (const px of [-2.2, 2.2]) B.box('steel', 0.12, 3.2, 0.12, px, 0, -0.05);
    B.put('signs', signGeometry(this.sg.end, 5.6, 1.9), 0, 2.9, 0);
    B.put('signs', signGeometry(this.sg.end, 5.6, 1.9), 0, 2.9, -0.04, 0, Math.PI, 0);
  }

  // ── Silver Lake: cracked-mud and salt-crust decals ────────────────
  // The terrain's lake bed is a flat colour at racing speed; overlapping
  // translucent tiles of curled mud plates and white salt near the road
  // give it texture where the camera actually looks.
  buildPlaya() {
    const t = this.t, rng = mulberry32(8080), S = {};
    const tiles = this.chunks();   // 500 m bands along the road, for culling
    const s0 = this.Z[2].s0 + 160, s1 = Math.min(t.length - 5, t.roadEnd + 60);
    for (let s = s0; s < s1; s += 7) {
      t.frame(s, S);
      for (const side of [-1, 1]) {
        for (let k = 0; k < 2; k++) {
          const wall = side > 0 ? S.wallR : S.wallL;
          const lat = side * (wall + 3 + Math.pow(rng(), 1.5) * 110);
          const x = S.x + S.rx * lat, z = S.z + S.rz * lat;
          const size = lerp(9, 22, rng()) * (1 + (Math.abs(lat) - wall) / 120);
          const salt = rng() < 0.25;
          tiles[this.chunkOf(s)].push({ x, y: this.gy(x, z) + 0.03, z, sx: size, sy: 1, sz: size * lerp(0.7, 1.2, rng()), ry: rng() * 6.3, col: salt ? 0xffffff : rpick(rng, [0xf0e4d4, 0xe4d4c0, 0xd8c8b4]), b: salt ? 1.15 : lerp(0.85, 1, rng()) });
        }
      }
    }
    const g = new THREE.PlaneGeometry(1, 1);
    g.rotateX(-Math.PI / 2);
    const m = new THREE.MeshStandardMaterial({ map: crackDecalTexture(5), transparent: true, depthWrite: false, roughness: 1, polygonOffset: true, polygonOffsetFactor: -2, polygonOffsetUnits: -2 });
    this.addInstanced(g, m, tiles, { receive: true });
  }

  // ── Spectators' camp along the last mile ──────────────────────────
  // Pickups and motorhomes pulled up in a loose line either side, tents,
  // camp chairs round fire rings, and people drifting toward the course.
  buildCamp() {
    const t = this.t, B = this.B, rng = mulberry32(909);
    this.fires = [];
    this.lanterns = [];
    this.cars = this.cars || [];
    const kinds = ['pickup', 'van', 'pickup', 'sedan', 'boxtruck', 'pickup', 'hatch'];
    const COLS = [0xc9ccd1, 0x8a1c1c, 0x2e6d8e, 0xe8e6df, 0x4d5a3a, 0x9aa3ad, 0xb5a27a, 0x2b2f36, 0xd8a03a];
    const TENT = [0xd84a2a, 0x2e7ac8, 0xe8b830, 0x3a8a4a, 0x8a4ab8, 0xe86a2a];
    const SHIRT = [0xe8e4dc, 0x2a2a2e, 0xb8322a, 0x2e5a8a, 0xd8a03a, 0x4a7a4a, 0x8a8a8e, 0xd86a8a];
    const tents = [], sets = [], people = [];
    const rvBody = [], rvGlass = [];
    const rvKinds = [0, 1, 2].map((k) => motorhome(mulberry32(70 + k)));
    const S = {};
    const person = (x, z, face) => people.push({ x, y: this.gy(x, z) - 0.02, z, sx: lerp(0.92, 1.08, rng()), ry: face + (rng() - 0.5) * 1.2, col: rpick(rng, SHIRT), b: lerp(0.8, 1.05, rng()) });
    for (let s = t.finishS - 1300; s < t.finishS + 120; s += rrange(rng, 18, 34)) {
      for (const side of [-1, 1]) {
        if (rng() < 0.25) continue;
        t.frame(s, S);
        const lat = side * ((side > 0 ? S.wallR : S.wallL) + rrange(rng, 16, 46));
        const x = S.x + S.rx * lat, z = S.z + S.rz * lat, y = S.y - 0.3;
        if (!this.free(x, z, 6)) continue;
        this.take(x, z, 6);
        const face = Math.atan2(S.fx, S.fz) + (rng() < 0.5 ? 0 : Math.PI) + (rng() - 0.5) * 0.6;
        if (rng() < 0.4) {
          // A motorhome, awning side toward the course.
          const rv = rvKinds[Math.floor(rng() * 3)];
          const yaw = Math.atan2(S.fx, S.fz) + (side > 0 ? Math.PI : 0) + (rng() - 0.5) * 0.3;
          const m4 = new THREE.Matrix4().compose(new THREE.Vector3(x, this.gy(x, z) - 0.05, z), new THREE.Quaternion().setFromEuler(new THREE.Euler(0, yaw, 0)), new THREE.Vector3(1, 1, 1));
          rvBody.push(rv.body.clone().applyMatrix4(m4));
          rvGlass.push(rv.glass.clone().applyMatrix4(m4));
          // Lantern under the awning.
          const aw = new THREE.Vector3(2.6, 2.3, -1.6).applyMatrix4(m4);
          this.lanterns.push({ x: aw.x, y: aw.y, z: aw.z, ph: rng() * 10 });
        } else {
          this.cars.push({ kind: rpick(rng, kinds), color: rpick(rng, COLS), seed: Math.floor(rng() * 1000), x, y, z, yaw: face });
        }
        // Canopy or dome tent, camp set and a fire on the course side.
        const cx = x - S.rx * side * 7, cz = z - S.rz * side * 7;
        const r = rng();
        if (r < 0.4) {
          B.setFrame(cx + S.fx * 3, y, cz + S.fz * 3, face);
          for (const [px, pz] of [[-1.4, -1.4], [1.4, -1.4], [-1.4, 1.4], [1.4, 1.4]]) B.box('steel', 0.05, 2.3, 0.05, px, 0, pz);
          B.box(rpick(rng, ['canvasW', 'canvasR', 'canvasB']), 3.1, 0.12, 3.1, 0, 2.3, 0);
          const pk = new THREE.ConeGeometry(2.2, 0.6, 4, 1);
          B.put(rpick(rng, ['canvasW', 'canvasR', 'canvasB']), pk, 0, 2.7, 0, 0, Math.PI / 4, 0);
          B.box('wood', 1.6, 0.06, 0.7, 0, 0.72, 0);
        } else if (r < 0.85) {
          for (let k = 0, n = 1 + Math.floor(rng() * 2); k < n; k++) {
            const tx = x + S.fx * (5 + k * 3.2) * (rng() < 0.5 ? 1 : -1) + S.rx * side * rrange(rng, -2, 3), tz = z + S.fz * (5 + k * 3.2) + S.rz * side * rrange(rng, -2, 3);
            if (!this.clearOfRoad(tx, tz, 2)) continue;
            tents.push({ x: tx, y: this.gy(tx, tz) - 0.02, z: tz, sx: lerp(0.9, 1.3, rng()), ry: rng() * 6.3, col: rpick(rng, TENT) });
          }
        }
        if (rng() < 0.75) {
          const fx = cx - S.fx * 2, fz = cz - S.fz * 2;
          sets.push({ x: fx, y: this.gy(fx, fz) - 0.02, z: fz, sx: 1, ry: rng() * 6.3 });
          this.fires.push({ x: fx, y: y + 0.55, z: fz, ph: rng() * 10 });
          for (let k = 0, n = Math.floor(rng() * 3); k < n; k++) {
            const a = rng() * 6.3;
            person(fx + Math.cos(a) * 2.8, fz + Math.sin(a) * 2.8, Math.atan2(-Math.cos(a), -Math.sin(a)));
          }
        }
      }
    }
    // Spectators along the barriers in the last few hundred metres.
    const toRoad = (side) => Math.atan2(-S.rx * side, -S.rz * side);
    const barrier = [];
    for (let s = t.finishS - 320; s < t.finishS + 60; s += 3) {
      t.frame(s, S);
      for (const side of [-1, 1]) {
        const wl = (side > 0 ? S.wallR : S.wallL) + 5;
        const bx = S.x + S.rx * side * wl, bz = S.z + S.rz * side * wl;
        barrier.push({ x: bx, y: this.gy(bx, bz), z: bz, sx: 1, ry: Math.atan2(S.fx, S.fz) });
        if (rng() < 0.55) {
          const d = wl + rrange(rng, 0.8, 4.5);
          person(S.x + S.rx * side * d + S.fx * rng() * 2, S.z + S.rz * side * d + S.fz * rng() * 2, toRoad(side));
        }
      }
    }
    // Crowd-control barrier: a steel section per 3 m.
    const bb = new ColorBuilder({ m: 0xb8bcc0 });
    bb.box('m', 0.06, 1.1, 0.06, 0, 0, -1.45); bb.box('m', 0.06, 1.1, 0.06, 0, 0, 1.45);
    bb.box('m', 0.06, 0.06, 2.95, 0, 1.05, 0); bb.box('m', 0.06, 0.06, 2.95, 0, 0.2, 0);
    for (let k = -6; k <= 6; k++) bb.box('m', 0.025, 0.85, 0.025, 0, 0.2, k * 0.22);
    for (const [px, pz] of [[0.35, -1.45], [-0.35, -1.45], [0.35, 1.45], [-0.35, 1.45]]) bb.box('m', 0.05, 0.04, 0.05, px, 0, pz);
    const barrierGeo = mergeGeometries([...bb.buckets.values()].flat());
    bb.buckets.clear();
    this.addInstanced(paint(barrierGeo, 0xb8bcc0), this.M.solid, [barrier]);
    this.addInstanced(domeTentGeometry(4), this.M.bush, [tents], { cast: true });
    this.addInstanced(campSetGeometry(9), this.M.solid, [sets]);
    this.people = people;
    this.shirts = SHIRT;
    for (const [list, mat] of [[rvBody, this.M.solid], [rvGlass, this.M.rvGlass]]) {
      if (!list.length) continue;
      const g = mergeGeometries(list, false);
      list.forEach((q) => q.dispose());
      g.computeBoundingSphere();
      const mesh = new THREE.Mesh(g, mat);
      mesh.castShadow = mat === this.M.solid;
      mesh.receiveShadow = true;
      mesh.matrixAutoUpdate = false;
      this.group.add(mesh);
    }
    // Fire flames: a few crossed emissive blades, drawn additive.
    if (this.fires.length) {
      const fl = [];
      for (let k = 0; k < 3; k++) {
        const b = new THREE.ConeGeometry(0.32, 1.1, 5, 1, true);
        b.translate(0, 0.55, 0);
        b.scale(1, 1, 0.3);
        b.rotateY((k / 3) * Math.PI);
        fl.push(b);
      }
      const fg = mergeGeometries(fl);
      const fm = new THREE.MeshBasicMaterial({ color: 0xff8a2a, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending, depthWrite: false, side: THREE.DoubleSide });
      this.flameMat = fm;
      this.flames = new THREE.InstancedMesh(fg, fm, this.fires.length);
      this.fires.forEach((f, i) => this.flames.setMatrixAt(i, new THREE.Matrix4().makeTranslation(f.x, f.y - 0.45, f.z)));
      this.flames.computeBoundingSphere();
      this.group.add(this.flames);
    }
  }

  // ── Finish: truss gantry, timing lights and floodlight towers ─────
  buildFinish() {
    const t = this.t, B = this.B;
    const f = t.frame(t.finishS);
    const yaw = Math.atan2(f.fx, f.fz);
    const wl = f.wallL + 1.2, wr = f.wallR + 1.2;
    const P = (lat, along = 0) => ({ x: f.x + f.rx * lat + f.fx * along, z: f.z + f.rz * lat + f.fz * along });
    const H = 8.5;
    for (const lat of [-wl, wr]) {
      const p = P(lat);
      B.setFrame(p.x, f.y, p.z, yaw);
      for (const [a, b] of [[-0.4, -0.4], [0.4, -0.4], [-0.4, 0.4], [0.4, 0.4]]) B.box('steel', 0.1, H + 1, 0.1, a, 0, b);
      for (let k = 0; k < 9; k++) B.box('steel', 0.8, 0.05, 0.05, 0, 0.6 + k, -0.4, 0, 0, 0.8);
      B.box('concrete', 1.4, 0.5, 1.4, 0, -0.3, 0);
    }
    const c = P((wr - wl) / 2);
    const span = wl + wr;
    B.setFrame(c.x, f.y, c.z, yaw);
    for (const [dy, dz] of [[H, -0.4], [H, 0.4], [H + 0.9, -0.4], [H + 0.9, 0.4]]) B.cbox('steel', span, 0.1, 0.1, 0, dy, dz);
    for (let k = 0; k <= span / 1.2; k++) B.cbox('steel', 0.05, 0.9, 0.05, -span / 2 + k * 1.2, H + 0.45, -0.4 + (k % 2) * 0.8);
    const tex = bannerTexture('FINISH', { w: 1024, h: 256, checker: true, font: 'italic 900 150px "Arial Narrow", Arial, sans-serif' });
    const bm = new THREE.MeshStandardMaterial({ map: tex, emissive: 0xffffff, emissiveMap: tex, emissiveIntensity: 0.3, roughness: 0.6, side: THREE.DoubleSide });
    this.world.addNight(bm, 'emissiveIntensity', 0.2, 0.9);
    const banner = new THREE.Mesh(new THREE.PlaneGeometry(span - 1.2, 2.8), bm);
    banner.position.set(c.x - f.fx * 0.5, f.y + H - 0.9, c.z - f.fz * 0.5);
    banner.rotation.y = Math.atan2(-f.fx, -f.fz);
    this.group.add(banner);
    // Timing trailer on the right.
    const tp = P(wr + 8, 6);
    B.setFrame(tp.x, f.y - 0.3, tp.z, yawZ(-f.rx, -f.rz));
    B.box('white', 6.5, 2.6, 2.4, 0, 0.6, 0);
    B.box('black', 6.5, 0.6, 2.2, 0, 0, 0);
    B.cbox('glassLit', 4, 0.9, 0.08, 0, 2.2, 1.22);
    B.put('signs', signGeometry(this.sg.timing, 3.2, 0.8), 0, 3.6, 1.1);
    B.box('steel', 3.4, 0.9, 0.1, 0, 3.2, 1.05);
    // Bleachers facing the line on the left, behind the barrier.
    const bp = P(-(wl + 13), -14);
    B.setFrame(bp.x, f.y - 0.3, bp.z, yawZ(f.rx, f.rz));
    for (let r = 0; r < 6; r++) {
      B.box('steel', 16, 0.08, 0.7, 0, 0.5 + r * 0.45, -r * 0.75);
      B.box('wood', 16, 0.06, 0.35, 0, 0.58 + r * 0.45, -r * 0.75 + 0.1);
    }
    for (let k = -4; k <= 4; k++) for (let r = 0; r < 6; r += 2) B.box('steel', 0.08, 0.5 + r * 0.45, 0.08, k * 2, 0, -r * 0.75);
    B.box('steel', 16, 0.06, 0.06, 0, 3.6, -4.1);
    // A crowd on the benches, then everyone goes into one instanced mesh.
    const by = yawZ(f.rx, f.rz), rng = mulberry32(4242);
    const people = this.people || [];
    for (let r = 0; r < 6; r++) for (let k = 0; k < 20; k++) {
      if (rng() < 0.35) continue;
      const lx = -7.6 + k * 0.8 + (rng() - 0.5) * 0.2, ly = 0.3 + r * 0.45, lz = -r * 0.75 - 0.1;
      people.push({ x: bp.x + lx * Math.cos(by) + lz * Math.sin(by), y: f.y - 0.3 + ly, z: bp.z - lx * Math.sin(by) + lz * Math.cos(by), sx: 1, sy: lerp(0.8, 0.95, rng()), sz: 1, ry: by + (rng() - 0.5) * 0.6, col: rpick(rng, this.shirts || [0xffffff]), b: lerp(0.8, 1.05, rng()) });
    }
    this.addInstanced(personGeometry(2), this.M.bush, [people]);
    // Floodlight towers either side, before and after the line.
    this.floods = [];
    for (const [lat, along] of [[-(wl + 16), -60], [wr + 16, -60], [-(wl + 16), 60], [wr + 16, 60]]) {
      const p = P(lat, along);
      const H2 = 14;
      B.setFrame(p.x, f.y - 0.3, p.z, 0);
      for (const [a, b] of [[-0.5, -0.5], [0.5, -0.5], [-0.5, 0.5], [0.5, 0.5]]) B.box('steel', 0.12, H2, 0.12, a, 0, b);
      for (let k = 1; k < 7; k++) B.box('steel', 1.1, 0.06, 1.1, 0, k * 2, 0);
      // Lamp bank facing the course.
      const toward = yawZ(-f.rx * Math.sign(lat), -f.rz * Math.sign(lat));
      B.setFrame(p.x, f.y - 0.3, p.z, toward);
      B.box('steel', 3.2, 2.0, 0.3, 0, H2, 0);
      for (let i = 0; i < 3; i++) for (let j = 0; j < 2; j++) {
        B.box('lamp', 0.8, 0.7, 0.12, -1 + i, H2 + 0.25 + j * 0.9, 0.17);
        const lx = -1 + i, ly = H2 + 0.6 + j * 0.9;
        this.lamps = this.lamps || [];
        this.lamps.push({ x: p.x + lx * Math.cos(toward) + 0.4 * Math.sin(toward), y: f.y - 0.3 + ly, z: p.z - lx * Math.sin(toward) + 0.4 * Math.cos(toward), ph: i + j * 3 });
      }
      // Diesel generator trailer at the tower's foot.
      B.box('paintRed', 1.4, 1.3, 2.6, 1.8, 0, -1.2);
      B.box('black', 1.45, 0.25, 2.65, 1.8, 1.3, -1.2);
      B.box('steel', 0.12, 1.1, 0.12, 2.2, 1.5, -2.2);
      this.floods.push({ x: p.x, y: f.y + H2 + 1, z: p.z });
      // Beam: from the lamp bank down onto the course just past the line.
      const aim = P(-lat * 0.2, along * 0.25);
      this.beams = this.beams || [];
      this.beams.push({ x: p.x - f.rx * Math.sign(lat) * 0.5, y: f.y + H2 + 1, z: p.z - f.rz * Math.sign(lat) * 0.5, tx: aim.x, ty: f.y, tz: aim.z });
      const q = P(lat * 0.35, along * 0.8);
      this.pools = this.pools || [];
      this.pools.push({ x: q.x, y: f.y + 0.03, z: q.z, r: 26, c: [0.26, 0.28, 0.32] });
    }
  }

  // Parked vehicles baked into merged meshes (like Beach.buildCars). Body
  // paint goes into a vertex-coloured bucket, so a car park of different
  // colours costs one draw call instead of one per colour. Wrecks get a
  // rusty bucket, sit tilted and sunk, and some have lost their wheels.
  buildCars() {
    if (!this.cars?.length) return;
    const B = new Builder();
    const mats = { paintV: this.M.paintV, rust: this.M.rust };
    const painted = { paintV: [], rust: [] };
    const col = new THREE.Color();
    for (const c of this.cars) {
      const m = buildVehicle(c.kind, { color: c.color, seed: c.seed, lod: 'low' });
      m.root.position.set(c.x, c.y, c.z);
      m.root.rotation.set(c.pitch || 0, c.yaw, c.roll || 0, 'YXZ');
      m.root.updateMatrixWorld(true);
      const bare = c.rust && (c.seed % 3 === 0);
      m.root.traverse((o) => {
        if (!o.isMesh) return;
        if (bare && o.parent?.name === 'wheel') return;
        const mt = Array.isArray(o.material) ? o.material[0] : o.material;
        if (o.name === 'paint' || (c.rust && o.name === 'stripe')) {
          const bucket = c.rust ? 'rust' : 'paintV';
          const g = (o.geometry.index ? o.geometry.toNonIndexed() : o.geometry.clone());
          for (const k of Object.keys(g.attributes)) if (!['position', 'normal'].includes(k)) g.deleteAttribute(k);
          g.applyMatrix4(o.matrixWorld);
          const n = g.getAttribute('position').count, a = new Float32Array(n * 3);
          col.setHex(c.rust ? c.color : mt.color.getHex());
          for (let i = 0; i < n; i++) {
            // Rust: blotchy, darker toward the sills.
            const k = c.rust ? 0.75 + 0.35 * hash2(Math.floor(i / 3), c.seed) : 1;
            a[i * 3] = col.r * k; a[i * 3 + 1] = col.g * k; a[i * 3 + 2] = col.b * k;
          }
          g.setAttribute('color', new THREE.BufferAttribute(a, 3));
          painted[bucket].push(g);
          return;
        }
        if (c.rust && (o.name === 'head' || o.name === 'tail' || o.name === 'rev')) return;
        const sig = [mt.type, mt.color?.getHexString(), mt.emissive?.getHexString(), mt.emissiveIntensity, mt.roughness, mt.metalness, mt.map?.uuid].join('|');
        if (!mats[sig]) mats[sig] = mt;
        B.setFrame(0, 0, 0, 0);
        B.add(sig, o.geometry, o.matrixWorld);
      });
    }
    for (const mesh of B.build(mats, { castShadow: true })) {
      mesh.name = 'desert:cars';
      this.group.add(mesh);
    }
    for (const [key, list] of Object.entries(painted)) {
      if (!list.length) continue;
      const g = mergeGeometries(list, false);
      list.forEach((q) => q.dispose());
      g.computeBoundingSphere();
      const mesh = new THREE.Mesh(g, mats[key]);
      mesh.name = 'desert:cars-' + key;
      mesh.castShadow = mesh.receiveShadow = true;
      mesh.matrixAutoUpdate = false;
      this.group.add(mesh);
    }
  }

  // ── Night lights: pools, flares, fires, floodlight halos ──────────
  buildGlows() {
    // Ground pools: flat additive quads; fires and flares flicker.
    const pools = (this.pools || []).slice();
    for (const f of this.flares) pools.push({ x: f.x, y: f.y - 0.08, z: f.z, r: 1.7, c: [0.55, 0.07, 0.03], fl: 0.6, ph: f.ph });
    for (const f of this.fires || []) pools.push({ x: f.x, y: f.y - 0.5, z: f.z, r: 6.5, c: [0.7, 0.32, 0.1], fl: 0.45, ph: f.ph });
    for (const l of this.lanterns || []) pools.push({ x: l.x, y: l.y - 2.2, z: l.z, r: 4, c: [0.4, 0.3, 0.16], fl: 0.08, ph: l.ph });
    if (pools.length) {
      const { mesh, material } = flickerPools(pools);
      this.group.add(mesh);
      this.poolMat = material;
    }
    // Sprite points: flares (red, flickering), fires (orange), floods (white),
    // fence reflectors (amber, only in headlights — approximated by night).
    const add = (list, o) => {
      if (!list?.length) return null;
      const pts = flickerPoints(list, o);
      this.group.add(pts);
      return pts.material;
    };
    this.flareMat = add(this.flares, { size: 1.1, color: new THREE.Color(3, 0.35, 0.15), rate: 1.6, depth: 0.55 });
    this.fireMat = add(this.fires, { size: 2.4, color: new THREE.Color(2.6, 1.2, 0.35), rate: 1, depth: 0.4 });
    this.lanternMat = add(this.lanterns, { size: 0.7, color: new THREE.Color(2.4, 1.7, 0.9), depth: 0.05 });
    this.floodMat = add(this.floods, { size: 7, color: new THREE.Color(2.2, 2.3, 2.5), depth: 0 });
    this.lampMat = add(this.lamps, { size: 1.6, color: new THREE.Color(2.2, 2.3, 2.5), depth: 0 });
    this.reflMat = add(this.reflectors, { size: 0.5, color: new THREE.Color(2.5, 1.4, 0.3), depth: 0 });
    this.bulbMat = add(this.bulbs, { size: 0.35, color: new THREE.Color(2.6, 1.9, 1.0), blink: 2.2 });
    this.stringMat = add(this.strings, { size: 0.3, color: new THREE.Color(2.4, 1.6, 0.8), depth: 0.08, rate: 0.3 });
    this.bbLampMat = add(this.lampPools, { size: 1.2, color: new THREE.Color(2.4, 2.1, 1.6), depth: 0 });
    // Flare cores: small emissive sticks so they read by day too.
    if (this.flares.length) {
      const g = paint(prep(new THREE.CylinderGeometry(0.03, 0.03, 0.25, 5)), 0xffffff);
      g.rotateZ(Math.PI / 2);
      const m = new THREE.MeshStandardMaterial({ color: 0xff3010, emissive: 0xff3010, emissiveIntensity: 2 });
      this.addInstanced(g, m, [this.flares.map((p) => ({ x: p.x, y: p.y - 0.08, z: p.z, sx: 1, ry: p.ph }))]);
    }
    // Floodlight beams: open additive cones, visible only after dark.
    if (this.beams?.length) {
      const geo = beamGeometry(1, 1);
      const mat = new THREE.MeshBasicMaterial({ vertexColors: true, transparent: true, opacity: 0, blending: THREE.AdditiveBlending, depthWrite: false, fog: true });
      // Front faces only: from inside the beam it vanishes rather than
      // washing out the screen. It fades where the surface turns edge-on,
      // so it reads as a soft shaft of lit dust, not a solid lampshade.
      mat.onBeforeCompile = (sh) => {
        sh.vertexShader = sh.vertexShader
          .replace('#include <common>', '#include <common>\nvarying float vFace;')
          .replace('#include <fog_vertex>', `#include <fog_vertex>
            vec3 n_ = normal;
            #ifdef USE_INSTANCING
              n_ = mat3(instanceMatrix) * n_;
            #endif
            vFace = abs(dot(normalize(normalMatrix * n_), normalize(-mvPosition.xyz)));`);
        sh.fragmentShader = sh.fragmentShader
          .replace('#include <common>', '#include <common>\nvarying float vFace;')
          .replace('#include <alphatest_fragment>', 'diffuseColor.rgb *= vFace * vFace;\n#include <alphatest_fragment>');
      };
      mat.customProgramCacheKey = () => 'desert-beam';
      const im = new THREE.InstancedMesh(geo, mat, this.beams.length);
      const q = new THREE.Quaternion(), dn = new THREE.Vector3(0, -1, 0), d = new THREE.Vector3(), m4 = new THREE.Matrix4();
      this.beams.forEach((b, i) => {
        d.set(b.tx - b.x, b.ty - b.y, b.tz - b.z);
        const len = d.length() * 1.05;
        q.setFromUnitVectors(dn, d.normalize());
        m4.compose(new THREE.Vector3(b.x, b.y, b.z), q, new THREE.Vector3(len * 0.3, len, len * 0.3));
        im.setMatrixAt(i, m4);
      });
      im.computeBoundingSphere();
      im.renderOrder = 3;
      this.group.add(im);
      this.beamMat = mat;
    }
  }

  // Tumbleweeds bowling across the highway on the wind, recycled round
  // the player: each rolls in from the left, crosses, and dies far right.
  updateWeeds(dt, s) {
    const R = this.rollers;
    if (!R) return;
    const inZone = s > this.Z[1].s0 - 200 && s < this.Z[2].s0 + 300;
    const m4 = new THREE.Matrix4(), p = new THREE.Vector3(), sc = new THREE.Vector3(), ax = new THREE.Vector3(), dq = new THREE.Quaternion();
    const F = {};
    for (const w of R.list) {
      if (!w.live) {
        if (!inZone || Math.random() > dt * 0.8) { R.im.setMatrixAt(w.i, m4.makeScale(0, 0, 0)); continue; }
        const ss = s + 30 + Math.random() * 160;
        this.t.frame(ss, F);
        const lat = -(F.wallL + 25 + Math.random() * 20);
        w.x = F.x + F.rx * lat; w.z = F.z + F.rz * lat;
        // Wind from the left, a little along the road.
        const sp = 3.5 + Math.random() * 4;
        w.vx = (F.rx + F.fx * (Math.random() - 0.3) * 0.6) * sp; w.vz = (F.rz + F.fz * (Math.random() - 0.3) * 0.6) * sp;
        w.r = 0.55 + Math.random() * 0.35;
        w.age = 0; w.bounce = 0; w.by = 0; w.life = (70 + F.wallR + F.wallL) / sp;
        w.live = true;
      }
      w.age += dt;
      // Hops: a damped bounce every so often.
      w.by -= 9.8 * dt;
      w.bounce += w.by * dt;
      if (w.bounce <= 0) { w.bounce = 0; w.by = Math.random() < 0.35 ? 2 + Math.random() * 2.5 : 0; }
      const gust = 1 + 0.35 * Math.sin(this.time * 1.7 + w.i);
      w.x += w.vx * dt * gust; w.z += w.vz * dt * gust;
      const v = Math.hypot(w.vx, w.vz) * gust;
      ax.set(w.vz, 0, -w.vx).normalize();
      dq.setFromAxisAngle(ax, (v * dt) / w.r);
      w.q.premultiply(dq);
      const fade = Math.min(1, w.age / 0.6, (w.life - w.age) / 0.6);
      p.set(w.x, this.gy(w.x, w.z) + w.r * 0.9 + w.bounce, w.z);
      R.im.setMatrixAt(w.i, m4.compose(p, w.q, sc.setScalar(w.r * Math.max(0, fade))));
      if (w.age > w.life) w.live = false;
    }
    R.im.instanceMatrix.needsUpdate = true;
  }

  // ── Per frame ─────────────────────────────────────────────────────
  animate(dt, night, camera, s) {
    this.time = (this.time || 0) + dt;
    const T = this.time;
    const k = smoothstep(0.15, 0.7, night);
    glowTime.value = T;
    if (this.flareMat) this.flareMat.color.setRGB(3.2, 0.4, 0.15).multiplyScalar(0.25 + 0.75 * k);
    if (this.fireMat) this.fireMat.color.setRGB(2.6, 1.2, 0.35).multiplyScalar(0.3 + 0.7 * k);
    if (this.lanternMat) this.lanternMat.color.setRGB(2.4, 1.7, 0.9).multiplyScalar(k);
    if (this.floodMat) this.floodMat.color.setRGB(2.2, 2.3, 2.5).multiplyScalar(0.15 + 0.85 * k);
    if (this.lampMat) this.lampMat.color.setRGB(2.2, 2.3, 2.5).multiplyScalar(0.1 + 0.9 * k);
    if (this.reflMat) this.reflMat.color.setRGB(2.5, 1.4, 0.3).multiplyScalar(k);
    if (this.bulbMat) this.bulbMat.color.setRGB(2.6, 1.9, 1.0).multiplyScalar(0.35 + 0.65 * k);
    if (this.stringMat) this.stringMat.color.setRGB(2.4, 1.6, 0.8).multiplyScalar(k);
    if (this.bbLampMat) this.bbLampMat.color.setRGB(2.4, 2.1, 1.6).multiplyScalar(k);
    if (this.flameMat) this.flameMat.opacity = 0.35 + 0.5 * k;
    if (this.beamMat) this.beamMat.opacity = 0.13 * smoothstep(0.3, 0.8, night);
    if (this.poolMat) this.poolMat.opacity = k;
    this.updateTrain(dt, night, s ?? 0);
    this.updateWeeds(dt, s ?? 0);
  }
}
