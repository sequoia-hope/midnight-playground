// What the scenery tells the road before it is built, and the road, sky and
// sea of the browser's scene exports (roadmap WP 3.5), so that mr_worldgen's
// Road, Sky and Sea can be held to the JS (the L3 road digest) in CI and in
// wasm, without the cache.
//
//   NODE_OPTIONS=--import=./tools/parity/kernel/register.mjs \
//     node tools/parity/road-plan.mjs [--check]
//
// The plan: World.build's first stages run under Node with the parity kernel
// as tools/parity/terrain-plan.mjs runs them (the Track, `new Terrain`, each
// scenery module made as loadScenery makes it and its plan() run), then the
// road's inputs are read off the track: `fenceGaps` (Valley, Beach),
// `noMarks` (Streets) and `runout` (City, Harbor, Streets). They are checked
// against the world golden (parity/golden/world/<level>.json, the browser's
// track after the whole build), which lists the same values.
//
// The digests: from the cached scene exports (parity/cache/<key>/scenes/,
// `node tools/parity/scene-export.mjs [--base]`), per level
//   - road: one line per child of the group `road` (type, vertex and index
//     counts, each attribute's SHA-256, the index's, the shadow flags, the
//     instance count and matrices' SHA-256) hashed together, and per child
//     the index of its material in a table of the materials' descriptions
//     (textures described by their sampler and, but for canvas textures,
//     the SHA-256 of their pixels; canvas pixels are WP 3.2's threshold
//     gate);
//   - sky: the dome's mesh line, its material, the four roots' nodes, the
//     lights, the fog, exposure and night factor, and the focus the lights
//     were placed at;
//   - sea (levels with a sea, from the full export): its node, mesh line,
//     material and the wave normal map's pixels' SHA-256;
//   - the number of textures in the base export (terrain, road and sky).
// Without the cache the committed values are kept.
//
// The time of day along the route: a Sky made under Node, updated at 41
// points from the start to the finish (dt 0, the focus on the road there),
// everything update() sets hashed in a fixed order (the `skyRoute` hash;
// see crates/mr_worldgen/tests/road.rs `sky_route`).
//
// Writes parity/golden/road/<level>.json. Numbers of the plan are hex f64
// bits. --check regenerates in memory and fails if a file would change.

import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { ROOT, jsTreeKey } from './lib/jstree.mjs';
import { seedRandom, RANDOM_SEED } from './lib/seed-random.mjs';
import { kernelInstalled } from '../../src/parity/kernel.js';

if (!kernelInstalled()) {
  console.error('road-plan: run with NODE_OPTIONS=--import=./tools/parity/kernel/register.mjs');
  process.exit(1);
}
seedRandom(RANDOM_SEED);

await import('../../test/unit/support/three.js');
const { LEVELS } = await import('../../test/unit/support/levels.js');
const { Track } = await import('../../src/track/Track.js');
const { Terrain } = await import('../../src/world/Terrain.js');
const THREE = await import('three');
const { Sky } = await import('../../src/world/Sky.js');

const check = process.argv.includes('--check');
const OUT = path.join(ROOT, 'parity/golden/road');
const IDS = ['sierra', 'coast', 'streets', 'desert', 'seaside', 'cruise'];

const f64 = new Float64Array(1);
const u32 = new Uint32Array(f64.buffer);
const bits = (x) => { f64[0] = x; return u32[1].toString(16).padStart(8, '0') + u32[0].toString(16).padStart(8, '0'); };
const sha = (buf) => createHash('sha256').update(buf).digest('hex');

// World.loadScenery and the plan stage of World.build.
async function plan(level) {
  const track = new Track(level);
  const terrain = new Terrain(track, level);
  const world = { level, track, terrain, zoneIndex: (key) => level.zones.findIndex((z) => z.key === key) };
  const names = [...new Set(level.zones.map((z) => z.scenery).filter(Boolean))];
  const kept = [], failed = [];
  for (const name of names) {
    const mod = await import(`../../src/world/${name}.js`);
    const zone = level.zones.findIndex((z) => z.scenery === name);
    const s = new mod.default({ zone, key: level.zones[zone].key, level });
    try { s.plan?.(world); kept.push(name); } catch (e) { failed.push(`${name}: ${e.message}`); }
  }
  return { track, kept, failed };
}

