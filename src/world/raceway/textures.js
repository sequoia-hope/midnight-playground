import * as THREE from 'three';
import { mulberry32 } from '../../util/math.js';

// Canvas textures for Seaside Raceway: kerbs, tyre walls, catch fencing,
// the crowd in the grandstands, and the banners and boards round the lap.

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

function toTexture(c, { repeat = true, aniso = 8 } = {}) {
  const t = new THREE.CanvasTexture(c);
  if (repeat) t.wrapS = t.wrapT = THREE.RepeatWrapping;
  t.colorSpace = THREE.SRGBColorSpace;
  t.anisotropy = aniso;
  return t;
}

// Kerb: one red and one white block along v, with a painted edge and a
// little wear. u runs across the kerb.
export function kerbTexture() {
  return cached('kerb', () => {
    const [c, g] = canvas(64, 128);
    g.fillStyle = '#c8281e'; g.fillRect(0, 0, 64, 64);
    g.fillStyle = '#f1efe8'; g.fillRect(0, 64, 64, 64);
    const rng = mulberry32(5);
    for (let k = 0; k < 260; k++) {
      g.fillStyle = `rgba(40,36,32,${0.05 + rng() * 0.12})`;
      g.fillRect(rng() * 64, rng() * 128, 1 + rng() * 3, 1 + rng() * 2);
    }
    // Rubber on the inner edge.
    const grd = g.createLinearGradient(0, 0, 20, 0);
    grd.addColorStop(0, 'rgba(20,20,20,0.45)'); grd.addColorStop(1, 'rgba(20,20,20,0)');
    g.fillStyle = grd; g.fillRect(0, 0, 20, 128);
    return toTexture(c);
  });
}

// Tyre wall: stacked tyres behind a belt of conveyor rubber, the belt in
// bands of colour. u across the face (height), v along the wall.
export function tyreTexture() {
  return cached('tyre', () => {
    const [c, g] = canvas(128, 256);
    g.fillStyle = '#18191b'; g.fillRect(0, 0, 128, 256);
    const bands = ['#d8d6d0', '#c62a22', '#d8d6d0', '#1f4fa0'];
    for (let k = 0; k < 4; k++) {
      g.fillStyle = bands[k];
      g.fillRect(0, k * 64 + 6, 128, 52);
    }
    // Bolt lines and grime.
    const rng = mulberry32(9);
    g.fillStyle = 'rgba(0,0,0,0.35)';
    for (let y = 0; y < 256; y += 16) g.fillRect(0, y, 128, 2);
    for (let k = 0; k < 400; k++) {
      g.fillStyle = `rgba(30,26,22,${0.05 + rng() * 0.15})`;
      g.fillRect(rng() * 128, rng() * 256, 2 + rng() * 6, 1 + rng() * 3);
    }
    const grd = g.createLinearGradient(0, 0, 128, 0);
    grd.addColorStop(0, 'rgba(60,45,30,0.5)'); grd.addColorStop(0.3, 'rgba(60,45,30,0)');
    g.fillStyle = grd; g.fillRect(0, 0, 128, 256);
    return toTexture(c);
  });
}

// Catch fence: diamond wire mesh with a top and bottom cable (alpha).
export function fenceTexture() {
  return cached('fence', () => {
    const [c, g] = canvas(128, 128);
    g.clearRect(0, 0, 128, 128);
    g.strokeStyle = 'rgba(190,196,200,0.95)';
    g.lineWidth = 1.6;
    for (let k = -128; k < 256; k += 16) {
      g.beginPath(); g.moveTo(k, 0); g.lineTo(k + 128, 128); g.stroke();
      g.beginPath(); g.moveTo(k + 128, 0); g.lineTo(k, 128); g.stroke();
    }
    g.fillStyle = 'rgba(150,155,160,1)';
    g.fillRect(0, 0, 128, 3); g.fillRect(0, 125, 128, 3);
    const t = toTexture(c);
    t.generateMipmaps = true;
    return t;
  });
}

