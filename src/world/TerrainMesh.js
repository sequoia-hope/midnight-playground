import * as THREE from 'three';
import { mergeGeometries } from 'three/addons/utils/BufferGeometryUtils.js';
import { clamp, lerp, smoothstep, hash2, fbm } from '../util/math.js';
import { detailTexture, rockTexture, terrainDetailTexture } from './textures.js';
import { TERRAIN_TILE } from './Terrain.js';

// Turns the Terrain height function into render tiles: 4 m near the road,
// coarser further out. Each tile hangs a skirt off its edges so differing
// resolutions never show a crack.

const lin = (hex) => { const c = new THREE.Color(hex); return [c.r, c.g, c.b]; };
const P = {
  gravel: lin(0x7a7266),
  alpine: lin(0x5b6a3c),
  alpineDry: lin(0x7d7a4c),
  forest: lin(0x2f4526),
  rock: lin(0x77726b),
  rockDark: lin(0x55504b),
  snow: lin(0xe9eef2),
  grass: lin(0x5e8a33),
  grassDry: lin(0x88914a),
  wheat: lin(0xc4a452),
  plowed: lin(0x6e5439),
  crop: lin(0x4d8a2c),
  hay: lin(0x9e9a52),
  lavender: lin(0x8a8a5c),
  city: lin(0x4c4f4a),
  cityPark: lin(0x3f5f2e),
  dirt: lin(0x7b6448),
  sandstone: lin(0x9c8468),
  cliffGrey: lin(0x7c7670),
  coastGrass: lin(0x7f8a4a),
  scrub: lin(0x4c5c34),
  sand: lin(0xd9c69a),
  wetSand: lin(0xa89572),
  seabed: lin(0x4f6258),
  pavement: lin(0x86857f),
  quay: lin(0x6e6f71),
  asphaltLot: lin(0x46474b),
  redRock: lin(0xa0573a),
  redRockDark: lin(0x6e3a28),
  desertSand: lin(0xc9a878),
  desertScrub: lin(0x8a8458),
  playa: lin(0xf0eadc),
  redSand: lin(0xb97a52),
  pavementDark: lin(0x7e6a54),
  playaDark: lin(0xd4ccbc),
  rangeBlue: lin(0x6c6a78),
  // Enrichment: patches that break up each landform's base colour.
  heather: lin(0x6a5c44),
  lichen: lin(0x8c8a6e),
  meadow: lin(0x7d9a3a),
  lush: lin(0x46702a),
  icePlant: lin(0x7a6a3c),
  varnish: lin(0x5a3a2c),
  concrete: lin(0x7c7b76),
  yardGravel: lin(0x6f685e),
  rust: lin(0x7a5a44),
};
// Sandstone layers, bottom to top of each 9 m band set.
const STRATA = [lin(0xa4553a), lin(0xb86a44), lin(0x8a3f2c), lin(0xc88a5c), lin(0xa04a34), lin(0xd4a47a), lin(0x94503a), lin(0xb4603e)];

function mix3(out, a, b, t) {
  out[0] = a[0] + (b[0] - a[0]) * t;
  out[1] = a[1] + (b[1] - a[1]) * t;
  out[2] = a[2] + (b[2] - a[2]) * t;
  return out;
}

const FIELD_COLS = [P.wheat, P.plowed, P.crop, P.hay, P.grassDry, P.crop, P.wheat, P.lavender];

export class TerrainColorizer {
  constructor(terrain) {
    this.t = terrain;
    this.noise = terrain.noise2;
  }

  // Valley field patchwork: a rotated grid of parcels, each its own crop.
  fieldColor(x, z, out) {
    const a = 0.38; // grid rotation
    const u = x * Math.cos(a) - z * Math.sin(a);
    const v = x * Math.sin(a) + z * Math.cos(a);
    const fu = Math.floor(u / 110), fv = Math.floor(v / 160);
    const h = hash2(fu, fv, 3);
    const col = FIELD_COLS[Math.floor(h * FIELD_COLS.length)];
    // Furrows: stripes inside the parcel.
    const stripe = Math.sin(((h > 0.5 ? u : v) / 2.2) * Math.PI) * 0.5 + 0.5;
    const edge = Math.min(u / 110 - fu, 1 - (u / 110 - fu), v / 160 - fv, 1 - (v / 160 - fv));
    out[0] = col[0] * (0.9 + stripe * 0.12);
    out[1] = col[1] * (0.9 + stripe * 0.12);
    out[2] = col[2] * (0.9 + stripe * 0.12);
    if (edge < 0.025) mix3(out, out, P.grass, 0.8); // hedgerow / grass margin
    return h;
  }

