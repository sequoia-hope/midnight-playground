import * as THREE from 'three';
import { mulberry32 } from '../util/math.js';

// Canvas-generated textures. Nothing is loaded from disk, so the game has no
// asset pipeline and starts instantly from a static server.

const cache = new Map();
function cached(key, make) {
  if (!cache.has(key)) cache.set(key, make());
  return cache.get(key);
}

function canvas(w, h) {
  const c = document.createElement('canvas');
  c.width = w; c.height = h;
  return [c, c.getContext('2d')];
}

function toTexture(c, { repeat = true, srgb = true, aniso = 8 } = {}) {
  const t = new THREE.CanvasTexture(c);
  if (repeat) t.wrapS = t.wrapT = THREE.RepeatWrapping;
  t.colorSpace = srgb ? THREE.SRGBColorSpace : THREE.NoColorSpace;
  t.anisotropy = aniso;
  t.generateMipmaps = true;
  t.minFilter = THREE.LinearMipmapLinearFilter;
  return t;
}

// Soft grey noise used to break up flat vertex colours (multiplies).
export function detailTexture() {
  return cached('detail', () => {
    const S = 256;
    const [c, g] = canvas(S, S);
    const img = g.createImageData(S, S);
    const rng = mulberry32(11);
    // Value noise at a few scales, tileable by wrapping lattice.
    const layers = [8, 16, 32, 64];
    const lat = layers.map((n) => { const a = new Float32Array(n * n); for (let i = 0; i < a.length; i++) a[i] = rng(); return a; });
    const sample = (a, n, x, y) => {
      const fx = x * n / S, fy = y * n / S;
      const x0 = Math.floor(fx), y0 = Math.floor(fy), tx = fx - x0, ty = fy - y0;
      const at = (i, j) => a[((j % n + n) % n) * n + ((i % n + n) % n)];
      const sx = tx * tx * (3 - 2 * tx), sy = ty * ty * (3 - 2 * ty);
      return (at(x0, y0) * (1 - sx) + at(x0 + 1, y0) * sx) * (1 - sy) + (at(x0, y0 + 1) * (1 - sx) + at(x0 + 1, y0 + 1) * sx) * sy;
    };
    for (let y = 0; y < S; y++) for (let x = 0; x < S; x++) {
      let v = 0, amp = 0.5;
      layers.forEach((n, k) => { v += sample(lat[k], n, x, y) * amp; amp *= 0.6; });
      v += (rng() - 0.5) * 0.12;
      const g8 = Math.max(0, Math.min(255, 150 + v * 110));
      const i = (y * S + x) * 4;
      img.data[i] = img.data[i + 1] = img.data[i + 2] = g8; img.data[i + 3] = 255;
    }
    g.putImageData(img, 0, 0);
    return toTexture(c);
  });
}

// Tileable value noise on an S×S grid: sum of lattice layers (cells per
// side) with falling amplitude. Shared by the procedural data textures.
function tileNoise(S, rng, layers, gain = 0.6) {
  const lat = layers.map((n) => { const a = new Float32Array(n * n); for (let i = 0; i < a.length; i++) a[i] = rng(); return a; });
  const out = new Float32Array(S * S);
  let norm = 0, amp = 0.5;
  for (const _ of layers) { norm += amp; amp *= gain; }
  for (let y = 0; y < S; y++) for (let x = 0; x < S; x++) {
    let v = 0; amp = 0.5;
    layers.forEach((n, k) => {
      const fx = x * n / S, fy = y * n / S;
      const x0 = Math.floor(fx), y0 = Math.floor(fy), tx = fx - x0, ty = fy - y0;
      const a = lat[k];
      const at = (i, j) => a[((j % n + n) % n) * n + ((i % n + n) % n)];
      const sx = tx * tx * (3 - 2 * tx), sy = ty * ty * (3 - 2 * ty);
      v += ((at(x0, y0) * (1 - sx) + at(x0 + 1, y0) * sx) * (1 - sy) + (at(x0, y0 + 1) * (1 - sx) + at(x0 + 1, y0 + 1) * sx) * sy) * amp;
      amp *= gain;
    });
    out[y * S + x] = v / norm;
  }
  return out;
}

