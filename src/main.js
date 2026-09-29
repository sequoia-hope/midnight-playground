import * as THREE from 'three';
import { EffectComposer } from 'three/addons/postprocessing/EffectComposer.js';
import { RenderPass } from 'three/addons/postprocessing/RenderPass.js';
import { UnrealBloomPass } from 'three/addons/postprocessing/UnrealBloomPass.js';
import { OutputPass } from 'three/addons/postprocessing/OutputPass.js';
import { World } from './world/World.js';
import { buildVehicle } from './vehicles/CarModel.js';
import { CAR_SPECS } from './vehicles/CarPhysics.js';
import { GameAudio } from './game/Audio.js';
import { Input } from './game/Input.js';
import { TouchControls, isTouchDevice } from './game/TouchControls.js';
import { Race } from './game/Race.js';
import { fmtTime } from './game/HUD.js';
import { LEVELS, levelById } from './levels/index.js';
import { clamp, wrapAngle } from './util/math.js';

// ── Settings (per-viewer conveniences only) ───────────────────────
const params = new URLSearchParams(location.search);
const store = {
  get(k, d) { try { const v = localStorage.getItem('mr.' + k); return v === null ? d : JSON.parse(v); } catch { return d; } },
  set(k, v) { try { localStorage.setItem('mr.' + k, JSON.stringify(v)); } catch { /* private mode */ } },
};
// Phones and tablets get on-screen controls and lighter rendering by default.
const touchUI = isTouchDevice(params);
document.body.classList.toggle('touch', touchUI);
const settings = {
  music: store.get('musicVol', 0.7),
  sfx: store.get('sfxVol', 0.85),
  mph: store.get('mph', true),
  hq: store.get('hq', !touchUI),
  autogas: store.get('autogas', false),
  fullscreen: store.get('fullscreen', true),
  car: store.get('car', 'sports'),
  level: store.get('level', 'sierra'),
  track: store.get('track', 'auto'), // music: 'auto' = the level's own track
};
if (params.has('level')) settings.level = params.get('level');

// ── Renderer ──────────────────────────────────────────────────────
const canvas = document.getElementById('game');
const renderer = new THREE.WebGLRenderer({ canvas, antialias: false, powerPreference: 'high-performance' });
renderer.toneMapping = THREE.ACESFilmicToneMapping;
renderer.shadowMap.type = THREE.PCFSoftShadowMap;

const scene = new THREE.Scene();
const camera = new THREE.PerspectiveCamera(62, window.innerWidth / window.innerHeight, 0.3, 9000);
const rt = new THREE.WebGLRenderTarget(1, 1, { type: THREE.HalfFloatType, samples: 4 });
const composer = new EffectComposer(renderer, rt);
composer.addPass(new RenderPass(scene, camera));
const bloom = new UnrealBloomPass(new THREE.Vector2(1, 1), 0.38, 0.35, 0.92);
composer.addPass(bloom);
composer.addPass(new OutputPass());

let race = null;
let world = null;
function applyQuality() {
  const pr = settings.hq ? Math.min(window.devicePixelRatio, 1.5) : 1;
  renderer.setPixelRatio(pr);
  composer.setPixelRatio(pr);
  renderer.shadowMap.enabled = settings.hq;
  resize();
}
function resize() {
  const w = window.innerWidth, h = window.innerHeight;
  renderer.setSize(w, h);
  composer.setSize(w, h);
  camera.aspect = w / h;
  camera.updateProjectionMatrix();
  race?.effects.resize(h * renderer.getPixelRatio(), camera.fov);
}
window.addEventListener('resize', resize);
applyQuality();

const $ = (id) => document.getElementById(id);
const screens = ['loading', 'menu', 'pause', 'results'];
function showScreen(name) {
  for (const s of screens) $(s).classList.toggle('hidden', s !== name);
  // On-screen controls only while driving; the turn-sideways hint on the menu.
  touch?.show(!name && !!race);
  $('rotate-hint').classList.toggle('hidden', !touchUI || name !== 'menu');
}