// A cached export: its header and digest (null without the cache).
function exported(name) {
  const dir = path.join(ROOT, 'parity/cache', jsTreeKey(), 'scenes');
  const scene = path.join(dir, name + '.mrscene'), dig = path.join(dir, name + '.digest.json');
  if (!fs.existsSync(scene) || !fs.existsSync(dig)) return null;
  const fd = fs.openSync(scene, 'r');
  const head = Buffer.alloc(16);
  fs.readSync(fd, head, 0, 16, 0);
  const json = Buffer.alloc(head.readUInt32LE(12));
  fs.readSync(fd, json, 0, json.length, 16);
  fs.closeSync(fd);
  return { H: JSON.parse(json.toString('utf8')), D: JSON.parse(fs.readFileSync(dig, 'utf8')) };
}

// mr_worldgen's tests make the same line from mr_scene's digest.
const meshLine = (m) => `${m.vertices} ${m.indices} ${Object.entries(m.attributes).map(([k, v]) => k + '=' + v).join(',')} ${m.index}`;

// A material with each texture reference replaced by what the texture is
// (as crates/mr_worldgen/tests/road.rs `material_view` does), and a
// ShaderMaterial's GLSL by its SHA-256.
function materialView({ H, D }, i) {
  const walk = (v) => {
    if (Array.isArray(v)) return v.map(walk);
    if (v && typeof v === 'object') {
      const keys = Object.keys(v);
      if (keys.length === 1 && keys[0] === 'texture' && typeof v.texture === 'number') {
        const t = H.textures[v.texture];
        // The header leaves a texture's url out when it has none; mr_scene
        // reads that as null.
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
  receive_shadow: n.receive_shadow, layers: n.layers,
});

function road(ex) {
  const { H, D } = ex;
  const g = H.nodes.find((n) => n.name === 'road');
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
    lines.push(`${n.type} ${meshLine(m)} ${n.cast_shadow ? 1 : 0}${n.receive_shadow ? 1 : 0} ${dr.instances ?? '-'} ${dr.instance_matrices ?? '-'}`);
  }
  return {
    count: lines.length, vertices, sha256: sha(Buffer.from(lines.join('\n'))),
    materials: mats,
    table: table.map((i) => materialView(ex, i)),
  };
}

function sky(ex) {
  const { H, D } = ex;
  const roots = H.roots.map((r) => H.nodes[r]);
  const dome = roots.find((n) => n.type === 'Mesh' && H.materials[n.materials[0]].kind === 'SkyDome');
  const lights = ['DirectionalLight', 'Object3D', 'HemisphereLight'].map((t) => roots.find((n) => n.type === t));
  const env = H.environment;
  return {
    mesh: meshLine(D.meshes[dome.mesh]),
    material: materialView(ex, dome.materials[0]),
    nodes: [dome, ...lights].map(nodeView),
    lights: H.lights.map(({ node, ...l }) => l),
    focus: H.lights.find((l) => l.type === 'DirectionalLight').target,
    fog: env.fog, exposure: env.tone_mapping_exposure, night: env.night,
  };
}

function sea(ex) {
  const { H, D } = ex;
  const root = H.nodes[H.roots[0]];
  const c = root.children.find((k) => { const n = H.nodes[k]; return n.materials && H.materials[n.materials[0]].kind === 'Sea'; });
  if (c === undefined) return null;
  const n = H.nodes[c];
  const mat = H.materials[n.materials[0]];
  return {
    child: root.children.indexOf(c),
    node: nodeView(n),
    mesh: meshLine(D.meshes[n.mesh]),
    material: materialView(ex, n.materials[0]),
    normal_map_sha256: D.textures[mat.params.normalMap.texture].sha256,
  };
}

// Sky.update at SKY_SAMPLES points along the route: every value it sets,
// as f64 bits, in the order crates/mr_worldgen/tests/road.rs writes them.
const SKY_SAMPLES = 41;
function skyRoute(level, track) {
  const scene = new THREE.Scene();
  const renderer = { toneMappingExposure: 1 };
  const sky = new Sky(scene, renderer, track, level.sky, level.sunAzimuth);
  const u = sky.uniforms;
  const values = [];
  const put = (...xs) => { for (const x of xs) values.push(x); };
  const rgb = (c) => put(c.r, c.g, c.b);
  const xyz = (v) => put(v.x, v.y, v.z);
  for (let i = 0; i < SKY_SAMPLES; i++) {
    const s = (i / (SKY_SAMPLES - 1)) * track.length;
    const f = track.frame(s);
    const focus = new THREE.Vector3(f.x, f.y, f.z);
    const k = sky.update(0, s, focus);
    put(s, sky.night);
    rgb(u.uZenith.value); rgb(u.uHorizon.value); rgb(u.uGround.value); rgb(u.uSunColor.value);
    xyz(u.uSunDir.value); xyz(u.uMoonDir.value);
    put(u.uNight.value, u.uCloud.value, u.uHaze.value);
    rgb(sky.sun.color); put(sky.sun.intensity); xyz(sky.sun.position); xyz(sky.sun.target.position);
    rgb(sky.hemi.color); rgb(sky.hemi.groundColor); put(sky.hemi.intensity);
    rgb(scene.fog.color); put(scene.fog.density, renderer.toneMappingExposure);
    xyz(sky.dome.position);
    put(k.sunEl);
  }
  const buf = Buffer.from(new Float64Array(values).buffer);
  return { samples: SKY_SAMPLES, values: values.length, sha256: sha(buf) };
}

let problems = 0;
fs.mkdirSync(OUT, { recursive: true });
for (const id of IDS) {
  const level = LEVELS.find((l) => l.id === id);
  const { track, kept, failed } = await plan(level);
  const golden = JSON.parse(fs.readFileSync(path.join(ROOT, 'parity/golden/world', id + '.json'), 'utf8')).track.scalars;
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const gapsOk = same(track.fenceGaps, golden.fenceGaps);
  const marksOk = same(track.noMarks ?? null, golden.noMarks ?? null);
  const runoutOk = track.runout === golden.runout;
  console.log(`${id}: scenery ${kept.join(', ')}${failed.length ? ' (failed: ' + failed.join('; ') + ')' : ''}; ${track.fenceGaps.length} fence gaps ${gapsOk ? 'ok' : 'DIFFER'}, ${(track.noMarks || []).length} unpainted stretches ${marksOk ? 'ok' : 'DIFFER'}, runout ${track.runout} ${runoutOk ? 'ok' : 'DIFFERS'} (against the world golden)`);
  if (!gapsOk || !marksOk || !runoutOk) problems++;
  const file = path.join(OUT, id + '.json');
  const old = fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, 'utf8')) : {};
  const out = {
    note: 'Generated by tools/parity/road-plan.mjs with the parity kernel on. Do not edit. Numbers of the plan are hex f64 bits.',
    level: id,
    scenery: kept,
    failed,
    fenceGaps: track.fenceGaps.map((g) => ({ s0: bits(g.s0), s1: bits(g.s1), side: bits(g.side) })),
    noMarks: (track.noMarks || []).map((g) => ({ s0: bits(g.s0), s1: bits(g.s1) })),
    runout: bits(track.runout),
    skyRoute: skyRoute(level, track),
    textures: old.textures ?? null,
    road: old.road ?? null,
    sky: old.sky ?? null,
    sea: old.sea ?? null,
  };
  const base = exported(id + '.base');
  if (base) {
    out.textures = base.H.textures.length;
    out.road = road(base);
    out.sky = sky(base);
  }
  const full = exported(id);
  if (full) out.sea = sea(full);
  console.log(`  ${base ? 'from the cached exports' : 'no export in the cache, committed digests kept'}: road ${out.road ? `${out.road.count} meshes, ${out.road.vertices} vertices, ${out.road.table.length} materials` : 'none'}; sea ${out.sea ? out.sea.mesh.split(' ')[0] + ' vertices' : 'none'}; ${out.textures} textures`);
  const text = JSON.stringify(out, null, 1) + '\n';
  if (check) {
    if (!fs.existsSync(file) || fs.readFileSync(file, 'utf8') !== text) { console.error(`${path.relative(ROOT, file)}: would change`); problems++; }
  } else fs.writeFileSync(file, text);
}
if (problems) { console.error(`road-plan: ${problems} problem(s)`); process.exit(1); }
