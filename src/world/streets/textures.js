import * as THREE from 'three';
import { mulberry32 } from '../../util/math.js';
import { FACADE_HASH, FACADE_WINDOW_LIGHT } from './facades.js';

// Canvas textures for Downtown Streets: a street-level façade atlas (shop
// fronts, rowhouses, lobbies), a neon sign atlas, pavement and the
// red/white barrier stripe. All names on signs are invented.

function canvas(w, h) {
  const c = document.createElement('canvas');
  c.width = w; c.height = h;
  return [c, c.getContext('2d')];
}
function tex(c, { srgb = true, repeat = true } = {}) {
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = srgb ? THREE.SRGBColorSpace : THREE.NoColorSpace;
  t.anisotropy = 8;
  if (repeat) t.wrapS = t.wrapT = THREE.RepeatWrapping;
  return t;
}

// ── Street-level façade atlas ────────────────────────────────────
// A 4×3 grid of 384 px cells, with the metres one repeat covers [w, h].
// `cell` in the geometry is the cell index + 16 × a per-building seed, so the
// shader can vary the lights building by building (see patchStreetAtlas).
export const S_CELLS = 12;
export const SHOP_A = 0, SHOP_B = 1, ROWHOUSE = 2, STUCCO = 3, SHUTTER = 4, LOBBY = 5,
  KONBINI = 6, ROW_GROUND = 7, AWNING_C = 8, VENDING = 9, STALL = 10, POSTER = 11;
export const S_TILE = [
  [10, 4.6],   // shop fronts: display windows and a door under a sign band
  [9, 4.6],    // bar / restaurant: small warm windows, a lit doorway
  [5.4, 3.4],  // rowhouse floor: two tall sash windows with trim (tinted)
  [6, 3.4],    // plain stucco wall with small windows (tinted)
  [8, 4.6],    // closed shop: roller shutter, one lit sign box
  [12, 6],     // office lobby: double-height glass, reception, ceiling lights
  [10, 4.6],   // convenience store: bright shelves behind full glass
  [5.4, 3.5],  // rowhouse street storey: garage door and raised entry (tinted)
  [1.2, 1],    // awning fabric stripes (tinted), lit through from below
  [1, 1.9],    // drinks vending machine front
  [3, 2.4],    // street food stall front: noren curtain, counter, menu
  [1.4, 2],    // lit poster panel (bus shelters, kiosks)
];
// Window grid and lighting kind per cell (see facades.js windowLight): 0 = as
// drawn with a per-building brightness, 1 = flats.
const S_GRID = [[1, 1, 0], [1, 1, 0], [2, 1, 1], [2, 1, 1], [1, 1, 0], [1, 1, 0], [1, 1, 0], [1, 1, 0], [1, 1, 0], [1, 1, 0], [1, 1, 0], [1, 1, 0]];
const SC = 384;

