// Music Lab page: the song, generator, parts, instruments, drum machine and
// mix controls, around one AudioWorklet (B, worklet.js) and the game's own
// Music.js (A, read-only), playing the same song in step.
//
//   B: music-lab worklet ─ gain ─┐
//   A: Music.js ─ gain ──────────┴─ speaker-sim (engine lab) ─ volume ─ analyser ─ out

import { Music } from '../../src/game/audio/Music.js';
import { askForPlayback } from '../../src/game/Audio.js';
import { TRACKS, PATCHES } from '../../src/game/audio/tracks.js';
import { BPATCH, KITS, KNOBS, FM_OP_KNOBS, DRUM_KNOBS, voiceTrack, kitVoice } from './instruments.js';
import { GENRES } from './gen.js';
import { DRUM_LANES } from './seq.js';
import { WORKLET_URL, measureAll } from './measure.js';

const SPEAKER_URL = new URL('../engine-lab/worklet.js', import.meta.url);
const $ = (id) => document.getElementById(id);
const status = (html, err = false) => { $('status').innerHTML = html; $('status').classList.toggle('err', err); };

// Game songs keep the kit the lab picked for their style.
const SONG_KIT = { 'midnight-run': 'tr808', seabright: 'tr808', 'neon-rush': 'tr909', mirage: 'tr808', interstate: 'tr909', 'chrome-heart': 'tr909', afterburner: 'tr909' };
const songs = TRACKS.map((T) => ({ track: voiceTrack(T, PATCHES), gameId: T.id, kit: SONG_KIT[T.id] ?? 'tr909' }));
let generated = null;
let cur = songs[0];
let source = 'B', playing = false;
let ctx = null, lab = null, speaker = null, vol = null, analyser = null, bIn = null, aIn = null, music = null;
const mute = new Set();
let solo = null;
let kitTweak = {};

// ── Controls ─────────────────────────────────────────────────────
function fillSongs() {
  const s = $('song');
  s.innerHTML = '';
  const g1 = document.createElement('optgroup'); g1.label = 'The game\'s songs, re-voiced';
  songs.forEach((S, i) => g1.append(new Option(`${S.track.title} · ${S.track.style}`, 'g' + i)));
  s.append(g1);
  if (generated) {
    const g2 = document.createElement('optgroup'); g2.label = 'Made here';
    g2.append(new Option(`${generated.track.title} · ${generated.track.style} · ${generated.track.bpm} bpm`, 'made'));
    s.append(g2);
  }
  s.value = cur === generated ? 'made' : 'g' + songs.indexOf(cur);
}
$('song').onchange = () => { const v = $('song').value; selectSong(v === 'made' ? generated : songs[Number(v.slice(1))]); };

function bindRange(el, fmt = (v) => v, on = null) {
  const out = el.parentElement.querySelector('output');
  const show = () => { if (out) out.textContent = fmt(Number(el.value)); };
  el.addEventListener('input', () => { show(); on?.(Number(el.value)); });
  show();
}
const pct = (v) => Math.round(v * 100) + '%';
bindRange($('dejavu'), pct);
bindRange($('energy'), pct, (v) => lab?.port.postMessage({ type: 'energy', value: v }));
for (const k of ['tape', 'glue', 'reverb', 'revTone']) bindRange($(k), pct, (v) => lab?.port.postMessage({ type: 'mix', [k]: v }));
bindRange($('revSize'), (v) => v.toFixed(1), (v) => lab?.port.postMessage({ type: 'mix', revSize: v }));
bindRange($('vol'), pct, (v) => vol?.gain.setTargetAtTime(v, ctx.currentTime, 0.03));
const mixNow = () => ({ tape: +$('tape').value, glue: +$('glue').value, reverb: +$('reverb').value, revSize: +$('revSize').value, revTone: +$('revTone').value });

