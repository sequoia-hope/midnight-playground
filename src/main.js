// Parity hooks for the Rust port (off unless asked for); first, so they run
// before any other game module.
import { parity, streamsForRace } from './parity/hooks.js';
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
import { TiltSteer } from './game/TiltSteer.js';
import { MenuNav } from './game/MenuNav.js';
import { PadSetup } from './game/PadSetup.js';
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
  // Touch steering: 'stick' (analogue thumb stick), 'buttons' (◂ ▸) or
  // 'tilt'. Before there was a choice, tilt was a checkbox.
  steering: store.get('steering', store.get('tilt', false) ? 'tilt' : 'stick'),
  tiltSens: store.get('tiltSens', 0.5),
  pedals: store.get('pedals', 'slider'), // 'slider' (brake, gas and N₂O on one) or 'buttons'
  fullscreen: store.get('fullscreen', true),
  car: store.get('car', 'sports'),
  level: store.get('level', 'sierra'),
  track: store.get('track', 'auto'), // music: 'auto' = the level's own track
  flash: store.get('flash', true), // police lights strobe (off: they glow steadily)
  rumble: store.get('rumble', true), // gamepad
};
// Race or Hot Pursuit, per level (only levels with police offer the choice).
const modeFor = (l) => (l.police && store.get('mode.' + l.id, 'race') === 'pursuit' ? 'pursuit' : 'race');
// ?pursuit=1 / 0 forces it; ?heat=1..5 starts hotter; ?cops=N caps the units.
const forcePursuit = params.has('pursuit') ? params.get('pursuit') === '1' : null;
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
const screens = ['loading', 'menu', 'pause', 'results', 'padsetup'];
let screenNow = 'loading';
function showScreen(name) {
  screenNow = name;
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
const tilt = touch ? new TiltSteer() : null;
if (touch) Object.assign(touch, { autoGas: settings.autogas, tilt });
// Gamepads: remapped controllers keep their own maps (Controller screen).
Object.assign(input.pads, { maps: store.get('padMaps', {}), onSave: (m) => store.set('padMaps', m), rumbleOn: settings.rumble });
window.__audio = audio;
window.__pads = input.pads;
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
  if (l.laps) return `${l.laps} laps of ${(l.lapLength / 1000).toFixed(1)} km · ${(l.lapLength / 1609.34).toFixed(2)} mi · ${l.rivals.length} rivals · no traffic`;
  const len = l.segments.reduce((a, s) => a + s[0], 0) - (l.finishRunoff ?? 180);
  return `${(len / 1000).toFixed(1)} km · ${(len / 1609.34).toFixed(1)} mi · ${l.rivals.length} rivals · traffic`;
}
function renderLevelCard() {
  const l = levelById(settings.level);
  const m = modeFor(l);
  $('mode-pick').classList.toggle('hidden', !l.police);
  $('mode-pick').querySelectorAll('button').forEach((b) => b.classList.toggle('sel', b.dataset.mode === m));
  $('lvl-num').textContent = l.num;
  $('lvl-name').textContent = l.title;
  $('lvl-desc').textContent = l.desc;
  $('lvl-len').textContent = levelStats(l);
  const best = store.get(l.mode === 'cruise' ? 'bestScore.' + l.id : bestKey(l), null);
  const lap = l.laps ? store.get('bestLap.' + l.id, null) : null;
  $('lvl-best').textContent = [
    best == null ? '' : l.mode === 'cruise' ? `Best score ${Math.round(best).toLocaleString()}` : `Best winning time ${fmtTime(best)}`,
    lap == null ? '' : `Lap record ${fmtTime(lap)}`,
  ].filter(Boolean).join(' · ');
  $('btn-start').textContent = l.mode === 'cruise' ? 'Cruise' : m === 'pursuit' ? 'Hot Pursuit' : 'Race';
}
// Winning times are kept apart for Hot Pursuit.
const bestKey = (l) => 'best.' + l.id + (modeFor(l) === 'pursuit' ? '.pursuit' : '');
$('mode-pick').querySelectorAll('button').forEach((b) => {
  b.onclick = () => {
    const l = levelById(settings.level);
    if (!l.police) return;
    store.set('mode.' + l.id, b.dataset.mode);
    renderLevelCard();
  };
});
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
for (const [id, key] of [['opt-mph', 'mph'], ['opt-hq', 'hq'], ['opt-autogas', 'autogas'], ['opt-fullscreen', 'fullscreen'], ['opt-flash', 'flash'], ['opt-rumble', 'rumble']]) {
  const el = $(id);
  el.checked = settings[key];
  el.onchange = () => {
    settings[key] = el.checked; store.set(key, el.checked);
    if (key === 'hq') applyQuality();
    if (key === 'mph' && race) race.hud.mph = el.checked;
    if (key === 'autogas' && touch) touch.autoGas = el.checked;
    if (key === 'flash' && race?.pv) race.pv.flash = race.pv.pursuit.flash = el.checked;
    if (key === 'rumble') { input.pads.rumbleOn = el.checked; input.pads.kick(0.5, 0.7, 300); } // a buzz to show it's on
  };
}