let sAtlas = null;
export function streetAtlas() {
  if (sAtlas) return sAtlas;
  const S = SC;
  const [c, g] = canvas(S * 4, S * 3);
  const [ce, ge] = canvas(S * 4, S * 3);
  ge.fillStyle = '#000'; ge.fillRect(0, 0, S * 4, S * 3);
  const rng = mulberry32(311);
  const at = (k) => [(k % 4) * S, Math.floor(k / 4) * S];
  const cell = (k, fn) => {
    const [x0, y0] = at(k);
    g.save(); ge.save();
    g.beginPath(); g.rect(x0, y0, S, S); g.clip();
    ge.beginPath(); ge.rect(x0, y0, S, S); ge.clip();
    g.translate(x0, y0); ge.translate(x0, y0);
    fn();
    g.restore(); ge.restore();
  };
  const speck = (n, a) => { for (let q = 0; q < n; q++) { g.fillStyle = `rgba(${rng() < 0.5 ? '0,0,0' : '255,255,255'},${a * rng()})`; g.fillRect(rng() * S, rng() * S, 2, 2); } };
  // Goods on shelves: rows of small coloured boxes, drawn into both maps.
  const goods = (x, y, w, h, bright) => {
    for (let sy = y + 18; sy < y + h - 8; sy += 30) {
      ge.fillStyle = 'rgba(0,0,0,0.5)'; ge.fillRect(x, sy, w, 4);
      for (let gx = x + 2; gx < x + w - 6; gx += 5 + rng() * 7) {
        const hue = Math.floor(rng() * 360), gh = 8 + rng() * 14;
        ge.fillStyle = `hsl(${hue},${50 + rng() * 40}%,${bright ? 55 + rng() * 25 : 25 + rng() * 25}%)`;
        ge.fillRect(gx, sy - gh, 3 + rng() * 5, gh);
      }
    }
  };
  // A little person silhouette inside a lit window.
  const person = (x, y, s) => {
    ge.fillStyle = 'rgba(10,8,8,0.8)';
    ge.beginPath(); ge.arc(x, y, 5 * s, 0, 7); ge.fill();
    ge.fillRect(x - 7 * s, y + 6 * s, 14 * s, 30 * s);
  };

  // Shop fronts: dark frame, big display windows lit warm or cool.
  for (const [k, warm] of [[SHOP_A, false], [SHOP_B, true]]) {
    cell(k, () => {
      g.fillStyle = warm ? '#2e2420' : '#26282c'; g.fillRect(0, 0, S, S);
      speck(600, 0.08);
      // Sign band (the neon signs sit over it) with a lit box sign.
      g.fillStyle = '#131216'; g.fillRect(0, 0, S, 64);
      g.fillStyle = '#3a3a40'; g.fillRect(0, 62, S, 6);
      ge.fillStyle = warm ? '#4a2008' : '#0a2440'; ge.fillRect(14, 12, S - 28, 38);
      ge.fillStyle = warm ? '#ffb060' : '#9fe0ff';
      ge.font = '700 26px "Arial Narrow", Arial, sans-serif'; ge.textAlign = 'center'; ge.textBaseline = 'middle';
      ge.fillText(warm ? 'BAR · GRILL · LATE' : 'FASHION · GIFTS', S / 2, 32);
      const panes = k === SHOP_A ? [[14, 88, 150, 250], [220, 88, 150, 250]] : [[18, 110, 96, 150], [270, 110, 96, 150]];
      const door = k === SHOP_A ? [168, 96, 48, 288] : [140, 92, 104, 292];
      for (const [px, py, pw, ph] of panes) {
        g.fillStyle = '#111317'; g.fillRect(px, py, pw, ph);
        const grd = ge.createLinearGradient(0, py, 0, py + ph);
        const cols = warm ? ['#ffd49a', '#b85a20'] : [['#d0f0ff', '#3a6cb8'], ['#ffe0f4', '#a03c84'], ['#f0ffe0', '#4a8a3c']][Math.floor(rng() * 3)];
        grd.addColorStop(0, cols[0]); grd.addColorStop(1, cols[1]);
        ge.fillStyle = grd; ge.fillRect(px, py, pw, ph);
        // Spotlights along the top, goods or diners inside.
        for (let q = px + 12; q < px + pw - 6; q += 26) { ge.fillStyle = '#ffffff'; ge.beginPath(); ge.arc(q, py + 6, 3, 0, 7); ge.fill(); }
        if (warm) {
          ge.fillStyle = 'rgba(60,20,4,0.55)'; ge.fillRect(px, py + ph * 0.62, pw, ph * 0.38); // tables
          for (let q = 0; q < 2; q++) person(px + 20 + rng() * (pw - 40), py + ph * 0.38, 1.1);
        } else {
          goods(px + 4, py + 30, pw - 8, ph - 40, true);
          if (rng() < 0.7) { // mannequin
            ge.fillStyle = 'rgba(20,16,20,0.65)'; const mx = px + pw * (0.3 + rng() * 0.4);
            ge.beginPath(); ge.arc(mx, py + 60, 8, 0, 7); ge.fill(); ge.fillRect(mx - 12, py + 70, 24, 70);
          }
        }
        // Window lettering and a reflection sheen.
        g.strokeStyle = '#6a6a70'; g.lineWidth = 4; g.strokeRect(px, py, pw, ph);
        ge.fillStyle = 'rgba(255,255,255,0.12)';
        ge.beginPath(); ge.moveTo(px + pw * 0.1, py + ph); ge.lineTo(px + pw * 0.45, py); ge.lineTo(px + pw * 0.6, py); ge.lineTo(px + pw * 0.25, py + ph); ge.fill();
      }
      g.fillStyle = '#0c0c0e'; g.fillRect(door[0], door[1], door[2], door[3]);
      const dg = ge.createLinearGradient(0, door[1], 0, door[1] + door[3]);
      dg.addColorStop(0, warm ? '#c07030' : '#4a78a0'); dg.addColorStop(1, warm ? '#502008' : '#182838');
      ge.fillStyle = dg; ge.fillRect(door[0] + 6, door[1] + 6, door[2] - 12, door[3] - 6);
      if (warm) person(door[0] + door[2] / 2, door[1] + 90, 1.6);
      ge.fillStyle = '#000'; ge.fillRect(door[0] + door[2] / 2 - 2, door[1], 4, door[3]);
      g.fillStyle = '#c8c0a8'; g.fillRect(door[0] + door[2] / 2 + 6, door[1] + 150, 4, 30); // handle
      // Stall risers.
      g.fillStyle = '#3a3634'; g.fillRect(0, S - 30, S, 30);
      g.fillStyle = 'rgba(255,255,255,0.08)'; g.fillRect(0, S - 30, S, 2);
    });
  }

  // Rowhouse floor: light base so vertex colours tint it; two tall sash
  // windows (at a quarter and three quarters across) with white casings.
  cell(ROWHOUSE, () => {
    g.fillStyle = '#e8e4dc'; g.fillRect(0, 0, S, S);
    g.fillStyle = '#cfc9bf';
    for (let y = 0; y < S; y += 16) g.fillRect(0, y, S, 3); // shiplap siding
    g.fillStyle = 'rgba(0,0,0,0.05)';
    for (let y = 3; y < S; y += 16) g.fillRect(0, y, S, 2);
    g.fillStyle = '#fbfaf6'; g.fillRect(0, 0, S, 20); // floor band
    for (const cxw of [S * 0.25, S * 0.75]) {
      const ww = 96, wx = cxw - ww / 2, wy = 80, wh = 236;
      g.fillStyle = '#fbfaf6'; g.fillRect(wx - 14, wy - 12, ww + 28, wh + 24);  // casing
      g.fillStyle = '#fbfaf6'; g.fillRect(wx - 24, wy - 34, ww + 48, 20);        // hood
      g.fillStyle = '#e0dcd2';
      g.beginPath(); g.moveTo(wx - 24, wy - 34); g.lineTo(cxw, wy - 64); g.lineTo(wx + ww + 24, wy - 34); g.fill(); // pediment
      g.fillStyle = '#fbfaf6'; g.fillRect(wx - 20, wy + wh + 10, ww + 40, 12);    // sill
      g.fillStyle = '#1b2230'; g.fillRect(wx, wy, ww, wh);
      const grd = ge.createLinearGradient(0, wy, 0, wy + wh);
      grd.addColorStop(0, '#ffe0a8'); grd.addColorStop(1, '#c07838');
      ge.fillStyle = grd; ge.fillRect(wx, wy, ww, wh);
      // Lace curtains and a lamp.
      ge.fillStyle = 'rgba(255,250,240,0.35)';
      ge.fillRect(wx, wy, ww * 0.3, wh); ge.fillRect(wx + ww * 0.7, wy, ww * 0.3, wh);
      ge.fillStyle = 'rgba(0,0,0,0.5)'; ge.fillRect(wx, wy, ww, 30 + rng() * 50); // blind
      g.fillStyle = '#fbfaf6'; ge.fillStyle = '#000';
      for (const [rx, ry, rw, rh] of [[wx, wy + wh / 2 - 4, ww, 8], [wx + ww / 2 - 3, wy, 6, wh]]) { g.fillRect(rx, ry, rw, rh); ge.fillRect(rx, ry, rw, rh); }
    }
  });

  // Stucco wall with small windows.
  cell(STUCCO, () => {
    g.fillStyle = '#dcd6cc'; g.fillRect(0, 0, S, S);
    speck(900, 0.06);
    for (const cxw of [S * 0.25, S * 0.75]) {
      const wx = cxw - 32, wy = 120, ww = 64, wh = 140;
      g.fillStyle = '#20242c'; g.fillRect(wx, wy, ww, wh);
      g.fillStyle = '#f4f2ec'; g.fillRect(wx - 6, wy + wh, ww + 12, 10);
      ge.fillStyle = '#ffb860'; ge.fillRect(wx, wy, ww, wh);
      ge.fillStyle = 'rgba(0,0,0,0.5)'; ge.fillRect(wx, wy, ww, 20 + rng() * 60);
    }
  });

  // Closed shop: roller shutter with graffiti and a lit sign box.
  cell(SHUTTER, () => {
    g.fillStyle = '#26262a'; g.fillRect(0, 0, S, S);
    g.fillStyle = '#17161a'; g.fillRect(0, 0, S, 64);
    g.fillStyle = '#7d8088'; g.fillRect(20, 80, S - 40, 290);
    for (let y = 84; y < 370; y += 12) { g.fillStyle = '#5c5f66'; g.fillRect(20, y, S - 40, 4); g.fillStyle = 'rgba(255,255,255,0.12)'; g.fillRect(20, y + 5, S - 40, 1); }
    const tag = ['#ff3c8a', '#37d0ff', '#ffd23c', '#8aff4a'];
    g.lineWidth = 9; g.lineCap = 'round';
    for (let q = 0; q < 4; q++) {
      g.strokeStyle = tag[Math.floor(rng() * tag.length)];
      g.beginPath();
      let x = 60 + rng() * 220, y = 180 + rng() * 140;
      g.moveTo(x, y);
      for (let n = 0; n < 5; n++) { x += (rng() - 0.3) * 50; y += (rng() - 0.5) * 50; g.lineTo(x, y); }
      g.stroke();
    }
    ge.fillStyle = '#301008'; ge.fillRect(12, 14, S - 24, 36);
    ge.fillStyle = '#ff7040'; ge.font = '700 24px "Arial Narrow", Arial, sans-serif'; ge.textAlign = 'center'; ge.textBaseline = 'middle';
    ge.fillText('PAWN · GOLD · CASH', S / 2, 32);
  });

  // Lobby: double-height glass with mullions; inside, a downlit ceiling, a
  // warm stone back wall with the lifts, a reception desk and a polished
  // floor that reflects the lights.
  cell(LOBBY, () => {
    g.fillStyle = '#15181d'; g.fillRect(0, 0, S, S);
    g.fillStyle = '#1c2530'; g.fillRect(8, 30, S - 16, S - 38);
    // Ceiling.
    ge.fillStyle = '#1a140e'; ge.fillRect(8, 30, S - 16, 50);
    // Back wall: stone panels.
    const wg = ge.createLinearGradient(0, 80, 0, 260);
    wg.addColorStop(0, '#6a5234'); wg.addColorStop(1, '#8a6c46');
    ge.fillStyle = wg; ge.fillRect(8, 80, S - 16, 180);
    ge.fillStyle = 'rgba(0,0,0,0.3)';
    for (let x = 8; x < S; x += 46) ge.fillRect(x, 80, 2, 180);
    ge.fillRect(8, 170, S - 16, 2);
    // Lifts: a lit recess with steel doors.
    ge.fillStyle = '#e8d4ae'; ge.fillRect(210, 150, 130, 110);
    ge.fillStyle = '#3a342c'; for (const x of [222, 262, 302]) ge.fillRect(x, 162, 30, 98);
    // Floor: polished, dark with streaks of reflected light.
    const fg = ge.createLinearGradient(0, 260, 0, S);
    fg.addColorStop(0, '#3a2e22'); fg.addColorStop(1, '#6a5640');
    ge.fillStyle = fg; ge.fillRect(8, 260, S - 16, S - 268);
    for (let x = 40; x < S; x += 76) {
      ge.fillStyle = '#fff6e0'; ge.beginPath(); ge.ellipse(x, 52, 14, 4, 0, 0, 7); ge.fill();
      const rg = ge.createLinearGradient(0, 262, 0, S);
      rg.addColorStop(0, 'rgba(255,230,190,0.35)'); rg.addColorStop(1, 'rgba(255,230,190,0)');
      ge.fillStyle = rg; ge.fillRect(x - 6, 262, 12, S - 262);
    }
    // Reception desk with a lit front edge, a guard behind it.
    person(110, 250, 1.2);
    ge.fillStyle = '#1e1810'; ge.fillRect(50, 280, 150, 44);
    ge.fillStyle = '#ffd890'; ge.fillRect(50, 280, 150, 3); ge.fillRect(50, 321, 150, 3);
    // A lit logo on the back wall.
    ge.fillStyle = '#ffe8c0'; ge.fillRect(60, 110, 110, 6); ge.fillRect(60, 124, 70, 4);
    // Mullions and a transom.
    ge.fillStyle = '#000'; g.fillStyle = '#343c48';
    for (let x = 8; x < S; x += 75) { ge.fillRect(x, 30, 7, S - 30); g.fillRect(x, 30, 7, S - 30); }
    ge.fillRect(0, 176, S, 7); g.fillRect(0, 176, S, 7);
    g.fillStyle = '#2a2e36'; g.fillRect(0, 0, S, 30);
  });

  // Convenience store: blue-white fluorescent, full glass, colourful shelves.
  cell(KONBINI, () => {
    g.fillStyle = '#e8eaec'; g.fillRect(0, 0, S, S);
    g.fillStyle = '#1a8a4a'; g.fillRect(0, 0, S, 22); g.fillStyle = '#ff7a1a'; g.fillRect(0, 22, S, 14); g.fillStyle = '#d02a2a'; g.fillRect(0, 36, S, 10);
    ge.fillStyle = '#304030'; ge.fillRect(0, 0, S, 46);
    const grd = ge.createLinearGradient(0, 60, 0, S);
    grd.addColorStop(0, '#c8d4e0'); grd.addColorStop(1, '#6a8098');
    ge.fillStyle = grd; ge.fillRect(6, 60, S - 12, S - 80);
    goods(10, 110, S - 20, 220, true);
    for (let x = 20; x < S; x += 60) { ge.fillStyle = '#ffffff'; ge.fillRect(x, 64, 40, 4); }
    person(90, 190, 1.3); person(290, 200, 1.2);
    ge.fillStyle = '#000'; g.fillStyle = '#9aa0a8';
    for (const x of [6, 128, 256, S - 10]) { ge.fillRect(x, 60, 5, S - 80); g.fillRect(x, 60, 5, S - 80); }
    g.fillStyle = '#6a6e74'; g.fillRect(0, S - 20, S, 20);
  });

  // Rowhouse street storey: a garage door on one side, the recessed entry
  // up a few steps on the other. Tinted by vertex colour like the floors.
  cell(ROW_GROUND, () => {
    g.fillStyle = '#e4e0d8'; g.fillRect(0, 0, S, S);
    g.fillStyle = '#d0cbc2';
    for (let y = 0; y < S; y += 16) g.fillRect(0, y, S, 3);
    g.fillStyle = '#fbfaf6'; g.fillRect(0, 0, S, 18);
    // Garage door (panelled, white-ish).
    const gx = 20, gw = 200, gy = 150;
    g.fillStyle = '#fbfaf6'; g.fillRect(gx - 10, gy - 14, gw + 20, S - gy + 14);
    g.fillStyle = '#d8d4cc'; g.fillRect(gx, gy, gw, S - gy);
    g.fillStyle = 'rgba(0,0,0,0.12)';
    for (let y = gy + 10; y < S; y += 38) for (let x = gx + 8; x < gx + gw - 8; x += 48) g.fillRect(x, y, 40, 28);
    ge.fillStyle = '#fff0c8'; ge.fillRect(gx + gw / 2 - 8, gy - 30, 16, 10); // lamp over garage
    // Entry: panelled door with a lit fan transom, raised.
    const dx = 262, dw = 76, dy = 70, dh = 230;
    g.fillStyle = '#fbfaf6'; g.fillRect(dx - 16, dy - 40, dw + 32, dh + 40);
    g.fillStyle = '#3a2418'; g.fillRect(dx, dy, dw, dh);
    g.fillStyle = 'rgba(0,0,0,0.25)'; g.fillRect(dx + 10, dy + 20, dw - 20, 80); g.fillRect(dx + 10, dy + 120, dw - 20, 90);
    ge.fillStyle = '#ffc070'; ge.beginPath(); ge.ellipse(dx + dw / 2, dy - 4, dw / 2, 28, 0, Math.PI, 0); ge.fill();
    ge.fillStyle = '#ffd890'; ge.fillRect(dx + 14, dy + 24, dw - 28, 70);
    ge.fillStyle = 'rgba(0,0,0,0.5)'; ge.fillRect(dx + dw / 2 - 2, dy + 24, 4, 70);
  });

  // Awning fabric: two-tone stripes (tinted); lit faintly through.
  cell(AWNING_C, () => {
    for (let x = 0; x < S; x += 64) {
      g.fillStyle = '#f4f0e8'; g.fillRect(x, 0, 32, S);
      g.fillStyle = '#b8b0a8'; g.fillRect(x + 32, 0, 32, S);
    }
    g.fillStyle = 'rgba(0,0,0,0.25)'; g.fillRect(0, S - 30, S, 30);
    for (let x = 0; x < S; x += 32) { g.fillStyle = 'rgba(0,0,0,0.3)'; g.beginPath(); g.arc(x + 16, S - 30, 16, 0, Math.PI); g.fill(); }
    ge.fillStyle = '#2a1a10'; ge.fillRect(0, 0, S, S);
  });

  // Vending machine: lit display of cans, buttons, pickup slot.
  cell(VENDING, () => {
    g.fillStyle = '#d8dce0'; g.fillRect(0, 0, S, S);
    const col = ['#d8202a', '#2050c0', '#f0f0f0'][Math.floor(rng() * 3)];
    g.fillStyle = col; g.fillRect(0, 0, S, 50); g.fillRect(0, S - 40, S, 40);
    ge.fillStyle = '#e8f4ff'; ge.fillRect(20, 60, S - 40, 200);
    for (let r = 0; r < 4; r++) for (let q = 0; q < 8; q++) {
      const cx = 36 + q * 40, cy = 80 + r * 48;
      ge.fillStyle = `hsl(${Math.floor(rng() * 360)},70%,${45 + rng() * 20}%)`; ge.fillRect(cx - 9, cy, 18, 30);
      ge.fillStyle = 'rgba(0,0,0,0.3)'; ge.fillRect(cx - 9, cy + 26, 18, 4);
      ge.fillStyle = '#ff4040'; ge.fillRect(cx - 5, cy + 34, 10, 4);
    }
    g.fillStyle = '#1a1c20'; g.fillRect(60, 290, S - 120, 36);
    g.fillStyle = '#404448'; g.fillRect(S - 70, 270, 40, 14);
    ge.fillStyle = '#80ff80'; ge.fillRect(S - 66, 272, 16, 6);
  });

  // Food stall: noren curtain, steaming counter, lit menu boards.
  cell(STALL, () => {
    g.fillStyle = '#3a2418'; g.fillRect(0, 0, S, S);
    const grd = ge.createLinearGradient(0, 0, 0, S);
    grd.addColorStop(0, '#ffc880'); grd.addColorStop(1, '#a04a10');
    ge.fillStyle = grd; ge.fillRect(0, 60, S, 190);
    // Noren: split cloth panels hanging from the top.
    for (let x = 0; x < S; x += 48) {
      g.fillStyle = rng() < 0.5 ? '#1c2a50' : '#6a1414'; g.fillRect(x + 2, 0, 44, 120);
      ge.fillStyle = '#000'; ge.fillRect(x + 2, 0, 44, 120);
      g.fillStyle = '#f0e8d8'; g.font = '700 30px sans-serif'; g.textAlign = 'center'; g.fillText('●', x + 24, 70);
    }
    person(140, 150, 1.6); person(270, 160, 1.5);
    // Counter with pots.
    g.fillStyle = '#6a4a30'; g.fillRect(0, 250, S, S - 250);
    g.fillStyle = 'rgba(0,0,0,0.3)'; for (let x = 0; x < S; x += 24) g.fillRect(x, 250, 2, S - 250);
    ge.fillStyle = 'rgba(0,0,0,0.85)'; ge.fillRect(0, 250, S, S - 250);
    ge.fillStyle = '#ffe0b0'; ge.fillRect(0, 250, S, 6);
    for (const x of [70, 190, 310]) { ge.fillStyle = '#c8c8c8'; ge.fillRect(x - 30, 214, 60, 36); }
  });

  // Poster panel: backlit ad (invented brands).
  cell(POSTER, () => {
    g.fillStyle = '#202224'; g.fillRect(0, 0, S, S);
    const grd = ge.createLinearGradient(0, 0, 0, S);
    grd.addColorStop(0, '#2a0a50'); grd.addColorStop(1, '#ff3ca8');
    ge.fillStyle = grd; ge.fillRect(16, 16, S - 32, S - 32);
    ge.fillStyle = '#ffffff'; ge.font = '900 italic 64px "Arial Narrow", Arial, sans-serif'; ge.textAlign = 'center';
    ge.fillText('NIGHT', S / 2, 130); ge.fillText('SHIFT', S / 2, 196);
    ge.font = '700 26px Arial'; ge.fillStyle = '#7cf6ff'; ge.fillText('ENERGY · ALL NIGHT', S / 2, 300);
    ge.fillStyle = 'rgba(255,255,255,0.9)'; ge.beginPath(); ge.arc(S / 2, 240, 22, 0, 7); ge.fill();
    g.fillStyle = '#505458'; g.fillRect(0, 0, S, 16); g.fillRect(0, S - 16, S, 16); g.fillRect(0, 0, 16, S); g.fillRect(S - 16, 0, 16, S);
  });

  sAtlas = { map: tex(c, { repeat: false }), emissive: tex(ce, { repeat: false }) };
  return sAtlas;
}