// ── Environment map from the live sky ─────────────────────────────
const pmrem = new THREE.PMREMGenerator(renderer);
const envScene = new THREE.Scene();
let envDome = null, envAt = -1, envTex = null;
function refreshEnv(p) {
  if (envAt >= 0 && Math.abs(p - envAt) < 0.025) return;
  envAt = p;
  const next = pmrem.fromScene(envScene, 0.04, 0.1, 200).texture;
  envTex?.dispose();
  envTex = next;
  scene.environment = envTex;
  scene.environmentIntensity = 0.7;
}

// ── Levels: build (and tear down) worlds ──────────────────────────
const attract = { s: 120 };
let loading = null;
async function loadLevel(id) {
  const level = levelById(id);
  while (loading) await loading;
  if (world && world.level.id === level.id) return world;
  loading = (async () => {
    race?.dispose(); race = null;
    world?.dispose(); world = null;
    const prevMode = mode;
    mode = 'loading';
    showScreen('loading');
    $('load-fill').style.width = '0%';
    const w = new World(scene, renderer, level);
    await w.build((label, f) => {
      $('load-fill').style.width = (f * 100).toFixed(1) + '%';
      $('load-label').textContent = label;
    });
    world = w;
    window.__world = w;
    if (envDome) envScene.remove(envDome);
    envDome = new THREE.Mesh(w.sky.dome.geometry, w.sky.dome.material);
    envDome.scale.setScalar(50);
    envScene.add(envDome);
    envAt = -1;
    if (params.has('t')) w.sky.override = Number(params.get('t'));
    attract.s = w.track.startS + 60;
    mode = prevMode === 'loading' ? 'menu' : prevMode;
  })();
  await loading;
  loading = null;
  return world;
}

const audio = new GameAudio();
const input = new Input();
const touch = touchUI ? new TouchControls(input) : null;
input.touch = touch;
if (touch) touch.autoGas = settings.autogas;
window.__audio = audio;
// Test hooks: headless checks raycast and inspect with these.
window.__camera = camera;
window.__THREE = THREE;
function applyVolume() {
  audio.setVolume({ music: settings.music, sfx: settings.sfx });
  audio.setMusic(settings.music > 0.001);
}

// ── Music: level track, picker, now playing, next track (T) ──────
let musicKey = null;
// Only switch when the level or the choice changed, so a restart (or a
// skipped-to track) keeps playing.
function pickMusic() {
  const key = settings.level + '|' + settings.track;
  if (key === musicKey) return;
  musicKey = key;
  audio.playTrack(settings.track === 'auto' ? GameAudio.levelTrack(settings.level) : settings.track);
}
let npTimer = 0;
function showNowPlaying(info, force = false) {
  if (!info) return;
  $('np-pause').textContent = `♪ ${info.title} · ${info.style}`;
  if (settings.music <= 0.001 && !force) return;
  const el = $('np-toast');
  el.replaceChildren(`♪ ${info.title}`, Object.assign(document.createElement('small'), { textContent: info.style }));
  el.classList.add('show');
  clearTimeout(npTimer);
  npTimer = setTimeout(() => el.classList.remove('show'), 4000);
}
audio.onTrackChange = (info) => showNowPlaying(info);
function nextTrack() { if (audio.ready) showNowPlaying(audio.nextTrack(), true); }
$('btn-next-track').onclick = nextTrack;
window.addEventListener('keydown', (e) => { if (e.code === 'KeyT' && !e.repeat && mode !== 'loading') nextTrack(); });
const trackSel = $('opt-track');
trackSel.add(new Option('Level\u2019s own', 'auto'));
for (const t of GameAudio.tracks) trackSel.add(new Option(`${t.title} (${t.style})`, t.id));
if (![...trackSel.options].some((o) => o.value === settings.track)) settings.track = 'auto';
trackSel.value = settings.track;
trackSel.onchange = () => { settings.track = trackSel.value; store.set('track', settings.track); if (audio.ready) pickMusic(); };
// Audio only starts inside a user gesture, and on a phone the pointerdown
// of a tap isn't one (its pointerup, touchend and click are). So the first
// touch builds the audio, and every gesture restarts it until it runs.
// Nothing waits for it: a race starts whether or not the sound has.
function wakeAudio() { audio.init(); audio.unlock(); }
for (const t of ['pointerdown', 'pointerup', 'touchend', 'click', 'keydown']) {
  document.addEventListener(t, wakeAudio, { capture: true, passive: true });
}
// Menu buttons click.
document.addEventListener('click', (e) => {
  const b = e.target.closest?.('button');
  if (b) audio.uiClick(['btn-start', 'btn-again', 'btn-restart'].includes(b.id) ? 'start' : 'click');
});

