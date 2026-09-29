import * as THREE from 'three';
import { rrange, rpick } from '../../util/math.js';
import { signGeometry } from './atlas.js';

// Seabright building kit. Every function draws into a Builder in its current
// frame: local +Z is the street front, X runs along the street, Y is up and
// y = 0 is the ground. Buildings occupy z ∈ [−depth, 0].

const V = (x, y, z) => new THREE.Vector3(x, y, z);
export const WALLS = ['wPink', 'wMint', 'wYellow', 'wBlue', 'wWhite', 'wPeach', 'wLilac', 'wTeal', 'wSand'];
const AWNINGS = ['awnRed', 'awnBlue', 'awnYellow', 'awnGreen'];

// Row of windows on the front face.
// Each pane sits in a trim surround a little proud of the wall; some rows
// get painted shutters.
function windows(B, rng, x0, x1, y, h, z, spacing = 2.4, w = 1.4, litP = 0.35, shutter = null) {
  const n = Math.max(1, Math.floor((x1 - x0) / spacing));
  const step = (x1 - x0) / n;
  for (let i = 0; i < n; i++) {
    const x = x0 + step * (i + 0.5);
    B.cbox('trim', w + 0.26, h + 0.22, 0.08, x, y + h / 2, z);
    B.cbox(rng() < litP ? 'glassLit' : 'glass', w, h, 0.1, x, y + h / 2, z + 0.02);
    B.cbox('trim', w + 0.36, 0.12, 0.24, x, y - 0.06, z + 0.08);
    if (shutter) for (const sx of [-1, 1]) B.cbox(shutter, w * 0.42, h + 0.1, 0.06, x + sx * (w / 2 + w * 0.21 + 0.14), y + h / 2, z + 0.03);
  }
}

// Side windows (on ±X faces).
function sideWindows(B, rng, side, xFace, z0, z1, y, h, spacing = 2.6, litP = 0.3) {
  const n = Math.max(1, Math.floor((z1 - z0) / spacing));
  const step = (z1 - z0) / n;
  for (let i = 0; i < n; i++) {
    const z = z0 + step * (i + 0.5);
    B.cbox(rng() < litP ? 'glassLit' : 'glass', 0.1, h, 1.2, xFace + side * 0.02, y + h / 2, z);
  }
}

function parapet(B, key, w, d, H, t = 0.25, ph = 0.7) {
  B.box(key, w + 0.1, ph, t, 0, H, -t / 2 + 0.05);
  B.box(key, w + 0.1, ph, t, 0, H, -d + t / 2 - 0.05);
  B.box(key, t, ph, d, -w / 2 + t / 2 - 0.05, H, -d / 2);
  B.box(key, t, ph, d, w / 2 - t / 2 + 0.05, H, -d / 2);
}

function roofClutter(B, rng, w, d, H) {
  const n = Math.floor(rrange(rng, 1, 4));
  for (let i = 0; i < n; i++) {
    B.box('metal', rrange(rng, 1, 2.2), rrange(rng, 0.8, 1.4), rrange(rng, 1, 2.2), rrange(rng, -w / 3, w / 3), H, -rrange(rng, d * 0.3, d * 0.7));
  }
}

// Striped canvas awning sloping out over the sidewalk.
function awning(B, key, w, y, depth = 2.2, drop = 0.7) {
  const a = Math.atan2(drop, depth);
  const L = Math.hypot(depth, drop);
  B.cbox(key, w, 0.08, L, 0, y - drop / 2, depth / 2, a, 0, 0);
  B.cbox(key, w, 0.4, 0.06, 0, y - drop - 0.2, depth);
}

export function sign(B, rect, w, h, x, y, z, ry = 0, key = 'signs') {
  B.put(key, signGeometry(rect, w, h), x, y, z, 0, ry, 0);
}

// ── Storefronts ───────────────────────────────────────────────────
// One or two storey shop with display windows, awning and a sign board.
const TRIMS = ['trim', 'trim', 'wWhite', 'wSand', 'wTeal', 'woodDark'];
const SHUTTERS = ['wTeal', 'wBlue', 'paintRed', 'woodDark', 'wMint'];

