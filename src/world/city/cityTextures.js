import * as THREE from 'three';
import { facadeTextures } from '../textures.js';
import { mulberry32 } from '../../util/math.js';

// City-only canvas textures: the building atlas, billboard ads, tunnel
// tiles and sound-wall panels.

export const CELLS = 10;
// Metres covered by one repeat of each atlas cell [width, height]. Chosen so
// windows come out ~3 m wide and ~3.6 m per floor on every variant.
export const CELL_TILE = [
  [24, 72], [21, 60.8], [28, 93.6], [18, 50.4], [31.2, 105], [16, 38],
  [16, 16],   // 6: roof gravel
  [24, 48],   // 7: dark curtain-wall glass (crowns, podium glazing)
  [24, 48],   // 8: corrugated warehouse siding with roll-up doors (bottom-aligned)
  [18, 36],   // 9: brick apartments
];
export const CELL_COLS = [8, 6, 10, 5, 12, 4, 1, 8, 4, 6];
export const CELL_ROWS = [20, 16, 26, 14, 30, 10, 1, 12, 1, 12];
export const ROOF_CELL = 6, GLASS_CELL = 7, WAREHOUSE_CELL = 8, BRICK_CELL = 9;

function canvas(w, h) {
  const c = document.createElement('canvas');
  c.width = w; c.height = h;
  return [c, c.getContext('2d')];
}

function tex(c, { srgb = true, repeat = false } = {}) {
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = srgb ? THREE.SRGBColorSpace : THREE.NoColorSpace;
  t.anisotropy = 8;
  if (repeat) t.wrapS = t.wrapT = THREE.RepeatWrapping;
  return t;
}