// Same trick as the city atlas: `uv` in tile units, `cell` picks the cell
// (plus 16 × a per-building seed that varies the lights). Street level also
// gets a little bounce light from the lamps and shop windows, so kerb-side
// walls read in the dark.
export function patchStreetAtlas(material) {
  const spec = S_GRID.map(([a, b, k]) => `vec3(${a}.0, ${b}.0, ${k}.0)`).join(', ');
  material.onBeforeCompile = (shader) => {
    shader.vertexShader = shader.vertexShader
      .replace('#include <common>', '#include <common>\nattribute float cell;\nvarying float vCell;\nvarying vec2 vAUv;')
      .replace('#include <uv_vertex>', '#include <uv_vertex>\nvCell = cell;\nvAUv = uv;');
    shader.fragmentShader = shader.fragmentShader
      .replace('#include <common>', `#include <common>
varying float vCell;
varying vec2 vAUv;
vec2 sUv; vec2 sDx; vec2 sDy; float sIdx; float sSeed;
const vec3 SSPEC[12] = vec3[12](${spec});
${FACADE_HASH}
${FACADE_WINDOW_LIGHT}`)
      .replace('#include <map_fragment>', `
sIdx = mod(floor(vCell + 0.5), 16.0);
sSeed = floor((vCell + 0.5) / 16.0);
{
  vec2 cxy = vec2(mod(sIdx, 4.0), floor(sIdx / 4.0));
  sUv = vec2((cxy.x + clamp(fract(vAUv.x), 0.004, 0.996)) / 4.0, (2.0 - cxy.y + clamp(fract(vAUv.y), 0.004, 0.996)) / 3.0);
  sDx = dFdx(vAUv) * vec2(0.25, 1.0 / 3.0);
  sDy = dFdy(vAUv) * vec2(0.25, 1.0 / 3.0);
}
#ifdef USE_MAP
  diffuseColor *= textureGrad(map, sUv, sDx, sDy);
#endif
`)
      .replace('#include <emissivemap_fragment>', `
#ifdef USE_EMISSIVEMAP
{
  vec3 sp = SSPEC[int(sIdx)];
  vec3 wl = sp.z < 0.5 ? vec3(0.65 + 0.6 * fHash(vec2(sSeed, 5.0))) : windowLight(floor(vAUv * sp.xy), sSeed, sp.z) * 1.4;
  totalEmissiveRadiance *= textureGrad(emissiveMap, sUv, sDx, sDy).rgb * wl;
}
#endif
totalEmissiveRadiance += diffuseColor.rgb * vec3(0.075, 0.06, 0.05);
`);
  };
  material.customProgramCacheKey = () => 'street-atlas-2';
  return material;
}

