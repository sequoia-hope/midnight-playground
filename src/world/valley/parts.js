import * as THREE from 'three';
import { rrange, rpick } from '../../util/math.js';

// Farm building kit. Every function draws into a Builder in its current
// frame: local +Z is the front (facing the road), X is across, Y is up, and
// y = 0 is the ground.

const V = (x, y, z) => new THREE.Vector3(x, y, z);

// Gable-ended prism filling the triangle above the walls.
function gablePrism(w, d, rh) {
  const s = new THREE.Shape();
  s.moveTo(-d / 2, 0); s.lineTo(d / 2, 0); s.lineTo(0, rh); s.closePath();
  const g = new THREE.ExtrudeGeometry(s, { depth: w, bevelEnabled: false });
  g.rotateY(Math.PI / 2);
  g.translate(-w / 2, 0, 0);
  return g;
}

// Two sloped roof slabs over a gable (ridge along X).
function gableRoof(B, key, w, d, H, rh, over = 0.5, t = 0.16) {
  const a = Math.atan2(rh, d / 2);
  const L = (d / 2) / Math.cos(a) + over;
  for (const side of [1, -1]) {
    const cz = side * (L / 2) * Math.cos(a) + side * Math.sin(a) * t * 0.5;
    const cy = H + rh - (L / 2) * Math.sin(a) + Math.cos(a) * t * 0.5;
    B.cbox(key, w + over * 2, t, L, 0, cy, cz, side * a, 0, 0);
  }
}

// Sash window: glass, frame, a mullion cross and (optionally) a pair of
// louvred shutters. ry turns it about Y; the glass faces local +Z.
function windowPane(B, lit, x, y, z, w, h, ry = 0, shutter = null) {
  B.cbox(lit ? 'window' : 'windowDark', w, h, 0.08, x, y, z, 0, ry, 0);
  const fr = 'trim';
  const c = Math.cos(ry), s = Math.sin(ry);
  const off = (dx, dn = 0) => [x + c * dx + s * dn, z - s * dx + c * dn];
  let [fx, fz] = off(0);
  B.cbox(fr, w + 0.16, 0.08, 0.1, fx, y + h / 2 + 0.04, fz, 0, ry, 0);
  B.cbox(fr, w + 0.2, 0.1, 0.16, fx, y - h / 2 - 0.05, fz, 0, ry, 0);
  [fx, fz] = off(-w / 2 - 0.04);
  B.cbox(fr, 0.08, h, 0.1, fx, y, fz, 0, ry, 0);
  [fx, fz] = off(w / 2 + 0.04);
  B.cbox(fr, 0.08, h, 0.1, fx, y, fz, 0, ry, 0);
  // Mullions: the meeting rail and a centre bar.
  [fx, fz] = off(0, 0.04);
  B.cbox(fr, w, 0.05, 0.04, fx, y, fz, 0, ry, 0);
  B.cbox(fr, 0.04, h, 0.04, fx, y, fz, 0, ry, 0);
  if (shutter) {
    for (const sx of [-1, 1]) {
      [fx, fz] = off(sx * (w / 2 + 0.1 + w * 0.24), 0.03);
      B.cbox(shutter, w * 0.46, h * 1.02, 0.06, fx, y, fz, 0, ry, 0);
    }
  }
}