$('speaker').onchange = () => speaker?.port.postMessage({ type: 'profile', name: $('speaker').value });
function setKit(name) {
  $('kit').value = name; $('kit2').value = name;
  cur.kit = name;
  lab?.port.postMessage({ type: 'kit', name, tweak: kitTweak });
  buildPads();
}
$('kit').onchange = () => setKit($('kit').value);
$('kit2').onchange = () => setKit($('kit2').value);

$('dice').onclick = () => { $('seed').value = 1 + Math.floor(Math.random() * 99999); makeTrack(); };
$('make').onclick = () => makeTrack();
function makeTrack() {
  const g = $('genre').value, seed = Math.max(1, Number($('seed').value) | 0);
  const track = GENRES[g](seed, { dejavu: Number($('dejavu').value) });
  generated = { track, gameId: null, kit: track.kitName };
  fillSongs();
  selectSong(generated, true);
}

// ── Song ─────────────────────────────────────────────────────────
function selectSong(S, autoplay = false) {
  cur = S;
  fillSongs();
  const T = S.track;
  $('songNote').textContent = `${T.bpm} bpm, ${T.sections.reduce((a, s) => a + s.bars, 0)} bars.` + (S.gameId ? '' : ' Made here: there is no A for it.');
  $('srcA').disabled = !S.gameId;
  if (!S.gameId && source === 'A') setSource('B');
  $('kit').value = S.kit; $('kit2').value = S.kit;
  mute.clear(); solo = null;
  buildSections(); buildParts(); buildPads();
  if (lab) {
    lab.port.postMessage({ type: 'track', track: T, kit: S.kit });
    lab.port.postMessage({ type: 'kit', name: S.kit, tweak: kitTweak });
    sendMutes();
  }
  if (playing || autoplay) startAt(0);
}

function sectionBar(i) { let b = 0; for (let k = 0; k < i; k++) b += cur.track.sections[k].bars; return b; }
function buildSections() {
  const el = $('sections');
  el.innerHTML = '';
  cur.track.sections.forEach((s, i) => {
    const b = document.createElement('button');
    const parts = Object.keys(s.p || {}).join(', ');
    b.textContent = `${i + 1}. ${s.drums ?? '–'} ${s.bars}`;
    b.title = `bar ${sectionBar(i) + 1}: ${parts || 'no parts'}${s.drop ? ', drop' : ''}${s.riser ? ', riser' : ''}`;
    b.onclick = () => startAt(sectionBar(i));
    el.append(b);
  });
}

async function startAt(bar) {
  try {
    await ensureAudio();
    await ctx.resume();
    const wasOn = music.on;
    if (cur.gameId) { music.seek(cur.gameId, bar); music.start(); }
    // Music.js starts 0.1 s out (0.05 s when already running): B starts with it.
    lab.port.postMessage({ type: 'play', bar, delay: cur.gameId ? (wasOn ? 0.05 : 0.1) : 0.03 });
    playing = true;
    applyRouting();
  } catch (e) { status('Audio failed: ' + e.message, true); console.error(e); }
}
function stopAll() {
  playing = false;
  lab?.port.postMessage({ type: 'stop' });
  music?.stop();
  applyRouting();
}
async function togglePlay() {
  if (!playing) { status('Starting audio…'); await startAt(0); } else stopAll();
}
$('play').onclick = togglePlay;

function setSource(s) {
  if (s === 'A' && !cur.gameId) return;
  source = s;
  $('srcA').classList.toggle('on', s === 'A'); $('srcB').classList.toggle('on', s === 'B');
  applyRouting();
}
$('srcA').onclick = () => setSource('A');
$('srcB').onclick = () => setSource('B');

