// Music Lab: the composer (docs/vision/sound.md 3.4). What the genre
// grammars in gen.js share, and what makes a generated track hold together
// where a pile of random steps does not:
//
//   keys and chords   a scale, diatonic chords on its degrees with the
//                     extensions a genre wants, progressions as degree lists;
//   rhythm            Euclidean patterns and per-beat cells with metric
//                     accents, so a lane has a shape instead of a coin flip
//                     per step;
//   motifs            a melodic idea is a rhythm plus a contour (intervals in
//                     scale steps). Realising it over a chord snaps the strong
//                     notes to chord tones, so the same figure follows the
//                     harmony: that is what makes a hook recognisable when
//                     it comes back over a different chord. Phrases are
//                     question and answer (open ending, then closed on the
//                     root), and a sparse version of the hook keeps the
//                     shape with half the notes, for breakdowns;
//   bass              a lane syntax with 'n', a pickup into the next chord's
//                     root, so the bass leads the harmony instead of
//                     trailing it;
//   déjà vu           variations are made from a core once; each 8-bar
//                     block plays the core (probability dejavu) or one of
//                     its variations, so the loop returns to its identity
//                     instead of drifting (Marbles locks, it does not walk);
//   arrangement       long sections are split into 8-bar blocks that pick
//                     their variations, and the seams (crash, downlifter,
//                     riser, fill, gap) follow the tension curve.

import { rng } from './dsp.js';
import { parseChord } from './seq.js';

export const NOTE = ['C', 'C#', 'D', 'Eb', 'E', 'F', 'F#', 'G', 'Ab', 'A', 'Bb', 'B'];

// ── Seeded choices ───────────────────────────────────────────────
// `salt` keeps one seed from drawing the same key and tempo in every genre
// (each grammar's first draws are those).
export function R(seed, salt = 0) {
  const r = rng(salt ? (Math.imul(seed + salt * 104729, 2654435761) >>> 0) || 1 : seed);
  const api = {
    f: r,
    int: (a, b) => a + Math.floor(r() * (b - a + 1)),
    pick: (xs) => xs[Math.floor(r() * xs.length)],
    chance: (p) => r() < p,
    weighted: (pairs) => { let t = 0; for (const [, w] of pairs) t += w; let u = r() * t; for (const [v, w] of pairs) { if ((u -= w) < 0) return v; } return pairs[0][0]; },
    shuffle: (xs) => { const a = xs.slice(); for (let i = a.length - 1; i > 0; i--) { const j = Math.floor(r() * (i + 1)); [a[i], a[j]] = [a[j], a[i]]; } return a; },
    // A seeded child, so one choice does not shift every later draw.
    fork: (k) => R((seed * 7919 + k * 104729) >>> 0 || 1),
  };
  return api;
}

// ── Keys and chords ──────────────────────────────────────────────
export const SCALES = {
  minor: [0, 2, 3, 5, 7, 8, 10],
  dorian: [0, 2, 3, 5, 7, 9, 10],
  phrygian: [0, 1, 3, 5, 7, 8, 10],
  harmonicMinor: [0, 2, 3, 5, 7, 8, 11],
  major: [0, 2, 4, 5, 7, 9, 11],
  mixolydian: [0, 2, 4, 5, 7, 9, 10],
};
export const MINOR_PENTA = [0, 3, 5, 7, 10];

// Semitones above the tonic of scale degree k (any integer; 7 = the octave).
export const degSemi = (scale, k) => scale[((k % 7) + 7) % 7] + 12 * Math.floor(k / 7);

// The diatonic chord on degree d, named as seq.js parses it. `ext`: '' triad,
// '7', '9', 'add9', 'sus2', 'sus4', '6'. Non-diatonic colour is the caller's
// business (a genre may write chord names directly).
export function chordOn(scale, tonic, d, ext = '') {
  const root = (tonic + degSemi(scale, d)) % 12;
  // 'dom': the dominant seventh on the degree's root whatever the scale says
  // (the V7 of a minor key: cumbia, country, the classical cadence).
  if (ext === 'dom') return NOTE[root] + '7';
  const third = degSemi(scale, d + 2) - degSemi(scale, d), fifth = degSemi(scale, d + 4) - degSemi(scale, d);
  const seventh = degSemi(scale, d + 6) - degSemi(scale, d), sixth = degSemi(scale, d + 5) - degSemi(scale, d);
  let q;
  if (ext === 'sus2' || ext === 'sus4') q = ext;
  else if (fifth === 6) q = ext === '7' || ext === '9' ? 'm7b5' : 'dim';
  else if (third === 3) q = ext === '7' ? 'm7' : ext === '9' ? 'm9' : ext === 'add9' ? 'madd9' : ext === '6' && sixth === 9 ? 'm6' : ext === '6' ? 'm7' : 'm';
  else q = ext === '7' ? (seventh === 11 ? 'maj7' : '7') : ext === '9' ? (seventh === 11 ? 'maj9' : '9') : ext === 'add9' ? 'add9' : ext === '6' ? '6' : '';
  return NOTE[root] + q;
}