// Returns local anchor points of interest (porch light, front door).
export function farmhouse(B, rng, opts = {}) {
  const twoStory = opts.twoStory ?? rng() < 0.6;
  const w = rrange(rng, 9, 12), d = rrange(rng, 7, 8.5);
  const H = twoStory ? 5.8 : 3.2;
  const rh = d * 0.42;
  const wall = opts.wall || rpick(rng, ['wallCream', 'wallWhite', 'wallWhite', 'wallYellow', 'wallBlue', 'wallGreen']);
  const roof = opts.roof || rpick(rng, ['roofShingle', 'roofMetal', 'roofRust', 'roofSlate', 'roofGreen']);
  const lit = () => rng() < 0.6;
  const shutter = rng() < 0.65 ? rpick(rng, ['shutterGreen', 'shutterBlack', 'shutterRed', 'shutterBlue']) : null;
  // Foundation + walls
  B.box('stone', w + 0.3, 0.5, d + 0.3, 0, -0.3, 0);
  B.box(wall, w, H, d, 0, 0.2, 0);
  B.put(wall, gablePrism(w, d, rh), 0, H + 0.2, 0);
  gableRoof(B, roof, w, d, H + 0.2, rh);
  // Corner trim
  for (const sx of [-1, 1]) for (const sz of [-1, 1]) B.box('trim', 0.18, H, 0.18, sx * w / 2, 0.2, sz * d / 2);
  // Barge boards up both gable ends.
  for (const sx of [-1, 1]) {
    for (const sz of [-1, 1]) B.beam('trim', V(sx * (w / 2 + 0.52), H + 0.1, sz * (d / 2 + 0.55)), V(sx * (w / 2 + 0.52), H + 0.2 + rh + 0.12, 0), 0.14);
  }
  // Chimney with a corbelled cap.
  const cx = rrange(rng, -w * 0.3, w * 0.3);
  B.box('brick', 0.8, rh + 1.6, 0.8, cx, H + 0.2, -d * 0.12);
  B.box('brick', 1.0, 0.2, 1.0, cx, H + rh + 1.6, -d * 0.12);
  // Kitchen wing out the back on some houses: a lower gable.
  if (rng() < 0.6) {
    const ww = w * rrange(rng, 0.45, 0.6), wd = d * 0.55, wh = 3.0, wrh = wd * 0.4;
    const wx = rrange(rng, -1, 1) * (w - ww) * 0.4;
    B.pushFrame(wx, 0, -d / 2 - wd / 2 + 0.1, 0);
    B.box('stone', ww + 0.3, 0.5, wd + 0.3, 0, -0.3, 0);
    B.box(wall, ww, wh, wd, 0, 0.2, 0);
    B.put(wall, gablePrism(ww, wd, wrh), 0, wh + 0.2, 0);
    gableRoof(B, roof, ww, wd, wh + 0.2, wrh, 0.4);
    windowPane(B, lit(), 0, 1.7, -wd / 2 - 0.03, 1.0, 1.2, Math.PI, shutter);
    B.popFrame();
  }
  // Front door + porch
  B.cbox('woodDark', 1.1, 2.1, 0.1, 0, 1.25, d / 2 + 0.04);
  const pw = w * 0.7, pd = 2.4;
  B.box('woodGray', pw, 0.35, pd, 0, 0, d / 2 + pd / 2);
  for (const px of [-pw / 2 + 0.15, -pw / 6, pw / 6, pw / 2 - 0.15]) B.box('trim', 0.14, 2.6, 0.14, px, 0.35, d / 2 + pd - 0.15);
  B.cbox(roof, pw + 0.4, 0.12, pd + 0.4, 0, 3.05, d / 2 + pd / 2, 0.14, 0, 0);
  // Porch railing with balusters (gap at the steps), steps down to the yard.
  const rz = d / 2 + pd - 0.15;
  for (const sx of [-1, 1]) {
    const x0 = sx * 0.8, x1 = sx * (pw / 2 - 0.15);
    B.beam('trim', V(x0, 1.1, rz), V(x1, 1.1, rz), 0.08);
    B.beam('trim', V(x0, 0.45, rz), V(x1, 0.45, rz), 0.06);
    for (let bx = x0; Math.abs(bx) < Math.abs(x1); bx += sx * 0.34) B.box('trim', 0.05, 0.65, 0.05, bx, 0.45, rz);
  }
  for (let k = 0; k < 2; k++) B.box('woodGray', 1.6, 0.35 - k * 0.17, 0.35, 0, -0.1, d / 2 + pd + 0.17 + k * 0.33);
  // Porch fascia board.
  B.box('trim', pw + 0.3, 0.22, 0.08, 0, 2.85, d / 2 + pd + 0.05);
  // Windows (ground floor + upper floor)
  const floors = twoStory ? [1.5, 4.4] : [1.55];
  for (const fy of floors) {
    for (const wx of [-w * 0.33, w * 0.33]) windowPane(B, lit(), wx, fy + 0.2, d / 2 + 0.03, 1.0, 1.4, 0, shutter);
    if (fy > 3) windowPane(B, lit(), 0, fy + 0.2, d / 2 + 0.03, 0.9, 1.3, 0, shutter);
    for (const sx of [-1, 1]) windowPane(B, lit(), sx * (w / 2 + 0.03), fy + 0.2, 0, 1.0, 1.4, sx * Math.PI / 2, shutter);
    windowPane(B, lit(), -w * 0.2, fy + 0.2, -d / 2 - 0.03, 1.0, 1.4, Math.PI, shutter);
  }
  // Attic gable window
  windowPane(B, rng() < 0.5, w / 2 + 0.03, H + 0.2 + rh * 0.35, 0, 0.6, 0.6, Math.PI / 2);
  // Porch lamp
  B.cbox('lamp', 0.2, 0.26, 0.2, pw * 0.25, 2.55, d / 2 + 0.25);
  return { door: V(0, 0, d / 2 + pd), lamp: V(pw * 0.25, 2.55, d / 2 + 0.3), w, d };
}

