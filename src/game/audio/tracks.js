// The soundtrack: seven arranged songs for the Music sequencer (Music.js).
//
// A track has a tempo, chord progressions (one chord per bar, or several
// per bar joined by commas), drum patterns (one char per 16th: X accent,
// x hit, o ghost, . rest; patterns longer than a bar loop over bars), parts
// (instrument patch + mixer channel + patterns) and a list of sections that
// say which patterns play, for how many bars, and what happens at the
// seams: crash, drop (sub boom), fill, riser / swell (noise riser and
// reverse crash into the next section), down (noise downlifter), gap (the
// last 16ths go silent) and lp (a low-pass sweep over the whole section).
//
// Part types:
//   bass  one char per 16th relative to the chord's bass note: r root,
//         o octave, f fifth, t third, s seventh, l low root, u high fifth;
//         uppercase accents, '-' holds, '~' holds and slides into the next.
//   chord x / X hits of the voiced chord, '-' holds.
//   arp   space-separated chord-tone indexes (0 = lowest; past the top wraps up
//         an octave), '_' hold, '.' rest; `res` 16ths per token.
//   mel   space-separated notes (C#5), '_' hold, '.' rest, ! accent; `res`.
//
// Patch fields (Music.note): type saw|square|tri|sine|pulse|fm, voices,
// detune (cents across the stack), width (stereo), sub, oct, cutoff, q,
// fenv / fdec (filter envelope), fattack, keytrack, a d s r, gain, vib /
// vibRate / vibDelay, bend / bendT, glide; fm: mods [{ratio, index, dec,
// sus}], fmDetune. Channel fields: level, pan, rev, dly, pump, drive, hp,
// chorus.