// A progression: one chord per bar from degree items (a degree, or [degree,
// ext]; an array of items in one bar joins them with commas, the format's
// two-chords-a-bar).
export function progression(scale, tonic, bars, ext = '') {
  const one = (it) => (Array.isArray(it) ? chordOn(scale, tonic, it[0], it[1] ?? ext) : chordOn(scale, tonic, it, ext));
  return bars.map((b) => (Array.isArray(b) && Array.isArray(b[0]) ? b.map(one).join(',') : one(b))).join(' ');
}

// Progression families, as degree lists (0 = i). Minor-key dance music lives
// on a few of these; the names are what producers call them.
export const PROGS = {
  // The "axis" loops that carry trance, eurobeat choruses, big-room house.
  anthem: [[0, 5, 2, 6], [0, 6, 5, 6], [0, 5, 6, 0], [5, 6, 0, 0], [0, 3, 5, 6], [0, 2, 6, 5], [0, 4, 5, 3], [5, 3, 0, 6]],
  // Verses that sit still and wait for the chorus.
  verse: [[0, 0, 5, 5], [0, 0, 6, 6], [0, 5, 0, 5], [0, 6, 0, 6], [0, 0, 3, 3], [0, 0, 0, 6]],
  // Deep house and liquid: two-chord vamps and 7th/9th colour.
  deep: [[0, 0, 3, 3], [0, 3, 0, 4], [0, 5, 2, 6], [0, 0, 5, 6], [3, 4, 0, 0], [0, 6, 5, 4]],
  // Techno and psy: one chord, or a step away and back.
  drone: [[0, 0, 0, 0], [0, 0, 0, 6], [0, 0, 1, 0], [0, 0, 0, 5], [0, 1, 0, 1]],
  // Major keys for the brightest choruses.
  bright: [[0, 4, 5, 3], [5, 3, 0, 4], [0, 5, 3, 4], [3, 4, 5, 5], [0, 3, 5, 4]],
  // Cumbia and chicha, in minor: i, iv and the dominant V7 ([4, 'dom']),
  // the VII and VI of the Andean side; two-chord vamps mostly.
  cumbia: [[0, 0, 3, 3], [0, 3, [4, 'dom'], 0], [0, 6, 0, 6], [0, 0, [4, 'dom'], [4, 'dom']], [0, 3, 0, [4, 'dom']], [3, [4, 'dom'], 0, 0], [0, [4, 'dom'], 0, [4, 'dom']], [5, 6, 0, 0]],
};

// Pitch class set of a chord name, and the chord's root pitch class.
export function chordPcs(name) {
  const c = parseChord(name);
  return { root: c.root, pcs: c.iv.map((i) => (c.root + i) % 12), bass: c.bass };
}

// ── Rhythm ───────────────────────────────────────────────────────
export const lane = (n, f) => Array.from({ length: n }, (_, i) => f(i)).join('');
// n bars of 16 steps from a function of the bar index.
export const bars = (n, f) => Array.from({ length: n }, (_, b) => f(b)).join('');
// Repeats a one-bar lane over n bars, with a different last bar if given.
export const loop = (n, one, last = one) => bars(n, (b) => (b === n - 1 ? last : one));

// Bjorklund's algorithm: k hits as evenly spread over n steps as possible
// (E(3,8) is the tresillo, E(5,16) the "x..x..x." family), rotated.
export function euclid(k, n, rot = 0, hit = 'x') {
  if (k <= 0) return '.'.repeat(n);
  if (k >= n) return hit.repeat(n);
  let a = Array.from({ length: k }, () => [1]), b = Array.from({ length: n - k }, () => [0]);
  while (b.length > 1) {
    const m = Math.min(a.length, b.length);
    const next = [];
    for (let i = 0; i < m; i++) next.push(a[i].concat(b[i]));
    const rest = a.length > m ? a.slice(m) : b.slice(m);
    a = next; b = rest;
  }
  const seq = a.concat(b).flat();
  const s = seq.map((v) => (v ? hit : '.')).join('');
  const rr = ((rot % n) + n) % n;
  return s.slice(n - rr) + s.slice(0, n - rr);
}