// Faded red barn: gambrel (or gable) roof, white trim, X-braced doors.
export function barn(B, rng, opts = {}) {
  const w = rrange(rng, 11, 14), d = rrange(rng, 16, 22), H = rrange(rng, 4.2, 5.2);
  const W = w / 2, R = W * 0.78;
  const body = opts.body || rpick(rng, ['barnRed', 'barnRed', 'barnRed2', 'woodGray']);
  const roof = opts.roof || rpick(rng, ['roofMetal', 'roofRust', 'roofShingle']);
  const gambrel = opts.gambrel ?? rng() < 0.75;
  const prof = gambrel
    ? [[-W, 0], [W, 0], [W, H], [0.72 * W, H + 0.62 * R], [0, H + R], [-0.72 * W, H + 0.62 * R], [-W, H]]
    : [[-W, 0], [W, 0], [W, H], [0, H + R * 0.8], [-W, H]];
  const s = new THREE.Shape();
  prof.forEach(([x, y], i) => (i ? s.lineTo(x, y) : s.moveTo(x, y)));
  s.closePath();
  const g = new THREE.ExtrudeGeometry(s, { depth: d, bevelEnabled: false });
  g.translate(0, 0, -d / 2);
  B.box('stone', w + 0.3, 0.45, d + 0.3, 0, -0.3, 0);
  B.put(body, g, 0, 0.1, 0);
  // Roof cap: a thin band following the upper outline.
  const top = prof.slice(2); // from right eave over the ridge to left eave
  const t = 0.28;
  const outer = top.map(([x, y]) => {
    const nx = Math.sign(x) * (Math.abs(x) >= W - 0.01 ? 1 : 0.55);
    return [x + nx * (Math.abs(x) >= W - 0.01 ? 0.45 : t), y + (Math.abs(x) >= W - 0.01 ? -0.35 : t)];
  });
  const cap = new THREE.Shape();
  outer.forEach(([x, y], i) => (i ? cap.lineTo(x, y) : cap.moveTo(x, y)));
  top.slice().reverse().forEach(([x, y]) => cap.lineTo(x, y));
  cap.closePath();
  const cg = new THREE.ExtrudeGeometry(cap, { depth: d + 0.9, bevelEnabled: false });
  cg.translate(0, 0, -(d + 0.9) / 2);
  B.put(roof, cg, 0, 0.1, 0);
  // Trim: corners and eave lines on the front gable.
  for (const sx of [-1, 1]) for (const sz of [-1, 1]) B.box('trim', 0.22, H, 0.22, sx * W, 0.1, sz * d / 2);
  for (const sz of [-1, 1]) {
    for (let i = 2; i < prof.length - 1; i++) {
      const [x0, y0] = prof[i], [x1, y1] = prof[i + 1];
      B.beam('trim', V(x0, y0 + 0.1, sz * (d / 2 + 0.03)), V(x1, y1 + 0.1, sz * (d / 2 + 0.03)), 0.2);
    }
    const [xl, yl] = prof[prof.length - 1];
    B.beam('trim', V(xl, yl + 0.1, sz * (d / 2 + 0.03)), V(prof[prof.length - 2][0], prof[prof.length - 2][1] + 0.1, sz * (d / 2 + 0.03)), 0.2);
  }
  // Big doors with white border and X-bracing.
  const dw = Math.min(5, w * 0.42), dh = Math.min(3.9, H - 0.4);
  const fz = d / 2 + 0.06;
  B.cbox('barnDoor', dw, dh, 0.1, 0, 0.1 + dh / 2, fz);
  B.cbox('trim', dw + 0.3, 0.2, 0.14, 0, 0.1 + dh, fz);
  for (const sx of [-1, 0, 1]) B.cbox('trim', 0.18, dh, 0.14, sx * dw / 2, 0.1 + dh / 2, fz);
  const diag = Math.hypot(dw / 2, dh);
  for (const sx of [-1, 1]) {
    const a = Math.atan2(dw / 2, dh);
    B.cbox('trim', 0.16, diag, 0.13, sx * dw / 4, 0.1 + dh / 2, fz + 0.01, 0, 0, a);
    B.cbox('trim', 0.16, diag, 0.13, sx * dw / 4, 0.1 + dh / 2, fz + 0.01, 0, 0, -a);
  }
  // Hayloft door.
  const hy = H + (gambrel ? 0.5 : 0.2);
  B.cbox('barnDoor', 1.8, 1.6, 0.1, 0, hy + 0.8, fz);
  B.cbox('trim', 2.1, 0.16, 0.13, 0, hy + 1.65, fz);
  B.cbox('trim', 2.1, 0.16, 0.13, 0, hy - 0.05, fz);
  // Hay hood beam.
  B.cbox('woodDark', 0.2, 0.2, 1.2, 0, hy + 2.0, fz + 0.5);
  // Side windows (small, dark).
  for (let k = -1; k <= 1; k++) for (const sx of [-1, 1]) B.cbox('windowDark', 0.08, 0.8, 1.0, sx * (W + 0.03), H * 0.6, k * d * 0.3);
  // Yard light on the gable.
  B.cbox('lamp', 0.35, 0.2, 0.35, 0, H + (gambrel ? R * 0.35 : R * 0.2), fz + 0.4);
  // Ventilator cupolas along the ridge, with a weathervane on the first.
  const ridge = H + (gambrel ? R : R * 0.8) + 0.1;
  const nCup = rng() < 0.5 ? 1 : 2;
  for (let k = 0; k < nCup; k++) {
    const cz = nCup === 1 ? 0 : (k ? 1 : -1) * d * 0.22;
    B.box(body, 1.5, 1.5, 1.5, 0, ridge - 0.5, cz);
    for (const sx of [-1, 1]) B.cbox('trim', 0.06, 0.9, 1.1, sx * 0.77, ridge + 0.5, cz);
    B.put(roof, new THREE.ConeGeometry(1.35, 1.1, 4, 1), 0, ridge + 1.55, cz, 0, Math.PI / 4, 0);
    if (k === 0) {
      B.box('steelOld', 0.05, 1.3, 0.05, 0, ridge + 2.0, cz);
      B.cbox('steelOld', 0.04, 0.3, 0.9, 0, ridge + 2.95, cz);
      B.cbox('steelOld', 0.7, 0.03, 0.03, 0, ridge + 2.7, cz);
      B.cbox('steelOld', 0.03, 0.03, 0.7, 0, ridge + 2.7, cz);
    }
  }
  // Open-fronted lean-to down one side on most barns.
  if (rng() < 0.65) {
    const sx = rng() < 0.5 ? -1 : 1, lw = 4.5, lh = H * 0.62, ld = d * rrange(rng, 0.45, 0.7);
    const lz = rrange(rng, -0.2, 0.2) * (d - ld);
    B.box(body, 0.25, lh, ld, sx * (W + lw - 0.12), 0, lz);
    B.box(body, lw, lh, 0.25, sx * (W + lw / 2), 0, lz - ld / 2 + 0.12);
    for (let k = 1; k < 4; k++) B.box('woodDark', 0.2, lh, 0.2, sx * (W + lw - 0.12), 0, lz - ld / 2 + (k * ld) / 4);
    const a = Math.atan2(H - 0.4 - lh, lw);
    B.cbox(roof, lw + 0.8, 0.14, ld + 0.6, sx * (W + lw / 2 + 0.1), (H - 0.35 + lh) / 2 + 0.2, lz, 0, 0, -sx * a);
  }
  return { w, d, H: H + R, lamp: V(0, H + R * 0.35, fz + 0.4) };
}

