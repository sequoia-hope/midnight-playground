// Music Lab: prints the evaluated instrument and sequencer tables as JSON
// (BPATCH, KITS, SAMPLE_TWEAK, FILLS, DRUM_LANES, with the SHA-256 of the
// canonical BPATCH and KITS), for the Rust port's generated tables
// (crates/mp_music/src/patches.rs, kits.rs). Run: node tools/music-lab/test/dump-tables.mjs
import { BPATCH, KITS, SAMPLE_TWEAK } from '../instruments.js';
import { FILLS, DRUM_LANES } from '../seq.js';
import { createHash } from 'node:crypto';
function canon(v) {
  if (v === null) return 'null';
  if (typeof v === 'number') return Number.isFinite(v) ? String(v === 0 ? 0 : v) : 'null';
  if (typeof v === 'boolean') return v ? 'true' : 'false';
  if (typeof v === 'string') return JSON.stringify(v);
  if (Array.isArray(v)) return '[' + v.map((x) => (x === undefined ? 'null' : canon(x))).join(',') + ']';
  const keys = Object.keys(v).filter((k) => v[k] !== undefined).sort();
  return '{' + keys.map((k) => JSON.stringify(k) + ':' + canon(v[k])).join(',') + '}';
}
const fills = {}; for (const [k, f] of Object.entries(FILLS)) fills[k] = { from: f.from, ramp: f.ramp, lanes: f.lanes };
console.log(JSON.stringify({ BPATCH, KITS, SAMPLE_TWEAK, FILLS: fills, DRUM_LANES, sha: { BPATCH: createHash('sha256').update(canon(BPATCH)).digest('hex'), KITS: createHash('sha256').update(canon(KITS)).digest('hex') } }));
