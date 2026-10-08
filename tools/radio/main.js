// Radio page: the station player (mp_music.wasm in music-worklet.js, the
// game's own) behind a dial, with what is on and the block's programme
// from a second copy of the wasm on the main thread (mpm_stations,
// mpm_schedule), and the DJ's clips between songs as the game plays them.
//
//   music-worklet (mp-radio) ─ gain ─ analyser ─ media stream ─ <audio>
//   dj clip (AudioBufferSource) ─ gain ──┘   (ducks the station)
//
// It plays like a music app (radio.md 7): the graph ends in an <audio>
// element playing a MediaStream, not the context's destination, so the
// browser treats the page as media: it keeps playing with the screen
// locked and the app in the background, the Silent switch does not mute
// it, and the lock screen and the notification shade show the station and
// the song with play, pause and next. The Media Session API carries the
// metadata and the controls; `navigator.audioSession` asks iOS for a
// playback session before the context exists.

const $ = (id) => document.getElementById(id);
const status = (html, err = false) => { $('status').innerHTML = html; $('status').classList.toggle('err', err); };

// The build's files: beside the page on Pages (the Rust build is at the
// site root, tools under it), under dist/next/ on the dev server.
const BASES = ['../../dist/next/', '../../'];
async function find(file) {
  for (const b of BASES) {
    const url = new URL(b + file, import.meta.url);
    try { const r = await fetch(url, { method: 'HEAD' }); if (r.ok) return url; } catch { /* next */ }
  }
  throw new Error(`${file} not found: run cargo xtask web --release`);
}

// ── The wasm on the main thread: the stations and the schedule ──
let info = null;
async function loadInfo() {
  const url = await find('mp_music.wasm');
  const { instance } = await WebAssembly.instantiateStreaming(fetch(url), {});
  const x = instance.exports;
  const text = (ptr) => new TextDecoder().decode(new Uint8Array(x.memory.buffer, ptr, x.mpm_text_len()));
  info = {
    stations: JSON.parse(text(x.mpm_stations())),
    schedule: (station, wall) => { const day = Math.floor(wall / 86400); return JSON.parse(text(x.mpm_schedule(station, day, wall - day * 86400))); },
  };
  return info;
}

// ── Audio ────────────────────────────────────────────────────────
let ctx = null, node = null, gain = null, stationGain = null, analyser = null, module = null, out = null;
let station = -1, serial = 0;
// The station to come back to on play, and the one remembered between visits.
const STORE = 'radio.station';
let last = 0;
async function ensureAudio() {
  if (ctx) return;
  const AC = window.AudioContext || window.webkitAudioContext;
  if (!AC) throw new Error('this browser has no Web Audio');
  if (!window.isSecureContext || !window.AudioWorkletNode) throw new Error('AudioWorklet needs a secure page (https or localhost): open this through the tailnet https address');
  // iOS: a playback session (music), not ambient sound, before the context.
  try { if (navigator.audioSession) navigator.audioSession.type = 'playback'; } catch { /* older Safari */ }
  const ac = new AC({ latencyHint: 'playback' });
  await ac.audioWorklet.addModule(await find('music-worklet.js'));
  module = await WebAssembly.compileStreaming(fetch(await find('mp_music.wasm')));
  node = new AudioWorkletNode(ac, 'mp-radio', { numberOfInputs: 0, outputChannelCount: [2], processorOptions: { module, seed: 2026 } });
  stationGain = ac.createGain(); // the DJ ducks this one
  gain = ac.createGain(); gain.gain.value = Number($('vol').value);
  analyser = ac.createAnalyser(); analyser.fftSize = 1024;
  node.connect(stationGain); stationGain.connect(gain); gain.connect(analyser);
  // Out through a media element (see the top): the page is a music app.
  const el = $('out');
  if (ac.createMediaStreamDestination && el && 'srcObject' in el) {
    const dest = ac.createMediaStreamDestination();
    analyser.connect(dest);
    el.srcObject = dest.stream;
    out = el;
  } else {
    analyser.connect(ac.destination);
  }
  ctx = ac;
  mediaActions();
}
// Falls back to the context's own output if the element will not play.
async function playOut() {
  if (!out) return;
  try { await out.play(); } catch (e) {
    console.warn('media element', e);
    analyser.connect(ctx.destination);
    out = null;
  }
}
document.addEventListener('click', () => { if (ctx && ctx.state !== 'running') ctx.resume(); }, { capture: true });
// Back from the lock screen or another app: the context may have been
// interrupted (iOS says so with a state of its own); pick it up again.
const wake = () => { if (ctx && station >= 0 && ctx.state !== 'running') { ctx.resume(); playOut(); } };
document.addEventListener('visibilitychange', () => { if (!document.hidden) wake(); });
window.addEventListener('pageshow', wake);
window.addEventListener('focus', wake);

