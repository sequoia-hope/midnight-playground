// The golden for mp_worldgen::beach::parts (roadmap WP 7.1; Desert uses
// the motel, the gas station and the diner): the game's own
// beach/parts.js under Node with the parity kernel, each building drawn
// into a ColorBuilder with Beach.js's palette, in a placed frame, from a
// seeded stream. crates/mp_worldgen/tests/beach_parts.rs makes the same
// calls and requires the same bits.
//
//   node --import=./tools/parity/kernel/register.mjs tools/parity/beach-parts.mjs [--check]
//
// Per case: what the function returned, the stream's next draw (so the
// number of draws is checked too), and per bucket in order its key, piece
// count and the SHA-256 of each attribute of the merged pieces
// (mergeGeometries, as ColorBuilder.build merges them). Writes
// parity/golden/beach/parts.json.

import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { ROOT } from './lib/jstree.mjs';
import { kernelInstalled } from '../../src/parity/kernel.js';

if (!kernelInstalled()) {
  console.error('beach-parts: run with --import=./tools/parity/kernel/register.mjs');
  process.exit(1);
}

await import('../../test/unit/support/three.js');
const { mergeGeometries } = await import('three/addons/utils/BufferGeometryUtils.js');
const { ColorBuilder } = await import('../../src/world/beach/ColorBuilder.js');
const P = await import('../../src/world/beach/parts.js');
const { mulberry32 } = await import('../../src/util/math.js');

const OUT = path.join(ROOT, 'parity/golden/beach/parts.json');
const check = process.argv.includes('--check');

// Beach.js's PALETTE (module-private there).
const PALETTE = {
  wPink: 0xe9aea8, wMint: 0xa9d8c1, wYellow: 0xf1d690, wBlue: 0x9fc3de, wWhite: 0xefebe1,
  wPeach: 0xf1bf95, wLilac: 0xc6b6de, wTeal: 0x6db8b1, wSand: 0xdcc9a2,
  trim: 0xf6f3ea, concrete: 0xc4bdb0, white: 0xf2f2ee, wood: 0x9c7651, woodDark: 0x5a3f2b,
  roofTar: 0x55524d, roofTile: 0xbf6a4c, black: 0x1b1b1d, boardB: 0xff8a3c, paintRed: 0xc83a34,
  lawn: 0x6f9a45, hedge: 0x3f6a32, sailBlue: 0x2b4f86, hullWhite: 0xf1f2ee,
};

const RECT = { u0: 0.1, u1: 0.35, v0: 0.6, v1: 0.72, aspect: 4 };
const RECT2 = { u0: 0.5, u1: 0.55, v0: 0.2, v1: 0.3, aspect: 0.5 };

const f64 = new Float64Array(1);
const u32 = new Uint32Array(f64.buffer);
const bits = (x) => { f64[0] = x; return u32[1].toString(16).padStart(8, '0') + u32[0].toString(16).padStart(8, '0'); };
const sha = (a) => createHash('sha256').update(new Uint8Array(a.buffer, a.byteOffset, a.byteLength)).digest('hex');

