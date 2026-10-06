// What Old Mill Valley (src/world/Valley.js, zone 1 of Sierra) contributes to
// the browser's scene export, small enough to commit, so that mp_worldgen's
// port can be held to it in CI and in wasm (roadmap WP 3.7, the L3 gate for
// zone 1; DECISIONS D331).
//
//   node tools/parity/valley-golden.mjs [--check]
//
// Reads parity/cache/<key>/scenes/sierra.mrscene and its digest
// (node tools/parity/scene-export.mjs --levels sierra) and writes
// parity/golden/valley/sierra.json:
//   - one line per child of the group `valley`, in order: type, name, vertex
//     and index counts, each attribute's SHA-256 in order, the index's, the
//     shadow flags, matrixAutoUpdate, the local matrix as hex f64 bits, the
//     instance count and the SHA-256 of the instance matrices and colours;
//     and the SHA-256 of all the lines;
//   - per child the index of its material in a table of the materials'
//     descriptions (every parameter, uniform and kind option; textures by
//     their sampler and, but for canvas textures, the SHA-256 of their
//     pixels: canvas pixels are WP 3.2's threshold gate);
//   - each canvas texture's 8×8 block means per channel, so the threshold
//     gate also runs without the cache;
//   - the night parameters of the group's materials and the export's night
//     factor.
// Without the cache the committed file is kept. --check fails if the file
// would change.

import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { ROOT, jsTreeKey } from './lib/jstree.mjs';

const check = process.argv.includes('--check');
const OUT = path.join(ROOT, 'parity/golden/valley');
const FILE = path.join(OUT, 'sierra.json');
const sha = (buf) => createHash('sha256').update(buf).digest('hex');
const f64 = new Float64Array(1);
const u32 = new Uint32Array(f64.buffer);
const bits = (x) => { f64[0] = x; return u32[1].toString(16).padStart(8, '0') + u32[0].toString(16).padStart(8, '0'); };

function exported(name) {
  const dir = path.join(ROOT, 'parity/cache', jsTreeKey(), 'scenes');
  const scene = path.join(dir, name + '.mrscene'), dig = path.join(dir, name + '.digest.json');
  if (!fs.existsSync(scene) || !fs.existsSync(dig)) return null;
  const buf = fs.readFileSync(scene);
  const jl = buf.readUInt32LE(12);
  const H = JSON.parse(buf.subarray(16, 16 + jl).toString('utf8'));
  const bin = 16 + jl + ((8 - ((16 + jl) % 8)) % 8);
  return { H, D: JSON.parse(fs.readFileSync(dig, 'utf8')), buf, bin };
}

// As tools/parity/road-plan.mjs and crates/mp_worldgen/tests/valley.rs.
const meshLine = (m) => `${m.vertices} ${m.indices} ${Object.entries(m.attributes).map(([k, v]) => k + '=' + v).join(',')} ${m.index}`;

function materialView({ H, D }, i) {
  const walk = (v) => {
    if (Array.isArray(v)) return v.map(walk);
    if (v && typeof v === 'object') {
      const keys = Object.keys(v);
      if (keys.length === 1 && keys[0] === 'texture' && typeof v.texture === 'number') {
        const t = H.textures[v.texture];
        const desc = walk({ ...t, url: t.url ?? null });
        desc.pixels = null;
        const td = D.textures[v.texture];
        return { texture: desc, sha256: t.source === 'canvas' ? null : td.sha256, channels: td.channels, item_size: H.accessors[t.pixels].item_size };
      }
      return Object.fromEntries(keys.map((k) => [k, walk(v[k])]));
    }
    return v;
  };
  const m = walk(H.materials[i]);
  if (m.shader) m.shader = { vertex: sha(Buffer.from(m.shader.vertex, 'utf8')), fragment: sha(Buffer.from(m.shader.fragment, 'utf8')) };
  return m;
}

// 8×8 block means per channel of a texture's pixels (rows as stored).
function blocks({ H, buf, bin }, t) {
  const T = H.textures[t];
  const a = H.accessors[T.pixels];
  const px = buf.subarray(bin + a.offset, bin + a.offset + a.count * a.item_size);
  const out = [];
  const ch = T.channels;
  for (let by = 0; by < 8; by++) for (let bx = 0; bx < 8; bx++) {
    const x0 = Math.floor((bx * T.width) / 8), x1 = Math.floor(((bx + 1) * T.width) / 8);
    const y0 = Math.floor((by * T.height) / 8), y1 = Math.floor(((by + 1) * T.height) / 8);
    const sum = new Array(ch).fill(0);
    for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) for (let c = 0; c < ch; c++) sum[c] += px[(y * T.width + x) * ch + c];
    const n = (x1 - x0) * (y1 - y0);
    for (let c = 0; c < ch; c++) out.push(Math.round((sum[c] / n) * 100) / 100);
  }
  return out;
}

const ex = exported('sierra');
let out;
if (!ex) {
  console.log('valley-golden: no sierra export in the cache; the committed golden is kept');
  process.exit(0);
}
{
  const { H, D } = ex;
  const g = H.nodes.find((n) => n.name === 'valley');
  const table = [], mats = [], lines = [];
  let vertices = 0;
  for (const c of g.children) {
    const n = H.nodes[c];
    const m = D.meshes[n.mesh];
    vertices += m.vertices;
    const dr = D.drawables.find((d) => d.node === c);
    let k = table.indexOf(n.materials[0]);
    if (k < 0) { table.push(n.materials[0]); k = table.length - 1; }
    mats.push(k);
    lines.push(`${n.type} ${n.name || '-'} ${meshLine(m)} ${n.cast_shadow ? 1 : 0}${n.receive_shadow ? 1 : 0} ${n.matrix_auto_update ? 1 : 0} ${n.matrix.map(bits).join(',')} ${dr.instances ?? '-'} ${dr.instance_matrices ?? '-'} ${dr.instance_colors ?? '-'}`);
  }
  // Canvas textures the group's materials use, in order of first use.
  const canvas = [];
  for (const mi of table) {
    for (const [k, v] of Object.entries(H.materials[mi].params)) {
      if (v && typeof v === 'object' && typeof v.texture === 'number' && H.textures[v.texture].source === 'canvas') {
        canvas.push({ material: table.indexOf(mi), param: k, width: H.textures[v.texture].width, height: H.textures[v.texture].height, blocks: blocks(ex, v.texture) });
      }
    }
  }
  const night = H.night_params
    .filter((p) => table.includes(p.material))
    .map((p) => ({ material: table.indexOf(p.material), prop: p.prop, day: p.day, night: p.night }));
  out = {
    note: 'Generated by tools/parity/valley-golden.mjs from the cached Sierra scene export. Do not edit.',
    level: 'sierra',
    count: lines.length,
    vertices,
    sha256: sha(Buffer.from(lines.join('\n'))),
    lines,
    materials: mats,
    table: table.map((i) => materialView(ex, i)),
    canvas,
    night,
    exportNight: H.environment.night,
  };
  console.log(`valley: ${lines.length} children, ${vertices} vertices, ${table.length} materials, ${canvas.length} canvas texture uses, ${night.length} night parameters`);
}
fs.mkdirSync(OUT, { recursive: true });
const text = JSON.stringify(out, null, 1) + '\n';
if (check) {
  if (!fs.existsSync(FILE) || fs.readFileSync(FILE, 'utf8') !== text) { console.error(`${path.relative(ROOT, FILE)}: would change`); process.exit(1); }
} else fs.writeFileSync(FILE, text);
