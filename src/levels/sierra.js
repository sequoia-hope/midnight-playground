// Level 1 — "Sierra to the City".
//
// The road is laid out like a surveyor would: a turtle walks forward along a
// list of segments, each with a length, a total turn and a total climb. Within
// a segment curvature eases in and out (a smoothed trapezoid), so straights
// flow into arcs the way real highway spirals do instead of kinking.
//
// turn > 0 is a right-hander. Heading 0 points down +X; the right-hand vector
// of a heading (fx, fz) is (-fz, fx).
//
// [length m, turn deg, rise m, extras]
// extras: zone (index), road (type key), tag (named feature for scenery),
//         elevated (true = on a bridge/viaduct; the ground isn't raised to it)

const SEGMENTS = [
  // ── SIERRA PASS ────────────────────────────────────────────────
  [170, 0, 1, { zone: 0, road: 'mountain', tag: 'start' }],
  [150, 22, 6],
  [120, -38, 9],
  [140, 30, 12],
  [90, 0, 8],
  // Swing right to traverse the face, then stack four hairpins up it.
  [110, 72, 9],
  [150, 0, 15, { tag: 'switchbacks' }],
  [88, -180, 6, { tag: 'hairpin' }],
  [170, 0, 17],
  [88, 180, 6, { tag: 'hairpin' }],
  [170, 0, 17],
  [88, -180, 6, { tag: 'hairpin' }],
  [150, 8, 14],
  [120, 82, 9],
  // Narrow canyon: rocky S-curves between walls.
  [90, -38, 8, { tag: 'canyon' }],
  [85, 46, 7],
  [80, -52, 6],
  [100, 34, 7],
  [110, -22, 5],
  // The summit and its lookout.
  [160, 0, 2, { tag: 'summit' }],
  [110, 18, -3],
  // Descent toward the valley, sweeping bends with long views.
  [160, -46, -15, { tag: 'descent' }],
  [140, 62, -15],
  [120, -34, -13],
  [150, 52, -16],
  [140, -62, -15],
  [170, 24, -17],
  [160, -26, -15],
  [200, 12, -16],

  // ── OLD MILL VALLEY ────────────────────────────────────────────
  [200, 34, -10, { zone: 1, road: 'valley', tag: 'valley-entry' }],
  [250, 0, -6, { tag: 'farm' }],
  [50, 0, 3.2, { tag: 'crest' }],
  [50, 0, -3.8],
  [200, -30, -3],
  [150, 45, -2, { tag: 'farm' }],
  [120, -45, -2],
  [220, 0, -3, { tag: 'bridge' }],
  [60, 0, 3.8, { tag: 'crest' }],
  [60, 0, -4.4],
  [180, 66, -3, { tag: 'farm' }],
  [260, 0, -4, { tag: 'fields' }],
  [140, -58, -3],
  [50, 0, 3.2, { tag: 'crest' }],
  [50, 0, -3.8],
  [220, 22, -5, { tag: 'farm' }],
  [160, 0, -3],

  // ── INTERSTATE 9 / DOWNTOWN MERIDIAN ───────────────────────────
  [220, 48, -3, { zone: 2, road: 'freeway', tag: 'onramp' }],
  [300, -10, -2, { tag: 'merge' }],
  [400, -22, 0, { tag: 'outskirts' }],
  [300, 26, 10, { tag: 'viaduct-up' }],
  [420, 0, 0, { tag: 'viaduct' }],
  [300, -30, -10, { tag: 'viaduct-down' }],
  [260, 0, 0, { tag: 'tunnel' }],
  [400, 20, 0, { tag: 'downtown' }],
  [300, -16, 0],
  [280, 0, 0, { tag: 'finish' }],
];