// ── Patches ──────────────────────────────────────────────────────
const P = {
  sawBass: { type: 'saw', voices: 2, detune: 10, sub: 0.7, cutoff: 360, q: 3, fenv: 4, fdec: 0.14, a: 0.003, d: 0.2, s: 0.8, r: 0.05, gain: 0.13 },
  pluckBass: { type: 'square', sub: 0.7, cutoff: 320, q: 5, fenv: 5, fdec: 0.09, d: 0.16, s: 0.45, r: 0.04, gain: 0.22 },
  reese: { type: 'saw', voices: 3, detune: 30, width: 0.3, sub: 0.9, cutoff: 560, q: 1.6, a: 0.01, s: 1, r: 0.08, gain: 0.2 },
  sub: { type: 'sine', a: 0.004, d: 0.5, s: 0.85, r: 0.08, gain: 0.15, glide: 0.08 },
  darkBass: { type: 'saw', voices: 2, detune: 14, sub: 0.6, cutoff: 650, q: 2.5, fenv: 3, fdec: 0.1, s: 0.7, r: 0.04, gain: 0.2 },
  acid: { type: 'saw', cutoff: 380, q: 13, fenv: 8, fdec: 0.17, a: 0.002, d: 0.22, s: 0.55, r: 0.03, gain: 0.14, glide: 0.07 },
  roundBass: { type: 'tri', sub: 0.8, cutoff: 900, fenv: 1, fdec: 0.1, s: 0.8, r: 0.08, gain: 0.25 },
  superPad: { type: 'saw', voices: 4, detune: 26, width: 0.75, cutoff: 1900, q: 0.7, fattack: 1.2, a: 0.35, d: 1, s: 0.85, r: 0.9, gain: 0.075 },
  warmPad: { type: 'tri', voices: 2, detune: 12, width: 0.6, cutoff: 1500, a: 0.8, s: 1, r: 1.6, gain: 0.12 },
  darkPad: { type: 'saw', voices: 3, detune: 18, width: 0.8, cutoff: 850, q: 1.2, a: 0.7, s: 1, r: 1.1, gain: 0.065 },
  stab: { type: 'saw', voices: 3, detune: 22, width: 0.6, cutoff: 2300, q: 1, fenv: 1.6, fdec: 0.12, a: 0.003, d: 0.18, s: 0.3, r: 0.12, gain: 0.17 },
  hit: { type: 'saw', voices: 4, detune: 30, width: 0.8, sub: 0.3, cutoff: 3000, q: 1, fenv: 1, fdec: 0.3, a: 0.004, d: 0.5, s: 0.2, r: 0.4, gain: 0.15 },
  ep: { type: 'fm', mods: [{ ratio: 1, index: 1.5, dec: 0.9, sus: 0.15 }, { ratio: 14, index: 0.16, dec: 0.05, sus: 0 }], fmDetune: 6, a: 0.003, d: 1.3, s: 0.25, r: 0.45, gain: 0.145 },
  bell: { type: 'fm', mods: [{ ratio: 3.5, index: 2.2, dec: 0.6, sus: 0.05 }], a: 0.002, d: 0.9, s: 0.1, r: 0.6, gain: 0.14 },
  glass: { type: 'fm', mods: [{ ratio: 2, index: 1.2, dec: 0.3, sus: 0.1 }], a: 0.002, d: 0.35, s: 0.1, r: 0.3, gain: 0.12 },
  pluck: { type: 'saw', voices: 2, detune: 8, cutoff: 650, q: 2, fenv: 6, fdec: 0.08, a: 0.002, d: 0.18, s: 0, r: 0.12, gain: 0.085 },
  sqArp: { type: 'pulse', cutoff: 3000, q: 1, fenv: 1, fdec: 0.08, d: 0.12, s: 0.5, r: 0.08, gain: 0.1 },
  twang: { type: 'fm', mods: [{ ratio: 1, index: 2.6, dec: 0.25, sus: 0.12 }, { ratio: 3, index: 0.7, dec: 0.08, sus: 0 }], bend: 0.7, bendT: 0.07, a: 0.002, d: 0.7, s: 0.2, r: 0.3, gain: 0.16 },
  brassLead: { type: 'saw', voices: 3, detune: 14, width: 0.4, cutoff: 2400, q: 1.5, fenv: 1.3, fdec: 0.25, a: 0.02, d: 0.35, s: 0.8, r: 0.2, vib: 14, vibRate: 5.3, vibDelay: 0.25, gain: 0.12 },
  sawLead: { type: 'saw', voices: 4, detune: 32, width: 0.6, cutoff: 4000, q: 1, a: 0.008, s: 0.9, r: 0.14, vib: 10, vibDelay: 0.3, gain: 0.1, glide: 0.05 },
  sqLead: { type: 'pulse', voices: 2, detune: 10, width: 0.3, cutoff: 3400, q: 2, fenv: 0.8, fdec: 0.2, a: 0.006, s: 0.8, r: 0.12, vib: 12, vibDelay: 0.2, gain: 0.15, glide: 0.04 },
  whistle: { type: 'sine', a: 0.03, s: 0.9, r: 0.25, vib: 24, vibRate: 5.8, vibDelay: 0.16, bend: 0.5, bendT: 0.08, gain: 0.12, glide: 0.06 },
  flute: { type: 'tri', cutoff: 3200, a: 0.05, s: 0.85, r: 0.22, vib: 16, vibDelay: 0.25, gain: 0.14, glide: 0.05 },
};

