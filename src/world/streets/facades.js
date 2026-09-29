import * as THREE from 'three';
import { mulberry32 } from '../../util/math.js';

// Upper-floor façades for Downtown Streets: a 4×4 atlas of 256 px cells,
// each one a few bays by a few floors drawn at close-up detail (sills,
// mullions, spandrels, AC units, curtains). The emissive map has EVERY
// window lit; the shader then decides per window, from its bay/floor index
// across the whole building and a per-building seed, whether it is on and
// what colour it is. So a tile can repeat up a 200 m tower without the
// pattern of lights repeating, offices light whole floors while flats light
// odd windows, and street light bounces warm up the lowest floors so no
// wall is a dead black slab.
//
// Geometry feeds three things per vertex:
//   uv     in tile units (unbounded, like the city atlas)
//   cell   atlas cell + 16 × building seed
//   fdata  (street level y, bounce strength, bounce hue; 0 = sodium warm)

export const F = {
  NAPT: 0, NTILE: 1, BRICK: 2, RIBBON: 3, CURTAIN: 4, STONE: 5, BANDS: 6, FINS: 7,
  DARK: 8, ROOF: 9, CROWN: 10, PANEL: 11, FAR_RES: 12, FAR_OFF: 13, HOTEL: 14, MECH: 15,
};
// Per cell: metres one repeat covers [w, h], window grid [cols, rows], and
// the lighting kind: 0 as drawn, 1 flats, 2 offices, 3 mostly dark offices.
export const F_TILE = [
  [12, 12], [12, 12], [12, 12.8], [12, 12], [12, 16], [12, 16], [12, 16], [12, 16],
  [12, 16], [16, 16], [12, 6], [4, 4], [24, 24], [24, 32], [12, 12], [8, 4],
];
const GRID = [
  [4, 4, 1], [4, 4, 1], [4, 4, 1], [4, 4, 2], [8, 4, 2], [4, 4, 2], [6, 4, 2], [6, 4, 2],
  [8, 4, 3], [1, 1, 0], [1, 1, 0], [1, 1, 0], [8, 8, 1], [8, 8, 2], [4, 4, 1], [1, 1, 0],
];
// Floor height of each façade cell, for stacking floors exactly.
export const floorH = (cell) => F_TILE[cell][1] / GRID[cell][1];

const S = 256;

function canvas(w, h) {
  const c = document.createElement('canvas');
  c.width = w; c.height = h;
  return [c, c.getContext('2d')];
}
function tex(c, srgb = true) {
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = srgb ? THREE.SRGBColorSpace : THREE.NoColorSpace;
  t.anisotropy = 8;
  return t;
}

// Speckle + streaks so plain wall reads as a material, not a fill.
function grime(g, x, y, w, h, rng, n = 500, dark = 0.06) {
  for (let q = 0; q < n; q++) {
    g.fillStyle = rng() < 0.5 ? `rgba(0,0,0,${dark * rng()})` : `rgba(255,255,255,${dark * 0.6 * rng()})`;
    g.fillRect(x + rng() * w, y + rng() * h, 1 + rng() * 3, 1 + rng() * 3);
  }
  for (let q = 0; q < 6; q++) {
    const gx = x + rng() * w, gy = y + rng() * h * 0.6;
    const grd = g.createLinearGradient(0, gy, 0, gy + h * 0.4);
    grd.addColorStop(0, `rgba(20,18,16,${0.12 * rng()})`); grd.addColorStop(1, 'rgba(20,18,16,0)');
    g.fillStyle = grd; g.fillRect(gx, gy, 2 + rng() * 5, h * 0.4);
  }
}