// ── Neon signs ───────────────────────────────────────────────────
// A 4×4 atlas of horizontal signs (2:1) and a strip of 8 vertical blade
// signs (1:4). Drawn as glowing tubes on black for additive blending.
const WORDS = ['RAMEN', 'KARAOKE', 'HOTEL', 'LIVE JAZZ', 'NOODLES', 'ARCADE', 'OPEN 24H', 'SUSHI', 'DINER', 'CLUB 88', 'TATTOO', 'DUMPLINGS', 'LIQUOR', 'CAFE', 'PAWN', 'BILLIARDS'];
const BLADES = ['HOTEL', 'BAR', 'RAMEN', 'CLUB', 'EAT', 'DANCE', 'SAKE', 'JAZZ'];
export const NEON_COLORS = ['#ff3ca8', '#3ce8ff', '#ff5a3c', '#ffd23c', '#7cff5a', '#b45aff', '#ff8a2a', '#5a8cff'];
export const H_SIGNS = WORDS.length, V_SIGNS = BLADES.length;

function neonText(g, text, x, y, size, col, maxW) {
  g.font = `700 ${size}px "Arial Narrow", Arial, sans-serif`;
  g.textAlign = 'center'; g.textBaseline = 'middle';
  g.lineJoin = 'round';
  for (const [blur, lw, c] of [[size * 0.5, size * 0.16, col], [size * 0.2, size * 0.1, col], [0, size * 0.045, '#ffffff']]) {
    g.shadowColor = col; g.shadowBlur = blur;
    g.strokeStyle = c; g.lineWidth = lw;
    g.strokeText(text, x, y, maxW);
  }
  g.shadowBlur = 0;
}

