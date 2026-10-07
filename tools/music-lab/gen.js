// Music Lab: genre grammars (docs/vision/sound.md 3.2, milestone S4).
//
// A seed and a genre make a whole track in the game's song format (with lab
// patches and the lab extensions in seq.js), so the same sequencer plays it
// and the song can later be written down as a tracks.js entry.
//
// Controlled randomness, after Mutable Instruments' Marbles: every pattern
// comes from the seeded generator, and `dejavu` (0..1) says how much of it
// repeats. A long section is played as 8-bar blocks; at each block each
// pattern either comes back as it was (probability dejavu) or mutates a
// few steps. Low dejavu wanders; high dejavu locks into a loop, which is
// what techno wants and house only partly.

import { rng } from './dsp.js';
import { BPATCH } from './instruments.js';

const NOTE = ['C', 'C#', 'D', 'Eb', 'E', 'F', 'F#', 'G', 'Ab', 'A', 'Bb', 'B'];

function R(seed) {
  const r = rng(seed);
  const api = {
    f: r,
    int: (a, b) => a + Math.floor(r() * (b - a + 1)),
    pick: (xs) => xs[Math.floor(r() * xs.length)],
    chance: (p) => r() < p,
    weighted: (pairs) => { let t = 0; for (const [, w] of pairs) t += w; let u = r() * t; for (const [v, w] of pairs) { if ((u -= w) < 0) return v; } return pairs[0][0]; },
  };
  return api;
}

// Chords as scale degrees of a minor key: [semitones above the tonic, quality].
const MINOR = { i: [0, 'm'], i7: [0, 'm7'], i9: [0, 'm9'], ii: [2, 'dim'], III: [3, ''], IIImaj7: [3, 'maj7'], iv: [5, 'm'], iv7: [5, 'm7'], iv9: [5, 'm9'], v: [7, 'm'], v7: [7, 'm7'], VI: [8, ''], VImaj7: [8, 'maj7'], VII: [10, ''], VII7: [10, '7'], VIIadd9: [10, 'add9'] };
const chordName = (tonic, deg) => { const [s, q] = MINOR[deg]; return NOTE[(tonic + s) % 12] + q; };

// House chord vamps (one chord per bar), classic deep-house moves.
const HOUSE_PROGS = [
  ['i9', 'i9', 'iv9', 'iv9'], ['i7', 'VImaj7', 'IIImaj7', 'VII'], ['i9', 'iv9', 'i9', 'v7'],
  ['i7', 'i7', 'VImaj7', 'VII'], ['iv9', 'v7', 'i9', 'i9'], ['i9', 'VIIadd9', 'VImaj7', 'v7'],
];
const TECHNO_PROGS = [['i', 'i', 'i', 'i'], ['i7', 'i7', 'i7', 'VI'], ['i', 'i', 'VII', 'i'], ['i7', 'i7', 'iv7', 'i7']];

// ── Pattern makers ───────────────────────────────────────────────
const lane = (n, f) => Array.from({ length: n }, (_, i) => f(i)).join('');

// Mutate a few positions of a lane string (only within `alphabet`).
function mutate(r, s, amount, alphabet) {
  const a = s.split('');
  const n = Math.max(1, Math.round(a.length * amount));
  for (let k = 0; k < n; k++) {
    const i = r.int(0, a.length - 1);
    a[i] = r.pick(alphabet);
  }
  return a.join('');
}