export function silo(B, rng, x, z, opts = {}) {
  const r = opts.r ?? rrange(rng, 2.2, 3.0);
  const h = opts.h ?? rrange(rng, 11, 16);
  const metal = opts.metal ?? rng() < 0.5;
  const key = metal ? 'siloMetal' : 'concrete';
  B.put(key, new THREE.CylinderGeometry(r, r, h, 18, 1, true), x, h / 2, z);
  B.put('siloDome', new THREE.SphereGeometry(r * 1.03, 18, 7, 0, Math.PI * 2, 0, Math.PI / 2), x, h, z);
  const bands = metal ? 7 : 4;
  for (let i = 1; i < bands; i++) B.put('siloBand', new THREE.CylinderGeometry(r + 0.05, r + 0.05, 0.12, 18, 1, true), x, (h * i) / bands, z);
  // Ladder cage with hoops, a vent cap and (on stave silos) the chute.
  B.box('siloBand', 0.5, h, 0.08, x, 0, z + r + 0.15);
  for (let y = 2.5; y < h; y += 1.4) B.put('siloBand', new THREE.TorusGeometry(0.4, 0.03, 3, 8, Math.PI), x, y, z + r + 0.2, Math.PI / 2, 0, 0);
  B.put('siloBand', new THREE.CylinderGeometry(0.25, 0.35, 0.6, 8), x, h + r * 1.03 - 0.1, z);
  if (!metal) B.box('woodGray', 0.9, h - 1, 0.7, x - r - 0.3, 0, z);
  return { r, h };
}

export function shed(B, rng, opts = {}) {
  const w = opts.w ?? rrange(rng, 5, 8), d = opts.d ?? rrange(rng, 4, 6), H = rrange(rng, 2.6, 3.4);
  const wall = opts.wall || rpick(rng, ['woodGray', 'woodGray', 'barnRed2']);
  B.box(wall, w, H, d, 0, 0, 0);
  // Lean-to roof sloping back.
  const a = 0.18;
  B.cbox(opts.roof || 'roofRust', w + 0.6, 0.12, d + 0.8, 0, H + 0.25, 0, -a, 0, 0);
  B.cbox('woodDark', w * 0.45, H * 0.75, 0.08, -w * 0.15, H * 0.375, d / 2 + 0.04);
  return { w, d };
}