let neon = null;
export function neonAtlas() {
  if (neon) return neon;
  // Horizontal: 4×4 cells of 256×128.
  const [c, g] = canvas(1024, 512);
  g.fillStyle = '#000'; g.fillRect(0, 0, 1024, 512);
  WORDS.forEach((w, i) => {
    const cx = (i % 4) * 256 + 128, cy = Math.floor(i / 4) * 128 + 64;
    const col = NEON_COLORS[i % NEON_COLORS.length];
    g.save();
    g.beginPath(); g.rect(cx - 124, cy - 60, 248, 120); g.clip();
    // A tube border on some.
    if (i % 3 === 0) {
      g.shadowColor = col; g.shadowBlur = 14; g.strokeStyle = col; g.lineWidth = 5;
      g.strokeRect(cx - 112, cy - 48, 224, 96);
      g.shadowBlur = 0; g.strokeStyle = '#fff'; g.lineWidth = 1.5; g.strokeRect(cx - 112, cy - 48, 224, 96);
    }
    neonText(g, w, cx, cy + 2, w.length > 7 ? 46 : 62, col, 200);
    g.restore();
  });
  // Vertical: 8 cells of 128×512, letters stacked.
  const [cv, gv] = canvas(1024, 512);
  gv.fillStyle = '#000'; gv.fillRect(0, 0, 1024, 512);
  BLADES.forEach((w, i) => {
    const cx = i * 128 + 64;
    const col = NEON_COLORS[(i * 3 + 1) % NEON_COLORS.length];
    gv.save();
    gv.beginPath(); gv.rect(cx - 62, 2, 124, 508); gv.clip();
    gv.shadowColor = col; gv.shadowBlur = 12; gv.strokeStyle = col; gv.lineWidth = 5;
    gv.strokeRect(cx - 50, 14, 100, 484);
    gv.shadowBlur = 0;
    const n = w.length, step = Math.min(96, 460 / n);
    for (let k = 0; k < n; k++) neonText(gv, w[k], cx, 256 - ((n - 1) / 2 - k) * step, Math.min(84, step * 0.95), col, 90);
    gv.restore();
  });
  neon = { h: tex(c, { repeat: false }), v: tex(cv, { repeat: false }) };
  return neon;
}