// Tileable cellular noise: stones. Each jittered cell gets its own tone,
// darkening toward the gap with its neighbours (F2 - F1 small). ~0 in the
// gaps, 0.3..1 on the stones.
function tileCells(S, rng, n) {
  const pts = new Float32Array(n * n * 3);
  for (let i = 0; i < n * n; i++) { pts[i * 3] = rng(); pts[i * 3 + 1] = rng(); pts[i * 3 + 2] = rng(); }
  const out = new Float32Array(S * S);
  for (let y = 0; y < S; y++) for (let x = 0; x < S; x++) {
    const fx = x * n / S, fy = y * n / S, cx = Math.floor(fx), cy = Math.floor(fy);
    let f1 = 9, f2 = 9, tone = 0;
    for (let j = -1; j <= 1; j++) for (let i = -1; i <= 1; i++) {
      const gx = cx + i, gy = cy + j;
      const k = (((gy % n) + n) % n) * n + (((gx % n) + n) % n);
      const d = Math.hypot(gx + pts[k * 3] - fx, gy + pts[k * 3 + 1] - fy);
      if (d < f1) { f2 = f1; f1 = d; tone = pts[k * 3 + 2]; } else if (d < f2) f2 = d;
    }
    const edge = Math.min(1, (f2 - f1) * 5);
    out[y * S + x] = edge * (0.3 + 0.7 * tone) * (1 - f1 * 0.35);
  }
  return out;
}

// Terrain detail, four independent tileable channels in one texture so the
// ground shader gets several scales of variation from a few fetches:
//   R  soft multi-octave noise (identical to detailTexture's grey)
//   G  fine grain: grass tufts / sand grain, high frequency
//   B  cellular: pebbles and dry cracks
//   A  broad smooth noise, for macro variation when sampled very large
// Linear data (not sRGB) — the shader interprets the values itself.
export function terrainDetailTexture() {
  return cached('terrainDetail', () => {
    const S = 256;
    const data = new Uint8Array(S * S * 4);
    // R: same generator and seed as detailTexture so the base look is kept.
    const rng = mulberry32(11);
    const layers = [8, 16, 32, 64];
    const lat = layers.map((n) => { const a = new Float32Array(n * n); for (let i = 0; i < a.length; i++) a[i] = rng(); return a; });
    const sample = (a, n, x, y) => {
      const fx = x * n / S, fy = y * n / S;
      const x0 = Math.floor(fx), y0 = Math.floor(fy), tx = fx - x0, ty = fy - y0;
      const at = (i, j) => a[((j % n + n) % n) * n + ((i % n + n) % n)];
      const sx = tx * tx * (3 - 2 * tx), sy = ty * ty * (3 - 2 * ty);
      return (at(x0, y0) * (1 - sx) + at(x0 + 1, y0) * sx) * (1 - sy) + (at(x0, y0 + 1) * (1 - sx) + at(x0 + 1, y0 + 1) * sx) * sy;
    };
    for (let y = 0; y < S; y++) for (let x = 0; x < S; x++) {
      let v = 0, amp = 0.5;
      layers.forEach((n, k) => { v += sample(lat[k], n, x, y) * amp; amp *= 0.6; });
      v += (rng() - 0.5) * 0.12;
      data[(y * S + x) * 4] = Math.max(0, Math.min(255, 150 + v * 110));
    }
    const r2 = mulberry32(77);
    const grain = tileNoise(S, r2, [64, 128], 0.7);
    const cells = tileCells(S, r2, 24);
    const broad = tileNoise(S, r2, [3, 6, 12], 0.55);
    for (let i = 0; i < S * S; i++) {
      const g = (grain[i] - 0.5) * 1.6 + (r2() - 0.5) * 0.35 + 0.5;
      data[i * 4 + 1] = Math.max(0, Math.min(255, g * 255));
      data[i * 4 + 2] = Math.max(0, Math.min(255, cells[i] * 255));
      data[i * 4 + 3] = Math.max(0, Math.min(255, ((broad[i] - 0.5) * 1.8 + 0.5) * 255));
    }
    const t = new THREE.DataTexture(data, S, S, THREE.RGBAFormat);
    t.wrapS = t.wrapT = THREE.RepeatWrapping;
    t.colorSpace = THREE.NoColorSpace;
    t.anisotropy = 8;
    t.generateMipmaps = true;
    t.minFilter = THREE.LinearMipmapLinearFilter;
    t.magFilter = THREE.LinearFilter;
    t.needsUpdate = true;
    return t;
  });
}

