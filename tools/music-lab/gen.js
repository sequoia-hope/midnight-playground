// Music Lab: genre grammars (docs/vision/sound.md 3.2 and 3.4, milestone S4).
//
// A seed and a genre make a whole track in the game's song format (with lab
// patches and the lab extensions in seq.js), so the same sequencer plays it
// and the song can later be written down as a tracks.js entry. The musical
// machinery (keys, rhythm, motifs, pickups, déjà vu, arrangement) is
// compose.js; each grammar here is the genre's own vocabulary: tempo, kit,
// progressions, the lanes it is built from, its instruments and its form.
//
// What every grammar does, because it is what makes a track grab:
//   - one hook: a motif made once, stated sparse in the breakdown and in
//     full at the drop, answered in the drop's second half;
//   - patterns with a shape: cells and Euclidean figures, 4-bar phrases with
//     a turnaround, 8-bar blocks with the kick out before the crash;
//   - bass that leads into the next chord ('n' pickups);
//   - a tension curve: intro, groove, break, build, drop, and the seams
//     (crash, downlifter, riser, roll, gap) placed by the form;
//   - déjà vu: every long section plays its core and returns to it.
//
// Genres: house (deep, anthem), techno (acid, melodic), trance, eurobeat
// (the touge: Initial D's mountain roads), psytrance, drum & bass (liquid,
// roller) and UK garage.

import { R, NOTE, SCALES, PROGS, noDim, progression, lane, bars, loop, euclid, cells, FIGURES, mutate, motif, thin, realise, bassBar, changesAfter, variants, expand } from './compose.js';
import { BPATCH } from './instruments.js';

// A lab patch by name, with overrides.
const P = (name, over = {}) => ({ ...BPATCH[name], ...over, name: over.name ?? name });
const scaleName = (s) => Object.keys(SCALES).find((k) => SCALES[k] === s);
// A bass register: the key's root between A1 and G#2.
const bassLo = (tonic) => 33 + ((tonic - 9 + 12) % 12);

// Holds each hit over the rests after it, up to max steps ('x--.').
function hold(s, max) {
  const a = s.split('');
  for (let i = 0; i < a.length; i++) {
    if (a[i] !== 'x') continue;
    for (let j = 1; j <= max && i + j < a.length && a[i + j] === '.'; j++) a[i + j] = '-';
  }
  return a.join('');
}
// Chord names per bar of a progression (the first of a two-chord bar).
const barChords = (prog) => prog.trim().split(/\s+/).map((b) => b.split(',')[0]);

// The hook: one motif realised three ways over eight bars of a progression
// (two statements of a four-bar loop): full, thinned, and answered (the
// same figure a third higher). `prog2` realises the full hook in another
// key (eurobeat's final chorus).
function hooks(r, m, { scale, tonic, prog, lo, hi, gate, phrase = 4, avoid = [] }) {
  const c = barChords(prog);
  const chords = c.length >= 8 ? c.slice(0, 8) : [...c, ...c].slice(0, 8);
  const over = (mm, opts = {}) => realise(mm, { scale, tonic, chords, lo, hi, gate, phrase, avoid, ...opts });
  return { hook: over(m), sparse: over(thin(m), { gate: null }), answer: over(m, { start: 2 }) };
}