// Pavement: concrete slabs with joints (world-mapped, 1 repeat = 3 m).
let pave = null;
export function pavementTexture() {
  if (pave) return pave;
  const S = 256;
  const [c, g] = canvas(S, S);
  g.fillStyle = '#9d9891'; g.fillRect(0, 0, S, S);
  const rng = mulberry32(5150);
  const img = g.getImageData(0, 0, S, S);
  for (let i = 0; i < S * S; i++) {
    const n = (rng() - 0.5) * 18;
    img.data[i * 4] += n; img.data[i * 4 + 1] += n; img.data[i * 4 + 2] += n;
  }
  g.putImageData(img, 0, 0);
  for (let k = 0; k < 14; k++) {
    g.fillStyle = `rgba(40,36,30,${0.05 + rng() * 0.08})`;
    g.beginPath(); g.ellipse(rng() * S, rng() * S, 6 + rng() * 26, 4 + rng() * 14, rng() * 3, 0, 7); g.fill();
  }
  g.fillStyle = 'rgba(30,28,26,0.55)';
  g.fillRect(0, 0, S, 3); g.fillRect(0, 0, 3, S); g.fillRect(0, S / 2, S, 2); g.fillRect(S / 2, 0, 2, S);
  pave = tex(c);
  return pave;
}