// [name, seed, draw(B, rng) → returned value]
const CASES = [
  ['shop', 1, (B, r) => P.shop(B, r)],
  ['shop-rect-blade', 2, (B, r) => P.shop(B, r, { w: 11.5, d: 14, rect: RECT, blade: RECT2 })],
  ['shop-two-story', 3, (B, r) => P.shop(B, r, { twoStory: true, rect: RECT, blade: RECT2 })],
  ['shop-wall', 4, (B, r) => P.shop(B, r, { twoStory: true, wall: 'wLilac' })],
  ['surfShop', 5, (B, r) => P.surfShop(B, r, RECT)],
  ['diner', 6, (B, r) => P.diner(B, r, RECT)],
  ['tacoStand', 7, (B, r) => P.tacoStand(B, r, RECT)],
  ['tacoStand-plain', 7, (B, r) => P.tacoStand(B, r, null)],
  ['picnicTable', 8, (B) => P.picnicTable(B, 1.5, -2)],
  ['motel', 9, (B, r) => P.motel(B, r, { w: 42, signRect: RECT, neonRect: RECT2 })],
  ['motel-default', 10, (B, r) => P.motel(B, r)],
  ['gasStation', 11, (B, r) => P.gasStation(B, r, { signRect: RECT, priceRect: RECT2 })],
  ['gasStation-plain', 11, (B, r) => P.gasStation(B, r)],
  ...[12, 13, 14, 15, 16, 17].map((s) => [`beachHouse-${s}`, s, (B, r) => P.beachHouse(B, r)]),
  ['beachHouse-opts', 18, (B, r) => P.beachHouse(B, r, { w: 10.5, d: 12, floors: 3 })],
  ['beachHouse-tile', 19, (B, r) => P.beachHouse(B, r, { w: 11, d: 11, floors: 2, tile: true })],
  ['hotel', 20, (B, r) => P.hotel(B, r, { w: 36, rect: RECT })],
  ['hotel-default', 21, (B, r) => P.hotel(B, r)],
  ['bench', 22, (B) => P.bench(B, 0.5, 1, 0.3)],
  ['promLamp', 23, (B) => P.promLamp(B, -1, 2)],
  ['streetLight', 24, (B) => P.streetLight(B, 0, 0, 0, 3.4)],
  ['streetLight-tall', 24, (B) => P.streetLight(B, 1, -2, 0.4, 3.2, 9.5)],
  ['trashCan', 25, (B) => P.trashCan(B, 1.6, 0.2)],
  ['lifeguardTower', 26, (B, r) => P.lifeguardTower(B, r, RECT2)],
  ...[27, 28, 29, 30, 31, 32, 33, 34].map((s) => [`umbrellaSet-${s}`, s, (B, r) => P.umbrellaSet(B, r, 'awnRed')]),
  ['umbrellaSet-open', 35, (B, r) => P.umbrellaSet(B, r, 'awnBlue', true)],
  ['volleyballNet', 36, (B) => P.volleyballNet(B)],
  ['surfboardsInSand', 37, (B, r) => P.surfboardsInSand(B, r, 4)],
  ['sailboat', 38, (B, r) => P.sailboat(B, r)],
  ['sailboat-2', 39, (B, r) => P.sailboat(B, r)],
  ['motorboat', 40, (B, r) => P.motorboat(B, r)],
  ['motorboat-2', 41, (B, r) => P.motorboat(B, r)],
];

function ret(v) {
  if (v === undefined) return null;
  if (typeof v === 'number') return bits(v);
  return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, bits(x)]));
}

const cases = CASES.map(([name, seed, draw]) => {
  const B = new ColorBuilder(PALETTE);
  B.setFrame(12.5, 1.25, -7.75, 0.7);
  const rng = mulberry32(seed);
  const value = ret(draw(B, rng));
  const buckets = [...B.buckets].map(([key, geos]) => {
    const g = mergeGeometries(geos, false);
    return { key, pieces: geos.length, attrs: Object.entries(g.attributes).map(([n, a]) => `${n}=${sha(a.array)}`) };
  });
  return { name, seed, value, next: bits(rng()), buckets };
});

const text = JSON.stringify({ note: 'Generated by tools/parity/beach-parts.mjs. Do not edit.', rect: RECT, rect2: RECT2, cases }, null, 1) + '\n';
if (check) {
  if (!fs.existsSync(OUT) || fs.readFileSync(OUT, 'utf8') !== text) { console.error(`${OUT} would change`); process.exit(1); }
  console.log('beach-parts: the golden is current');
} else {
  fs.mkdirSync(path.dirname(OUT), { recursive: true });
  fs.writeFileSync(OUT, text);
  console.log(`beach-parts: ${cases.length} cases in ${path.relative(ROOT, OUT)}`);
}