const wallNow = () => Date.now() / 1000;

async function tune(i) {
  try {
    await ensureAudio();
    await ctx.resume();
  } catch (e) { status('Audio failed: ' + e.message, true); console.error(e); return; }
  station = i;
  if (i >= 0) { last = i; try { localStorage.setItem(STORE, info.stations[i].key); } catch { /* private mode */ } }
  serial++;
  const t = ctx.currentTime;
  const wall = wallNow();
  const day = Math.floor(wall / 86400);
  const p = node.parameters;
  p.get('wallDay').setValueAtTime(day, t);
  p.get('wallSec').setValueAtTime(wall - day * 86400, t);
  p.get('station').setValueAtTime(i, t);
  p.get('tune').setValueAtTime(serial, t);
  p.get('energy').setValueAtTime(Number($('energy').value), t);
  lastSlot = null;
  djDue = i >= 0 && Math.random() < 0.5 ? wall + 2 : null;
  if (i >= 0) await playOut(); else out?.pause();
  if ('mediaSession' in navigator) navigator.mediaSession.playbackState = i >= 0 ? 'playing' : 'paused';
  buildDial();
  refresh(true);
  status(i >= 0 ? `Tuned to <b>${info.stations[i].name} ${info.stations[i].freq}</b>.` : 'Off.');
}
// The player's buttons: play comes back to the last station (live: the
// song has moved on), pause is off, next and previous step the dial.
const step = (d) => tune((last + d + info.stations.length) % info.stations.length);
const toggle = () => tune(station >= 0 ? -1 : last);

function buildDial() {
  const el = $('dial');
  el.innerHTML = '';
  info.stations.forEach((s, i) => {
    const b = document.createElement('button');
    b.className = i === station ? 'on' : '';
    b.innerHTML = `${s.name} <small>${s.freq} · ${s.genres.map(([g]) => g).join(', ')}${s.dj ? ' · DJ ' + s.dj : ''}</small>`;
    b.onclick = () => tune(i);
    el.append(b);
  });
  $('off').className = station < 0 && ctx ? 'on' : '';
  $('play').textContent = station >= 0 ? '❚❚' : '▶';
  $('play').setAttribute('aria-label', station >= 0 ? 'Pause' : 'Play');
  $('play').title = station >= 0 ? 'Pause (space)' : `Play ${info.stations[last].name} (space)`;
}
$('off').onclick = () => tune(-1);
$('play').onclick = toggle;
$('prev').onclick = () => step(-1);
$('next').onclick = () => step(1);
document.addEventListener('keydown', (e) => {
  if (e.target.tagName === 'INPUT' || e.metaKey || e.ctrlKey || e.altKey) return;
  if (e.code === 'Space') { e.preventDefault(); toggle(); } else if (e.code === 'ArrowRight' || e.code === 'KeyT') step(1); else if (e.code === 'ArrowLeft') step(-1);
});