function houseHats(r) {
  return lane(16, (i) => (i % 4 === 2 ? '.' : i % 2 === 0 ? (r.chance(0.85) ? 'x' : 'o') : r.chance(0.55) ? 'o' : '.'));
}
function percLane(r, density) {
  return lane(16, (i) => (i % 4 === 0 ? '.' : r.chance(density * (i % 2 ? 1.1 : 0.6)) ? (r.chance(0.3) ? 'x' : 'o') : '.'));
}
// House bass: off-beat roots and octaves, sometimes a fifth or a pickup.
function houseBass(r) {
  return lane(16, (i) => {
    if (i % 4 === 2) return r.chance(0.92) ? r.weighted([['r', 5], ['o', 2], ['R', 1]]) : '.';
    if (i % 4 === 3) return r.chance(0.25) ? r.weighted([['o', 2], ['f', 1], ['r', 1]]) : '.';
    if (i % 4 === 0) return r.chance(0.08) ? 'l' : '.';
    return r.chance(0.15) ? r.weighted([['r', 2], ['f', 1], ['s', 1]]) : '.';
  });
}
// Syncopated chord stabs (the "x..x..x." family).
function houseStabs(r) {
  const base = r.pick(['x..x..x...x..x..', '..x...x...x...x.', 'x..x..x.x..x..x.', '.x..x..x.x..x...', 'x-.x-.x...x-.x..']);
  return r.chance(0.4) ? mutate(r, base, 0.08, ['x', '.', '.']) : base;
}
// The acid line: steps with notes, rests, accents and slides (bass lane
// syntax: degrees from the chord root, uppercase accents, ~ slide-holds).
function acidLine(r, density = 0.75) {
  const degs = [['r', 6], ['o', 4], ['f', 2], ['s', 2], ['t', 1], ['l', 1], ['u', 1]];
  let out = '';
  for (let i = 0; i < 16; i++) {
    if (out.length > i) continue;
    if (!r.chance(density)) { out += '.'; continue; }
    let d = r.weighted(degs);
    if (r.chance(0.28)) d = d.toUpperCase();
    out += d;
    if (i < 14 && r.chance(0.2)) out += '~';
  }
  return out.slice(0, 16);
}
function technoHats(r) { return lane(16, (i) => (i % 4 === 2 ? 'x' : r.chance(0.7) ? 'o' : '.')); }

// A long section as 8-bar blocks with déjà-vu mutation of the named lanes.
function evolve(r, dejavu, blocks, start, muts) {
  const out = [{ ...start }];
  for (let b = 1; b < blocks; b++) {
    const prev = out[b - 1], next = { ...prev };
    for (const [k, f] of Object.entries(muts)) if (!r.chance(dejavu)) next[k] = f(prev[k]);
    out.push(next);
  }
  return out;
}

