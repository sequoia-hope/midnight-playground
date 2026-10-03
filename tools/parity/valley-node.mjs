// Old Mill Valley's plan, build and updater under Node (roadmap WP 3.7):
// World.build's stages that Valley needs (the Track, `new Terrain`, every
// scenery module's plan() in order, buildFields, resolveFlattens, the Sky),
// then Valley.build with a canvas that draws nothing, then the updater it
// registers, run over a few frames with the sky updated before each, as
// World.update does.
//
//   node --import=./tools/parity/kernel/register.mjs tools/parity/valley-node.mjs [--check]
//
// Writes parity/golden/valley/animators.json: per frame (dt, s) every value
// the updater sets (the windpump wheels' instance matrices as the SHA-256 of
// their Float32Array, the waterwheel's and the sails' quaternions, the
// creek's normal-map offset and its emissive tint) as hex f64 bits;
// crates/mr_worldgen/tests/valley.rs runs Valley's animator over the same
// frames. Also prints what the build counted, and how many Math.random
// draws separate the canvases the build creates (three.js draws four per
// uuid, so the wood signs' plank noise starts at stream positions that
// depend on every object made before them; DECISIONS D332).

import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { ROOT } from './lib/jstree.mjs';
import { seedRandom, RANDOM_SEED } from './lib/seed-random.mjs';
import { kernelInstalled } from '../../src/parity/kernel.js';

if (!kernelInstalled()) {
  console.error('valley-node: run with --import=./tools/parity/kernel/register.mjs');
  process.exit(1);
}
const check = process.argv.includes('--check');
seedRandom(RANDOM_SEED);
let draws = 0;
const random = Math.random;
Math.random = () => { draws++; return random(); };

// A canvas whose context accepts every call and draws nothing.
const canvases = [];
const ctx = new Proxy({}, {
  get: (_, k) => (k === 'createImageData' ? (w, h) => ({ width: w, height: h, data: new Uint8ClampedArray(w * h * 4) })
    : k === 'measureText' ? () => ({ width: 0 }) : () => {}),
  set: () => true,
});
globalThis.document = {
  createElement: (tag) => {
    if (tag === 'canvas') canvases.push(draws);
    return { width: 0, height: 0, getContext: () => ctx, style: {} };
  },
};

await import('../../test/unit/support/three.js');
const THREE = await import('three');
const { LEVELS } = await import('../../test/unit/support/levels.js');
const { Track } = await import('../../src/track/Track.js');
const { Terrain } = await import('../../src/world/Terrain.js');
const { Sky } = await import('../../src/world/Sky.js');

const f64 = new Float64Array(1);
const u32 = new Uint32Array(f64.buffer);
const bits = (x) => { f64[0] = x; return u32[1].toString(16).padStart(8, '0') + u32[0].toString(16).padStart(8, '0'); };
const sha = (buf) => createHash('sha256').update(buf).digest('hex');

const level = LEVELS.find((l) => l.id === 'sierra');
const track = new Track(level);
const terrain = new Terrain(track, level);
const scene = new THREE.Scene();
const world = {
  level, track, terrain, scene, updaters: [], night: [],
  zoneIndex: (key) => level.zones.findIndex((z) => z.key === key),
  addNight(material, prop, day, night) { this.night.push({ material, prop, day, night }); },
};
const names = [...new Set(level.zones.map((z) => z.scenery).filter(Boolean))];
const mods = [];
for (const name of names) {
  const mod = await import(`../../src/world/${name}.js`);
  const zone = level.zones.findIndex((z) => z.scenery === name);
  const s = new mod.default({ zone, key: level.zones[zone].key, level });
  s.plan?.(world);
  mods.push([name, s]);
}
terrain.buildFields();
terrain.resolveFlattens();
const renderer = { toneMappingExposure: 1 };
world.sky = new Sky(scene, renderer, track, level.sky, level.sunAzimuth);
const valley = mods.find(([n]) => n === 'Valley')[1];
const d0 = draws;
canvases.length = 0;
await valley.build(world);
const st = valley.stats;
console.log(`farms ${st.farms}, trees ${st.trees}, bales ${st.bales}, cows ${st.cows}, meshes ${st.meshes}; hedges ${valley.hedges.length}, crop runs ${valley.cropRuns.length}, grass ${valley.verge.grass.length}, flowers ${valley.verge.flowers.length}, poles ${valley.poles?.length}, wheels ${valley.wheels.length}`);
console.log(`Math.random draws from the start of build() to each canvas: ${canvases.map((c) => c - d0).join(', ')}`);
console.log(`between consecutive canvases: ${canvases.slice(1).map((c, i) => c - canvases[i]).join(', ')}`);

// The updater over a few frames: (dt, s), the sky updated first.
const FRAMES = [[0, 0], [1 / 60, 0], [0.25, 1500], [0.5, 4000], [1 / 30, 6500], [2, 9000]];
const updater = world.updaters[world.updaters.length - 1];
const frames = FRAMES.map(([dt, s]) => {
  world.sky.update(dt, s);
  updater(dt, world.sky.night);
  const q = (o) => [o.quaternion.x, o.quaternion.y, o.quaternion.z, o.quaternion.w].map(bits);
  const m = valley.waterMat;
  return {
    dt: bits(dt), s: bits(s),
    wheels: sha(Buffer.from(valley.wheelMesh.instanceMatrix.array.buffer)),
    waterwheel: q(valley.waterwheel),
    sails: q(valley.sails),
    offset: [m.normalMap.offset.x, m.normalMap.offset.y].map(bits),
    emissive: [m.emissive.r, m.emissive.g, m.emissive.b].map(bits),
  };
});
const out = {
  note: 'Generated by tools/parity/valley-node.mjs with the parity kernel on. Do not edit. Numbers are hex f64 bits.',
  frames,
};
const FILE = path.join(ROOT, 'parity/golden/valley/animators.json');
const text = JSON.stringify(out, null, 1) + '\n';
if (check) {
  if (!fs.existsSync(FILE) || fs.readFileSync(FILE, 'utf8') !== text) { console.error(`${path.relative(ROOT, FILE)}: would change`); process.exit(1); }
  console.log('valley animators: the golden reproduces');
} else {
  fs.mkdirSync(path.dirname(FILE), { recursive: true });
  fs.writeFileSync(FILE, text);
  console.log(`valley animators: ${frames.length} frames`);
}