// Metric weight of a 16th: downbeat, beats, off-beat 8ths, then the rest.
export const weight = (i) => (i % 16 === 0 ? 1 : i % 4 === 0 ? 0.8 : i % 2 === 0 ? 0.6 : 0.4);
// A hit character by metric weight: accents on the beats.
export const accent = (i, strong = 0.8) => (weight(i) >= strong ? 'x' : 'o');

// A bar from per-beat cells: `table` is [[cell, weight]...] of 4-char cells.
// The same cell set every beat keeps the pattern one idea; `last` is the
// cell set for beat 4 (the turnaround), if different.
export function cells(r, table, last = table) {
  let s = '';
  for (let b = 0; b < 4; b++) s += r.weighted(b === 3 ? last : table);
  return s;
}

// Rhythm templates: whole-bar figures dance music is built on.
export const FIGURES = {
  tresillo: 'x..x..x.x..x..x.',
  tresilloB: 'x..x..x...x...x.',
  clave: 'x..x..x...x.x...',
  dotted: 'x..x..x..x..x..x',
  offbeat: '..x...x...x...x.',
  offbeat16: '.x.x.x.x.x.x.x.x',
  eighths: 'x.x.x.x.x.x.x.x.',
  gallop: 'x..xx..xx..xx..x',
  push: 'x...x...x..x..x.',
  broken: 'x.x..x..x.x..x..',
  stabs: 'x..x..x.........',
  drop1: 'x...............',
};

// Mutates a few positions of a lane within an alphabet (a variation of a
// core pattern, never a walk away from it: callers mutate the core).
export function mutate(r, s, amount, alphabet) {
  const a = s.split('');
  const n = Math.max(1, Math.round(a.length * amount));
  for (let k = 0; k < n; k++) {
    const i = r.int(0, a.length - 1);
    a[i] = r.pick(alphabet);
  }
  return a.join('');
}

// ── Motifs ───────────────────────────────────────────────────────
// Onset cells by density, for the motif's rhythm.
const MEL_CELLS = {
  sparse: [['x...', 5], ['....', 2], ['x.x.', 1], ['..x.', 1], ['x..x', 1]],
  medium: [['x...', 3], ['x.x.', 3], ['x..x', 2], ['..x.', 1], ['.x.x', 1], ['x.xx', 1], ['xx..', 1], ['....', 1]],
  dense: [['x.x.', 3], ['xx.x', 2], ['x.xx', 2], ['xxx.', 1], ['xxxx', 1], ['x..x', 1]],
};
const MEL_FIGURES = {
  sparse: ['x.......x.......', 'x...........x...', 'x.....x.........', 'x.......x...x...'],
  medium: [FIGURES.tresillo, FIGURES.tresilloB, FIGURES.push, FIGURES.broken, 'x.x...x.x.x...x.', 'x...x.x.x...x...'],
  dense: ['x.x.x.x.x..x..x.', 'x.xx.x.xx.x.x.x.', 'xx.x.xx.x.x.x...', 'x.x.x.x.x.x.x..x', FIGURES.gallop],
};

// An interval step for the contour: mostly steps, some thirds, few leaps;
// after a leap the line turns back by step. `bias` leans the direction.
function interval(r, prev, bias) {
  if (Math.abs(prev) >= 3) return -Math.sign(prev) * r.weighted([[1, 3], [2, 1]]);
  const size = r.weighted([[0, 1.2], [1, 4], [2, 2], [3, 0.8], [4, 0.5], [7, 0.15]]);
  if (size === 0) return 0;
  const up = r.chance(0.5 + 0.35 * bias);
  return up ? size : -size;
}

// A motif: a rhythm over `bars` bars (onsets as 16th indexes) and a contour
// (intervals in scale steps between consecutive onsets). The shape is a
// rise, fall, arch or wave over the motif, which is what the ear follows.
export function motif(r, { bars: nb = 1, density = 'medium', figure = 0.5, shape = null } = {}) {
  let rhythm = '';
  for (let b = 0; b < nb; b++) rhythm += r.chance(figure) ? r.pick(MEL_FIGURES[density]) : cells(r, MEL_CELLS[density]);
  if (rhythm[0] !== 'x') rhythm = 'x' + rhythm.slice(1); // a motif states itself on the downbeat
  const onsets = [];
  for (let i = 0; i < rhythm.length; i++) if (rhythm[i] === 'x') onsets.push(i);
  shape ||= r.pick(['arch', 'rise', 'fall', 'wave', 'valley']);
  const ivs = [];
  let prev = 0;
  for (let k = 1; k < onsets.length; k++) {
    const u = k / Math.max(1, onsets.length - 1);
    const bias = shape === 'rise' ? 0.6 : shape === 'fall' ? -0.6 : shape === 'arch' ? (u < 0.5 ? 0.8 : -0.8) : shape === 'valley' ? (u < 0.5 ? -0.8 : 0.8) : Math.sin(u * Math.PI * 2) * 0.8;
    const iv = interval(r, prev, bias);
    ivs.push(iv);
    prev = iv || prev;
  }
  return { bars: nb, onsets, ivs, shape, len: rhythm.length };
}