// Lattice tower of an old windpump; returns the hub position.
export function windpumpTower(B, h = 10) {
  const base = 1.5, top = 0.3;
  const corners = [[-1, -1], [1, -1], [1, 1], [-1, 1]];
  for (const [cx, cz] of corners) B.beam('steelOld', V(cx * base, 0, cz * base), V(cx * top, h, cz * top), 0.1);
  for (let lv = 0; lv < 4; lv++) {
    const y0 = (h * lv) / 4, y1 = (h * (lv + 1)) / 4;
    const w0 = base + (top - base) * (lv / 4), w1 = base + (top - base) * ((lv + 1) / 4);
    for (let e = 0; e < 4; e++) {
      const [ax, az] = corners[e], [bx, bz] = corners[(e + 1) % 4];
      B.beam('steelOld', V(ax * w0, y0, az * w0), V(bx * w1, y1, bz * w1), 0.05);
      B.beam('steelOld', V(ax * w1, y1, az * w1), V(bx * w1, y1, bz * w1), 0.06);
    }
  }
  // Head, tail boom and vane (vane faces along -Z: wheel faces +Z).
  B.cbox('steelOld', 0.5, 0.5, 0.9, 0, h + 0.3, 0);
  B.cbox('steelOld', 0.08, 0.08, 2.6, 0, h + 0.35, -1.6);
  B.cbox('vane', 0.05, 1.1, 1.6, 0, h + 0.45, -3.0);
  // Pump rod + water tank at the foot.
  B.beam('steelOld', V(0, 0.5, 0), V(0, h, 0), 0.05);
  B.put('woodDark', new THREE.CylinderGeometry(1.6, 1.6, 1.2, 14), 2.8, 0.6, 1.2);
  return V(0, h + 0.3, 0.75);
}

// Blade wheel geometry (axis = local Z), for instancing.
export function windpumpWheelGeometry(B) {
  const n = 16;
  for (let i = 0; i < n; i++) {
    const a = (i / n) * Math.PI * 2;
    const r = 1.35;
    B.put('x', new THREE.BoxGeometry(0.42, 1.5, 0.03), Math.sin(a) * r, Math.cos(a) * r, 0, 0.35, 0, -a);
  }
  B.put('x', new THREE.TorusGeometry(2.05, 0.035, 4, 32), 0, 0, 0);
  B.put('x', new THREE.TorusGeometry(0.65, 0.035, 4, 20), 0, 0, 0);
  for (let i = 0; i < 4; i++) B.put('x', new THREE.BoxGeometry(0.05, 4.1, 0.05), 0, 0, 0, 0, 0, (i * Math.PI) / 4);
  B.put('x', new THREE.CylinderGeometry(0.15, 0.15, 0.4, 8), 0, 0, 0, Math.PI / 2, 0, 0);
  return B.mergeAll();
}

// Mill waterwheel (axis = local X).
export function waterwheelGeometry(B, R = 3.2, W = 1.4) {
  for (const sx of [-W / 2, W / 2]) {
    B.put('x', new THREE.TorusGeometry(R, 0.1, 5, 32), sx, 0, 0, 0, Math.PI / 2, 0);
    B.put('x', new THREE.TorusGeometry(R * 0.45, 0.08, 5, 20), sx, 0, 0, 0, Math.PI / 2, 0);
    for (let i = 0; i < 8; i++) B.put('x', new THREE.BoxGeometry(0.12, R * 2, 0.14), sx, 0, 0, (i * Math.PI) / 8, 0, 0);
  }
  const n = 20;
  for (let i = 0; i < n; i++) {
    const a = (i / n) * Math.PI * 2;
    B.put('x', new THREE.BoxGeometry(W + 0.1, 0.06, 0.7), 0, Math.cos(a) * (R - 0.3), Math.sin(a) * (R - 0.3), -a, 0, 0);
  }
  B.put('x', new THREE.CylinderGeometry(0.22, 0.22, W + 1.2, 10), 0, 0, 0, 0, 0, Math.PI / 2);
  return B.mergeAll();
}