// Asphalt: dark aggregate with speckles and faint tyre-wear bands.
// Layered rock: horizontal strata with cracks, sampled triplanar on cliffs.
export function rockTexture() {
  return cached('rock', () => {
    const S = 256;
    const [c, g] = canvas(S, S);
    const img = g.createImageData(S, S);
    const rng = mulberry32(31);
    const bands = new Float32Array(S);
    let v = 0.8;
    for (let y = 0; y < S; y++) { if (rng() < 0.08) v = 0.62 + rng() * 0.45; bands[y] = v; }
    const jitter = new Float32Array(S);
    for (let x = 0; x < S; x++) jitter[x] = Math.sin(x / S * Math.PI * 2 * 3) * 4 + Math.sin(x / S * Math.PI * 2 * 7 + 1) * 2;
    for (let y = 0; y < S; y++) for (let x = 0; x < S; x++) {
      const yy = ((y + Math.round(jitter[x])) % S + S) % S;
      let b = bands[yy] + (rng() - 0.5) * 0.16;
      if (rng() < 0.004) b *= 0.5;
      const g8 = Math.max(0, Math.min(255, b * 200));
      const i = (y * S + x) * 4;
      img.data[i] = g8; img.data[i + 1] = g8 * 0.97; img.data[i + 2] = g8 * 0.93; img.data[i + 3] = 255;
    }
    g.putImageData(img, 0, 0);
    // Vertical cracks.
    g.strokeStyle = 'rgba(20,18,16,0.5)';
    for (let k = 0; k < 26; k++) {
      let x = rng() * S, y = rng() * S;
      g.lineWidth = 0.6 + rng() * 1.4;
      g.beginPath(); g.moveTo(x, y);
      for (let j = 0; j < 5; j++) { x += (rng() - 0.5) * 10; y += rng() * 22; g.lineTo(x, y); }
      g.stroke();
    }
    return toTexture(c);
  });
}

export function asphaltTexture(tone = 0) {
  return cached('asphalt' + tone, () => {
    const S = 512;
    const [c, g] = canvas(S, S);
    const base = tone === 1 ? [70, 70, 72] : tone === 2 ? [58, 58, 62] : [66, 64, 62];
    g.fillStyle = `rgb(${base})`;
    g.fillRect(0, 0, S, S);
    const rng = mulberry32(5 + tone);
    const img = g.getImageData(0, 0, S, S);
    for (let i = 0; i < S * S; i++) {
      const n = (rng() - 0.5) * 34 + (rng() < 0.03 ? 40 : 0) - (rng() < 0.02 ? 30 : 0);
      img.data[i * 4] += n; img.data[i * 4 + 1] += n; img.data[i * 4 + 2] += n;
    }
    g.putImageData(img, 0, 0);
    // Patches and cracks.
    for (let k = 0; k < 18; k++) {
      g.fillStyle = `rgba(${rng() < 0.5 ? '20,20,22' : '110,108,104'},${0.05 + rng() * 0.08})`;
      g.beginPath();
      g.ellipse(rng() * S, rng() * S, 20 + rng() * 80, 10 + rng() * 40, rng() * 3, 0, 7);
      g.fill();
    }
    g.strokeStyle = 'rgba(15,15,15,0.35)';
    g.lineWidth = 1.2;
    for (let k = 0; k < 10; k++) {
      let x = rng() * S, y = rng() * S;
      g.beginPath(); g.moveTo(x, y);
      for (let j = 0; j < 6; j++) { x += (rng() - 0.5) * 40; y += (rng() - 0.5) * 40; g.lineTo(x, y); }
      g.stroke();
    }
    return toTexture(c);
  });
}

// Gravel verge: fines with a soft mottle, then stones of mixed size and
// tone, each lit from one side with a contact shadow so they read as lumps.
export function gravelTexture() {
  return cached('gravel', () => {
    const S = 256;
    const [c, g] = canvas(S, S);
    const rng = mulberry32(19);
    const mott = tileNoise(S, rng, [6, 12, 24, 48], 0.6);
    const img = g.createImageData(S, S);
    for (let i = 0; i < S * S; i++) {
      const v = 124 + (mott[i] - 0.5) * 60 + (rng() - 0.5) * 26;
      img.data[i * 4] = v * 1.03; img.data[i * 4 + 1] = v * 0.98; img.data[i * 4 + 2] = v * 0.9; img.data[i * 4 + 3] = 255;
    }
    g.putImageData(img, 0, 0);
    const stone = (x, y, r, v) => {
      // Draw with wrap so the texture tiles.
      for (const ox of [-S, 0, S]) for (const oy of [-S, 0, S]) {
        const X = x + ox, Y = y + oy;
        if (X < -r * 2 || X > S + r * 2 || Y < -r * 2 || Y > S + r * 2) continue;
        g.fillStyle = 'rgba(20,18,15,0.45)';
        g.beginPath(); g.ellipse(X + r * 0.35, Y + r * 0.35, r * 1.05, r * 0.85, 0, 0, 7); g.fill();
        g.fillStyle = `rgb(${v * 1.02 | 0},${v * 0.97 | 0},${v * 0.9 | 0})`;
        g.beginPath(); g.ellipse(X, Y, r, r * 0.8, rng() * 3, 0, 7); g.fill();
        g.fillStyle = 'rgba(255,250,240,0.22)';
        g.beginPath(); g.ellipse(X - r * 0.3, Y - r * 0.3, r * 0.45, r * 0.35, 0, 0, 7); g.fill();
      }
    };
    for (let k = 0; k < 1700; k++) {
      const r = 0.7 + Math.pow(rng(), 3) * 3.2;
      stone(rng() * S, rng() * S, r, 70 + rng() * 120);
    }
    return toTexture(c);
  });
}

