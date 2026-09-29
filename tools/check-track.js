// Sanity-checks a level's route (src/levels/<id>.js; pass the id): length per zone, height
// range, tightest corners, and any place where two distant parts of the road
// come close enough to overlap. `--dump file.json` writes the samples out for
// plotting.
import { Track } from '../src/track/Track.js';
import { levelById } from '../src/levels/index.js';

const level = levelById(process.argv.find((a) => !a.startsWith('-') && !a.includes('/') && a !== 'shoulder') || 'sierra');
import { writeFileSync } from 'node:fs';

const t = new Track(level);
const fmt = (v) => v.toFixed(1);
console.log(`${level.id}: length ${t.length} m, finish at ${t.finishS}${t.loop ? ' (loop)' : ''}`);
for (const z of t.zones) {
  let lo = Infinity, hi = -Infinity;
  for (let i = z.s0; i < z.s1; i++) { lo = Math.min(lo, t.py[i]); hi = Math.max(hi, t.py[i]); }
  console.log(`  ${z.key.padEnd(9)} s ${z.s0}–${z.s1} (${z.s1 - z.s0} m)  y ${fmt(lo)}–${fmt(hi)}`);
}
let minR = Infinity, at = 0;
let maxK = 0; for (let i = 0; i < t.n; i++) maxK = Math.max(maxK, Math.abs(t.kSmooth[i]));
console.log(`tightest smoothed radius ${(1 / maxK).toFixed(0)} m`);
for (let i = 0; i < t.n; i++) { const r = 1 / Math.abs(t.kappa[i] || 1e-9); if (r < minR) { minR = r; at = i; } }
console.log(`tightest radius ${fmt(minR)} m at s=${at}`);
let maxG = 0; for (let i = 0; i < t.n; i++) maxG = Math.max(maxG, Math.abs(t.grade[i]));
console.log(`steepest grade ${(maxG * 100).toFixed(1)}%`);
console.log(`bounds x ${fmt(t.bounds.minX)}..${fmt(t.bounds.maxX)}  z ${fmt(t.bounds.minZ)}..${fmt(t.bounds.maxZ)}`);

// Near-misses between non-adjacent parts of the route.
const step = 4, gap = 300, worst = [];
const far = (i, j) => (t.loop ? Math.min(j - i, t.n - (j - i)) : j - i) >= gap;
for (let i = 0; i < t.n; i += step) {
  for (let j = i + gap; j < t.n; j += step) {
    if (!far(i, j)) continue;
    const d = Math.hypot(t.px[i] - t.px[j], t.pz[i] - t.pz[j]);
    const need = t.hw[i] + t.hw[j] + 18;
    if (d < need) worst.push({ i, j, d, dy: t.py[j] - t.py[i] });
  }
}
worst.sort((a, b) => a.d - b.d);
const seen = new Set();
for (const w of worst) {
  const key = Math.round(w.i / 100) + ':' + Math.round(w.j / 100);
  if (seen.has(key)) continue;
  seen.add(key);
  console.log(`  CLOSE: s=${w.i} and s=${w.j}  dist ${fmt(w.d)} m  dy ${fmt(w.dy)} m`);
}
if (!worst.length) console.log('no overlaps');

const dumpAt = process.argv.indexOf('--dump');
if (dumpAt > 0) {
  const pick = (a) => Array.from(a).filter((_, i) => i % 2 === 0);
  writeFileSync(process.argv[dumpAt + 1], JSON.stringify({
    x: pick(t.px), y: pick(t.py), z: pick(t.pz), zone: pick(t.zone), hw: pick(t.hw),
    line: pick(t.racingLine), v: pick(t.speedProfile), tags: t.tags,
  }));
}
