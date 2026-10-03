// Material test scenes (roadmap WP 0.6, SPEC 6.2 "Verification"): each
// MaterialKind on a sphere and a plane under fixed light, plus a bloom
// chart, a fog ramp, a shadow edge and grids of the plain standard and
// physical materials, rendered by three.js through the game's own post
// chain. The Rust client renders the same scenes (M2.3) and the two are
// compared per pixel (cargo xtask parity shots), which isolates shading from
// geometry.
//
//   node tools/parity/materials.mjs [--run a]
//
// The scenes are data (SCENES below, written out to
// parity/golden/materials/scenes.json with each kind's source resolved), so
// the Rust side builds exactly what the JS built. A kind scene takes its
// material object from the live game: the first mesh with that kind in the
// level's scene graph (levels in the order below, captured as the scene
// export captures them: ?kernel=1&freeze=1&s=0), or from the models the
// export builds (cars, effects, pursuit props). Its test geometry carries
// every extra vertex attribute of the source mesh, set to the source's first
// vertex (and first instance) everywhere; a few kinds (OVERRIDES, geometry:
// 'source' or 'source-unscaled') use the source's own geometry instead,
// centred on the origin and, for 'source', scaled so its largest extent is 3.
//
// Images go to parity/cache/<js-tree-key>/materials/<run>/<group>/<scene>.png.

import fs from 'node:fs';
import path from 'node:path';
import { launch, openGame } from '../../test/e2e/harness.js';
import { cacheDir, ROOT } from './lib/jstree.mjs';
import { seedRandom, RANDOM_SEED } from './lib/seed-random.mjs';

const RUN = process.argv.includes('--run') ? process.argv[process.argv.indexOf('--run') + 1] : 'a';
const LEVELS = ['sierra', 'coast', 'streets', 'desert', 'seaside', 'cruise'];

// The common setup of every scene (the Rust side reads this too).
export const SETUP = {
  size: [512, 512],
  renderer: { toneMapping: 'ACESFilmic', exposure: 1, outputColorSpace: 'srgb', pixelRatio: 1, msaa: 4, target: 'HalfFloat' },
  bloom: { strength: 0.38, radius: 0.35, threshold: 0.92 },
  camera: { fov: 45, near: 0.1, far: 2000, position: [0, 1.6, 4.6], lookAt: [0, -0.3, 0] },
  sun: { color: 0xfff4e6, intensity: 3, position: [4, 6, 3], target: [0, 0, 0], shadow: { mapSize: 2048, box: 5, near: 0.5, far: 30, bias: -0.0004, normalBias: 0.6, type: 'PCFSoft' } },
  hemi: { sky: 0xbcd4ff, ground: 0x4a4030, intensity: 1 },
  // The level's own sky dome at the capture point, prefiltered as main.js
  // does (PMREM fromScene(envScene, 0.04, 0.1, 200), dome scaled 50).
  environment: { from: 'level sky dome', intensity: 0.7 },
  background: 0x202428,
  sphere: { radius: 1, widthSegments: 64, heightSegments: 32, position: [0, 0, 0] },
  plane: { size: 6, segments: 8, y: -1 },
};

