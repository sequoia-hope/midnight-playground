// Writes parity/golden/music/: the Music Lab run offline under Node, for the
// Rust port in crates/mp_music to be checked against (its golden tests).
// Run with `node tools/music-lab/test/golden.mjs`; regenerating must give
// identical files.
//
// Three files:
//   tracks.json   every genre over a few seeds, as a hash of the track's
//                 canonical JSON (sorted keys, JS number formatting), plus
//                 one whole track per genre so a port's difference can be
//                 read, not just detected;
//   events.json   the sequencer's events for a few tracks from the first
//                 bar: lanes, velocities, notes, durations, swing offsets;
//   renders.json  the engine's stereo render of a few tracks from a given
//                 bar: RMS per 2400 samples (50 ms), and whole 256-sample
//                 windows at a few points. Numbers that are f32 are stored
//                 as base64 of their little-endian bytes.
// The grammars and the sequencer are integer and seeded-float arithmetic,
// so a port reproduces the first two exactly; the renders agree to within a
// tolerance (the lab uses Math.*, the port the platform's f64 functions).

import fs from 'node:fs';
import { createHash } from 'node:crypto';
import { setRate } from '../dsp.js';
import { GENRES } from '../gen.js';
import { Seq } from '../seq.js';
import { Engine } from '../engine.js';

const RATE = 48000;
setRate(RATE);
const OUT = new URL('../../../parity/golden/music/', import.meta.url);
const SEEDS = [1, 2, 3, 4, 5, 6];
const ENGINE_SEED = 5;
const RMS = 2400;
const WIN = 256;

// Canonical JSON: keys sorted, `undefined` members dropped (as
// JSON.stringify drops them), numbers as JS prints them (-0 is "0").
export function canon(v) {
  if (v === null) return 'null';
  if (typeof v === 'number') return Number.isFinite(v) ? String(v === 0 ? 0 : v) : 'null';
  if (typeof v === 'boolean') return v ? 'true' : 'false';
  if (typeof v === 'string') return JSON.stringify(v);
  if (Array.isArray(v)) return '[' + v.map((x) => (x === undefined ? 'null' : canon(x))).join(',') + ']';
  const keys = Object.keys(v).filter((k) => v[k] !== undefined).sort();
  return '{' + keys.map((k) => JSON.stringify(k) + ':' + canon(v[k])).join(',') + '}';
}
const sha = (s) => createHash('sha256').update(s).digest('hex');
const b64 = (arr) => Buffer.from(new Float32Array(arr).buffer).toString('base64');

fs.mkdirSync(OUT, { recursive: true });

// ── tracks.json ──────────────────────────────────────────────────
const tracks = { seeds: SEEDS, dejavu: 'default', hashes: {}, sample: {} };
for (const [g, make] of Object.entries(GENRES)) {
  tracks.hashes[g] = {};
  for (const seed of SEEDS) tracks.hashes[g][seed] = sha(canon(make(seed)));
  tracks.sample[g] = canon(make(SEEDS[0]));
}
fs.writeFileSync(new URL('tracks.json', OUT), JSON.stringify(tracks, null, 1) + '\n');

// ── events.json ──────────────────────────────────────────────────
// Each entry: the track, the bar it starts at, and the events of the next
// `steps` 16ths as canonical JSON (one array per step).
const EVENT_RUNS = [['house', 3, 0, 64], ['chicha', 2, 0, 64], ['dnb', 5, 40, 48], ['garage', 1, 16, 32], ['eurobeat', 4, 56, 32]];
const events = [];
for (const [g, seed, bar, steps] of EVENT_RUNS) {
  const T = GENRES[g](seed);
  const s = new Seq(T);
  s.seekBar(bar);
  const out = [];
  for (let i = 0; i < steps; i++) out.push(s.step());
  events.push({ genre: g, seed, bar, steps, events: canon(out) });
}
fs.writeFileSync(new URL('events.json', OUT), JSON.stringify(events, null, 1) + '\n');

// ── renders.json ─────────────────────────────────────────────────
function dropBar(T) {
  let b = 0;
  for (const s of T.sections) { if (s.drop) break; b += s.bars; }
  return b;
}
// [genre, seed, bar ('drop' for the drop's bar), seconds, energy, window starts (s)]
const RENDER_RUNS = [
  ['house', 1, 'drop', 6, 1, [0.5, 1.2, 2.7, 4.4, 5.6]],
  ['house', 1, 'drop', 6, 0.3, [0.5, 1.2, 2.7, 4.4, 5.6]],
  ['techno', 2, 0, 6, 1, [0.5, 1.2, 2.7, 4.4, 5.6]],
  ['techno', 1, 'drop', 6, 1, [0.5, 1.2, 2.7, 4.4, 5.6]],
  ['trance', 1, 'drop', 6, 1, [0.5, 1.2, 2.7, 4.4, 5.6]],
  ['eurobeat', 1, 'drop', 6, 1, [0.5, 1.2, 2.7, 4.4, 5.6]],
  ['psytrance', 1, 'drop', 6, 1, [0.5, 1.2, 2.7, 4.4, 5.6]],
  ['dnb', 1, 'drop', 6, 1, [0.5, 1.2, 2.7, 4.4, 5.6]],
  ['dnb', 2, 'drop', 6, 1, [0.5, 1.2, 2.7, 4.4, 5.6]],
  ['garage', 1, 'drop', 6, 1, [0.5, 1.2, 2.7, 4.4, 5.6]],
  ['chicha', 1, 0, 6, 1, [0.5, 1.2, 2.7, 4.4, 5.6]],
  ['chicha', 1, 'drop', 6, 1, [0.5, 1.2, 2.7, 4.4, 5.6]],
  ['chicha', 3, 'drop', 6, 1, [0.5, 1.2, 2.7, 4.4, 5.6]],
  ['chicha', 3, 56, 6, 1, [0.5, 1.2, 2.7, 4.4, 5.6]],
];
const renders = [];
for (const [g, seed, barSpec, secs, energy, wins] of RENDER_RUNS) {
  const T = GENRES[g](seed);
  const bar = barSpec === 'drop' ? dropBar(T) : barSpec;
  const e = new Engine({ seed: ENGINE_SEED, rate: RATE });
  e.setTrack(T);
  e.setEnergy(energy);
  e.play(bar);
  const n = Math.round((secs * RATE) / 128) * 128;
  const L = new Float32Array(n), R = new Float32Array(n);
  for (let i = 0; i < n; i += 128) e.process(L.subarray(i, i + 128), R.subarray(i, i + 128), 128);
  const rms = (x) => {
    const out = [];
    for (let s = 0; s + RMS <= x.length; s += RMS) {
      let a = 0;
      for (let i = s; i < s + RMS; i++) a += x[i] * x[i];
      out.push(Math.sqrt(a / RMS));
    }
    return b64(out);
  };
  const windows = wins.map((t) => {
    const s = Math.round(t * RATE);
    return { start: s, l: b64(L.subarray(s, s + WIN)), r: b64(R.subarray(s, s + WIN)) };
  });
  renders.push({ genre: g, seed, bar, secs, energy, engineSeed: ENGINE_SEED, rate: RATE, samples: n, rmsL: rms(L), rmsR: rms(R), windows });
  console.log(`${g} ${seed} bar ${bar}: ${n} samples`);
}
fs.writeFileSync(new URL('renders.json', OUT), JSON.stringify(renders, null, 1) + '\n');
console.log('wrote', OUT.pathname);