// Stone-and-timber watermill building (front toward +Z). Wheel mounts on +X side.
export function millHouse(B, rng) {
  const w = 9, d = 8, H = 6.5, rh = 3.2;
  B.box('stone', w + 0.6, 3.6, d + 0.6, 0, -3.4, 0);   // foundation, goes down the bank
  B.box('stoneLight', w, 3.2, d, 0, 0, 0);
  B.box('woodGray', w, H - 3.2, d, 0, 3.2, 0);
  B.put('woodGray', gablePrism(d, w, rh), 0, H, 0, 0, Math.PI / 2, 0);
  // Ridge runs along Z here (gable faces front).
  const a = Math.atan2(rh, w / 2);
  const L = (w / 2) / Math.cos(a) + 0.5;
  for (const side of [1, -1]) {
    B.cbox('roofShingle', L, 0.16, d + 1, side * (L / 2) * Math.cos(a), H + rh - (L / 2) * Math.sin(a) + 0.08, 0, 0, 0, -side * a);
  }
  for (const sx of [-1, 1]) for (const sz of [-1, 1]) B.box('trim', 0.2, H - 3.2, 0.2, sx * w / 2, 3.2, sz * d / 2);
  windowPane(B, true, -2, 4.6, d / 2 + 0.03, 1.0, 1.3);
  windowPane(B, rng() < 0.5, 2, 4.6, d / 2 + 0.03, 1.0, 1.3);
  windowPane(B, true, 0, 1.8, d / 2 + 0.03, 1.2, 1.1);
  B.cbox('woodDark', 1.4, 2.3, 0.1, -3, 1.15, d / 2 + 0.04);
  B.box('brick', 0.9, 3.5, 0.9, -2.5, H + 0.5, -1.5);
  // Axle bearing on the +X wall.
  B.cbox('steelOld', 0.6, 0.6, 0.6, w / 2 + 0.3, 1.8, 0);
  B.cbox('lamp', 0.25, 0.3, 0.25, -3, 2.6, d / 2 + 0.25);
  return { w, d, wheel: V(w / 2 + 1.4, 1.8, 0) };
}

// Western false-front general store with a porch; returns sign anchor.
export function generalStore(B, rng) {
  const w = 13, d = 10, H = 4.2;
  B.box('stone', w + 0.3, 0.45, d + 0.3, 0, -0.3, 0);
  B.box('woodGray', w, H, d, 0, 0.15, 0);
  B.put('woodGray', gablePrism(d, w, 2.2), 0, H + 0.15, 0, 0, Math.PI / 2, 0);
  gableRoofZ(B, 'roofRust', w, d, H + 0.15, 2.2);
  // False front
  B.box('wallCream', w + 0.6, 7.6, 0.35, 0, 0.15, d / 2 + 0.18);
  B.box('trim', w + 1.0, 0.35, 0.6, 0, 7.75, d / 2 + 0.2);
  // Porch
  B.box('woodDark', w + 1, 0.3, 3, 0, 0, d / 2 + 1.8);
  for (let k = 0; k < 5; k++) B.box('trim', 0.16, 3.1, 0.16, -w / 2 + (k * w) / 4, 0.3, d / 2 + 3.1);
  B.cbox('roofRust', w + 1.2, 0.12, 3.4, 0, 3.55, d / 2 + 1.9, 0.12, 0, 0);
  // Shop windows (lit) and door
  windowPane(B, true, -3.8, 1.9, d / 2 + 0.38, 2.6, 1.8);
  windowPane(B, true, 3.8, 1.9, d / 2 + 0.38, 2.6, 1.8);
  B.cbox('woodDark', 1.4, 2.4, 0.1, 0, 1.45, d / 2 + 0.4);
  windowPane(B, true, 0, 5.6, d / 2 + 0.38, 1.4, 1.1);
  B.cbox('lamp', 0.25, 0.3, 0.25, -1.3, 3.1, d / 2 + 0.5);
  B.cbox('lamp', 0.25, 0.3, 0.25, 1.3, 3.1, d / 2 + 0.5);
  return { w, d, sign: V(0, 6.6, d / 2 + 0.38), neon: V(3.8, 2.95, d / 2 + 0.42) };
}

// Gable roof with the ridge along Z (gable facing the front).
function gableRoofZ(B, key, w, d, H, rh, over = 0.4, t = 0.16) {
  const a = Math.atan2(rh, w / 2);
  const L = (w / 2) / Math.cos(a) + over;
  for (const side of [1, -1]) {
    B.cbox(key, L, t, d + over * 2, side * (L / 2) * Math.cos(a), H + rh - (L / 2) * Math.sin(a) + t / 2, 0, 0, 0, -side * a);
  }
}

// Vintage gas pump (front toward +Z).
export function gasPump(B, x, z) {
  B.box('pumpRed', 0.7, 1.7, 0.55, x, 0.25, z);
  B.cbox('trim', 0.5, 0.6, 0.05, x, 1.4, z + 0.29);
  B.put('pumpGlobe', new THREE.SphereGeometry(0.28, 12, 8), x, 2.25, z);
  B.box('stoneLight', 1.6, 0.25, 1.2, x, 0, z);
  B.cbox('black', 0.06, 0.06, 0.6, x + 0.38, 1.1, z + 0.1, 0.4, 0, 0);
}

export function mailbox(B, x, z, ry) {
  B.box('woodDark', 0.12, 1.05, 0.12, x, 0, z, ry);
  B.put('mailbox', new THREE.CylinderGeometry(0.2, 0.2, 0.55, 10, 1, false, 0, Math.PI), x, 1.2, z, 0, ry, Math.PI / 2);
  B.put('mailbox', new THREE.BoxGeometry(0.4, 0.2, 0.55), x, 1.1, z, 0, ry, 0);
}