// A lit room seen through a window: warm or cool base, a ceiling glow, a
// curtain or blind, and furniture silhouettes. The shader tints and dims it.
function room(ge, x, y, w, h, rng, office = false) {
  const grd = ge.createLinearGradient(0, y, 0, y + h);
  if (office) {
    grd.addColorStop(0, '#f4f8ff'); grd.addColorStop(0.25, '#b8c4d0'); grd.addColorStop(1, '#58606a');
  } else {
    grd.addColorStop(0, '#ffe2b0'); grd.addColorStop(0.6, '#d89a58'); grd.addColorStop(1, '#8a5a30');
  }
  ge.fillStyle = grd; ge.fillRect(x, y, w, h);
  if (office) {
    // Ceiling light strips and desk partitions.
    ge.fillStyle = '#ffffff';
    for (let q = x + 3; q < x + w - 4; q += 11) ge.fillRect(q, y + 2, 7, 2);
    ge.fillStyle = 'rgba(20,24,30,0.55)';
    ge.fillRect(x, y + h * 0.62, w, h * 0.38);
    for (let q = 0; q < 3; q++) ge.fillRect(x + rng() * w, y + h * 0.45, 3 + rng() * 5, h * 0.2);
  } else {
    // Curtains drawn part way from either side, or a blind from the top.
    ge.fillStyle = `rgba(${rng() < 0.5 ? '120,40,30' : '40,50,70'},0.55)`;
    if (rng() < 0.5) {
      ge.fillRect(x, y, w * (0.15 + rng() * 0.25), h);
      ge.fillRect(x + w * (0.7 + rng() * 0.2), y, w, h);
    } else ge.fillRect(x, y, w, h * (0.2 + rng() * 0.5));
    ge.fillStyle = 'rgba(30,18,10,0.6)';
    if (rng() < 0.6) ge.fillRect(x + w * rng() * 0.7, y + h * 0.65, w * 0.3, h * 0.35); // sofa, shelf
    if (rng() < 0.4) { ge.beginPath(); ge.arc(x + w * (0.2 + rng() * 0.6), y + h * 0.3, 3, 0, 7); ge.fillStyle = '#fff4d8'; ge.fill(); }
  }
}