  color(x, y, z, ny, out) {
    const t = this.t;
    const w = t.zoneWeights(x);
    const info = t.roadInfo(x, z);
    const ctx = { x, y, z, d: info.d, n1: fbm(this.noise, x / 160, z / 160, 3), slope: 1 - ny, pv: 0 };
    const c = [0, 0, 0];
    out[0] = out[1] = out[2] = 0;
    // How much of this spot is paved (concrete/asphalt yards, pavements):
    // forms set ctx.pv; the ground shader draws slab joints there instead
    // of grass/soil detail.
    this.paved = 0;
    for (let k = 0; k < w.length; k++) {
      if (w[k] <= 0.001) continue;
      ctx.pv = 0;
      this[t.forms[k]](c, ctx);
      out[0] += c[0] * w[k]; out[1] += c[1] * w[k]; out[2] += c[2] * w[k];
      this.paved += ctx.pv * w[k];
    }
    // Shorelines: wet sand at the waterline, seabed below it.
    if (t.seaY !== null) {
      const sea = t.seaY;
      const sandy = (1 - smoothstep(sea + 1.2, sea + 3.5, y + ctx.n1 * 1.5)) * (1 - smoothstep(0.35, 0.6, ctx.slope));
      mix3(out, out, P.sand, sandy * 0.9);
      mix3(out, out, P.wetSand, (1 - smoothstep(sea - 0.3, sea + 0.8, y)) * 0.8);
      mix3(out, out, P.seabed, 1 - smoothstep(sea - 4, sea - 1.2, y));
    }
    return out;
  }

  mountain(c, { x, y, z, d, n1, slope }) {
    const rock = smoothstep(0.22, 0.42, slope + n1 * 0.08);
    mix3(c, P.alpine, P.alpineDry, clamp(0.5 + n1, 0, 1));
    const forest = smoothstep(0.1, 0.35, fbm(this.noise, x / 420 + 7, z / 420, 3)) * (1 - smoothstep(300, 380, y));
    mix3(c, c, P.forest, forest * 0.8);
    // Heather and dry patches on the open slopes, lichen on the rock.
    mix3(c, c, P.heather, smoothstep(0.15, 0.45, fbm(this.noise, x / 90 + 2, z / 90 - 5, 2)) * 0.45 * (1 - forest));
    const rockCol = mix3([0, 0, 0], P.rock, P.rockDark, clamp(0.5 + n1 * 1.5, 0, 1));
    mix3(rockCol, rockCol, P.lichen, smoothstep(0.1, 0.5, fbm(this.noise, x / 45 - 3, z / 45, 2)) * 0.35);
    mix3(c, c, rockCol, rock);
    const snow = smoothstep(470, 560, y + n1 * 80) * (1 - smoothstep(0.45, 0.7, slope));
    mix3(c, c, P.snow, snow);
    if (d < 30) mix3(c, c, P.gravel, (1 - smoothstep(8, 22, d)) * 0.85);
  }

  valley(c, { x, z, d, n1, slope }) {
    mix3(c, P.grass, P.grassDry, clamp(0.45 + n1 * 1.2, 0, 1));
    // Lusher grass in hollows, meadow flowers in drifts.
    mix3(c, c, P.lush, smoothstep(0.1, 0.4, fbm(this.noise, x / 130 - 6, z / 130, 2)) * 0.5);
    mix3(c, c, P.meadow, smoothstep(0.3, 0.6, fbm(this.noise, x / 40 + 9, z / 40, 2)) * 0.35);
    const fieldOK = smoothstep(14, 26, d) * (1 - smoothstep(700, 1000, d)) * (1 - smoothstep(0.06, 0.14, slope));
    if (fieldOK > 0) {
      const f = [0, 0, 0];
      this.fieldColor(x, z, f);
      mix3(c, c, f, fieldOK);
    }
    const hillForest = smoothstep(0.05, 0.3, fbm(this.noise, x / 300 + 3, z / 300, 3)) * smoothstep(500, 900, d);
    mix3(c, c, P.forest, hillForest * 0.85);
    mix3(c, c, P.rock, smoothstep(0.35, 0.6, slope));
    if (d < 16) mix3(c, c, P.dirt, (1 - smoothstep(7, 12, d)) * 0.6);
  }