// Steering choice. With tilt, the sensitivity slider shows, and a line
// under the options says why tilt isn't steering, when it isn't.
const TILT_NOTES = {
  insecure: 'Tilt needs the game\u2019s https:// address; steering with the thumb stick',
  none: 'No tilt sensor answered; steering with the thumb stick',
  ask: 'Tap Race to allow motion access',
  denied: 'Motion access was turned down; steering with the thumb stick',
};
function showTiltState() {
  const on = settings.steering === 'tilt';
  $('tilt-sens-row').classList.toggle('hidden', !on || !tilt);
  $('tilt-note').textContent = (on && TILT_NOTES[tilt?.state]) || '';
}
if (touch) {
  const sel = $('opt-steer');
  if (![...sel.options].some((o) => o.value === settings.steering)) settings.steering = 'stick';
  sel.value = settings.steering;
  sel.onchange = () => {
    settings.steering = sel.value; store.set('steering', sel.value);
    touch.setMode(sel.value);
    tilt.enable(sel.value === 'tilt'); // an iPhone may take this as the tap to ask in
    showTiltState();
  };
  const ped = $('opt-pedals');
  if (![...ped.options].some((o) => o.value === settings.pedals)) settings.pedals = 'slider';
  ped.value = settings.pedals;
  ped.onchange = () => { settings.pedals = ped.value; store.set('pedals', ped.value); touch.setPedals(ped.value); };
  touch.setPedals(settings.pedals);
  const sens = $('opt-tilt-sens');
  sens.value = Math.round(settings.tiltSens * 100);
  sens.oninput = () => { settings.tiltSens = sens.value / 100; store.set('tiltSens', settings.tiltSens); tilt.setSensitivity(settings.tiltSens); };
  tilt.setSensitivity(settings.tiltSens);
  tilt.onChange = showTiltState;
  touch.setMode(settings.steering);
  if (settings.steering === 'tilt') tilt.enable(true);
  showTiltState();
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
    if (settings.steering === 'tilt') tilt?.enable(true); // an iPhone asks for motion access in this tap
    audio.setPaused(false); // Restart from the pause screen
    wakeAudio();
    pickMusic();
    applyVolume();
    audio.setCar?.(settings.car);
    await loadLevel(settings.level);
    race?.dispose();
    const lvl = world.level;
    const pursuit = lvl.police && (forcePursuit ?? modeFor(lvl) === 'pursuit') ? {
      heat: Number(params.get('heat') || 1), cops: params.has('cops') ? Number(params.get('cops')) : 6,
      flash: settings.flash, hq: settings.hq,
    } : null;
    race = new Race({
      world, scene, camera, renderer, input, audio, buildVehicle, carKind: settings.car,
      onFinish: showResults, pursuit, rngs: streamsForRace(),
    });
    race.hud.mph = settings.mph;
    race.hud.bestScore = store.get('bestScore.' + world.level.id, 0);
    // Compile the new cars' shaders up front so the countdown doesn't
    // stutter, but never hold the start on it.
    const compiled = Promise.resolve().then(() => renderer.compileAsync(scene, camera)).catch(() => {});
    await Promise.race([compiled, new Promise((r) => setTimeout(r, 3000))]);
    race.effects.resize(window.innerHeight * renderer.getPixelRatio(), camera.fov);
    input.pressed.clear(); // keys pressed on the menu don't carry into the race
    input.pads.hush(); // nor does the A that started it
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
  if (!on) { input.pressed.clear(); input.pads.hush(); } // nor do keys pressed while paused
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
    const key = res.pursuit ? 'best.' + id + '.pursuit' : 'best.' + id;
    const best = store.get(key, null);
    if (me.place === 1 && (!best || me.time < best)) store.set(key, me.time);
    const b = store.get(key, null);
    $('res-best').textContent = b ? `Best winning time: ${fmtTime(b)}` : 'Win the race to set a best time';
  }
  // Hot Pursuit: what the police cost you (and what you cost them).
  const p = !res.cruise && res.pursuit;
  // Circuits: each lap's time, and the best lap you've ever done here.
  const lp = !res.cruise && res.laps;
  let stats = null;
  if (p) {
    stats = [
      ['Busted', p.busts], ['Wrecked', p.wrecks], ['Takedowns', p.takedowns],
      ['Penalty', `+${p.penalty.toFixed(1)} s`], ['Top heat', '★'.repeat(p.heat)],
    ];
  } else if (lp && lp.times.length) {
    const prev = store.get('bestLap.' + id, null);
    if (lp.best != null && (prev == null || lp.best < prev)) store.set('bestLap.' + id, lp.best);
    stats = lp.times.map((t, i) => [`Lap ${i + 1}${t === lp.best ? ' ★' : ''}`, fmtTime(t)]);
    stats.push([prev == null || lp.best < prev ? 'New lap record' : 'Lap record', fmtTime(store.get('bestLap.' + id, null))]);
  }
  $('res-extra').innerHTML = stats ? stats.map(([a, b]) => `<div class="res-stat"><b>${b}</b><small>${a}</small></div>`).join('') : '';
  $('res-extra').classList.toggle('hidden', !stats);
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