// ── Menu ──────────────────────────────────────────────────────────
function levelStats(l) {
  if (l.mode === 'cruise') return 'Endless loop · heavy traffic · score attack';
  const len = l.segments.reduce((a, s) => a + s[0], 0) - (l.finishRunoff ?? 180);
  return `${(len / 1000).toFixed(1)} km · ${(len / 1609.34).toFixed(1)} mi · ${l.rivals.length} rivals · traffic`;
}
function renderLevelCard() {
  const l = levelById(settings.level);
  $('lvl-num').textContent = l.num;
  $('lvl-name').textContent = l.title;
  $('lvl-desc').textContent = l.desc;
  $('lvl-len').textContent = levelStats(l);
  const best = store.get(l.mode === 'cruise' ? 'bestScore.' + l.id : 'best.' + l.id, null);
  $('lvl-best').textContent = best == null ? '' : l.mode === 'cruise' ? `Best score ${Math.round(best).toLocaleString()}` : `Best winning time ${fmtTime(best)}`;
  $('btn-start').textContent = l.mode === 'cruise' ? 'Cruise' : 'Race';
}
const levelRow = $('level-pick');
for (const l of LEVELS) {
  const b = document.createElement('button');
  b.className = 'lvl-tab' + (settings.level === l.id ? ' sel' : '');
  b.innerHTML = `<small>${l.num}</small><b>${l.title}</b>`;
  b.onclick = async () => {
    if (mode !== 'menu') return;
    settings.level = l.id; store.set('level', l.id);
    levelRow.querySelectorAll('.lvl-tab').forEach((p) => p.classList.remove('sel'));
    b.classList.add('sel');
    renderLevelCard();
    if (audio.ready) pickMusic();
    // Build the new level behind the menu so it's ready when you start.
    await loadLevel(l.id);
    if (mode === 'menu') showScreen('menu');
  };
  levelRow.appendChild(b);
}
renderLevelCard();

const pickRow = $('car-pick');
for (const [key, spec] of Object.entries(CAR_SPECS)) {
  const b = document.createElement('button');
  b.className = 'pick' + (settings.car === key ? ' sel' : '');
  b.innerHTML = `<b><span class="sw" style="background:#${spec.color.toString(16).padStart(6, '0')}"></span>${spec.label}</b><small>${spec.blurb}</small>`;
  b.onclick = () => {
    settings.car = key; store.set('car', key);
    pickRow.querySelectorAll('.pick').forEach((p) => p.classList.remove('sel'));
    b.classList.add('sel');
  };
  pickRow.appendChild(b);
}
// Volume sliders appear in the menu and the pause screen; keep them in sync.
function bindSlider(cls, key, storeKey) {
  const els = document.querySelectorAll(cls);
  els.forEach((el) => {
    el.value = Math.round(settings[key] * 100);
    el.oninput = () => {
      settings[key] = el.value / 100;
      store.set(storeKey, settings[key]);
      els.forEach((o) => { if (o !== el) o.value = el.value; });
      if (audio.ready) applyVolume();
    };
  });
}
bindSlider('.vol-music', 'music', 'musicVol');
bindSlider('.vol-sfx', 'sfx', 'sfxVol');
for (const [id, key] of [['opt-mph', 'mph'], ['opt-hq', 'hq'], ['opt-autogas', 'autogas'], ['opt-fullscreen', 'fullscreen']]) {
  const el = $(id);
  el.checked = settings[key];
  el.onchange = () => {
    settings[key] = el.checked; store.set(key, el.checked);
    if (key === 'hq') applyQuality();
    if (key === 'mph' && race) race.hud.mph = el.checked;
    if (key === 'autogas' && touch) touch.autoGas = el.checked;
  };
}