// The scenes that need no game material.
export const FIXED_SCENES = [
  { name: 'bloom-chart', group: 'fixed', what: 'Eight unlit white quads at 0.25 to 20 times white on black: where bloom starts and how far it spreads',
    background: 0x000000, environment: null, lights: false, camera: { position: [0, 0, 9], lookAt: [0, 0, 0] },
    quads: [0.25, 0.5, 0.9, 1.0, 1.5, 3, 8, 20].map((v, i) => ({ value: v, x: -3.15 + i * 0.9, size: [0.5, 1.6] })) },
  { name: 'fog-ramp', group: 'fixed', what: 'White standard boxes from 5 m to 900 m in FogExp2 (0x8899aa, density 0.002), each 4° wide and in its own column, left (near) to right (far)',
    background: 0x8899aa, fog: { color: 0x8899aa, density: 0.002 }, camera: { position: [0, 0, 0], lookAt: [0, 0, -1] },
    boxes: [5, 15, 35, 70, 120, 200, 300, 450, 650, 900].map((d, i) => { const a = (-18 + i * 4) * Math.PI / 180; return { x: d * Math.sin(a), y: -d * 0.035, z: -d * Math.cos(a), size: d * 0.07 }; }) },
  { name: 'shadow-edge', group: 'fixed', what: 'A white standard sphere on a white plane, the sun low (from (6, 2.5, 1)): the soft shadow edge',
    sunPosition: [6, 2.5, 1], material: { type: 'Standard', color: 0xffffff, roughness: 0.8, metalness: 0 } },
  ...[0, 0.33, 0.66, 1].map((metal) => ({
    name: `standard-metal-${metal}`, group: 'fixed', what: `Standard spheres, metalness ${metal}, roughness 0.05 to 1 left to right`,
    camera: { position: [0, 2.2, 7.6], lookAt: [0, -0.4, 0] },
    grid: [0.05, 0.3, 0.6, 1].map((r, i) => ({ x: -2.4 + i * 1.6, material: { type: 'Standard', color: 0xb03a2e, roughness: r, metalness: metal } })),
  })),
  { name: 'physical-clearcoat-sheen', group: 'fixed', what: 'Physical spheres: clearcoat 0, 0.5, 1 (roughness 0.1), and sheen 0.6 (as the supercar paint)',
    camera: { position: [0, 2.2, 7.6], lookAt: [0, -0.4, 0] },
    grid: [
      { x: -2.4, material: { type: 'Physical', color: 0x1f4fd8, roughness: 0.36, metalness: 0.5, clearcoat: 0 } },
      { x: -0.8, material: { type: 'Physical', color: 0x1f4fd8, roughness: 0.36, metalness: 0.5, clearcoat: 0.5, clearcoatRoughness: 0.1 } },
      { x: 0.8, material: { type: 'Physical', color: 0x1f4fd8, roughness: 0.36, metalness: 0.5, clearcoat: 1, clearcoatRoughness: 0.1 } },
      { x: 2.4, material: { type: 'Physical', color: 0xf2b705, roughness: 0.3, metalness: 0.45, sheen: 0.6, clearcoat: 1, clearcoatRoughness: 0.1 } },
    ] },
];

// Kinds whose look comes from per-frame state (an animator's night level or
// clock, live particles, the siren) rather than from the material as
// captured: the test sets that state explicitly, and records it, so the
// Rust side sets the same. Uniform values are set on the compiled program's
// uniforms (a patch's own uniform objects).
export const OVERRIDES = {
  SkyGlow: { uniforms: { uK: 0.2 }, why: 'full night (City.js:1415)' },
  TrafficStreams: { uniforms: { uNight: 1, uTime: 2 }, attributes: { aDir: [0, 0, 0], aPar: [0.5, 0, 0] }, camera: { position: [0, 3, 90], lookAt: [0, 0, 0] },
    why: 'full night (City.js:1371); the points held still on the grid (aDir 0), or they fly off along their road; seen from 90 m, since they fade out within 25 to 70 m' },
  GlowPoints: { material: { color: [1.6, 1.6, 1.6] }, uniforms: { uMinPx: 16 }, why: 'full night (colour 1.6 × night, City.js:1540) and a 16 px minimum point size (the game uses 1.6 px, for glows seen from far off)' },
  Steam: { uniforms: { uTime: 1.5 }, why: 'a second and a half in (the steam animator)' },
  FloodBeam: { material: { opacity: 0.13 }, why: 'its night opacity (Desert.js:1874)' },
  GroundPool: { material: { opacity: 1 }, uniforms: { uTime: 1 }, why: 'night (Desert.js:1875), the flicker clock at 1 s' },
  LighthouseBeam: { uniforms: { uStrength: 1 }, geometry: 'source-unscaled', camera: { position: [0, 40, 240], lookAt: [0, 0, 0] },
    why: 'full strength, on its own cone at full size (the shade follows the beam\'s length) seen from 240 m (it fades out within 15 to 120 m)' },
  Particles: { attributes: { aSize: [1.5], aAlpha: [0.85], aColor: [1, 0.62, 0.3] }, why: 'a live particle (the buffers are empty until something spawns)' },
  SkidMarks: { attributes: { alpha: [0.6] }, background: 0x9a9a9a, why: 'a fresh mark (empty until a car skids), on a light background' },
  PoliceGlow: { uniforms: { uRed: [2.2, 0.15, 0.12], uBlue: [0.12, 0.3, 2.4] }, geometry: 'source', why: 'both siren colours lit (setSiren sets these per frame), on its own light-bar quads (billboards need their corner attribute)' },
};

// ── In the page ──────────────────────────────────────────────────────────
// Everything below runs in the page (serialised by game.eval): no closures
// over Node values.