export function shop(B, rng, { w = 12, d = 14, rect = null, twoStory = rng() < 0.4, wall = rpick(rng, WALLS), blade = null } = {}) {
  const H = twoStory ? 7 : 4.2;
  const trim = rpick(rng, TRIMS);
  B.box(wall, w, H, d, 0, 0, -d / 2);
  B.box('concrete', w + 0.2, 0.3, d + 0.2, 0, -0.1, -d / 2);
  // Pilasters at the corners, a tiled bulkhead under the display window,
  // the window itself and a glazed door with a transom.
  for (const sx of [-1, 1]) B.box(trim, 0.45, H, 0.18, sx * (w / 2 - 0.22), 0, 0.06);
  B.cbox('glass', w * 0.62, 2.1, 0.1, -w * 0.12, 1.75, 0.03);
  B.cbox(trim, w * 0.62 + 0.3, 0.2, 0.2, -w * 0.12, 2.85, 0.07);
  B.box(rpick(rng, ['woodDark', 'paintRed', 'wTeal', 'black']), w * 0.62 + 0.25, 0.7, 0.16, -w * 0.12, 0, 0.06);
  B.cbox('trim', w * 0.62 + 0.25, 0.1, 0.25, -w * 0.12, 0.72, 0.1);
  B.cbox(trim, 1.5, 2.9, 0.1, w * 0.33, 1.45, 0.02);
  B.cbox('glass', 1.0, 1.4, 0.1, w * 0.33, 1.45, 0.05);
  B.cbox('woodDark', 1.2, 0.7, 0.12, w * 0.33, 0.35, 0.05);
  B.cbox('glass', 1.2, 0.4, 0.1, w * 0.33, 2.62, 0.05);
  awning(B, rpick(rng, AWNINGS), w * 0.95, 3.35);
  if (rect) {
    B.box('trim', w * 0.9, 1.25, 0.2, 0, 3.5, 0.02);
    sign(B, rect, w * 0.84, 1.05, 0, 4.12, 0.14);
  }
  // A projecting blade sign on a bracket by the door.
  if (blade) {
    const bx = w / 2 - 0.5;
    B.cbox('black', 0.08, 0.08, 1.1, bx, 3.55 + (twoStory ? 1.8 : 0.2), 0.55);
    B.cbox('black', 0.9, 1.5, 0.1, bx, 2.75 + (twoStory ? 1.8 : 0.2), 1.0, 0, Math.PI / 2, 0);
    sign(B, blade, 0.8, 1.4, bx + 0.06, 2.75 + (twoStory ? 1.8 : 0.2), 1.0, Math.PI / 2);
    sign(B, blade, 0.8, 1.4, bx - 0.06, 2.75 + (twoStory ? 1.8 : 0.2), 1.0, -Math.PI / 2);
  }
  if (twoStory) windows(B, rng, -w / 2 + 0.8, w / 2 - 0.8, 4.7, 1.5, 0, 2.6, 1.3, 0.4, rng() < 0.4 ? rpick(rng, SHUTTERS) : null);
  // Cornice, and on some a stepped false front above the parapet.
  B.box(trim, w + 0.3, 0.3, 0.35, 0, H - 0.1, 0.05);
  parapet(B, wall, w, d, H);
  if (rng() < 0.45) {
    B.box(wall, w * 0.5, 0.9, 0.25, 0, H + 0.7, -0.08);
    B.box(trim, w * 0.5 + 0.2, 0.18, 0.32, 0, H + 1.6, -0.06);
  }
  B.box('roofTar', w - 0.3, 0.1, d - 0.3, 0, H - 0.05, -d / 2);
  roofClutter(B, rng, w, d, H);
  return { H };
}

// Surf shop: a shop with a giant surfboard on the roof.
export function surfShop(B, rng, rect) {
  const w = 14, d = 13;
  const { H } = shop(B, rng, { w, d, rect, twoStory: false, wall: rpick(rng, ['wTeal', 'wYellow', 'wBlue']) });
  // Board standing on the roof.
  const g = new THREE.CylinderGeometry(1, 1, 1, 18);
  B.put('wTeal', g, 0, H + 3.6, -3, Math.PI / 2, 0, 0, 1.3, 0.18, 3.8);
  B.box('metal', 0.15, 1.2, 0.15, -0.6, H, -3.1); B.box('metal', 0.15, 1.2, 0.15, 0.6, H, -3.1);
  // Boards racked outside.
  for (let i = 0; i < 4; i++) B.put(i % 2 ? 'boardB' : 'wTeal', g, -w / 2 + 1 + i * 0.6, 1.3, 0.8, 0.2, 0, 0.08, 0.28, 2.4, 0.04);
}