// Phones: go fullscreen and hold landscape when a race starts. Must run
// inside the tap that starts it; every step is best-effort (iPhone Safari
// has no element fullscreen, most browsers refuse the orientation lock
// outside fullscreen).
function enterFullscreen() {
  if (!touchUI || !settings.fullscreen || document.fullscreenElement) return;
  const el = document.documentElement;
  const req = el.requestFullscreen || el.webkitRequestFullscreen;
  if (!req) return;
  try {
    Promise.resolve(req.call(el, { navigationUI: 'hide' }))
      .then(() => screen.orientation?.lock?.('landscape'))
      .catch(() => {});
  } catch { /* not allowed here */ }
}

// ── Session flow ──────────────────────────────────────────────────
let mode = 'loading';
window.__game = { get mode() { return mode; } }; // test hook
let starting = false;
async function startRace() {
  if (starting) return; // a double tap starts one race
  starting = true;
  try {
    enterFullscreen();
    audio.setPaused(false); // Restart from the pause screen
    wakeAudio();
    pickMusic();
    applyVolume();
    audio.setCar?.(settings.car);
    await loadLevel(settings.level);
    race?.dispose();
    race = new Race({
      world, scene, camera, renderer, input, audio, buildVehicle, carKind: settings.car,
      onFinish: showResults,
    });
    race.hud.mph = settings.mph;
    race.hud.bestScore = store.get('bestScore.' + world.level.id, 0);
    // Compile the new cars' shaders up front so the countdown doesn't
    // stutter, but never hold the start on it.
    const compiled = Promise.resolve().then(() => renderer.compileAsync(scene, camera)).catch(() => {});
    await Promise.race([compiled, new Promise((r) => setTimeout(r, 3000))]);
    race.effects.resize(window.innerHeight * renderer.getPixelRatio(), camera.fov);
    input.pressed.clear(); // keys pressed on the menu don't carry into the race
    input.enabled = true;
    mode = 'race';
    showScreen(null);
    $('btn-end').classList.toggle('hidden', !race.cruise);
    window.__race = race;
  } finally {
    starting = false;
  }
}
function pause(on) {
  if (!race || (on && mode !== 'race')) return;
  mode = on ? 'paused' : 'race';
  if (!on) input.pressed.clear(); // nor do keys pressed while paused
  if (on) showNowPlaying(audio.trackInfo, false);
  showScreen(on ? 'pause' : null);
  audio.setPaused(on);
}
function toMenu() {
  race?.dispose();
  race = null;
  mode = 'menu';
  showScreen('menu');
  audio.setPaused(false);
  audio.update?.(0.016, { rpm: 0, rpmMax: 7800, throttle: 0, gear: 0, speed: 0, skid: 0, nitro: false, onGround: true, scrape: 0 });
  audio.setRivalEngines?.([]);
  audio.setEnvironment?.('open');
  if (world) attract.s = world.track.startS + 60;
}
function showResults(res) {
  const id = world.level.id;
  if (res.cruise) {
    const prev = store.get('bestScore.' + id, 0);
    const isBest = res.score > prev;
    if (isBest) store.set('bestScore.' + id, res.score);
    $('res-title').textContent = isBest ? 'New best!' : 'Run over';
    const km = res.dist / 1000;
    const rows = [
      ['Score', res.score.toLocaleString()],
      ['Distance', settings.mph ? `${(km / 1.60934).toFixed(2)} mi` : `${km.toFixed(2)} km`],
      ['Top speed', settings.mph ? `${Math.round(res.top * 2.23694)} mph` : `${Math.round(res.top * 3.6)} km/h`],
      ['Near misses', res.nearMisses],
      ['Time', fmtTime(res.time)],
    ];
    $('res-table').innerHTML = rows.map(([a, b]) => `<tr><td></td><td>${a}</td><td>${b}</td></tr>`).join('');
    $('res-best').textContent = `Best score: ${Math.max(prev, res.score).toLocaleString()}`;
  } else {
    const me = res.find((r) => r.player);
    $('res-title').textContent = me.place === 1 ? 'You win!' : `${me.place}${['st', 'nd', 'rd'][me.place - 1] || 'th'} place`;
    $('res-table').innerHTML = res.map((r) => `<tr class="${r.player ? 'me' : ''}"><td>${r.place}</td><td><span class="sw" style="display:inline-block;width:10px;height:10px;border-radius:50%;margin-right:8px;background:#${r.color.toString(16).padStart(6, '0')}"></span>${r.name}</td><td>${r.estimated ? '~' : ''}${fmtTime(r.time)}</td></tr>`).join('');
    const best = store.get('best.' + id, null);
    if (me.place === 1 && (!best || me.time < best)) store.set('best.' + id, me.time);
    const b = store.get('best.' + id, null);
    $('res-best').textContent = b ? `Best winning time: ${fmtTime(b)}` : 'Win the race to set a best time';
  }
  renderLevelCard();
  mode = 'results';
  showScreen('results');
}
// Leaving the page (switching apps, locking the phone) pauses the race.
document.addEventListener('visibilitychange', () => { if (document.hidden && mode === 'race') pause(true); });
$('btn-start').onclick = startRace;
$('btn-resume').onclick = () => pause(false);
$('btn-restart').onclick = startRace;
$('btn-quit').onclick = toMenu;
$('btn-end').onclick = () => { if (race?.cruise) { audio.setPaused(false); showResults(race.cruiseResults()); } };
$('btn-again').onclick = startRace;
$('btn-menu').onclick = toMenu;

