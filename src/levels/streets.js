// Level 3 — "Downtown Streets". A midnight street race through Meridian's
// grid: the Neon District's shopping streets, the switchback-free but very
// steep streets of Nob Hill (level at every crossing, so the car flies off
// each one going downhill), and the towers of the Financial District.
//
// The route is laid out on the street grid rather than written as turtle
// segments by hand: a list of moves from crossing to crossing, turned into
// straights and 90° corners whose lengths are chosen so every straight runs
// exactly down the middle of a grid street. The level also owns the ground
// (flat blocks, and a ridge under Nob Hill with the streets stepped across
// it), which the road, the terrain and the scenery all read.

import { clamp, smoothstep, DEG } from '../util/math.js';

// ── The grid ─────────────────────────────────────────────────────
const PX = 130;        // crossing spacing east–west (m)
const PZ = 105;        // and north–south
const HW = 7.0;        // kerb-to-centre (matches roadTypes.street)
const WALK = 4.5;      // pavement width
const FLAT = 22;       // level stretch either side of a crossing on the hill
const EASE = 15;       // rounding where a hill street tips over (m)
const BASE = 20;       // street level away from the hill

// Nob Hill: an east–west ridge. Heights at crossings; streets ramp between.
const RIDGE = { z: 6 * PZ, x0: 12 * PX - 60, x1: 16 * PX + 60, fade: 250, A: 34, sigma: 200 };
function ridge(x, z) {
  const w = smoothstep(RIDGE.x0 - RIDGE.fade, RIDGE.x0, x) * (1 - smoothstep(RIDGE.x1, RIDGE.x1 + RIDGE.fade, x));
  if (w <= 0) return 0;
  const d = (z - RIDGE.z) / RIDGE.sigma;
  return RIDGE.A * Math.exp(-0.5 * d * d) * w;
}
const crossingY = (i, j) => BASE + ridge(i * PX, j * PZ);

// 0 → 1 across a block: level round each crossing, a straight ramp between
// with rounded ends (a trapezoid slope, integrated).
function ramp(t, P) {
  const a = FLAT, b = P - FLAT, L = b - a, m = 1 / (L - EASE);
  if (t <= a) return 0;
  if (t >= b) return 1;
  if (t < a + EASE) return (m * (t - a) ** 2) / (2 * EASE);
  if (t > b - EASE) return 1 - (m * (b - t) ** 2) / (2 * EASE);
  return m * (EASE / 2 + (t - a - EASE));
}

// Ground height anywhere: a bilinear blend of the four surrounding
// crossings, eased so every street is level across and stepped along.
export function ground(x, z) {
  const fi = x / PX, fj = z / PZ;
  const i = Math.floor(fi), j = Math.floor(fj);
  const u = ramp((fi - i) * PX, PX), v = ramp((fj - j) * PZ, PZ);
  const a = crossingY(i, j), b = crossingY(i + 1, j), c = crossingY(i, j + 1), d = crossingY(i + 1, j + 1);
  return (a + (b - a) * u) * (1 - v) + (c + (d - c) * u) * v;
}

// ── The route ────────────────────────────────────────────────────
// Moves from crossing to crossing: [direction, blocks, extras]. Extras on a
// move apply from its start (zone, road, tag); `tags` add named stretches
// by crossing along the move for the scenery.
const START = { i: 0, j: 0, x: 20 };          // the road begins mid-block, heading east
const MOVES = [
  // ── NEON DISTRICT ──
  ['E', 4, { zone: 0, road: 'street', tag: 'start' }],
  ['S', 2],
  ['E', 2, { tag: 'arcade' }],
  ['N', 1],
  ['E', 3, { tag: 'el' }],
  ['S', 2],
  ['E', 3],
  // ── NOB HILL ──
  ['S', 6, { zone: 1, tag: 'hill-down' }],
  ['E', 2],
  ['N', 3, { tag: 'hill-up' }],
  ['E', 2, { tag: 'crest' }],
  ['S', 3, { tag: 'hill-down' }],
  // ── FINANCIAL DISTRICT ──
  ['E', 3, { zone: 2 }],
  ['N', 2],
  ['E', 4, { tag: 'towers' }],
  ['S', 1],
  ['E', 4, { tag: 'finish' }],
];
const CORNER = 48;     // length of a 90° corner (minimum radius ≈ 21 m)
const END_RUN = 45;    // the last move stops this far short of its crossing

const DIRS = { E: [1, 0], S: [0, 1], W: [-1, 0], N: [0, -1] };
const HEAD = { E: 0, S: 90, W: 180, N: -90 };

// The same easing Track.walkSegments uses, so the turtle here lands where
// the real one does.
function trapezoid(t, r) {
  if (t < r) return smoothstep(0, r, t);
  if (t > 1 - r) return smoothstep(0, r, 1 - t);
  return 1;
}
function walk(state, len, turnDeg) {
  const r = Math.abs(turnDeg) > 120 ? 0.22 : 0.32;
  const kPeak = (turnDeg * DEG) / (len * (1 - r));
  for (let k = 0; k < len; k++) {
    const kap = kPeak * trapezoid((k + 0.5) / len, r);
    state.h += kap;
    state.x += Math.cos(state.h - kap * 0.5);
    state.z += Math.sin(state.h - kap * 0.5);
  }
}
// Setback of a corner: how far before the crossing a 90° corner starts.
function cornerSetback(len) {
  const s = { x: 0, z: 0, h: 0 };
  walk(s, len, 90);
  return s.x;
}

