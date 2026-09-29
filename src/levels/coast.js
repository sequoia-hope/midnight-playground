// Level 2 — "Coast Highway". A dawn run with the sea on your left the whole
// way: a cliff road carved above the ocean, down into the beach town of
// Seabright as the sun comes up over the water, then over the harbour bridge
// into Port Meridian's container docks.
//
// Same segment format as Level 1: [length m, turn deg, rise m, extras].

const SEGMENTS = [
  // ── HOLLOW POINT: cliff road ───────────────────────────────────
  [180, 0, 1, { zone: 0, road: 'coastal', tag: 'start' }],
  [160, -24, 3],
  [140, 38, 4],
  [120, -46, -2, { tag: 'cove' }],
  [110, 52, 2],
  [160, 0, 5, { tag: 'lighthouse' }],
  [140, -30, 6],
  [95, 72, 3],
  [130, -58, 2],
  [150, 34, -3, { tag: 'arch' }],
  [100, -40, 3],
  [180, 22, 6, { tag: 'bluff' }],
  [160, -30, 4],
  [140, 45, -4],
  [120, -52, -2, { tag: 'cove' }],
  [160, 26, -8, { tag: 'descent' }],
  [180, -20, -14],
  [160, 34, -16],
  [200, -16, -18],

  // ── SEABRIGHT: beach town boulevard ────────────────────────────
  [220, 20, -20, { zone: 1, road: 'boulevard', tag: 'town-entry' }],
  [200, -18, -20],
  [160, 0, -9, { tag: 'promenade' }],
  [260, 8, -1, { tag: 'pier' }],
  [200, -14, 0],
  [240, 0, 0, { tag: 'boardwalk' }],
  [180, 20, 0],
  [220, -10, 0, { tag: 'marina' }],
  [200, 0, 1],

  // ── PORT MERIDIAN: harbour bridge and container docks ──────────
  [200, 18, 0, { zone: 2, road: 'freeway', tag: 'approach' }],
  [420, 0, 38, { tag: 'bridge-up', elevated: true }],
  [900, 0, 0, { tag: 'bridge', elevated: true }],
  [420, 0, -40, { tag: 'bridge-down', elevated: true }],
  [260, -30, 0, { tag: 'port' }],
  [300, 26, 0, { tag: 'containers' }],
  [260, -14, 0],
  [240, 0, 0, { tag: 'finish' }],
];

// Dawn: blue hour on the cliffs, sunrise over the sea at the beach town,
// golden morning on the bridge. The blue-hour keys carry a strong sky fill
// (hemisphere light) so the cliffs read before the sun is up.
const SKY = [
  { s: 0.00, sunEl: -9, zen: 0x0c1636, hor: 0x3e4c80, sun: 0xa4b8e6, sunI: 0.55, hemiS: 0x5a6ca8, hemiG: 0x2e2e40, hemiI: 1.0, fog: 0x34406a, fogD: 0.0003, exp: 1.22, night: 0.75 },
  { s: 0.18, sunEl: -5, zen: 0x182652, hor: 0x76648e, sun: 0xb8a8d8, sunI: 0.55, hemiS: 0x6c76ac, hemiG: 0x36303e, hemiI: 1.05, fog: 0x55547c, fogD: 0.0003, exp: 1.16, night: 0.55 },
  { s: 0.32, sunEl: -1.5, zen: 0x22346e, hor: 0xe08a6c, sun: 0xff8a58, sunI: 0.9, hemiS: 0x8a88b8, hemiG: 0x4a3c3e, hemiI: 1.0, fog: 0xa27888, fogD: 0.00028, exp: 1.08, night: 0.3 },
  { s: 0.44, sunEl: 2.5, zen: 0x33549a, hor: 0xffb070, sun: 0xff9a50, sunI: 2.2, hemiS: 0xa8b0d0, hemiG: 0x5e4e44, hemiI: 1.0, fog: 0xe0a882, fogD: 0.00026, exp: 1.04, night: 0.05 },
  { s: 0.60, sunEl: 7, zen: 0x3f6cb4, hor: 0xf6cfa0, sun: 0xffc080, sunI: 2.9, hemiS: 0xb8c8e6, hemiG: 0x6e6252, hemiI: 1.25, fog: 0xe6c8a8, fogD: 0.00022, exp: 1.0, night: 0 },
  { s: 0.80, sunEl: 13, zen: 0x3e78c6, hor: 0xcfe0ee, sun: 0xffe8c8, sunI: 3.3, hemiS: 0xc2d6f2, hemiG: 0x857a64, hemiI: 1.45, fog: 0xc2d4e4, fogD: 0.0002, exp: 1.0, night: 0 },
  { s: 1.00, sunEl: 18, zen: 0x3a78cc, hor: 0xc6dcee, sun: 0xfff0d8, sunI: 3.4, hemiS: 0xc2d6f2, hemiG: 0x8a8068, hemiI: 1.55, fog: 0xbfd3e6, fogD: 0.0002, exp: 1.0, night: 0 },
];

export default {
  id: 'coast',
  mode: 'race',
  num: 'LEVEL 2',
  title: 'Coast Highway',
  desc: 'Leave before dawn on the cliff road above the ocean, cruise the beach boulevard of Seabright as the sun comes up, and finish over the harbour bridge in Port Meridian.',
  startHeight: 85,
  startHeading: 0,
  finishRunoff: 180,
  segments: SEGMENTS,
  sea: { y: 0 },
  zones: [
    { key: 'coast', name: 'HOLLOW POINT', sub: 'The cliff road', landform: 'coast', scenery: 'Coast', color: '#9a7f66' },
    { key: 'beach', name: 'SEABRIGHT', sub: 'Beach town at sunrise', landform: 'beach', scenery: 'Beach', color: '#e0b45a', blend: 350 },
    { key: 'harbor', name: 'PORT MERIDIAN', sub: 'Harbour bridge', landform: 'harbor', scenery: 'Harbor', color: '#3f8fc0', blend: 300 },
  ],
  sky: SKY,
  sunAzimuth: -0.45, // rising over the sea, ahead and to the left
  traffic: [
    { gap: [240, 480], mix: [['sedan', 0.35], ['hatch', 0.25], ['van', 0.2], ['pickup', 0.2]], oncoming: 0.75, speed: [13, 18] },
    { gap: [70, 160], mix: [['hatch', 0.3], ['sedan', 0.3], ['van', 0.2], ['pickup', 0.2]], oncoming: 0.5, speed: [12, 17] },
    { gap: [50, 120], mix: [['sedan', 0.3], ['hatch', 0.15], ['van', 0.15], ['pickup', 0.1], ['boxtruck', 0.3]], oncoming: 0, speed: [21, 29], opposite: 0.5 },
  ],
  rivals: [
    { name: 'Razor', kind: 'super', color: 0xe9ecef, skill: 0.995, power: 530 },
    { name: 'Marlowe', kind: 'electric', color: 0x1ab8c4, skill: 0.98, power: 520 },
    { name: 'Kaito', kind: 'sports', color: 0x19c46b, skill: 0.97, power: 505 },
    { name: 'Vex', kind: 'muscle', color: 0x8b2cff, skill: 0.96, power: 545 },
    { name: 'Nina', kind: 'sports', color: 0xff7a1a, skill: 0.95, power: 500 },
  ],
};