let atlas = null;
export function buildingAtlas() {
  if (atlas) return atlas;
  const W = 256, H = 512;
  const [c, g] = canvas(W * CELLS, H);
  const [ce, ge] = canvas(W * CELLS, H);
  // A faint glow in the wall itself so buildings read as shapes at night,
  // not just floating windows.
  ge.fillStyle = '#0d0e12';
  ge.fillRect(0, 0, W * CELLS, H);
  const rng = mulberry32(77);
  for (let v = 0; v < 6; v++) {
    const f = facadeTextures(v);
    g.drawImage(f.map.image, v * W, 0, W, H);
    ge.globalCompositeOperation = 'lighten';
    ge.drawImage(f.emissive.image, v * W, 0, W, H);
    ge.globalCompositeOperation = 'source-over';
    // Switch off ~40% of the lit windows: late at night most offices are dark.
    const cw = W / CELL_COLS[v], rh = H / CELL_ROWS[v];
    for (let r = 0; r < CELL_ROWS[v]; r++) {
      const floorOff = rng() < 0.25;
      for (let q = 0; q < CELL_COLS[v]; q++) {
        if (floorOff || rng() < 0.3) {
          ge.fillStyle = '#0d0e12';
          ge.fillRect(v * W + q * cw, r * rh, cw, rh);
        }
      }
    }
  }
  // Roof: tar and gravel with patches.
  g.fillStyle = '#4a4a4c';
  g.fillRect(6 * W, 0, W, H);
  for (let k = 0; k < 3000; k++) {
    const v = 50 + rng() * 60;
    g.fillStyle = `rgb(${v},${v},${v + 3})`;
    g.fillRect(6 * W + rng() * W, rng() * H, 2, 2);
  }
  for (let k = 0; k < 14; k++) {
    g.fillStyle = `rgba(${rng() < 0.5 ? '30,30,32' : '110,110,108'},0.25)`;
    g.fillRect(6 * W + rng() * W, rng() * H, 20 + rng() * 80, 20 + rng() * 80);
  }
  // Glass curtain wall: blue-black with mullions and a sky gradient.
  const grd = g.createLinearGradient(0, 0, 0, H);
  grd.addColorStop(0, '#2a3a52');
  grd.addColorStop(1, '#10161f');
  g.fillStyle = grd;
  g.fillRect(7 * W, 0, W, H);
  g.fillStyle = 'rgba(160,190,220,0.18)';
  for (let x = 0; x < 8; x++) g.fillRect(7 * W + x * (W / 8), 0, 2, H);
  for (let y = 0; y < 12; y++) g.fillRect(7 * W, y * (H / 12), W, 2);
  // A few lit panes in the glass so crowns aren't dead at night.
  for (let k = 0; k < 18; k++) {
    const x = Math.floor(rng() * 8), y = Math.floor(rng() * 12);
    ge.fillStyle = rng() < 0.5 ? 'rgba(170,210,255,0.8)' : 'rgba(255,220,160,0.7)';
    ge.fillRect(7 * W + x * (W / 8) + 3, y * (H / 12) + 3, W / 8 - 6, H / 12 - 6);
  }
  // Warehouse: corrugated metal, roll-up doors along the bottom, a band of
  // high windows. 10.7 px per metre; the bottom of the canvas is ground level.
  {
    const x0 = 8 * W, pxm = W / 24;
    g.fillStyle = '#4a5058';
    g.fillRect(x0, 0, W, H);
    for (let x = 0; x < W; x += 4) {
      g.fillStyle = x % 8 ? 'rgba(255,255,255,0.07)' : 'rgba(0,0,0,0.18)';
      g.fillRect(x0 + x, 0, 2, H);
    }
    for (let k = 0; k < 10; k++) {
      g.fillStyle = `rgba(${rng() < 0.5 ? '120,70,40' : '30,30,34'},0.18)`;
      g.fillRect(x0 + rng() * W, rng() * H, 10 + rng() * 40, 30 + rng() * 120);
    }
    for (let d = 0; d < 4; d++) {
      const dx = x0 + d * 6 * pxm + 1.2 * pxm, dw = 3.6 * pxm, dh = 4.2 * pxm;
      g.fillStyle = '#2c3036';
      g.fillRect(dx, H - dh, dw, dh);
      for (let y = H - dh; y < H; y += 5) { g.fillStyle = 'rgba(255,255,255,0.08)'; g.fillRect(dx, y, dw, 1); }
      // Light spilling under half-open doors, and a lamp above each.
      if (rng() < 0.6) { ge.fillStyle = '#ffcf80'; ge.fillRect(dx, H - 0.45 * pxm, dw, 0.45 * pxm); }
      ge.fillStyle = '#fff0c8';
      ge.fillRect(dx + dw / 2 - 5, H - dh - 0.9 * pxm, 10, 6);
      g.fillStyle = '#d8d0b8';
      g.fillRect(dx + dw / 2 - 5, H - dh - 0.9 * pxm, 10, 6);
    }
    const wy = H - 7.2 * pxm;
    for (let q = 0; q < 8; q++) {
      const wx = x0 + q * 3 * pxm + 0.4 * pxm;
      g.fillStyle = '#1a2230';
      g.fillRect(wx, wy, 2.2 * pxm, 0.9 * pxm);
      if (rng() < 0.35) { ge.fillStyle = rng() < 0.5 ? '#bfe0ff' : '#ffe2a8'; ge.fillRect(wx, wy, 2.2 * pxm, 0.9 * pxm); }
    }
  }
  // Brick apartments: 6 × 12 windows of 3 m pitch.
  {
    const x0 = 9 * W, cw = W / 6, rh = H / 12;
    g.fillStyle = '#6b3b2e';
    g.fillRect(x0, 0, W, H);
    for (let k = 0; k < 2600; k++) {
      g.fillStyle = rng() < 0.5 ? 'rgba(40,20,14,0.35)' : 'rgba(160,110,90,0.25)';
      g.fillRect(x0 + rng() * W, rng() * H, 3, 1.5);
    }
    for (let y = 0; y < H; y += 5) { g.fillStyle = 'rgba(200,180,160,0.08)'; g.fillRect(x0, y, W, 1); }
    for (let r = 0; r < 12; r++) for (let q = 0; q < 6; q++) {
      const wx = x0 + q * cw + cw * 0.27, wy = r * rh + rh * 0.22, ww = cw * 0.46, wh = rh * 0.56;
      g.fillStyle = '#1b1d22';
      g.fillRect(wx, wy, ww, wh);
      g.fillStyle = 'rgba(230,220,200,0.5)';
      g.fillRect(wx - 2, wy + wh, ww + 4, 3);
      if (rng() < 0.42) {
        const col = rng() < 0.8 ? ['#ffd28a', '#ffc070', '#ffe6b0'][Math.floor(rng() * 3)] : '#a8c8ff';
        ge.fillStyle = col;
        ge.globalAlpha = 0.6 + rng() * 0.4;
        ge.fillRect(wx, wy, ww, wh);
        ge.globalAlpha = 1;
        g.fillStyle = col; g.globalAlpha = 0.25; g.fillRect(wx, wy, ww, wh); g.globalAlpha = 1;
      }
    }
  }
  const map = tex(c), emissive = tex(ce);
  atlas = { map, emissive };
  return atlas;
}

// Patch a MeshStandardMaterial so `uv` is in cell-tile units (unbounded) and
// the `cell` attribute picks which atlas column to sample. textureGrad keeps
// mip selection continuous across the fract() wrap.
export function patchAtlasMaterial(material) {
  material.onBeforeCompile = (shader) => {
    shader.vertexShader = shader.vertexShader
      .replace('#include <common>', '#include <common>\nattribute float cell;\nvarying float vCell;\nvarying vec2 vAUv;')
      .replace('#include <uv_vertex>', '#include <uv_vertex>\nvCell = cell;\nvAUv = uv;');
    shader.fragmentShader = shader.fragmentShader
      .replace('#include <common>', `#include <common>
varying float vCell;
varying vec2 vAUv;
vec2 atlasUv;
vec2 atlasDx;
vec2 atlasDy;`)
      .replace('#include <map_fragment>', `
atlasUv = vec2((floor(vCell + 0.5) + fract(vAUv.x)) / ${CELLS}.0, fract(vAUv.y));
// Keep a sliver away from the cell edges to limit bleeding.
atlasUv.x = clamp(atlasUv.x, (floor(vCell + 0.5) + 0.004) / ${CELLS}.0, (floor(vCell + 0.5) + 0.996) / ${CELLS}.0);
atlasDx = dFdx(vAUv) * vec2(1.0 / ${CELLS}.0, 1.0);
atlasDy = dFdy(vAUv) * vec2(1.0 / ${CELLS}.0, 1.0);
#ifdef USE_MAP
  diffuseColor *= textureGrad(map, atlasUv, atlasDx, atlasDy);
#endif
`)
      .replace('#include <emissivemap_fragment>', `
#ifdef USE_EMISSIVEMAP
  totalEmissiveRadiance *= textureGrad(emissiveMap, atlasUv, atlasDx, atlasDy).rgb;
#endif
`);
  };
  material.customProgramCacheKey = () => 'city-atlas';
  return material;
}