function buildRoute() {
  const T = cornerSetback(CORNER);
  const segs = [];
  const turtle = { x: START.x, z: START.j * PZ, h: 0 };
  let ci = START.i, cj = START.j;
  const legs = [];   // straight runs on grid streets, for the scenery
  MOVES.forEach(([dir, blocks, extra], m) => {
    const [dx, dz] = DIRS[dir];
    const ti = ci + dx * blocks, tj = cj + dz * blocks;
    const next = MOVES[m + 1];
    const tx = ti * PX, tz = tj * PZ;
    // Distance left along this street to the target crossing.
    const along = dx ? (tx - turtle.x) * dx : (tz - turtle.z) * dz;
    const stop = next ? T : END_RUN;
    const len = Math.max(1, Math.round(along - stop));
    const x0 = turtle.x, z0 = turtle.z;
    walk(turtle, len, 0);
    legs.push({ dir, x0, z0, x1: turtle.x, z1: turtle.z, line: dx ? cj : ci, axis: dx ? 'z' : 'x' });
    segs.push([len, 0, 0, extra]);
    if (next) {
      let turn = HEAD[next[0]] - HEAD[dir];
      if (turn > 180) turn -= 360;
      if (turn < -180) turn += 360;
      walk(turtle, CORNER, turn);
      segs.push([CORNER, turn, 0, { tag: 'corner', cross: [ti, tj] }]);
      // Snap the heading error away (it is ~1e-4 rad) so straights stay on the grid.
      turtle.h = HEAD[next[0]] * DEG;
    }
    ci = ti; cj = tj;
  });
  return { segs, legs, setback: T };
}

const ROUTE = buildRoute();

// ── Look ─────────────────────────────────────────────────────────
// Midnight all the way; the haze gets a little thicker downtown.
const NIGHT = { sunEl: -16, zen: 0x04050d, hor: 0x2a1a36, sun: 0x9ab0e0, sunI: 0.3, hemiS: 0x2e3456, hemiG: 0x1a1420, hemiI: 0.34, fog: 0x21182c, fogD: 0.00055, exp: 1.22, night: 1 };
const SKY = [
  { s: 0.0, ...NIGHT },
  { s: 0.5, ...NIGHT, hor: 0x241a38, fog: 0x1c1830, fogD: 0.00045 },
  { s: 1.0, ...NIGHT, hor: 0x301c34, fog: 0x241a2c, fogD: 0.0006 },
];

export default {
  id: 'streets',
  mode: 'race',
  num: 'LEVEL 3',
  title: 'Downtown Streets',
  desc: 'A midnight street race through the grid of downtown Meridian: ninety-degree corners under the Neon District signs, flat-out jumps over the crossings of Nob Hill, and a sprint between the towers to the finish.',
  startHeight: BASE,
  startHeading: 0,
  startX: START.x,
  startZ: START.j * PZ,
  finishRunoff: 170,
  segments: ROUTE.segs,
  elevation: ground,
  elevationSmooth: 2,
  ground,
  // The street grid, for the scenery (Streets.js).
  grid: {
    PX, PZ, HW, WALK, FLAT, BASE, crossingY, ground, ridge: RIDGE,
    start: START, legs: ROUTE.legs, setback: ROUTE.setback,
    // Districts by grid column: Neon District west of 12, Nob Hill to 16.
    district: (i) => (i < 12 ? 0 : i <= 16 ? 1 : 2),
  },
  zones: [
    { key: 'neon', name: 'NEON DISTRICT', sub: 'Shopping streets after dark', landform: 'streets', scenery: 'Streets', color: '#e0409a' },
    { key: 'hill', name: 'NOB HILL', sub: 'Level at every crossing', landform: 'streets', scenery: 'Streets', color: '#e0a040', blend: 200 },
    { key: 'fin', name: 'FINANCIAL DISTRICT', sub: 'Sprint through the towers', landform: 'streets', scenery: 'Streets', color: '#40b8e0', blend: 200 },
  ],
  sky: SKY,
  sunAzimuth: 0.9,
  trafficPaint: [0xf2c318, 0xf2c318, 0xf2c318, 0xc9ccd1, 0x2b2f36, 0x8a1c1c, 0x1d3f75, 0xe8e6df, 0x9aa3ad, 0x2e6d8e],
  traffic: [
    { gap: [70, 150], mix: [['sedan', 0.4], ['hatch', 0.25], ['van', 0.2], ['boxtruck', 0.15]], oncoming: 0.5, speed: [10, 14] },
    { gap: [110, 220], mix: [['sedan', 0.45], ['hatch', 0.3], ['van', 0.15], ['pickup', 0.1]], oncoming: 0.5, speed: [8, 12] },
    { gap: [60, 130], mix: [['sedan', 0.45], ['hatch', 0.15], ['van', 0.2], ['boxtruck', 0.2]], oncoming: 0.5, speed: [10, 15] },
  ],
  rivals: [
    { name: 'Razor', kind: 'super', color: 0xe9ecef, skill: 0.985, power: 520 },
    { name: 'Kaito', kind: 'sports', color: 0x19c46b, skill: 0.972, power: 505 },
    { name: 'Vex', kind: 'muscle', color: 0x8b2cff, skill: 0.958, power: 540 },
    { name: 'Juno', kind: 'electric', color: 0x6fe0ff, skill: 0.945, power: 500 },
    { name: 'Rook', kind: 'rally', color: 0x2a6ee8, skill: 0.93, power: 505 },
  ],
};
