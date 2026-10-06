// The pursuit's props of the browser's models export (roadmap WP 8.2):
// PursuitView.js's sawhorse (its striped canvas board) and a spike strip
// laid on Sierra's road at startS + 200 from lat -4 to 4, the children of
// the export's group `pursuit-props` (tools/parity/lib/scene-page.js
// exportModels), as a small digest, so mp_worldgen::pursuit_props can be
// held to the JS (L3) in CI and in wasm without the cache.
//
//   node tools/parity/pursuit-props-golden.mjs [--check]
//
// Per prop (each child of `pursuit-props`, in order): its node lines,
// written out whole (there are few), as tools/parity/car-model-golden.mjs
// writes them hashed; which materials each node uses, as indices into one
// table in order of first use; each material's canonical hash and the 8×8
// block means of its 4-channel textures. crates/mp_worldgen/tests/
// pursuit_props.rs builds the same and compares.
//
// --check regenerates in memory and fails if the file would change.

import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { ROOT, jsTreeKey } from './lib/jstree.mjs';

const check = process.argv.includes('--check');
const OUT = path.join(ROOT, 'parity/golden/car_model/props.json');

const f64 = new Float64Array(1);
const u32 = new Uint32Array(f64.buffer);
const bits = (x) => { f64[0] = x; return u32[1].toString(16).padStart(8, '0') + u32[0].toString(16).padStart(8, '0'); };
const sha = (buf) => createHash('sha256').update(buf).digest('hex');

// A cached export: header, digest and the binary section.
function exported(name) {
  const dir = path.join(ROOT, 'parity/cache', jsTreeKey(), 'scenes');
  const scene = path.join(dir, name + '.mrscene'), dig = path.join(dir, name + '.digest.json');
  if (!fs.existsSync(scene) || !fs.existsSync(dig)) return null;
  const buf = fs.readFileSync(scene);
  const J = buf.readUInt32LE(12);
  const H = JSON.parse(buf.subarray(16, 16 + J).toString('utf8'));
  const B = Math.ceil((16 + J) / 8) * 8;
  const D = JSON.parse(fs.readFileSync(dig, 'utf8'));
  const bytes = (a) => {
    const acc = H.accessors[a];
    const size = { f32: 4, f64: 8, u8: 1, u16: 2, u32: 4, i8: 1, i16: 2, i32: 4 }[acc.component];
    return buf.subarray(B + acc.offset, B + acc.offset + acc.count * acc.item_size * size);
  };
  return { H, D, bytes };
}

const meshLine = (m) => `${m.vertices} ${m.indices} ${Object.entries(m.attributes).map(([k, v]) => k + '=' + v).join(',')} ${m.index}`;

// Keys sorted, numbers as f64 bits: the form both sides hash.
const canon = (v) => {
  if (Array.isArray(v)) return v.map(canon);
  if (v && typeof v === 'object') return Object.fromEntries(Object.keys(v).sort().map((k) => [k, canon(v[k])]));
  if (typeof v === 'number') return bits(v);
  return v;
};

// As tools/parity/road-plan.mjs materialView.
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

// Per channel means over an 8×8 grid of blocks, two decimals (textures.mjs).
function blocks(width, height, rgba) {
  const out = [];
  for (let by = 0; by < 8; by++) for (let bx = 0; bx < 8; bx++) {
    const x0 = Math.floor(bx * width / 8), x1 = Math.floor((bx + 1) * width / 8);
    const y0 = Math.floor(by * height / 8), y1 = Math.floor((by + 1) * height / 8);
    const s = [0, 0, 0, 0];
    for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) for (let k = 0; k < 4; k++) s[k] += rgba[(y * width + x) * 4 + k];
    const n = (x1 - x0) * (y1 - y0);
    out.push(s.map((v) => Math.round(v / n * 100) / 100));
  }
  return out;
}

