// Seaside Raceway's animators, time step by time step (roadmap WP 7.4; the
// pattern of tools/parity/desert-animators.mjs, D555, whose file and
// golden it does not touch): the game's own `world.update(dt, s, focus,
// camera)` on Seaside Raceway, with the parity kernel, then the race's
// `world.onCountdown(cd)`, over a fixed list of frames, and every value
// they change under `world.root` recorded after each frame. Raceway
// registers no updater: what moves is the road's dew, the night parameters
// and the sky World.update applies, and the start lights, whose five
// materials' emissiveIntensity onCountdown sets from the countdown (the
// race calls it every frame with the seconds left, 3.999 to 0, or -1 once
// racing).
//
//   node --import=./tools/parity/kernel/register.mjs tools/parity/seaside-animators.mjs [--check]
//   node tools/parity/seaside-animators.mjs --browser [--check]
//
// As animators.mjs: by default under Node (a canvas that draws nothing, a
// patched material's uniforms taken by running its onBeforeCompile on an
// empty shader, the aerial photo's texture an empty one, since nothing
// reads its pixels); --browser takes the same capture in the game itself
// (?kernel=1&freeze=1&s=0, the menu: no race, so nothing else calls
// onCountdown).
//
// The frames: uneven steps (dt 0 to 3.3 s) round the lap with the camera
// at the line, the hairpin and the Corkscrew, the countdown running down
// from 3.999 through every light to GO and racing (-1), then back; then 360
// ticks of 1/120 s with the player at 60 m/s and the countdown running from
// 4 to 0 (cd = 4 - (k + 1) / 90). Writes parity/golden/animators/seaside.json;
// crates/mp_worldgen/tests/seaside_animators.rs replays it on the Rust build
// (WorldBuild::update_sky, the night parameters, WorldBuild::update, then
// WorldBuild::countdown).

import fs from 'node:fs';
import path from 'node:path';
import { ROOT } from './lib/jstree.mjs';
import { seedRandom, RANDOM_SEED } from './lib/seed-random.mjs';

const CHECK = process.argv.includes('--check');
const BROWSER = process.argv.includes('--browser');
const OUT = path.join(ROOT, 'parity/golden/animators/seaside.json');
const LEVEL = 'seaside';