// Keeps the strong onsets of a motif (beats, and the last onset of each
// bar), for breakdowns: the hook at half the notes, same shape.
export function thin(m) {
  const keep = new Set();
  for (let b = 0; b < m.bars; b++) {
    const inBar = m.onsets.filter((o) => Math.floor(o / 16) === b);
    for (const o of inBar) if (o % 4 === 0) keep.add(o);
    if (inBar.length) keep.add(inBar[inBar.length - 1]);
  }
  const onsets = m.onsets.filter((o) => keep.has(o));
  // The contour between kept onsets is the sum of the skipped intervals.
  const ivs = [];
  let acc = 0;
  for (let k = 1; k < m.onsets.length; k++) {
    acc += m.ivs[k - 1];
    if (keep.has(m.onsets[k])) { ivs.push(acc); acc = 0; }
  }
  return { ...m, onsets, ivs };
}

// The nearest scale degree to k whose pitch class is in `pcs` (ties go the
// way the line was moving).
function snap(scale, tonicPc, k, pcs, dir = 1) {
  for (let d = 0; d < 7; d++) {
    for (const s of d === 0 ? [0] : [dir * d, -dir * d]) {
      const kk = k + s;
      if (pcs.includes(((tonicPc + degSemi(scale, kk)) % 12 + 12) % 12)) return kk;
    }
  }
  return k;
}

// Realises a motif over chords as a `mel` lane (res 1: one token per 16th).
//   chords   one chord name per bar (the motif's bars cycle over them);
//   tonic    the key's tonic as a midi note near the wanted register;
//   lo, hi   the register, in scale degrees from the tonic;
//   phrase   bars per phrase: the last onset of a phrase ends it, 'open'
//            (3rd or 5th) on odd phrases and 'closed' (root) on even ones;
//   gate     longest note in 16ths (null: until the next onset);
//   home     the degree the motif starts on (null: the nearest chord tone to
//            the middle of the register);
//   avoid    scale degrees (0..6) the line steps over when it is not on a
//            chord tone: [1, 5] in minor leaves the minor pentatonic, the
//            Andean side of chicha; [3, 6] in major the major pentatonic.
export function realise(m, { scale, tonic, chords, lo = -3, hi = 9, phrase = 4, gate = null, home = null, start = 0, accents = true, avoid = [] }) {
  const nb = chords.length;
  const tonicPc = ((tonic % 12) + 12) % 12;
  const toks = new Array(nb * 16).fill('.');
  const midiOf = (k) => tonic + degSemi(scale, k);
  const deg = (kk) => ((kk % 7) + 7) % 7;
  // Steps past the avoided degrees the way the line is moving, turning back
  // at the register's edge.
  const skip = (kk, d) => {
    for (let n = 0; n < 7 && avoid.includes(deg(kk)); n++) {
      kk += d;
      if (kk > hi || kk < lo) { d = -d; kk += 2 * d; }
    }
    return kk;
  };
  let k = home ?? Math.round((lo + hi) / 2);
  for (let b0 = 0; b0 < nb; b0 += m.bars) {
    const onsets = m.onsets.map((o) => o + b0 * 16).filter((o) => o < nb * 16);
    let dir = 1;
    for (let i = 0; i < onsets.length; i++) {
      const o = onsets[i], bar = Math.floor(o / 16);
      const ch = chordPcs(chords[bar % nb]);
      if (i === 0) {
        // Each statement starts where the hook lives, on a chord tone.
        k = snap(scale, tonicPc, home ?? Math.round((lo + hi) / 2) + start, ch.pcs, 1);
      } else {
        let iv = m.ivs[i - 1];
        if (k + iv > hi || k + iv < lo) iv = -iv;
        k += iv;
        if (iv) dir = Math.sign(iv);
        k = Math.max(lo, Math.min(hi, k));
      }
      const last = i === onsets.length - 1 || Math.floor(onsets[i + 1] / 16) !== bar;
      const phraseEnd = last && (bar % phrase === phrase - 1 || bar === nb - 1);
      if (phraseEnd) {
        const closed = Math.floor(bar / phrase) % 2 === 1 || bar === nb - 1;
        const want = closed ? [ch.root] : ch.pcs.filter((p) => p !== ch.root);
        k = snap(scale, tonicPc, k, want.length ? want : ch.pcs, dir);
      } else if (o % 4 === 0 || last) {
        k = snap(scale, tonicPc, k, ch.pcs, dir);
      } else if (avoid.length) {
        k = skip(k, dir);
      }
      const next = i + 1 < onsets.length ? onsets[i + 1] : (b0 + m.bars) * 16;
      let len = Math.max(1, next - o);
      if (gate) len = Math.min(len, gate);
      if (phraseEnd) len = Math.min(len, 8);
      const midi = midiOf(k);
      toks[o] = NOTE[((midi % 12) + 12) % 12] + (Math.floor(midi / 12) - 1) + (accents && o % 16 === 0 ? '!' : '');
      for (let j = 1; j < len && o + j < toks.length; j++) toks[o + j] = '_';
    }
  }
  return toks.join(' ');
}