  city(c, ctx) {
    const { x, z, d, slope } = ctx;
    const park = smoothstep(0.2, 0.35, fbm(this.noise, x / 200 + 1, z / 200, 2));
    mix3(c, P.city, P.cityPark, park);
    ctx.pv = (1 - park) * (1 - smoothstep(1200, 2000, d)) * (1 - smoothstep(0.2, 0.4, slope));
    mix3(c, c, P.forest, smoothstep(1200, 2000, d) * 0.8);
    mix3(c, c, P.rock, smoothstep(0.4, 0.65, slope));
  }

  coast(c, { x, z, y, d, n1, slope }) {
    // Dry coastal grass and scrub on top, banded sandstone/grey cliffs.
    mix3(c, P.coastGrass, P.scrub, smoothstep(-0.1, 0.35, fbm(this.noise, x / 240 + 5, z / 240, 3)));
    // Ice plant mats and bare earth scattered through the grass.
    mix3(c, c, P.icePlant, smoothstep(0.25, 0.55, fbm(this.noise, x / 55 - 1, z / 55 + 4, 2)) * 0.45);
    mix3(c, c, P.dirt, smoothstep(0.35, 0.6, fbm(this.noise, x / 30 + 3, z / 30 - 2, 2)) * 0.3);
    const rock = smoothstep(0.25, 0.45, slope + n1 * 0.1);
    const rockCol = mix3([0, 0, 0], P.sandstone, P.cliffGrey, clamp(0.5 + n1 * 1.6, 0, 1));
    mix3(c, c, rockCol, rock);
    if (d < 26) mix3(c, c, P.gravel, (1 - smoothstep(8, 20, d)) * 0.8);
  }

  beach(c, ctx) {
    const { x, z, d, n1, slope } = ctx;
    // Town side: pavements, lots and gardens near the road, giving way to
    // dry grass and scrub on slopes and further inland. Sea side: sand.
    const garden = smoothstep(0.15, 0.4, fbm(this.noise, x / 150 + 2, z / 150, 2));
    mix3(c, P.pavement, P.grass, garden * 0.7);
    const wild = Math.max(smoothstep(330, 470, d), smoothstep(0.08, 0.2, slope));
    const scrubCol = mix3([0, 0, 0], P.coastGrass, P.scrub, clamp(0.5 + n1 * 1.4, 0, 1));
    mix3(c, c, scrubCol, wild);
    const sand = smoothstep(10, 26, d) * this.t.seaSide(this.t.far(x, z));
    mix3(c, c, P.sand, sand);
    ctx.pv = (1 - garden * 0.7) * (1 - wild) * (1 - sand);
    mix3(c, c, P.forest, smoothstep(700, 1300, d) * 0.7);
    mix3(c, c, P.rock, smoothstep(0.4, 0.65, slope));
  }

  // Downtown Streets: paved lots and yards between buildings, parks and
  // wooded hills far off.
  streets(c, ctx) {
    const { x, z, d, slope } = ctx;
    const lot = smoothstep(0.0, 0.3, fbm(this.noise, x / 90 + 4, z / 90, 2));
    mix3(c, P.pavement, P.asphaltLot, lot * 0.8);
    const park = smoothstep(0.25, 0.4, fbm(this.noise, x / 260 + 1, z / 260, 2)) * smoothstep(60, 200, d) * 0.8;
    mix3(c, c, P.cityPark, park);
    ctx.pv = (1 - park) * (1 - smoothstep(900, 1700, d)) * (1 - smoothstep(0.2, 0.45, slope));
    mix3(c, c, P.forest, smoothstep(900, 1700, d) * 0.8);
    mix3(c, c, P.rock, smoothstep(0.45, 0.7, slope));
  }

  // Seaside Raceway: the aerial photo's own colour (level.groundColor,
  // sRGB 0..1). An overhead summer photo is pale and hazy, so it's graded a
  // little darker and richer for the game's sunlight. Grey, unsaturated
  // ground (the paddock, car parks, service roads) is paved.
  raceway(c, ctx) {
    const g = this.t.level.groundColor(ctx.x, ctx.z, this._rw || (this._rw = [0, 0, 0]));
    const lin = (v) => Math.pow(v, 2.2);
    // Warmer (the photo's haze is blue) and richer.
    const r = lin(g[0]) * 1.04, gg = lin(g[1]), b = lin(g[2]) * 0.82;
    const l = (r + gg + b) / 3;
    const sat = 1.55, k = 0.74;
    c[0] = Math.max(0, l + (r - l) * sat) * k;
    c[1] = Math.max(0, l + (gg - l) * sat) * k;
    c[2] = Math.max(0, l + (b - l) * sat) * k;
    // Paved yards and lots get slab joints; not the run-off inside the
    // barriers (that's the scenery's asphalt or bare graded dirt).
    const chroma = Math.max(g[0], g[1], g[2]) - Math.min(g[0], g[1], g[2]);
    const grey = (1 - smoothstep(0.035, 0.08, chroma)) * smoothstep(0.4, 0.55, (g[0] + g[1] + g[2]) / 3);
    ctx.pv = grey * smoothstep(36, 50, ctx.d) * 0.6;
  }

