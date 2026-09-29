// The music: a small step sequencer playing arranged songs (tracks.js) on
// synthesized instruments, all in WebAudio.
//
// Per song:
//   drum voices ─────────────────────────────┐
//   instrument channels ─ [pump] ─ song filter ─ song bus ─┐
//          └ sends ─ song reverb send ─ shared reverb ─────┼─ out (the game's music bus)
//          └ sends ─ song ping-pong delay ─ song filter    │
//   risers / sweeps ───────────────────────────────────────┘
//
// Every song gets its own channels, delay and bus, so a skip can fade the old
// one out while the next starts, and the per-song mix (levels, sends, pump,
// drive, chorus) never leaks into the next. Drums are pre-rendered buffers
// (samples.js): one source + gain per hit. Synth notes build a few nodes per
// note (a pad chord is one filter/amp around all its oscillators), which keeps
// a busy 16th at a couple of dozen nodes, not hundreds.
//
// Timing: a lookahead scheduler (setTimeout every 25 ms, events placed
// ~0.2 s ahead on the audio clock). If the main thread stalls long enough that
// a step is already in the past, that step is dropped rather than played late,
// so the groove stays on the grid. pumpUntil() is public so an
// OfflineAudioContext can drive it to render songs faster than real time.

import { renderKit } from './samples.js';
import { TRACKS, PLAYLIST, LEVEL_TRACK } from './tracks.js';

const mtof = (m) => 440 * Math.pow(2, (m - 69) / 12);
const LOOKAHEAD = 0.2;
// The kit buffers are normalised near full scale; this sits them under the synths.
const DRUM_TRIM = 0.16;
const TICK_MS = 25;