function applyRouting() {
  $('play').textContent = playing ? '■ Stop' : '▶ Play';
  $('play').classList.toggle('on', playing);
  if (ctx) {
    const t = ctx.currentTime;
    bIn.gain.setTargetAtTime(source === 'B' ? 1 : 0, t, 0.02);
    aIn.gain.setTargetAtTime(source === 'A' ? 1 : 0, t, 0.02);
  }
  status(playing ? `Playing <b>${cur.track.title}</b>, <b>${source === 'B' ? 'B · new sound' : 'A · game today'}</b>. Keys: A / B to switch, space to stop.` : 'Stopped.');
}

// ── Parts ────────────────────────────────────────────────────────
let editing = null;
function sendMutes() {
  lab?.port.postMessage({ type: 'mute', names: [...mute] });
  lab?.port.postMessage({ type: 'solo', name: solo });
  if (music) music.solo = solo && solo !== 'drums' && !DRUM_LANES.includes(solo) ? solo : solo === 'drums' ? 'drums' : null;
}
function buildParts() {
  const tb = $('parts');
  tb.innerHTML = '<tr><th>Part</th><th>Plays</th><th>Instrument (B)</th><th>Game patch (A)</th><th></th></tr>';
  const rows = Object.entries(cur.track.parts).map(([name, p]) => [name, p.type, `${p.lab.kind} · ${p.lab.name ?? ''}`, cur.gameId ? patchLabel(p.inst) : '–']);
  rows.push(['drums', 'drums', cur.kit === 'tr808' ? 'TR-808' : 'TR-909', cur.gameId ? 'samples.js kit' : '–']);
  for (const [name, type, b, a] of rows) {
    const tr = document.createElement('tr');
    tr.innerHTML = `<td>${name}</td><td>${type}</td><td>${b}</td><td>${a}</td><td></td>`;
    const td = tr.lastChild;
    const mk = (label, on, fn) => { const x = document.createElement('button'); x.className = 'sm' + (on ? ' on' : ''); x.textContent = label; x.onclick = fn; td.append(x); };
    mk('Mute', mute.has(name), () => { mute.has(name) ? mute.delete(name) : mute.add(name); sendMutes(); buildParts(); });
    mk('Solo', solo === name, () => { solo = solo === name ? null : name; sendMutes(); buildParts(); });
    if (name !== 'drums') mk('Edit', editing === name, () => editPart(name));
    tb.append(tr);
  }
}
function patchLabel(inst) { for (const [k, v] of Object.entries(PATCHES)) if (v === inst) return k; return 'a variant'; }

function knobPanel(el, labP, send) {
  el.innerHTML = '';
  const add = (label, get, set, min, max, step) => {
    const l = document.createElement('label'); l.className = 'sl';
    l.innerHTML = `${label} <input type="range" min="${min}" max="${max}" step="${step}"><output></output>`;
    const inp = l.querySelector('input'), out = l.querySelector('output');
    inp.value = get() ?? min;
    const show = () => { out.textContent = +Number(inp.value).toFixed(3); };
    show();
    inp.addEventListener('input', () => { set(Number(inp.value)); show(); send(); });
    el.append(l);
  };
  for (const [k, label, min, max, step] of KNOBS[labP.kind] ?? []) add(label, () => labP[k], (v) => { labP[k] = v; }, min, max, step);
  if (labP.kind === 'fm') {
    labP.ops.forEach((op, i) => {
      if (!op) return;
      for (const [k, label, min, max, step] of FM_OP_KNOBS) add(`Op ${i + 1} ${label}`, () => op[k], (v) => { op[k] = v; }, min, max, step);
    });
  }
}
function editPart(name) {
  editing = editing === name ? null : name;
  buildParts();
  $('partEdit').hidden = !editing;
  if (!editing) return;
  const P = cur.track.parts[name];
  $('partEditTitle').textContent = `${name}: ${P.lab.kind} · ${P.lab.name ?? ''}`;
  knobPanel($('partKnobs'), P.lab, () => lab?.port.postMessage({ type: 'patch', part: name, lab: structuredClone(P.lab) }));
}
$('partCopy').onclick = () => copyJson(editing && { [editing]: cur.track.parts[editing].lab }, 'partNote');
$('partReset').onclick = () => {
  if (!editing) return;
  const P = cur.track.parts[editing];
  const base = cur.gameId ? voiceTrack(TRACKS.find((t) => t.id === cur.gameId), PATCHES).parts[editing].lab : null;
  if (base) { Object.keys(P.lab).forEach((k) => delete P.lab[k]); Object.assign(P.lab, base); }
  lab?.port.postMessage({ type: 'patch', part: editing, lab: structuredClone(P.lab) });
  const n = editing; editing = null; editPart(n);
};