// Streamline diner with a rounded end, chrome band and a neon roof sign.
export function diner(B, rng, neonRect) {
  const w = 18, d = 10, H = 4.4;
  B.box('concrete', w + 1, 0.4, d + 1, 0, -0.1, -d / 2);
  B.box('wWhite', w - d / 2, H, d, -d / 4, 0, -d / 2);
  const cyl = new THREE.CylinderGeometry(d / 2, d / 2, H, 20, 1, false, 0, Math.PI);
  B.put('wWhite', cyl, w / 2 - d / 2, H / 2, -d / 2, 0, 0, 0);
  const band = new THREE.CylinderGeometry(d / 2 + 0.06, d / 2 + 0.06, 0.4, 20, 1, true, 0, Math.PI);
  B.put('metal', band, w / 2 - d / 2, 3.2, -d / 2);
  B.cbox('metal', w - d / 2, 0.4, 0.1, -d / 4, 3.2, 0.03);
  B.cbox('paintRed', w - d / 2, 0.5, 0.1, -d / 4, 0.55, 0.03);
  B.put('paintRed', band, w / 2 - d / 2, 0.55, -d / 2, 0, 0, 0, 1, 1.25, 1);
  // Ribbon windows.
  B.cbox('glassLit', w - d / 2 - 1, 1.6, 0.1, -d / 4, 1.9, 0.05);
  const wcyl = new THREE.CylinderGeometry(d / 2 + 0.03, d / 2 + 0.03, 1.6, 20, 1, true, 0, Math.PI);
  B.put('glassLit', wcyl, w / 2 - d / 2, 1.9, -d / 2);
  // Flat roof + neon sign on a frame.
  B.box('roofTar', w - d / 2, 0.2, d, -d / 4, H, -d / 2);
  B.box('metal', 0.15, 1.2, 0.15, -3, H, -1); B.box('metal', 0.15, 1.2, 0.15, 3, H, -1);
  B.box('black', 7.4, 2.4, 0.3, 0, H + 1.1, -1.1);
  sign(B, neonRect, 7, 2.1, 0, H + 2.3, -0.93, 0, 'neon');
}

// Taco stand: a small hut with a counter, awning and picnic tables.
export function tacoStand(B, rng, rect) {
  const w = 7, d = 5, H = 3.2;
  B.box('wYellow', w, H, d, 0, 0, -d / 2 - 2);
  B.cbox('woodDark', w * 0.7, 1.1, 0.2, 0, 1.0, -2 + 0.1);
  B.cbox('glass', w * 0.7, 0.9, 0.08, 0, 1.9, -2 + 0.05);
  awning(B, 'awnGreen', w + 0.6, 3.1, 1.8, 0.5);
  B.box('roofTile', w + 0.6, 0.25, d + 0.6, 0, H, -d / 2 - 2);
  if (rect) sign(B, rect, 4.6, 1.3, 0, H + 1.0, -2.1);
  if (rect) B.box('woodDark', 4.8, 1.5, 0.12, 0, H + 0.25, -2.25);
  for (const [tx, tz] of [[-3.5, 3], [0.5, 3.4], [4, 2.6]]) picnicTable(B, tx, tz);
}

export function picnicTable(B, x, z) {
  B.box('wood', 1.8, 0.08, 0.8, x, 0.75, z);
  B.box('wood', 1.8, 0.06, 0.3, x, 0.45, z - 0.65);
  B.box('wood', 1.8, 0.06, 0.3, x, 0.45, z + 0.65);
  for (const sx of [-0.7, 0.7]) B.box('woodDark', 0.1, 0.75, 1.5, x + sx, 0, z);
}