// Billboard / neon advertisement. All brands are invented.
const ADS = [
  { t: 'NIGHTSHIFT', s: 'ENERGY DRINK', bg: ['#12002a', '#3a0060'], fg: '#ff3cf0', fg2: '#7cf6ff' },
  { t: 'MERIDIAN MOTORS', s: 'SINCE 1962 · EXIT 43', bg: ['#1a0a00', '#442000'], fg: '#ffb13c', fg2: '#fff2c0' },
  { t: 'NEON NOODLE', s: 'OPEN ALL NIGHT', bg: ['#001a18', '#004a40'], fg: '#35ffc9', fg2: '#ff5a7a' },
  { t: 'CRUISE FM 101.9', s: 'THE SOUND OF THE CITY', bg: ['#07072a', '#1a1a66'], fg: '#8fb0ff', fg2: '#ffd84a' },
  { t: 'VELOCITY TYRES', s: 'GRIP THE NIGHT', bg: ['#1a0000', '#520010'], fg: '#ff4050', fg2: '#ffffff' },
  { t: 'SKYLINE HOTEL', s: 'ROOFTOP BAR · 48TH FLOOR', bg: ['#02101c', '#0a3050'], fg: '#62d6ff', fg2: '#ffe0a0' },
];
const adCache = new Map();
export function adTexture(i) {
  i = ((i % ADS.length) + ADS.length) % ADS.length;
  if (adCache.has(i)) return adCache.get(i);
  const a = ADS[i];
  const W = 1024, H = 384;
  const [c, g] = canvas(W, H);
  const grd = g.createLinearGradient(0, 0, W, H);
  grd.addColorStop(0, a.bg[0]);
  grd.addColorStop(1, a.bg[1]);
  g.fillStyle = grd;
  g.fillRect(0, 0, W, H);
  // Diagonal stripes for some visual energy.
  g.globalAlpha = 0.08;
  g.fillStyle = a.fg;
  for (let x = -H; x < W; x += 60) {
    g.beginPath(); g.moveTo(x, H); g.lineTo(x + H, 0); g.lineTo(x + H + 24, 0); g.lineTo(x + 24, H); g.fill();
  }
  g.globalAlpha = 1;
  g.textAlign = 'center';
  g.textBaseline = 'middle';
  g.font = 'italic 900 118px "Arial Narrow", Arial, sans-serif';
  g.shadowColor = a.fg;
  g.shadowBlur = 28;
  g.fillStyle = a.fg;
  g.fillText(a.t, W / 2, H * 0.42, W * 0.92);
  g.shadowBlur = 0;
  g.fillStyle = '#ffffff';
  g.globalAlpha = 0.9;
  g.fillText(a.t, W / 2, H * 0.42, W * 0.92);
  g.globalAlpha = 1;
  g.font = 'bold 44px "Arial Narrow", Arial, sans-serif';
  g.fillStyle = a.fg2;
  g.fillText(a.s, W / 2, H * 0.78, W * 0.9);
  g.strokeStyle = a.fg;
  g.lineWidth = 10;
  g.strokeRect(8, 8, W - 16, H - 16);
  const t = tex(c);
  adCache.set(i, t);
  return t;
}
export const AD_COUNT = ADS.length;

let tunnelTex = null;
export function tunnelTileTexture() {
  if (tunnelTex) return tunnelTex;
  const S = 256;
  const [c, g] = canvas(S, S);
  // Cream ceramic tiles with thin grout, as in most city road tunnels.
  g.fillStyle = '#8f8a80';
  g.fillRect(0, 0, S, S);
  const rng = mulberry32(5);
  for (let y = 0; y < 8; y++) for (let x = 0; x < 8; x++) {
    const v = 226 + rng() * 14;
    g.fillStyle = `rgb(${v},${v - 3},${v - 10})`;
    g.fillRect(x * 32 + 1.5, y * 32 + 1.5, 29, 29);
    g.fillStyle = 'rgba(255,255,255,0.10)';
    g.fillRect(x * 32 + 2, y * 32 + 2, 27, 6);
  }
  tunnelTex = tex(c, { repeat: true });
  return tunnelTex;
}