// Water-filled barrier: white body with a red/white chevron band.
let barrier = null;
export function barrierTexture() {
  if (barrier) return barrier;
  const [c, g] = canvas(256, 128);
  g.fillStyle = '#e8e6e0'; g.fillRect(0, 0, 256, 128);
  g.fillStyle = '#d0201c';
  for (let x = -128; x < 256; x += 64) {
    g.beginPath(); g.moveTo(x, 110); g.lineTo(x + 32, 30); g.lineTo(x + 64, 30); g.lineTo(x + 32, 110); g.closePath(); g.fill();
  }
  g.fillStyle = '#1a1a1a'; g.fillRect(0, 118, 256, 10);
  g.fillStyle = 'rgba(0,0,0,0.25)'; g.fillRect(0, 0, 256, 6);
  barrier = tex(c);
  return barrier;
}

// Puddle: an irregular soft blob (alpha in all channels) for additive
// reflections of the lights above it on the wet road.
let puddle = null;
export function puddleTexture() {
  if (puddle) return puddle;
  const S = 128;
  const [c, g] = canvas(S, S);
  g.fillStyle = '#000'; g.fillRect(0, 0, S, S);
  const rng = mulberry32(808);
  g.filter = 'blur(6px)';
  for (let q = 0; q < 9; q++) {
    const a = rng() * 6.28, r = 12 + rng() * 22;
    g.fillStyle = `rgba(255,255,255,${0.5 + rng() * 0.5})`;
    g.beginPath(); g.ellipse(S / 2 + Math.cos(a) * r * 0.8, S / 2 + Math.sin(a) * r * 0.5, 14 + rng() * 22, 8 + rng() * 14, rng() * 3, 0, 7); g.fill();
  }
  g.filter = 'none';
  puddle = tex(c, { repeat: false });
  return puddle;
}