// Two-storey motel: rooms facing a parking court, exterior walkway, office,
// and a tall VACANCY pole sign near the street.
export function motel(B, rng, { w = 40, signRect, neonRect, wall = rpick(rng, ['wPink', 'wPeach', 'wTeal', 'wMint']) } = {}) {
  const d = 10, H = 6.2, back = -26;
  // Room block along the back of the lot.
  B.box(wall, w, H, d, 0, 0, back + d / 2 - d / 2);
  const zf = back + d / 2;       // front face of the room block
  B.box('concrete', w + 0.4, 0.2, 3, 0, 0, zf + 1.5);
  B.box('concrete', w + 0.4, 0.25, 2.2, 0, 3.0, zf + 1.1);           // walkway slab
  B.box('white', w + 0.4, 0.06, 0.06, 0, 4.05, zf + 2.15);           // railing top
  for (let x = -w / 2; x <= w / 2; x += 2.5) B.box('white', 0.06, 1.0, 0.06, x, 3.05, zf + 2.15);
  for (let x = -w / 2 + 2; x < w / 2; x += 5) B.box('white', 0.2, 3.0, 0.2, x, 0, zf + 2.1);
  const rooms = Math.floor(w / 4);
  for (let i = 0; i < rooms; i++) {
    const x = -w / 2 + 4 * (i + 0.5);
    for (const fy of [0.2, 3.25]) {
      B.cbox(rpick(rng, ['wTeal', 'paintRed', 'wYellow']), 0.95, 2.1, 0.1, x - 0.9, fy + 1.05, zf + 0.04);
      B.cbox(rng() < 0.3 ? 'glassLit' : 'glass', 1.4, 1.0, 0.08, x + 0.7, fy + 1.45, zf + 0.04);
    }
  }
  // Stairs at one end.
  for (let k = 0; k < 10; k++) B.box('concrete', 1.2, 0.3, 0.35, w / 2 + 1, k * 0.3, zf + 2 - k * 0.35);
  B.box('roofTile', w + 1, 0.3, d + 3.5, 0, H, back + d / 2 - d / 2 + 1.2);
  // Office with a sign, at the street end of the lot.
  B.box(wall, 9, 4, 8, -w / 2 + 5, 0, -6);
  B.cbox('glassLit', 6, 2, 0.1, -w / 2 + 5, 1.6, -1.95);
  B.box('roofTile', 10, 0.3, 9, -w / 2 + 5, 4, -6);
  if (signRect) sign(B, signRect, 7.5, 1.6, -w / 2 + 5, 5.3, -1.6);
  if (signRect) B.box('trim', 7.8, 1.9, 0.15, -w / 2 + 5, 4.3, -1.8);
  // Parking stripes + a pool.
  for (let x = -w / 2 + 12; x < w / 2 - 2; x += 3) B.box('paintWhite', 0.12, 0.02, 5, x, 0.05, zf + 6);
  B.box('concrete', 10, 0.18, 6, w / 2 - 7, 0, -6);
  B.box('pool', 8, 0.05, 4, w / 2 - 7, 0.16, -6);
  // Pole sign by the street: MOTEL + neon VACANCY.
  const px = w / 2 - 2, pz = 1.2;
  B.box('metal', 0.35, 9, 0.35, px, 0, pz);
  if (neonRect) {
    B.box('black', 4.6, 1.5, 0.35, px, 5.6, pz);
    sign(B, neonRect, 4.4, 1.3, px, 6.35, pz + 0.19, 0, 'neon');
    sign(B, neonRect, 4.4, 1.3, px, 6.35, pz - 0.19, Math.PI, 'neon');
  }
  return { d: 30 };
}

// Gas station: canopy with a lit underside, pump islands, a kiosk.
export function gasStation(B, rng, { signRect, priceRect } = {}) {
  const cw = 20, cd = 12, ch = 5.4, cz = 1 - cd / 2 - 3;
  B.box('asphaltLot', 34, 0.08, 26, 0, 0, -12);
  for (const [x, z] of [[-cw / 2 + 1.5, cz - cd / 2 + 1.5], [cw / 2 - 1.5, cz - cd / 2 + 1.5], [-cw / 2 + 1.5, cz + cd / 2 - 1.5], [cw / 2 - 1.5, cz + cd / 2 - 1.5]]) {
    B.box('white', 0.5, ch, 0.5, x, 0, z);
  }
  B.box('white', cw, 0.9, cd, 0, ch, cz);
  B.box('paintRed', cw + 0.1, 0.35, cd + 0.1, 0, ch + 0.45, cz);
  B.cbox('canopyLight', cw - 1, 0.05, cd - 1, 0, ch - 0.03, cz);
  for (const px of [-4.5, 4.5]) {
    B.box('concrete', 1.4, 0.2, 5, px, 0, cz);
    for (const pz of [-1.3, 1.3]) {
      B.box('white', 0.7, 1.7, 0.5, px, 0.2, cz + pz);
      B.cbox('paintRed', 0.72, 0.35, 0.52, px, 1.75, cz + pz);
      B.cbox('glassLit', 0.4, 0.3, 0.02, px, 1.25, cz + pz + 0.26);
    }
  }
  // Kiosk.
  B.box('wWhite', 12, 4, 8, 0, 0, -20);
  B.cbox('glassLit', 9, 2.2, 0.1, 0, 1.5, -15.95);
  B.box('paintRed', 12.2, 0.6, 8.2, 0, 3.6, -20);
  // Price sign pole.
  if (priceRect) {
    B.box('metal', 0.4, 7.5, 0.4, -15, 0, 0);
    B.box('white', 3.6, 3.4, 0.4, -15, 7, 0);
    sign(B, priceRect, 3.3, 3.1, -15, 8.7, 0.21);
    sign(B, priceRect, 3.3, 3.1, -15, 8.7, -0.21, Math.PI);
  }
  if (signRect) sign(B, signRect, 6, 1.2, 0, ch + 0.45, cz + cd / 2 + 0.07);
}

