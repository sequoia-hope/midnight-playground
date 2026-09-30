// Level 4 — "Desert Run". Golden hour to moonrise: a twisting two-lane
// through a red sandstone canyon, out onto the open highway across the
// basin as the sun goes down behind the far range, and a flat-out finish
// across a dry lake bed after dark.
//
// Same segment format as Levels 1 and 2: [length m, turn deg, rise m, extras].

const SEGMENTS = [
  // ── RED ROCK CANYON ────────────────────────────────────────────
  [180, 0, -1, { zone: 0, road: 'coastal', tag: 'start' }],
  [140, -26, -2],
  [120, 34, -3],
  [110, -40, -3, { tag: 'narrows' }],
  [100, 48, -2, { tag: 'narrows' }],
  [90, -52, -3],
  [130, 22, -4],
  [160, 0, -4, { tag: 'arch' }],
  [120, -44, -5],
  [100, 60, -4],
  [140, -30, -5, { tag: 'hoodoos' }],
  [180, 18, -6, { tag: 'hoodoos' }],
  [110, -64, -4, { tag: 'narrows' }],
  [95, 70, -3, { tag: 'narrows' }],
  [150, 0, -5, { tag: 'fin' }],
  [140, -36, -5],
  [130, 28, -6],
  [170, -12, -7, { tag: 'mouth' }],
  [220, 8, -6, { tag: 'mouth' }],

  // ── ROUTE 66: the open highway ─────────────────────────────────
  [260, 16, -4, { zone: 1, road: 'desert', tag: 'flats' }],
  [380, 0, -3],
  // Washes: the road drops into each one and crests the far bank, which
  // can put a fast car in the air.
  [35, 0, -2.4, { tag: 'dip' }], [35, 0, 2.9], [35, 0, -2.5],
  [300, 0, -1, { tag: 'oasis' }],
  [35, 0, -2.5, { tag: 'dip' }], [35, 0, 3.0], [35, 0, -2.6],
  [260, -14, 0],
  [340, 0, -2, { tag: 'rail' }],
  [35, 0, -2.5, { tag: 'dip' }], [35, 0, 3.1], [35, 0, -2.6],
  [110, 0, 0],
  [35, 0, -2.4, { tag: 'dip' }], [35, 0, 2.9], [35, 0, -2.5],
  [300, 12, -1],
  [220, 0, -2],

  // ── SILVER LAKE: the dry lake bed ──────────────────────────────
  [300, -8, -2, { zone: 2, road: 'playa', tag: 'lakeshore' }],
  [1900, 0, 0, { tag: 'lakebed' }],
  [450, 0, 0, { tag: 'finish' }],
];

// Golden hour in the canyon, sunset down the highway, moonlight on the lake.
const SKY = [
  { s: 0.00, sunEl: 12, zen: 0x3563a8, hor: 0xe9c79a, sun: 0xffd29a, sunI: 3.3, hemiS: 0xbccbe4, hemiG: 0x8a6448, hemiI: 1.35, fog: 0xd8b894, fogD: 0.00018, exp: 1.0, night: 0 },
  { s: 0.22, sunEl: 7, zen: 0x3a5c9e, hor: 0xf4b77c, sun: 0xffb866, sunI: 3.1, hemiS: 0xb4b8d4, hemiG: 0x7a5440, hemiI: 1.15, fog: 0xe0ac7c, fogD: 0.0002, exp: 1.02, night: 0 },
  { s: 0.40, sunEl: 2.5, zen: 0x33508e, hor: 0xff9a58, sun: 0xff8a40, sunI: 2.6, hemiS: 0x9a96bc, hemiG: 0x62463c, hemiI: 0.9, fog: 0xd88a62, fogD: 0.00022, exp: 1.04, night: 0.05 },
  { s: 0.55, sunEl: -0.6, zen: 0x283c78, hor: 0xf2765a, sun: 0xff6a3a, sunI: 1.3, hemiS: 0x7c76a8, hemiG: 0x483644, hemiI: 0.7, fog: 0xa86470, fogD: 0.00024, exp: 1.08, night: 0.25 },
  { s: 0.68, sunEl: -4, zen: 0x18204e, hor: 0xa45474, sun: 0xd8704a, sunI: 0.5, hemiS: 0x505a8c, hemiG: 0x2c2434, hemiI: 0.5, fog: 0x5c4466, fogD: 0.00026, exp: 1.12, night: 0.55 },
  { s: 0.84, sunEl: -9, zen: 0x0a1030, hor: 0x40385e, sun: 0x9ab0e0, sunI: 0.4, hemiS: 0x44508a, hemiG: 0x24222e, hemiI: 0.42, fog: 0x2a2a46, fogD: 0.00024, exp: 1.16, night: 0.85 },
  { s: 1.00, sunEl: -14, zen: 0x050a20, hor: 0x2a2a48, sun: 0xa8bce8, sunI: 0.42, hemiS: 0x3e4a7c, hemiG: 0x22222c, hemiI: 0.42, fog: 0x1e2034, fogD: 0.00022, exp: 1.2, night: 1 },
];

export default {
  id: 'desert',
  mode: 'race',
  num: 'LEVEL 4',
  title: 'Desert Run',
  desc: 'Wind down a red rock canyon at golden hour, race the sunset and a freight train down the old highway, then go flat out across a dry lake bed under the moon.',
  startHeight: 420,
  startHeading: 0,
  finishRunoff: 450,
  segments: SEGMENTS,
  zones: [
    { key: 'canyon', name: 'RED ROCK CANYON', sub: 'Golden hour in the sandstone', landform: 'canyon', scenery: 'Desert', color: '#c8643c' },
    { key: 'desert', name: 'ROUTE 66', sub: 'Race the sunset', landform: 'desert', scenery: 'Desert', color: '#e0a652', blend: 380 },
    { key: 'playa', name: 'SILVER LAKE', sub: 'Flat out on the dry lake', landform: 'playa', scenery: 'Desert', color: '#b9c6d6', blend: 420 },
  ],
  sky: SKY,
  sunAzimuth: 0.12,
  moonDir: [0.85, 0.3, -0.35], // rising low ahead over the lake bed
  traffic: [
    { gap: [320, 620], mix: [['sedan', 0.35], ['pickup', 0.35], ['van', 0.15], ['hatch', 0.15]], oncoming: 0.8, speed: [13, 18] },
    { gap: [220, 420], mix: [['pickup', 0.35], ['sedan', 0.25], ['boxtruck', 0.25], ['van', 0.15]], oncoming: 0.7, speed: [20, 27] },
    // Nobody else is out on the lake bed.
    { gap: [300, 500], mix: [], oncoming: 0, speed: [0, 0] },
  ],
  // Hot Pursuit: the canyon hides you (heat 2); Route 66 and the dry lake
  // are open ground, where they can always see you (4 / 5).
  police: { heatCap: [2, 4, 5], losOpenGround: [false, true, true] },
  rivals: [
    { name: 'Razor', kind: 'super', color: 0xe9ecef, skill: 0.99, power: 530 },
    { name: 'Marlowe', kind: 'electric', color: 0x1ab8c4, skill: 0.978, power: 520 },
    { name: 'Kaito', kind: 'sports', color: 0x19c46b, skill: 0.968, power: 505 },
    { name: 'Rook', kind: 'rally', color: 0x2a6ee8, skill: 0.958, power: 530 },
    { name: 'Duke', kind: 'muscle', color: 0x30343b, skill: 0.94, power: 535 },
  ],
};