// Runs in the page (env null) or under Node, synchronously until the
// hashing at the end.
async function run(env) {
  const page = !env;
  if (page) env = { THREE: window.__THREE, world: window.__world, camera: window.__camera.clone() };
  const { THREE, world, camera } = env;
  const renderer = world.renderer;
  if (page) renderer.compile(world.realScene, window.__camera);
  const f64 = new Float64Array(1), u32 = new Uint32Array(f64.buffer);
  const bits = (x) => { f64[0] = x; return u32[1].toString(16).padStart(8, '0') + u32[0].toString(16).padStart(8, '0'); };
  const hex = (buf) => Array.from(new Uint8Array(buf), (b) => b.toString(16).padStart(2, '0')).join('');
  const sha = async (a) => 'sha256:' + hex(await crypto.subtle.digest('SHA-256', new Uint8Array(a.buffer, a.byteOffset, a.byteLength)));

  // The targets, numbered as the scene export numbers them.
  const nodes = [];
  const walk = (o) => { nodes.push(o); for (const c of o.children) walk(c); };
  walk(world.root);
  const mats = [], matIdx = new Map();
  for (const o of nodes) {
    if (!(o.isMesh || o.isPoints || o.isLine || o.isSprite)) continue;
    for (const m of Array.isArray(o.material) ? o.material : [o.material]) if (!matIdx.has(m)) { matIdx.set(m, mats.length); mats.push(m); }
  }
  const geoNode = new Map();
  nodes.forEach((o, i) => { if (o.geometry && !geoNode.has(o.geometry)) geoNode.set(o.geometry, i); });
  const isPatched = (m) => Object.hasOwn(m, 'onBeforeCompile') && m.onBeforeCompile !== THREE.Material.prototype.onBeforeCompile;
  const SHADER_ID = {
    MeshStandardMaterial: 'physical', MeshPhysicalMaterial: 'physical', MeshLambertMaterial: 'lambert',
    MeshBasicMaterial: 'basic', LineBasicMaterial: 'basic', SpriteMaterial: 'sprite', PointsMaterial: 'points',
  };
  const patchUniforms = new Map();
  const uniformsOf = (m) => {
    if (m.isShaderMaterial) return Object.entries(m.uniforms);
    if (!isPatched(m)) return [];
    let compiled;
    if (page) {
      compiled = renderer.properties.get(m).uniforms;
      if (!compiled) throw new Error('seaside-animators: a patched material was never compiled');
    } else {
      // The patch's own uniform objects, as onBeforeCompile hands them to
      // the program.
      if (!patchUniforms.has(m)) {
        const sh = { uniforms: {}, defines: {}, vertexShader: '', fragmentShader: '' };
        m.onBeforeCompile(sh, renderer);
        patchUniforms.set(m, sh.uniforms);
      }
      compiled = patchUniforms.get(m);
    }
    const base = THREE.ShaderLib[SHADER_ID[m.type]].uniforms;
    return Object.entries(compiled).filter(([k]) => !(k in base));
  };
  const SKIP = new Set(['uuid', 'id', 'name', 'type', 'version', 'userData', '_listeners', 'uniforms', 'uniformsGroups', 'vertexShader', 'fragmentShader']);
  const texKey = new Map();
  mats.forEach((m, j) => {
    for (const k of Object.keys(m)) if (m[k]?.isTexture && !texKey.has(m[k])) texKey.set(m[k], `m${j}.${k.replace(/^_/, '')}`);
    for (const [k, u] of uniformsOf(m)) if (u.value?.isTexture && !texKey.has(u.value)) texKey.set(u.value, `m${j}.u.${k}`);
  });

  // A value as the golden writes it: numbers as bits, vectors and colours
  // as their components' bits, booleans as words; typed arrays are copied
  // here and hashed at the end.
  const enc = (v) => {
    if (typeof v === 'number') return bits(v);
    if (typeof v === 'boolean') return String(v);
    if (v?.isColor) return [v.r, v.g, v.b].map(bits).join(',');
    if (v?.isVector2 || v?.isVector3 || v?.isVector4 || v?.isQuaternion) return v.toArray().map(bits).join(',');
    return undefined;
  };
  const versions = new Map();
  // Everything the updaters could touch, as key → value (a string, or a
  // typed array copy to hash).
  function snapshot(first) {
    const out = new Map();
    const arr = (key, a, owner) => {
      const v = a.version;
      if (first) { versions.set(owner, v); out.set(key, 'unchanged'); return; }
      out.set(key, v === versions.get(owner) ? 'unchanged' : a.array.slice());
    };
    nodes.forEach((o, i) => {
      out.set(`n${i}.position`, enc(o.position));
      out.set(`n${i}.quaternion`, enc(o.quaternion));
      out.set(`n${i}.scale`, enc(o.scale));
      out.set(`n${i}.visible`, enc(o.visible));
      if (o.isLight) out.set(`n${i}.light`, [o.color.r, o.color.g, o.color.b, o.intensity].map(bits).join(','));
      if (o.isInstancedMesh) {
        out.set(`n${i}.count`, enc(o.count));
        arr(`n${i}.instanceMatrix`, o.instanceMatrix, o.instanceMatrix);
        if (o.instanceColor) arr(`n${i}.instanceColor`, o.instanceColor, o.instanceColor);
      }
      if (o.geometry && geoNode.get(o.geometry) === i) {
        for (const [name, a] of Object.entries(o.geometry.attributes)) arr(`n${i}.attr.${name}`, a, a);
      }
    });
    mats.forEach((m, j) => {
      for (const k of Object.keys(m)) {
        if (SKIP.has(k) || /^is[A-Z]/.test(k)) continue;
        const e = enc(m[k]);
        if (e !== undefined) out.set(`m${j}.${k.replace(/^_/, '')}`, e);
      }
      for (const [k, u] of uniformsOf(m)) {
        const e = enc(u.value);
        if (e !== undefined) out.set(`m${j}.u.${k}`, e);
      }
    });
    for (const [t, key] of texKey) {
      out.set(`${key}.offset`, enc(t.offset));
      out.set(`${key}.repeat`, enc(t.repeat));
      out.set(`${key}.rotation`, enc(t.rotation));
      out.set(`${key}.center`, enc(t.center));
    }
    return out;
  }

  // Where the frames put the camera: at the line, by the hairpin, at the
  // Corkscrew.
  const t = world.track;
  const mid = (name) => { const g = t.tag(name)[0]; return (g.s0 + g.s1) / 2; };
  const st = (s) => { const f = t.frame(s, {}); return [f.x, f.y + 6, f.z]; };
  const at = { line: st(20), hairpin: st(mid('hairpin')), corkscrew: st(mid('corkscrew')) };
  const PLACES = ['line', 'hairpin', 'corkscrew'];
  // (dt, s, where, cd): uneven steps round the lap and past the line, the
  // countdown lighting each column in turn, GO, racing, then a fresh start.
  const FRAMES = [
    [0.0, 0, 'line', 3.999], [1 / 120, 0, 'line', 3.5], [1 / 60, 40, 'hairpin', 3.2], [0.25, 300, 'line', 2.7],
    [0.5, 600, 'hairpin', 2.4], [2, 1200, 'corkscrew', 1.6], [1 / 30, 1800, 'line', 1.2], [0.1, 2200, 'hairpin', 0.8],
    [3.3, 2560, 'corkscrew', 0.3], [0.7, 3000, 'line', 0.0001], [1 / 120, 3400, 'hairpin', 0], [1.25, 3594, 'corkscrew', -1],
    [0.05, 3700, 'line', -1], [0.333, 5000, 'hairpin', 3.999], [2, 7000, 'corkscrew', 2], [0.016, 10700, 'line', -1],
  ];
  // Then fixed ticks with the player moving at 60 m/s, the camera hopping
  // and the countdown running from 4 to 0.
  const TICKS = 360, TICK = 1 / 120, S0 = 1000;
  const cdAt = (k) => 4 - (k + 1) / 90;
  const focus = new THREE.Vector3();
  const step = (dt, s, where, cd, look = true) => {
    camera.position.fromArray(at[where]);
    focus.fromArray(at[where]);
    world.update(dt, s, focus, camera);
    world.onCountdown?.(cd);
    return look ? snapshot(false) : null;
  };

  // A frozen frame first (dt 0 at the line, as the menu's, but at a fixed
  // place: the menu's own drifts with the clock), so that the first
  // snapshot is the same wherever the world was built. It does not call
  // onCountdown: the lights are as built, dark.
  camera.position.fromArray(at.line);
  focus.fromArray(at.line);
  world.update(0, 0, focus, camera);
  const base = snapshot(true);
  const frames = FRAMES.map(([dt, s, where, cd]) => ({ dt, s, where, cd, snap: step(dt, s, where, cd) }));
  const ticks = [];
  for (let k = 0; k < TICKS; k++) {
    const where = PLACES[Math.floor(k / 40) % 3];
    const s = S0 + k * TICK * 60;
    ticks.push({ s, where, snap: step(TICK, s, where, cdAt(k)) });
  }

  // The keys that changed, in the order snapshot() lists them.
  const changed = (key, v) => v !== base.get(key);
  const keys = [...base.keys()].filter((key) => frames.some((f) => changed(key, f.snap.get(key))) || ticks.some((f) => changed(key, f.snap.get(key))));
  const value = async (v) => (typeof v === 'string' ? v : sha(v));
  const row = async (snap) => { const r = []; for (const key of keys) r.push(await value(snap.get(key))); return r; };
  const digest = async (r) => hex(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(keys.map((key, i) => `${key}=${r[i]}`).join('\n'))));
  const targets = {};
  for (const key of keys) {
    const id = key.split('.')[0];
    if (targets[id]) continue;
    if (id[0] === 'n') { const o = nodes[+id.slice(1)]; targets[id] = `${o.isInstancedMesh ? 'InstancedMesh' : o.type} ${o.name}`.trim(); }
    else { const m = mats[+id.slice(1)]; targets[id] = `${m.type} ${m.userData.kind ?? ''}`.trim(); }
  }
  const out = {
    camera: Object.fromEntries(Object.entries(at).map(([k, p]) => [k, p.map(bits)])),
    fov: bits(camera.fov),
    viewportHeight: bits(renderer.domElement.height),
    targets,
    keys,
    base: keys.map((key) => base.get(key)),
    frames: [],
    ticks: { s0: bits(S0), dt: bits(TICK), count: TICKS, speed: bits(60), cd: '4 - (k + 1) / 90', digests: [] },
  };
  for (const f of frames) out.frames.push({ dt: bits(f.dt), s: bits(f.s), at: f.where, cd: bits(f.cd), values: await row(f.snap) });
  for (const f of ticks) out.ticks.digests.push(await digest(await row(f.snap)));
  return out;
}

