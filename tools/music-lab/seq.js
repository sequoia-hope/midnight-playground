// Music Lab: the sequencer. Reads the game's song format (tracks.js, see its
// header) and turns each 16th into events, the way Music.js does it, but
// with no audio nodes: the worklet plays the events sample-accurately on the
// lab's instruments, and the tests can read them directly.
//
// Compilation, voicing, fills and the per-hit humanising are Music.js's,
// copied (that file stays the game's and is used read-only here). Lab
// extensions to the format, used by the generated tracks (gen.js):
//   section.auto  { 'part.param': [from, to] } ramps a patch parameter over
//                 the section (techno's slow filter motion);
//   T.lay         { lane or part: energy } mutes that lane or part while the
//                 live energy (0..1) is below the value;
//   bass 'n'      the next chord's root (a pickup into the change), 'N'
//                 accented; the same note as 'r' when the chord stays.

const PC = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 };
function pcOf(s) {
  let p = PC[s[0]];
  if (s[1] === '#') p++; else if (s[1] === 'b') p--;
  return (p + 12) % 12;
}
export function noteToMidi(tok) {
  const m = /^([A-G])([#b]?)(-?\d)$/.exec(tok);
  if (!m) return null;
  return 12 * (Number(m[3]) + 1) + PC[m[1]] + (m[2] === '#' ? 1 : m[2] === 'b' ? -1 : 0);
}
const QUAL = {
  '': [0, 4, 7], m: [0, 3, 7], 7: [0, 4, 7, 10], m7: [0, 3, 7, 10], maj7: [0, 4, 7, 11], sus2: [0, 2, 7], sus4: [0, 5, 7],
  5: [0, 7, 12], add9: [0, 4, 7, 14], madd9: [0, 3, 7, 14], m9: [0, 3, 7, 10, 14], maj9: [0, 4, 7, 11, 14], dim: [0, 3, 6],
  6: [0, 4, 7, 9], m6: [0, 3, 7, 9], '7sus4': [0, 5, 7, 10], aug: [0, 4, 8], 9: [0, 4, 7, 10, 14], m11: [0, 3, 7, 10, 14, 17],
  m7b5: [0, 3, 6, 10], // lab: the half-diminished 7th on a scale's diminished degree (compose.js)
};
export function parseChord(name) {
  const [head, slash] = name.split('/');
  const m = /^([A-G][#b]?)(.*)$/.exec(head);
  const root = pcOf(m[1]);
  const iv = QUAL[m[2]] ?? QUAL[''];
  return { name, root, iv, bass: slash ? pcOf(slash) : root };
}

// Chord tones from lo up, in the inversion that moves least from the last.
export function voice(ch, lo, prev) {
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

const VEL = { X: 1, x: 0.8, o: 0.42 };
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
  const prog = {};
  for (const [k, v] of Object.entries(T.prog)) prog[k] = v.trim().split(/\s+/).map((bar) => bar.split(',').map(parseChord));
  const drums = {};
  for (const [k, lanes] of Object.entries(T.drums)) drums[k] = Object.entries(lanes).map(([v, str]) => ({ voice: v, steps: str.replace(/\s+/g, '').split('') }));
  const parts = {};
  for (const [name, p] of Object.entries(T.parts)) {
    const pats = {};
    for (const [k, str] of Object.entries(p.pat)) pats[k] = p.type === 'bass' ? compileBass(str) : p.type === 'chord' ? compileHits(str) : compileTokens(str, p.res || 1, p.type === 'arp');
    parts[name] = { ...p, pats };
  }
  return { prog, drums, parts };
}

export const FILLS = {
  snare: { from: 8, lanes: { snare: '........x.xxXxXX', kick: 'x.......x.......' } },
  tom: { from: 8, lanes: { tomH: '........x.x.....', tomM: '............x.x.', tomL: '..............xX', kick: 'x.......x.......' } },
  roll: { from: 0, ramp: true, lanes: { snare: 'x.x.x.x.xxxxxxxx', kick: 'x...x...x...x...' } },
  dnb: { from: 8, lanes: { snare: '........X.oXxoXX', kick: 'x.........x.....' } },
  west: { from: 8, lanes: { tomL: '........x..x..x.', tomM: '..........x..x..', snare: '..............xX', kick: 'x.......x.......' } },
  crash: { from: 12, lanes: { snare: '............XXXX', kick: 'x.......x...x...' } },
  // Lab: a clap build for house, a hat-and-rim one for techno, a kick roll
  // for trance and psy, and a 2-step turnaround for garage.
  clap: { from: 8, lanes: { clap: '........x.x.xxxx', kick: 'x...x...x.......' } },
  perc: { from: 8, lanes: { rim: '........x..x.x.x', hat: '........xxxxxxxx', kick: 'x...x...x...x...' } },
  kickroll: { from: 0, ramp: true, lanes: { kick: 'x...x...x.x.xxxx', snare: '........x.x.xxxx' } },
  skip: { from: 8, lanes: { snare: '........x..x.xoX', kick: 'x.....x.x...x...' } },
  // Chicha: the timbalero's abanico (a roll on the high drum opening onto
  // the low one and the bell) into the chorus; the congas and güiro play on.
  abanico: { from: 8, lanes: { timbaleH: '........xxxxxxx.', timbaleL: '...............X', cowbell: '...............x' } },
};
const FILL_REPLACES = new Set(['snare', 'clap', 'hat', 'ohat', 'ride', 'shaker', 'rim', 'snap', 'tomL', 'tomM', 'tomH', 'timbaleH', 'timbaleL', 'cascara']);
for (const f of Object.values(FILLS)) f.c = Object.entries(f.lanes).map(([v, str]) => ({ voice: v, steps: str.split('') }));
export const DRUM_LANES = ['kick', 'snare', 'clap', 'hat', 'ohat', 'ride', 'crash', 'revCrash', 'shaker', 'rim', 'snap', 'tomL', 'tomM', 'tomH', 'boom', 'cowbell',
  // The latin kit's (instruments.js KITS.latin).
  'congaO', 'congaS', 'tumba', 'bongoH', 'bongoL', 'timbaleH', 'timbaleL', 'cascara', 'guiroL', 'guiroS', 'clave'];

export class Seq {
  constructor(T) {
    this.T = T;
    this.c = compileTrack(T);
    this.stepDur = 60 / T.bpm / 4;
    this.voicing = {}; this.last = {};
    this.pos = { sec: 0, bar: 0, step: 0 };
    this.energy = 1;
    this.n = 0;
    this.mute = new Set();
    this.solo = null;
    this.bars = T.sections.reduce((a, s) => a + s.bars, 0);
  }

  seekBar(bar) {
    const T = this.T;
    this.pos = { sec: 0, bar: 0, step: 0 };
    let b = Math.max(0, bar);
    while (b > 0 && this.pos.sec < T.sections.length - 1 && b >= T.sections[this.pos.sec].bars) { b -= T.sections[this.pos.sec].bars; this.pos.sec++; }
    this.pos.bar = Math.min(b, T.sections[this.pos.sec].bars - 1);
    this.last = {};
  }
  get barIndex() { let b = this.pos.bar; for (let i = 0; i < this.pos.sec; i++) b += this.T.sections[i].bars; return b; }

  _on(name) {
    if (this.solo && this.solo !== name && !(this.solo === 'drums' && DRUM_LANES.includes(name))) return false;
    if (this.mute.has(name) || (DRUM_LANES.includes(name) && this.mute.has('drums'))) return false;
    const lay = this.T.lay?.[name];
    return lay == null || this.energy >= lay;
  }

  _human(idx, voiceName) {
    let h = (idx * 2654435761 + voiceName.length * 97 + this.pos.sec * 131 + this.pos.bar * 17) >>> 0;
    h ^= h >>> 15;
    return 0.94 + ((h % 1000) / 1000) * 0.12;
  }

  // The events of the current 16th, then advance. Each event's `dt` is its
  // offset from the step's grid time, in seconds (swing).
  step() {
    const ev = [];
    const T = this.T, c = this.c, p = this.pos;
    const sec = T.sections[p.sec];
    const st = p.step, bar = p.bar;
    const dt = st % 2 ? (T.swing || 0) * this.stepDur : 0;
    this._clock = this.n++ * this.stepDur + dt;
    const barDur = this.stepDur * 16;
    const lastBar = bar === sec.bars - 1;
    if (st === 0) this._barStart(ev, sec, bar, barDur);
    const gap = lastBar && sec.gap && st >= 16 - sec.gap;
    if (!gap) {
      const prog = c.prog[sec.prog || 'a'];
      const pbar = prog[bar % prog.length];
      const ci = Math.floor((st * pbar.length) / 16);
      const chord = pbar[ci];
      const chordStart = (st * pbar.length) % 16 === 0;
      // The chord after this one (for bass pickups): later in the bar, else
      // the next bar's first; at the section's end, the section's own loop.
      const nextChord = ci + 1 < pbar.length ? pbar[ci + 1] : prog[(bar + 1) % prog.length][0];
      const idx = bar * 16 + st;
      const fill = lastBar && sec.fill ? FILLS[sec.fill] : null;
      const inFill = fill && st >= fill.from;
      const lanes = sec.drums ? c.drums[sec.drums] : null;
      if (lanes) {
        for (const L of lanes) {
          if (inFill && (FILL_REPLACES.has(L.voice) || (L.voice === 'kick' && fill.lanes.kick))) continue;
          if (!this._on(L.voice)) continue;
          const ch = L.steps[idx % L.steps.length];
          if (ch !== '.') ev.push({ k: 'drum', lane: L.voice, vel: VEL[ch] * this._human(idx, L.voice), dt });
        }
      }
      if (inFill) {
        for (const L of fill.c) {
          const ch = L.steps[st];
          if (ch === '.' || !this._on(L.voice)) continue;
          const ramp = fill.ramp ? 0.35 + 0.65 * (st / 15) : 1;
          ev.push({ k: 'drum', lane: L.voice, vel: VEL[ch] * ramp, dt });
        }
      }
      for (const [name, key] of Object.entries(sec.p || {})) {
        if (!this._on(name)) continue;
        const part = c.parts[name];
        const pat = key && part?.pats[key];
        if (!pat) continue;
        const e = pat[(bar * 16 + st) % pat.length];
        if (!e) continue;
        const n = this._part(name, part, e, chord, chordStart, dt, nextChord);
        if (n) ev.push(n);
      }
    }
    // Advance.
    if (++p.step === 16) {
      p.step = 0;
      if (++p.bar >= sec.bars) {
        p.bar = 0;
        if (++p.sec >= T.sections.length) { p.sec = 0; ev.push({ k: 'end' }); }
      }
    }
    return ev;
  }

  _barStart(ev, sec, bar, barDur) {
    const secDur = barDur * sec.bars;
    if (bar === 0) {
      ev.push({ k: 'section', sec: this.pos.sec });
      if (sec.crash && this._on('crash')) ev.push({ k: 'drum', lane: 'crash', vel: 1, dt: 0 });
      if (sec.drop && this._on('boom')) ev.push({ k: 'drum', lane: 'boom', vel: 1, dt: 0 });
      ev.push(sec.lp ? { k: 'lp', from: sec.lp[0], to: sec.lp[1], dur: secDur } : { k: 'lp', from: 20000, to: 20000, dur: 0 });
      for (const [target, [from, to]] of Object.entries(sec.auto || {})) ev.push({ k: 'auto', target, from, to, dur: secDur });
    }
    const left = sec.bars - bar;
    if (sec.riser && left === sec.riser) ev.push({ k: 'riser', dur: barDur * sec.riser });
    if (sec.down && bar === 0) ev.push({ k: 'down', dur: barDur * sec.down });
    // The reverse crash ends on the next section's downbeat.
    if (sec.swell && left === 1 && this._on('revCrash')) ev.push({ k: 'drum', lane: 'revCrash', vel: 0.9, dt: 0, len: barDur });
  }

  _part(name, part, e, chord, chordStart, dt, nextChord = chord) {
    const dur = e.len * this.stepDur;
    if (part.type === 'bass') {
      const lo = part.lo ?? 33;
      const root = lo + ((chord.bass - lo) % 12 + 12) % 12;
      const d = e.deg;
      const iv = d === 't' ? chord.iv[1] : d === 's' ? (chord.iv[3] ?? 10) : BASS_DEG[d] ?? 0;
      const midi = d === 'n' ? lo + ((nextChord.bass - lo) % 12 + 12) % 12 : root + iv;
      const prev = this.last[name];
      const glideFrom = prev && prev.slide ? prev.midi : null;
      this.last[name] = { midi, slide: e.slide };
      return { k: 'note', part: name, midis: [midi], vel: e.accent ? 1 : 0.82, dur: dur * (e.slide ? 1.05 : part.gate ?? 0.9), glideFrom, accent: e.accent, dt };
    }
    if (part.type === 'chord') {
      if (chordStart || !this.voicing[name] || this.voicing[name].chord !== chord) {
        this.voicing[name] = { chord, notes: voice(chord, part.lo ?? 55, this.voicing[name]?.notes) };
      }
      return { k: 'note', part: name, midis: this.voicing[name].notes, vel: e.vel, dur: dur * (part.gate ?? 1), dt };
    }
    if (part.type === 'arp') {
      const v = voice(chord, part.lo ?? 60, null);
      const k = v.length, i = e.idx;
      const midi = v[((i % k) + k) % k] + 12 * Math.floor(i / k);
      return { k: 'note', part: name, midis: [midi], vel: e.accent ? 1 : 0.8, dur: dur * (part.gate ?? 0.8), dt };
    }
    const prev = this.last[name];
    const now = this._clock;
    const glideFrom = part.legato && prev && prev.end > now - 0.01 ? prev.midi : null;
    this.last[name] = { midi: e.midi, end: now + dur };
    return { k: 'note', part: name, midis: [e.midi], vel: e.accent ? 1 : 0.85, dur: dur * (part.gate ?? 0.95), glideFrom, dt };
  }
}