async function pageSetup(SETUP) {
  const THREE = window.__THREE;
  const { EffectComposer } = await import('three/addons/postprocessing/EffectComposer.js');
  const { RenderPass } = await import('three/addons/postprocessing/RenderPass.js');
  const { UnrealBloomPass } = await import('three/addons/postprocessing/UnrealBloomPass.js');
  const { OutputPass } = await import('three/addons/postprocessing/OutputPass.js');
  const [W, H] = SETUP.size;
  const canvas = document.createElement('canvas');
  canvas.width = W; canvas.height = H;
  const renderer = new THREE.WebGLRenderer({ canvas, antialias: false, preserveDrawingBuffer: true, powerPreference: 'high-performance' });
  renderer.setPixelRatio(1);
  renderer.setSize(W, H, false);
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = SETUP.renderer.exposure;
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  const rt = new THREE.WebGLRenderTarget(W, H, { type: THREE.HalfFloatType, samples: SETUP.renderer.msaa });
  const composer = new EffectComposer(renderer, rt);
  composer.setPixelRatio(1);
  composer.setSize(W, H);
  const camera = new THREE.PerspectiveCamera(SETUP.camera.fov, W / H, SETUP.camera.near, SETUP.camera.far);
  const pass = new RenderPass(new THREE.Scene(), camera);
  composer.addPass(pass);
  composer.addPass(new UnrealBloomPass(new THREE.Vector2(W, H), SETUP.bloom.strength, SETUP.bloom.radius, SETUP.bloom.threshold));
  composer.addPass(new OutputPass());
  const w = window.__world;
  const envScene = new THREE.Scene();
  const dome = new THREE.Mesh(w.sky.dome.geometry, w.sky.dome.material);
  dome.scale.setScalar(50);
  envScene.add(dome);
  const env = new THREE.PMREMGenerator(renderer).fromScene(envScene, 0.04, 0.1, 200).texture;
  window.__mt = { THREE, renderer, composer, camera, pass, env, SETUP };
}

// The kind of a material, as the scene exporter names it.
function pageKindOf(m) {
  const B = { MeshStandardMaterial: 'Standard', MeshPhysicalMaterial: 'Physical', MeshLambertMaterial: 'Lambert', MeshBasicMaterial: 'Basic', LineBasicMaterial: 'Line', SpriteMaterial: 'Sprite', PointsMaterial: 'Points' };
  return m.userData?.kind ?? B[m.type] ?? null;
}

// Every drawable under root with its path (child indices) and kind, in
// traversal order.
function pageDrawables(root) {
  const out = [];
  const walk = (o, p) => {
    if (o.isMesh || o.isPoints || o.isLine || o.isSprite) {
      const mats = Array.isArray(o.material) ? o.material : [o.material];
      mats.forEach((m, i) => { const k = window.__mtKindOf(m); if (k) out.push({ obj: o, path: p, materialIndex: i, kind: k, name: o.name || '' }); });
    }
    o.children.forEach((c, i) => walk(c, [...p, i]));
  };
  walk(root, []);
  return out;
}

// The models group, built as the scene exporter builds it (names and order).
async function pageModels() {
  const THREE = window.__THREE, world = window.__world, camera = window.__camera;
  const url = (p) => new URL(p, document.baseURI).href;
  const CarModel = await import(url('src/vehicles/CarModel.js'));
  const { Effects } = await import(url('src/game/Effects.js'));
  const { sawhorseModel, spikeStrip } = await import(url('src/game/PursuitView.js'));
  const root = new THREE.Group();
  const add = (name, kind, opts) => { const h = CarModel.buildVehicle(kind, opts); const g = new THREE.Group(); g.name = name; g.add(h.root); root.add(g); };
  for (const kind of CarModel.VEHICLE_KINDS) { add(`car:${kind}:high`, kind, { lod: 'high', seed: 0 }); add(`car:${kind}:low`, kind, { lod: 'low', far: true, seed: 0 }); }
  for (const kind of ['muscle', 'sports']) { add(`car:${kind}:police:high`, kind, { lod: 'high', livery: 'police', seed: 0 }); add(`car:${kind}:police:low`, kind, { lod: 'low', livery: 'police', far: true, seed: 0 }); }
  const fx = new THREE.Group(); fx.name = 'effects'; root.add(fx);
  const effects = new Effects(fx, world.renderer, camera);
  const fxCar = CarModel.buildVehicle('sports', { lod: 'high', seed: 0 }); fxCar.root.name = 'effects-car'; fx.add(fxCar.root);
  effects.addCar({ model: fxCar });
  const props = new THREE.Group(); props.name = 'pursuit-props'; root.add(props);
  const saw = sawhorseModel(); saw.root.name = 'sawhorse'; props.add(saw.root);
  const spikes = spikeStrip(world.track, world.track.startS + 200, -4, 4); spikes.name = 'spike-strip'; props.add(spikes);
  root.updateMatrixWorld(true);
  return root;
}

