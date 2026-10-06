// Engine Sound Lab page: builds the audio graph, runs the driver every frame,
// and feeds either the new model (B, worklet.js) or the current game engine
// (A, GameAudio from src/game/Audio.js, read-only) the same state.
//
//   B: engine-lab worklet ─┐
//   A: GameAudio limiter ──┴─ speaker-sim worklet ─ master volume ─ analyser ─ out
//
// GameAudio runs on our context (init({ context })) and its limiter, which
// it connects to the destination, is re-routed into the speaker simulation.

import { GameAudio, askForPlayback } from '../../src/game/Audio.js';
import { PRESETS, ORDER, TWEAKS, presetParams } from './presets.js';
import { Driver } from './drive.js';
import { WORKLET_URL, measureAll, measurePhone, renderModel, renderGame, analyse } from './measure.js';

const $ = (id) => document.getElementById(id);
const status = (html, err = false) => { $('status').innerHTML = html; $('status').classList.toggle('err', err); };
const GAME_NAMES = { muscle: 'muscle (cross-plane V8)', sports: 'sports (flat-plane V8)', super: 'super (V10)', rally: 'rally (turbo four)' };
const PSYCHO_DEFAULT = { headphones: 0, big: 0, laptop: 0.6, phone: 1 };

let key = ORDER[0];
let params = presetParams(key);
let source = 'B';
let ctx = null, engine = null, speaker = null, master = null, analyser = null, gameIn = null, game = null;
let playing = false;
const driver = new Driver();
driver.setPreset(params);

// ── Controls ─────────────────────────────────────────────────────
for (const k of ORDER) $('preset').add(new Option(PRESETS[k].name, k));
function bindRange(el, fmt = (v) => v) {
  const out = el.parentElement.querySelector('output');
  const show = () => { if (out) out.textContent = fmt(Number(el.value)); };
  el.addEventListener('input', show); show();
  return show;
}
const showRpm = bindRange($('rpm'), (v) => Math.round(v));
const showThr = bindRange($('thr'), (v) => Math.round(v * 100) + '%');
bindRange($('psycho'), (v) => Math.round(v * 100) + '%');
bindRange($('vol'), (v) => Math.round(v * 100) + '%');

const tweakShows = {};
for (const [k, label, min, max, step] of TWEAKS) {
  const l = document.createElement('label');
  l.className = 'sl';
  l.innerHTML = `${label} <input type="range" min="${min}" max="${max}" step="${step}" data-k="${k}"><output></output>`;
  $('tweaks').append(l);
  const el = l.querySelector('input');
  tweakShows[k] = () => { el.value = params[k] ?? 0; l.querySelector('output').textContent = +Number(el.value).toFixed(3); };
  el.addEventListener('input', () => {
    params[k] = Number(el.value);
    l.querySelector('output').textContent = +Number(el.value).toFixed(3);
    sendParams();
  });
}
const showTweaks = () => { for (const f of Object.values(tweakShows)) f(); };

function selectPreset(k) {
  key = k; params = presetParams(k);
  driver.setPreset(params);
  $('rpm').min = params.idle; $('rpm').max = params.redline;
  if (driver.mode === 'manual') { $('rpm').value = params.idle; driver.manualRpm = params.idle; }
  showRpm();
  $('presetNote').textContent = `Idle ${params.idle}, redline ${params.redline} rpm · ${params.events.length} cylinders · ${params.pipes === 1 ? 'one pipe' : 'twin pipes'}${params.turbo ? ` · ${params.turbo === 2 ? 'twin turbo' : 'turbo'}` : ''}.`;
  $('abNote').textContent = `A plays the game's ${GAME_NAMES[params.gameCar]}, the closest car it has.`;
  $('boostBox').hidden = !params.turbo;
  showTweaks();
  sendParams();
  if (game?.ready) game.setCar(params.gameCar);
}
$('preset').onchange = () => selectPreset($('preset').value);

function sendParams() { engine?.port.postMessage({ type: 'params', params }); }

$('rpm').addEventListener('input', () => { driver.manualRpm = Number($('rpm').value); if (driver.mode !== 'manual') setMode('manual'); });
$('thr').addEventListener('input', () => { driver.manualThr = Number($('thr').value); if (driver.mode !== 'manual') setMode('manual'); });

function setMode(m) {
  if (m === 'manual') { driver.manualRpm = driver.rpm; driver.manualThr = driver.thr; }
  driver.start(m);
  for (const [id, mm] of [['mManual', 'manual'], ['mRev', 'rev'], ['mDrive', 'drive']]) $(id).classList.toggle('on', m === mm);
}
$('mManual').onclick = () => setMode('manual');
$('mRev').onclick = async () => { setMode('rev'); if (!playing) await togglePlay(); };
$('mDrive').onclick = async () => { setMode('drive'); if (!playing) await togglePlay(); };