export function concreteTexture() {
  return cached('concrete', () => {
    const S = 256;
    const [c, g] = canvas(S, S);
    g.fillStyle = '#b9b6ae';
    g.fillRect(0, 0, S, S);
    const rng = mulberry32(23);
    const img = g.getImageData(0, 0, S, S);
    for (let i = 0; i < S * S; i++) {
      const n = (rng() - 0.5) * 22;
      img.data[i * 4] += n; img.data[i * 4 + 1] += n; img.data[i * 4 + 2] += n;
    }
    g.putImageData(img, 0, 0);
    g.fillStyle = 'rgba(60,55,50,0.18)';
    for (let k = 0; k < 20; k++) g.fillRect(rng() * S, rng() * S, 2 + rng() * 30, 1 + rng() * 60);
    g.fillStyle = 'rgba(0,0,0,0.25)';
    g.fillRect(0, 0, S, 2);
    return toTexture(c);
  });
}

// Chevron sign (yellow with black arrow). dir: 1 = points right.
export function chevronTexture() {
  return cached('chevron', () => {
    const [c, g] = canvas(128, 128);
    g.fillStyle = '#f2c230'; g.fillRect(0, 0, 128, 128);
    g.fillStyle = '#111';
    g.beginPath();
    g.moveTo(30, 14); g.lineTo(62, 14); g.lineTo(100, 64); g.lineTo(62, 114); g.lineTo(30, 114); g.lineTo(68, 64);
    g.closePath(); g.fill();
    g.strokeStyle = '#111'; g.lineWidth = 6; g.strokeRect(3, 3, 122, 122);
    return toTexture(c, { repeat: false });
  });
}

// Checkerboard banner/start line.
export function checkerTexture(n = 8) {
  return cached('checker' + n, () => {
    const [c, g] = canvas(256, 64);
    const w = 256 / (n * 2), h = 64 / 4;
    for (let y = 0; y < 4; y++) for (let x = 0; x < n * 2; x++) {
      g.fillStyle = (x + y) % 2 ? '#111' : '#f4f4f4';
      g.fillRect(x * w, y * h, w, h);
    }
    return toTexture(c);
  });
}

// Generic text sign. Returns {texture, aspect}.
export function signTexture(lines, { bg = '#0b6b3a', fg = '#fff', border = '#fff', w = 512, h = 256, font = 'bold 64px "Arial Narrow", Arial, sans-serif', arrow = null } = {}) {
  const key = 'sign:' + lines.join('|') + bg + fg + w + h + arrow;
  return cached(key, () => {
    const [c, g] = canvas(w, h);
    g.fillStyle = bg;
    g.fillRect(0, 0, w, h);
    if (border) {
      g.strokeStyle = border; g.lineWidth = Math.max(4, h * 0.03);
      const r = h * 0.06;
      g.beginPath();
      g.roundRect ? g.roundRect(10, 10, w - 20, h - 20, r) : g.rect(10, 10, w - 20, h - 20);
      g.stroke();
    }
    g.fillStyle = fg;
    g.textAlign = 'center';
    g.textBaseline = 'middle';
    g.font = font;
    const lh = h / (lines.length + (arrow ? 1 : 0) + 0.4);
    lines.forEach((l, i) => g.fillText(l, w / 2, lh * (i + 0.7 + 0.2)));
    if (arrow) {
      const y = lh * (lines.length + 0.7 + 0.1);
      g.save(); g.translate(w / 2, y); g.rotate(arrow === 'right' ? Math.PI / 4 : arrow === 'left' ? -Math.PI / 4 : 0);
      g.beginPath(); g.moveTo(0, -lh * 0.4); g.lineTo(lh * 0.3, -lh * 0.05); g.lineTo(lh * 0.1, -lh * 0.05);
      g.lineTo(lh * 0.1, lh * 0.4); g.lineTo(-lh * 0.1, lh * 0.4); g.lineTo(-lh * 0.1, -lh * 0.05); g.lineTo(-lh * 0.3, -lh * 0.05);
      g.closePath(); g.fill(); g.restore();
    }
    const t = toTexture(c, { repeat: false });
    return { texture: t, aspect: w / h };
  });
}