  // ── Desert Run (Level 4) ───────────────────────────────────────
  // Banded sandstone on the steep faces, red sand on the floor and ledges.
  strataColor(out, y, n1) {
    const k = Math.floor((y + n1 * 5) / 4.5);
    const a = STRATA[((k % STRATA.length) + STRATA.length) % STRATA.length];
    const b = STRATA[(((k + 1) % STRATA.length) + STRATA.length) % STRATA.length];
    return mix3(out, a, b, clamp(((y + n1 * 5) / 4.5 - k) * 1.5 - 0.9, 0, 1));
  }

  canyon(c, { x, y, z, n1, slope, d }) {
    mix3(c, P.redSand, P.desertSand, clamp(0.35 + n1 * 1.4, 0, 1) * 0.55);
    mix3(c, c, P.desertScrub, smoothstep(0.1, 0.35, fbm(this.noise, x / 70 + 3, z / 70, 2)) * 0.25 * (1 - smoothstep(0.1, 0.25, slope)));
    const rock = smoothstep(0.18, 0.4, slope + n1 * 0.08);
    // Desert varnish: dark streaks down the faces.
    const strata = this.strataColor([0, 0, 0], y, n1);
    mix3(strata, strata, P.varnish, smoothstep(0.2, 0.6, fbm(this.noise, x / 14, z / 14 + y / 60, 2)) * 0.4);
    mix3(c, c, strata, rock);
    if (d < 24) mix3(c, c, P.gravel, (1 - smoothstep(7, 18, d)) * 0.55);
  }

  desert(c, { x, y, z, n1, slope, d }) {
    // Pale sand, a dark varnished gravel "pavement" in patches, scrub.
    mix3(c, P.desertSand, P.pavementDark, smoothstep(0.0, 0.4, fbm(this.noise, x / 220 + 7, z / 220, 3)) * 0.55);
    mix3(c, c, P.desertScrub, smoothstep(-0.1, 0.3, fbm(this.noise, x / 60 + 1, z / 60, 2)) * 0.3);
    mix3(c, c, this.strataColor([0, 0, 0], y, n1), smoothstep(0.25, 0.45, slope));
    mix3(c, c, P.rangeBlue, smoothstep(1800, 3200, d) * 0.35 * smoothstep(0.15, 0.4, slope));
    if (d < 24) mix3(c, c, P.gravel, (1 - smoothstep(7, 18, d)) * 0.5);
  }

  playa(c, { x, y, z, slope }) {
    mix3(c, P.playa, P.playaDark, clamp(0.5 + fbm(this.noise, x / 90, z / 90, 3) * 1.2, 0, 1) * 0.6);
    // Wet-season stains, long and faint.
    mix3(c, c, P.playaDark, smoothstep(0.3, 0.5, fbm(this.noise, x / 400 + 2, z / 40, 2)) * 0.25);
    // The shore: sand and scrub on the fans, rock on the ranges.
    const above = y - (this.t.far(x, z, this._pf || (this._pf = {})).y - 0.35);
    mix3(c, c, P.desertSand, smoothstep(0.4, 3, above));
    mix3(c, c, P.desertScrub, smoothstep(3, 12, above) * 0.3);
    mix3(c, c, P.redRockDark, smoothstep(0.3, 0.55, slope) * 0.7);
  }

  // Port land: a grid of yards along the quay — pale concrete aprons, dark
  // asphalt container stacks, gravel rail yards — with lighter haul roads
  // between them and rust/oil stains, instead of one grey sheet.
  harbor(c, ctx) {
    const { x, z, d, slope } = ctx;
    const a = 0.21;
    const u = x * Math.cos(a) - z * Math.sin(a), v = x * Math.sin(a) + z * Math.cos(a);
    const fu = Math.floor(u / 95), fv = Math.floor(v / 62);
    const h = hash2(fu, fv, 9);
    const yard = h < 0.38 ? P.concrete : h < 0.7 ? P.asphaltLot : h < 0.85 ? P.yardGravel : P.quay;
    const edge = Math.min(u / 95 - fu, 1 - (u / 95 - fu), v / 62 - fv, 1 - (v / 62 - fv));
    mix3(c, yard, P.concrete, (1 - smoothstep(0.03, 0.06, edge)) * 0.8);
    const stain = smoothstep(0.2, 0.55, fbm(this.noise, x / 35 + 9, z / 35, 2));
    mix3(c, c, h > 0.85 ? P.rust : P.asphaltLot, stain * 0.3);
    // Weathering: large faint patches.
    const wear = fbm(this.noise, x / 150 + 9, z / 150, 2);
    c[0] *= 0.92 + wear * 0.16; c[1] *= 0.92 + wear * 0.16; c[2] *= 0.92 + wear * 0.16;
    const far = smoothstep(1100, 1900, d);
    mix3(c, c, P.forest, far * 0.75);
    const rock = smoothstep(0.4, 0.65, slope);
    mix3(c, c, P.rock, rock);
    ctx.pv = (h < 0.85 ? 1 : 0.3) * (1 - far) * (1 - rock);
  }


}