// Test geometry with the source's extra attributes, each set to its first
// vertex's (or first instance's) value everywhere.
function pageWithAttrs(geo, src, over = {}) {
  const THREE = window.__THREE;
  const n = geo.attributes.position.count;
  for (const [name, a] of Object.entries(src.attributes)) {
    if (name === 'position' || name === 'normal' || (name === 'uv' && geo.attributes.uv)) continue;
    const k = a.itemSize, first = [];
    for (let j = 0; j < k; j++) first.push(over[name] ? over[name][j] : a.getComponent ? a.getComponent(0, j) : a.array[j]);
    if (a.isInstancedBufferAttribute) { geo.setAttribute(name, new THREE.InstancedBufferAttribute(new a.array.constructor(first), k, a.normalized)); continue; }
    const arr = new a.array.constructor(n * k);
    for (let i = 0; i < n; i++) for (let j = 0; j < k; j++) arr[i * k + j] = first[j];
    geo.setAttribute(name, new THREE.BufferAttribute(arr, k, a.normalized));
  }
  return geo;
}

function pageKindObjects(src, m, over = {}) {
  const { THREE, SETUP } = window.__mt;
  const S = SETUP.sphere, P = SETUP.plane;
  const g = src.geometry;
  if (window.__mtGeometry === 'source' || window.__mtGeometry === 'source-unscaled') {
    const geo = g.clone();
    geo.computeBoundingBox();
    const bb = geo.boundingBox, c = bb.getCenter(new THREE.Vector3()), sz = bb.getSize(new THREE.Vector3());
    const k = window.__mtGeometry === 'source' ? 3 / Math.max(sz.x, sz.y, sz.z, 1e-6) : 1;
    geo.translate(-c.x, -c.y, -c.z).scale(k, k, k);
    return [src.isPoints ? new THREE.Points(geo, m) : new THREE.Mesh(geo, m)];
  }
  if (src.isSprite) { const s = new THREE.Sprite(m); s.scale.set(2, 2, 1); return [s]; }
  if (src.isPoints) {
    const pos = [];
    for (let i = 0; i < 11; i++) for (let j = 0; j < 11; j++) pos.push(-2.5 + i * 0.5, -0.8 + j * 0.25, -j * 0.4);
    const geo = new THREE.BufferGeometry(); geo.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
    return [new THREE.Points(pageWithAttrs(geo, g, over), m)];
  }
  if (src.isLine) return [new THREE.LineSegments(pageWithAttrs(new THREE.WireframeGeometry(new THREE.SphereGeometry(S.radius, 24, 12)), g, over), m)];
  const make = (geo) => {
    pageWithAttrs(geo, g, over);
    let o;
    if (src.isInstancedMesh) {
      o = new THREE.InstancedMesh(geo, m, 1);
      o.setMatrixAt(0, new THREE.Matrix4());
      if (src.instanceColor) { const c = new THREE.Color(); src.getColorAt(0, c); o.setColorAt(0, c); }
    } else o = new THREE.Mesh(geo, m);
    o.castShadow = o.receiveShadow = true;
    return o;
  };
  const sphere = make(new THREE.SphereGeometry(S.radius, S.widthSegments, S.heightSegments));
  const plane = make(new THREE.PlaneGeometry(P.size, P.size, P.segments, P.segments).rotateX(-Math.PI / 2).translate(0, P.y, 0));
  return [sphere, plane];
}

function pageLights(scene, sunPosition) {
  const { THREE, SETUP } = window.__mt;
  const sun = new THREE.DirectionalLight(SETUP.sun.color, SETUP.sun.intensity);
  sun.position.fromArray(sunPosition ?? SETUP.sun.position);
  sun.target.position.fromArray(SETUP.sun.target);
  sun.castShadow = true;
  const sh = SETUP.sun.shadow;
  sun.shadow.mapSize.set(sh.mapSize, sh.mapSize);
  Object.assign(sun.shadow.camera, { left: -sh.box, right: sh.box, top: sh.box, bottom: -sh.box, near: sh.near, far: sh.far });
  sun.shadow.bias = sh.bias; sun.shadow.normalBias = sh.normalBias;
  scene.add(sun, sun.target, new THREE.HemisphereLight(SETUP.hemi.sky, SETUP.hemi.ground, SETUP.hemi.intensity));
}