// One node's line (crates/mp_worldgen/tests/car_model.rs `node_line`).
function nodeLine({ H, D }, n) {
  let ml = '-', groups = '-', sphere = '-';
  if (n.mesh !== undefined && n.mesh !== null) {
    const mesh = H.meshes[n.mesh];
    ml = meshLine(D.meshes[n.mesh]);
    if (mesh.groups.length) groups = mesh.groups.map((g) => `${g.start}+${g.count}:${g.material_index}`).join(',');
    if (mesh.bounding_sphere) sphere = mesh.bounding_sphere.map(bits).join(',');
  }
  const multi = n.multi_material === undefined || n.multi_material === null ? '-' : n.multi_material ? 1 : 0;
  const matrix = sha(Buffer.from(n.matrix.map(bits).join(','))).slice(0, 16);
  return `${n.type} ${n.name} ${ml} ${groups} ${sphere} ${n.cast_shadow ? 1 : 0}${n.receive_shadow ? 1 : 0} ${n.visible ? 1 : 0} ${n.render_order} ${n.frustum_culled ? 1 : 0} ${n.matrix_auto_update ? 1 : 0} ${matrix} ${JSON.stringify(canon(n.user_data ?? {}))} ${multi}`;
}

function props(ex) {
  const { H } = ex;
  const root = H.nodes[H.roots[0]];
  const group = root.children.map((i) => H.nodes[i]).find((n) => n.name === 'pursuit-props');
  if (!group) throw new Error('no pursuit-props group in the models export');
  const table = [];
  const out = [];
  for (const ci of group.children) {
    const lines = [], mats = [];
    const walk = (c) => {
      const n = H.nodes[c];
      lines.push(nodeLine(ex, n));
      mats.push((n.materials ?? []).map((m) => {
        let k = table.indexOf(m);
        if (k < 0) { table.push(m); k = table.length - 1; }
        return k;
      }));
      for (const k of n.children ?? []) walk(k);
    };
    walk(ci);
    out.push({ name: H.nodes[ci].name, lines, mats });
  }
  const textures = [];
  const materials = table.map((i, k) => {
    const M = H.materials[i];
    for (const [where, obj] of [['params', M.params], ['uniforms', M.uniforms ?? {}]]) {
      for (const [key, v] of Object.entries(obj)) {
        if (!(v && typeof v === 'object' && typeof v.texture === 'number' && Object.keys(v).length === 1)) continue;
        const t = H.textures[v.texture];
        if (t.channels !== 4) continue;
        textures.push({ material: k, key: `${where}.${key}`, width: t.width, height: t.height, blocks: blocks(t.width, t.height, ex.bytes(t.pixels)) });
      }
    }
    return { kind: M.kind, sha256: sha(Buffer.from(JSON.stringify(canon(materialView(ex, i))))) };
  });
  return { props: out, materials, textures };
}

function stringify(obj) {
  return JSON.stringify(obj, null, 1)
    .replace(/\[\s+([-\d.,\s]+?)\s+\]/g, (m, inner) => `[${inner.replace(/\s+/g, '')}]`)
    .replace(/\[\s+((?:\[[-\d,]*\],?\s*)+)\]/g, (m, inner) => `[${inner.replace(/\s+/g, '')}]`) + '\n';
}

const ex = exported('models');
if (!ex) {
  console.log('pursuit-props-golden: no cached models export (node tools/parity/scene-export.mjs); kept the committed golden');
  process.exit(0);
}
const out = {
  note: 'Generated by tools/parity/pursuit-props-golden.mjs from the cached models export. Do not edit.',
  query: ex.H.meta.query,
  ...props(ex),
};
const text = stringify(out);
console.log(`pursuit-props-golden: ${out.props.length} props, ${out.materials.length} materials, ${out.textures.length} textures`);
if (check) {
  const old = fs.existsSync(OUT) ? fs.readFileSync(OUT, 'utf8') : null;
  if (old !== text) { console.error(`${path.relative(ROOT, OUT)} would change`); process.exit(1); }
} else {
  fs.mkdirSync(path.dirname(OUT), { recursive: true });
  fs.writeFileSync(OUT, text);
}