function buildTileGeometry(terrain, colorizer, tile) {
  const { x0, z0, size, step } = tile;
  const N = Math.round(size / step);
  const V = N + 1;
  // Heights on a padded grid so normals at edges are correct.
  const P1 = V + 2;
  const H = new Float32Array(P1 * P1);
  for (let j = 0; j < P1; j++) for (let i = 0; i < P1; i++) {
    H[j * P1 + i] = terrain.heightAt(x0 + (i - 1) * step, z0 + (j - 1) * step);
  }
  // Stitch: along an edge shared with a coarser tile, use that tile's
  // linear interpolation so the two surfaces meet exactly.
  const at = (i, j) => (j + 1) * P1 + (i + 1);
  const edgeIdx = [(k) => at(k, 0), (k) => at(k, V - 1), (k) => at(0, k), (k) => at(V - 1, k)];
  (tile.nsteps || []).forEach((ns, e) => {
    if (ns <= step) return;
    const ratio = Math.round(ns / step);
    const src = [];
    for (let k = 0; k < V; k++) src.push(H[edgeIdx[e](k)]);
    for (let k = 0; k < V; k++) {
      const k0 = Math.floor(k / ratio) * ratio, k1 = Math.min(V - 1, k0 + ratio);
      if (k0 === k) continue;
      H[edgeIdx[e](k)] = src[k0] + (src[k1] - src[k0]) * ((k - k0) / (k1 - k0));
    }
  });
  const skirtDepth = step * 1.5 + 3;
  const vCount = V * V + 4 * V;
  const pos = new Float32Array(vCount * 3);
  const nor = new Float32Array(vCount * 3);
  const col = new Float32Array(vCount * 3);
  const uv = new Float32Array(vCount * 2);
  const surf = new Float32Array(vCount);
  const tmp = [0, 0, 0];
  let v = 0;
  const put = (x, y, z, nx, ny, nz, c, pv = 0) => {
    surf[v] = pv;
    pos[v * 3] = x; pos[v * 3 + 1] = y; pos[v * 3 + 2] = z;
    nor[v * 3] = nx; nor[v * 3 + 1] = ny; nor[v * 3 + 2] = nz;
    col[v * 3] = c[0]; col[v * 3 + 1] = c[1]; col[v * 3 + 2] = c[2];
    uv[v * 2] = x / 9; uv[v * 2 + 1] = z / 9;
    return v++;
  };
  for (let j = 0; j < V; j++) for (let i = 0; i < V; i++) {
    const h = H[(j + 1) * P1 + (i + 1)];
    const hl = H[(j + 1) * P1 + i], hr = H[(j + 1) * P1 + i + 2];
    const hd = H[j * P1 + i + 1], hu = H[(j + 2) * P1 + i + 1];
    let nx = hl - hr, ny = 2 * step, nz = hd - hu;
    const l = Math.hypot(nx, ny, nz);
    nx /= l; ny /= l; nz /= l;
    const x = x0 + i * step, z = z0 + j * step;
    colorizer.color(x, h, z, ny, tmp);
    // Crease shading: ground lower than its neighbours (gullies, the foot of
    // a cutting) darkens, crests lighten a touch. Measured in grade over one
    // node step so it reads alike on fine and coarse tiles.
    const cav = ((hl + hr + hd + hu) * 0.25 - h) / step;
    const ao = clamp(1 - cav * 0.9, 0.7, 1.06);
    // Mottling a few nodes across so neighbouring vertices aren't identical.
    const mot = 1 + colorizer.noise(x / 23 + 11.3, z / 23 - 4.1) * 0.06 + (hash2(Math.round(x), Math.round(z), 5) - 0.5) * 0.035;
    tmp[0] *= ao * mot; tmp[1] *= ao * mot; tmp[2] *= ao * mot;
    put(x, h, z, nx, ny, nz, tmp, colorizer.paved);
  }
  const idx = [];
  for (let j = 0; j < N; j++) for (let i = 0; i < N; i++) {
    const a = j * V + i, b = a + 1, c = a + V, d = c + 1;
    // Split each quad along the diagonal whose ends are closest in height,
    // so terrace edges and cliff lips follow the ground instead of zigzagging
    // across it; on even ground alternate for a less directional look.
    const dad = Math.abs(pos[a * 3 + 1] - pos[d * 3 + 1]), dbc = Math.abs(pos[b * 3 + 1] - pos[c * 3 + 1]);
    const useBC = Math.abs(dad - dbc) > 0.25 ? dbc < dad : ((i + j) & 1);
    if (useBC) idx.push(a, c, b, b, c, d);
    else idx.push(a, c, d, a, d, b);
  }
  // Skirts: four edges.
  const edges = [
    (k) => k,                       // z0 edge  (j=0)
    (k) => (V - 1) * V + k,         // z1 edge  (j=V-1)
    (k) => k * V,                   // x0 edge  (i=0)
    (k) => k * V + V - 1,           // x1 edge  (i=V-1)
  ];
  for (let e = 0; e < 4; e++) {
    const base = v;
    for (let k = 0; k < V; k++) {
      const src = edges[e](k);
      tmp[0] = col[src * 3]; tmp[1] = col[src * 3 + 1]; tmp[2] = col[src * 3 + 2];
      put(pos[src * 3], pos[src * 3 + 1] - skirtDepth, pos[src * 3 + 2], nor[src * 3], nor[src * 3 + 1], nor[src * 3 + 2], tmp, surf[src]);
    }
    for (let k = 0; k < V - 1; k++) {
      const a = edges[e](k), b = edges[e](k + 1), c = base + k, d = base + k + 1;
      // Double-sided by emitting both windings — skirts are thin and cheap.
      idx.push(a, b, c, b, d, c, a, c, b, b, c, d);
    }
  }
  const g = new THREE.BufferGeometry();
  g.setAttribute('position', new THREE.BufferAttribute(pos, 3));
  g.setAttribute('normal', new THREE.BufferAttribute(nor, 3));
  g.setAttribute('color', new THREE.BufferAttribute(col, 3));
  g.setAttribute('uv', new THREE.BufferAttribute(uv, 2));
  g.setAttribute('aSurf', new THREE.BufferAttribute(surf, 1));
  g.setIndex(vCount > 65535 ? new THREE.Uint32BufferAttribute(idx, 1) : new THREE.Uint16BufferAttribute(idx, 1));
  return g;
}

