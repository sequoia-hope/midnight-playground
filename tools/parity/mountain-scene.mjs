// The Sierra Pass scenery of the browser's scene export, as a small digest
// (roadmap WP 3.6), so that mp_worldgen's Mountain can be held to the JS
// (the L3 gate for zone 0) in CI and in wasm, without the cache.
//
//   node tools/parity/mountain-scene.mjs [--check]
//
// Reads parity/cache/<key>/scenes/sierra.mrscene and its digest
// (`node tools/parity/scene-export.mjs`) and writes
// parity/golden/mountain/sierra.json:
//   - the group `mountain`: its node and its place among the root's children;
//   - per child, in order: its node (name, type, matrix, flags, render
//     order, a sprite's centre), its mesh line (vertex and index counts,
//     each attribute's SHA-256, the index's), its instances (count, the
//     SHA-256 of the matrices and colours, the bounding sphere), the index
//     of its material in a table, and for a group (the parked cars) the
//     same for its children;
//   - the table: each material (after the first of its class, as what
//     differs from that one) described as crates/mp_worldgen/tests/
//     road.rs `material_view` describes it (textures by sampler and, but
//     for canvas textures, the SHA-256 of their pixels);
//   - every canvas texture the materials use, in order of first use: size,
//     SHA-256 and the 8×8 block means (WP 3.2's threshold gate needs only
//     those);
//   - the export's camera and night factor (the updaters ran there).
// Without the cache the committed file is kept (--check then passes).

import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { ROOT, jsTreeKey } from './lib/jstree.mjs';

const check = process.argv.includes('--check');
const OUT = path.join(ROOT, 'parity/golden/mountain/sierra.json');
const sha = (buf) => createHash('sha256').update(buf).digest('hex');

function exported(name) {
  const dir = path.join(ROOT, 'parity/cache', jsTreeKey(), 'scenes');
  const scene = path.join(dir, name + '.mrscene'), dig = path.join(dir, name + '.digest.json');
  if (!fs.existsSync(scene) || !fs.existsSync(dig)) return null;
  const buf = fs.readFileSync(scene);
  const jsonLen = buf.readUInt32LE(12);
  const H = JSON.parse(buf.subarray(16, 16 + jsonLen).toString('utf8'));
  // The binary chunk follows the header, padded to 8 bytes (FORMAT.md).
  const bin = 16 + Math.ceil(jsonLen / 8) * 8;
  return { H, D: JSON.parse(fs.readFileSync(dig, 'utf8')), buf, bin };
}

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

const nodeView = (n) => ({
  name: n.name, type: n.type, matrix: n.matrix, visible: n.visible, matrix_auto_update: n.matrix_auto_update,
  frustum_culled: n.frustum_culled, render_order: n.render_order, cast_shadow: n.cast_shadow,
  receive_shadow: n.receive_shadow, layers: n.layers, center: n.center ?? null,
});

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

function pixels(ex, t) {
  const a = ex.H.accessors[t.pixels];
  const off = ex.bin + a.offset;
  return ex.buf.subarray(off, off + a.count * a.item_size);
}

// The table with each material after the first of its class written as
// what differs from that one: `{ like: k, ...top-level keys that differ,
// params: { keys that differ } }`.
function compact(views) {
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  return views.map((v, i) => {
    const k = views.findIndex((w) => w.type === v.type);
    if (k === i) return v;
    const b = views[k];
    const out = { like: k };
    for (const key of Object.keys(v)) {
      if (key === 'params') {
        const p = {};
        for (const [pk, pv] of Object.entries(v.params)) if (!same(pv, b.params[pk])) p[pk] = pv;
        out.params = p;
      } else if (!same(v[key], b[key])) out[key] = v[key];
    }
    return out;
  });
}

function digestOf(ex) {
  const { H, D } = ex;
  const root = H.nodes[H.roots[0]];
  const gi = root.children.find((c) => H.nodes[c].name === 'mountain');
  const g = H.nodes[gi];
  const table = [];
  const child = (c) => {
    const n = H.nodes[c];
    const out = { node: nodeView(n) };
    if (n.mesh !== undefined && n.mesh !== null) {
      out.mesh = meshLine(D.meshes[n.mesh]);
      let k = table.indexOf(n.materials[0]);
      if (k < 0) { table.push(n.materials[0]); k = table.length - 1; }
      out.material = k;
      const dr = D.drawables.find((d) => d.node === c);
      if (n.instances !== undefined && n.instances !== null) {
        const inst = H.instances[n.instances];
        out.instances = { count: dr.instances, matrices: dr.instance_matrices, colors: dr.instance_colors, bounding_sphere: inst.bounding_sphere };
      }
    }
    if (n.children.length) out.children = n.children.map(child);
    return out;
  };
  const children = g.children.map(child);
  const canvas = [];
  const seen = new Set();
  for (const i of table) {
    const m = H.materials[i];
    const refs = [];
    const walk = (v) => {
      if (Array.isArray(v)) v.forEach(walk);
      else if (v && typeof v === 'object') {
        if (typeof v.texture === 'number' && Object.keys(v).length === 1) refs.push(v.texture);
        else Object.values(v).forEach(walk);
      }
    };
    walk(m.params); walk(m.uniforms);
    for (const t of refs) {
      const tex = H.textures[t];
      if (tex.source !== 'canvas' || seen.has(tex.pixels)) continue;
      seen.add(tex.pixels);
      canvas.push({ width: tex.width, height: tex.height, sha256: D.textures[t].sha256, blocks: blocks(tex.width, tex.height, pixels(ex, tex)) });
    }
  }
  return {
    group: { child: root.children.indexOf(gi), node: nodeView(g) },
    camera: H.environment.camera.position,
    night: H.environment.night,
    children,
    table: compact(table.map((i) => materialView(ex, i))),
    canvas,
  };
}

const ex = exported('sierra');
let out;
if (ex) {
  out = {
    note: 'Generated by tools/parity/mountain-scene.mjs from the cached scene export (parity/cache/<key>/scenes/sierra.mrscene). Do not edit.',
    level: 'sierra',
    ...digestOf(ex),
  };
} else if (fs.existsSync(OUT)) {
  out = JSON.parse(fs.readFileSync(OUT, 'utf8'));
  console.log('mountain-scene: no export in the cache, the committed digest is kept');
} else {
  console.error('mountain-scene: no export in the cache (node tools/parity/scene-export.mjs) and no committed digest');
  process.exit(1);
}
// Children and blocks one per line, so the file stays small and readable.
const keep = [];
const text = JSON.stringify(out, (k, v) => {
  if ((k === 'blocks' || k === 'children') && Array.isArray(v)) {
    // A child or a block list per line.
    keep.push('[\n' + v.map((x) => '  ' + JSON.stringify(x)).join(',\n') + '\n ]');
    return `@@${keep.length - 1}@@`;
  }
  return v;
}, 1).replace(/"@@(\d+)@@"/g, (_, i) => keep[+i]) + '\n';
if (check) {
  if (!fs.existsSync(OUT) || fs.readFileSync(OUT, 'utf8') !== text) {
    console.error(`${path.relative(ROOT, OUT)}: would change`);
    process.exit(1);
  }
  console.log('mountain-scene: identical');
} else {
  fs.mkdirSync(path.dirname(OUT), { recursive: true });
  fs.writeFileSync(OUT, text);
  console.log(`mountain-scene: wrote ${path.relative(ROOT, OUT)}: ${out.children.length} children, ${out.table.length} materials, ${out.canvas.length} canvas textures (${text.length} bytes)`);
}