// ── House ────────────────────────────────────────────────────────
export function house(seed, { dejavu = 0.6 } = {}) {
  const r = R(seed);
  const tonic = r.int(0, 11), bpm = r.int(120, 125);
  const progDegs = r.pick(HOUSE_PROGS);
  const prog = progDegs.map((d) => chordName(tonic, d)).join(' ');
  const keyName = NOTE[tonic] + ' minor';
  const bassLo = 33 + ((tonic - 9 + 12) % 12) % 12; // root between A1 and G#2
  const T = {
    id: `house-${seed}`, title: `House ${seed}`, style: `House · ${keyName}`, bpm, swing: r.chance(0.5) ? 0.06 : 0, gain: 1,
    delay: 0.75, delayFb: 0.32, pump: { depth: 0.45, release: 0.18 },
    kitName: 'tr909',
    kit: { kick: { s: 'kickHouse' }, clap: { rev: 0.3 }, ohat: { g: 0.9 } },
    prog: { a: prog },
    drums: {}, parts: {}, sections: [],
    lay: { shaker: 0.45, rim: 0.55, ride: 0.7, arp: 0.6, ohat: 0.25, hat: 0.15 },
  };
  // Instruments.
  T.parts.bass = { type: 'bass', lo: bassLo, ch: { pump: true, level: 1 }, pat: {}, lab: { ...BPATCH.pluckBass, cutoff: 260, res: 0.45, decay: 0.16, name: 'house bass' } };
  const stabPatch = r.chance(0.5) ? { ...BPATCH.organ, name: 'organ' } : { ...BPATCH.dubChord, cutoff: 1400, name: 'stab' };
  T.parts.stab = { type: 'chord', lo: 58, ch: { pump: true, rev: 0.3, dly: 0.25 }, pat: {}, lab: stabPatch };
  T.parts.pad = { type: 'chord', lo: 55, ch: { pump: true, rev: 0.5, hp: 180, level: 0.8 }, pat: { hold: 'x---------------' }, lab: { ...BPATCH.warmPad, name: 'pad' } };
  T.parts.arp = { type: 'arp', lo: 67, res: 1, ch: { rev: 0.3, dly: 0.45, pan: -0.25, pump: true, level: 0.7 }, pat: {}, lab: { ...BPATCH.ep, name: 'keys' } };

  const groove = { kick: 'x...x...x...x...', clap: '....x.......x...', ohat: '..x...x...x...x.', hat: houseHats(r), shaker: lane(16, (i) => (i % 2 ? 'x' : 'o')), rim: percLane(r, 0.3) };
  const blocks = evolve(r, dejavu, 6, { hat: groove.hat, rim: groove.rim, bass: houseBass(r), stab: houseStabs(r), arp: Array.from({ length: 8 }, () => r.pick(['0', '1', '2', '3', '.', '4'])).join(' ') }, {
    hat: (s) => mutate(r, s, 0.12, ['x', 'o', '.']),
    rim: (s) => mutate(r, s, 0.15, ['x', 'o', '.', '.']),
    bass: (s) => mutate(r, s, 0.1, ['r', 'o', '.', 'f']),
    stab: (s) => mutate(r, s, 0.08, ['x', '.']),
    arp: (s) => s.split(' ').map((t) => (r.chance(0.2) ? r.pick(['0', '1', '2', '3', '4', '.']) : t)).join(' '),
  });
  blocks.forEach((b, i) => {
    T.drums['g' + i] = { ...groove, hat: b.hat, rim: b.rim };
    T.parts.bass.pat['b' + i] = b.bass;
    T.parts.stab.pat['s' + i] = b.stab;
    T.parts.arp.pat['a' + i] = b.arp;
  });
  T.drums.intro = { kick: groove.kick, hat: blocks[0].hat, ohat: groove.ohat };
  T.drums.brk = { hat: '..x...x...x...x.', shaker: groove.shaker };
  T.drums.build = { kick: 'x...x...x...x...', clap: '....x.......x...', hat: 'x.x.x.x.x.x.x.x.' };
  const S = T.sections;
  S.push({ bars: 16, drums: 'intro', lp: [500, 16000], p: { bass: 'b0' } });
  S.push({ bars: 8, drums: 'g0', crash: true, p: { bass: 'b0', stab: 's0' } });
  S.push({ bars: 8, drums: 'g1', fill: 'clap', p: { bass: 'b1', stab: 's1', pad: 'hold' } });
  S.push({ bars: 8, drums: 'g2', crash: true, p: { bass: 'b2', stab: 's2', pad: 'hold', arp: 'a2' } });
  S.push({ bars: 16, drums: 'brk', down: 2, auto: { 'pad.cutoff': [700, 2600] }, p: { pad: 'hold', arp: 'a2' } });
  S.push({ bars: 8, drums: 'build', riser: 8, swell: true, fill: 'roll', gap: 2, p: { pad: 'hold', stab: 's2' } });
  S.push({ bars: 8, drums: 'g3', crash: true, drop: true, p: { bass: 'b3', stab: 's3', pad: 'hold', arp: 'a3' } });
  S.push({ bars: 8, drums: 'g4', fill: 'clap', p: { bass: 'b4', stab: 's4', pad: 'hold', arp: 'a4' } });
  S.push({ bars: 8, drums: 'g5', p: { bass: 'b5', stab: 's5', arp: 'a5' } });
  S.push({ bars: 16, drums: 'intro', lp: [16000, 400], p: { bass: 'b5' } });
  return T;
}