// ── Notes, chords, voicings ──────────────────────────────────────
const PC = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 };
function pcOf(s) {
  let p = PC[s[0]];
  if (s[1] === '#') p++; else if (s[1] === 'b') p--;
  return (p + 12) % 12;
}
export function noteToMidi(tok) {
  const m = /^([A-G])([#b]?)(-?\d)$/.exec(tok);
  if (!m) return null;
  return 12 * (Number(m[3]) + 1) + pcOf(m[1] + m[2]);
}
const QUAL = {
  '': [0, 4, 7], m: [0, 3, 7], 7: [0, 4, 7, 10], m7: [0, 3, 7, 10], maj7: [0, 4, 7, 11], sus2: [0, 2, 7], sus4: [0, 5, 7],
  5: [0, 7, 12], add9: [0, 4, 7, 14], madd9: [0, 3, 7, 14], m9: [0, 3, 7, 10, 14], maj9: [0, 4, 7, 11, 14], dim: [0, 3, 6],
  6: [0, 4, 7, 9], m6: [0, 3, 7, 9], '7sus4': [0, 5, 7, 10], aug: [0, 4, 8], 9: [0, 4, 7, 10, 14],
};
function parseChord(name) {
  const [head, slash] = name.split('/');
  const m = /^([A-G][#b]?)(.*)$/.exec(head);
  const root = pcOf(m[1]);
  const iv = QUAL[m[2]] ?? QUAL[''];
  return { name, root, iv, bass: slash ? pcOf(slash) : root };
}

// Chord tones between lo and ~lo+16, choosing the inversion that moves the
// least from the previous voicing (smooth voice leading).
function voice(ch, lo, prev) {
  const pcs = [...new Set(ch.iv.map((i) => (ch.root + i) % 12))];
  let best = null, bestScore = Infinity;
  for (let r = 0; r < pcs.length; r++) {
    const notes = [];
    let m = lo + ((pcs[r] - lo) % 12 + 12) % 12;
    for (let k = 0; k < pcs.length; k++) {
      const pc = pcs[(r + k) % pcs.length];
      if (k > 0) { m++; while ((m % 12 + 12) % 12 !== pc) m++; }
      notes.push(m);
    }
    const score = prev && prev.length ? notes.reduce((a, n, i) => a + Math.abs(n - (prev[i] ?? prev[prev.length - 1])), 0) : notes[0] - lo;
    if (score < bestScore) { bestScore = score; best = notes; }
  }
  return best;
}

// ── Pattern compilation ──────────────────────────────────────────
// Drum lanes: one char per 16th. X accent, x hit, o ghost, . rest.
const VEL = { X: 1, x: 0.8, o: 0.42 };
// Bass lanes: one char per 16th. r root, o octave, f fifth, t third,
// s seventh, l root an octave down, u fifth an octave up; uppercase = accent;
// '-' holds, '~' holds and slides into the next note, '.' rest.
const BASS_DEG = { r: 0, o: 12, f: 7, l: -12, u: 19 };
function compileBass(str) {
  const s = str.replace(/\s+/g, ''), n = s.length, out = new Array(n).fill(null);
  for (let i = 0; i < n; i++) {
    const c = s[i];
    if ('-~.'.includes(c)) continue;
    let len = 1, slide = false;
    while (i + len < n && (s[i + len] === '-' || s[i + len] === '~')) { if (s[i + len] === '~') slide = true; len++; }
    out[i] = { deg: c.toLowerCase(), accent: c !== c.toLowerCase(), len, slide };
  }
  return out;
}
// Chord lanes: x hit, X accent, '-' hold, '.' rest.
function compileHits(str) {
  const s = str.replace(/\s+/g, ''), n = s.length, out = new Array(n).fill(null);
  for (let i = 0; i < n; i++) {
    if (s[i] !== 'x' && s[i] !== 'X') continue;
    let len = 1;
    while (i + len < n && s[i + len] === '-') len++;
    out[i] = { vel: s[i] === 'X' ? 1 : 0.8, len };
  }
  return out;
}
// Token lanes (arps, melodies): space separated, each token `res` 16ths long.
// A token is a note (C#5), a chord-tone index (arps), '_' hold or '.' rest.
// A trailing ! accents.
function compileTokens(str, res, arp) {
  const toks = str.trim().split(/\s+/);
  const n = toks.length * res, out = new Array(n).fill(null);
  for (let i = 0; i < toks.length; i++) {
    let t = toks[i];
    if (t === '_' || t === '.') continue;
    const accent = t.endsWith('!');
    if (accent) t = t.slice(0, -1);
    let len = 1;
    while (i + len < toks.length && toks[i + len] === '_') len++;
    const v = arp ? { idx: Number(t) } : { midi: noteToMidi(t) };
    if (!arp && v.midi == null) throw new Error('bad note ' + t);
    out[i * res] = { ...v, len: len * res, accent };
  }
  return out;
}

export function compileTrack(T) {
  if (T._c) return T._c;
  const prog = {};
  for (const [k, v] of Object.entries(T.prog)) prog[k] = v.trim().split(/\s+/).map((bar) => bar.split(',').map(parseChord));
  const drums = {};
  for (const [k, lanes] of Object.entries(T.drums)) {
    drums[k] = Object.entries(lanes).map(([voice, str]) => ({ voice, steps: str.replace(/\s+/g, '').split('') }));
  }
  const parts = {};
  for (const [name, p] of Object.entries(T.parts)) {
    const pats = {};
    for (const [k, str] of Object.entries(p.pat)) {
      pats[k] = p.type === 'bass' ? compileBass(str) : p.type === 'chord' ? compileHits(str) : compileTokens(str, p.res || 1, p.type === 'arp');
    }
    parts[name] = { ...p, pats };
  }
  T._c = { prog, drums, parts };
  return T._c;
}

// Drum fills for the last bar of a section, from step `from`. They replace
// the snare/tom/hat lanes; the kick keeps its own pattern unless the fill has one.
const FILLS = {
  snare: { from: 8, lanes: { snare: '........x.xxXxXX', kick: 'x.......x.......' } },
  tom: { from: 8, lanes: { tomH: '........x.x.....', tomM: '............x.x.', tomL: '..............xX', kick: 'x.......x.......' } },
  roll: { from: 0, ramp: true, lanes: { snare: 'x.x.x.x.xxxxxxxx', kick: 'x...x...x...x...' } },
  dnb: { from: 8, lanes: { snare: '........X.oXxoXX', kick: 'x.........x.....' } },
  west: { from: 8, lanes: { tomL: '........x..x..x.', tomM: '..........x..x..', snare: '..............xX', kick: 'x.......x.......' } },
  crash: { from: 12, lanes: { snare: '............XXXX', kick: 'x.......x...x...' } },
};
const FILL_REPLACES = new Set(['snare', 'clap', 'hat', 'ohat', 'ride', 'shaker', 'rim', 'snap', 'tomL', 'tomM', 'tomH']);
for (const f of Object.values(FILLS)) f.c = Object.entries(f.lanes).map(([voice, str]) => ({ voice, steps: str.split('') }));

// Default kit: buffer, level, reverb send, playback rate. Levels are set
// by measured RMS in a full mix (kick ≈ -3 dB under the whole mix, the snare
// ~8 dB under the kick, hats ~15 dB under). A track's kit entries override
// the buffer / send / rate, and scale the level (g is a multiplier there).
const KIT = {
  kick: { s: 'kickPunch', g: 0.9, rev: 0 },
  snare: { s: 'snareGated', g: 1.7, rev: 0.25 },
  clap: { s: 'clap', g: 3, rev: 0.3 },
  hat: { s: 'hat', g: 1.5, rev: 0 },
  ohat: { s: 'ohat', g: 0.9, rev: 0.05 },
  ride: { s: 'ride', g: 0.7, rev: 0.05 },
  crash: { s: 'crash', g: 0.6, rev: 0.2 },
  revCrash: { s: 'revCrash', g: 0.6, rev: 0.2 },
  shaker: { s: 'shaker', g: 0.9, rev: 0.05 },
  rim: { s: 'rim', g: 1.2, rev: 0.2 },
  snap: { s: 'snap', g: 1.2, rev: 0.35 },
  tomL: { s: 'tomL', g: 1, rev: 0.25 },
  tomM: { s: 'tomM', g: 1, rev: 0.25 },
  tomH: { s: 'tomH', g: 1, rev: 0.25 },
  boom: { s: 'boom', g: 0.9, rev: 0.1 },
};

export class Music {
  constructor(ctx, out) {
    this.ctx = ctx;
    this.out = out;
    this.song = null;
    this.on = false;
    this.onTrack = null; // (info) => void, called when a track starts playing
    this.realtime = !(typeof OfflineAudioContext !== 'undefined' && ctx instanceof OfflineAudioContext);
    this.playlist = PLAYLIST.slice();
    this._timer = null;
    this.solo = null; // mixing aid: a part name, or 'drums', plays alone
  }

  static get tracks() { return TRACKS.map(({ id, title, style, bpm }) => ({ id, title, style, bpm })); }
  static levelTrack(level) { return LEVEL_TRACK[level] || PLAYLIST[0]; }

  build() {
    const ctx = this.ctx;
    this.kit = renderKit(ctx);
    // Pulse waves for the square-ish leads (25 % duty: hollow and nasal).
    const H = 64, re = new Float32Array(H), im = new Float32Array(H);
    // Cosine terms: a sine-only series of the same magnitudes is a spiky, quiet wave.
    for (let h = 1; h < H; h++) re[h] = (2 / (h * Math.PI)) * Math.sin(h * Math.PI * 0.25);
    this.pulse = ctx.createPeriodicWave(re, im);
    // Shared hall reverb: stereo, 2.6 s, darkening as it decays, with a
    // short pre-delay and a few early reflections.
    const sr = ctx.sampleRate, len = Math.floor(sr * 2.6);
    const ir = ctx.createBuffer(2, len, sr);
    for (let c = 0; c < 2; c++) {
      const d = ir.getChannelData(c);
      let lp = 0, seed = c ? 0x9e3779b9 : 0x7f4a7c15;
      const rand = () => { seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5; return ((seed >>> 0) / 4294967296) * 2 - 1; };
      const pre = Math.floor(0.018 * sr);
      for (let i = pre; i < len; i++) {
        const t = (i - pre) / sr, u = i / len;
        const k = 0.75 - 0.6 * u; // one-pole low-pass that closes over the tail
        lp += k * (rand() - lp);
        d[i] = lp * Math.exp((-6.9 * t) / 2.3) * (t < 0.03 ? t / 0.03 : 1);
      }
      for (let k = 0; k < 10; k++) {
        const at = pre + Math.floor(sr * (0.005 + 0.06 * ((k * 0.37 + c * 0.19) % 1)));
        d[at] += (k % 2 ? -1 : 1) * 0.35 * (1 - k / 12);
      }
    }
    this.reverb = ctx.createConvolver();
    this.reverb.buffer = ir;
    this.revOut = ctx.createGain(); this.revOut.gain.value = 0.42;
    this.reverb.connect(this.revOut); this.revOut.connect(this.out);
  }

  get current() { return this.song ? this._info(this.song.T) : null; }
  // The playing track, else the one queued to play when music starts.
  get info() {
    if (this.song) return this._info(this.song.T);
    if (this._pending) return this._info(this._pending.T);
    const T = TRACKS.find((x) => x.id === this.wanted);
    return T ? this._info(T) : null;
  }
  _info(T) { return { id: T.id, title: T.title, style: T.style, bpm: T.bpm }; }

  // ── Transport ───────────────────────────────────────────────────
  // Start (or keep) a track. If it is already the one playing, nothing
  // changes, so restarting a race doesn't restart the song.
  play(id, { bar = 0, fade = true } = {}) {
    const T = TRACKS.find((t) => t.id === id) || TRACKS[0];
    if (this.song && this.song.T === T && !bar) { this._pending = null; this.wanted = T.id; return; }
    this.wanted = T.id;
    if (!this.on) { this._pending = { T, bar }; return; }
    const t = this.ctx.currentTime;
    const start = this.song && fade ? t + 0.55 : t + 0.1;
    if (this.song) this._retire(this.song, t, fade ? 0.12 : 0.02);
    this._begin(T, start, bar);
  }

  next() {
    const cur = this.song?.T.id ?? this._pending?.T.id ?? this.wanted;
    const i = this.playlist.indexOf(cur);
    const id = this.playlist[(i + 1) % this.playlist.length];
    if (!this.on) { this._pending = { T: TRACKS.find((t) => t.id === id), bar: 0 }; this.wanted = id; return this._info(this._pending.T); }
    this.play(id);
    return this.current;
  }

  // Scheduling on/off. Stopping keeps the position; starting again picks the
  // song up from the start of the bar it stopped in.
  start() {
    if (this.on) return;
    this.on = true;
    const t = this.ctx.currentTime;
    if (this._pending) { const { T, bar } = this._pending; this._pending = null; if (this.song) this._retire(this.song, t, 0.02); this._begin(T, t + 0.1, bar); }
    else if (this.song) { this.pos.step = 0; this.nextT = t + 0.1; this.song.bus.gain.cancelScheduledValues(t); this.song.bus.gain.setTargetAtTime(this.song.T.gain ?? 1, t, 0.05); }
    else this._begin(TRACKS.find((x) => x.id === this.wanted) || TRACKS[0], t + 0.1, 0);
    if (this.realtime) this._tick();
  }

  stop() {
    this.on = false;
    clearTimeout(this._timer);
  }

  onResume() {
    if (this.on && this.song) this.nextT = Math.max(this.nextT, this.ctx.currentTime + 0.05);
  }

  _tick() {
    if (!this.on) return;
    this.pumpUntil(this.ctx.currentTime + LOOKAHEAD);
    this._timer = setTimeout(() => this._tick(), TICK_MS);
  }

  pumpUntil(until) {
    const now = this.ctx.currentTime;
    let guard = 0;
    while (this.song && this.nextT < until && guard++ < 256) {
      const S = this.song, T = S.T;
      const late = this.realtime && this.nextT < now - 0.03;
      if (!late) this._step(this.nextT);
      this.nextT += S.stepDur;
      const p = this.pos;
      if (++p.step === 16) {
        p.step = 0;
        if (++p.bar >= T.sections[p.sec].bars) {
          p.bar = 0;
          if (++p.sec >= T.sections.length) {
            // Song over: let it ring out, then the playlist moves on.
            const i = this.playlist.indexOf(T.id);
            const nextT = TRACKS.find((x) => x.id === this.playlist[(i + 1) % this.playlist.length]);
            this._retire(S, this.nextT + 2.5, 0.8);
            this._begin(nextT, this.nextT + 1.2, 0);
          }
        }
      }
    }
  }

  // ── Song setup / teardown ──────────────────────────────────────
  _begin(T, time, bar = 0) {
    const ctx = this.ctx;
    const c = compileTrack(T);
    const S = { T, c, stepDur: 60 / T.bpm / 4, ch: {}, dv: {}, lfos: [], voicing: {}, last: {} };
    S.bus = ctx.createGain(); S.bus.gain.value = 0;
    S.bus.gain.setValueAtTime(0, time - 0.01);
    S.bus.gain.linearRampToValueAtTime(T.gain ?? 1, time + 0.03);
    S.bus.connect(this.out);
    S.filter = ctx.createBiquadFilter(); S.filter.type = 'lowpass'; S.filter.frequency.value = 20000; S.filter.Q.value = 0.9;
    S.filter.connect(S.bus);
    S.pump = ctx.createGain(); S.pump.connect(S.filter);
    S.rev = ctx.createGain(); S.rev.gain.value = T.rev ?? 1; S.rev.connect(this.reverb);
    // Ping-pong delay, tempo-synced (dotted eighth unless the track says otherwise).
    const dt = (T.delay ?? 0.75) * (60 / T.bpm);
    S.dly = ctx.createGain(); S.dly.gain.value = 1;
    const dL = ctx.createDelay(2), dR = ctx.createDelay(2);
    dL.delayTime.value = dt; dR.delayTime.value = dt;
    const fb = ctx.createGain(); fb.gain.value = T.delayFb ?? 0.38;
    const dlp = ctx.createBiquadFilter(); dlp.type = 'lowpass'; dlp.frequency.value = 3200;
    const dhp = ctx.createBiquadFilter(); dhp.type = 'highpass'; dhp.frequency.value = 280;
    const merge = ctx.createChannelMerger(2);
    const dOut = ctx.createGain(); dOut.gain.value = 0.5;
    S.dly.connect(dhp); dhp.connect(dL); dL.connect(merge, 0, 0); dL.connect(dlp); dlp.connect(fb); fb.connect(dR);
    dR.connect(merge, 0, 1); dR.connect(dL);
    merge.connect(dOut); dOut.connect(S.filter);
    // Instrument channels.
    for (const [name, p] of Object.entries(c.parts)) S.ch[name] = this._channel(S, p.ch || {});
    this.song = S;
    this.pos = { sec: 0, bar: 0, step: 0 };
    // Starting mid-song (tests, WAV renders): jump to the section holding `bar`.
    let b = bar;
    while (b > 0 && this.pos.sec < T.sections.length - 1 && b >= T.sections[this.pos.sec].bars) { b -= T.sections[this.pos.sec].bars; this.pos.sec++; }
    this.pos.bar = Math.min(b, T.sections[this.pos.sec].bars - 1);
    this.nextT = time;
    const info = this._info(T);
    if (this.onTrack) {
      const fire = () => { if (this.song === S) this.onTrack(info); };
      if (this.realtime) setTimeout(fire, Math.max(0, (time - ctx.currentTime) * 1000)); else fire();
    }
  }

  _retire(S, time, tc) {
    for (const g of [S.bus.gain, S.rev.gain, S.dly.gain]) { g.cancelScheduledValues(time); g.setTargetAtTime(0, time, tc); }
    if (this.song === S) this.song = null;
    const wait = Math.max(0, time - this.ctx.currentTime) + tc * 8 + 3;
    const kill = () => {
      for (const o of S.lfos) { try { o.stop(); } catch { /* already stopped */ } }
      S.bus.disconnect(); S.rev.disconnect(); S.dly.disconnect();
    };
    if (this.realtime) setTimeout(kill, wait * 1000);
  }

  // A mixer channel: level → [drive] → [high-pass] → [chorus] → pan → pump or
  // song filter, with reverb and delay sends.
  _channel(S, o) {
    const ctx = this.ctx;
    const inp = ctx.createGain();
    let node = inp;
    if (o.drive) {
      const sh = ctx.createWaveShaper();
      const n = 1024, curve = new Float32Array(n);
      for (let i = 0; i < n; i++) { const x = (i * 2) / (n - 1) - 1; curve[i] = Math.tanh(o.drive * x) / Math.tanh(o.drive); }
      sh.curve = curve; sh.oversample = '2x';
      const post = ctx.createBiquadFilter(); post.type = 'lowpass'; post.frequency.value = o.driveLp ?? 5000;
      node.connect(sh); sh.connect(post); node = post;
    }
    if (o.hp) { const hp = ctx.createBiquadFilter(); hp.type = 'highpass'; hp.frequency.value = o.hp; node.connect(hp); node = hp; }
    const pan = ctx.createStereoPanner ? ctx.createStereoPanner() : ctx.createGain();
    if (pan.pan) pan.pan.value = o.pan ?? 0;
    if (o.chorus) {
      // Two modulated short delays panned apart, under the dry signal.
      for (const [base, rate, side] of [[0.011, 0.53, -0.8], [0.017, 0.71, 0.8]]) {
        const d = ctx.createDelay(0.05); d.delayTime.value = base;
        const lfo = ctx.createOscillator(); lfo.frequency.value = rate;
        const lg = ctx.createGain(); lg.gain.value = 0.0025 * o.chorus;
        lfo.connect(lg); lg.connect(d.delayTime); lfo.start(); S.lfos.push(lfo);
        const p = ctx.createStereoPanner ? ctx.createStereoPanner() : ctx.createGain();
        if (p.pan) p.pan.value = side;
        const wg = ctx.createGain(); wg.gain.value = 0.7;
        node.connect(d); d.connect(wg); wg.connect(p); p.connect(pan);
      }
    }
    node.connect(pan);
    // The fader comes after the drive, so level never changes the distortion.
    const lvl = ctx.createGain(); lvl.gain.value = o.level ?? 1;
    pan.connect(lvl);
    lvl.connect(o.pump ? S.pump : S.filter);
    if (o.rev) { const g = ctx.createGain(); g.gain.value = o.rev; lvl.connect(g); g.connect(S.rev); }
    if (o.dly) { const g = ctx.createGain(); g.gain.value = o.dly; lvl.connect(g); g.connect(S.dly); }
    return inp;
  }

  _drumVoice(S, name) {
    if (S.dv[name]) return S.dv[name];
    const ctx = this.ctx;
    const o = S.T.kit?.[name] || {};
    const k = { ...KIT[name], ...o, g: KIT[name].g * (o.g ?? 1) };
    const g = ctx.createGain(); g.gain.value = k.g * DRUM_TRIM;
    g.connect(S.filter);
    if (k.rev) { const r = ctx.createGain(); r.gain.value = k.rev; g.connect(r); r.connect(S.rev); }
    return (S.dv[name] = { in: g, buf: this.kit[k.s], rate: k.rate ?? 1 });
  }

  hit(S, name, time, vel = 1, rate = 1, offset = 0) {
    const v = this._drumVoice(S, name);
    if (!v.buf) return;
    const src = this.ctx.createBufferSource();
    src.buffer = v.buf;
    src.playbackRate.value = v.rate * rate;
    const g = this.ctx.createGain(); g.gain.value = vel;
    src.connect(g); g.connect(v.in);
    src.start(time, offset);
    if (name === 'kick' && S.T.pump) this._duck(S, time);
  }

  // Sidechain pump: every kick ducks the pumped channels and lets them swell back.
  _duck(S, time) {
    const { depth = 0.6, release = 0.16 } = S.T.pump;
    const g = S.pump.gain;
    g.setValueAtTime(1, time);
    g.linearRampToValueAtTime(1 - depth, time + 0.006);
    g.setTargetAtTime(1, time + 0.03, release / 3);
  }

  // ── The sequencer step ──────────────────────────────────────────
  _step(time) {
    const S = this.song, T = S.T, c = S.c, p = this.pos;
    const sec = T.sections[p.sec];
    const st = p.step, bar = p.bar;
    const t = time + (st % 2 ? (T.swing || 0) * S.stepDur : 0);
    const barDur = S.stepDur * 16;
    const lastBar = bar === sec.bars - 1;
    if (st === 0) this._barStart(S, sec, bar, t, barDur);
    // A gap: everything drops out for the last steps before a drop.
    if (lastBar && sec.gap && st >= 16 - sec.gap) return;

    // Harmony.
    const prog = c.prog[sec.prog || 'a'];
    const pbar = prog[bar % prog.length];
    const ci = Math.floor((st * pbar.length) / 16);
    const chord = pbar[ci];
    const chordStart = (st * pbar.length) % 16 === 0;

    // Drums (with the fill over the end of the last bar).
    const idx = bar * 16 + st;
    const fill = lastBar && sec.fill ? FILLS[sec.fill] : null;
    const inFill = fill && st >= fill.from;
    const solo = this.solo;
    const lanes = sec.drums && (!solo || solo === 'drums' || KIT[solo]) ? c.drums[sec.drums] : null;
    if (lanes) {
      for (const L of lanes) {
        if (inFill && (FILL_REPLACES.has(L.voice) || (L.voice === 'kick' && fill.lanes.kick))) continue;
        if (solo && solo !== 'drums' && solo !== L.voice) continue;
        const ch = L.steps[idx % L.steps.length];
        if (ch !== '.') this.hit(S, L.voice, t, VEL[ch] * this._human(S, idx, L.voice), 1);
      }
    }
    if (inFill && (!solo || solo === 'drums')) {
      for (const L of fill.c) {
        const ch = L.steps[st];
        if (ch === '.') continue;
        const ramp = fill.ramp ? 0.35 + 0.65 * (st / 15) : 1;
        this.hit(S, L.voice, t, VEL[ch] * ramp, 1);
      }
    }

    // Instruments.
    for (const [name, key] of Object.entries(sec.p || {})) {
      if (solo && solo !== name) continue;
      const part = c.parts[name];
      const pat = key && part.pats[key];
      if (!pat) continue;
      const pidx = (bar * 16 + st) % pat.length;
      const ev = pat[pidx];
      if (!ev) continue;
      this._part(S, name, part, ev, chord, chordStart, t);
    }
  }

  // Deterministic per-hit humanising (±6 %), so repeats never sound pasted.
  _human(S, idx, voice) {
    let h = (idx * 2654435761 + voice.length * 97 + this.pos.sec * 131 + this.pos.bar * 17) >>> 0;
    h ^= h >>> 15;
    return 0.94 + ((h % 1000) / 1000) * 0.12;
  }

  _barStart(S, sec, bar, t, barDur) {
    const secDur = barDur * sec.bars;
    if (bar === 0) {
      if (sec.crash) this.hit(S, 'crash', t, 1);
      if (sec.drop) this.hit(S, 'boom', t, 1);
      const f = S.filter.frequency;
      f.cancelScheduledValues(t);
      if (sec.lp) {
        f.setValueAtTime(sec.lp[0], t);
        f.exponentialRampToValueAtTime(sec.lp[1], t + secDur);
      } else f.setValueAtTime(20000, t);
    }
    const left = sec.bars - bar;
    if (sec.riser && left === sec.riser) this._riser(S, t, barDur * sec.riser);
    if (sec.down && bar === 0) this._downlifter(S, t, barDur * sec.down);
    // A reverse crash sucking into the next section's downbeat.
    // It can be longer than a bar: then it starts part-way in, on this downbeat.
    if (sec.swell && left === 1) {
      const start = t + barDur - this.kit.revCrash.duration;
      this.hit(S, 'revCrash', Math.max(t, start), 0.9, 1, Math.max(0, t - start));
    }
  }

  // White noise through a band-pass that sweeps up while it swells.
  _riser(S, t, dur) {
    const ctx = this.ctx;
    const src = ctx.createBufferSource();
    src.buffer = this._noiseBuf || (this._noiseBuf = this._makeNoise());
    src.loop = true;
    const bp = ctx.createBiquadFilter(); bp.type = 'bandpass'; bp.Q.value = 1.4;
    bp.frequency.setValueAtTime(350, t); bp.frequency.exponentialRampToValueAtTime(7500, t + dur);
    const g = ctx.createGain();
    g.gain.setValueAtTime(0.0001, t); g.gain.exponentialRampToValueAtTime(0.16 * (S.T.riserGain ?? 1), t + dur * 0.98);
    g.gain.linearRampToValueAtTime(0, t + dur + 0.02);
    src.connect(bp); bp.connect(g); g.connect(S.bus);
    const rg = ctx.createGain(); rg.gain.value = 0.5; g.connect(rg); rg.connect(S.rev);
    src.start(t); src.stop(t + dur + 0.05);
  }

  _downlifter(S, t, dur) {
    const ctx = this.ctx;
    const src = ctx.createBufferSource(); src.buffer = this._noiseBuf || (this._noiseBuf = this._makeNoise()); src.loop = true;
    const bp = ctx.createBiquadFilter(); bp.type = 'bandpass'; bp.Q.value = 1.2;
    bp.frequency.setValueAtTime(6000, t); bp.frequency.exponentialRampToValueAtTime(250, t + dur);
    const g = ctx.createGain();
    g.gain.setValueAtTime(0.12 * (S.T.riserGain ?? 1), t); g.gain.exponentialRampToValueAtTime(0.0005, t + dur);
    src.connect(bp); bp.connect(g); g.connect(S.bus);
    const rg = ctx.createGain(); rg.gain.value = 0.6; g.connect(rg); rg.connect(S.rev);
    src.start(t); src.stop(t + dur + 0.05);
  }

  _makeNoise() {
    const ctx = this.ctx, n = ctx.sampleRate * 2, b = ctx.createBuffer(2, n, ctx.sampleRate);
    for (let c = 0; c < 2; c++) { const d = b.getChannelData(c); for (let i = 0; i < n; i++) d[i] = Math.random() * 2 - 1; }
    return b;
  }

  _part(S, name, part, ev, chord, chordStart, t) {
    const dur = ev.len * S.stepDur;
    const dest = S.ch[name];
    const inst = part.inst;
    if (part.type === 'bass') {
      const lo = part.lo ?? 33;
      const root = lo + ((chord.bass - lo) % 12 + 12) % 12;
      const d = ev.deg;
      const iv = d === 't' ? chord.iv[1] : d === 's' ? (chord.iv[3] ?? 10) : BASS_DEG[d] ?? 0;
      const midi = root + iv;
      const prev = S.last[name];
      const glideFrom = prev && prev.slide ? prev.midi : null;
      this.note(dest, t, [midi], dur * (ev.slide ? 1.05 : part.gate ?? 0.9), inst, ev.accent ? 1 : 0.82, glideFrom);
      S.last[name] = { midi, slide: ev.slide };
    } else if (part.type === 'chord') {
      if (chordStart || !S.voicing[name] || S.voicing[name].chord !== chord) {
        S.voicing[name] = { chord, notes: voice(chord, part.lo ?? 55, S.voicing[name]?.notes) };
      }
      this.note(dest, t, S.voicing[name].notes, dur * (part.gate ?? 1), inst, ev.vel);
    } else if (part.type === 'arp') {
      const v = voice(chord, part.lo ?? 60, null);
      const k = v.length, i = ev.idx;
      const midi = v[((i % k) + k) % k] + 12 * Math.floor(i / k);
      this.note(dest, t, [midi], dur * (part.gate ?? 0.8), inst, ev.accent ? 1 : 0.8);
    } else {
      const prev = S.last[name];
      this.note(dest, t, [ev.midi], dur * (part.gate ?? 0.95), inst, ev.accent ? 1 : 0.85, part.legato && prev && prev.end > t - 0.01 ? prev.midi : null);
      S.last[name] = { midi: ev.midi, end: t + dur };
    }
  }

  // ── Instruments ────────────────────────────────────────────────
  // One note or chord on a synth patch (see tracks.js for the fields).
  note(dest, t, midis, dur, P, vel = 1, glideFrom = null) {
    const ctx = this.ctx;
    const a = P.a ?? 0.004, d = P.d ?? 0.25, s = P.s ?? 0.7, r = P.r ?? 0.12;
    const end = t + Math.max(dur, a);
    const stopAt = end + r + 0.05;
    const amp = ctx.createGain();
    const peak = (P.gain ?? 0.1) * vel / Math.sqrt(midis.length);
    amp.gain.setValueAtTime(0, t);
    amp.gain.linearRampToValueAtTime(peak, t + a);
    if (s < 1) amp.gain.setTargetAtTime(peak * s, t + a, d / 3);
    amp.gain.setTargetAtTime(0, end, r / 5);
    amp.connect(dest);

    // FM: carrier + modulator(s) per note, no filter.
    if (P.type === 'fm') {
      for (const m of midis) {
        const f = mtof(m + 12 * (P.oct ?? 0));
        for (const det of P.fmDetune ? [-P.fmDetune, P.fmDetune] : [0]) {
          const car = ctx.createOscillator(); car.frequency.value = f; car.detune.value = det;
          this._bend(car, t, f, P, glideFrom);
          for (const M of P.mods || [{ ratio: 1, index: 2, dec: 0.4, sus: 0.2 }]) {
            const mod = ctx.createOscillator(); mod.frequency.value = f * M.ratio;
            const mg = ctx.createGain();
            const depth = M.index * f * M.ratio * (0.6 + 0.4 * vel);
            mg.gain.setValueAtTime(depth, t);
            mg.gain.setTargetAtTime(depth * (M.sus ?? 0), t, (M.dec ?? 0.3) / 3);
            mod.connect(mg); mg.connect(car.frequency);
            mod.start(t); mod.stop(stopAt);
          }
          const cg = P.fmDetune ? ctx.createGain() : null;
          if (cg) { cg.gain.value = 0.6; car.connect(cg); cg.connect(amp); } else car.connect(amp);
          car.start(t); car.stop(stopAt);
        }
      }
      return;
    }

    // Subtractive: detuned oscillator stack (split L/R when wide) → low-pass.
    let into = amp;
    const baseCut = P.cutoff ?? 20000;
    if (baseCut < 18000 || P.fenv) {
      const lp = ctx.createBiquadFilter(); lp.type = 'lowpass'; lp.Q.value = P.q ?? 0.8;
      const kt = P.keytrack ? Math.pow(2, ((midis[0] - 60) / 12) * P.keytrack) : 1;
      const cut = Math.min(18000, baseCut * kt);
      if (P.fenv) {
        const top = Math.min(18000, cut * (1 + P.fenv * vel));
        lp.frequency.setValueAtTime(top, t);
        lp.frequency.setTargetAtTime(cut, t, (P.fdec ?? 0.15) / 3);
      } else if (P.fattack) {
        lp.frequency.setValueAtTime(cut * 0.2, t);
        lp.frequency.setTargetAtTime(cut, t, P.fattack / 3);
      } else lp.frequency.value = cut;
      lp.connect(amp);
      into = lp;
    }
    const nv = P.voices ?? 1, spread = P.detune ?? 0;
    let sides = [into, into];
    if (P.width && nv > 1 && ctx.createStereoPanner) {
      sides = [-P.width, P.width].map((pv) => { const g = ctx.createGain(); const pn = ctx.createStereoPanner(); pn.pan.value = pv; g.connect(pn); pn.connect(into); return g; });
    }
    const mix = 1 / Math.sqrt(nv);
    const mixers = sides[0] === sides[1] ? [ctx.createGain()] : sides.map(() => ctx.createGain());
    mixers.forEach((g, i) => { g.gain.value = mix; g.connect(sides[i] ?? into); });
    let vib = null;
    if (P.vib) {
      vib = ctx.createGain();
      vib.gain.setValueAtTime(0, t);
      vib.gain.linearRampToValueAtTime(P.vib, t + (P.vibDelay ?? 0.3) + 0.2);
      const lfo = ctx.createOscillator(); lfo.frequency.value = P.vibRate ?? 5.5;
      lfo.connect(vib); lfo.start(t); lfo.stop(stopAt);
    }
    for (const m of midis) {
      const f = mtof(m + 12 * (P.oct ?? 0));
      for (let v = 0; v < nv; v++) {
        const o = ctx.createOscillator();
        if (P.type === 'pulse') o.setPeriodicWave(this.pulse); else o.type = P.type === 'tri' ? 'triangle' : P.type || 'sawtooth';
        o.frequency.value = f;
        o.detune.value = nv > 1 ? (v / (nv - 1) - 0.5) * spread : 0;
        this._bend(o, t, f, P, glideFrom);
        if (vib) vib.connect(o.detune);
        o.connect(mixers[v % mixers.length]);
        o.start(t); o.stop(stopAt);
      }
      if (P.sub) {
        // An octave down, unless that falls below ~40 Hz (felt, not heard, and
        // it eats headroom): then it doubles the fundamental instead.
        const down = f / 2 >= 40;
        const fs = down ? f / 2 : f;
        const o = ctx.createOscillator(); o.type = P.subType || 'sine'; o.frequency.value = fs;
        this._bend(o, t, fs, P, glideFrom != null ? glideFrom - (down ? 12 : 0) : null);
        const g = ctx.createGain(); g.gain.value = P.sub;
        o.connect(g); g.connect(into === amp ? amp : into);
        o.start(t); o.stop(stopAt);
      }
    }
  }

  _bend(o, t, f, P, glideFrom) {
    if (glideFrom != null) {
      const f0 = mtof(glideFrom + 12 * (P.oct ?? 0));
      o.frequency.setValueAtTime(f0, t);
      o.frequency.exponentialRampToValueAtTime(f, t + (P.glide ?? 0.06));
    } else if (P.bend) {
      o.frequency.setValueAtTime(f * Math.pow(2, -P.bend / 12), t);
      o.frequency.exponentialRampToValueAtTime(f, t + (P.bendT ?? 0.06));
    }
  }
}