// Time of day along the route (s = fraction of the track length).
const SKY = [
  // s is a fraction of the track length.
  { s: 0.00, sunEl: 30, zen: 0x2f63b0, hor: 0xb9d0e6, sun: 0xfff0d6, sunI: 3.4, hemiS: 0xc2d6f2, hemiG: 0x8a8068, hemiI: 1.6, fog: 0xb5cadf, fogD: 0.00024, exp: 1.0, night: 0 },
  { s: 0.18, sunEl: 17, zen: 0x3a64a8, hor: 0xe8cfa6, sun: 0xffdca8, sunI: 3.3, hemiS: 0xc0cfe6, hemiG: 0x857a64, hemiI: 1.45, fog: 0xd2c3a8, fogD: 0.00025, exp: 1.0, night: 0 },
  { s: 0.30, sunEl: 6, zen: 0x3e5a9a, hor: 0xf6b77a, sun: 0xffb46a, sunI: 3.0, hemiS: 0xb0b4d0, hemiG: 0x6a5a4e, hemiI: 1.1, fog: 0xdcae86, fogD: 0.00027, exp: 1.02, night: 0 },
  { s: 0.42, sunEl: 1.2, zen: 0x33467e, hor: 0xff8f52, sun: 0xff7a3a, sunI: 2.2, hemiS: 0x8f8fb8, hemiG: 0x5a4a46, hemiI: 0.85, fog: 0xc78468, fogD: 0.0003, exp: 1.05, night: 0.15 },
  { s: 0.54, sunEl: -3, zen: 0x1c2452, hor: 0xb86068, sun: 0xff6a40, sunI: 0.6, hemiS: 0x6c6a98, hemiG: 0x3e3640, hemiI: 0.6, fog: 0x5e4a66, fogD: 0.00034, exp: 1.1, night: 0.5 },
  { s: 0.66, sunEl: -8, zen: 0x0e1432, hor: 0x3c3a66, sun: 0x9ab0e0, sunI: 0.3, hemiS: 0x3a4270, hemiG: 0x1c1a22, hemiI: 0.3, fog: 0x2a2c48, fogD: 0.00038, exp: 1.15, night: 0.82 },
  { s: 0.80, sunEl: -14, zen: 0x05070f, hor: 0x1e1c30, sun: 0x9ab0e0, sunI: 0.26, hemiS: 0x2a3050, hemiG: 0x151218, hemiI: 0.26, fog: 0x181828, fogD: 0.00042, exp: 1.2, night: 1 },
  { s: 1.00, sunEl: -18, zen: 0x03050c, hor: 0x241c2a, sun: 0x9ab0e0, sunI: 0.26, hemiS: 0x283050, hemiG: 0x151218, hemiI: 0.26, fog: 0x1a1624, fogD: 0.00042, exp: 1.2, night: 1 },
];

export default {
  id: 'sierra',
  mode: 'race',
  num: 'LEVEL 1',
  title: 'Sierra to the City',
  desc: 'Sprint up the switchbacks of Sierra Pass, drop through the old farms of Mill Valley, and finish on the Interstate through downtown Meridian. Sunset to midnight.',
  startHeight: 120,
  startHeading: 0,
  finishRunoff: 180,
  segments: SEGMENTS,
  zones: [
    { key: 'mountain', name: 'SIERRA PASS', sub: 'Climb the switchbacks', landform: 'mountain', scenery: 'Mountain', color: '#b8925f' },
    { key: 'valley', name: 'OLD MILL VALLEY', sub: 'Farm country', landform: 'valley', scenery: 'Valley', color: '#82aa44', blend: 350 },
    { key: 'city', name: 'INTERSTATE 9', sub: 'Downtown Meridian', landform: 'city', scenery: 'City', color: '#5a4acb', blend: 300, blendOffset: 120 },
  ],
  sky: SKY,
  sunAzimuth: 0.2,
  traffic: [
    { gap: [260, 520], mix: [['sedan', 0.4], ['hatch', 0.3], ['pickup', 0.2], ['van', 0.1]], oncoming: 0.85, speed: [12, 16] },
    { gap: [180, 360], mix: [['pickup', 0.3], ['sedan', 0.25], ['hatch', 0.15], ['tractor', 0.15], ['boxtruck', 0.15]], oncoming: 0.65, speed: [16, 21] },
    { gap: [45, 110], mix: [['sedan', 0.34], ['hatch', 0.2], ['van', 0.14], ['pickup', 0.14], ['boxtruck', 0.18]], oncoming: 0, speed: [21, 31], opposite: 0.5 },
  ],
  rivals: [
    { name: 'Razor', kind: 'super', color: 0xe9ecef, skill: 0.99, power: 525 },
    { name: 'Kaito', kind: 'sports', color: 0x19c46b, skill: 0.972, power: 505 },
    { name: 'Vex', kind: 'muscle', color: 0x8b2cff, skill: 0.958, power: 540 },
    { name: 'Nina', kind: 'rally', color: 0xff7a1a, skill: 0.945, power: 495 },
    { name: 'Duke', kind: 'muscle', color: 0x30343b, skill: 0.93, power: 530 },
  ],
};