// Planar UVs smear on cliffs, so sample the detail map from above and a
// stratified rock map from the sides, weighted by the world normal.
//
// On top of that the ground shader layers several scales from one packed
// detail texture (see terrainDetailTexture): a very broad macro tint that
// breaks up tiling over hundreds of metres, the original 9 m / 61 m detail,
// and close-up grain that reads as grass tufts, sand or pebbles depending on
// the vertex colour (green-dominant = grass, warm = sand/dirt). A cheap
// derivative bump (no extra fetches) gives the close ground relief, faded out
// with distance so it never shimmers.
export function patchTriplanar(material, rockTex, detailTex = null) {
  material.onBeforeCompile = (shader) => {
    shader.uniforms.tRock = { value: rockTex };
    const packed = !!detailTex;
    if (packed) shader.uniforms.tDetail = { value: detailTex };
    shader.vertexShader = shader.vertexShader
      .replace('#include <common>', '#include <common>\nvarying vec3 vTPos;\nvarying vec3 vTNorm;' + (packed ? '\nattribute float aSurf;\nvarying float vSurf;' : ''))
      .replace('#include <fog_vertex>', '#include <fog_vertex>\nvTPos = (modelMatrix * vec4(transformed, 1.0)).xyz;\nvTNorm = normalize(mat3(modelMatrix) * objectNormal);' + (packed ? '\nvSurf = aSurf;' : ''));
    if (!packed) {
      shader.fragmentShader = shader.fragmentShader
        .replace('#include <common>', '#include <common>\nvarying vec3 vTPos;\nvarying vec3 vTNorm;\nuniform sampler2D tRock;')
        .replace('#include <map_fragment>', `
          vec3 tw = pow(abs(normalize(vTNorm)), vec3(4.0));
          tw /= (tw.x + tw.y + tw.z);
          vec3 top = texture2D(map, vTPos.xz / 9.0).rgb;
          vec3 top2 = texture2D(map, vTPos.xz / 61.0 + 0.37).rgb;
          vec3 sx = texture2D(tRock, vec2(vTPos.z / 23.0, vTPos.y / 15.0 + top2.r * 0.35)).rgb;
          vec3 sz = texture2D(tRock, vec2(vTPos.x / 23.0, vTPos.y / 15.0 + top2.g * 0.35)).rgb;
          vec3 side = mix(vec3(0.85), sx * tw.x / max(tw.x + tw.z, 1e-3) + sz * tw.z / max(tw.x + tw.z, 1e-3), 0.75) * (0.75 + 0.5 * top.r);
          vec3 tex = (top * 0.6 + top2 * 0.6) * tw.y + side * (tw.x + tw.z);
          diffuseColor.rgb *= tex * 1.3;
        `);
      return;
    }
    shader.fragmentShader = shader.fragmentShader
      .replace('#include <common>', `#include <common>
varying vec3 vTPos;
varying vec3 vTNorm;
varying float vSurf;
uniform sampler2D tRock;
uniform sampler2D tDetail;
// Bump from a scalar height via screen-space derivatives (as three's
// perturbNormalArb, but without needing a bump map).
vec3 mrPerturb(vec3 surfPos, vec3 surfNorm, float h, float faceDir) {
  vec3 sx = dFdx(surfPos), sy = dFdy(surfPos);
  vec3 r1 = cross(sy, surfNorm), r2 = cross(surfNorm, sx);
  float det = dot(sx, r1) * faceDir;
  if (abs(det) < 1e-12) return surfNorm;
  vec3 grad = sign(det) * (dFdx(h) * r1 + dFdy(h) * r2);
  return normalize(abs(det) * surfNorm - grad);
}`)
      .replace('#include <map_fragment>', `
        float mrDist = length(vTPos - cameraPosition);
        float mrNear = 1.0 - smoothstep(14.0, 55.0, mrDist);
        float mrMid = 1.0 - smoothstep(90.0, 420.0, mrDist);
        vec3 tn = normalize(vTNorm);
        vec3 tw = pow(abs(tn), vec3(4.0));
        tw /= (tw.x + tw.y + tw.z);
        vec4 dA = texture2D(tDetail, vTPos.xz / 9.0);
        vec4 dB = texture2D(tDetail, vTPos.xz / 61.0 + 0.37);
        vec4 dM = texture2D(tDetail, vTPos.xz / 900.0 + 0.11);
        // Close grain and the side (rock) samples only where they show;
        // both conditions are smooth over the screen so the branches stay
        // coherent.
        vec4 dC = vec4(0.5);
        if (mrNear > 0.0) dC = texture2D(tDetail, vTPos.xz / 2.3 + 0.61);
        vec3 sx = vec3(0.85), sz = vec3(0.85);
        float varnish = 0.0;
        if (tw.x + tw.z > 0.004) {
          sx = texture2D(tRock, vec2(vTPos.z / 23.0, vTPos.y / 15.0 + dB.r * 0.35)).rgb;
          sz = texture2D(tRock, vec2(vTPos.x / 23.0, vTPos.y / 15.0 + dB.a * 0.35)).rgb;
          // Desert varnish on red rock: dark streaks running down the face
          // (the noise stretched vertically), only on warm-red faces.
          float red = smoothstep(0.35, 0.65, (vColor.r - vColor.g) / (vColor.r + 0.02));
          if (red > 0.0) {
            vec4 vs = texture2D(tDetail, vec2((vTPos.x + vTPos.z) / 7.0, vTPos.y / 55.0));
            varnish = smoothstep(0.45, 0.8, vs.a) * red;
          }
        }
        // What kind of ground this is, read from the vertex colour.
        vec3 vc = vColor;
        float grassy = smoothstep(0.08, 0.32, (vc.g - max(vc.r, vc.b)) / (vc.g + 0.02));
        float sandy = smoothstep(0.3, 0.6, (vc.r - vc.b) / (vc.r + 0.02)) * (1.0 - grassy);
        float fineN = mix(dA.g, dC.g, mrNear);
        // Grass: tufts and darker clumps; sand: soft grain and wind ripples;
        // dirt/gravel (the rest): pebbles with dark gaps.
        float grassT = mix(1.0, 0.72 + 0.52 * fineN, mrMid) * (0.9 + 0.2 * dB.a);
        float rip = sin(dot(vTPos.xz, vec2(1.9, 1.1)) + dB.r * 9.0) * 0.5 + 0.5;
        float sandT = 1.0 + ((fineN - 0.5) * 0.14 + (rip - 0.5) * 0.08 * mrNear) * mrMid;
                float dirtT = mix(1.0, 0.9 + (dC.b - 0.45) * 0.3 * mrNear + (fineN - 0.5) * 0.22, mrMid);
        // Paved yards and lots: aggregate speckle, oil stains and slab joints
        // (5 m concrete panels) that fade out before they can alias.
        float paved = clamp(vSurf, 0.0, 1.0);
        vec2 slab = vTPos.xz / vec2(5.0, 4.0);
        vec2 sj = abs(fract(slab) - 0.5);
        float jw = fwidth(slab.x) + fwidth(slab.y);
        float joint = 1.0 - (1.0 - smoothstep(0.0, jw * 1.5 + 0.012, 0.5 - max(sj.x, sj.y))) * 0.3 * (1.0 - smoothstep(0.08, 0.3, jw));
        float slabTone = 0.94 + 0.12 * fract(sin(dot(floor(slab), vec2(12.9898, 78.233))) * 43758.5453);
        float pavedT = mix(1.0, (0.92 + (fineN - 0.5) * 0.18) * slabTone * joint, mrMid) * (1.0 - smoothstep(0.55, 0.8, dB.a) * 0.18);
        float groundT = mix(mix(mix(dirtT, sandT, sandy), grassT, grassy), pavedT, paved);
        vec3 top = vec3((dA.r * 0.6 + dB.r * 0.6) * groundT);
        vec3 side = mix(vec3(0.85), sx * tw.x / max(tw.x + tw.z, 1e-3) + sz * tw.z / max(tw.x + tw.z, 1e-3), 0.75) * (0.75 + 0.5 * dA.r);
        side *= 1.0 - varnish * 0.38;
        vec3 tex = top * tw.y + side * (tw.x + tw.z);
        // Macro: broad brightness and hue drift so the same parcel of colour
        // doesn't repeat, strongest in the mid distance where tiling shows.
        float mac = dM.a - 0.5;
        tex *= (1.0 + mac * 0.28) * vec3(1.0 + mac * 0.06, 1.0, 1.0 - mac * 0.1);
        diffuseColor.rgb *= tex * 1.3;
        float mrH = ((dC.b * (1.0 - grassy) * (1.0 - sandy) * 0.6 + fineN * 0.35) * mrNear * tw.y * 0.02 + dA.r * mrMid * 0.12) * (1.0 - paved * 0.8);
      `)
      .replace('#include <normal_fragment_maps>', '#include <normal_fragment_maps>\nnormal = mrPerturb(-vViewPosition, normal, mrH, faceDirection);')
      .replace('#include <roughnessmap_fragment>', '#include <roughnessmap_fragment>\nroughnessFactor = clamp(roughnessFactor - sandy * 0.08 * (1.0 - fineN), 0.0, 1.0);');
  };
}