// A crowd seen from the track: rows of heads and shirts in every colour,
// with gaps. u along the row, v up the tiers (one tier per 32 px).
export function crowdTexture() {
  return cached('crowd', () => {
    const W = 512, H = 256;
    const [c, g] = canvas(W, H);
    g.fillStyle = '#6c6f73'; g.fillRect(0, 0, W, H);
    const rng = mulberry32(31);
    const shirts = ['#d8423a', '#f2f0ea', '#2f5fb8', '#f4c542', '#2c2c30', '#3fa35a', '#e8742a', '#8a4fc2', '#b8d4ea', '#c8b08a'];
    const skin = ['#f0c8a0', '#d9a57a', '#a86e48', '#6e4630', '#f4d6b8'];
    for (let row = 0; row < H / 32; row++) {
      // Seat back.
      g.fillStyle = '#4f6fa0';
      g.fillRect(0, row * 32 + 26, W, 4);
      for (let x = 2; x < W; x += 9 + rng() * 3) {
        if (rng() < 0.18) continue; // empty seat
        const y = row * 32 + 8 + rng() * 3;
        g.fillStyle = shirts[Math.floor(rng() * shirts.length)];
        g.fillRect(x, y + 7, 7, 12);
        g.fillStyle = rng() < 0.2 ? '#222' : skin[Math.floor(rng() * skin.length)];
        g.beginPath(); g.arc(x + 3.5, y + 4, 3.4, 0, Math.PI * 2); g.fill();
        if (rng() < 0.15) { g.fillStyle = shirts[Math.floor(rng() * shirts.length)]; g.fillRect(x - 1, y - 1, 9, 3); } // cap
      }
    }
    return toTexture(c);
  });
}

// Banners and boards, all in one atlas: returns { texture, rect(name) }
// where rect gives [u0, v0, u1, v1] of each panel.
export const BANNERS = [
  { name: 'seaside', bg: '#10325c', fg: '#f4f1e8', text: 'SEASIDE RACEWAY', sub: 'MONTEREY COUNTY' },
  { name: 'midnight', bg: '#15121c', fg: '#ff5a8a', text: 'MIDNIGHT RACER' },
  { name: 'vento', bg: '#d81e36', fg: '#ffffff', text: 'VENTO GT' },
  { name: 'kestrel', bg: '#ee5a12', fg: '#1a1a1a', text: 'KESTREL RS' },
  { name: 'ion', bg: '#dfe7ee', fg: '#0e6f86', text: 'ION ARC' },
  { name: 'stiletto', bg: '#f2b705', fg: '#141414', text: 'STILETTO' },
  { name: 'brawler', bg: '#1f4fd8', fg: '#ffffff', text: 'BRAWLER 69' },
  { name: 'tyres', bg: '#1b1b1d', fg: '#f2c230', text: 'MERIDIAN TYRES' },
  { name: 'oil', bg: '#0d6b3a', fg: '#f4f1e8', text: 'SEABRIGHT OIL' },
  { name: 'startfinish', bg: '#f4f1e8', fg: '#111111', text: 'START · FINISH', checker: true },
  { name: 'b150', board: '150' }, { name: 'b100', board: '100' }, { name: 'b50', board: '50' },
];

export function bannerAtlas() {
  return cached('banners', () => {
    const PW = 512, PH = 96, cols = 2;
    const rows = Math.ceil(BANNERS.length / cols);
    const [c, g] = canvas(PW * cols, PH * rows);
    const rects = new Map();
    BANNERS.forEach((b, i) => {
      const x = (i % cols) * PW, y = Math.floor(i / cols) * PH;
      g.save();
      g.beginPath(); g.rect(x, y, PW, PH); g.clip();
      if (b.board) {
        // Braking board: black numerals on white with a red frame.
        g.fillStyle = '#f4f2ec'; g.fillRect(x, y, PW, PH);
        g.fillStyle = '#c8281e'; g.fillRect(x, y, PW, 10); g.fillRect(x, y + PH - 10, PW, 10);
        g.fillStyle = '#111';
        g.font = 'bold 76px "Arial Black", Arial, sans-serif';
        g.textAlign = 'center'; g.textBaseline = 'middle';
        g.fillText(b.board, x + PW / 2, y + PH / 2 + 3);
      } else {
        g.fillStyle = b.bg; g.fillRect(x, y, PW, PH);
        if (b.checker) {
          for (let k = 0; k < 8; k++) for (let j = 0; j < 3; j++) {
            g.fillStyle = (k + j) % 2 ? '#111' : '#f4f1e8';
            g.fillRect(x + k * 12, y + 12 + j * 24, 12, 24);
            g.fillRect(x + PW - 96 + k * 12, y + 12 + j * 24, 12, 24);
          }
        }
        g.fillStyle = b.fg;
        g.textAlign = 'center'; g.textBaseline = 'middle';
        g.font = `bold ${b.sub ? 50 : 62}px "Arial Black", Arial, sans-serif`;
        g.fillText(b.text, x + PW / 2, y + (b.sub ? PH * 0.4 : PH / 2 + 3), PW - (b.checker ? 210 : 30));
        if (b.sub) { g.font = 'bold 22px Arial, sans-serif'; g.fillText(b.sub, x + PW / 2, y + PH * 0.8); }
      }
      g.restore();
      rects.set(b.name, [x / c.width, 1 - (y + PH) / c.height, (x + PW) / c.width, 1 - y / c.height]);
    });
    const texture = toTexture(c, { repeat: false });
    return { texture, rect: (name) => rects.get(name) };
  });
}