function pageMaterial(d) {
  const THREE = window.__THREE;
  const { type, ...p } = d;
  return type === 'Physical' ? new THREE.MeshPhysicalMaterial(p) : new THREE.MeshStandardMaterial(p);
}

// Render one scene; returns a PNG data URL.
function pageRender(def, objects) {
  const { THREE, composer, camera, pass, env, SETUP } = window.__mt;
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(def.background ?? SETUP.background);
  if (def.environment !== null) { scene.environment = env; scene.environmentIntensity = SETUP.environment.intensity; }
  if (def.fog) scene.fog = new THREE.FogExp2(def.fog.color, def.fog.density);
  if (def.lights !== false) pageLights(scene, def.sunPosition);
  for (const o of objects) scene.add(o);
  const c = def.camera ?? SETUP.camera;
  camera.position.fromArray(c.position ?? SETUP.camera.position);
  camera.lookAt(new THREE.Vector3().fromArray(c.lookAt ?? SETUP.camera.lookAt));
  camera.updateMatrixWorld();
  pass.scene = scene;
  composer.render();
  return composer.renderer.domElement.toDataURL('image/png');
}

function pageFixed(def) {
  const { THREE, SETUP } = window.__mt;
  const objs = [];
  if (def.quads) for (const q of def.quads) { const m = new THREE.MeshBasicMaterial(); m.color.setScalar(q.value); const o = new THREE.Mesh(new THREE.PlaneGeometry(q.size[0], q.size[1]), m); o.position.x = q.x; objs.push(o); }
  if (def.boxes) for (const b of def.boxes) { const o = new THREE.Mesh(new THREE.BoxGeometry(b.size, b.size, b.size), new THREE.MeshStandardMaterial({ color: 0xffffff, roughness: 0.9 })); o.position.set(b.x, b.y, b.z); objs.push(o); }
  if (def.material || def.grid) {
    const S = SETUP.sphere, P = SETUP.plane;
    const plane = new THREE.Mesh(new THREE.PlaneGeometry(P.size * 2, P.size, P.segments, P.segments).rotateX(-Math.PI / 2).translate(0, P.y, 0), pageMaterial(def.material ?? { type: 'Standard', color: 0x808080, roughness: 0.9 }));
    plane.receiveShadow = true; objs.push(plane);
    for (const s of def.grid ?? [{ x: 0, material: def.material }]) {
      const o = new THREE.Mesh(new THREE.SphereGeometry(def.grid ? 0.7 : S.radius, S.widthSegments, S.heightSegments), pageMaterial(s.material));
      o.position.x = s.x; o.castShadow = o.receiveShadow = true; objs.push(o);
    }
  }
  return pageRender(def, objs);
}

// Kind scenes for every kind under root not already done.
function pageKinds(root, source, done, OVERRIDES) {
  const { THREE, renderer } = window.__mt;
  const out = [];
  const plain = (v) => (typeof v === 'number' || typeof v === 'boolean' ? v : v?.isTexture ? 'texture' : v?.toArray ? v.toArray() : Array.isArray(v) ? 'array' : v === null ? null : typeof v);
  for (const d of pageDrawables(root)) {
    if (done.includes(d.kind)) continue;
    done.push(d.kind);
    const m = Array.isArray(d.obj.material) ? d.obj.material[d.materialIndex] : d.obj.material;
    const over = OVERRIDES[d.kind] ?? null;
    window.__mtGeometry = over?.geometry ?? null;
    const objs = pageKindObjects(d.obj, m, over?.attributes ?? {});
    const def = { name: `kind-${d.kind}`, group: 'kinds', kind: d.kind, source, path: d.path, materialIndex: d.materialIndex, objectType: d.obj.type, meshName: d.name,
      attributes: Object.keys(d.obj.geometry?.attributes ?? {}), instanced: !!d.obj.isInstancedMesh, overrides: over,
      ...(over?.background !== undefined ? { background: over.background } : {}), ...(over?.camera ? { camera: over.camera } : {}) };
    let png = pageRender(def, objs); // also compiles the program
    // The uniforms the patch or shader adds (beyond three's own for the type).
    const props = renderer.properties.get(m);
    const compiled = props.uniforms ?? m.uniforms ?? {};
    const base = new Set(Object.keys(THREE.ShaderLib[{ MeshStandardMaterial: 'physical', MeshPhysicalMaterial: 'physical', MeshLambertMaterial: 'lambert', MeshBasicMaterial: 'basic', LineBasicMaterial: 'basic', SpriteMaterial: 'sprite', PointsMaterial: 'points' }[m.type]]?.uniforms ?? {}));
    def.patchUniforms = Object.fromEntries(Object.entries(compiled).filter(([k]) => !base.has(k)).map(([k, u]) => [k, plain(u.value)]));
    if (over && (over.uniforms || over.material)) {
      for (const [k, v] of Object.entries(over.uniforms ?? {})) {
        const u = compiled[k] ?? m.uniforms?.[k];
        if (!u) throw new Error(`${d.kind}: no uniform ${k} to override`);
        if (typeof v === 'number') u.value = v; else u.value.fromArray ? u.value.fromArray(v) : u.value.set(...v);
      }
      for (const [k, v] of Object.entries(over.material ?? {})) { if (m[k]?.fromArray) m[k].fromArray(v); else m[k] = v; }
      png = pageRender(def, objs);
    }
    out.push({ def, png });
  }
  return out;
}