// Radial glow, used for light pools on the road and sprite halos.
export function glowTexture() {
  return cached('glow', () => {
    const S = 128;
    const [c, g] = canvas(S, S);
    const grd = g.createRadialGradient(S / 2, S / 2, 0, S / 2, S / 2, S / 2);
    grd.addColorStop(0, 'rgba(255,255,255,1)');
    grd.addColorStop(0.35, 'rgba(255,255,255,0.45)');
    grd.addColorStop(1, 'rgba(255,255,255,0)');
    g.fillStyle = grd;
    g.fillRect(0, 0, S, S);
    return toTexture(c, { repeat: false });
  });
}

export function smokeTexture() {
  return cached('smoke', () => {
    const S = 64;
    const [c, g] = canvas(S, S);
    const grd = g.createRadialGradient(S / 2, S / 2, 0, S / 2, S / 2, S / 2);
    grd.addColorStop(0, 'rgba(255,255,255,0.9)');
    grd.addColorStop(0.5, 'rgba(255,255,255,0.35)');
    grd.addColorStop(1, 'rgba(255,255,255,0)');
    g.fillStyle = grd;
    g.fillRect(0, 0, S, S);
    return toTexture(c, { repeat: false });
  });
}

// Building façade with a grid of windows. `lit` fraction are warm/cool lit.
// Returns {map, emissive} — the emissive map holds only the lit windows so
// the city can light up as night falls by raising emissiveIntensity.
export function facadeTextures(variant = 0) {
  return cached('facade' + variant, () => {
    const W = 256, H = 512;
    const rng = mulberry32(100 + variant);
    const styles = [
      { wall: '#4a4f58', win: '#1c2430', cols: 8, rows: 20, gap: 0.28, lit: 0.42 },
      { wall: '#6b6258', win: '#20242a', cols: 6, rows: 16, gap: 0.35, lit: 0.36 },
      { wall: '#2b3440', win: '#15202c', cols: 10, rows: 26, gap: 0.16, lit: 0.5 }, // glass tower
      { wall: '#7a6f64', win: '#262626', cols: 5, rows: 14, gap: 0.4, lit: 0.3 },
      { wall: '#3a3d44', win: '#10161e', cols: 12, rows: 30, gap: 0.12, lit: 0.55 }, // curtain wall
      { wall: '#8b8074', win: '#2a2622', cols: 4, rows: 10, gap: 0.42, lit: 0.45 }, // low-rise brick-ish
    ];
    const st = styles[variant % styles.length];
    const [c, g] = canvas(W, H);
    const [ce, ge] = canvas(W, H);
    g.fillStyle = st.wall; g.fillRect(0, 0, W, H);
    ge.fillStyle = '#000'; ge.fillRect(0, 0, W, H);
    const cw = W / st.cols, rh = H / st.rows;
    const warm = ['#ffd99a', '#ffe7b8', '#fff2d6', '#ffcf80'];
    const cool = ['#cfe6ff', '#b8d4ff', '#e8f4ff'];
    for (let r = 0; r < st.rows; r++) {
      // Whole floors tend to be lit together (offices).
      const floorLit = rng() < 0.35 ? 0.85 : st.lit * 0.6;
      for (let q = 0; q < st.cols; q++) {
        const x = q * cw + cw * st.gap * 0.5, y = r * rh + rh * st.gap * 0.5;
        const w = cw * (1 - st.gap), h = rh * (1 - st.gap);
        g.fillStyle = st.win;
        g.fillRect(x, y, w, h);
        g.fillStyle = 'rgba(255,255,255,0.06)';
        g.fillRect(x, y, w, h * 0.3);
        if (rng() < floorLit) {
          const col = rng() < 0.7 ? warm[Math.floor(rng() * warm.length)] : cool[Math.floor(rng() * cool.length)];
          ge.fillStyle = col;
          ge.globalAlpha = 0.55 + rng() * 0.45;
          ge.fillRect(x, y, w, h);
          ge.globalAlpha = 1;
          g.fillStyle = col;
          g.globalAlpha = 0.25;
          g.fillRect(x, y, w, h);
          g.globalAlpha = 1;
        }
      }
    }
    const map = toTexture(c);
    const emissive = toTexture(ce);
    return { map, emissive, cols: st.cols, rows: st.rows };
  });
}