// Beach house / small apartment: stucco box(es), balconies with glass rails,
// roof deck, garage.
export function beachHouse(B, rng, { w = rrange(rng, 9, 13), d = rrange(rng, 10, 14), floors = rng() < 0.55 ? 2 : 3, wall = rpick(rng, WALLS), tile = rng() < 0.4, porch = rng() < 0.5 } = {}) {
  const fh = 3.1, H = floors * fh;
  const shutter = rng() < 0.45 ? rpick(rng, SHUTTERS) : null;
  B.box('concrete', w + 0.4, 0.35, d + 0.4, 0, -0.2, -d / 2);
  B.box(wall, w, H, d, 0, 0, -d / 2);
  // Corner boards and a band at each floor line.
  for (const sx of [-1, 1]) B.box('trim', 0.3, H, 0.3, sx * (w / 2 - 0.1), 0, -0.1);
  for (let f = 1; f < floors; f++) B.box('trim', w + 0.12, 0.22, 0.14, 0, f * fh - 0.15, 0.02);
  for (let f = 0; f < floors; f++) {
    const y = f * fh;
    if (f === 0) {
      B.cbox('trim', w * 0.42, 2.3, 0.1, -w * 0.22, 1.15, 0.03);
      B.cbox('woodDark', 1.0, 2.2, 0.1, w * 0.26, 1.1, 0.03);
      windows(B, rng, w * 0.05, w / 2 - 0.6, 0.9, 1.3, 0, 2.2, 1.1, 0.3, shutter);
      if (porch) {
        // Front porch: deck, posts, a rail, steps and a shed roof over it.
        const pw = w * 0.9, pd = 2.4;
        B.box('wood', pw, 0.45, pd, 0, 0, pd / 2 + 0.05);
        for (let k = 0; k <= 3; k++) B.box('trim', 0.18, 2.6, 0.18, -pw / 2 + 0.1 + k * (pw - 0.2) / 3, 0.45, pd - 0.05);
        B.box('trim', pw, 0.08, 0.1, 0, 1.4, pd - 0.05);
        for (let x = -pw / 2 + 0.3; x < pw / 2 - 0.2; x += 0.35) if (Math.abs(x - w * 0.26) > 0.7) B.box('trim', 0.05, 0.9, 0.05, x, 0.5, pd - 0.05);
        for (let k = 0; k < 2; k++) B.box('concrete', 1.4, 0.15 + k * 0.15, 0.35, w * 0.26, 0, pd + 0.35 - k * 0.3);
        B.cbox(tile ? 'roofTile' : 'roofTar', pw + 0.4, 0.12, pd + 0.4, 0, 3.15, pd / 2 + 0.05, 0.12, 0, 0);
      }
    } else {
      windows(B, rng, -w / 2 + 0.6, w / 2 - 0.6, y + 0.6, 1.9, 0, 2.3, 1.6, 0.45, shutter && rng() < 0.6 ? shutter : null);
      if (rng() < 0.75 && !(porch && f === 1)) {
        // Balcony with glass balustrade.
        B.box('concrete', w * 0.8, 0.2, 1.6, 0, y - 0.1, 0.8);
        B.cbox('balGlass', w * 0.8, 1.0, 0.05, 0, y + 0.6, 1.58);
        B.box('white', w * 0.8, 0.06, 0.1, 0, y + 1.1, 1.58);
      }
    }
    sideWindows(B, rng, 1, w / 2, -d + 1, -1, y + 0.8, 1.4);
    sideWindows(B, rng, -1, -w / 2, -d + 1, -1, y + 0.8, 1.4);
  }
  if (tile) {
    // Low hipped tile roof (a squashed pyramid).
    const cone = new THREE.ConeGeometry(Math.SQRT1_2, 1, 4, 1);
    cone.rotateY(Math.PI / 4);
    B.put('roofTile', cone, 0, H + 1.0, -d / 2, 0, 0, 0, w + 0.9, 2.0, d + 0.9);
  } else {
    parapet(B, 'trim', w, d, H, 0.2, 0.9);
    B.box('roofTar', w - 0.3, 0.1, d - 0.3, 0, H - 0.05, -d / 2);
    if (rng() < 0.5) {
      // Roof-deck pergola.
      for (const [sx, sz] of [[-1, -1], [1, -1], [-1, 1], [1, 1]]) B.box('wood', 0.15, 2.4, 0.15, sx * w * 0.25, H, -d / 2 + sz * d * 0.2);
      for (let k = -3; k <= 3; k++) B.box('wood', w * 0.55, 0.12, 0.12, 0, H + 2.4, -d / 2 + k * d * 0.07);
    }
  }
  return { w, d, H };
}