async function copyJson(obj, noteId) {
  if (!obj) return;
  const json = JSON.stringify(obj, null, 2);
  $('json').hidden = false; $('json').value = json;
  try { await navigator.clipboard.writeText(json); $(noteId).textContent = 'Copied.'; } catch { $(noteId).textContent = 'Select the text below and copy it.'; $('json').select(); }
}

// ── Instruments (keyboard) ───────────────────────────────────────
let inst = 'acid', instP = structuredClone(BPATCH.acid), octave = 3;
const byKind = {};
for (const [k, v] of Object.entries(BPATCH)) (byKind[v.kind] ||= []).push(k);
const KIND_NAME = { tb303: 'TB-303', juno: 'Juno-style poly', mono: 'Moog-style mono', fm: 'DX-style FM' };
for (const [kind, names] of Object.entries(byKind)) {
  const g = document.createElement('optgroup'); g.label = KIND_NAME[kind];
  for (const n of names) g.append(new Option(n, n));
  $('inst').append(g);
}
$('inst').value = inst;
function selectInst(k) {
  inst = k; instP = structuredClone(BPATCH[k]);
  lab?.port.postMessage({ type: 'audition', lab: instP });
  knobPanel($('instKnobs'), instP, () => lab?.port.postMessage({ type: 'audition', lab: structuredClone(instP) }));
}
$('inst').onchange = () => selectInst($('inst').value);
$('instCopy').onclick = () => copyJson({ [inst]: instP }, 'instNote');
$('instReset').onclick = () => selectInst(inst);
$('oct-').onclick = () => { octave = Math.max(0, octave - 1); buildKeys(); };
$('oct+').onclick = () => { octave = Math.min(6, octave + 1); buildKeys(); };

const held = new Map();
async function noteOn(midi, vel = 0.9, extra = {}) {
  await ensureAudio(); await ctx.resume();
  lab.port.postMessage({ type: 'on', midi, vel, ...extra });
  document.querySelector(`.keys [data-m="${midi}"]`)?.classList.add('down');
}
function noteOff(midi) {
  lab?.port.postMessage({ type: 'off', midi });
  document.querySelector(`.keys [data-m="${midi}"]`)?.classList.remove('down');
}
function buildKeys() {
  const el = $('keys');
  el.innerHTML = '';
  const base = 12 * (octave + 1), W = 100 / 15;
  let wi = 0;
  for (let i = 0; i < 25; i++) {
    const m = base + i, pc = i % 12, black = [1, 3, 6, 8, 10].includes(pc);
    const d = document.createElement('div');
    d.dataset.m = m;
    d.className = black ? 'b' : 'w';
    if (black) { d.style.left = `calc(${wi * W}% - ${W * 0.3}%)`; d.style.width = W * 0.6 + '%'; } else { d.style.left = wi * W + '%'; d.style.width = W + '%'; wi++; }
    if (pc === 0) d.innerHTML = `<span>C${octave + 1 + Math.floor(i / 12) - 1}</span>`;
    d.onpointerdown = (e) => { d.setPointerCapture(e.pointerId); noteOn(m); };
    d.onpointerup = d.onpointercancel = () => noteOff(m);
    el.append(d);
  }
}
const KEYMAP = 'awsedftgyhujk';
document.addEventListener('keydown', (e) => {
  if (e.target.tagName === 'INPUT' && e.target.type === 'number') return;
  if (e.repeat || e.metaKey || e.ctrlKey) return;
  const k = e.key.toLowerCase();
  if (e.key === ' ') { e.preventDefault(); togglePlay(); return; }
  if ($('keysOn').checked) {
    const i = KEYMAP.indexOf(k);
    if (i >= 0) { const m = 12 * (octave + 1) + i; held.set(k, m); noteOn(m); return; }
    if (k === 'z') { $('oct-').click(); return; }
    if (k === 'x') { $('oct+').click(); return; }
  }
  if (k === 'a') setSource('A');
  if (k === 'b') setSource('B');
});
document.addEventListener('keyup', (e) => { const k = e.key.toLowerCase(); if (held.has(k)) { noteOff(held.get(k)); held.delete(k); } });