// ── The lock screen: Media Session ───────────────────────────────
const art = {};
// A tile per station for the lock screen: the station's name and
// frequency on the dial's dark, drawn once.
function artwork(st) {
  if (art[st.key]) return art[st.key];
  const c = document.createElement('canvas'); c.width = c.height = 512;
  const g = c.getContext('2d');
  g.fillStyle = '#0d0f14'; g.fillRect(0, 0, 512, 512);
  const glow = g.createRadialGradient(256, 330, 20, 256, 330, 300);
  glow.addColorStop(0, 'rgba(255,61,127,0.35)'); glow.addColorStop(1, 'rgba(255,61,127,0)');
  g.fillStyle = glow; g.fillRect(0, 0, 512, 512);
  g.strokeStyle = '#e6e9ee'; g.lineWidth = 10; g.lineCap = 'round';
  g.beginPath(); g.arc(256, 340, 180, Math.PI * 1.1, Math.PI * 1.9); g.stroke();
  g.strokeStyle = '#ff3d7f'; g.lineWidth = 14;
  const a = Math.PI * (1.1 + 0.8 * (parseFloat(st.freq) - 87) / 21);
  g.beginPath(); g.moveTo(256, 340); g.lineTo(256 + 185 * Math.cos(a), 340 + 185 * Math.sin(a)); g.stroke();
  g.fillStyle = '#ff3d7f'; g.beginPath(); g.arc(256, 340, 16, 0, Math.PI * 2); g.fill();
  g.fillStyle = '#e6e9ee'; g.textAlign = 'center';
  g.font = 'bold 54px ui-sans-serif, system-ui, sans-serif'; g.fillText(st.name, 256, 420);
  g.font = '36px ui-sans-serif, system-ui, sans-serif'; g.fillStyle = '#8b94a3'; g.fillText(st.freq, 256, 468);
  const url = c.toDataURL('image/png');
  art[st.key] = [{ src: url, sizes: '512x512', type: 'image/png' }];
  return art[st.key];
}
let mediaSlot = null;
function mediaUpdate(st, s, into) {
  if (!('mediaSession' in navigator)) return;
  const ms = navigator.mediaSession;
  const slot = `${station}:${s.title}`;
  if (slot !== mediaSlot) {
    mediaSlot = slot;
    ms.metadata = new MediaMetadata({ title: s.title, artist: `${st.name} ${st.freq}`, album: s.style, artwork: artwork(st) });
  }
  try { ms.setPositionState({ duration: s.secs, position: Math.max(0, Math.min(into, s.secs)), playbackRate: 1 }); } catch { /* not everywhere */ }
}
function mediaActions() {
  if (!('mediaSession' in navigator)) return;
  const ms = navigator.mediaSession;
  const on = (name, f) => { try { ms.setActionHandler(name, f); } catch { /* not supported here */ } };
  on('play', () => tune(last));
  on('pause', () => tune(-1));
  on('stop', () => tune(-1));
  on('nexttrack', () => step(1));
  on('previoustrack', () => step(-1));
}

function bindRange(el, fmt, on) {
  const out_ = el.parentElement.querySelector('output');
  const show = () => { out_.textContent = fmt(Number(el.value)); };
  el.addEventListener('input', () => { show(); on?.(Number(el.value)); });
  show();
}
const pct = (v) => Math.round(v * 100) + '%';
bindRange($('vol'), pct, (v) => gain?.gain.setTargetAtTime(v, ctx.currentTime, 0.03));
bindRange($('energy'), pct, (v) => node?.parameters.get('energy').setTargetAtTime(v, ctx.currentTime, 0.5));