// Pastel hotel block, 5–6 floors, balconies on every floor, a roof sign.
export function hotel(B, rng, { w = 34, d = 16, rect } = {}) {
  const floors = 6, fh = 3.2, H = floors * fh;
  B.box('wPeach', w, H, d, 0, 0, -d / 2);
  B.box('trim', w + 0.3, 0.6, d + 0.3, 0, H, -d / 2);
  B.cbox('glassLit', w * 0.5, 2.6, 0.1, 0, 1.6, 0.04);
  B.box('white', w * 0.6, 0.3, 4, 0, 3.2, 2);
  for (let f = 1; f < floors; f++) {
    const y = f * fh;
    for (let x = -w / 2 + 2.2; x < w / 2 - 1; x += 4.4) {
      B.cbox(rng() < 0.4 ? 'glassLit' : 'glass', 2.4, 2.1, 0.1, x, y + 1.3, 0.03);
      B.box('white', 3.4, 0.16, 1.3, x, y, 0.65);
      B.cbox('balGlass', 3.4, 0.95, 0.04, x, y + 0.6, 1.3);
    }
  }
  for (let f = 1; f < floors; f++) {
    sideWindows(B, rng, 1, w / 2, -d + 1.5, -1.5, f * fh + 0.6, 2.0, 3.4, 0.4);
    sideWindows(B, rng, -1, -w / 2, -d + 1.5, -1.5, f * fh + 0.6, 2.0, 3.4, 0.4);
  }
  if (rect) {
    B.box('metal', 0.2, 2.2, 0.2, -6, H + 0.6, -2); B.box('metal', 0.2, 2.2, 0.2, 6, H + 0.6, -2);
    sign(B, rect, 15, 2.6, 0, H + 2.4, -1.9, 0, 'neon');
    B.box('black', 15.4, 2.9, 0.2, 0, H + 1.0, -2.05);
  }
  roofClutter(B, rng, w, d, H + 0.6);
}

// ── Street furniture ──────────────────────────────────────────────
export function bench(B, x, z, ry = 0) {
  B.pushFrame(x, 0, z, ry);
  B.box('wood', 1.9, 0.08, 0.45, 0, 0.45, 0);
  B.box('wood', 1.9, 0.4, 0.06, 0, 0.55, -0.22);
  for (const sx of [-0.8, 0.8]) B.box('black', 0.08, 0.45, 0.45, sx, 0, 0);
  B.popFrame();
}

// Promenade lamp: slim post with a pair of globes.
export function promLamp(B, x, z) {
  B.box('black', 0.14, 4.2, 0.14, x, 0, z);
  B.box('black', 1.2, 0.08, 0.08, x, 4.1, z);
  const s = new THREE.SphereGeometry(0.22, 10, 8);
  B.put('lampGlow', s, x - 0.55, 4.4, z); B.put('lampGlow', s, x + 0.55, 4.4, z);
}