// Paddock fence around a rectangle (local), leaving a gate gap on +Z.
export function paddock(B, w, d, gate = 4) {
  const posts = [];
  const edge = (ax, az, bx, bz, gap) => {
    const len = Math.hypot(bx - ax, bz - az);
    const n = Math.max(1, Math.round(len / 3));
    for (let i = 0; i <= n; i++) {
      const t = i / n;
      const x = ax + (bx - ax) * t, z = az + (bz - az) * t;
      if (gap && Math.abs(x) < gate / 2) continue;
      posts.push([x, z]);
    }
    for (const h of [0.55, 1.05]) {
      if (gap) {
        B.beam('woodGray', V(ax, h, az), V(-gate / 2, h, bz), 0.09);
        B.beam('woodGray', V(gate / 2, h, az), V(bx, h, bz), 0.09);
      } else B.beam('woodGray', V(ax, h, az), V(bx, h, bz), 0.09);
    }
  };
  edge(-w / 2, d / 2, w / 2, d / 2, true);
  edge(w / 2, d / 2, w / 2, -d / 2);
  edge(w / 2, -d / 2, -w / 2, -d / 2);
  edge(-w / 2, -d / 2, -w / 2, d / 2);
  for (const [x, z] of posts) B.box('woodDark', 0.14, 1.25, 0.14, x, 0, z);
}

// Wooden board sign on two posts, textured face.
export function boardSign(B, key, w, h, x, z, ry, y0 = 1.0) {
  const c = Math.cos(ry), s = Math.sin(ry);
  for (const sx of [-1, 1]) B.box('woodDark', 0.16, y0 + h, 0.16, x + c * sx * (w / 2 - 0.2), 0, z - s * sx * (w / 2 - 0.2));
  B.put('woodDark', new THREE.BoxGeometry(w + 0.2, h + 0.2, 0.08), x - s * 0.02, y0 + h / 2, z - c * 0.02, 0, ry, 0);
  B.put(key, new THREE.PlaneGeometry(w, h), x + s * 0.05, y0 + h / 2, z + c * 0.05, 0, ry, 0);
}

// Merged cow geometry (length along Z, head at +Z) for instancing: a
// rounded barrel of a body, bony hips, a drooping neck and tapered head.
export function cowGeometry(B) {
  B.put('x', new THREE.CapsuleGeometry(0.4, 0.95, 3, 8), 0, 1.02, -0.02, Math.PI / 2, 0, 0, 1, 1, 1.05);
  B.put('x', new THREE.BoxGeometry(0.62, 0.3, 0.4), 0, 1.3, -0.62);                    // hips
  B.put('x', new THREE.CylinderGeometry(0.2, 0.3, 0.55, 7), 0, 1.18, 0.82, 1.05, 0, 0);   // neck
  B.put('x', new THREE.CylinderGeometry(0.11, 0.19, 0.55, 7), 0, 1.02, 1.18, 2.2, 0, 0);  // head, muzzle down
  for (const sx of [-1, 1]) {
    B.put('x', new THREE.BoxGeometry(0.2, 0.05, 0.1), sx * 0.2, 1.22, 1.05, 0, 0, sx * 0.3); // ears
    B.put('x', new THREE.ConeGeometry(0.03, 0.14, 4), sx * 0.11, 1.33, 1.03, 0, 0, -sx * 0.6); // horns
  }
  for (const [lx, lz] of [[-0.24, 0.52], [0.24, 0.52], [-0.24, -0.58], [0.24, -0.58]]) B.put('x', new THREE.CylinderGeometry(0.07, 0.1, 0.78, 5), lx, 0.39, lz);
  B.put('x', new THREE.CylinderGeometry(0.025, 0.03, 0.7, 4), 0, 1.0, -0.98, 0.18, 0, 0);
  B.put('x', new THREE.SphereGeometry(0.14, 6, 4), 0, 0.62, -0.25, 0, 0, 0, 1, 0.7, 1.2); // udder
  return B.mergeAll();
}

// Telephone pole with crossarm (arm along local X).
export function poleGeometry(B) {
  B.put('x', new THREE.CylinderGeometry(0.13, 0.17, 9.2, 7), 0, 4.6, 0);
  B.put('x', new THREE.BoxGeometry(2.2, 0.14, 0.14), 0, 8.6, 0);
  for (const x of [-0.95, 0, 0.95]) B.put('x', new THREE.CylinderGeometry(0.05, 0.06, 0.2, 6), x, 8.77, 0);
  B.put('x', new THREE.BoxGeometry(0.06, 0.8, 0.06), 0.5, 8.2, 0, 0, 0, 0.9);
  B.put('x', new THREE.BoxGeometry(0.06, 0.8, 0.06), -0.5, 8.2, 0, 0, 0, -0.9);
  return B.mergeAll();
}