// ── Debug fly camera (?s=…) and attract mode ─────────────────────
const fly = params.has('s') ? {
  s: Number(params.get('s')), h: Number(params.get('h') ?? 5), back: Number(params.get('back') ?? 14),
  lat: Number(params.get('lat') ?? 0), speed: Number(params.get('v') ?? 0),
  yaw: Number(params.get('yaw') ?? 0), pitch: Number(params.get('pitch') ?? -0.08),
} : null;
window.__dbg = fly;

function flyCamera(dt, f) {
  const track = world.track;
  f.s = track.loop ? track.wrap(f.s + f.speed * dt) : Math.min(track.roadEnd - 1, f.s + f.speed * dt);
  const a = track.frame(track.loop ? f.s - f.back : Math.max(0, f.s - f.back));
  const b = track.frame(f.s + 20);
  camera.position.set(a.x + a.rx * f.lat, a.y + f.h, a.z + a.rz * f.lat);
  camera.lookAt(b.x, b.y + f.h * 0.4, b.z);
  camera.rotateY(f.yaw);
  camera.rotateX(f.pitch);
  return f.s;
}

// Simple autopilot for automated tests (?autodrive=1).
function autopilot(inp) {
  const v = race.player, t = world.track;
  const la = 10 + Math.hypot(v.vx, v.vz) * 0.35;
  const p = t.pointAt(v.s + la, t.racingLine[t.idx(v.s + la)] * 0.6);
  const want = Math.atan2(p.z - v.z, p.x - v.x);
  const err = wrapAngle(want - v.yaw);
  const target = t.speedProfile[t.idx(v.s + 15)] * 0.93;
  const sp = Math.hypot(v.vx, v.vz);
  inp.steer = clamp(err * 2.2, -1, 1);
  inp.throttle = sp < target ? 1 : 0;
  inp.brake = sp > target + 3 ? 1 : 0;
  inp.handbrake = false;
  inp.nitro = sp < target - 8 && Math.abs(err) < 0.1;
  return inp;
}