// ── Techno ───────────────────────────────────────────────────────
export function techno(seed, { dejavu = 0.8 } = {}) {
  const r = R(seed);
  const tonic = r.int(0, 11), bpm = r.int(128, 134);
  const prog = r.pick(TECHNO_PROGS).map((d) => chordName(tonic, d)).join(' ');
  const bassLo = 28 + ((tonic - 4 + 12) % 12);
  const T = {
    id: `techno-${seed}`, title: `Techno ${seed}`, style: `Techno · ${NOTE[tonic]} minor`, bpm, gain: 1.25,
    delay: r.pick([0.75, 0.5, 1.5]), delayFb: 0.45, pump: { depth: 0.3, release: 0.12 },
    kitName: r.chance(0.6) ? 'tr909' : 'tr808',
    kit: { kick: { s: 'kickTight', g: 1.05 }, clap: { rev: 0.45 } },
    prog: { a: prog },
    drums: {}, parts: {}, sections: [],
    lay: { ride: 0.55, rim: 0.4, clap: 0.3, ohat: 0.2, acid: 0.35, chord: 0.5 },
  };
  T.parts.rumble = { type: 'bass', lo: bassLo, ch: { pump: true, rev: 0.15 }, pat: { off: '..l...l...l...l.', roll: '.ll..ll..ll..ll.' }, lab: { ...BPATCH.rumble, name: 'rumble' } };
  T.parts.acid = { type: 'bass', lo: bassLo + 12, ch: { drive: 1.6, driveLp: 7000, dly: 0.2, level: 0.75 }, pat: {}, lab: { ...BPATCH.acid, cutoff: 220, res: 0.86, env: 0.5, decay: 0.3, name: 'acid' } };
  T.parts.chord = { type: 'chord', lo: 60, ch: { rev: 0.45, dly: 0.6, pan: 0.2, level: 0.8 }, pat: { dub: r.pick(['..x.............', '......x.........', '..x.......x.....', '...x......x.....']) }, lab: { ...BPATCH.dubChord, name: 'dub chord' } };

  const polyLen = r.pick([3, 5, 6, 7]);
  const poly = lane(polyLen, (i) => (i === 0 ? 'x' : '.'));
  const base = { kick: 'x...x...x...x...', hat: technoHats(r), ohat: '..x...x...x...x.', ride: '..x...x...x...x.', rim: poly, clap: r.chance(0.6) ? '....x.......x...' : '............x...' };
  const blocks = evolve(r, dejavu, 8, { acid: acidLine(r, 0.8), hat: base.hat }, {
    acid: (s) => mutate(r, s, 0.12, ['r', 'o', 'R', 'f', '.', 's']),
    hat: (s) => mutate(r, s, 0.1, ['x', 'o', '.']),
  });
  blocks.forEach((b, i) => { T.drums['d' + i] = { ...base, hat: b.hat }; T.parts.acid.pat['a' + i] = b.acid; });
  T.drums.intro = { kick: base.kick, hat: blocks[0].hat };
  T.drums.brk = { hat: blocks[0].hat, rim: poly, ride: base.ride };
  const S = T.sections;
  // Long sections whose filters move slowly: the evolution is the music.
  S.push({ bars: 16, drums: 'intro', lp: [400, 16000], p: { rumble: 'off' } });
  S.push({ bars: 8, drums: 'd0', crash: true, auto: { 'acid.cutoff': [150, 350] }, p: { rumble: 'off', acid: 'a0' } });
  S.push({ bars: 8, drums: 'd1', auto: { 'acid.cutoff': [350, 700], 'acid.res': [0.82, 0.9] }, p: { rumble: 'off', acid: 'a1' } });
  S.push({ bars: 8, drums: 'd2', fill: 'perc', auto: { 'acid.cutoff': [700, 1200] }, p: { rumble: 'off', acid: 'a2', chord: 'dub' } });
  S.push({ bars: 16, drums: 'brk', down: 2, auto: { 'acid.cutoff': [400, 2000], 'acid.decay': [0.25, 0.8] }, p: { acid: 'a3', chord: 'dub' } });
  S.push({ bars: 8, drums: 'd3', riser: 8, swell: true, gap: 1, auto: { 'acid.cutoff': [900, 1800] }, p: { rumble: 'roll', acid: 'a3' } });
  S.push({ bars: 8, drums: 'd4', crash: true, drop: true, auto: { 'acid.cutoff': [1600, 900] }, p: { rumble: 'off', acid: 'a4', chord: 'dub' } });
  S.push({ bars: 8, drums: 'd5', auto: { 'acid.cutoff': [900, 500], 'acid.env': [0.5, 0.75] }, p: { rumble: 'off', acid: 'a5', chord: 'dub' } });
  S.push({ bars: 8, drums: 'd6', fill: 'perc', auto: { 'acid.cutoff': [500, 300] }, p: { rumble: 'off', acid: 'a6' } });
  S.push({ bars: 8, drums: 'd7', auto: { 'acid.cutoff': [300, 180] }, p: { rumble: 'off', acid: 'a7' } });
  S.push({ bars: 16, drums: 'intro', lp: [16000, 350], p: { rumble: 'off' } });
  return T;
}

export const GENRES = { house, techno };