function setSource(s) {
  source = s;
  $('srcA').classList.toggle('on', s === 'A');
  $('srcB').classList.toggle('on', s === 'B');
  applyRouting();
}
$('srcA').onclick = () => setSource('A');
$('srcB').onclick = () => setSource('B');
addEventListener('keydown', (e) => {
  if (e.target.closest('input, select, textarea')) return;
  if (e.key === 'a' || e.key === 'A') setSource('A');
  if (e.key === 'b' || e.key === 'B') setSource('B');
  if (e.key === ' ') { e.preventDefault(); togglePlay(); }
});

$('speaker').onchange = () => {
  const s = $('speaker').value;
  speaker?.port.postMessage({ type: 'profile', name: s });
  $('psycho').value = PSYCHO_DEFAULT[s]; $('psycho').dispatchEvent(new Event('input'));
};
$('psycho').addEventListener('input', () => engine?.port.postMessage({ type: 'psycho', amount: Number($('psycho').value) }));
$('vol').addEventListener('input', () => { if (master) master.gain.setTargetAtTime(Number($('vol').value), ctx.currentTime, 0.03); });

$('copy').onclick = async () => {
  const json = JSON.stringify({ [key]: params }, null, 2);
  $('json').hidden = false; $('json').value = json;
  try { await navigator.clipboard.writeText(json); $('copyNote').textContent = 'Copied.'; } catch { $('copyNote').textContent = 'Select the text below and copy it.'; $('json').select(); }
};
$('reset').onclick = () => selectPreset(key);

// ── Audio ────────────────────────────────────────────────────────
async function ensureAudio() {
  if (ctx) return;
  const AC = window.AudioContext || window.webkitAudioContext;
  if (!AC) throw new Error('this browser has no Web Audio');
  // AudioWorklet only exists in a secure context: https, or localhost.
  if (!window.isSecureContext || !window.AudioWorkletNode) throw new Error('AudioWorklet needs a secure page (https or localhost): open this through the tailnet https address');
  askForPlayback();
  const ac = new AC({ latencyHint: 'balanced' });
  await ac.audioWorklet.addModule(WORKLET_URL);
  ctx = ac;
  engine = new AudioWorkletNode(ctx, 'engine-lab', {
    numberOfInputs: 0, outputChannelCount: [2],
    processorOptions: { params, state: { rpm: driver.rpm, throttle: 0 }, psycho: Number($('psycho').value) },
  });
  speaker = new AudioWorkletNode(ctx, 'speaker-sim', { outputChannelCount: [2], processorOptions: { profile: $('speaker').value } });
  master = ctx.createGain(); master.gain.value = Number($('vol').value);
  analyser = ctx.createAnalyser(); analyser.fftSize = 8192; analyser.smoothingTimeConstant = 0.75;
  gameIn = ctx.createGain(); gameIn.gain.value = 0;
  engine.connect(speaker); gameIn.connect(speaker);
  speaker.connect(master); master.connect(analyser); analyser.connect(ctx.destination);
  // The current game engine, on the same context.
  game = new GameAudio();
  await game.init({ context: ctx });
  game.setVolume({ master: 1, sfx: 1, music: 0 });
  game.setCar(params.gameCar);
  game.limiter.disconnect();
  game.limiter.connect(gameIn);
  window.engineLab.ctx = ctx; window.engineLab.game = game; window.engineLab.engine = engine;
}

function applyRouting() {
  if (!ctx) return;
  const t = ctx.currentTime;
  engine.port.postMessage({ type: 'run', on: playing && source === 'B' });
  gameIn.gain.setTargetAtTime(playing && source === 'A' ? 1 : 0, t, 0.03);
}

async function togglePlay() {
  try {
    if (!playing) {
      status('Starting audio…');
      await ensureAudio();
      await ctx.resume();
      playing = true;
    } else playing = false;
    $('play').textContent = playing ? '■ Stop' : '▶ Play';
    $('play').classList.toggle('on', playing);
    applyRouting();
    status(playing ? `Playing <b>${source === 'B' ? 'B · new model' : 'A · current game engine'}</b>. Keys: A / B to switch, space to stop.` : 'Stopped.');
  } catch (e) {
    status('Audio failed: ' + e.message, true);
    console.error(e);
  }
}
$('play').onclick = togglePlay;
document.addEventListener('click', () => { if (ctx && playing && ctx.state !== 'running') ctx.resume(); }, { capture: true });