// ── What is on ───────────────────────────────────────────────────
let lastSlot = null, lastSec = -1, sched = null;
const mmss = (s) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`;
function refresh(force = false) {
  if (!info || station < 0) {
    $('now').textContent = ctx ? `Off · tap ▶ for ${info?.stations[last]?.name ?? 'the radio'}` : '–'; $('nowSub').textContent = ''; $('prog').firstChild.style.width = '0';
    $('clock').textContent = ''; $('prog-table').innerHTML = ''; $('blockNote').textContent = '';
    return;
  }
  const wall = wallNow();
  const sec = Math.floor(wall);
  if (!force && sec === lastSec) return;
  lastSec = sec;
  sched = info.schedule(station, wall);
  const st = info.stations[station];
  const s = sched.slots[sched.slot];
  const into = sched.into - s.start;
  $('now').innerHTML = `<b>${s.title}</b> · ${s.style}`;
  $('nowSub').textContent = `${st.name} ${st.freq} · ${Math.round(s.bpm)} bpm · ${s.bars} bars · ${mmss(into)} / ${mmss(s.secs)} · 16th ${sched.step}`;
  $('prog').firstChild.style.width = Math.min(100, (into / s.secs) * 100) + '%';
  const since = wall - 1767225600;
  $('clock').textContent = `station time ${new Date(wall * 1000).toISOString().replace('T', ' ').slice(0, 19)} UTC · block ${sched.block} (${mmss(sched.into)} of 20:00) · on air ${Math.floor(since / 86400)} days`;
  $('blockNote').textContent = `Block ${sched.block}: ${sched.slots.length} songs, the last fitted to the boundary (its tempo nudged).`;
  const tb = $('prog-table');
  tb.innerHTML = '<tr><th>At</th><th>Song</th><th>Style</th><th class="r">bpm</th><th class="r">Length</th></tr>';
  sched.slots.forEach((x, i) => {
    const tr = document.createElement('tr');
    tr.className = i === sched.slot ? 'cur' : i < sched.slot ? 'past' : '';
    tr.innerHTML = `<td>${mmss(x.start)}</td><td>${x.title}</td><td>${x.style}</td><td class="r">${x.bpm.toFixed(1)}</td><td class="r">${mmss(x.secs)}</td>`;
    tb.append(tr);
  });
  mediaUpdate(st, s, into);
  const slot = `${sched.block}:${sched.slot}`;
  if (lastSlot !== null && slot !== lastSlot && Math.random() < 0.35) djDue = wall;
  lastSlot = slot;
  maybeDj(wall);
}

// ── The DJ ───────────────────────────────────────────────────────
let djIndex = null, djDue = null, djLast = -Infinity, djHistory = [];
async function loadDj() {
  for (const b of ['../../audio/dj/', '../../dist/next/audio/dj/']) {
    try {
      const r = await fetch(new URL(b + 'index.json', import.meta.url));
      if (r.ok) { djIndex = { base: b, clips: (await r.json()).clips }; return; }
    } catch { /* next */ }
  }
  $('dj').textContent = 'No DJ clips here (audio/dj/ is not in this build).';
}
async function maybeDj(wall) {
  if (!$('djOn').checked || !djIndex || djDue === null || wall < djDue) return;
  djDue = null;
  if (wall - djLast < 150) return;
  const dj = info.stations[station].dj;
  if (!dj) return;
  const topic = Math.random() < 0.7 ? 'music' : 'ident';
  const ids = Object.keys(djIndex.clips).filter((id) => id.startsWith(`${dj}-${topic}-`) && !djHistory.includes(id));
  if (!ids.length) return;
  const id = ids[Math.floor(Math.random() * ids.length)];
  djHistory.push(id); if (djHistory.length > 8) djHistory.shift();
  djLast = wall;
  const takes = djIndex.clips[id].takes || 1;
  const take = 1 + Math.floor(Math.random() * takes);
  const file = take === 1 ? `${id}.mp3` : `${id}.${take}.mp3`;
  try {
    const buf = await ctx.decodeAudioData(await (await fetch(new URL(djIndex.base + file, import.meta.url))).arrayBuffer());
    const src = ctx.createBufferSource(); src.buffer = buf;
    src.connect(gain);
    const t0 = ctx.currentTime + 0.01;
    // Duck the station under the voice, as GameAudio does (to 0.4, 50 ms
    // in, back over 150 ms after the clip).
    const sg = stationGain.gain;
    sg.cancelScheduledValues(t0);
    sg.setTargetAtTime(0.4, t0, 0.05);
    sg.setTargetAtTime(1, t0 + buf.duration, 0.15);
    src.start(t0);
    $('dj').textContent = `${dj}: “${djIndex.clips[id].text}”`;
    setTimeout(() => { if ($('dj').textContent.startsWith(dj)) $('dj').textContent = ''; }, buf.duration * 1000 + 2000);
  } catch (e) { console.warn('dj clip', id, e); }
}

// ── Level meter and the clock ────────────────────────────────────
// The meter on frames; what is on (and the DJ, and the lock screen's
// metadata) on a timer, which still runs with the screen off, where
// frames do not.
const data = new Float32Array(1024);
let level = 0;
function frame() {
  requestAnimationFrame(frame);
  if (analyser) {
    analyser.getFloatTimeDomainData(data);
    let p = 0; for (const v of data) p = Math.max(p, Math.abs(v));
    level = p;
    $('meter').firstChild.style.width = Math.min(100, p * 120) + '%';
  }
}

try {
  await loadInfo();
  try { const k = localStorage.getItem(STORE); const i = info.stations.findIndex((s) => s.key === k); if (i >= 0) last = i; } catch { /* private mode */ }
  buildDial();
  status(`Tap ▶ or a station to start${last ? `: last time it was ${info.stations[last].name}` : ''}.`);
  await loadDj();
  requestAnimationFrame(frame);
  setInterval(refresh, 1000);
} catch (e) { status(e.message, true); console.error(e); }
window.radio = { tune, step, toggle, get station() { return station; }, get schedule() { return sched; }, get ctx() { return ctx; }, get out() { return out; }, get level() { return level; } };
