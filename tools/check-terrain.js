// Verifies the terrain never pokes up through the road: samples the paved
// surface across its width every few metres and reports where the ground
// is above the asphalt. Also times the field build.
// (Scenery flatten/carve modifiers aren't applied here — they need three.js.)
import { Track } from '../src/track/Track.js';
import { levelById } from '../src/levels/index.js';

const level = levelById(process.argv.find((a) => !a.startsWith('-') && !a.includes('/') && a !== 'shoulder') || 'sierra');
import { Terrain } from '../src/world/Terrain.js';

const t = new Track(level);
const T = new Terrain(t, level);
let t0 = performance.now();
T.buildFields();
T.resolveFlattens();
console.log(`fields built in ${(performance.now() - t0).toFixed(0)} ms, near tiles: ${T.nearTiles.size}`);
const bad = [];
const f = {};
for (let s = 0; s < t.length; s += 3) {
  t.frame(s, f);
  const elevated = T.isElevated(t.idx(s));
  for (const u of [-1, -0.66, -0.33, 0, 0.33, 0.66, 1]) {
    const lat = u * (process.argv[2] === "shoulder" ? f.hw + 0.6 : f.hw);
    const x = f.x + f.rx * lat, z = f.z + f.rz * lat;
    const road = f.y - lat * f.bank;
    const g = T.heightAt(x, z);
    if (g > road + 0.03 && !elevated) bad.push({ s, lat: lat.toFixed(1), over: (g - road).toFixed(2), zone: f.zone });
  }
}
console.log(`${bad.length} samples where terrain is above the road`);
const byS = new Map();
for (const b of bad) { const k = Math.round(b.s / 50) * 50; if (!byS.has(k) || byS.get(k).over < b.over) byS.set(k, b); }
for (const b of [...byS.values()].slice(0, 25)) console.log(`  s=${b.s} lat=${b.lat} +${b.over} m (zone ${b.zone})`);