// Demo phrases: an acid line for the 303, chords for the polys, a riff for monos.
let demoTimer = null;
$('demo').onclick = async () => {
  clearInterval(demoTimer);
  await ensureAudio(); await ctx.resume();
  const kind = instP.kind, step = 60 / 124 / 4;
  let ev;
  if (kind === 'tb303') {
    const line = [[36, 1, 0], [36, 0, 0], [48, 0, 1], [46, 1, 0], [0], [36, 0, 0], [39, 1, 1], [41, 0, 0], [36, 0, 0], [0], [48, 1, 0], [36, 0, 0], [43, 0, 1], [46, 0, 0], [36, 1, 0], [34, 0, 0]];
    ev = line.map(([m, acc, sl], i) => m ? { t: i, midi: m + 12 * (octave - 2), dur: step * (sl ? 1.05 : 0.55), accent: !!acc, glideFrom: i && line[i - 1][2] ? line[i - 1][0] + 12 * (octave - 2) : undefined } : null).filter(Boolean);
  } else if (kind === 'mono') {
    const ns = [0, 0, 12, 0, 10, 0, 7, 3];
    ev = ns.map((n, i) => ({ t: i * 2, midi: 12 * (octave + 1) + n, dur: step * 1.6 }));
  } else {
    const chords = [[0, 3, 7, 10], [5, 8, 12, 15], [3, 7, 10, 14], [-2, 2, 5, 9]];
    ev = [];
    chords.forEach((c, i) => c.forEach((n) => ev.push({ t: i * 8, midi: 12 * (octave + 1) + n, dur: step * 7 })));
  }
  let i = 0;
  const t0 = performance.now();
  demoTimer = setInterval(() => {
    const now = (performance.now() - t0) / 1000 / step;
    while (i < ev.length && ev[i].t <= now) {
      const e = ev[i++];
      lab.port.postMessage({ type: 'on', midi: e.midi, vel: e.accent ? 1 : 0.8, dur: e.dur, accent: e.accent, glideFrom: e.glideFrom });
    }
    if (i >= ev.length) clearInterval(demoTimer);
  }, 5);
};