let wallTex = null;
export function soundWallTexture() {
  if (wallTex) return wallTex;
  const W = 256, H = 256;
  const [c, g] = canvas(W, H);
  g.fillStyle = '#9d988c';
  g.fillRect(0, 0, W, H);
  const rng = mulberry32(9);
  const img = g.getImageData(0, 0, W, H);
  for (let i = 0; i < W * H; i++) {
    const n = (rng() - 0.5) * 18;
    img.data[i * 4] += n; img.data[i * 4 + 1] += n; img.data[i * 4 + 2] += n;
  }
  g.putImageData(img, 0, 0);
  // Panel joints and vertical ribs.
  g.fillStyle = 'rgba(40,38,34,0.55)';
  g.fillRect(0, 0, 4, H);
  for (let x = 16; x < W; x += 20) {
    g.fillStyle = 'rgba(255,255,255,0.10)';
    g.fillRect(x, 0, 3, H);
    g.fillStyle = 'rgba(0,0,0,0.14)';
    g.fillRect(x + 3, 0, 3, H);
  }
  g.fillStyle = 'rgba(30,28,24,0.4)';
  g.fillRect(0, H - 22, W, 22);
  wallTex = tex(c, { repeat: true });
  return wallTex;
}

let grassTex = null;
export function parkTexture() {
  if (grassTex) return grassTex;
  const S = 256;
  const [c, g] = canvas(S, S);
  g.fillStyle = '#3f6a2e';
  g.fillRect(0, 0, S, S);
  const rng = mulberry32(12);
  for (let k = 0; k < 4000; k++) {
    const v = rng();
    g.fillStyle = v < 0.5 ? 'rgba(80,120,50,0.5)' : 'rgba(40,70,28,0.5)';
    g.fillRect(rng() * S, rng() * S, 2, 3);
  }
  grassTex = tex(c, { repeat: true });
  return grassTex;
}

// Tunnel name / portal sign and the finish banner.
export function bannerTexture(text, { w = 1024, h = 192, bg = '#101014', fg = '#f4f4f4', checker = false, font = 'italic 900 120px "Arial Narrow", Arial, sans-serif' } = {}) {
  const [c, g] = canvas(w, h);
  g.fillStyle = bg;
  g.fillRect(0, 0, w, h);
  if (checker) {
    const sq = h / 4;
    for (let y = 0; y < 4; y++) for (let x = 0; x < w / sq; x++) {
      if (y === 1 || y === 2) continue;
      g.fillStyle = (x + y) % 2 ? '#111' : '#f2f2f2';
      g.fillRect(x * sq, y * sq, sq, sq);
    }
    g.fillStyle = '#111';
    g.fillRect(0, sq, w, sq * 2);
  }
  g.fillStyle = fg;
  g.textAlign = 'center';
  g.textBaseline = 'middle';
  g.font = font;
  g.fillText(text, w / 2, h / 2 + 4, w * 0.9);
  return tex(c);
}

// ── Night-city façades (City.js) ──────────────────────────────
// A second atlas with the same cell layout as buildingAtlas(), drawn with
// every window unlit, plus a glass mask. patchCityMaterial() then decides
// per window, in the shader, which rooms have their lights on, their colour
// temperature and blinds, reflects the city glow in the dark glass and turns
// the ground floor into shopfronts — so each building gets its own pattern
// instead of the atlas repeating the same lit windows every few floors.
const FACADE = {
  //    wall       glass      frame      gx: glass fraction of the bay's width, gt/gb: top/bottom spandrel fractions
  0: { wall: '#595e66', glass: '#16202b', frame: '#2c3036', gx: 0.84, gt: 0.2, gb: 0.12, panes: 2, band: 'rgba(20,22,26,0.35)' },
  1: { wall: '#7d7163', glass: '#1a1f26', frame: '#3b352f', gx: 0.62, gt: 0.3, gb: 0.16, panes: 2, sill: 'rgba(225,215,195,0.45)' },
  2: { wall: '#27313e', glass: '#172434', frame: '#3a4656', gx: 0.94, gt: 0.24, gb: 0.06, panes: 1 },
  3: { wall: '#877a68', glass: '#1c2027', frame: '#4a4238', gx: 0.56, gt: 0.3, gb: 0.12, panes: 2, balcony: true },
  4: { wall: '#2f3743', glass: '#121a26', frame: '#46525f', gx: 0.96, gt: 0.14, gb: 0.04, panes: 1 },
  5: { wall: '#8e8474', glass: '#1d2026', frame: '#4c463c', gx: 0.58, gt: 0.3, gb: 0.2, panes: 2, sill: 'rgba(235,228,210,0.4)' },
  9: { wall: '#6b3b2e', glass: '#1b1d22', frame: '#d8cfc0', gx: 0.46, gt: 0.22, gb: 0.22, panes: 2, brick: true },
};
// Warm-light bias per cell (0 = cool offices … 1 = warm homes) for the shader.
const CELL_WARM = [0.35, 0.55, 0.2, 0.8, 0.15, 0.75, 0.5, 0.25, 0.6, 0.85];