// ── Frame loop ───────────────────────────────────────────────────
let last = performance.now(), lastSrc = source;
const freq = new Float32Array(4096);
function frame(now) {
  requestAnimationFrame(frame);
  const dt = Math.min(0.05, (now - last) / 1000); last = now;
  const s = driver.step(dt);
  if (driver.mode !== 'manual') { $('rpm').value = s.rpm; $('thr').value = s.throttle; showRpm(); showThr(); }
  $('rRpm').textContent = Math.round(s.rpm);
  $('rThr').textContent = Math.round(s.throttle * 100) + '%';
  $('rGear').textContent = s.gear ? s.gear : 'N';
  $('rSpeed').textContent = Math.round(s.speed * 3.6);
  $('rBoost').textContent = Math.round(s.boost * 100) + '%';
  if (!ctx) return;
  engine.port.postMessage({ type: 'state', rpm: s.rpm, throttle: s.throttle, boost: s.boost, speed: s.speed });
  if (game.ready) {
    const on = playing && source === 'A';
    if (on && lastSrc !== 'A') game.setCar(params.gameCar);
    // As Race.js does it (rpmMax is always 7800 in the game).
    game.update(dt, on
      ? { rpm: Math.min(s.rpm, 7800 * 1.05), rpmMax: 7800, throttle: s.throttle, gear: s.gear || 1, speed: s.speed, onGround: true, boost: s.boost }
      : { rpm: 0, throttle: 0, speed: 0 });
    if (on) for (const e of s.events) game.shift(e === 'up');
    lastSrc = on ? 'A' : 'B';
  }
  drawSpectrum();
}
requestAnimationFrame(frame);

function drawSpectrum() {
  const c = $('spec'), g = c.getContext('2d');
  const W = c.width, H = c.height, sr = ctx.sampleRate;
  analyser.getFloatFrequencyData(freq);
  g.fillStyle = '#0a0c10'; g.fillRect(0, 0, W, H);
  const fx = (f) => (Math.log(f / 20) / Math.log(20000 / 20)) * W;
  const dbY = (d) => H - ((d + 110) / 100) * H;
  g.fillStyle = 'rgba(255,184,77,0.10)'; g.fillRect(fx(40), 0, fx(150) - fx(40), H);
  g.fillStyle = 'rgba(111,183,255,0.10)'; g.fillRect(fx(1000), 0, fx(4000) - fx(1000), H);
  g.strokeStyle = '#262c36'; g.fillStyle = '#8b94a3'; g.font = '20px system-ui';
  for (const f of [50, 100, 200, 500, 1000, 2000, 5000, 10000]) {
    const x = fx(f); g.beginPath(); g.moveTo(x, 0); g.lineTo(x, H); g.stroke();
    g.fillText(f >= 1000 ? f / 1000 + 'k' : f, x + 3, H - 6);
  }
  g.strokeStyle = '#ff3d7f'; g.lineWidth = 2; g.beginPath();
  const bin = sr / analyser.fftSize;
  let pLow = 0, pHigh = 0;
  for (let k = 1; k < freq.length; k++) {
    const f = k * bin; if (f < 20) continue;
    const p = Math.pow(10, freq[k] / 10);
    if (f >= 40 && f < 150) pLow += p; else if (f >= 1000 && f < 4000) pHigh += p;
    const x = fx(f), y = dbY(freq[k]);
    if (k === 1 || f - bin < 20) g.moveTo(x, y); else g.lineTo(x, y);
  }
  g.stroke();
  const db = (p) => 10 * Math.log10(p + 1e-20);
  $('eLow').textContent = db(pLow).toFixed(0) + ' dB';
  $('eHigh').textContent = db(pHigh).toFixed(0) + ' dB';
  $('eRatio').textContent = playing ? (db(pLow) - db(pHigh)).toFixed(1) + ' dB' : '–';
}

// ── Measure ──────────────────────────────────────────────────────
$('measure').onclick = async () => {
  const tb = $('mTable');
  $('measure').disabled = true;
  tb.innerHTML = '<tr><th>Preset</th><th>Point</th><th>B peak</th><th>B RMS dB</th><th>B rumble−whine</th><th>B cycle corr.</th><th>A (game) RMS dB</th><th>A rumble−whine</th><th>A cycle corr.</th></tr>';
  let n = 0;
  try {
    await measureAll(ORDER, (r) => {
      n++; $('mNote').textContent = `${n} / ${ORDER.length * 5}…`;
      tb.insertAdjacentHTML('beforeend', `<tr><td>${PRESETS[r.preset].name.split(' (')[0]}</td><td>${r.point}</td><td>${r.model.peak}</td><td>${r.model.rmsDb}</td><td>${r.model.lowVsHighDb}</td><td>${r.model.cycleCorr}</td><td>${r.game.rmsDb}</td><td>${r.game.lowVsHighDb}</td><td>${r.game.cycleCorr}</td></tr>`);
    });
    $('mNote').textContent = 'Done.';
  } catch (e) { $('mNote').textContent = 'Failed: ' + e.message; console.error(e); }
  $('measure').disabled = false;
};

window.engineLab = { PRESETS, ORDER, presetParams, measureAll, measurePhone, renderModel, renderGame, analyse, driver, setMode, setSource, selectPreset, togglePlay, get params() { return params; } };
selectPreset(key);