async function captureBrowser() {
  const { launch, openGame } = await import('../../test/e2e/harness.js');
  const browser = await launch();
  try {
    const game = await openGame(browser, { query: `level=${LEVEL}&kernel=1&freeze=1&s=0`, init: seedRandom, initArgs: [RANDOM_SEED] });
    const state = await game.eval(() => ({ id: window.__world?.level?.id, kernel: !!window.__parity?.kernel, freeze: !!window.__parity?.freeze }));
    if (state.id !== LEVEL || !state.kernel || !state.freeze) throw new Error(`page state ${JSON.stringify(state)}`);
    const out = await game.eval(run, null);
    const errors = [...game.errors, ...(await game.eval('(window.__parity?.errors || []).map(String)'))];
    if (errors.length) throw new Error('page errors: ' + errors.slice(0, 3).join(' | '));
    await game.close();
    return out;
  } finally {
    await browser.close();
  }
}

// World.build under Node: the game's modules, a canvas that accepts every
// call and draws nothing, a renderer that is only its drawing buffer's
// height (the harness's desktop page) and the exposure the sky sets.
async function captureNode() {
  const { kernelInstalled } = await import('../../src/parity/kernel.js');
  if (!kernelInstalled()) {
    console.error('seaside-animators: run with --import=./tools/parity/kernel/register.mjs (or --browser)');
    process.exit(1);
  }
  seedRandom(RANDOM_SEED);
  const image = (w, h) => ({ width: w, height: h, data: new Uint8ClampedArray(Math.max(0, w * h * 4)) });
  const ctx = new Proxy({}, {
    get: (_, k) => {
      if (k === 'createImageData' || k === 'getImageData') return (...a) => image(a.at(-2), a.at(-1));
      if (k === 'measureText') return () => ({ width: 0 });
      if (k === 'createLinearGradient' || k === 'createRadialGradient' || k === 'createPattern') return () => ({ addColorStop() {} });
      return () => {};
    },
    set: () => true,
  });
  globalThis.document = { createElement: () => ({ width: 0, height: 0, getContext: () => ctx, style: {} }) };
  await import('../../test/unit/support/three.js');
  const THREE = await import('three');
  const { LEVELS } = await import('../../test/unit/support/levels.js');
  const { World } = await import('../../src/world/World.js');
  // The aerial photo: nothing reads its pixels, and Node has no image
  // loader.
  THREE.TextureLoader.prototype.loadAsync = async () => new THREE.Texture();
  const level = LEVELS.find((l) => l.id === LEVEL);
  const renderer = { toneMappingExposure: 1, domElement: { height: 800 } };
  const world = new World(new THREE.Scene(), renderer, level);
  const { warn, error } = console;
  const said = [];
  console.warn = console.error = (...a) => said.push(a.map(String).join(' '));
  try {
    await world.build();
  } finally {
    Object.assign(console, { warn, error });
  }
  // blobGeometry calls toNonIndexed on an icosahedron, which already is:
  // three warns and returns it (the game does the same).
  const complaints = said.filter((m) => !m.includes('BufferGeometry is already non-indexed'));
  if (complaints.length) throw new Error('the build complained: ' + complaints.slice(0, 3).join(' | '));
  // main.js's camera.
  const camera = new THREE.PerspectiveCamera(62, 1280 / 800, 0.3, 9000);
  return run({ THREE, world, camera });
}