// Elevated train car: one texture for the whole box. Left half: the side
// (silver, a band of lit windows with passengers, doors, a stripe); right
// quarter: the cab end; last quarter: the roof. Emissive in `e`.
let trainTex = null;
export function trainTexture() {
  if (trainTex) return trainTex;
  const W = 1024, H = 128;
  const [c, g] = canvas(W, H);
  const [ce, ge] = canvas(W, H);
  const rng = mulberry32(99);
  ge.fillStyle = '#000'; ge.fillRect(0, 0, W, H);
  // Side (0..640).
  const sg = g.createLinearGradient(0, 0, 0, H);
  sg.addColorStop(0, '#c8ccd0'); sg.addColorStop(0.5, '#9aa0a6'); sg.addColorStop(1, '#6a7076');
  g.fillStyle = sg; g.fillRect(0, 0, 640, H);
  for (let x = 0; x < 640; x += 6) { g.fillStyle = 'rgba(0,0,0,0.06)'; g.fillRect(x, 70, 2, 58); }
  g.fillStyle = '#1a8a5a'; g.fillRect(0, 92, 640, 10);
  ge.fillStyle = '#0a3020'; ge.fillRect(0, 92, 640, 10);
  for (let x = 20; x < 620; x += 60) {
    const door = (x - 20) % 180 === 120;
    g.fillStyle = '#20262c';
    if (door) { g.fillRect(x, 26, 44, 94); ge.fillStyle = '#e8e0c8'; ge.fillRect(x + 4, 30, 16, 46); ge.fillRect(x + 24, 30, 16, 46); continue; }
    g.fillRect(x, 28, 48, 44);
    const grd = ge.createLinearGradient(0, 28, 0, 72);
    grd.addColorStop(0, '#fff6e0'); grd.addColorStop(1, '#c8b890');
    ge.fillStyle = grd; ge.fillRect(x + 2, 30, 44, 40);
    ge.fillStyle = 'rgba(10,10,14,0.85)';
    for (let q = 0; q < 2; q++) if (rng() < 0.7) { const px = x + 8 + rng() * 30; ge.beginPath(); ge.arc(px, 46, 4, 0, 7); ge.fill(); ge.fillRect(px - 6, 51, 12, 20); }
  }
  // Cab end (640..896): windscreen, headlights, destination sign.
  g.fillStyle = '#b0b6bc'; g.fillRect(640, 0, 256, H);
  g.fillStyle = '#1a2028'; g.fillRect(680, 22, 176, 50);
  ge.fillStyle = '#403828'; ge.fillRect(684, 26, 168, 42);
  ge.fillStyle = '#ffb030'; ge.font = '700 18px Arial'; ge.textAlign = 'center'; ge.fillText('LOOP · DOWNTOWN', 768, 16);
  for (const x of [676, 860]) { ge.fillStyle = '#ffffff'; ge.beginPath(); ge.arc(x, 96, 9, 0, 7); ge.fill(); g.fillStyle = '#eee'; g.beginPath(); g.arc(x, 96, 9, 0, 7); g.fill(); }
  g.fillStyle = '#1a8a5a'; g.fillRect(640, 108, 256, 8);
  // Roof (896..1024).
  g.fillStyle = '#7a8086'; g.fillRect(896, 0, 128, H);
  for (let y = 0; y < H; y += 16) { g.fillStyle = 'rgba(0,0,0,0.2)'; g.fillRect(896, y, 128, 3); }
  trainTex = { map: tex(c, { repeat: false }), e: tex(ce, { repeat: false }) };
  return trainTex;
}