export async function buildTerrainMeshes(terrain, onProgress = () => {}) {
  const group = new THREE.Group();
  group.name = 'terrain';
  const detail = detailTexture();
  const material = new THREE.MeshStandardMaterial({
    vertexColors: true,
    map: detail,
    roughness: 0.96,
    metalness: 0,
  });
  patchTriplanar(material, rockTexture(), terrainDetailTexture());
  const colorizer = new TerrainColorizer(terrain);
  const tiles = terrain.tileList();
  // Group coarse tiles into larger meshes to keep draw calls down.
  const groups = new Map();
  let done = 0;
  const yieldEvery = 24;
  for (const tile of tiles) {
    const g = tile.step === 4 ? 2 : tile.step === 16 ? 3 : 5;
    const gx = Math.floor((tile.x0 - terrain.minX) / (TERRAIN_TILE * g));
    const gz = Math.floor((tile.z0 - terrain.minZ) / (TERRAIN_TILE * g));
    const key = `${tile.step}:${gx}:${gz}`;
    const geo = buildTileGeometry(terrain, colorizer, tile);
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(geo);
    done++;
    if (done % yieldEvery === 0) {
      onProgress(done / tiles.length);
      await new Promise((r) => setTimeout(r, 0));
    }
  }
  for (const geos of groups.values()) {
    const geo = geos.length === 1 ? geos[0] : mergeGeometries(geos, false);
    if (geos.length > 1) geos.forEach((g) => g.dispose());
    geo.computeBoundingSphere();
    const mesh = new THREE.Mesh(geo, material);
    mesh.receiveShadow = true;
    mesh.matrixAutoUpdate = false;
    group.add(mesh);
  }
  onProgress(1);
  return { group, material };
}
