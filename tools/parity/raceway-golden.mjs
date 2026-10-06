// Seaside Raceway of the browser's scene export (roadmap WP 7.4):
// everything Raceway.js and raceway/* contribute (the group `raceway`), as
// a small digest, so that mp_worldgen's Raceway can be held to the JS (L3)
// in CI and in wasm, without the cache. The pattern of
// tools/parity/desert-golden.mjs, whose file it does not touch.
//
//   node tools/parity/raceway-golden.mjs [--check]
//
// From the cached full export (parity/cache/<key>/scenes/seaside.mrscene
// and .digest.json, `node tools/parity/scene-export.mjs`):
//   - one line per node under the group `raceway`, depth first in the order
//     added (type, name, the mesh's vertex and index counts and each
//     attribute's and the index's SHA-256, the shadow flags, visibility,
//     render order, culling, matrixAutoUpdate, the local matrix's bits, the
//     instance count and the SHA-256 of the instance matrices and colours),
//     each line's SHA-256 (16 hex digits) and all of them hashed together;
//   - which material each drawable uses, as an index into a table of the
//     materials in order of first use, each material described (textures by
//     their sampler; canvas pixels are WP 3.2's threshold gate) and hashed in
//     a canonical form (keys sorted, every number as its f64 bits), which
//     crates/mp_worldgen/tests/seaside.rs makes the same way;
//   - per texture a material uses, the 8×8 block means of its pixels (as
//     tools/parity/textures.mjs writes them), for the threshold gate without
//     the cache;
//   - every texture of the scene that is not a canvas (the terrain's data
//     textures, the aerial photo and the loose-ground mask made from the
//     survey): its index, source, size, channels and the SHA-256 of its
//     pixels, which must come out byte-identical;
//   - the night factor and camera the export was taken with, the night
//     parameters in the order registered, and each
//     light's description, hashed in the canonical form.
//
// --check regenerates in memory and fails if a file would change.

import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { ROOT, jsTreeKey } from './lib/jstree.mjs';

const check = process.argv.includes('--check');
const OUT = path.join(ROOT, 'parity/golden/seaside');
const IDS = ['seaside'];

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

function groupView(ex, name) {
  const { H, D, bytes } = ex;
  const group = H.nodes.findIndex((n) => n.name === name);
  const lines = [], mats = [], table = [];
  let vertices = 0;
  const walk = (c) => {
    const n = H.nodes[c];
    let ml = '-', inst = '- - -', mk = -1;
    if (n.mesh !== undefined && n.mesh !== null) {
      const m = D.meshes[n.mesh];
      vertices += m.vertices;
      ml = meshLine(m);
      const dr = D.drawables.find((d) => d.node === c);
      inst = `${dr.instances ?? '-'} ${dr.instance_matrices ?? '-'} ${dr.instance_colors ?? '-'}`;
      mk = table.indexOf(n.materials[0]);
      if (mk < 0) { table.push(n.materials[0]); mk = table.length - 1; }
    }
    lines.push(`${n.type} ${n.name} ${ml} ${n.cast_shadow ? 1 : 0}${n.receive_shadow ? 1 : 0} ${n.visible ? 1 : 0} ${n.render_order} ${n.frustum_culled ? 1 : 0} ${n.matrix_auto_update ? 1 : 0} ${sha(Buffer.from(n.matrix.map(bits).join(','))).slice(0, 16)} ${inst}`);
    mats.push(mk);
    for (const k of n.children ?? []) walk(k);
  };
  for (const c of H.nodes[group].children) walk(c);
  const textures = [];
  const materials = table.map((i, k) => {
    const view = materialView(ex, i);
    const M = H.materials[i];
    for (const [where, obj] of [['params', M.params], ['uniforms', M.uniforms ?? {}]]) {
      for (const [key, v] of Object.entries(obj)) {
        if (!(v && typeof v === 'object' && typeof v.texture === 'number' && Object.keys(v).length === 1)) continue;
        const t = H.textures[v.texture];
        if (t.channels !== 4) continue;
        textures.push({ material: k, key: `${where}.${key}`, width: t.width, height: t.height, blocks: blocks(t.width, t.height, bytes(t.pixels)) });
      }
    }
    return { kind: M.kind, sha256: sha(Buffer.from(JSON.stringify(canon(view)))) };
  });
  return {
    count: lines.length, vertices,
    sha256: sha(Buffer.from(lines.join('\n'))),
    lines: lines.map((l) => sha(Buffer.from(l)).slice(0, 16)),
    mats, materials, textures,
  };
}

// One line per texture and per block list, so the file stays readable.
function stringify(obj) {
  return JSON.stringify(obj, null, 1).replace(/\[\s+([-\d.,\s]+?)\s+\]/g, (m, inner) => `[${inner.replace(/\s+/g, '')}]`) + '\n';
}

let problems = 0;
fs.mkdirSync(OUT, { recursive: true });
for (const id of IDS) {
  const file = path.join(OUT, id + '.json');
  const ex = exported(id);
  if (!ex) {
    console.log(`${id}: no cached export (node tools/parity/scene-export.mjs); kept the committed golden`);
    continue;
  }
  const env = ex.H.environment;
  const out = {
    note: 'Generated by tools/parity/raceway-golden.mjs from the cached scene export. Do not edit.',
    level: id,
    night: env.night,
    camera: { position: env.camera.position, fov: env.camera.fov },
    lights: ex.H.lights.map((l) => sha(Buffer.from(JSON.stringify(canon(l)))).slice(0, 16)),
    night_params: ex.H.night_params,
    raceway: groupView(ex, 'raceway'),
    data: ex.H.textures.flatMap((t, i) => (t.source === 'canvas' ? [] : [{
      texture: i, source: t.source, width: t.width, height: t.height, channels: ex.D.textures[i].channels, sha256: ex.D.textures[i].sha256,
    }])),
  };
  const text = stringify(out);
  console.log(`${id}: raceway ${out.raceway.count} nodes, ${out.raceway.vertices} vertices, ${out.raceway.materials.length} materials, ${out.raceway.textures.length} textures; ${out.data.length} data textures`);
  if (check) {
    const old = fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : null;
    if (old !== text) { console.error(`${id}: ${file} would change`); problems++; }
  } else fs.writeFileSync(file, text);
}
process.exit(problems ? 1 : 0);