let cityAtlas = null;
export function cityFacadeAtlas() {
  if (cityAtlas) return cityAtlas;
  const W = 256, H = 512;
  const [c, g] = canvas(W * CELLS, H);
  const [ce, ge] = canvas(W * CELLS, H);
  const [cm, gm] = canvas(W * CELLS, H);
  const rng = mulberry32(3131);
  // Roof and warehouse cells come straight from the classic atlas.
  const old = buildingAtlas();
  for (const k of [ROOF_CELL, WAREHOUSE_CELL]) {
    g.drawImage(old.map.image, k * W, 0, W, H, k * W, 0, W, H);
    ge.drawImage(old.emissive.image, k * W, 0, W, H, k * W, 0, W, H);
  }
  gm.fillStyle = '#000';
  gm.fillRect(0, 0, W * CELLS, H);
  for (const [key, st] of Object.entries(FACADE)) {
    const k = +key, x0 = k * W, cols = CELL_COLS[k], rows = CELL_ROWS[k];
    const bw = W / cols, fh = H / rows;
    ge.fillStyle = '#000';
    ge.fillRect(x0, 0, W, H);
    g.fillStyle = st.wall;
    g.fillRect(x0, 0, W, H);
    if (st.brick) {
      for (let q = 0; q < 2600; q++) {
        g.fillStyle = rng() < 0.5 ? 'rgba(40,20,14,0.35)' : 'rgba(160,110,90,0.25)';
        g.fillRect(x0 + rng() * W, rng() * H, 3, 1.5);
      }
      for (let y = 0; y < H; y += 5) { g.fillStyle = 'rgba(200,180,160,0.08)'; g.fillRect(x0, y, W, 1); }
    } else {
      // Panel-to-panel tone variation and rain streaks.
      for (let r = 0; r < rows; r++) for (let q = 0; q < cols; q++) {
        g.fillStyle = `rgba(${rng() < 0.5 ? '255,255,255' : '0,0,0'},${0.02 + rng() * 0.04})`;
        g.fillRect(x0 + q * bw, r * fh, bw, fh);
      }
      for (let q = 0; q < 40; q++) {
        g.fillStyle = 'rgba(0,0,0,0.06)';
        g.fillRect(x0 + rng() * W, rng() * H, 1 + rng() * 2, 20 + rng() * 120);
      }
    }
    for (let r = 0; r < rows; r++) {
      const y = r * fh;
      // The emissive map keeps a baked lit pattern: the shader fades to it
      // once windows are too small to light one by one, so distant towers
      // stay speckled (and mip-filtered) instead of turning into grey slabs.
      const fr = rng(), floorLit = fr < 0.25 ? 0 : fr > 0.92 ? 0.9 : 0.3;
      // Spandrel band / floor slab line.
      if (st.band) { g.fillStyle = st.band; g.fillRect(x0, y + fh * (1 - st.gb) - 1, W, fh * st.gb + 1); }
      if (st.balcony) {
        g.fillStyle = 'rgba(220,210,190,0.45)';
        g.fillRect(x0, y + fh - 3, W, 3);
        g.fillStyle = 'rgba(30,30,32,0.5)';
        g.fillRect(x0, y + fh * 0.62, W, 1);
      }
      for (let q = 0; q < cols; q++) {
        const wx = x0 + q * bw + bw * (1 - st.gx) / 2, ww = bw * st.gx;
        const wy = y + fh * st.gt, wh = fh * (1 - st.gt - st.gb);
        // Frame, then glass inset one pixel, with a sky sheen at the top.
        g.fillStyle = st.frame;
        g.fillRect(wx - 1, wy - 1, ww + 2, wh + 2);
        const grd = g.createLinearGradient(0, wy, 0, wy + wh);
        grd.addColorStop(0, shade(st.glass, 1.6));
        grd.addColorStop(0.45, st.glass);
        grd.addColorStop(1, shade(st.glass, 0.8));
        g.fillStyle = grd;
        g.fillRect(wx, wy, ww, wh);
        gm.fillStyle = '#fff';
        gm.fillRect(wx, wy, ww, wh);
        if (rng() < floorLit) {
          ge.fillStyle = rng() < 0.6 ? ['#ffd28f', '#ffe6bd', '#ffc27a'][Math.floor(rng() * 3)] : ['#cfe4ff', '#e6f2ff'][Math.floor(rng() * 2)];
          ge.globalAlpha = 0.5 + rng() * 0.4;
          ge.fillRect(wx, wy, ww, wh);
          ge.globalAlpha = 1;
        }
        // Mullions between panes.
        g.fillStyle = st.frame;
        gm.fillStyle = '#000';
        for (let p = 1; p < st.panes; p++) {
          const mx = wx + (ww * p) / st.panes - 1;
          g.fillRect(mx, wy, 2, wh);
          gm.fillRect(mx, wy, 2, wh);
        }
        if (st.sill) { g.fillStyle = st.sill; g.fillRect(wx - 2, wy + wh, ww + 4, 3); g.fillRect(wx - 1, wy - 3, ww + 2, 2); }
        if (st.brick) { g.fillStyle = 'rgba(230,220,200,0.5)'; g.fillRect(wx - 2, wy + wh, ww + 4, 3); }
        if (!st.brick && st.gx < 0.8) {
          // Reveal shadow on the upper and left edges of punched windows.
          g.fillStyle = 'rgba(0,0,0,0.35)';
          g.fillRect(wx, wy, ww, 2);
          g.fillRect(wx, wy, 2, wh);
        }
      }
    }
    // Curtain walls: continuous vertical mullions over the spandrels.
    if (st.gx > 0.9) {
      g.fillStyle = shade(st.frame, 1.2);
      for (let q = 0; q <= cols; q++) g.fillRect(x0 + q * bw - 1, 0, 2, H);
    }
  }
  // Curtain-wall glass (crowns, podium glazing): blue-black with a sky
  // gradient; every pane is glass.
  {
    const x0 = GLASS_CELL * W, cols = CELL_COLS[GLASS_CELL], rows = CELL_ROWS[GLASS_CELL];
    const grd = g.createLinearGradient(0, 0, 0, H);
    grd.addColorStop(0, '#2a3a52');
    grd.addColorStop(1, '#10161f');
    g.fillStyle = grd;
    g.fillRect(x0, 0, W, H);
    ge.fillStyle = '#000';
    ge.fillRect(x0, 0, W, H);
    for (let k = 0; k < 20; k++) {
      ge.fillStyle = rng() < 0.5 ? '#aac8f0' : '#f0d8a8';
      ge.fillRect(x0 + Math.floor(rng() * cols) * (W / cols) + 2, Math.floor(rng() * rows) * (H / rows) + 3, W / cols - 4, H / rows - 5);
    }
    gm.fillStyle = '#fff';
    gm.fillRect(x0, 0, W, H);
    g.fillStyle = 'rgba(160,190,220,0.22)';
    gm.fillStyle = '#000';
    for (let x = 0; x < cols; x++) { g.fillRect(x0 + x * (W / cols), 0, 2, H); gm.fillRect(x0 + x * (W / cols), 0, 2, H); }
    for (let y = 0; y < rows; y++) { g.fillRect(x0, y * (H / rows), W, 2); gm.fillRect(x0, y * (H / rows), W, 3); }
  }
  const map = tex(c), emissive = tex(ce), mask = tex(cm, { srgb: false });
  cityAtlas = { map, emissive, mask };
  return cityAtlas;
}