// Transposes the motif's statement a third up (a sequence) for an answer.
export const answer = (m) => ({ ...m, start: 2 });

// ── Bass ─────────────────────────────────────────────────────────
// Bass lanes use seq.js's degree syntax (r o f l u t s) plus the lab's 'n':
// the next chord's root, a pickup. A one-bar maker from per-beat cells with
// a pickup cell on beat 4 of the bars before a chord change.
export function bassBar(r, table, pickup, changes) {
  let s = '';
  for (let b = 0; b < 4; b++) s += b === 3 && changes && r.chance(pickup) ? r.pick(['..rn', '..n.', 'r.nn', 'r..n', '.r.n']) : r.weighted(table);
  return s;
}
// Whether the chord changes after bar b of a progression.
export function changesAfter(prog, b) {
  const bars_ = prog.trim().split(/\s+/);
  return bars_[b % bars_.length] !== bars_[(b + 1) % bars_.length];
}

// ── Déjà vu and arrangement ──────────────────────────────────────
// Variations of a core lane: each is the core mutated once (not a walk).
export function variants(r, core, n, amount, alphabet, f = null) {
  const out = [core];
  for (let i = 0; i < n; i++) out.push(f ? f(core, i) : mutate(r, core, amount, alphabet));
  return out;
}

// Picks which variant an 8-bar block plays: the core with probability
// dejavu, else one of the others. Block 0 of a section is always the core.
export function pickVariant(r, dejavu, n, block) {
  if (block === 0 || n <= 1 || r.chance(dejavu)) return 0;
  return r.int(1, n - 1);
}

// Expands sections into 8-bar blocks. A section may carry `v`: { part:
// nVariants } and `vd`: nDrumVariants; the keys are `<base><index>` (the
// pattern names a genre wrote). Events stay where they belong: crash, drop,
// down and lp on the first block; riser, swell, fill and gap on the last;
// `auto` ramps are split across the blocks.
export function expand(sections, r, dejavu) {
  const out = [];
  for (const s of sections) {
    const n = s.bars > 8 && s.bars % 8 === 0 ? s.bars / 8 : 1;
    for (let b = 0; b < n; b++) {
      const first = b === 0, last = b === n - 1;
      const o = { bars: s.bars / n, drums: s.drums, p: { ...(s.p || {}) } };
      if (s.prog) o.prog = s.prog;
      if (s.vd && s.drums) o.drums = s.drums + pickVariant(r, dejavu, s.vd, b);
      for (const [part, nv] of Object.entries(s.v || {})) if (o.p[part]) o.p[part] = o.p[part] + pickVariant(r, dejavu, nv, b);
      if (first) for (const k of ['crash', 'drop', 'down']) if (s[k]) o[k] = s[k];
      if (last) for (const k of ['riser', 'swell', 'fill', 'gap']) if (s[k]) o[k] = s[k];
      if (s.lp) o.lp = n === 1 ? s.lp : [s.lp[0] * Math.pow(s.lp[1] / s.lp[0], b / n), s.lp[0] * Math.pow(s.lp[1] / s.lp[0], (b + 1) / n)];
      if (s.auto) {
        o.auto = {};
        for (const [k, [a, z]] of Object.entries(s.auto)) o.auto[k] = [a + (z - a) * (b / n), a + (z - a) * ((b + 1) / n)];
      }
      out.push(o);
    }
  }
  return out;
}

// The total bars and a one-line description for the page.
export const totalBars = (S) => S.reduce((a, s) => a + s.bars, 0);