// Cobra-head street light; arm reaches toward −Z by `reach`.
export function streetLight(B, x, z, ry, reach = 3.2, h = 8.6) {
  B.pushFrame(x, 0, z, ry);
  B.box('metal', 0.26, h, 0.26, 0, 0, 0);
  B.beam('metal', V(0, h - 0.2, 0), V(0, h + 0.4, reach * 0.5), 0.14);
  B.beam('metal', V(0, h + 0.4, reach * 0.5), V(0, h + 0.5, reach), 0.14);
  B.cbox('metal', 0.5, 0.22, 1.1, 0, h + 0.4, reach + 0.3);
  B.cbox('lampGlow', 0.38, 0.06, 0.8, 0, h + 0.27, reach + 0.3);
  B.popFrame();
}

export function trashCan(B, x, z) {
  B.put('wTeal', new THREE.CylinderGeometry(0.32, 0.3, 0.95, 10), x, 0.48, z);
}

// Lifeguard tower: hut on stilts with a ramp, number on the side.
export function lifeguardTower(B, rng, rect) {
  const col = rpick(rng, ['wBlue', 'wYellow', 'wPink', 'wTeal']);
  for (const [sx, sz] of [[-1.3, -1.1], [1.3, -1.1], [-1.3, 1.1], [1.3, 1.1]]) B.box('white', 0.2, 2.2, 0.2, sx, 0, sz);
  B.box('white', 3.4, 0.2, 3, 0, 2.2, 0);
  B.box(col, 3, 2.2, 2.6, 0, 2.4, -0.1);
  B.cbox('glass', 2.4, 0.9, 0.08, 0, 3.7, 1.21);
  B.cbox('glass', 0.08, 0.9, 1.6, 1.51, 3.7, -0.1);
  B.cbox('glass', 0.08, 0.9, 1.6, -1.51, 3.7, -0.1);
  const cone = new THREE.ConeGeometry(Math.SQRT1_2, 1, 4, 1);
  cone.rotateY(Math.PI / 4);
  B.put('white', cone, 0, 5.05, -0.1, 0, 0, 0, 3.8, 0.9, 3.4);
  // Deck + ramp down to the sand.
  B.box('wood', 3.4, 0.12, 1.2, 0, 2.2, 1.9);
  B.cbox('wood', 1.3, 0.1, 4.6, 0, 1.05, 4.3, 0.49, 0, 0);
  B.box('white', 3.4, 0.06, 0.06, 0, 3.2, 2.5);
  if (rect) sign(B, rect, 1.4, 1.1, 1.52, 3.0, -0.1, Math.PI / 2);
  // Rescue board and buoy.
  B.put('paintRed', new THREE.CylinderGeometry(1, 1, 1, 12), -1.9, 1.4, 0.3, 0, 0, 0.12, 0.26, 2.6, 0.05);
}

// Beach umbrella (mostly still folded at dawn) with a couple of loungers.
export function umbrellaSet(B, rng, fabric, open = rng() < 0.55) {
  B.box('white', 0.07, open ? 2.6 : 2.5, 0.07, 0, -0.2, 0);
  if (open) {
    B.put(fabric, new THREE.ConeGeometry(1.5, 0.55, 10, 1, true), 0, 2.45, 0);
    B.put('white', new THREE.SphereGeometry(0.06, 5, 4), 0, 2.75, 0);
  } else {
    B.put(fabric, new THREE.ConeGeometry(0.36, 1.7, 8, 1, true), 0, 1.55, 0);
    B.put(fabric, new THREE.ConeGeometry(0.36, 0.5, 8, 1, true), 0, 2.65, 0, Math.PI, 0, 0);
  }
  if (rng() < 0.5) {
    for (const sx of [-1, 1]) {
      B.pushFrame(sx * 1.2, 0, 0.7, rrange(rng, -0.2, 0.2));
      B.box(fabric, 0.65, 0.08, 1.9, 0, 0.32, 0);
      B.cbox(fabric, 0.65, 0.08, 0.6, 0, 0.55, -0.85, -0.9, 0, 0);
      for (const lz of [-0.8, 0.8]) B.box('white', 0.6, 0.3, 0.05, 0, 0, lz);
      B.popFrame();
    }
  } else {
    // Towels spread on the sand, a cooler and a bag.
    const n = 1 + Math.floor(rng() * 2);
    for (let k = 0; k < n; k++) {
      B.pushFrame(rrange(rng, -1.6, 1.6), 0, rrange(rng, 0.6, 1.6), rrange(rng, -0.5, 0.5));
      B.box(rpick(rng, AWNINGS), 0.9, 0.03, 1.8, 0, 0.01, 0);
      B.popFrame();
    }
    if (rng() < 0.6) B.box(rpick(rng, ['wTeal', 'paintRed', 'wBlue', 'white']), 0.55, 0.4, 0.38, rrange(rng, -1, 1), 0, -0.6, rng());
    if (rng() < 0.4) B.box(rpick(rng, ['wYellow', 'wPink', 'wMint']), 0.45, 0.3, 0.25, rrange(rng, -1, 1), 0, 0.3, rng());
  }
}