let atlas = null;
export function facadeAtlas() {
  if (atlas) return atlas;
  const [c, g] = canvas(S * 4, S * 4);
  const [ce, ge] = canvas(S * 4, S * 4);
  ge.fillStyle = '#000'; ge.fillRect(0, 0, S * 4, S * 4);
  const rng = mulberry32(4242);
  const at = (k) => [(k % 4) * S, Math.floor(k / 4) * S];
  // Window grid helper: fn(x, y, w, h, col, row) for each bay; row 0 is the
  // lowest floor (canvas bottom), matching the shader's floor index.
  const bays = (k, fn) => {
    const [x0, y0] = at(k), [cols, rows] = GRID[k];
    const cw = S / cols, rh = S / rows;
    for (let r = 0; r < rows; r++) for (let q = 0; q < cols; q++) fn(x0 + q * cw, y0 + S - (r + 1) * rh, cw, rh, q, r);
  };
  const clip = (k, fn) => { const [x0, y0] = at(k); g.save(); ge.save(); g.beginPath(); g.rect(x0, y0, S, S); g.clip(); ge.beginPath(); ge.rect(x0, y0, S, S); ge.clip(); fn(x0, y0); g.restore(); ge.restore(); };

  // 0 · Neon District flats: stained concrete, small framed windows, AC
  // units and a drainpipe.
  clip(F.NAPT, (x0, y0) => {
    g.fillStyle = '#6f6a64'; g.fillRect(x0, y0, S, S);
    grime(g, x0, y0, S, S, rng, 900, 0.1);
    bays(F.NAPT, (x, y, w, h) => {
      g.fillStyle = 'rgba(0,0,0,0.18)'; g.fillRect(x, y + h - 5, w, 3); // floor slab line
      const wx = x + w * 0.2, wy = y + h * 0.2, ww = w * 0.6, wh = h * 0.52;
      g.fillStyle = '#8a8680'; g.fillRect(wx - 3, wy - 3, ww + 6, wh + 8);
      g.fillStyle = '#1a1f26'; g.fillRect(wx, wy, ww, wh);
      g.fillStyle = '#9a968e'; g.fillRect(wx + ww / 2 - 1, wy, 2, wh);
      room(ge, wx, wy, ww, wh, rng);
      ge.fillStyle = '#000'; ge.fillRect(wx + ww / 2 - 1, wy, 2, wh);
      if (rng() < 0.55) {
        // AC unit under the window.
        g.fillStyle = '#c8c6c0'; g.fillRect(wx + ww * 0.55, wy + wh + 6, ww * 0.42, h * 0.14);
        g.fillStyle = '#5a5a58';
        for (let q = 0; q < 5; q++) g.fillRect(wx + ww * 0.58 + q * 4, wy + wh + 9, 2, h * 0.1);
      }
    });
    g.fillStyle = '#4c4a46'; g.fillRect(x0 + S - 8, y0, 4, S);
  });

  // 1 · Tiled flats with balconies (railings, laundry, plants).
  clip(F.NTILE, (x0, y0) => {
    g.fillStyle = '#b8b2a4'; g.fillRect(x0, y0, S, S);
    g.fillStyle = 'rgba(0,0,0,0.08)';
    for (let y = 0; y < S; y += 6) g.fillRect(x0, y0 + y, S, 1);
    for (let x = 0; x < S; x += 12) g.fillRect(x0 + x, y0, 1, S);
    grime(g, x0, y0, S, S, rng, 500, 0.08);
    bays(F.NTILE, (x, y, w, h) => {
      const wx = x + 6, wy = y + 8, ww = w - 12, wh = h * 0.72;
      g.fillStyle = '#222830'; g.fillRect(wx, wy, ww, wh);
      room(ge, wx, wy, ww, wh, rng);
      // Sliding door frames.
      g.fillStyle = '#707478'; g.fillRect(wx + ww * 0.5 - 1, wy, 3, wh); g.fillRect(wx, wy, ww, 2);
      ge.fillStyle = '#000'; ge.fillRect(wx + ww * 0.5 - 1, wy, 3, wh);
      // Balcony slab and railing in front.
      const by = y + h * 0.62;
      g.fillStyle = '#d6d0c4'; g.fillRect(x + 2, y + h - 7, w - 4, 7);
      g.fillStyle = '#3a3c40';
      g.fillRect(x + 2, by, w - 4, 2);
      for (let q = x + 4; q < x + w - 3; q += 4) g.fillRect(q, by, 1.5, y + h - 7 - by);
      ge.fillStyle = 'rgba(0,0,0,0.6)';
      ge.fillRect(x + 2, by, w - 4, 2);
      for (let q = x + 4; q < x + w - 3; q += 4) ge.fillRect(q, by, 1.5, y + h - 7 - by);
      if (rng() < 0.35) { // laundry
        const cs = ['#d84a4a', '#f0f0e8', '#4a78c8', '#e8c040'];
        for (let q = 0; q < 4; q++) { g.fillStyle = cs[Math.floor(rng() * 4)]; g.fillRect(x + 8 + q * 12, by - 14, 9, 12); ge.fillStyle = '#000'; ge.fillRect(x + 8 + q * 12, by - 14, 9, 12); }
      }
      if (rng() < 0.4) { g.fillStyle = '#2e5a2a'; g.beginPath(); g.arc(x + w - 12, by - 4, 7, 0, 7); g.fill(); }
    });
  });

  // 2 · Brick walk-ups: arched lintels, stone sills, a belt course.
  clip(F.BRICK, (x0, y0) => {
    g.fillStyle = '#6e3a2c'; g.fillRect(x0, y0, S, S);
    for (let y = 0; y < S; y += 4) {
      for (let x = (y / 4) % 2 ? 0 : 5; x < S; x += 10) {
        const v = rng();
        g.fillStyle = v < 0.3 ? 'rgba(40,16,10,0.35)' : v < 0.6 ? 'rgba(170,100,76,0.25)' : 'rgba(120,60,40,0.15)';
        g.fillRect(x0 + x, y0 + y, 9, 3);
      }
    }
    bays(F.BRICK, (x, y, w, h, q, r) => {
      if (r === 0) { g.fillStyle = '#b8ab98'; g.fillRect(x, y + h - 6, w, 6); }
      const wx = x + w * 0.24, wy = y + h * 0.22, ww = w * 0.52, wh = h * 0.56;
      g.fillStyle = '#5a2a1e'; g.beginPath(); g.ellipse(wx + ww / 2, wy, ww / 2 + 4, 8, 0, Math.PI, 0); g.fill();
      g.fillStyle = '#1b1d22'; g.fillRect(wx, wy, ww, wh);
      g.fillStyle = '#d8d0c0'; g.fillRect(wx - 3, wy + wh, ww + 6, 4);
      g.fillStyle = '#e8e0d0'; g.fillRect(wx, wy + wh * 0.5 - 1, ww, 2); g.fillRect(wx + ww / 2 - 1, wy, 2, wh);
      room(ge, wx, wy, ww, wh, rng);
      ge.fillStyle = '#000'; ge.fillRect(wx, wy + wh * 0.5 - 1, ww, 2); ge.fillRect(wx + ww / 2 - 1, wy, 2, wh);
    });
  });

  // 3 · Mid-century offices: concrete frame with strip windows.
  clip(F.RIBBON, (x0, y0) => {
    g.fillStyle = '#8c8a84'; g.fillRect(x0, y0, S, S);
    grime(g, x0, y0, S, S, rng, 600, 0.09);
    bays(F.RIBBON, (x, y, w, h) => {
      const wy = y + h * 0.3, wh = h * 0.5;
      g.fillStyle = '#1c232c'; g.fillRect(x + 3, wy, w - 6, wh);
      g.fillStyle = '#a8a69e'; g.fillRect(x + w / 3, wy, 2, wh); g.fillRect(x + (2 * w) / 3, wy, 2, wh);
      g.fillStyle = '#6c6a64'; g.fillRect(x, y + h - 4, w, 4);
      room(ge, x + 3, wy, w - 6, wh, rng, true);
      ge.fillStyle = '#000'; ge.fillRect(x + w / 3, wy, 2, wh); ge.fillRect(x + (2 * w) / 3, wy, 2, wh);
    });
    g.fillStyle = 'rgba(0,0,0,0.25)';
    for (let q = 0; q < 4; q++) g.fillRect(x0 + q * 64, y0, 4, S);
  });

  // 4 · Curtain wall: blue-grey vision glass, mullions every 1.5 m, dark
  // spandrel panels at each slab.
  clip(F.CURTAIN, (x0, y0) => {
    const grd = g.createLinearGradient(0, y0, 0, y0 + S);
    grd.addColorStop(0, '#3a4a60'); grd.addColorStop(1, '#1a2230');
    g.fillStyle = grd; g.fillRect(x0, y0, S, S);
    bays(F.CURTAIN, (x, y, w, h) => {
      const sp = h * 0.3; // spandrel at the bottom of each floor
      g.fillStyle = '#141a22'; g.fillRect(x, y + h - sp, w, sp);
      g.fillStyle = 'rgba(140,170,210,0.12)'; g.fillRect(x, y + h - sp, w, 2);
      g.fillStyle = `rgba(150,180,220,${0.05 + rng() * 0.1})`; g.fillRect(x + 1, y, w - 2, h - sp);
      room(ge, x + 1, y, w - 2, h - sp, rng, true);
      g.fillStyle = '#8a96a4'; g.fillRect(x, y, 2, h);
      ge.fillStyle = '#000'; ge.fillRect(x, y, 2, h);
    });
  });

  // 5 · Limestone grid with deep punched windows.
  clip(F.STONE, (x0, y0) => {
    g.fillStyle = '#c4b8a2'; g.fillRect(x0, y0, S, S);
    grime(g, x0, y0, S, S, rng, 800, 0.08);
    g.fillStyle = 'rgba(80,70,56,0.25)';
    for (let y = 0; y < S; y += 16) g.fillRect(x0, y0 + y, S, 1);
    bays(F.STONE, (x, y, w, h) => {
      const wx = x + w * 0.16, wy = y + h * 0.14, ww = w * 0.68, wh = h * 0.64;
      g.fillStyle = '#8a7e6a'; g.fillRect(wx - 4, wy - 4, ww + 8, wh + 8); // reveal shadow
      g.fillStyle = '#20262e'; g.fillRect(wx, wy, ww, wh);
      g.fillStyle = '#5a5e62'; g.fillRect(wx + ww / 2 - 1, wy, 3, wh);
      room(ge, wx, wy, ww, wh, rng, true);
      ge.fillStyle = '#000'; ge.fillRect(wx + ww / 2 - 1, wy, 3, wh);
      g.fillStyle = '#d8ccb6'; g.fillRect(wx - 5, wy + wh + 4, ww + 10, 3);
    });
  });

  // 6 · Ribbon glass between dark aluminium bands.
  clip(F.BANDS, (x0, y0) => {
    g.fillStyle = '#26292e'; g.fillRect(x0, y0, S, S);
    bays(F.BANDS, (x, y, w, h) => {
      const wy = y + h * 0.08, wh = h * 0.6;
      g.fillStyle = '#2c3a4a'; g.fillRect(x, wy, w, wh);
      g.fillStyle = 'rgba(170,200,230,0.15)'; g.fillRect(x, wy, w, 3);
      room(ge, x, wy, w, wh, rng, true);
      g.fillStyle = '#50565e'; g.fillRect(x, wy, 1.5, wh);
      ge.fillStyle = '#000'; ge.fillRect(x, wy, 1.5, wh);
      g.fillStyle = 'rgba(255,255,255,0.06)'; g.fillRect(x, y + h * 0.72, w, 2);
    });
  });

  // 7 · Bronze fins: deep vertical fins between tall glass.
  clip(F.FINS, (x0, y0) => {
    g.fillStyle = '#1e2630'; g.fillRect(x0, y0, S, S);
    bays(F.FINS, (x, y, w, h) => {
      g.fillStyle = '#18202a'; g.fillRect(x, y + h - 10, w, 10);
      room(ge, x + 7, y + 2, w - 14, h - 14, rng, true);
    });
    for (let q = 0; q <= 6; q++) {
      const fx = x0 + (q * S) / 6 - 5;
      const grd = g.createLinearGradient(fx, 0, fx + 10, 0);
      grd.addColorStop(0, '#5a4028'); grd.addColorStop(0.5, '#a8804e'); grd.addColorStop(1, '#4a3420');
      g.fillStyle = grd; g.fillRect(fx, y0, 10, S);
      ge.fillStyle = '#000'; ge.fillRect(fx, y0, 10, S);
    }
  });

  // 8 · Dark reflective glass for crowns and sleek shafts.
  clip(F.DARK, (x0, y0) => {
    const grd = g.createLinearGradient(x0, y0, x0 + S, y0 + S);
    grd.addColorStop(0, '#2a3446'); grd.addColorStop(0.5, '#141a24'); grd.addColorStop(1, '#222a38');
    g.fillStyle = grd; g.fillRect(x0, y0, S, S);
    bays(F.DARK, (x, y, w, h) => {
      g.fillStyle = 'rgba(160,190,230,0.2)'; g.fillRect(x, y, 1, h); g.fillRect(x, y + h - 1, w, 1);
      room(ge, x + 1, y + 1, w - 2, h - 8, rng, true);
    });
  });

  // 9 · Roof: tar and gravel with patches and walkway pads.
  clip(F.ROOF, (x0, y0) => {
    g.fillStyle = '#48484a'; g.fillRect(x0, y0, S, S);
    for (let q = 0; q < 2400; q++) { const v = 50 + rng() * 60; g.fillStyle = `rgb(${v},${v},${v + 3})`; g.fillRect(x0 + rng() * S, y0 + rng() * S, 2, 2); }
    for (let q = 0; q < 10; q++) { g.fillStyle = `rgba(${rng() < 0.5 ? '30,30,32' : '110,110,108'},0.25)`; g.fillRect(x0 + rng() * S, y0 + rng() * S, 20 + rng() * 60, 20 + rng() * 60); }
    g.fillStyle = 'rgba(160,160,150,0.35)';
    for (let q = 0; q < 8; q++) g.fillRect(x0 + 20 + q * 28, y0 + 120, 24, 16);
  });

  // 10 · Crown: lit louvres and a glowing top band (always on).
  clip(F.CROWN, (x0, y0) => {
    g.fillStyle = '#1a1c20'; g.fillRect(x0, y0, S, S);
    const grd = ge.createLinearGradient(0, y0, 0, y0 + S);
    grd.addColorStop(0, '#fff4e0'); grd.addColorStop(0.35, '#e0b878'); grd.addColorStop(1, '#402a10');
    ge.fillStyle = grd; ge.fillRect(x0, y0 + 20, S, S - 40);
    ge.fillStyle = 'rgba(0,0,0,0.55)';
    for (let y = 24; y < S - 20; y += 10) ge.fillRect(x0, y0 + y, S, 4);
    ge.fillStyle = '#000';
    for (let x = 0; x < S; x += 21) ge.fillRect(x0 + x, y0, 4, S);
    g.fillStyle = '#3a3a3e';
    for (let x = 0; x < S; x += 21) g.fillRect(x0 + x, y0, 4, S);
    g.fillRect(x0, y0, S, 20); g.fillRect(x0, y0 + S - 20, S, 20);
  });

  // 11 · Plain panel: metal/stone cladding for piers, penthouses, parapets.
  clip(F.PANEL, (x0, y0) => {
    g.fillStyle = '#9a968e'; g.fillRect(x0, y0, S, S);
    grime(g, x0, y0, S, S, rng, 700, 0.1);
    g.fillStyle = 'rgba(0,0,0,0.2)';
    for (let q = 0; q < S; q += 64) { g.fillRect(x0 + q, y0, 2, S); g.fillRect(x0, y0 + q, S, 2); }
  });

  // 12 · Far flats: 8 × 8 small windows (seen from a distance).
  clip(F.FAR_RES, (x0, y0) => {
    g.fillStyle = '#5a5450'; g.fillRect(x0, y0, S, S);
    grime(g, x0, y0, S, S, rng, 400, 0.12);
    bays(F.FAR_RES, (x, y, w, h) => {
      g.fillStyle = '#16191e'; g.fillRect(x + w * 0.22, y + h * 0.2, w * 0.56, h * 0.55);
      ge.fillStyle = rng() < 0.7 ? '#ffc880' : '#fff0d0'; ge.fillRect(x + w * 0.22, y + h * 0.2, w * 0.56, h * 0.55);
      ge.fillStyle = 'rgba(0,0,0,0.5)'; ge.fillRect(x + w * 0.22, y + h * 0.2, w * 0.56, h * 0.55 * rng());
    });
  });

  // 13 · Far offices: 8 × 8 bays of glass.
  clip(F.FAR_OFF, (x0, y0) => {
    g.fillStyle = '#1e242c'; g.fillRect(x0, y0, S, S);
    bays(F.FAR_OFF, (x, y, w, h) => {
      g.fillStyle = '#2a3442'; g.fillRect(x + 2, y + 3, w - 4, h - 10);
      ge.fillStyle = '#dfe8ff'; ge.fillRect(x + 2, y + 3, w - 4, h - 10);
      ge.fillStyle = 'rgba(0,0,0,0.4)'; ge.fillRect(x + 2, y + h * 0.5, w - 4, h * 0.3);
    });
  });

  // 14 · Painted hotel: pastel stucco, shuttered windows, pilasters.
  clip(F.HOTEL, (x0, y0) => {
    g.fillStyle = '#d8d0c4'; g.fillRect(x0, y0, S, S);
    grime(g, x0, y0, S, S, rng, 600, 0.08);
    bays(F.HOTEL, (x, y, w, h) => {
      g.fillStyle = 'rgba(255,255,255,0.35)'; g.fillRect(x, y + h - 6, w, 4);
      const wx = x + w * 0.3, wy = y + h * 0.2, ww = w * 0.4, wh = h * 0.58;
      g.fillStyle = '#1c2026'; g.fillRect(wx, wy, ww, wh);
      g.fillStyle = '#3a6a5a';
      g.fillRect(wx - ww * 0.42, wy, ww * 0.38, wh); g.fillRect(wx + ww * 1.04, wy, ww * 0.38, wh);
      g.fillStyle = 'rgba(0,0,0,0.3)';
      for (let q = wy; q < wy + wh; q += 5) { g.fillRect(wx - ww * 0.42, q, ww * 0.38, 1.5); g.fillRect(wx + ww * 1.04, q, ww * 0.38, 1.5); }
      room(ge, wx, wy, ww, wh, rng);
    });
    g.fillStyle = 'rgba(0,0,0,0.12)';
    for (let q = 0; q < 4; q++) g.fillRect(x0 + q * 64, y0, 5, S);
  });

  // 15 · Mechanical penthouse: louvred screen.
  clip(F.MECH, (x0, y0) => {
    g.fillStyle = '#4a4c50'; g.fillRect(x0, y0, S, S);
    g.fillStyle = '#2a2c30';
    for (let y = 8; y < S; y += 9) g.fillRect(x0, y0 + y, S, 4);
    g.fillStyle = '#6a6c70';
    for (let x = 0; x < S; x += 64) g.fillRect(x0 + x, y0, 4, S);
  });

  atlas = { map: tex(c), emissive: tex(ce) };
  return atlas;
}