// ── Node side ───────────────────────────────────────────────────────────
const outRoot = cacheDir(`materials/${RUN}`);
const scenes = [];
const save = (def, png) => {
  const dir = path.join(outRoot, def.group);
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, def.name + '.png'), Buffer.from(png.replace(/^data:image\/png;base64,/, ''), 'base64'));
  scenes.push(def);
};

const done = [];
const page = async (level, fn) => {
  const browser = await launch();
  try {
    const g = await openGame(browser, { query: `level=${level}&kernel=1&freeze=1&s=0`, init: seedRandom, initArgs: [RANDOM_SEED] });
    for (const f of [pageKindOf, pageDrawables, pageModels, pageWithAttrs, pageKindObjects, pageLights, pageMaterial, pageRender, pageFixed, pageKinds]) {
      await g.eval(`window.__mt_${f.name} = ${f}; window.${f.name} = window.__mt_${f.name};`);
    }
    await g.eval('window.__mtKindOf = pageKindOf;');
    await g.eval(`(${pageSetup})(${JSON.stringify(SETUP)})`);
    await fn(g);
    if (g.errors.length) throw new Error(`${level}: page errors: ${g.errors.join(' | ')}`);
    await g.close();
  } finally {
    await browser.close();
  }
};

// The fixed scenes, lit by Sierra's sky.
await page('sierra', async (g) => {
  for (const def of FIXED_SCENES) save(def, await g.eval(`pageFixed(${JSON.stringify(def)})`));
});
for (const level of LEVELS) {
  await page(level, async (g) => {
    const res = await g.eval(`pageKinds(window.__world.root, ${JSON.stringify({ level })}, ${JSON.stringify(done)}, ${JSON.stringify(OVERRIDES)})`);
    // The sky dome hangs off the scene, not the world's root.
    if (!done.includes('SkyDome')) res.push(...await g.eval(`pageKinds(window.__world.sky.dome, ${JSON.stringify({ level, sky: true })}, ${JSON.stringify(done)}, ${JSON.stringify(OVERRIDES)})`));
    for (const { def, png } of res) { done.push(def.kind); save(def, png); }
  });
}
await page('sierra', async (g) => {
  const res = await g.eval(`(async () => pageKinds(await pageModels(), { models: true, level: 'sierra' }, ${JSON.stringify(done)}, ${JSON.stringify(OVERRIDES)}))()`);
  for (const { def, png } of res) { done.push(def.kind); save(def, png); }
});

const golden = path.join(ROOT, 'parity/golden/materials/scenes.json');
fs.mkdirSync(path.dirname(golden), { recursive: true });
fs.writeFileSync(golden, JSON.stringify({ note: 'Generated by tools/parity/materials.mjs: the material test scenes, as data. Kind scenes take their material from the source (a level scene graph or the models group, by child-index path from the root, as the scene export walks it).', setup: SETUP, scenes }, null, 1) + '\n');
console.log(`${scenes.length} scenes (${scenes.filter((s) => s.group === 'kinds').length} kinds: ${done.join(', ')}) in ${path.relative(ROOT, outRoot)}`);