function shade(hex, k) {
  const n = parseInt(hex.slice(1), 16);
  const f = (v) => Math.max(0, Math.min(255, Math.round(v * k)));
  return `rgb(${f(n >> 16)},${f((n >> 8) & 255)},${f(n & 255)})`;
}

// Pack a per-building random seed into the `cell` attribute: the classic
// patch reads floor(cell + 0.5), so the fraction is free. The seed is
// quantised to 64 levels and stored mid-bin so interpolation noise can't
// flip it between pixels (the shader hash would turn that into sparkle).
export const cellSeed = (cell, seed) => cell - 0.4 + ((Math.floor(seed * 64) + 0.5) / 64) * 0.8;

// Like patchAtlasMaterial, for cityFacadeAtlas(): lit windows, glass
// reflections, street-level shopfronts and light spill are computed per
// pixel. `ground` is the pavement height (shopfronts sit on it).
export function patchCityMaterial(material, { mask, ground = 0 }) {
  const u = { uMask: { value: mask }, uGround: { value: ground } };
  material.userData.cityUniforms = u;
  const arr = (a, f) => a.map(f).join(', ');
  material.onBeforeCompile = (shader) => {
    Object.assign(shader.uniforms, u);
    shader.vertexShader = shader.vertexShader
      .replace('#include <common>', '#include <common>\nattribute float cell;\nvarying float vCell;\nvarying vec2 vAUv;\nvarying vec3 vCWorld;\nvarying float vCNy;')
      .replace('#include <uv_vertex>', '#include <uv_vertex>\nvCell = cell;\nvAUv = uv;')
      .replace('#include <project_vertex>', '#include <project_vertex>\nvCWorld = (modelMatrix * vec4(transformed, 1.0)).xyz;\nvCNy = normalize(mat3(modelMatrix) * objectNormal).y;');
    shader.fragmentShader = shader.fragmentShader
      .replace('#include <common>', `#include <common>
varying float vCell;
varying vec2 vAUv;
varying vec3 vCWorld;
varying float vCNy;
uniform sampler2D uMask;
uniform float uGround;
vec2 atlasUv;
vec2 atlasDx;
vec2 atlasDy;
const vec2 CGRID[${CELLS}] = vec2[${CELLS}](${arr(CELL_COLS, (c, i) => `vec2(${c}.0, ${CELL_ROWS[i]}.0)`)});
const vec2 CTILE[${CELLS}] = vec2[${CELLS}](${arr(CELL_TILE, (t) => `vec2(${t[0].toFixed(2)}, ${t[1].toFixed(2)})`)});
const float CWARM[${CELLS}] = float[${CELLS}](${arr(CELL_WARM, (w) => w.toFixed(2))});
float cHash(vec2 p) { p = fract(p * vec2(123.34, 456.21)); p += dot(p, p + 45.32); return fract(p.x * p.y); }
// Shopfront zone: 1 on the ground floor of walls (not glass crowns, roofs,
// sheds), with the shop's own random roll in y.
float shopZone(int ci) {
  float hgt = vCWorld.y - uGround;
  bool wallCell = ci <= 5 || ci == 9;
  return (wallCell && abs(vCNy) < 0.5 && hgt < 4.6) ? 1.0 : 0.0;
}
`)
      .replace('#include <map_fragment>', `
int cIdx = int(floor(vCell + 0.5));
float cSeed = floor((fract(vCell + 0.5) - 0.1) * 80.0) * 1.37 + 3.0;
atlasUv = vec2((float(cIdx) + fract(vAUv.x)) / ${CELLS}.0, fract(vAUv.y));
atlasUv.x = clamp(atlasUv.x, (float(cIdx) + 0.004) / ${CELLS}.0, (float(cIdx) + 0.996) / ${CELLS}.0);
atlasDx = dFdx(vAUv) * vec2(1.0 / ${CELLS}.0, 1.0);
atlasDy = dFdy(vAUv) * vec2(1.0 / ${CELLS}.0, 1.0);
float cShop = shopZone(cIdx);
float cHgt = vCWorld.y - uGround;
float cM = vAUv.x * CTILE[cIdx].x;             // metres along the wall
float cShopRoll = cHash(vec2(floor(cM / 7.0), cSeed));
#ifdef USE_MAP
  diffuseColor *= textureGrad(map, atlasUv, atlasDx, atlasDy);
#endif
float cGlass = textureGrad(uMask, atlasUv, atlasDx, atlasDy).r * (1.0 - cShop);
if (cShop > 0.5) {
  // Shop glass, fascia above, plinth below; roller shutters on closed shops.
  float inGlass = step(0.35, cHgt) * step(cHgt, 3.3);
  float fascia = step(3.4, cHgt) * step(cHgt, 4.35);
  vec3 dc = cShopRoll < 0.68 ? vec3(0.05, 0.06, 0.07) : vec3(0.32, 0.33, 0.34) * (0.8 + 0.2 * step(0.5, fract(cHgt * 6.0)));
  dc = mix(vec3(0.16, 0.15, 0.14), dc, inGlass);
  dc = mix(dc, vec3(0.1, 0.1, 0.12), fascia);
  diffuseColor.rgb = dc;
}
`)
      .replace('#include <roughnessmap_fragment>', `#include <roughnessmap_fragment>
roughnessFactor = mix(roughnessFactor, 0.12, cGlass);`)
      .replace('#include <metalnessmap_fragment>', `#include <metalnessmap_fragment>
metalnessFactor = mix(metalnessFactor, 0.6, cGlass);`)
      .replace('#include <emissivemap_fragment>', `
vec3 cEm = vec3(0.0);
#ifdef USE_EMISSIVEMAP
  cEm = textureGrad(emissiveMap, atlasUv, atlasDx, atlasDy).rgb;
#endif
{
  vec2 wp = vAUv * CGRID[cIdx];
  vec2 wi = floor(wp);
  vec2 wf = fract(wp);
  float occ = mix(0.08, 0.55, pow(cHash(vec2(cSeed, 1.3)), 1.3));
  float fr = cHash(vec2(wi.y, cSeed));
  float fState = fr < 0.25 ? 0.0 : (fr > 0.93 ? 0.95 : occ);
  float roomW = 2.0 + floor(cHash(vec2(wi.y + 7.0, cSeed)) * 4.0);
  float room = floor((wi.x + floor(cHash(vec2(cSeed, wi.y + 3.0)) * 4.0)) / roomW);
  float rh = cHash(vec2(room, wi.y) + cSeed * 1.7);
  float lit = step(rh, fState);
  float warmB = clamp(CWARM[cIdx] + (cHash(vec2(cSeed, 5.0)) - 0.5) * 0.5, 0.0, 1.0);
  float tp = cHash(vec2(room * 1.3, wi.y + 11.0) + cSeed);
  vec3 cWarm = vec3(1.0, 0.7, 0.4), cNeut = vec3(1.0, 0.9, 0.74), cCool = vec3(0.68, 0.84, 1.0);
  vec3 wc = tp < warmB ? mix(cWarm, cNeut, tp / max(warmB, 0.01) * 0.6) : (tp < 0.5 + warmB * 0.5 ? cNeut : cCool);
  float inten = 0.5 + 0.65 * cHash(vec2(room, wi.y + 2.0) + cSeed);
  // Ceiling lights: brighter toward the top of the pane; some blinds, some
  // half-drawn curtains.
  inten *= 0.72 + 0.42 * smoothstep(0.1, 0.95, wf.y);
  // Fine detail fades out before it can alias into sparkle.
  float fw = max(fwidth(wp.x), fwidth(wp.y));
  float bl = cHash(vec2(room + 5.0, wi.y) + cSeed * 3.1);
  if (bl < 0.22) inten *= mix(0.75, 0.5 + 0.5 * step(0.4, fract(wf.y * 7.0)), 1.0 - smoothstep(0.02, 0.07, fw));
  else if (bl < 0.36) inten *= mix(0.65, mix(0.3, 1.0, step(wf.x, 0.55)), 1.0 - smoothstep(0.1, 0.3, fw));
  vec3 win = wc * inten * lit;
  // Beyond ~1 window per pixel, fall back to the building's average glow.
  // Once windows shrink to a few pixels, cross-fade to the baked pattern
  // (scaled by this building's occupancy), which mip-filters cleanly.
  fw = smoothstep(0.2, 0.5, fw);
  win = mix(win * cGlass, cEm * clamp(occ * 2.4, 0.35, 1.3), fw);
  // Dark glass mirrors the orange city glow low down and the night above.
  vec3 V = normalize(vViewPosition);
  vec3 Rw = (vec4(reflect(-V, normal), 0.0) * viewMatrix).xyz;
  float fres = 0.1 + 0.9 * pow(1.0 - clamp(dot(V, normal), 0.0, 1.0), 4.0);
  vec3 env = Rw.y > 0.0 ? mix(vec3(0.16, 0.1, 0.1), vec3(0.015, 0.02, 0.04), smoothstep(0.0, 0.5, Rw.y)) : vec3(0.06, 0.05, 0.045);
  float glassK = (cIdx == 2 || cIdx == 4 || cIdx == 7) ? 1.0 : 0.55;
  vec3 refl = env * min(fres, 0.7) * glassK * 0.75 * (1.0 - lit * (1.0 - fw));
  bool winCell = cIdx <= 5 || cIdx == 7 || cIdx == 9;
  cEm = winCell ? vec3(0.007, 0.008, 0.011) + win + cGlass * refl : cEm;
  // Street-light spill washing the lower floors.
  cEm += (1.0 - cGlass) * (1.0 - cShop) * vec3(0.5, 0.36, 0.22) * 0.2 * exp(-max(cHgt, 0.0) / 7.0) * step(abs(vCNy), 0.5);
  if (cShop > 0.5) {
    float inGlass = step(0.35, cHgt) * step(cHgt, 3.3);
    float fascia = step(3.4, cHgt) * step(cHgt, 4.35);
    float pick = fract(cShopRoll * 7.13);
    vec3 sc = pick < 0.5 ? vec3(1.0, 0.68, 0.36) : pick < 0.7 ? vec3(0.8, 0.9, 1.0) : pick < 0.8 ? vec3(1.0, 0.25, 0.6) : pick < 0.9 ? vec3(0.2, 0.8, 0.9) : vec3(1.0, 0.45, 0.12);
    float open = step(cShopRoll, 0.68);
    float mfw = fwidth(cM);
    float mull = mix(0.95, step(0.05, fract(cM / 1.75)), 1.0 - smoothstep(0.08, 0.25, mfw));
    // Brightest at the ceiling and mid-shop; darker counters and displays
    // below, a dark awning strip on top, shelving in between.
    float sx = fract(cM / 7.0);
    float glow = (0.5 + 0.5 * smoothstep(0.3, 3.0, cHgt)) * (0.65 + 0.35 * sin(sx * 3.14159)) * mull;
    glow *= mix(1.0, 0.4, step(cHgt, 1.05));
    glow *= mix(1.0, 0.7 + 0.6 * cHash(vec2(floor(cM * 1.3), cSeed)), 1.0 - smoothstep(0.05, 0.15, mfw));
    glow *= 1.0 - 0.85 * step(2.95, cHgt);
    vec3 shop = sc * 0.6 * glow * inGlass * open;
    // Fascia sign: blocky "lettering" in a saturated colour.
    float letters = mix(0.65, step(0.35, cHash(vec2(floor(cM * 2.6), floor(cHgt * 4.0) + cSeed))), 1.0 - smoothstep(0.03, 0.1, mfw));
    vec3 signC = pick < 0.5 ? vec3(1.0, 0.25, 0.4) : pick < 0.75 ? vec3(0.25, 0.8, 1.0) : vec3(1.0, 0.75, 0.25);
    shop += fascia * step(cShopRoll, 0.8) * mix(signC * 0.25, signC * 1.3, letters) * step(0.12, fract(cM / 7.0)) * step(fract(cM / 7.0), 0.88);
    // Closed shops: a dim security light over the shutter.
    shop += inGlass * (1.0 - open) * vec3(0.9, 0.8, 0.6) * 0.1 * smoothstep(2.4, 3.3, cHgt);
    float lod = smoothstep(0.5, 2.0, mfw);
    cEm = mix(shop, sc * 0.22, lod);
  }
}
totalEmissiveRadiance *= cEm;
`);
  };
  material.customProgramCacheKey = () => 'city-atlas-lit';
  return material;
}