// Cheap integer-ish hash, the same in both shaders.
const HASH = `
float fHash(vec2 p) { p = fract(p * vec2(0.1031, 0.1030)); p += dot(p, p.yx + 33.33); return fract((p.x + p.y) * p.x); }
vec3 fHue(float h) { return clamp(abs(mod(h * 6.0 + vec3(0.0, 4.0, 2.0), 6.0) - 3.0) - 1.0, 0.0, 1.0); }
`;

// Per-window lighting. `w` is the window index across the whole building,
// `seed` the building's. kind 1: flats (odd warm windows, the odd TV blue);
// kind 2: offices (whole floors lit, cool or warm, a few late desks on the
// rest); kind 3: mostly dark glass.
const WINDOW_LIGHT = `
vec3 windowLight(vec2 w, float seed, float kind) {
  float h1 = fHash(w + seed * vec2(17.13, 31.7));
  if (kind < 0.5) return vec3(1.0);
  if (kind < 1.5) {
    float on = step(h1, 0.36);
    float h2 = fHash(w * 1.7 + seed + 3.0);
    vec3 tint = h2 < 0.55 ? vec3(1.0, 0.72, 0.42) : h2 < 0.8 ? vec3(1.0, 0.88, 0.7) : h2 < 0.93 ? vec3(0.5, 0.66, 1.0) : vec3(1.0, 0.45, 0.75);
    return tint * on * (0.45 + 0.7 * fHash(w + 9.1 + seed)) + vec3(0.012, 0.012, 0.018);
  }
  float share = kind < 2.5 ? 0.2 + 0.45 * fHash(vec2(seed, 1.7)) : 0.07;
  float fl = fHash(vec2(w.y * 1.31, seed * 3.1 + 7.0));
  float grp = fHash(vec2(floor(w.x / 3.0), w.y) + seed * 5.3);
  float floorOn = step(fl, share);
  float on = floorOn * step(0.12, grp) + (1.0 - floorOn) * step(h1, 0.05);
  vec3 tint = fHash(vec2(w.y, seed + 11.0)) < 0.7 ? vec3(0.78, 0.9, 1.05) : vec3(1.05, 0.9, 0.7);
  return tint * on * (0.6 + 0.4 * fHash(vec2(w.y, seed))) + vec3(0.01, 0.014, 0.022);
}
`;

