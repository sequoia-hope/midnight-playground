// Look inside a trace file (parity/trace-format.md):
//   node tools/parity/trace-inspect.mjs <file[.gz]>             summary
//   node tools/parity/trace-inspect.mjs <file> --tick N         every field of the full record at tick N
//   node tools/parity/trace-inspect.mjs <a> --diff <b>          first differing tick, and the fields that differ
//                                                             in the nearest full records at or after it
import fs from 'node:fs';
import zlib from 'node:zlib';
import { readTrace, decodeRecord, flatten, hashHex } from './lib/trace.mjs';

const load = (f) => { let b = fs.readFileSync(f); if (f.endsWith('.gz')) b = zlib.gunzipSync(b); return readTrace(new Uint8Array(b)); };
const [file, ...args] = process.argv.slice(2);
const A = load(file);
const same = (x, y) => Object.is(x, y);

if (args[0] === '--tick') {
  const n = Number(args[1]);
  const rec = A.full.find(([t]) => t === n) ?? A.full.find(([t]) => t >= n);
  if (!rec) throw new Error('no full record at or after tick ' + n);
  console.log(`tick ${rec[0]} (${rec[1].length} bytes)`);
  for (const [k, v] of flatten(decodeRecord(rec[1]))) console.log(k.padEnd(40), v);
} else if (args[0] === '--diff') {
  const B = load(args[1]);
  const n = Math.min(A.hashes.length, B.hashes.length);
  let first = -1;
  for (let i = 0; i < n; i++) if (A.hashes[i][0] !== B.hashes[i][0] || A.hashes[i][1] !== B.hashes[i][1]) { first = i + 1; break; }
  if (first < 0) { console.log(`no difference in ${n} common ticks (${A.hashes.length} vs ${B.hashes.length})`); process.exit(A.hashes.length === B.hashes.length ? 0 : 1); }
  console.log(`first differing tick: ${first}`);
  const fa = A.full.find(([t]) => t >= first), fb = B.full.find(([t]) => t === fa?.[0]);
  if (fa && fb) {
    const xa = flatten(decodeRecord(fa[1])), xb = new Map(flatten(decodeRecord(fb[1])));
    console.log(`fields that differ at tick ${fa[0]} (the first full record at or after it):`);
    let shown = 0;
    for (const [k, v] of xa) if (!same(v, xb.get(k)) && shown++ < 40) console.log('  ' + k.padEnd(40), v, '→', xb.get(k));
  }
  process.exit(1);
} else {
  console.log(JSON.stringify(A.meta));
  console.log(`version ${A.version}, ${A.hashes.length} ticks, full record every ${A.interval} (${A.full.length} kept), last hash ${hashHex(A.hashes.at(-1))}`);
  const last = decodeRecord(A.full.at(-1)[1]);
  const p = last.players[0];
  if (p) console.log(`last tick ${last.tick}: player s ${p.s.toFixed(1)} lat ${p.lat.toFixed(2)} speed ${p.speed.toFixed(1)} gear ${p.gear} nitro ${p.nitro.toFixed(2)}`);
  console.log(`rivals ${last.rivals.length}, traffic ${last.traffic.length} (${last.traffic.filter((c) => c.active).length} active)${last.pursuit ? `, pursuit ${last.pursuit.state} heat ${last.pursuit.heat}, police active ${last.pursuit.police.filter((u) => u.active).length}` : ''}, streams ${JSON.stringify(last.streams)}`);
}