// ── Drum machine ─────────────────────────────────────────────────
let pad = 'kick';
function buildPads() {
  const el = $('pads');
  el.innerHTML = '';
  for (const lane of Object.keys(KITS[cur.kit])) {
    const b = document.createElement('button');
    b.textContent = lane;
    b.className = lane === pad ? 'sel' : '';
    b.onpointerdown = async () => {
      await ensureAudio(); await ctx.resume();
      lab.port.postMessage({ type: 'hit', lane, vel: 1 });
      if (pad !== lane) { pad = lane; buildPads(); }
    };
    el.append(b);
  }
  const k = { ...kitVoice(cur.kit, pad), ...(kitTweak[pad] || {}) };
  const knobs = DRUM_KNOBS[k.t] ?? DRUM_KNOBS.default;
  const el2 = $('drumKnobs');
  el2.innerHTML = '';
  for (const [key, label, min, max, step] of knobs) {
    const l = document.createElement('label'); l.className = 'sl';
    l.innerHTML = `${pad} ${label.toLowerCase()} <input type="range" min="${min}" max="${max}" step="${step}"><output></output>`;
    const inp = l.querySelector('input'), out = l.querySelector('output');
    inp.value = k[key] ?? min;
    out.textContent = +Number(inp.value).toFixed(3);
    inp.addEventListener('input', () => {
      (kitTweak[pad] ||= {})[key] = Number(inp.value);
      out.textContent = +Number(inp.value).toFixed(3);
      lab?.port.postMessage({ type: 'kit', name: cur.kit, tweak: kitTweak });
    });
    inp.addEventListener('change', () => lab?.port.postMessage({ type: 'hit', lane: pad, vel: 1 }));
    el2.append(l);
  }
}
$('drumCopy').onclick = () => copyJson({ [cur.kit]: Object.fromEntries(Object.entries(kitTweak).map(([l, t]) => [l, { ...kitVoice(cur.kit, l), ...t }])) }, 'drumNote');
$('drumReset').onclick = () => { kitTweak = {}; lab?.port.postMessage({ type: 'kit', name: cur.kit, tweak: kitTweak }); buildPads(); };

// ── Audio ────────────────────────────────────────────────────────
let starting = null;
function ensureAudio() { return (starting ||= start()); }
async function start() {
  const AC = window.AudioContext || window.webkitAudioContext;
  if (!AC) throw new Error('this browser has no Web Audio');
  if (!window.isSecureContext || !window.AudioWorkletNode) throw new Error('AudioWorklet needs a secure page (https or localhost): open this through the tailnet https address');
  askForPlayback();
  const ac = new AC({ latencyHint: 'playback' });
  await ac.audioWorklet.addModule(WORKLET_URL);
  await ac.audioWorklet.addModule(SPEAKER_URL);
  lab = new AudioWorkletNode(ac, 'music-lab', { numberOfInputs: 0, outputChannelCount: [2], processorOptions: { seed: 5, track: cur.track, kit: cur.kit, mix: mixNow() } });
  lab.port.onmessage = (m) => onLab(m.data);
  lab.onprocessorerror = () => status('The audio thread stopped with an error (see the console). Reload to try again.', true);
  speaker = new AudioWorkletNode(ac, 'speaker-sim', { outputChannelCount: [2], processorOptions: { profile: $('speaker').value } });
  bIn = ac.createGain(); aIn = ac.createGain(); aIn.gain.value = 0;
  vol = ac.createGain(); vol.gain.value = Number($('vol').value);
  analyser = ac.createAnalyser(); analyser.fftSize = 8192; analyser.smoothingTimeConstant = 0.75;
  lab.connect(bIn); bIn.connect(speaker); aIn.connect(speaker);
  speaker.connect(vol); vol.connect(analyser); analyser.connect(ac.destination);
  // The game's music, as GameAudio routes it: music bus at its default level.
  const aBus = ac.createGain(); aBus.gain.value = 0.7; aBus.connect(aIn);
  music = new Music(ac, aBus);
  music.build();
  ctx = ac;
  lab.port.postMessage({ type: 'audition', lab: instP });
  lab.port.postMessage({ type: 'energy', value: Number($('energy').value) });
  sendMutes();
  window.musicLab.ctx = ctx; window.musicLab.lab = lab; window.musicLab.music = music;
}
document.addEventListener('click', () => { if (ctx && ctx.state !== 'running') ctx.resume(); }, { capture: true });