// The acid line: steps with notes, rests, accents and slides (bass lane
// syntax: degrees from the chord root, uppercase accents, ~ slide-holds).
function acidLine(r, density = 0.75, degs = [['r', 6], ['o', 4], ['f', 2], ['s', 2], ['t', 1], ['l', 1], ['u', 1]]) {
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

// Sets pattern variants on a part (`b0`, `b1`, ...) and returns the count.
function setPats(part, key, list) { list.forEach((s, i) => { part.pat[key + i] = s; }); return list.length; }
function setDrums(T, key, list) { list.forEach((d, i) => { T.drums[key + i] = d; }); return list.length; }

// The track skeleton.
function skeleton(id, fields) {
  return { id, gain: 1, delay: 0.75, delayFb: 0.32, pump: { depth: 0.45, release: 0.18 }, drums: {}, parts: {}, sections: [], lay: {}, ...fields };
}

// ── House ────────────────────────────────────────────────────────
// Deep: dorian, 9th chords, an organ or EP riff, no fuss. Anthem: minor,
// the axis progression, piano stabs, a choir and a hook over the drop.
export function house(seed, { dejavu = 0.7 } = {}) {
  const r = R(seed, 1);
  const sub = r.fork(1).chance(0.5) ? 'deep' : 'anthem';
  const tonic = r.int(0, 11), bpm = r.int(122, 126);
  const scale = sub === 'deep' ? SCALES.dorian : SCALES.minor;
  const ext = sub === 'deep' ? '9' : r.pick(['', '7']);
  const progA = progression(scale, tonic, r.pick(sub === 'deep' ? noDim(scale, PROGS.deep) : PROGS.anthem), ext);
  const progB = progression(scale, tonic, r.pick(noDim(scale, PROGS.verse)), ext);
  const T = skeleton(`house-${seed}`, {
    title: `House ${seed}`, style: `${sub === 'deep' ? 'Deep house' : 'Piano house'} · ${NOTE[tonic]} ${scaleName(scale)} · ${progA}`,
    bpm, gain: 0.9, swing: r.pick([0, 0.05, 0.08]), kitName: r.chance(0.7) ? 'tr909' : 'tr808',
    kit: { kick: { s: 'kickHouse' }, clap: { rev: 0.3 }, ohat: { g: 0.9 } },
    prog: { a: progA, b: progB },
    lay: { shaker: 0.45, rim: 0.55, ride: 0.7, ohat: 0.25, hat: 0.15, lead: 0.3, choir: 0.5, stab: 0.2 },
  });

  // Drums: an 8-bar groove with the turnaround and the kick out in bar 8.
  const hatCell = r.pick(['x...', 'xo..', 'xo.o', 'x..o', 'xx.o', 'xo.x']); // step 2 is the open hat's
  const hat = lane(4, () => hatCell).slice(0, 12) + r.pick([hatCell, 'xo.o', 'x..o']);
  const kick8 = loop(8, 'x...x...x...x...', r.pick(['x...x...x.......', 'x...x...x...x..x', 'x...x...x...x...']));
  const clap8 = loop(8, '....x.......x...', r.pick(['....x.......x.xx', '....x.......x...', '....x.......x..o']));
  const rim = euclid(r.pick([3, 5]), 16, r.int(0, 3), 'o').replace(/o/g, () => (r.chance(0.3) ? 'x' : 'o'));
  const groove = { kick: kick8, clap: clap8, ohat: '..x...x...x...x.', hat, shaker: 'xoxoxoxoxoxoxoxo', rim };
  const g1 = { ...groove, hat: mutate(r, hat, 0.12, ['x', 'o', '.']), rim: euclid(r.pick([3, 5, 7]), 16, r.int(0, 5), 'o') };
  const g2 = { ...groove, rim: mutate(r, rim, 0.15, ['x', 'o', '.', '.']), ride: '..x...x...x...x.' };
  const nd = setDrums(T, 'g', [groove, g1, g2]);
  T.drums.intro = { kick: 'x...x...x...x...', hat };
  T.drums.intro2 = { kick: 'x...x...x...x...', hat, ohat: groove.ohat, shaker: groove.shaker };
  T.drums.brk = { hat: '..x...x...x...x.', shaker: groove.shaker };
  T.drums.brk2 = { shaker: groove.shaker, rim, ohat: '..............x.' };
  T.drums.build = { kick: 'x...x...x...x...', clap: '....x.......x...', hat: 'x.x.x.x.x.x.x.x.' };

  // Bass: off-beat roots and octaves, a pickup before each change.
  const table = sub === 'deep' ? [['..r.', 6], ['..o.', 2], ['..rr', 1], ['..ro', 1], ['....', 0.5]] : [['..r.', 5], ['r.r.', 2], ['..o.', 2], ['..rr', 1], ['r..r', 1]];
  const bassCore = bars(4, (b) => bassBar(r, table, 0.75, changesAfter(progA, b)));
  T.parts.bass = { type: 'bass', lo: bassLo(tonic), ch: { pump: true, level: 1 }, pat: {}, lab: P('pluckBass', { cutoff: 260, res: 0.45, decay: 0.16, name: 'house bass' }) };
  const nb = setPats(T.parts.bass, 'b', variants(r, bassCore, 2, 0.1, ['r', 'o', '.', 'f']));

  // Stabs: a Euclidean figure, held longer on the deep side.
  const k = r.pick([3, 5, 6]), rot = r.int(0, 2);
  const stabBar = sub === 'deep' ? hold(r.pick([euclid(k, 16, rot), FIGURES.tresilloB, FIGURES.stabs]), 5) : hold(euclid(k, 16, rot), 2);
  const stabCore = loop(4, stabBar, hold(euclid(k, 16, rot + 1), sub === 'deep' ? 5 : 2));
  T.parts.stab = { type: 'chord', lo: sub === 'deep' ? 55 : 58, ch: { pump: true, rev: 0.3, dly: 0.25, level: 0.45 }, pat: {}, lab: sub === 'deep' ? (r.chance(0.5) ? P('organ') : P('ep')) : (r.chance(0.6) ? P('piano') : P('organ')) };
  const ns = setPats(T.parts.stab, 's', variants(r, stabCore, 2, 0.08, ['x', '.']));

  // The pad and the choir let go of a chord before the next one is well in
  // (five-note chords a bar apart would otherwise pile up for a second).
  T.parts.pad = { type: 'chord', lo: 55, ch: { pump: true, rev: 0.5, hp: 180, level: 0.8 }, pat: { hold: 'x---------------' }, lab: P('warmPad', { r: 0.6 }) };
  // The choir: 'ah' over the anthem, a quieter 'oo' under the deep one.
  T.parts.choir = { type: 'chord', lo: 60, ch: { pump: true, rev: 0.55, hp: 200, level: sub === 'deep' ? 0.35 : 0.5 }, pat: { hold: 'x---------------' }, lab: P('choir', { vowel: sub === 'deep' ? 'u' : r.pick(['a', 'o']), r: 0.5 }) };

  // The hook: a riff on keys (deep: low and sparse) or a melody over the
  // drop; its answer is realised over the verse progression it plays on.
  const m = motif(r, { bars: r.chance(0.5) ? 1 : 2, density: sub === 'deep' ? 'sparse' : 'medium', figure: 0.5 });
  const leadPatch = sub === 'deep' ? P('ep') : r.pick([P('ep'), P('glass'), P('piano'), P('bell')]);
  const H = hooks(r, m, { scale, tonic: (sub === 'deep' ? 48 : 60) + tonic, prog: progA, lo: -2, hi: 9, gate: sub === 'deep' ? 6 : 4 });
  const HB = hooks(r, m, { scale, tonic: (sub === 'deep' ? 48 : 60) + tonic, prog: progB, lo: -2, hi: 9, gate: sub === 'deep' ? 6 : 4 });
  T.parts.lead = { type: 'mel', res: 1, ch: { rev: 0.35, dly: 0.3, pan: 0.1, pump: true, hp: 250, level: 0.9 }, pat: { ...H, answerB: HB.answer }, lab: leadPatch };

  const S = T.sections;
  S.push({ bars: 8, drums: 'intro', lp: [600, 16000], p: {} });
  S.push({ bars: 8, drums: 'intro2', p: { bass: 'b0' } });
  S.push({ bars: 16, drums: 'g', vd: nd, crash: true, fill: 'clap', p: { bass: 'b', stab: 's', pad: 'hold' }, v: { bass: nb, stab: ns } });
  S.push({ bars: 8, drums: 'brk', down: 2, auto: { 'pad.cutoff': [600, 2400] }, p: { pad: 'hold', lead: 'sparse' } });
  S.push({ bars: 8, drums: 'build', riser: 8, swell: true, fill: 'roll', gap: 2, p: { pad: 'hold', stab: 's0', bass: 'b0' } });
  S.push({ bars: 16, drums: 'g', vd: nd, crash: true, drop: true, fill: 'clap', p: { bass: 'b', stab: 's', pad: 'hold', lead: 'hook', choir: 'hold' }, v: { bass: nb, stab: ns } });
  S.push({ bars: 16, drums: 'g', vd: nd, prog: 'b', fill: 'clap', p: { bass: 'b', stab: 's', lead: 'answerB', choir: 'hold' }, v: { bass: nb, stab: ns } });
  S.push({ bars: 16, drums: 'brk2', down: 2, auto: { 'pad.cutoff': [500, 2800] }, p: { pad: 'hold', lead: 'hook', choir: 'hold' } });
  S.push({ bars: 8, drums: 'build', riser: 8, swell: true, fill: 'roll', gap: 2, p: { pad: 'hold', stab: 's0', choir: 'hold' } });
  S.push({ bars: 16, drums: 'g', vd: nd, crash: true, drop: true, fill: 'clap', p: { bass: 'b', stab: 's', pad: 'hold', lead: 'hook', choir: 'hold' }, v: { bass: nb, stab: ns } });
  S.push({ bars: 16, drums: 'intro2', lp: [16000, 500], p: { bass: 'b0', stab: 's0' } });
  T.sections = expand(S, r, dejavu);
  return T;
}

// ── Techno ───────────────────────────────────────────────────────
// Acid: phrygian drone, a 303 motif that the filter carries for ten minutes
// of intent, dub chords, odd-length percussion that phases against the bar.
// Melodic: slower, a 16th arpeggio on a pluck over chords that change every
// two bars, a wide pad, a long lead note now and then.
export function techno(seed, { dejavu = 0.8 } = {}) {
  const r = R(seed, 2);
  const sub = r.fork(1).chance(0.5) ? 'acid' : 'melodic';
  const tonic = r.int(0, 11);
  const bpm = sub === 'acid' ? r.int(132, 138) : r.int(124, 128);
  const scale = sub === 'acid' ? (r.chance(0.6) ? SCALES.phrygian : SCALES.minor) : SCALES.minor;
  const degs = sub === 'acid' ? r.pick(PROGS.drone) : r.pick(PROGS.anthem).flatMap((d) => [d, d]);
  const prog = progression(scale, tonic, degs, sub === 'acid' ? r.pick(['', '7']) : r.pick(['', 'add9']));
  const T = skeleton(`techno-${seed}`, {
    title: `Techno ${seed}`, style: `${sub === 'acid' ? 'Acid techno' : 'Melodic techno'} · ${NOTE[tonic]} ${scaleName(scale)} · ${prog}`,
    bpm, gain: 1.3, delay: r.pick([0.75, 0.5, 1.5]), delayFb: 0.45, pump: { depth: 0.3, release: 0.12 },
    kitName: r.chance(0.6) ? 'tr909' : 'tr808',
    kit: { kick: { s: 'kickTight', g: 1.05 }, clap: { rev: 0.45 } },
    prog: { a: prog },
    lay: { ride: 0.55, rim: 0.4, clap: 0.3, ohat: 0.2, tomL: 0.5, snap: 0.6, acid: 0.3, arp: 0.3, chord: 0.5, blip: 0.6, lead: 0.4 },
  });

  // Drums: the kick out on the last beats of bar 8; hats with ghosts; a
  // Euclidean rim and a low tom; a 5- or 7-step snap that phases.
  const hat = cells(r, [['..x.', 5], ['.ox.', 2], ['..xo', 2], ['o.x.', 1]]);
  const kick8 = loop(8, 'x...x...x...x...', r.pick(['x...x...x.......', 'x...x...x...x...', 'x...x...........']));
  const rim = euclid(r.pick([5, 7]), 16, r.int(0, 4), 'o').replace(/o/g, () => (r.chance(0.25) ? 'x' : 'o'));
  const tom = euclid(3, 16, r.int(0, 15), 'o');
  const snapLen = r.pick([5, 7, 12]);
  const snap = lane(snapLen, (i) => (i === 0 ? 'x' : i === snapLen - 2 && r.chance(0.5) ? 'o' : '.'));
  const clap = r.chance(0.6) ? '....x.......x...' : '............x...';
  const base = { kick: kick8, hat, ohat: '..x...x...x...x.', clap, rim };
  const d0 = { ...base, tomL: tom };
  const d1 = { ...base, hat: mutate(r, hat, 0.1, ['x', 'o', '.']), ride: '..x...x...x...x.', snap };
  const d2 = { ...base, rim: euclid(r.pick([5, 7, 9]), 16, r.int(0, 6), 'o'), tomL: tom, ride: '..x...x...x...x.' };
  const nd = setDrums(T, 'd', [d0, d1, d2]);
  T.drums.intro = { kick: 'x...x...x...x...', hat };
  T.drums.intro2 = { kick: 'x...x...x...x...', hat, clap, ohat: base.ohat };
  T.drums.brk = { hat, ride: '..x...x...x...x.', rim };
  T.drums.build = { kick: 'x...x...x...x...', hat: 'x.x.x.x.x.x.x.x.', clap: '....x.......x...', rim };

  const lo = 28 + ((tonic - 4 + 12) % 12);
  T.parts.rumble = { type: 'bass', lo, ch: { pump: true, rev: 0.15 }, pat: { off: '..l...l...l...l.', roll: '.ll..ll..ll..ll.' }, lab: P('rumble') };
  T.parts.chord = { type: 'chord', lo: 60, ch: { rev: 0.45, dly: 0.6, pan: 0.2, level: 1.6 }, pat: { dub: r.pick(['..x.............', '......x.........', '..x.......x.....', '...x......x.....']) }, lab: P('dubChord') };
  T.parts.pad = { type: 'chord', lo: 52, ch: { rev: 0.55, hp: 140, level: 1.5, pump: true }, pat: { hold: 'x---------------' }, lab: sub === 'acid' ? P('darkPad') : P('superPadWide') };
  const S = T.sections;
  if (sub === 'acid') {
    // The acid motif: one bar that stays; the variations toggle accents
    // and slides and move two steps.
    const core = acidLine(r, 0.8);
    T.parts.acid = { type: 'bass', lo: lo + 12, ch: { drive: 1.6, driveLp: 7000, dly: 0.2, level: 0.75 }, pat: {}, lab: P('acid', { cutoff: 220, res: 0.86, env: 0.5, decay: 0.3 }) };
    const na = setPats(T.parts.acid, 'a', variants(r, core, 3, 0.12, ['r', 'o', 'R', 'f', '.', 's', 'O']));
    // A dub-techno blip: two notes with the delay, in and out.
    const bm = motif(r, { bars: 2, density: 'sparse', figure: 0.6 });
    T.parts.blip = { type: 'mel', res: 1, ch: { rev: 0.4, dly: 0.7, pan: -0.3, hp: 400, level: 1.4 }, pat: hooks(r, bm, { scale, tonic: 72 + tonic, prog, lo: -2, hi: 6, gate: 2 }), lab: P('glass') };
    S.push({ bars: 16, drums: 'intro', lp: [300, 16000], p: { rumble: 'off' } });
    S.push({ bars: 16, drums: 'intro2', crash: true, auto: { 'acid.cutoff': [150, 400] }, p: { rumble: 'off', acid: 'a0' } });
    S.push({ bars: 16, drums: 'd', vd: nd, fill: 'perc', auto: { 'acid.cutoff': [400, 900] }, p: { rumble: 'off', acid: 'a', chord: 'dub', blip: 'sparse' }, v: { acid: na } });
    S.push({ bars: 16, drums: 'brk', down: 2, auto: { 'acid.cutoff': [500, 2000], 'acid.decay': [0.25, 0.8], 'acid.res': [0.86, 0.92] }, p: { acid: 'a0', chord: 'dub', pad: 'hold' } });
    S.push({ bars: 8, drums: 'build', riser: 8, swell: true, gap: 1, fill: 'perc', auto: { 'acid.cutoff': [1200, 2400] }, p: { rumble: 'roll', acid: 'a0', pad: 'hold' } });
    S.push({ bars: 16, drums: 'd', vd: nd, crash: true, drop: true, auto: { 'acid.cutoff': [1800, 900], 'acid.decay': [0.5, 0.3] }, p: { rumble: 'off', acid: 'a', chord: 'dub', blip: 'hook' }, v: { acid: na } });
    S.push({ bars: 16, drums: 'd', vd: nd, fill: 'perc', auto: { 'acid.cutoff': [900, 500], 'acid.env': [0.5, 0.75] }, p: { rumble: 'off', acid: 'a', chord: 'dub', blip: 'answer' }, v: { acid: na } });
    S.push({ bars: 16, drums: 'd', vd: nd, auto: { 'acid.cutoff': [500, 250] }, p: { rumble: 'off', acid: 'a' }, v: { acid: na } });
    S.push({ bars: 16, drums: 'intro', lp: [16000, 300], p: { rumble: 'off' } });
  } else {
    // The sequence: a 16th arpeggio over chord tones, with its own shape.
    const arps = ['0 0 1 0 2 0 1 0 0 0 1 0 3 0 1 0', '0 1 2 1 3 1 2 1 0 1 2 1 4 1 2 1', '0 . 1 . 2 . 1 . 0 . 1 . 3 . 1 .', '0 0 2 0 0 2 0 0 2 0 0 2 0 0 1 0', '0 2 1 3 0 2 4 3 0 2 1 3 0 2 5 3'];
    const arpCore = r.pick(arps);
    const swapArp = (s) => s.split(' ').map((t) => (t !== '.' && r.chance(0.15) ? String(r.int(0, 4)) : t)).join(' ');
    T.parts.arp = { type: 'arp', lo: 60, res: 1, ch: { rev: 0.35, dly: 0.5, pan: -0.15, pump: true, level: 0.85 }, pat: {}, lab: P('pluckTrance', { cutoff: 700, gain: 0.5, name: 'sequence' }) };
    const na = setPats(T.parts.arp, 'a', variants(r, arpCore, 3, 0, null, swapArp));
    T.parts.sub = { type: 'bass', lo: lo + 12, ch: { pump: true, level: 1.6 }, pat: { off: '..r...r...r...r.', roll: 'r.r.r.r.r.r.r.r.' }, lab: P('sub') };
    const lm = motif(r, { bars: 2, density: 'sparse', figure: 0.7, shape: 'arch' });
    T.parts.lead = { type: 'mel', res: 1, legato: true, ch: { rev: 0.5, dly: 0.55, hp: 300, pump: true, level: 0.8 }, pat: hooks(r, lm, { scale, tonic: 60 + tonic, prog, lo: -2, hi: 8, gate: 12, phrase: 4 }), lab: P('sawLead', { cutoff: 1800, glide: 0.08 }) };
    S.push({ bars: 16, drums: 'intro', lp: [300, 16000], p: { rumble: 'off' } });
    S.push({ bars: 16, drums: 'intro2', crash: true, auto: { 'arp.cutoff': [500, 900] }, p: { rumble: 'off', sub: 'off', arp: 'a0' } });
    S.push({ bars: 16, drums: 'd', vd: nd, fill: 'perc', auto: { 'arp.cutoff': [900, 1600] }, p: { rumble: 'off', sub: 'off', arp: 'a', chord: 'dub' }, v: { arp: na } });
    S.push({ bars: 16, drums: 'brk', down: 2, auto: { 'pad.cutoff': [600, 2400], 'arp.cutoff': [800, 2400] }, p: { arp: 'a0', pad: 'hold', lead: 'sparse' } });
    S.push({ bars: 8, drums: 'build', riser: 8, swell: true, gap: 1, fill: 'perc', p: { sub: 'roll', arp: 'a0', pad: 'hold', lead: 'hook' } });
    S.push({ bars: 16, drums: 'd', vd: nd, crash: true, drop: true, p: { rumble: 'off', sub: 'off', arp: 'a', chord: 'dub', pad: 'hold', lead: 'hook' }, v: { arp: na } });
    S.push({ bars: 16, drums: 'd', vd: nd, fill: 'perc', p: { rumble: 'off', sub: 'off', arp: 'a', chord: 'dub', pad: 'hold', lead: 'answer' }, v: { arp: na } });
    S.push({ bars: 16, drums: 'd', vd: nd, auto: { 'arp.cutoff': [1600, 500] }, p: { rumble: 'off', sub: 'off', arp: 'a' }, v: { arp: na } });
    S.push({ bars: 16, drums: 'intro', lp: [16000, 300], p: { rumble: 'off', sub: 'off' } });
  }
  T.sections = expand(S, r, dejavu);
  return T;
}

// ── Trance ───────────────────────────────────────────────────────
// Uplifting: the rolling off-beat bass from bar one, a pluck arpeggio, the
// supersaw hook alone over pads in a long breakdown, a snare build, and the
// drop with everything.
export function trance(seed, { dejavu = 0.75 } = {}) {
  const r = R(seed, 3);
  const tonic = r.int(0, 11), bpm = r.int(136, 140);
  const scale = SCALES.minor;
  const prog = progression(scale, tonic, r.pick(PROGS.anthem), '');
  const T = skeleton(`trance-${seed}`, {
    title: `Trance ${seed}`, style: `Trance · ${NOTE[tonic]} minor · ${prog}`,
    bpm, gain: 1.3, delay: 0.75, delayFb: 0.4, pump: { depth: 0.5, release: 0.16 },
    kitName: 'tr909',
    kit: { kick: { s: 'kickTight', g: 1.05 }, clap: { rev: 0.35 }, snare: { s: 'snareCrisp', rev: 0.3 } },
    prog: { a: prog },
    lay: { clap: 0.2, snare: 0.6, ohat: 0.3, ride: 0.7, hat: 0.1, arp: 0.35, lead: 0.3, pad: 0.15 },
  });

  const hat = cells(r, [['o.x.', 5], ['oox.', 2], ['o.xo', 2]]);
  const kick8 = loop(8, 'x...x...x...x...', r.pick(['x...x...x.......', 'x...x...x...x...']));
  const groove = { kick: kick8, clap: '....x.......x...', hat };
  const d0 = { ...groove, snare: '....x.......x...', ohat: '..x...x...x...x.' };
  const d1 = { ...d0, hat: mutate(r, hat, 0.1, ['x', 'o', '.']), ride: '..x...x...x...x.' };
  const nd = setDrums(T, 'd', [d0, d1]);
  const ng = setDrums(T, 'g', [groove, { ...groove, hat: mutate(r, hat, 0.1, ['x', 'o', '.']) }]);
  T.drums.intro = { kick: 'x...x...x...x...', hat: '..x...x...x...x.' };
  T.drums.brk = { hat: lane(16, (i) => (i % 2 ? '.' : 'o')) };
  // The build: snare on the beats, then 8ths, 16ths, and the kick roll.
  T.drums.build = {
    kick: loop(7, 'x...x...x...x...', 'x.......x.......') + 'x...x...x...x...',
    snare: bars(8, (b) => (b < 2 ? '....x.......x...' : b < 4 ? 'x...x...x...x...' : b < 6 ? 'x.x.x.x.x.x.x.x.' : 'xxxxxxxxxxxxxxxx')),
    hat: 'x.x.x.x.x.x.x.x.',
  };

  const table = [['.r.r', 5], ['.rr.', 1], ['.rrr', 1.5], ['.R.r', 1.5], ['..r.', 0.5]];
  const bassCore = bars(4, (b) => bassBar(r, table, 0.6, changesAfter(prog, b)).replace(/\.\.rn|\.\.n\./g, '.r.n'));
  T.parts.bass = { type: 'bass', lo: bassLo(tonic), ch: { pump: true, level: 1.4 }, pat: {}, lab: P('tranceBass') };
  const nb = setPats(T.parts.bass, 'b', variants(r, bassCore, 2, 0.08, ['r', 'R', 'o', '.']));

  const arps = ['0 0 1 0 2 0 1 0 0 0 1 0 3 0 1 0', '0 1 2 1 3 1 2 1 0 1 2 1 4 1 2 1', '0 . 1 . 2 . 1 . 0 . 1 . 3 . 1 .', '0 0 2 0 0 2 0 0 2 0 0 2 0 0 1 0', '0 2 1 2 0 2 1 2 0 2 1 2 3 2 1 2'];
  const swapArp = (s) => s.split(' ').map((t) => (t !== '.' && r.chance(0.15) ? String(r.int(0, 4)) : t)).join(' ');
  T.parts.arp = { type: 'arp', lo: 64, res: 1, ch: { rev: 0.35, dly: 0.45, pan: -0.2, pump: true, level: 0.8 }, pat: {}, lab: P('pluckTrance', { gain: 0.5 }) };
  const na = setPats(T.parts.arp, 'a', variants(r, r.pick(arps), 2, 0, null, swapArp));

  T.parts.pad = { type: 'chord', lo: 55, ch: { rev: 0.55, hp: 200, level: 1.8, pump: true }, pat: { hold: 'x---------------' }, lab: P('superPadWide') };
  const m = motif(r, { bars: 2, density: 'medium', figure: 0.6 });
  T.parts.lead = { type: 'mel', res: 1, ch: { rev: 0.45, dly: 0.5, hp: 300, pump: true, level: 0.95 }, pat: hooks(r, m, { scale, tonic: 60 + tonic, prog, lo: -1, hi: 9, gate: 8 }), lab: P('supersaw', { gain: 0.13 }) };

  const S = T.sections;
  S.push({ bars: 16, drums: 'intro', lp: [500, 16000], p: { bass: 'b0' } });
  S.push({ bars: 16, drums: 'g', vd: ng, crash: true, fill: 'clap', p: { bass: 'b', arp: 'a' }, v: { bass: nb, arp: na } });
  S.push({ bars: 16, drums: 'brk', down: 2, auto: { 'pad.cutoff': [400, 1600] }, p: { pad: 'hold', lead: 'sparse' } });
  S.push({ bars: 16, drums: 'brk', auto: { 'lead.cutoff': [1500, 5000], 'pad.cutoff': [1600, 3000] }, p: { pad: 'hold', lead: 'hook', arp: 'a0' } });
  S.push({ bars: 8, drums: 'build', riser: 8, swell: true, fill: 'kickroll', gap: 2, auto: { 'lead.cutoff': [3000, 8000] }, p: { pad: 'hold', arp: 'a0', lead: 'hook' } });
  S.push({ bars: 16, drums: 'd', vd: nd, crash: true, drop: true, fill: 'clap', p: { bass: 'b', arp: 'a', lead: 'hook', pad: 'hold' }, v: { bass: nb, arp: na } });
  S.push({ bars: 16, drums: 'd', vd: nd, fill: 'clap', p: { bass: 'b', arp: 'a', lead: 'answer', pad: 'hold' }, v: { bass: nb, arp: na } });
  S.push({ bars: 16, drums: 'g', vd: ng, p: { bass: 'b', arp: 'a' }, v: { bass: nb, arp: na } });
  S.push({ bars: 16, drums: 'intro', lp: [16000, 400], p: { bass: 'b0' } });
  T.sections = expand(S, r, dejavu);
  return T;
}

// ── Eurobeat ─────────────────────────────────────────────────────
// The touge. Octave bass on the 8ths, gated snare, a bright saw lead with
// a dense chorus hook, brass hits, a 16th riff, and the final chorus a
// whole step up.
export function eurobeat(seed, { dejavu = 0.8 } = {}) {
  const r = R(seed, 4);
  const tonic = r.int(0, 11), bpm = r.int(152, 158);
  const scale = SCALES.minor;
  const verseDegs = r.pick(PROGS.verse), chorusDegs = r.pick([[5, 6, 0, 0], [5, 6, 0, 6], [0, 5, 6, 0], [3, 6, 0, 0], [0, 5, 2, 6]]);
  const progV = progression(scale, tonic, verseDegs), progC = progression(scale, tonic, chorusDegs);
  const up = (tonic + 2) % 12;
  const progC2 = progression(scale, up, chorusDegs);
  const T = skeleton(`eurobeat-${seed}`, {
    title: `Eurobeat ${seed}`, style: `Eurobeat · ${NOTE[tonic]} minor · ${progC} → ${NOTE[up]}`,
    bpm, gain: 1.25, delay: 0.5, delayFb: 0.28, pump: { depth: 0.25, release: 0.14 },
    kitName: 'tr909',
    kit: { kick: { s: 'kickTight' }, snare: { s: 'snareGated', rev: 0.35 }, clap: { rev: 0.3 } },
    prog: { v: progV, c: progC, c2: progC2 },
    lay: { clap: 0.4, ohat: 0.3, hat: 0.1, riff: 0.35, hits: 0.5, lead: 0.2, pad: 0.15 },
  });

  const verse = { kick: 'x...x...x...x...', snare: '....X.......X...', hat: '..x...x...x...x.' };
  const chorus = { kick: loop(8, 'x...x...x...x...', 'x...x...x...x.x.'), snare: '....X.......X...', clap: '....x.......x...', hat: 'xoxoxoxoxoxoxoxo', ohat: '..............x.' };
  const nc = setDrums(T, 'c', [chorus, { ...chorus, hat: 'x.x.x.x.x.x.x.x.', ohat: '......x.......x.' }]);
  T.drums.v = verse;
  T.drums.intro = { kick: 'x...x...x...x...', hat: 'x.x.x.x.x.x.x.x.' }; // the snare arrives with the verse
  T.drums.pre = { kick: 'x...x...x...x...', snare: '....X.......X...', hat: 'x.x.x.x.x.x.x.x.', clap: '....x.......x...' };
  T.drums.brk = { hat: '..x...x...x...x.', shaker: 'xoxoxoxoxoxoxoxo' };
  T.drums.build = { kick: 'x...x...x...x...', snare: bars(8, (b) => (b < 4 ? '....X.......X...' : b < 6 ? 'x...x...x...x...' : 'x.x.x.x.x.x.x.x.')), hat: 'x.x.x.x.x.x.x.x.' };

  const octave = 'r.o.r.o.r.o.r.o.';
  const last = r.pick(['r.o.r.o.r.o.rr.n', 'r.o.r.o.f.o.n.n.', 'r.o.r.o.r.o.r.n.', 'r.o.r.o.r.o.u.n.']);
  T.parts.bass = { type: 'bass', lo: bassLo(tonic), ch: { pump: true, level: 1 }, pat: {}, lab: P('sawBass', { cutoff: 320, fenv: 2.2 }) };
  const nb = setPats(T.parts.bass, 'b', [loop(4, octave, last), loop(4, octave, r.pick(['r.o.r.o.r.o.f.n.', 'r.o.r.o.rr.o.n.n'])), loop(4, octave, octave)]);

  const riffs = ['0 1 2 1 0 1 2 1 0 1 3 1 0 1 2 1', '0 2 1 2 0 2 1 2 3 2 1 2 0 2 1 2', '0 0 1 1 2 2 1 1 0 0 1 1 3 3 1 1', '0 1 2 3 2 1 0 1 2 3 2 1 0 1 2 1'];
  T.parts.riff = { type: 'arp', lo: 64, res: 1, ch: { rev: 0.25, dly: 0.3, pan: 0.25, pump: true, level: 0.7 }, pat: {}, lab: P('sqArp', { cutoff: 3000, gain: 0.2 }) };
  const nr = setPats(T.parts.riff, 'r', variants(r, r.pick(riffs), 1, 0, null, (s) => s.split(' ').map((t) => (r.chance(0.2) ? String(r.int(0, 3)) : t)).join(' ')));
  T.parts.hits = { type: 'chord', lo: 57, ch: { rev: 0.35, pump: true, level: 0.8 }, pat: { h: loop(2, r.pick(['x..x..x.........', 'x.....x.x.......', 'x..x............']), 'x..x..x...x.x...') }, lab: P('hit') };
  T.parts.pad = { type: 'chord', lo: 57, ch: { rev: 0.5, hp: 200, level: 1.1, pump: true }, pat: { hold: 'x---------------' }, lab: P('superPad') };

  // Two motifs: the chorus hook, dense; the verse, calmer and lower.
  const mc = motif(r, { bars: 2, density: 'dense', figure: 0.7 });
  const mv = motif(r, { bars: 2, density: 'medium', figure: 0.5 });
  const HC = hooks(r, mc, { scale, tonic: 60 + tonic, prog: progC, lo: -1, hi: 10, gate: 4 });
  const HC2 = hooks(r, mc, { scale, tonic: 60 + up, prog: progC2, lo: -1, hi: 10, gate: 4 });
  const HV = hooks(r, mv, { scale, tonic: 60 + tonic, prog: progV, lo: -3, hi: 6, gate: 6 });
  T.parts.lead = { type: 'mel', res: 1, legato: true, ch: { rev: 0.3, dly: 0.3, hp: 300, level: 0.95 }, pat: { hook: HC.hook, answer: HC.answer, sparse: HC.sparse, hook2: HC2.hook, verse: HV.hook, verse2: HV.answer }, lab: P('euroLead', { gain: 0.09 }) };

  const S = T.sections;
  S.push({ bars: 8, prog: 'v', drums: 'intro', crash: true, lp: [800, 16000], p: { riff: 'r0', bass: 'b0' } });
  S.push({ bars: 16, prog: 'v', drums: 'v', fill: 'snare', p: { bass: 'b', pad: 'hold', lead: 'verse' }, v: { bass: nb } });
  S.push({ bars: 8, prog: 'v', drums: 'pre', riser: 4, swell: true, fill: 'tom', gap: 1, p: { bass: 'b0', pad: 'hold', hits: 'h' } });
  S.push({ bars: 16, prog: 'c', drums: 'c', vd: nc, crash: true, drop: true, fill: 'snare', p: { bass: 'b', lead: 'hook', riff: 'r', hits: 'h', pad: 'hold' }, v: { bass: nb, riff: nr } });
  S.push({ bars: 16, prog: 'v', drums: 'v', fill: 'snare', p: { bass: 'b', pad: 'hold', lead: 'verse2', riff: 'r0' }, v: { bass: nb } });
  S.push({ bars: 8, prog: 'v', drums: 'brk', down: 2, auto: { 'pad.cutoff': [600, 2400] }, p: { pad: 'hold', lead: 'sparse' } });
  S.push({ bars: 8, prog: 'v', drums: 'build', riser: 8, swell: true, fill: 'roll', gap: 2, p: { bass: 'b0', pad: 'hold', hits: 'h' } });
  S.push({ bars: 16, prog: 'c', drums: 'c', vd: nc, crash: true, drop: true, fill: 'snare', p: { bass: 'b', lead: 'hook', riff: 'r', hits: 'h', pad: 'hold' }, v: { bass: nb, riff: nr } });
  S.push({ bars: 16, prog: 'c2', drums: 'c', vd: nc, crash: true, fill: 'snare', p: { bass: 'b', lead: 'hook2', riff: 'r', hits: 'h', pad: 'hold' }, v: { bass: nb, riff: nr } });
  S.push({ bars: 8, prog: 'c2', drums: 'intro', lp: [16000, 600], p: { riff: 'r0', bass: 'b0' } });
  T.sections = expand(S, r, dejavu);
  return T;
}

// ── Psytrance ────────────────────────────────────────────────────
// Full-on: the rolling bass between the kicks, all root, a 303 squelch under
// slow filter motion, FM zaps for the hook in phrygian, a dark pad in the
// break, odd-length percussion.
export function psytrance(seed, { dejavu = 0.8 } = {}) {
  const r = R(seed, 5);
  const tonic = r.int(0, 11), bpm = r.int(142, 146);
  const scale = SCALES.phrygian;
  const prog = progression(scale, tonic, r.pick(PROGS.drone), '');
  const T = skeleton(`psy-${seed}`, {
    title: `Psytrance ${seed}`, style: `Psytrance · ${NOTE[tonic]} phrygian · ${prog}`,
    bpm, gain: 1.25, delay: 0.75, delayFb: 0.42, pump: { depth: 0.4, release: 0.1 },
    kitName: 'tr909',
    kit: { kick: { s: 'kickTight', g: 1.1 }, clap: { rev: 0.35 } },
    prog: { a: prog },
    lay: { clap: 0.4, ohat: 0.3, rim: 0.5, snap: 0.6, hat: 0.1, acid: 0.3, lead: 0.35, pad: 0.2 },
  });

  const hat = cells(r, [['..x.', 5], ['.ox.', 2], ['..xo', 2]]);
  const kick8 = loop(8, 'x...x...x...x...', r.pick(['x...x...x.......', 'x...x...x...x...']));
  const rim = euclid(7, 16, r.int(0, 5), 'o');
  const snapLen = r.pick([6, 10]);
  const snap = lane(snapLen, (i) => (i === 0 ? 'x' : '.'));
  const g0 = { kick: kick8, hat, ohat: '......x.......x.', rim };
  const g1 = { ...g0, hat: 'xoxoxoxoxoxoxoxo'.replace(/x/g, (c, i) => (i % 4 === 2 ? 'x' : 'o')), snap };
  const ng = setDrums(T, 'g', [g0, g1]);
  const nd = setDrums(T, 'd', [{ ...g0, clap: '....x.......x...' }, { ...g1, clap: '....x.......x...' }]);
  T.drums.intro = { kick: 'x...x...x...x...', hat };
  T.drums.brk = { hat: lane(16, (i) => (i % 2 ? '.' : 'o')), rim };
  T.drums.build = { kick: loop(7, 'x...x...x...x...', 'x.......x.......') + 'x...x...x...x...', hat: 'xxxxxxxxxxxxxxxx', snare: bars(8, (b) => (b < 4 ? '....x.......x...' : b < 6 ? 'x...x...x...x...' : 'x.x.x.x.x.x.x.x.')) };

  const lo = bassLo(tonic);
  const beat = r.pick(['.rrr', '.rrr', '.rr.']);
  const bassCore = loop(4, beat.repeat(4), beat.repeat(3) + r.pick(['.ooo', '.fff', '.rrr', '.rRr']));
  T.parts.bass = { type: 'bass', lo, ch: { pump: true, level: 1 }, pat: {}, lab: P('psyBass') };
  const nb = setPats(T.parts.bass, 'b', [bassCore, loop(4, beat.repeat(4), '.rr.'.repeat(3) + '.ooo'), loop(4, beat.repeat(4), beat.repeat(2) + '.r.r.rrr')]);

  const core = acidLine(r, 0.7, [['r', 6], ['o', 3], ['f', 2], ['t', 2], ['s', 1], ['u', 1]]);
  T.parts.acid = { type: 'bass', lo: lo + 12, ch: { drive: 1.4, driveLp: 8000, dly: 0.25, level: 0.7 }, pat: {}, lab: P('acid', { cutoff: 260, res: 0.84, env: 0.55, decay: 0.25 }) };
  const na = setPats(T.parts.acid, 'a', variants(r, core, 2, 0.12, ['r', 'o', 'R', 'f', '.', 't']));

  T.parts.pad = { type: 'chord', lo: 48, ch: { rev: 0.6, hp: 150, level: 0.8, pump: true }, pat: { hold: 'x---------------' }, lab: P('darkPad') };
  const m = motif(r, { bars: 2, density: 'dense', figure: 0.5 });
  T.parts.lead = { type: 'mel', res: 1, ch: { rev: 0.3, dly: 0.45, hp: 400, pan: -0.15, level: 0.9 }, pat: hooks(r, m, { scale, tonic: 60 + tonic, prog, lo: -3, hi: 9, gate: 2 }), lab: P('zap', { gain: 0.16 }) };

  const S = T.sections;
  S.push({ bars: 16, drums: 'intro', lp: [250, 16000], p: { bass: 'b0' } });
  S.push({ bars: 16, drums: 'g', vd: ng, crash: true, auto: { 'acid.cutoff': [200, 600] }, p: { bass: 'b', acid: 'a' }, v: { bass: nb, acid: na } });
  S.push({ bars: 16, drums: 'g', vd: ng, fill: 'perc', auto: { 'acid.cutoff': [600, 1400] }, p: { bass: 'b', acid: 'a', lead: 'sparse' }, v: { bass: nb, acid: na } });
  S.push({ bars: 16, drums: 'brk', down: 2, auto: { 'acid.cutoff': [1200, 2400], 'acid.res': [0.84, 0.92], 'pad.cutoff': [500, 1800] }, p: { pad: 'hold', lead: 'sparse', acid: 'a0' } });
  S.push({ bars: 8, drums: 'build', riser: 8, swell: true, fill: 'kickroll', gap: 2, p: { bass: 'b0', pad: 'hold', acid: 'a0' } });
  S.push({ bars: 16, drums: 'd', vd: nd, crash: true, drop: true, auto: { 'acid.cutoff': [1800, 900] }, p: { bass: 'b', acid: 'a', lead: 'hook' }, v: { bass: nb, acid: na } });
  S.push({ bars: 16, drums: 'd', vd: nd, fill: 'perc', auto: { 'acid.cutoff': [900, 600] }, p: { bass: 'b', acid: 'a', lead: 'answer', pad: 'hold' }, v: { bass: nb, acid: na } });
  S.push({ bars: 16, drums: 'g', vd: ng, auto: { 'acid.cutoff': [600, 300] }, p: { bass: 'b', acid: 'a' }, v: { bass: nb, acid: na } });
  S.push({ bars: 16, drums: 'intro', lp: [16000, 300], p: { bass: 'b0' } });
  T.sections = expand(S, r, dejavu);
  return T;
}

// ── Drum & bass ──────────────────────────────────────────────────
// Liquid: 2-step breaks with ghost snares, a sub and a reese holding long
// notes, 7th chords on EP and a wide pad, a pretty hook. Roller: darker,
// the bass on the 8ths, a minimal chord, the hook on a square lead.
export function dnb(seed, { dejavu = 0.75 } = {}) {
  const r = R(seed, 6);
  const sub = r.fork(1).chance(0.6) ? 'liquid' : 'roller';
  const tonic = r.int(0, 11), bpm = r.int(172, 176);
  const scale = sub === 'liquid' ? (r.chance(0.5) ? SCALES.dorian : SCALES.minor) : SCALES.minor;
  const prog = progression(scale, tonic, r.pick(sub === 'liquid' ? noDim(scale, PROGS.deep) : PROGS.verse), sub === 'liquid' ? r.pick(['7', '9']) : r.pick(['', '7']));
  const T = skeleton(`dnb-${seed}`, {
    title: `Drum & bass ${seed}`, style: `${sub === 'liquid' ? 'Liquid' : 'Roller'} drum & bass · ${NOTE[tonic]} ${scaleName(scale)} · ${prog}`,
    bpm, gain: 1.2, delay: 0.75, delayFb: 0.35, pump: { depth: 0.3, release: 0.1 },
    kitName: 'tr909',
    kit: { kick: { s: 'kickTight', g: 1.2 }, snare: { s: 'snareCrisp', rev: 0.25, g: 1.25 }, hat: { g: 0.9 } },
    prog: { a: prog },
    lay: { shaker: 0.5, ride: 0.7, hat: 0.1, ep: 0.3, lead: 0.4, pad: 0.15, reese: 0.25 },
  });

  const kick4 = loop(4, 'x.........x.....', r.pick(['x.........x..x..', 'x.........x.....', 'x.......x.x.....']));
  const ghosts = r.pick(['....x..o....x..o', '....x.......x.o.', '....x..o....x...', '....x.....o.x..o']);
  const hat = r.pick(['xoxoxoxoxoxoxoxo', 'x.x.x.x.x.x.x.x.', 'xox.xoxoxox.xoxo']);
  const d0 = { kick: kick4, snare: ghosts, hat, shaker: sub === 'liquid' ? 'oooooooooooooooo' : undefined };
  if (!d0.shaker) delete d0.shaker;
  const d1 = { ...d0, hat: mutate(r, hat, 0.12, ['x', 'o', '.']), ride: 'x.x.x.x.x.x.x.x.' };
  const d2 = { ...d0, kick: loop(4, 'x.........x.....', 'x.........x..x.x'), snare: mutate(r, ghosts, 0.1, ['o', '.', '.']) };
  const nd = setDrums(T, 'd', [d0, d1, d2]);
  T.drums.intro = { hat: lane(16, (i) => (i % 2 ? '.' : 'o')), shaker: 'oooooooooooooooo' };
  T.drums.intro2 = { ...T.drums.intro, kick: 'x...............', snare: '............x...' };
  T.drums.brk = { hat: lane(16, (i) => (i % 2 ? '.' : 'o')), shaker: 'o.o.o.o.o.o.o.o.' };
  T.drums.build = { kick: 'x.........x.....', snare: bars(8, (b) => (b < 4 ? '....x.......x...' : b < 6 ? 'x...x...x...x...' : 'x.x.x.x.x.x.x.x.')), hat: 'x.x.x.x.x.x.x.x.' };

  const lo = bassLo(tonic) - 12;
  T.parts.sub = { type: 'bass', lo: lo + 12, ch: { pump: false, level: 1 }, pat: {}, lab: P('sub') };
  const subCore = bars(2, (b) => (b === 0 ? 'r---------o-----' : 'r---------' + (changesAfter(prog, 1) ? 'n-----' : 'o-----')));
  const ns = setPats(T.parts.sub, 's', [subCore, bars(2, (b) => (b === 0 ? 'r-------........' : 'r---------f---n-'))]);
  if (sub === 'liquid') {
    T.parts.reese = { type: 'bass', lo: lo + 12, ch: { level: 0.42, hp: 60, pump: true }, pat: {}, lab: P('reese', { drive: 1.4 }) };
    setPats(T.parts.reese, 'r', [loop(2, 'r---------------', 'r---------------'), loop(2, 'r-------........', 'r---------n-----')]);
    T.parts.ep = { type: 'chord', lo: 57, ch: { rev: 0.4, dly: 0.3, pump: true, level: 0.8 }, pat: { comp: r.pick(['x---.x--..x-....', 'x-----.x------..', 'x--.x--.x-......']) }, lab: P('ep', { gain: 0.14 }) };
  } else {
    T.parts.reese = { type: 'bass', lo: lo + 12, ch: { level: 0.42, hp: 60, pump: true, drive: 1.2, driveLp: 6000 }, pat: {}, lab: P('darkBass') };
    setPats(T.parts.reese, 'r', [loop(2, 'r.r..r.r.r.r..r.', 'r.r..r.r.r.r.rn.'), loop(2, 'r...r...r...r...', 'r.r.r.r.r.r.r.n.')]);
    T.parts.ep = { type: 'chord', lo: 60, ch: { rev: 0.5, dly: 0.5, pump: true, level: 0.7 }, pat: { comp: r.pick(['......x.........', 'x.......x.......', '..x.......x.....']) }, lab: P('dubChord', { gain: 1.4 }) };
  }
  T.parts.pad = { type: 'chord', lo: 60, ch: { rev: 0.55, hp: 200, level: 0.8, pump: true }, pat: { hold: 'x---------------' }, lab: sub === 'liquid' ? P('superPadWide') : P('darkPad') };
  const m = motif(r, { bars: 2, density: sub === 'liquid' ? 'medium' : 'dense', figure: 0.5 });
  const leadPatch = sub === 'liquid' ? r.pick([P('glass', { gain: 0.14 }), P('bell'), P('ep', { gain: 0.105 })]) : P('sqLead');
  T.parts.lead = { type: 'mel', res: 1, legato: sub === 'roller', ch: { rev: 0.45, dly: 0.4, hp: 300, pan: 0.15, level: 0.85 }, pat: hooks(r, m, { scale, tonic: 60 + tonic, prog, lo: -2, hi: 9, gate: sub === 'liquid' ? 6 : 3 }), lab: leadPatch };

  const S = T.sections;
  S.push({ bars: 16, drums: 'intro', lp: [500, 16000], p: { pad: 'hold', ep: 'comp' } });
  S.push({ bars: 16, drums: 'intro2', p: { pad: 'hold', ep: 'comp', lead: 'sparse' } });
  S.push({ bars: 8, drums: 'build', riser: 8, swell: true, fill: 'roll', gap: 1, auto: { 'pad.cutoff': [800, 3000] }, p: { pad: 'hold', ep: 'comp', lead: 'hook' } });
  S.push({ bars: 16, drums: 'd', vd: nd, crash: true, drop: true, fill: 'dnb', p: { sub: 's', reese: 'r0', ep: 'comp', lead: 'hook' }, v: { sub: ns } });
  S.push({ bars: 16, drums: 'd', vd: nd, fill: 'dnb', p: { sub: 's', reese: 'r1', ep: 'comp', lead: 'answer', pad: 'hold' }, v: { sub: ns } });
  S.push({ bars: 16, drums: 'brk', down: 2, auto: { 'pad.cutoff': [500, 2500] }, p: { pad: 'hold', ep: 'comp', lead: 'sparse' } });
  S.push({ bars: 8, drums: 'build', riser: 8, swell: true, fill: 'roll', gap: 1, p: { pad: 'hold', ep: 'comp', lead: 'hook' } });
  S.push({ bars: 16, drums: 'd', vd: nd, crash: true, drop: true, fill: 'dnb', p: { sub: 's', reese: 'r0', ep: 'comp', lead: 'hook', pad: 'hold' }, v: { sub: ns } });
  S.push({ bars: 16, drums: 'd', vd: nd, fill: 'dnb', p: { sub: 's', reese: 'r1', ep: 'comp', lead: 'answer', pad: 'hold' }, v: { sub: ns } });
  S.push({ bars: 16, drums: 'intro', lp: [16000, 500], p: { pad: 'hold', ep: 'comp' } });
  T.sections = expand(S, r, dejavu);
  return T;
}

// ── UK garage ────────────────────────────────────────────────────
// 2-step: the kick skips, the snare cracks on 2 and 4, skippy swung hats, an
// organ bass with pickups, chopped chords, vowel stabs from the choir.
export function garage(seed, { dejavu = 0.7 } = {}) {
  const r = R(seed, 7);
  const tonic = r.int(0, 11), bpm = r.int(132, 136);
  const scale = r.chance(0.6) ? SCALES.dorian : SCALES.minor;
  const prog = progression(scale, tonic, r.pick(noDim(scale, PROGS.deep)), r.pick(['7', '9']));
  const progB = progression(scale, tonic, r.pick(noDim(scale, PROGS.verse)), '7');
  const T = skeleton(`garage-${seed}`, {
    title: `Garage ${seed}`, style: `UK garage · ${NOTE[tonic]} ${scaleName(scale)} · ${prog}`,
    bpm, swing: r.pick([0.18, 0.22, 0.25]), gain: 1, delay: 0.75, delayFb: 0.3, pump: { depth: 0.35, release: 0.14 },
    kitName: 'tr909',
    kit: { kick: { s: 'kickTight' }, snare: { s: 'snareCrisp', rev: 0.3 }, rim: { rev: 0.3 } },
    prog: { a: prog, b: progB },
    lay: { shaker: 0.45, rim: 0.5, ohat: 0.3, hat: 0.1, vox: 0.5, lead: 0.4, chords: 0.2, pad: 0.15 },
  });

  const kick = r.pick(['x.....x...x.....', 'x......x..x.....', 'x.....x.x.x.....']);
  const hat = cells(r, [['x.xx', 3], ['x.x.', 3], ['.x.x', 2], ['xx.x', 1], ['x..x', 1]]);
  const g0 = { kick: loop(4, kick, kick.slice(0, 12) + 'x.x.'), snare: '....X.......X...', hat, rim: r.pick(['..o...o..o..o..o', '......o.......o.', '..o......o......']), shaker: 'o.o.o.o.o.o.o.o.' };
  const g1 = { ...g0, hat: mutate(r, hat, 0.12, ['x', 'o', '.']), ohat: '......x.......x.' };
  const g2 = { ...g0, kick: loop(4, kick, kick), rim: euclid(5, 16, r.int(0, 3), 'o') };
  const nd = setDrums(T, 'g', [g0, g1, g2]);
  T.drums.intro = { hat, shaker: g0.shaker };
  T.drums.intro2 = { kick, hat, shaker: g0.shaker, rim: g0.rim };
  T.drums.brk = { hat: '..x...x...x...x.', shaker: g0.shaker, rim: g0.rim };
  T.drums.build = { kick, snare: bars(8, (b) => (b < 4 ? '....X.......X...' : b < 6 ? 'x...x...x...x...' : 'x.x.x.x.x.x.x.x.')), hat: 'x.x.x.x.x.x.x.x.' };

  const table = [['r..r', 3], ['.r.r', 2], ['r...', 2], ['..r.', 2], ['r.o.', 1], ['....', 0.5]];
  const bassCore = bars(4, (b) => bassBar(r, table, 0.8, changesAfter(prog, b)));
  T.parts.bass = { type: 'bass', lo: bassLo(tonic), ch: { pump: true, level: 0.8 }, pat: {}, lab: P('pluckBass', { cutoff: 300, res: 0.4, decay: 0.14, name: 'organ bass' }) };
  const nb = setPats(T.parts.bass, 'b', variants(r, bassCore, 2, 0.1, ['r', 'o', '.', '.']));

  const chop = r.pick(['x.x..x..x...x.x.', 'x..x..x...x.x...', '.x..x..x.x..x...']);
  T.parts.chords = { type: 'chord', lo: 58, gate: 0.6, ch: { pump: true, rev: 0.3, dly: 0.3, level: 0.4 }, pat: {}, lab: r.chance(0.5) ? P('organ') : P('ep') };
  const nc = setPats(T.parts.chords, 'c', variants(r, loop(4, chop, chop.slice(1) + chop[0]), 2, 0.1, ['x', '.']));
  T.parts.vox = { type: 'chord', lo: 64, gate: 0.5, ch: { rev: 0.45, dly: 0.35, pan: -0.2, pump: true, level: 0.8 }, pat: { stab: r.pick(['..x...x.....x...', '......x.......x.', '..x.......x.....']) }, lab: P('choir', { a: 0.01, r: 0.2, vowel: r.pick(['o', 'e']), name: 'vox' }) };
  T.parts.pad = { type: 'chord', lo: 55, ch: { pump: true, rev: 0.5, hp: 180, level: 0.75 }, pat: { hold: 'x---------------' }, lab: P('warmPad') };
  const m = motif(r, { bars: 2, density: 'sparse', figure: 0.5 });
  // The hook, and its answer realised over the verse progression it plays on.
  const H = hooks(r, m, { scale, tonic: 60 + tonic, prog, lo: -2, hi: 8, gate: 4 });
  const HB = hooks(r, m, { scale, tonic: 60 + tonic, prog: progB, lo: -2, hi: 8, gate: 4 });
  T.parts.lead = { type: 'mel', res: 1, ch: { rev: 0.4, dly: 0.4, pan: 0.2, pump: true, hp: 300, level: 0.85 }, pat: { ...H, answerB: HB.answer }, lab: P('glass', { gain: 0.2 }) };

  const S = T.sections;
  S.push({ bars: 8, drums: 'intro', lp: [600, 16000], p: { chords: 'c0' } });
  S.push({ bars: 8, drums: 'intro2', p: { bass: 'b0', chords: 'c0' } });
  S.push({ bars: 16, drums: 'g', vd: nd, crash: true, fill: 'skip', p: { bass: 'b', chords: 'c', pad: 'hold' }, v: { bass: nb, chords: nc } });
  S.push({ bars: 8, drums: 'brk', down: 2, auto: { 'pad.cutoff': [600, 2400] }, p: { pad: 'hold', lead: 'sparse', vox: 'stab' } });
  S.push({ bars: 8, drums: 'build', riser: 8, swell: true, fill: 'roll', gap: 2, p: { pad: 'hold', chords: 'c0', bass: 'b0' } });
  S.push({ bars: 16, drums: 'g', vd: nd, crash: true, drop: true, fill: 'skip', p: { bass: 'b', chords: 'c', lead: 'hook', vox: 'stab' }, v: { bass: nb, chords: nc } });
  S.push({ bars: 16, drums: 'g', vd: nd, prog: 'b', fill: 'skip', p: { bass: 'b', chords: 'c', lead: 'answerB' }, v: { bass: nb, chords: nc } });
  S.push({ bars: 16, drums: 'brk', down: 2, auto: { 'pad.cutoff': [500, 2800] }, p: { pad: 'hold', lead: 'hook', vox: 'stab' } });
  S.push({ bars: 8, drums: 'build', riser: 8, swell: true, fill: 'roll', gap: 2, p: { pad: 'hold', chords: 'c0' } });
  S.push({ bars: 16, drums: 'g', vd: nd, crash: true, drop: true, fill: 'skip', p: { bass: 'b', chords: 'c', lead: 'hook', vox: 'stab', pad: 'hold' }, v: { bass: nb, chords: nc } });
  S.push({ bars: 16, drums: 'intro2', lp: [16000, 500], p: { bass: 'b0', chords: 'c0' } });
  T.sections = expand(S, r, dejavu);
  return T;
}

// ── Chicha ───────────────────────────────────────────────────────
// Peruvian cumbia of the late sixties on (sound.md 3.5.3). Costeña (Los
// Destellos, Los Mirlos): the surf guitar with tremolo carries the melody,
// two guitars in thirds in the chorus, a combo organ underneath. Amazónica
// (Juaneco y su Combo): the lead through a wah rocked once a beat, the
// organ up front. Under both: the cumbia bass (root on the beat, the fifth
// on the off-beat before the next), the güiro's long-short-short, congas
// and bongos, the timbales' cáscara and the bell in the chorus. The
// melodies are minor pentatonic (the huayno in it), the harmony i, iv and
// V7 vamps. Form: the guitar hook alone, verse, chorus, verse, an organ
// solo over the rhythm, the abanico back into the chorus, the chorus with
// the organ in unison, and the hook again to close.
export function chicha(seed, { dejavu = 0.75 } = {}) {
  const r = R(seed, 8);
  const sub = r.fork(1).chance(0.35) ? 'amazonica' : 'costena';
  const tonic = r.int(0, 11), bpm = sub === 'amazonica' ? r.int(90, 98) : r.int(96, 104);
  const scale = SCALES.minor;
  const progV = progression(scale, tonic, r.pick(PROGS.cumbia));
  const progC = progression(scale, tonic, r.pick([[0, [4, 'dom'], 0, [4, 'dom']], [0, 3, [4, 'dom'], 0], [0, 0, [4, 'dom'], [4, 'dom']], [3, [4, 'dom'], 0, 0]]));
  const T = skeleton(`chicha-${seed}`, {
    title: `Chicha ${seed}`, style: `${sub === 'amazonica' ? 'Cumbia amazónica' : 'Chicha'} · ${NOTE[tonic]} minor · ${progC}`,
    bpm, gain: 1.1, delay: 0.5, delayFb: 0.22, pump: { depth: 0.08, release: 0.12 },
    kitName: 'latin',
    prog: { v: progV, c: progC },
    lay: { guiroS: 0.3, cascara: 0.45, shaker: 0.5, bongoH: 0.4, bongoL: 0.4, cowbell: 0.55, clave: 0.6, lead2: 0.35, rhythm: 0.2, organ: 0.15 },
  });

  // Percussion. The güiro's stroke per beat: long on the beat, two shorts
  // after; the conga's open tones on the "and" of 2 and the end of the bar,
  // the slap on 2 and 4; the bongos' martillo in the verse; the cáscara and
  // the bell in the chorus.
  const guiro = { guiroL: 'x...x...x...x...', guiroS: r.pick(['..xx..xx..xx..xx', '.x.x.x.x.x.x.x.x', '..xx..xx..xx.xxx']) };
  const congas = { congaO: r.pick(['......x.......xx', '......x.......x.', '..x...x.......xx']), congaS: '....x.......x...', tumba: r.pick(['............x...', '............x..x']) };
  const bongos = { bongoH: 'o.x.o.x.o.x.o.x.', bongoL: r.pick(['......x.......x.', '..x.......x.....']) };
  const kick = { kick: r.pick(['x.......x.......', 'x.......x.....x.']) };
  const cascara = r.pick(['x.x.xx.x.x.xx.x.', 'x.xx.x.xx.xx.x.x']);
  const verse = { ...kick, ...guiro, ...congas, ...bongos };
  const chorus = { ...kick, ...guiro, ...congas, cascara, cowbell: 'x...x...x...x...', shaker: 'x.x.x.x.x.x.x.x.' };
  const nv = setDrums(T, 'v', [verse, { ...verse, congaO: mutate(r, congas.congaO, 0.1, ['x', '.', '.']) }]);
  const nc = setDrums(T, 'c', [chorus, { ...chorus, cowbell: 'x..xx..xx..xx..x' }, { ...chorus, cascara: mutate(r, cascara, 0.1, ['x', '.']) }]);
  T.drums.intro = { ...guiro, clave: 'x..x..x...x.x...' };
  T.drums.brk = { ...guiro, ...congas, clave: 'x..x..x...x.x...' };
  T.drums.pre = { ...kick, ...guiro, ...congas, cascara };
  T.drums.coda = { ...guiro, ...congas };

  // The bass: the tumbao, root on the beat and the fifth on the off-beat
  // before the next, a pickup into every change; the bordoneo (a repeated
  // root) now and then.
  const lo = bassLo(tonic);
  const plain = [['r.....f.r.....f.', 6], ['r.....o.r.....f.', 1], ['r..r..f.r.....f.', 1.5], ['r.....f.r..r..f.', 1]];
  const pickup = [['r.....f.r.....n.', 5], ['r.....f.r...r.n.', 1], ['r.....o.r.....n.', 1]];
  const bassOver = (prog) => bars(4, (b) => r.weighted(changesAfter(prog, b) ? pickup : plain));
  T.parts.bass = { type: 'bass', lo, gate: 0.85, ch: { pump: true, level: 1.05 }, pat: {}, lab: P('fingerBass') };
  const nb = setPats(T.parts.bass, 'b', [bassOver(progV), bassOver(progV), bassOver(progC)]);

  // The rhythm guitar's "chaka": muted strums on the off-beat 8ths, and a
  // 16th pair at the end of the phrase.
  const chaka = r.pick(['..x...x...x...x.', '..x...x...x...xx', '..x..xx...x...x.']);
  T.parts.rhythm = { type: 'chord', lo: 55, gate: 0.45, ch: { rev: 0.15, pan: 0.3, level: 0.7 }, pat: {}, lab: P('rhythmGuitar') };
  const nr = setPats(T.parts.rhythm, 'r', variants(r, loop(4, chaka, chaka.slice(0, 12) + 'x.xx'), 1, 0.1, ['x', '.']));

  // The organ: held chords under the verse, stabs on 2 and 4 in the chorus.
  T.parts.organ = { type: 'chord', lo: 60, ch: { rev: 0.3, pan: -0.25, level: sub === 'amazonica' ? 0.7 : 0.45 }, pat: { pad: 'x---------------', stab: '....x---....x---' }, lab: P('comboOrgan') };

  // The guitars. The hook is the chorus; the verse has its own melody, lower
  // and calmer; the second guitar answers a third up, at the same time.
  const leadPatch = sub === 'amazonica' ? P('wahGuitar', { wahRate: bpm / 60 }) : P('surfGuitar', { tremRate: r.pick([5.2, 5.8, 6.4]) });
  const pent = [1, 5];
  const mc = motif(r, { bars: 2, density: 'medium', figure: 0.5 });
  const mv = motif(r, { bars: 2, density: r.pick(['sparse', 'medium']), figure: 0.4 });
  const HC = hooks(r, mc, { scale, tonic: 60 + tonic, prog: progC, lo: -3, hi: 9, gate: 5, avoid: pent });
  const HV = hooks(r, mv, { scale, tonic: 60 + tonic, prog: progV, lo: -5, hi: 6, gate: 6, avoid: pent });
  T.parts.lead = { type: 'mel', res: 1, legato: true, ch: { rev: 0.4, dly: 0.2, pan: 0.1, level: 1.4 }, pat: { hook: HC.hook, sparse: HC.sparse, answer: HC.answer, verse: HV.hook, verse2: HV.answer }, lab: leadPatch };
  T.parts.lead2 = { type: 'mel', res: 1, legato: true, ch: { rev: 0.4, dly: 0.15, pan: -0.3, level: 0.8 }, pat: { third: HC.answer }, lab: P('surfGuitar', { trem: 0.3, tremRate: 5.2, gain: 0.26 }) };
  // The organ solo: its own figure over the verse chords, pentatonic too,
  // in the organ's middle register (the top octave of a combo organ cuts).
  const mo = motif(r, { bars: 2, density: 'dense', figure: 0.6 });
  T.parts.organLead = { type: 'mel', res: 1, ch: { rev: 0.3, dly: 0.2, pan: -0.2, level: 0.8 }, pat: hooks(r, mo, { scale, tonic: 60 + tonic, prog: progV, lo: -3, hi: 7, gate: 3, avoid: pent }), lab: P('comboOrgan', { gain: 0.15 }) };

  const S = T.sections;
  S.push({ bars: 8, prog: 'c', drums: 'intro', p: { lead: 'hook' } });
  S.push({ bars: 16, prog: 'v', drums: 'v', vd: nv, fill: 'abanico', p: { bass: 'b', rhythm: 'r', organ: 'pad', lead: 'verse' }, v: { bass: 2, rhythm: nr } });
  S.push({ bars: 16, prog: 'c', drums: 'c', vd: nc, crash: true, drop: true, p: { bass: 'b2', rhythm: 'r', organ: 'stab', lead: 'hook', lead2: 'third' }, v: { rhythm: nr } });
  S.push({ bars: 16, prog: 'v', drums: 'v', vd: nv, fill: 'abanico', p: { bass: 'b', rhythm: 'r', organ: 'pad', lead: 'verse2' }, v: { bass: 2, rhythm: nr } });
  S.push({ bars: 16, prog: 'v', drums: 'brk', p: { bass: 'b0', organLead: 'hook' } });
  S.push({ bars: 8, prog: 'v', drums: 'pre', fill: 'abanico', p: { bass: 'b0', rhythm: 'r0', organ: 'pad', lead: 'sparse' } });
  S.push({ bars: 16, prog: 'c', drums: 'c', vd: nc, crash: true, drop: true, p: { bass: 'b2', rhythm: 'r', organ: 'stab', lead: 'hook', lead2: 'third' }, v: { rhythm: nr } });
  S.push({ bars: 16, prog: 'c', drums: 'c', vd: nc, crash: true, fill: 'abanico', p: { bass: 'b2', rhythm: 'r', organ: 'stab', lead: 'answer', lead2: 'third', organLead: 'answer' }, v: { rhythm: nr } });
  S.push({ bars: 8, prog: 'c', drums: 'coda', p: { bass: 'b2', lead: 'sparse' } });
  T.sections = expand(S, r, dejavu);
  return T;
}

export const GENRES = { house, techno, trance, eurobeat, psytrance, dnb, garage, chicha };
export const GENRE_NAMES = { house: 'House', techno: 'Techno', trance: 'Trance', eurobeat: 'Eurobeat', psytrance: 'Psytrance', dnb: 'Drum & bass', garage: 'UK garage', chicha: 'Chicha' };