// ── Main loop ─────────────────────────────────────────────────────
const clock = new THREE.Clock();
const focus = new THREE.Vector3();
let lastError = '';
const timescale = Number(params.get('timescale') || 1); // test hook

// ?stats=1 — frame time, draw calls and triangles.
const statsEl = params.has('stats') ? Object.assign(document.createElement('div'), { id: 'stats' }) : null;
if (statsEl) {
  statsEl.style.cssText = 'position:fixed;left:8px;bottom:8px;font:12px ui-monospace,monospace;color:#9f9;background:rgba(0,0,0,.6);padding:4px 8px;border-radius:4px;z-index:9;pointer-events:none;white-space:pre';
  document.body.appendChild(statsEl);
}
let statT0 = performance.now(), statN = 0, statMs = 0;
window.__stats = {};
renderer.info.autoReset = false;
function updateStats(ms) {
  statN++; statMs += ms;
  const statT = performance.now() - statT0;
  if (statT < 500) return;
  const info = renderer.info;
  window.__stats = { fps: +(1000 * statN / statT).toFixed(1), cpuMs: +(statMs / statN).toFixed(2), calls: info.render.calls, tris: info.render.triangles, geos: info.memory.geometries, tex: info.memory.textures };
  if (statsEl) statsEl.textContent = `fps ${window.__stats.fps}  cpu ${window.__stats.cpuMs}ms\ncalls ${info.render.calls}  tris ${(info.render.triangles / 1e3).toFixed(0)}k`;
  statT0 = performance.now(); statN = 0; statMs = 0;
}

function frame() {
  requestAnimationFrame(frame);
  const t0 = performance.now();
  renderer.info.reset();
  try { tick(); updateStats(performance.now() - t0); } catch (e) {
    // Keep running; report each distinct error once.
    if (String(e) !== lastError) { lastError = String(e); console.error(e); }
  }
}
function tick() {
  const dt = Math.min(clock.getDelta(), 1 / 20) * timescale;
  let inp = input.update(dt);
  if (!world || mode === 'loading') return;
  const track = world.track;
  if (input.consume('pause') && (mode === 'race' || mode === 'paused')) pause(mode === 'race');
  if (input.consume('music') && audio.ready) {
    settings.music = settings.music > 0.001 ? 0 : 0.7;
    store.set('musicVol', settings.music);
    document.querySelectorAll('.vol-music').forEach((el) => { el.value = Math.round(settings.music * 100); });
    applyVolume();
  }

  let s;
  if (fly) {
    s = flyCamera(dt, fly);
    const f = track.frame(track.loop ? fly.s - fly.back : Math.max(0, fly.s - fly.back));
    focus.set(f.x, f.y, f.z);
  } else if (race && (mode === 'race' || mode === 'results')) {
    if (params.get('autodrive') === '1') inp = autopilot({ ...inp });
    race.update(dt, inp);
    s = race.player.s;
    focus.set(race.player.x, race.player.y, race.player.z);
  } else if (race && mode === 'paused') {
    s = race.player.s;
    focus.set(race.player.x, race.player.y, race.player.z);
  } else {
    // Attract mode behind the menu: drift slowly along the first zone.
    attract.s += dt * 16;
    const end = track.loop ? track.length : track.zones[0].s1 - 200;
    if (attract.s > end) attract.s = track.startS + 60;
    s = flyCamera(dt, { s: attract.s, h: 7, back: 22, lat: 3, speed: 0, yaw: 0, pitch: -0.05 });
    const f = track.frame(attract.s);
    focus.set(f.x, f.y, f.z);
  }
  world.update(dt, s, focus, camera);
  refreshEnv(world.sky.override ?? (track.loop ? 0.5 : s / track.length));
  composer.render();
}

requestAnimationFrame(frame);
await loadLevel(settings.level);
mode = 'menu';
showScreen(fly ? null : 'menu');
if (params.has('autostart')) { if (CAR_SPECS[params.get('autostart')]) settings.car = params.get('autostart'); startRace(); }
window.__ready = true;