function onLab(d) {
  if (d.type === 'pos' && d.pos) {
    const p = d.pos;
    $('rSec').textContent = `${p.sec + 1} / ${cur.track.sections.length}`;
    $('rBar').textContent = `${p.barIndex + 1} / ${p.bars}`;
    $('rGr').textContent = d.gr < 0.999 ? (20 * Math.log10(d.gr)).toFixed(1) + ' dB' : '0 dB';
    if (d.load != null) {
      $('rLoad').textContent = Math.round(d.load * 100) + '%';
      $('loadM').firstChild.style.width = Math.min(100, d.load * 100) + '%';
      $('loadM').classList.toggle('warn', d.load > 0.6);
    }
    [...$('sections').children].forEach((b, i) => b.classList.toggle('cur', i === p.sec && p.playing));
  } else if (d.type === 'end' && playing && cur.gameId) {
    // B loops the song; keep A with it.
    music.seek(cur.gameId, 0);
  }
}

// ── Spectrum ─────────────────────────────────────────────────────
const freq = new Float32Array(4096);
function frame() {
  requestAnimationFrame(frame);
  if (!ctx) return;
  const c = $('spec'), g = c.getContext('2d');
  const W = c.width, H = c.height, sr = ctx.sampleRate;
  analyser.getFloatFrequencyData(freq);
  g.fillStyle = '#0a0c10'; g.fillRect(0, 0, W, H);
  const fx = (f) => (Math.log(f / 20) / Math.log(20000 / 20)) * W;
  const dbY = (d) => H - ((d + 110) / 100) * H;
  g.strokeStyle = '#262c36'; g.fillStyle = '#8b94a3'; g.font = '20px system-ui';
  for (const f of [50, 100, 200, 500, 1000, 2000, 5000, 10000]) {
    const x = fx(f); g.beginPath(); g.moveTo(x, 0); g.lineTo(x, H); g.stroke();
    g.fillText(f >= 1000 ? f / 1000 + 'k' : f, x + 3, H - 6);
  }
  g.strokeStyle = '#ff3d7f'; g.lineWidth = 2; g.beginPath();
  const bin = sr / analyser.fftSize;
  let first = true;
  for (let k = 1; k < freq.length; k++) {
    const f = k * bin; if (f < 20) continue;
    const x = fx(f), y = dbY(freq[k]);
    if (first) { g.moveTo(x, y); first = false; } else g.lineTo(x, y);
  }
  g.stroke();
}
requestAnimationFrame(frame);

// ── Measure ──────────────────────────────────────────────────────
$('measure').onclick = async () => {
  const tb = $('mTable');
  $('measure').disabled = true;
  tb.innerHTML = '<tr><th>Song</th><th>Side</th><th>RMS dB</th><th>Peak</th><th>Crest dB</th><th>Width</th><th>sub</th><th>low</th><th>low-mid</th><th>mid</th><th>high</th><th>air</th></tr>';
  const list = generated ? [...songs, generated] : songs;
  let n = 0;
  const row = (title, side, r) => `<tr><td>${title}</td><td>${side}</td><td>${r.rmsDb}</td><td>${r.peak}</td><td>${r.crestDb}</td><td>${r.width}</td>${['sub', 'low', 'lowmid', 'mid', 'high', 'air'].map((b) => `<td>${r.bandsDb[b]}</td>`).join('')}</tr>`;
  try {
    await measureAll(list, (r) => {
      n++; $('mNote').textContent = `${n} / ${list.length}…`;
      if (r.a) tb.insertAdjacentHTML('beforeend', row(r.title, 'A', r.a));
      tb.insertAdjacentHTML('beforeend', row(r.a ? '' : r.title, 'B', r.b));
    });
    $('mNote').textContent = 'Done. Bands are dB against the low band (40–150 Hz).';
  } catch (e) { $('mNote').textContent = 'Failed: ' + e.message; console.error(e); }
  $('measure').disabled = false;
};

window.musicLab = { songs, GENRES, BPATCH, KITS, measureAll, selectSong, setSource, togglePlay, startAt, get cur() { return cur; } };
fillSongs();
selectSong(cur);
selectInst(inst);
buildKeys();
