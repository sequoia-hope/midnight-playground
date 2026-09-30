// Level 5 — "Seaside Raceway". Three laps of a real circuit: Laguna Seca,
// in the hills above Monterey Bay, rebuilt from survey data (see
// tools/seaside/build.py). The racing line follows OpenStreetMap, the road
// surface, its camber and the hills round it come from USGS lidar, the
// barriers stand where the real walls do, and the ground takes its colour,
// and the oaks their places, from aerial photos.
//
// It runs anticlockwise, 3.6 km a lap: over the crest at Turn 1, down into
// the hairpin, through Three to Six and up the long climb to the Corkscrew,
// a blind left-right that drops five storeys, then down through the Rainey
// curve and Ten to the last hairpin and the line.
//
// The data lives in ./seaside/ and is only loaded when the level is built
// (prepare()), so the menu doesn't pay for it.

// Sectors, by metres into the lap (the lap starts on the line).
const SECTORS = [0, 1180, 2480];

// Catmull-Rom through the 2 m survey points, 4 per span, so the 1 m
// resample in Track has no corners to find.
function spline(src, n, step, sub = 4) {
  const out = new Float64Array(n * sub);
  for (let i = 0; i < n; i++) {
    const p0 = src[(i - 1 + n) % n], p1 = src[i], p2 = src[(i + 1) % n], p3 = src[(i + 2) % n];
    for (let k = 0; k < sub; k++) {
      const t = k / sub, t2 = t * t, t3 = t2 * t;
      out[i * sub + k] = 0.5 * (2 * p1 + (-p0 + p2) * t + (2 * p0 - 5 * p1 + 4 * p2 - p3) * t2 + (-p0 + 3 * p1 - 3 * p2 + p3) * t3);
    }
  }
  return out;
}

const level = {
  id: 'seaside',
  mode: 'race',
  num: 'LEVEL 5',
  title: 'Seaside Raceway',
  desc: 'Three laps of a real circuit in the golden hills above the bay, rebuilt from lidar survey: over the crest into the hairpin, up the long climb and over the blind drop of the Corkscrew.',
  laps: 3,
  // Metres round the lap (the survey's centreline; the menu shows it).
  lapLength: 3593,
  // Survey data: filled in by prepare().
  data: null,
  async prepare() {
    if (this.data) return;
    const { load } = await import('./seaside/load.js');
    this.data = await load();
  },
  loop: {
    road: 'circuit',
    startS: 0,
    // Real heights: only the lightest smoothing on top of the survey's own.
    elevationSmooth: 1.5,
    path() {
      const d = level.data;
      if (!d) throw new Error('Seaside Raceway: prepare() first');
      const L = d.line, n = L.n;
      return {
        x: spline(L.x, n), z: spline(L.z, n), y: spline(L.y, n),
        bank: spline(L.bank, n), wallL: spline(L.wallL, n), wallR: spline(L.wallR, n),
        runL: spline(L.runL, n), runR: spline(L.runR, n),
      };
    },
    zones: SECTORS,
    // Named places round the lap (metres from the line).
    tags: [
      { tag: 'pit-straight', s0: 3380, s1: 3593 },
      { tag: 'crest', s0: 200, s1: 330 },
      { tag: 'hairpin', s0: 500, s1: 680 },
      { tag: 'climb', s0: 2040, s1: 2480 },
      { tag: 'corkscrew', s0: 2500, s1: 2640 },
      { tag: 'hairpin', s0: 3290, s1: 3420 },
    ],
    // The pit straight is wider than the rest of the lap.
    roads: [{ s0: 3380, s1: 3593 + 60, road: 'circuitWide' }],
  },
  // The level owns the ground: the lidar surface round the circuit.
  ground(x, z) { return level.data.height(x, z); },
  groundColor(x, z, out) { return level.data.color(x, z, out); },
  zones: [
    { key: 'hairpin', name: 'THE HAIRPIN', sub: 'Over the crest and down to the hairpin', landform: 'raceway', scenery: 'Raceway', color: '#d6a24a' },
    { key: 'climb', name: 'THE CLIMB', sub: 'Up through Five and Six', landform: 'raceway', scenery: 'Raceway', color: '#b9853c' },
    { key: 'corkscrew', name: 'THE CORKSCREW', sub: 'Over the top and five storeys down', landform: 'raceway', scenery: 'Raceway', color: '#e05a3a' },
  ],
  // Late afternoon: the sun low over the bay to the west-south-west, dry
  // gold hills and a little haze. The same all race (a loop reads the
  // middle of the keys).
  sky: [
    { s: 0, sunEl: 15, zen: 0x3a6cb8, hor: 0xe6d6b8, sun: 0xffddb0, sunI: 3.3, hemiS: 0xc4d2ea, hemiG: 0x8e7a58, hemiI: 1.45, fog: 0xd8cbb2, fogD: 0.00019, exp: 1.0, night: 0 },
    { s: 1, sunEl: 15, zen: 0x3a6cb8, hor: 0xe6d6b8, sun: 0xffddb0, sunI: 3.3, hemiS: 0xc4d2ea, hemiG: 0x8e7a58, hemiI: 1.45, fog: 0xd8cbb2, fogD: 0.00019, exp: 1.0, night: 0 },
  ],
  sunAzimuth: 2.79, // west-south-west (compass 250°)
  // A closed circuit: no traffic.
  traffic: [
    { gap: [300, 500], mix: [], oncoming: 0, speed: [0, 0] },
    { gap: [300, 500], mix: [], oncoming: 0, speed: [0, 0] },
    { gap: [300, 500], mix: [], oncoming: 0, speed: [0, 0] },
  ],
  rivals: [
    { name: 'Razor', kind: 'super', color: 0xe9ecef, skill: 0.985, power: 525 },
    { name: 'Kaito', kind: 'sports', color: 0x19c46b, skill: 0.972, power: 505 },
    { name: 'Marlowe', kind: 'electric', color: 0x1ab8c4, skill: 0.962, power: 515 },
    { name: 'Rook', kind: 'rally', color: 0x2a6ee8, skill: 0.95, power: 520 },
    { name: 'Vex', kind: 'muscle', color: 0x8b2cff, skill: 0.94, power: 540 },
  ],
};

export default level;