export function volleyballNet(B) {
  for (const sx of [-4.8, 4.8]) B.box('white', 0.12, 2.6, 0.12, sx, 0, 0);
  B.cbox('black', 9.5, 0.9, 0.03, 0, 2.05, 0);
  B.cbox('white', 9.5, 0.06, 0.05, 0, 2.5, 0);
}

export function surfboardsInSand(B, rng, n = 3) {
  const g = new THREE.CylinderGeometry(1, 1, 1, 14);
  for (let i = 0; i < n; i++) {
    B.put(rng() < 0.5 ? 'wTeal' : 'boardB', g, i * 0.7, 1.05, rrange(rng, -0.2, 0.2), rrange(rng, -0.12, 0.12), 0, rrange(rng, -0.15, 0.15), 0.27, 2.2, 0.035);
  }
}

// ── Boats ─────────────────────────────────────────────────────────
// Hull as a lathe-ish extrusion: pointed bow at +Z.
function hullGeometry(L, W, H) {
  const s = new THREE.Shape();
  s.moveTo(-W / 2, -L / 2);
  s.lineTo(W / 2, -L / 2);
  s.quadraticCurveTo(W / 2, L * 0.2, 0, L / 2);
  s.quadraticCurveTo(-W / 2, L * 0.2, -W / 2, -L / 2);
  const g = new THREE.ExtrudeGeometry(s, { depth: H, bevelEnabled: true, bevelThickness: 0.15, bevelSize: 0.12, bevelSegments: 2, curveSegments: 8 });
  g.rotateX(-Math.PI / 2); // extrude upward, shape in XZ (y of shape → −z)
  g.scale(1, 1, -1);
  return g;
}

export function sailboat(B, rng) {
  const L = rrange(rng, 8, 12), W = L * 0.32, H = 1.1;
  B.put('hullWhite', hullGeometry(L, W, H), 0, -0.4, 0);
  B.box('wood', W * 0.8, 0.08, L * 0.7, 0, H - 0.35, -L * 0.1);
  B.box('hullWhite', W * 0.55, 0.7, L * 0.3, 0, H - 0.3, -L * 0.08);
  B.cbox('glass', W * 0.56, 0.25, L * 0.2, 0, H + 0.2, -L * 0.02);
  const mh = L * 1.35;
  B.box('metal', 0.14, mh, 0.14, 0, H - 0.3, L * 0.08);
  B.beam('metal', V(0, H + 0.6, L * 0.08), V(0, H + 0.6, -L * 0.42), 0.1);           // boom
  B.cbox(rpick(rng, ['sailBlue', 'white', 'sailBlue']), 0.34, 0.4, L * 0.44, 0, H + 0.82, -L * 0.17); // furled sail cover
  B.beam('black', V(0, H + mh - 0.6, L * 0.08), V(0, H - 0.2, L * 0.48), 0.03);   // forestay
  B.beam('black', V(0, H + mh - 0.6, L * 0.08), V(0, H - 0.2, -L * 0.48), 0.03);  // backstay
  return mh + H;
}

export function motorboat(B, rng) {
  const L = rrange(rng, 6, 9), W = L * 0.36, H = 1.0;
  B.put(rng() < 0.5 ? 'hullWhite' : 'wBlue', hullGeometry(L, W, H), 0, -0.4, 0);
  B.box('hullWhite', W * 0.6, 0.9, L * 0.3, 0, H - 0.3, L * 0.02);
  B.cbox('glass', W * 0.62, 0.45, 0.06, 0, H + 0.3, L * 0.18, 0.5, 0, 0);
  B.box('wood', W * 0.8, 0.06, L * 0.3, 0, H - 0.35, -L * 0.3);
}