// Façade material: see the header. Street bounce: the street's own light
// (sodium lamps, or the neon hue in fdata.z) washing up the lowest floors,
// plus a faint sky/city ambient so walls read as shapes.
export function facadeMaterial() {
  const A = facadeAtlas();
  const m = new THREE.MeshStandardMaterial({
    map: A.map, emissiveMap: A.emissive, emissive: 0xffffff, emissiveIntensity: 1.1, roughness: 0.62, metalness: 0.2,
  });
  const spec = GRID.map(([c, r, k]) => `vec3(${c}.0, ${r}.0, ${k}.0)`).join(', ');
  m.onBeforeCompile = (shader) => {
    shader.vertexShader = shader.vertexShader
      .replace('#include <common>', '#include <common>\nattribute float cell;\nattribute vec3 fdata;\nvarying float vCell;\nvarying vec2 vAUv;\nvarying vec3 vFData;\nvarying float vWY;')
      .replace('#include <uv_vertex>', '#include <uv_vertex>\nvCell = cell;\nvAUv = uv;\nvFData = fdata;')
      .replace('#include <project_vertex>', '#include <project_vertex>\nvWY = (modelMatrix * vec4(transformed, 1.0)).y;');
    shader.fragmentShader = shader.fragmentShader
      .replace('#include <common>', `#include <common>
varying float vCell;
varying vec2 vAUv;
varying vec3 vFData;
varying float vWY;
vec2 fUv; vec2 fDx; vec2 fDy; float fIdx; float fSeed;
const vec3 FSPEC[16] = vec3[16](${spec});
${HASH}
${WINDOW_LIGHT}`)
      .replace('#include <map_fragment>', `
fIdx = mod(floor(vCell + 0.5), 16.0);
fSeed = floor((vCell + 0.5) / 16.0);
{
  vec2 cxy = vec2(mod(fIdx, 4.0), floor(fIdx / 4.0));
  fUv = vec2((cxy.x + clamp(fract(vAUv.x), 0.004, 0.996)) / 4.0, (3.0 - cxy.y + clamp(fract(vAUv.y), 0.004, 0.996)) / 4.0);
  fDx = dFdx(vAUv) * 0.25; fDy = dFdy(vAUv) * 0.25;
}
#ifdef USE_MAP
  diffuseColor *= textureGrad(map, fUv, fDx, fDy);
#endif
`)
      .replace('#include <emissivemap_fragment>', `
#ifdef USE_EMISSIVEMAP
{
  vec3 sp = FSPEC[int(fIdx)];
  totalEmissiveRadiance *= textureGrad(emissiveMap, fUv, fDx, fDy).rgb * windowLight(floor(vAUv * sp.xy), fSeed, sp.z);
}
#endif
{
  float hh = max(vWY - vFData.x, 0.0);
  vec3 bc = vFData.z > 0.001 ? mix(vec3(1.0, 0.7, 0.45), fHue(vFData.z), 0.65) : vec3(1.0, 0.68, 0.4);
  totalEmissiveRadiance += diffuseColor.rgb * (bc * vFData.y * exp(-hh / 7.0) + vec3(0.03, 0.032, 0.045));
}
`);
  };
  m.customProgramCacheKey = () => 'streets-facade';
  return m;
}

export { HASH as FACADE_HASH, WINDOW_LIGHT as FACADE_WINDOW_LIGHT };