const capture = () => (BROWSER ? captureBrowser() : captureNode());

// One frame's values per line, so the file diffs by frame.
function text(r) {
  const head = {
    note: 'Generated by tools/parity/seaside-animators.mjs: Seaside Raceway\'s world.update and world.onCountdown over fixed frames (kernel on), every value they change under world.root. Numbers are hex f64 bits; arrays the SHA-256 of their bytes. Do not edit.',
    camera: r.camera, fov: r.fov, viewportHeight: r.viewportHeight, targets: r.targets,
  };
  const lines = [JSON.stringify(head, null, 1).slice(0, -2) + ','];
  lines.push(' "keys": ' + JSON.stringify(r.keys) + ',');
  lines.push(' "base": ' + JSON.stringify(r.base) + ',');
  lines.push(' "frames": [');
  r.frames.forEach((f, k) => lines.push('  ' + JSON.stringify(f) + (k + 1 < r.frames.length ? ',' : '')));
  lines.push(' ],');
  const { digests, ...tk } = r.ticks;
  lines.push(' "ticks": ' + JSON.stringify(tk).slice(0, -1) + ', "digests": [');
  digests.forEach((d, k) => lines.push('  ' + JSON.stringify(d) + (k + 1 < digests.length ? ',' : '')));
  lines.push(' ]}');
  lines.push('}');
  return lines.join('\n') + '\n';
}

const first = text(await capture());
JSON.parse(first);
if (CHECK) {
  let bad = 0;
  if (BROWSER && text(await capture()) !== first) { bad++; console.log('  two captures differ'); }
  if (!fs.existsSync(OUT) || fs.readFileSync(OUT, 'utf8') !== first) { bad++; console.log(`  ${path.relative(ROOT, OUT)}: would change`); }
  if (bad) process.exit(1);
  console.log(`seaside-animators: ${BROWSER ? 'two captures in the game' : 'the Node capture'} and the golden agree`);
} else {
  fs.mkdirSync(path.dirname(OUT), { recursive: true });
  fs.writeFileSync(OUT, first);
  const j = JSON.parse(first);
  console.log(`seaside-animators: ${j.keys.length} values over ${j.frames.length} frames and ${j.ticks.count} ticks, ${Object.keys(j.targets).length} targets, in ${path.relative(ROOT, OUT)}`);
}