// Tower mill (the valley's windmill): a stone plinth, a tapering white
// boarded tower with a reefing stage, and a boat-shaped cap. The sails
// turn separately; returns the windshaft hub (sails face local +Z).
export function towerMill(B, rng) {
  B.put('stone', new THREE.CylinderGeometry(3.3, 3.7, 3, 8), 0, 1.3, 0, 0, Math.PI / 8, 0);
  B.put('barnWhite', new THREE.CylinderGeometry(2.1, 3.1, 9.4, 8), 0, 2.8 + 4.7, 0, 0, Math.PI / 8, 0);
  // Stage (gallery) with posts and a rail.
  B.put('woodDark', new THREE.CylinderGeometry(4.3, 4.3, 0.18, 16), 0, 5.4, 0);
  for (let k = 0; k < 16; k++) {
    const a = (k / 16) * Math.PI * 2;
    B.box('woodDark', 0.1, 1.0, 0.1, Math.cos(a) * 4.2, 5.45, Math.sin(a) * 4.2);
    const a2 = ((k + 1) / 16) * Math.PI * 2;
    B.beam('woodDark', V(Math.cos(a) * 4.2, 6.4, Math.sin(a) * 4.2), V(Math.cos(a2) * 4.2, 6.4, Math.sin(a2) * 4.2), 0.08);
    if (k % 4 === 0) B.beam('woodDark', V(Math.cos(a) * 4.2, 5.4, Math.sin(a) * 4.2), V(Math.cos(a) * 2.9, 3.4, Math.sin(a) * 2.9), 0.12);
  }
  // Cap: a ring with an onion of shingles, stretched fore-aft.
  B.put('woodDark', new THREE.CylinderGeometry(2.35, 2.35, 0.5, 16), 0, 12.45, 0);
  B.put('roofShingle', new THREE.SphereGeometry(2.4, 14, 6, 0, Math.PI * 2, 0, Math.PI / 2), 0, 12.65, 0, 0, 0, 0, 1, 0.95, 1.3);
  B.put('trim', new THREE.SphereGeometry(0.25, 8, 6), 0, 14.95, 0);
  // Fantail steering gear at the back.
  B.beam('woodDark', V(0, 12.5, -2.2), V(0, 11.0, -4.8), 0.12);
  B.put('vane', new THREE.CylinderGeometry(0.9, 0.9, 0.08, 10), 0, 11.4, -5.0, 0, 0, Math.PI / 2);
  // Door and windows up the tower.
  B.cbox('woodDark', 1.2, 2.2, 0.12, 0, 1.1 + 0.2, 3.62, -0.0, 0, 0);
  windowPane(B, rng() < 0.7, 0, 4.1, 3.1, 0.7, 0.9, 0, null);
  windowPane(B, rng() < 0.7, 0, 8.3, 2.55, 0.6, 0.8, 0, null);
  windowPane(B, false, 0, 10.4, -2.35, 0.55, 0.7, Math.PI, null);
  // Windshaft out through the front of the cap, clear of the stage.
  B.beam('steelOld', V(0, 13.25, 1.6), V(0, 13.1, 4.6), 0.4);
  return V(0, 13.1, 4.75);
}

// Four sails on a windshaft (axis = local Z): stocks, the lattice of each
// sail frame and its canvas. Built with a PaintBuilder so the wood and the
// sailcloth stay separate colours in one geometry.
export function millSailsGeometry(B) {
  for (let k = 0; k < 4; k++) {
    const a = (k / 4) * Math.PI * 2;
    const c = Math.cos(a), s = Math.sin(a);
    // Rotate a local (x across, y out) point into the sail's plane.
    const P = (x, y, z = 0) => V(c * x - s * y, s * x + c * y, z);
    B.beam('wood', P(0, -0.2), P(0, 10.8), 0.26);
    const x0 = 0.25, x1 = 2.1, r0 = 1.8, r1 = 10.4;
    B.beam('wood', P(x1, r0, 0.05), P(x1, r1, 0.05), 0.09);
    for (let r = r0; r <= r1 + 0.01; r += (r1 - r0) / 9) B.beam('wood', P(-0.35, r, 0.06), P(x1, r, 0.06), 0.07);
    // Canvas spread over most of the frame.
    const g = new THREE.PlaneGeometry(x1 - x0, r1 - r0 - 0.3);
    g.translate((x0 + x1) / 2, (r0 + r1) / 2, 0.1);
    g.rotateZ(a);
    B.add('cloth', g);
  }
  B.put('wood', new THREE.CylinderGeometry(0.4, 0.45, 0.8, 8), 0, 0, 0, Math.PI / 2, 0, 0);
  return B.mergeAll();
}
