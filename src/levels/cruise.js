// Night City Cruise — an endless loop of freeway through Meridian at night.
//
// The loop is a closed polar curve: a stretched circle (~13 km around) with
// a few layers of gentle waviness, so it keeps turning in long, slow sweeps
// without ever crossing itself. Viaducts and tunnels are placed by fraction
// of the loop.

function loopPath() {
  const R0 = 1900, N = 6000;
  const terms = [
    [2, 0.14, 0.3], [3, 0.05, 1.1], [5, 0.03, 2.0], [7, 0.02, 0.4],
    [11, 0.013, 2.6], [13, 0.019, 1.7], [17, 0.011, 0.9],
  ];
  const x = [], z = [];
  for (let i = 0; i < N; i++) {
    const th = (i / N) * Math.PI * 2;
    let r = 1;
    for (const [k, a, ph] of terms) r += a * Math.cos(k * th + ph);
    x.push(Math.cos(th) * r * R0 * 1.22);
    z.push(Math.sin(th) * r * R0);
  }
  return { x, z };
}

export default {
  id: 'cruise',
  mode: 'cruise',
  num: 'ENDLESS',
  title: 'Night City Cruise',
  desc: 'An endless loop of freeway through Meridian at midnight. No finish line: weave through traffic, chain near misses for a multiplier and keep the speed up.',
  loop: {
    path: loopPath,
    baseY: 20,
    // Fractions of the loop. elevated: metres above the street.
    tags: [
      { tag: 'viaduct', f0: 0.1, f1: 0.19, elevated: 10 },
      { tag: 'downtown', f0: 0.2, f1: 0.32 },
      { tag: 'tunnel', f0: 0.34, f1: 0.365 },
      { tag: 'viaduct', f0: 0.52, f1: 0.61, elevated: 11 },
      { tag: 'downtown', f0: 0.64, f1: 0.74 },
      { tag: 'tunnel', f0: 0.8, f1: 0.82 },
    ],
  },
  zones: [
    { key: 'city', name: 'MERIDIAN LOOP', sub: 'Endless night cruise', landform: 'city', scenery: 'City', color: '#5a4acb' },
  ],
  sky: [
    { s: 0, sunEl: -18, zen: 0x03050c, hor: 0x2a1e30, sun: 0x9ab0e0, sunI: 0.28, hemiS: 0x2c3252, hemiG: 0x16131a, hemiI: 0.3, fog: 0x1c1826, fogD: 0.00038, exp: 1.22, night: 1 },
    { s: 1, sunEl: -18, zen: 0x03050c, hor: 0x2a1e30, sun: 0x9ab0e0, sunI: 0.28, hemiS: 0x2c3252, hemiG: 0x16131a, hemiI: 0.3, fog: 0x1c1826, fogD: 0.00038, exp: 1.22, night: 1 },
  ],
  sunAzimuth: 0.2,
  traffic: [
    { gap: [30, 75], mix: [['sedan', 0.34], ['hatch', 0.2], ['van', 0.14], ['pickup', 0.12], ['boxtruck', 0.2]], oncoming: 0, speed: [21, 32], opposite: 0.8 },
  ],
  rivals: [],
};