// ── Gamepad: menus and the Controller screen ─────────────────────
let padReturn = 'menu';
const padSetup = new PadSetup(input.pads, $('padsetup'), { onClose: () => showScreen(padReturn) });
function openPadSetup() {
  if (screenNow !== 'menu' && screenNow !== 'pause') return;
  padReturn = screenNow;
  input.consume('pause'); // an Esc left over from the menu would close it at once
  showScreen('padsetup');
  padSetup.show();
}
$('btn-pad').onclick = openPadSetup;
$('btn-pad-pause').onclick = openPadSetup;
// B goes back a screen; Start races from the menu and the results.
const menuNav = new MenuNav({
  root: () => (screenNow && screenNow !== 'loading' ? $(screenNow) : null),
  back: () => {
    if (screenNow === 'pause') pause(false);
    else if (screenNow === 'results') toMenu();
    else if (screenNow === 'padsetup') padSetup.close();
  },
  start: () => { if (screenNow === 'menu' || screenNow === 'results') startRace(); },
});
let padShown = false;

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

// Frame time comes from the rAF timestamp, which marks the display's frame:
// a steady 60 Hz gives a steady step. Reading the clock whenever the callback
// happens to run adds the page's own scheduling jitter to every move, and at
// speed that shows as the world stuttering past.
let lastFrameT = null;
function frame(now) {
  requestAnimationFrame(frame);
  const t0 = performance.now();
  const frameDt = lastFrameT === null ? 0 : Math.max(0, now - lastFrameT) / 1000;
  lastFrameT = now;
  renderer.info.reset();
  try { tick(frameDt); updateStats(performance.now() - t0); } catch (e) {
    // Keep running; report each distinct error once.
    if (String(e) !== lastError) { lastError = String(e); console.error(e); }
    parity.errors.push(e); // a reference recording must not carry on past one
  }
}
function tick(frameDt) {
  // With the fixed-dt parity hook, game time is the ticks run, not the clock.
  const dt = parity.fixed ? parity.fixed.dt * parity.fixed.ticks : Math.min(frameDt, 1 / 20) * timescale;
  let inp = input.update(parity.fixed ? parity.fixed.dt : dt);
  const pads = input.pads.state;
  if (pads.connected !== padShown) { padShown = pads.connected; document.body.classList.toggle('pad', padShown); }
  // Esc, P or Start: on the Controller screen they leave it, not the pause.
  if (screenNow === 'padsetup' && input.consume('pause')) padSetup.escape();
  menuNav.update(pads.nav);
  if (padSetup.open) padSetup.update();
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
  } else if (race && (mode === 'race' || mode === 'results') && parity.fixed) {
    // Parity hook (?fixeddt=1): fixed ticks, the same number every frame,
    // input read per tick (the Rust input layer runs at the tick rate).
    for (let i = 0; i < parity.fixed.ticks; i++) {
      let ti = i === 0 ? inp : input.update(parity.fixed.dt);
      if (params.get('autodrive') === '1') ti = autopilot({ ...ti });
      if (parity.quantise) ti = parity.quantise(ti);
      race.update(parity.fixed.dt, ti);
      parity.onTick?.(race, ti);
    }
    s = race.player.s;
    focus.set(race.player.x, race.player.y, race.player.z);
  } else if (race && (mode === 'race' || mode === 'results')) {
    if (params.get('autodrive') === '1') inp = autopilot({ ...inp });
    if (parity.quantise) inp = parity.quantise(inp);
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