// ── Songs ────────────────────────────────────────────────────────
export const TRACKS = [
  {
    id: 'midnight-run', title: 'Midnight Run', style: 'Outrun', bpm: 110, gain: 1,
    pump: { depth: 0.35, release: 0.2 },
    kit: { kick: { s: 'kickPunch' }, snare: { s: 'snareGated', rev: 0.3 } },
    prog: { v: 'Am Am F F C C G G', c: 'F G Am Am F G C E', b: 'Dm Am F G' },
    drums: {
      hats: { hat: '..x...x...x...x.' },
      verse: { kick: 'x.......x.x.....', snare: '....X.......X...', hat: 'x.x.x.x.x.x.x.x.' },
      chorus: { kick: 'x...x...x...x...', snare: '....X.......X...', hat: 'xoxoxoxoxoxoxoxo', ohat: '..x...x...x...x.' },
      build: { kick: 'x...x...x...x...', snare: '............x...', hat: 'x.x.x.x.x.x.x.x.' },
      brk: { kick: 'x...............', hat: '..x...x...x...x.' },
    },
    parts: {
      bass: { type: 'bass', inst: P.sawBass, lo: 33, ch: { pump: true }, pat: { drive: 'rrrrrrrrrrrrrrrr', oct: 'r.o.r.o.r.o.r.o.' } },
      pad: { type: 'chord', inst: P.superPad, lo: 57, ch: { pump: true, rev: 0.5, hp: 160 }, pat: { hold: 'x---------------' } },
      arp: { type: 'arp', inst: P.sqArp, lo: 64, ch: { rev: 0.2, dly: 0.45, pan: 0.15 }, pat: { a: '0 1 2 3 1 2 3 4 2 3 4 5 3 4 5 6', b: '0 2 1 3 2 4 3 5 0 2 1 3 2 4 3 5' } },
      lead: {
        type: 'mel', inst: P.brassLead, res: 2, ch: { rev: 0.35, dly: 0.3 },
        pat: {
          hook: `A4 _ _ G4 _ _ F4 _   G4 _ _ B4 _ _ D5 _   E5 _ _ _ _ _ D5 C5   E5 _ _ _ A4 _ _ _
                 A4 _ _ G4 _ _ F4 _   G4 _ _ B4 _ _ D5 _   E5 _ _ G5 _ _ E5 _   G#5 _ _ _ _ _ B4 _`,
        },
      },
      bell: {
        type: 'mel', inst: P.bell, res: 2, ch: { rev: 0.5, dly: 0.4, pan: -0.15 },
        pat: { m: 'F5 _ E5 _ D5 _ A4 _   C5 _ _ _ E5 _ _ _   A5 _ G5 _ F5 _ C5 _   D5 _ _ _ B4 _ _ _' },
      },
    },
    sections: [
      { bars: 8, prog: 'v', drums: 'hats', lp: [600, 14000], swell: true, p: { pad: 'hold', arp: 'a' } },
      { bars: 16, prog: 'v', drums: 'verse', crash: true, fill: 'snare', p: { bass: 'drive', pad: 'hold', arp: 'a' } },
      { bars: 16, prog: 'c', drums: 'chorus', crash: true, fill: 'tom', p: { bass: 'oct', pad: 'hold', lead: 'hook', arp: 'b' } },
      { bars: 8, prog: 'b', drums: 'brk', down: 2, p: { pad: 'hold', bell: 'm' } },
      { bars: 8, prog: 'v', drums: 'build', riser: 4, swell: true, fill: 'roll', gap: 2, p: { bass: 'drive', pad: 'hold', arp: 'a' } },
      { bars: 16, prog: 'c', drums: 'chorus', crash: true, drop: true, fill: 'tom', p: { bass: 'oct', pad: 'hold', lead: 'hook', arp: 'b', bell: 'm' } },
      { bars: 8, prog: 'v', drums: 'hats', crash: true, lp: [14000, 500], p: { pad: 'hold', arp: 'a' } },
    ],
  },

  {
    id: 'seabright', title: 'Seabright Dawn', style: 'Chill drive', bpm: 92, swing: 0.1, gain: 1,
    delay: 0.75, pump: { depth: 0.15, release: 0.25 },
    kit: { kick: { s: 'kickSoft' }, snare: { s: 'snareSoft', g: 0.75, rev: 0.35 }, rim: { rev: 0.35 }, hat: { s: 'hatSoft', g: 0.9 }, crash: { g: 0.7 } },
    prog: { a: 'Dmaj7 Bm7 Gmaj7 A', c: 'Gmaj7 A F#m7 Bm7 Gmaj7 A D D', b: 'Em7 F#m7 Gmaj7 A' },
    drums: {
      intro: { shaker: 'xoxoxoxoxoxoxoxo' },
      groove: { kick: 'x......x..x.....', rim: '....x.......x...', shaker: 'xoxoxoxoxoxoxoxo', hat: '..x...x...x...x.' },
      groove2: { kick: 'x......x..x...x.', snare: '....x.......x...', hat: 'x.x.x.x.x.x.x.x.', shaker: 'xoxoxoxoxoxoxoxo', ohat: '..............x.' },
    },
    parts: {
      bass: { type: 'bass', inst: P.roundBass, lo: 38, ch: { level: 0.8 }, pat: { a: 'r.....rr..f.o...', b: 'r..r...ro..f.r..' } },
      ep: { type: 'chord', inst: P.ep, lo: 57, ch: { chorus: 1, rev: 0.35, pump: true }, pat: { comp: 'x-----.x------..' } },
      pad: { type: 'chord', inst: P.warmPad, lo: 62, ch: { rev: 0.5, hp: 200, pump: true, level: 0.8 }, pat: { hold: 'x---------------' } },
      pluck: { type: 'arp', inst: P.glass, lo: 69, res: 2, ch: { rev: 0.4, dly: 0.5, pan: 0.25 }, pat: { a: '0 2 4 2 3 5 4 2', b: '0 1 2 4 3 2 1 2' } },
      lead: {
        type: 'mel', inst: { ...P.flute, oct: 1 }, res: 2, legato: true, ch: { rev: 0.45, dly: 0.3, pan: -0.1 },
        pat: {
          hook: `B4 _ _ A4 _ _ F#4 _   E4 _ _ _ A4 _ C#5 _   C#5 _ _ _ _ _ A4 _   B4 _ _ _ D5 _ _ _
                 B4 _ _ A4 _ _ F#4 _   E4 _ _ F#4 _ _ A4 _   F#4 _ _ _ _ _ E4 D4   D4 _ _ _ _ _ . .`,
        },
      },
      bell: { type: 'mel', inst: P.bell, res: 2, ch: { rev: 0.55, dly: 0.45, pan: 0.2 }, pat: { m: 'G5 _ F#5 _ E5 _ B4 _   A5 _ _ _ C#5 _ _ _   B5 _ A5 _ F#5 _ D5 _   E5 _ _ _ _ _ . .' } },
    },
    sections: [
      { bars: 8, prog: 'a', drums: 'intro', lp: [700, 16000], p: { ep: 'comp', pad: 'hold' } },
      { bars: 16, prog: 'a', drums: 'groove', p: { bass: 'a', ep: 'comp', pad: 'hold', pluck: 'a' } },
      { bars: 16, prog: 'c', drums: 'groove2', crash: true, fill: 'snare', p: { bass: 'b', ep: 'comp', pad: 'hold', lead: 'hook', pluck: 'b' } },
      { bars: 8, prog: 'b', drums: 'intro', down: 2, swell: true, p: { ep: 'comp', pad: 'hold', bell: 'm' } },
      { bars: 16, prog: 'c', drums: 'groove2', crash: true, p: { bass: 'b', ep: 'comp', pad: 'hold', lead: 'hook', pluck: 'a' } },
      { bars: 8, prog: 'a', drums: 'intro', lp: [16000, 600], p: { ep: 'comp', pad: 'hold', pluck: 'a' } },
    ],
  },

  {
    id: 'neon-rush', title: 'Neon Rush', style: 'Drum & bass', bpm: 172, gain: 1,
    pump: { depth: 0.3, release: 0.12 },
    kit: { kick: { s: 'kickTight' }, snare: { s: 'snareCrisp', rev: 0.2 }, ride: { g: 0.9 }, hat: { g: 0.9 } },
    prog: { a: 'Fm Fm Db Db Ab Ab Eb Eb', b: 'Bbm Bbm Db Db Fm Fm C C' },
    drums: {
      ride: { ride: 'x.x.x.x.x.x.x.x.', shaker: 'xoxoxoxoxoxoxoxo' },
      main: { kick: 'x.........x.....', snare: '....X..o.o..X...', hat: 'x.x.x.x.x.xox.x.', ohat: '..............x.' },
      main2: { kick: 'x.x.......x..x..', snare: '....X..o.o..X..o', hat: 'xoxoxoxoxoxoxoxo', ride: 'x...x...x...x...' },
      half: { kick: 'x...............', snare: '........X.......', hat: 'x.x.x.x.x.x.x.x.' },
      build: { kick: 'x...x...x...x...', snare: '....x.......x...', hat: 'x.x.x.x.x.x.x.x.' },
    },
    parts: {
      bass: { type: 'bass', inst: P.reese, lo: 41, ch: { drive: 1.6, driveLp: 3000, level: 0.25 }, pat: { reese: 'r-----------r-o-', roll: 'r..r..r...r.o.r.' } },
      pad: { type: 'chord', inst: P.darkPad, lo: 60, ch: { rev: 0.6, hp: 220, pump: true }, pat: { hold: 'x-------------------------------' } },
      stab: { type: 'chord', inst: P.stab, lo: 60, ch: { rev: 0.3, dly: 0.25 }, pat: { s: '..x.......x..x..' } },
      arp: { type: 'arp', inst: P.sqArp, lo: 65, ch: { rev: 0.25, dly: 0.4, pan: -0.2 }, pat: { a: '0 1 2 3 4 3 2 1 0 1 2 3 4 3 2 1' } },
      lead: {
        type: 'mel', inst: P.sawLead, res: 2, legato: true, ch: { rev: 0.35, dly: 0.25 },
        pat: {
          hook: `C5 _ _ Ab4 _ _ F4 _   G4 _ Ab4 _ C5 _ _ _   Db5 _ _ C5 _ _ Ab4 _   F4 _ _ _ _ _ . .
                 Eb5 _ _ C5 _ _ Ab4 _   Bb4 _ C5 _ Eb5 _ _ _   G5 _ _ F5 _ _ Eb5 _   Bb4 _ _ _ G4 _ _ _`,
        },
      },
      air: {
        type: 'mel', inst: { ...P.flute, gain: 0.1 }, res: 4, legato: true, ch: { rev: 0.6, dly: 0.35 },
        pat: { m: 'F5 _ Db5 _   Bb4 _ _ _   Ab4 _ F5 _   Eb5 _ Db5 _   C5 _ _ _   Ab4 _ _ _   G4 _ E5 _   _ _ . .' },
      },
    },
    sections: [
      { bars: 16, prog: 'a', drums: 'ride', lp: [500, 7000], riser: 4, swell: true, gap: 2, p: { pad: 'hold', arp: 'a' } },
      { bars: 32, prog: 'a', drums: 'main', crash: true, drop: true, fill: 'dnb', p: { bass: 'reese', pad: 'hold', stab: 's' } },
      { bars: 16, prog: 'b', drums: 'main2', crash: true, fill: 'dnb', p: { bass: 'roll', pad: 'hold', arp: 'a' } },
      { bars: 16, prog: 'b', drums: 'half', down: 2, p: { pad: 'hold', air: 'm' } },
      { bars: 8, prog: 'a', drums: 'build', riser: 8, swell: true, fill: 'roll', gap: 2, p: { pad: 'hold', arp: 'a' } },
      { bars: 32, prog: 'a', drums: 'main', crash: true, drop: true, fill: 'dnb', p: { bass: 'reese', pad: 'hold', stab: 's', lead: 'hook' } },
      { bars: 8, prog: 'a', drums: 'ride', crash: true, lp: [12000, 400], p: { pad: 'hold', arp: 'a' } },
    ],
  },

  {
    id: 'mirage', title: 'Mirage Highway', style: 'Desert western', bpm: 96, swing: 0.14, gain: 0.92,
    delay: 0.5, delayFb: 0.3,
    kit: { kick: { s: 'kickBoom', g: 0.9 }, snare: { s: 'snareFat', g: 0.85, rev: 0.45 }, snap: { rev: 0.4 }, tomL: { g: 0.9, rev: 0.35 }, tomM: { g: 0.9, rev: 0.35 }, shaker: { g: 1.2 } },
    prog: { a: 'Em D C B', b: 'Am Em B7 Em', c: 'C D Em Em C D B B' },
    drums: {
      wind: { shaker: 'x..ox..ox..ox..o', tomL: 'x...............' },
      trot: { kick: 'x.....x...x.....', snap: '....x.......x...', shaker: 'x.xox.xox.xox.xo' },
      big: { kick: 'x.....x...x.....', snare: '....X.......X...', shaker: 'x.xox.xox.xox.xo', ohat: '..x...x...x...x.', tomM: '..............x.' },
      half: { kick: 'x...............', snare: '........X.......', shaker: 'x.xox.xox.xox.xo' },
    },
    parts: {
      bass: { type: 'bass', inst: P.roundBass, lo: 40, ch: {}, pat: { a: 'r.....r.f...r...', b: 'r...f...r...f...' } },
      twang: { type: 'arp', inst: P.twang, lo: 52, res: 2, ch: { rev: 0.45, dly: 0.35, pan: 0.2 }, pat: { arp: '0 1 2 3 2 1 2 1', arp2: '0 . 2 . 4 . 2 .', strum: '0 1 2 3 . 3 2 1' } },
      pad: { type: 'chord', inst: P.darkPad, lo: 52, ch: { rev: 0.6, hp: 180, level: 0.85 }, pat: { hold: 'x---------------' } },
      lead: {
        type: 'mel', inst: P.whistle, res: 2, legato: true, ch: { rev: 0.6, dly: 0.35, pan: -0.15 },
        pat: {
          whistle: `E5 _ _ _ _ _ G5 _   F#5 _ _ _ A5 _ _ _   B5 _ _ _ _ _ _ _   G5 _ F#5 _ E5 _ _ _
                    E5 _ _ _ _ _ G5 _   A5 _ _ _ F#5 _ D5 _   D#5 _ _ _ _ _ _ _   F#5 _ _ _ D#5 _ B4 _`,
        },
      },
    },
    sections: [
      { bars: 8, prog: 'a', drums: 'wind', lp: [900, 16000], p: { twang: 'arp', pad: 'hold' } },
      { bars: 16, prog: 'a', drums: 'trot', crash: true, fill: 'west', p: { bass: 'a', twang: 'arp', pad: 'hold' } },
      { bars: 16, prog: 'c', drums: 'big', crash: true, fill: 'west', p: { bass: 'b', pad: 'hold', lead: 'whistle', twang: 'strum' } },
      { bars: 8, prog: 'b', drums: 'half', down: 2, swell: true, p: { pad: 'hold', twang: 'arp2' } },
      { bars: 16, prog: 'c', drums: 'big', crash: true, drop: true, fill: 'west', p: { bass: 'b', pad: 'hold', lead: 'whistle', twang: 'strum' } },
      { bars: 8, prog: 'a', drums: 'wind', lp: [16000, 700], p: { twang: 'arp', pad: 'hold' } },
    ],
  },

  {
    id: 'interstate', title: 'Interstate Nights', style: 'Night drive house', bpm: 122, gain: 1,
    pump: { depth: 0.6, release: 0.17 },
    kit: { kick: { s: 'kickHouse' }, clap: { s: 'clap', rev: 0.3 }, ohat: { g: 0.9 }, ride: { g: 0.8 } },
    prog: { a: 'Gm7 Gm7 Ebmaj7 F', c: 'Ebmaj7 F Dm7 Gm7', b: 'Cm7 Dm7 Ebmaj7 F' },
    drums: {
      intro: { kick: 'x...x...x...x...', hat: '..x...x...x...x.' },
      full: { kick: 'x...x...x...x...', clap: '....x.......x...', hat: 'xo.oxo.oxo.oxo.o', ohat: '..x...x...x...x.' },
      full2: { kick: 'x...x...x...x...', clap: '....x.......x...', hat: 'xo.oxo.oxo.oxo.o', ohat: '..x...x...x...x.', ride: 'x...x...x...x...', shaker: 'oxoxoxoxoxoxoxox' },
      brk: { hat: '..x...x...x...x.', clap: '....o.......o...' },
      build: { kick: 'x...x...x...x...', clap: '....x.......x...', hat: 'x.x.x.x.x.x.x.x.' },
    },
    parts: {
      bass: { type: 'bass', inst: P.pluckBass, lo: 43, ch: { pump: true }, pat: { a: '..r...r...r...r.', b: '..rr..o...rr..o.' } },
      stab: { type: 'chord', inst: P.stab, lo: 60, ch: { pump: true, rev: 0.3, dly: 0.2 }, pat: { s: 'x..x..x...x..x..' } },
      pad: { type: 'chord', inst: P.superPad, lo: 55, ch: { pump: true, rev: 0.5, hp: 180, level: 0.85 }, pat: { hold: 'x---------------' } },
      bell: {
        type: 'mel', inst: P.bell, res: 2, ch: { rev: 0.4, dly: 0.45, pan: 0.15 },
        pat: {
          hook: 'G5 _ Bb5 _ . D6 _ C6   _ _ A5 _ F5 _ . .   F5 _ A5 _ . C6 _ Bb5   _ _ G5 _ D5 _ . .',
          hook2: 'G5 _ _ _ Eb5 _ _ _   F5 _ _ _ D5 _ _ _   G5 _ Bb5 _ D6 _ C6 _   A5 _ _ _ . . . .',
        },
      },
      arp: { type: 'arp', inst: P.pluck, lo: 67, ch: { rev: 0.25, dly: 0.4, pan: -0.25, pump: true }, pat: { a: '0 1 2 3 0 1 2 3 0 1 2 3 0 1 2 3' } },
    },
    sections: [
      { bars: 16, prog: 'a', drums: 'intro', lp: [350, 16000], riser: 4, p: { bass: 'a' } },
      { bars: 16, prog: 'a', drums: 'full', crash: true, p: { bass: 'a', stab: 's', pad: 'hold' } },
      { bars: 16, prog: 'c', drums: 'full2', crash: true, fill: 'snare', p: { bass: 'b', stab: 's', pad: 'hold', bell: 'hook' } },
      { bars: 16, prog: 'b', drums: 'brk', down: 2, p: { pad: 'hold', bell: 'hook2' } },
      { bars: 8, prog: 'c', drums: 'build', riser: 8, swell: true, fill: 'roll', gap: 2, p: { pad: 'hold', stab: 's' } },
      { bars: 16, prog: 'c', drums: 'full2', crash: true, drop: true, p: { bass: 'b', stab: 's', pad: 'hold', bell: 'hook', arp: 'a' } },
      { bars: 8, prog: 'a', drums: 'intro', lp: [16000, 400], p: { bass: 'a' } },
    ],
  },

  {
    id: 'chrome-heart', title: 'Chrome Heart', style: 'Darksynth', bpm: 118, gain: 0.88,
    pump: { depth: 0.4, release: 0.15 },
    kit: { kick: { s: 'kickBoom', g: 0.95 }, snare: { s: 'snareGated', rev: 0.35 }, clap: { s: 'clapBig', g: 0.8, rev: 0.4 }, crash: { g: 1.1 } },
    prog: { a: 'Cm Cm Ab Ab Fm Fm G G', b: 'Cm Db Cm Bb', c: 'Ab Bb Cm Cm Ab Bb G G' },
    drums: {
      pulse: { kick: 'x...x...x...x...' },
      drive: { kick: 'x...x...x...x...', snare: '....X.......X...', hat: 'xoxoxoxoxoxoxoxo' },
      big: { kick: 'x...x...x...x...', snare: '....X.......X...', clap: '....x.......x...', hat: 'xoxoxoxoxoxoxoxo', ohat: '..x...x...x...x.' },
      half: { kick: 'x.........x.....', snare: '........X.......', hat: 'x.x.x.x.x.x.x.x.' },
      build: { kick: 'x...x...x...x...', hat: 'x.x.x.x.x.x.x.x.' },
    },
    parts: {
      bass: { type: 'bass', inst: P.darkBass, lo: 36, ch: { drive: 3.2, driveLp: 4200, pump: true, level: 0.19 }, pat: { drive: 'rrorrrorrrorrror', pulse: 'r.r.r.r.r.r.r.r.' } },
      pad: { type: 'chord', inst: P.darkPad, lo: 55, ch: { pump: true, rev: 0.55, hp: 200 }, pat: { hold: 'x---------------' } },
      stab: { type: 'chord', inst: P.hit, lo: 48, ch: { rev: 0.5 }, pat: { hits: 'X-......x-......' } },
      lead: {
        type: 'mel', inst: P.sawLead, res: 2, legato: true, ch: { rev: 0.4, dly: 0.3, drive: 1.4, level: 0.75 },
        pat: {
          hook: `C5 _ _ _ Eb5 _ _ _   D5 _ _ _ F5 _ _ _   G5 _ _ _ _ _ F5 Eb5   G5 _ _ _ C5 _ _ _
                 Ab5 _ _ _ G5 _ Eb5 _   F5 _ _ _ D5 _ Bb4 _   B4 _ _ _ D5 _ _ _   G5 _ _ _ F5 _ D5 _`,
        },
      },
      dark: { type: 'mel', inst: { ...P.brassLead, oct: -1, gain: 0.09 }, res: 4, legato: true, ch: { rev: 0.6, dly: 0.3 }, pat: { m: 'G4 _ _ _   Ab4 _ _ _   G4 _ Eb4 _   F4 _ _ _' } },
    },
    sections: [
      { bars: 8, prog: 'b', drums: 'pulse', lp: [300, 6000], swell: true, p: { bass: 'pulse', pad: 'hold' } },
      { bars: 16, prog: 'a', drums: 'drive', crash: true, fill: 'tom', p: { bass: 'drive', pad: 'hold' } },
      { bars: 16, prog: 'c', drums: 'big', crash: true, drop: true, fill: 'crash', p: { bass: 'drive', pad: 'hold', lead: 'hook', stab: 'hits' } },
      { bars: 8, prog: 'b', drums: 'half', down: 2, p: { pad: 'hold', dark: 'm' } },
      { bars: 8, prog: 'a', drums: 'build', riser: 8, swell: true, fill: 'roll', gap: 2, p: { bass: 'pulse', pad: 'hold' } },
      { bars: 16, prog: 'c', drums: 'big', crash: true, drop: true, fill: 'tom', p: { bass: 'drive', pad: 'hold', lead: 'hook', stab: 'hits' } },
      { bars: 8, prog: 'a', drums: 'drive', crash: true, p: { bass: 'drive', pad: 'hold', dark: 'm' } },
      { bars: 8, prog: 'b', drums: 'pulse', lp: [8000, 300], p: { bass: 'pulse', pad: 'hold' } },
    ],
  },

  {
    id: 'afterburner', title: 'Afterburner', style: 'Breakbeat acid', bpm: 128, gain: 1,
    delay: 0.75,
    kit: { kick: { s: 'kickPunch' }, snare: { s: 'snareCrisp', rev: 0.25 }, clap: { g: 0.8 } },
    prog: { a: 'Dm Dm Bb C', b: 'Gm Bb C A' },
    drums: {
      hats: { hat: 'x.x.x.x.x.x.x.x.', ohat: '..............x.' },
      brk: { kick: 'x.x.......x.....', snare: '....X..o.o..X...', hat: 'x.x.x.x.x.x.x.x.', ohat: '..............x.' },
      brk2: { kick: 'x.........x..x..', snare: '....X..o.o..X..o', hat: 'xoxoxoxoxoxoxoxo', clap: '............x...' },
      half: { kick: 'x...............', snare: '........x.......', hat: 'x.x.x.x.x.x.x.x.' },
      build: { kick: 'x...x...x...x...', snare: '....x.......x...', hat: 'x.x.x.x.x.x.x.x.' },
    },
    parts: {
      acid: { type: 'bass', inst: P.acid, lo: 38, ch: { drive: 2.2, driveLp: 6000, dly: 0.15, level: 0.55 }, pat: { a: 'r.or.rOr.fr~o.rRr.or.rOr.sr~o.fF', b: 'rRr.o.rr.f~rO.rs' } },
      bass: { type: 'bass', inst: P.sub, lo: 38, ch: {}, pat: { a: 'r-----r---r-----' } },
      stab: { type: 'chord', inst: P.stab, lo: 62, ch: { rev: 0.35, dly: 0.3 }, pat: { s: '....x.......x...' } },
      pad: { type: 'chord', inst: P.warmPad, lo: 57, ch: { rev: 0.5, hp: 200 }, pat: { hold: 'x---------------' } },
      lead: {
        type: 'mel', inst: P.sqLead, res: 2, legato: true, ch: { rev: 0.3, dly: 0.35 },
        pat: {
          hook: 'A5 _ _ _ F5 _ D5 _   E5 _ F5 _ E5 _ D5 _   D5 _ _ _ F5 _ Bb5 _   A5 _ _ _ G5 _ E5 _',
          hook2: 'G5 _ _ _ Bb5 _ D6 _   C6 _ Bb5 _ A5 _ F5 _   G5 _ _ _ E5 _ C5 _   C#5 _ _ _ E5 _ A5 _',
        },
      },
    },
    sections: [
      { bars: 8, prog: 'a', drums: 'hats', lp: [500, 9000], p: { acid: 'a' } },
      { bars: 16, prog: 'a', drums: 'brk', crash: true, fill: 'snare', p: { acid: 'a', bass: 'a' } },
      { bars: 16, prog: 'b', drums: 'brk2', fill: 'tom', p: { acid: 'b', bass: 'a', stab: 's' } },
      { bars: 8, prog: 'a', drums: 'half', down: 2, p: { pad: 'hold', acid: 'a' } },
      { bars: 8, prog: 'b', drums: 'build', riser: 8, swell: true, fill: 'roll', gap: 2, p: { acid: 'b', pad: 'hold' } },
      { bars: 16, prog: 'a', drums: 'brk', crash: true, drop: true, p: { acid: 'a', bass: 'a', stab: 's', lead: 'hook' } },
      { bars: 16, prog: 'b', drums: 'brk2', fill: 'snare', p: { acid: 'b', bass: 'a', stab: 's', lead: 'hook2' } },
      { bars: 8, prog: 'a', drums: 'hats', crash: true, lp: [9000, 400], p: { acid: 'a' } },
    ],
  },
];

// Each level's own track; the playlist then moves on through the others.
export const LEVEL_TRACK = {
  sierra: 'midnight-run', // sunset to midnight, mountains into the city
  coast: 'seabright', // dawn on the coast road
  streets: 'neon-rush', // a flat-out sprint through the neon grid
  desert: 'mirage', // golden hour to moonrise on Route 66
  cruise: 'interstate', // endless night freeway
};
export const PLAYLIST = ['midnight-run', 'interstate', 'chrome-heart', 'seabright', 'neon-rush', 'afterburner', 'mirage'];
