// game/audio: the soundtrack. Every song in tracks.js compiles and hangs
// together (sections name real progressions, drum patterns, parts and
// patterns), each level has its own track, and the playlist holds them
// all. Then each song is played start to finish on a fake AudioContext
// that checks what a browser would reject or ignore: oscillator and filter
// types (an invalid type is silently ignored, so a 'saw' synth plays as a
// sine), negative or non-finite times, exponential ramps to zero, and
// drum hits with no sample.

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { Music, noteToMidi, compileTrack } from '../../src/game/audio/Music.js';
import { TRACKS, PLAYLIST, LEVEL_TRACK } from '../../src/game/audio/tracks.js';
import { LEVELS } from '../../src/levels/index.js';
import fs from 'node:fs';

// ── A strict fake of the Web Audio API ────────────────────────────
const OSC_TYPES = new Set(['sine', 'square', 'sawtooth', 'triangle']);
const FILTER_TYPES = new Set(['lowpass', 'highpass', 'bandpass', 'lowshelf', 'highshelf', 'peaking', 'notch', 'allpass']);

class FakeContext {
  constructor({ sampleRate = 22050 } = {}) {
    this.sampleRate = sampleRate;
    this.currentTime = 0;
    this.problems = [];
    this.oscTypes = []; // every oscillator type the code asked for
    this.destination = this.node();
  }
  bad(msg) { if (!this.problems.includes(msg)) this.problems.push(msg); }
  param(v = 0) {
    const ctx = this;
    const time = (t, what) => { if (!(Number.isFinite(t) && t >= 0)) ctx.bad(`${what} at time ${t}`); };
    const val = (v, what) => { if (!Number.isFinite(v)) ctx.bad(`${what} to ${v}`); };
    return {
      value: v,
      setValueAtTime(v, t) { val(v, 'setValueAtTime'); time(t, 'setValueAtTime'); return this; },
      linearRampToValueAtTime(v, t) { val(v, 'linearRamp'); time(t, 'linearRamp'); return this; },
      exponentialRampToValueAtTime(v, t) { val(v, 'exponentialRamp'); time(t, 'exponentialRamp'); if (v === 0) ctx.bad('exponentialRamp to 0 (a RangeError in browsers)'); return this; },
      setTargetAtTime(v, t, tc) { val(v, 'setTarget'); time(t, 'setTarget'); if (!(tc >= 0)) ctx.bad(`setTarget time constant ${tc}`); return this; },
      cancelScheduledValues(t) { time(t, 'cancelScheduledValues'); return this; },
    };
  }
  // (Copies property descriptors: Object.assign and spread would turn the
  // type getters and setters into plain values.)
  node(extra = {}) {
    const ctx = this;
    const n = {
      connect(d) { if (!d) ctx.bad('connect(undefined)'); return d; },
      disconnect() {},
    };
    return Object.defineProperties(n, Object.getOwnPropertyDescriptors(extra));
  }
  scheduled(extra) {
    const ctx = this;
    const n = this.node({
      start(t = 0, offset = 0) { if (!(t >= 0 && offset >= 0)) ctx.bad(`start(${t}, ${offset})`); },
      stop(t = 0) { if (!(t >= 0)) ctx.bad(`stop(${t})`); },
    });
    return Object.defineProperties(n, Object.getOwnPropertyDescriptors(extra));
  }
  createGain() { return this.node({ gain: this.param(1) }); }
  createDelay() { return this.node({ delayTime: this.param(0) }); }
  createStereoPanner() { return this.node({ pan: this.param(0) }); }
  createChannelMerger() { return this.node(); }
  createDynamicsCompressor() { return this.node({ threshold: this.param(), knee: this.param(), ratio: this.param(), attack: this.param(), release: this.param() }); }
  createConvolver() { return this.node({ buffer: null }); }
  createWaveShaper() {
    const ctx = this;
    let os = 'none';
    return this.node({ curve: null, get oversample() { return os; }, set oversample(v) { if (!['none', '2x', '4x'].includes(v)) ctx.bad(`oversample '${v}'`); else os = v; } });
  }
  createBiquadFilter() {
    const ctx = this;
    let type = 'lowpass';
    return this.node({
      frequency: this.param(350), Q: this.param(1), gain: this.param(0), detune: this.param(0),
      get type() { return type; },
      set type(v) { if (FILTER_TYPES.has(v)) type = v; else ctx.bad(`BiquadFilter type '${v}'`); },
    });
  }
  createOscillator() {
    const ctx = this;
    let type = 'sine';
    return this.scheduled({
      frequency: this.param(440), detune: this.param(0),
      get type() { return type; },
      // Browsers ignore an invalid type (with a console warning) and keep the old one.
      set type(v) { ctx.oscTypes.push(v); if (OSC_TYPES.has(v)) type = v; else ctx.bad(`OscillatorNode type '${v}'`); },
      setPeriodicWave(w) { if (!w) ctx.bad('setPeriodicWave(undefined)'); type = 'custom'; ctx.oscTypes.push('custom'); },
    });
  }
  createBufferSource() {
    return this.scheduled({ buffer: null, loop: false, playbackRate: this.param(1) });
  }
  createPeriodicWave(re, im) {
    if (re.length !== im.length) this.bad('createPeriodicWave: lengths differ');
    return { periodic: true };
  }
  createBuffer(channels, length, sampleRate) {
    const data = Array.from({ length: channels }, () => new Float32Array(length));
    return {
      numberOfChannels: channels, length, sampleRate, duration: length / sampleRate,
      getChannelData: (c) => data[c],
      copyToChannel: (src, c) => data[c].set(src),
    };
  }
}

// Music treats an OfflineAudioContext as "render as fast as you like"
// (no timers), which is what the fake is.
let savedOffline;
before(() => { savedOffline = globalThis.OfflineAudioContext; globalThis.OfflineAudioContext = FakeContext; });
after(() => { if (savedOffline === undefined) delete globalThis.OfflineAudioContext; else globalThis.OfflineAudioContext = savedOffline; });

function makeMusic() {
  const ctx = new FakeContext();
  const music = new Music(ctx, ctx.createGain());
  music.build();
  return { ctx, music };
}

// ── Notes ──────────────────────────────────────────────────────────
test('noteToMidi: scientific pitch to MIDI numbers', () => {
  assert.equal(noteToMidi('C4'), 60);
  assert.equal(noteToMidi('A4'), 69);
  assert.equal(noteToMidi('C#5'), 73);
  assert.equal(noteToMidi('Db5'), 73);
  assert.equal(noteToMidi('Bb3'), 58);
  assert.equal(noteToMidi('G#5'), 80);
  assert.equal(noteToMidi('E2'), 40);
  assert.equal(noteToMidi('C-1'), 0);
  for (const bad of ['H4', 'C', '4', 'c4', 'C##4', '', 'C44']) assert.equal(noteToMidi(bad), null, bad);
});

// The octave number belongs to the letter: Cb4 is the B just below C4, and
// B#3 is C4. (No song spells a note this way yet.)
test('noteToMidi: flats and sharps across the octave line', () => {
  assert.equal(noteToMidi('Cb4'), 59);
  assert.equal(noteToMidi('B#3'), 60);
});

// ── The songs ─────────────────────────────────────────────────────
test('the song list, the playlist and each level\'s track agree', () => {
  const ids = TRACKS.map((t) => t.id);
  assert.equal(new Set(ids).size, ids.length, 'unique ids');
  assert.deepEqual([...PLAYLIST].sort(), [...ids].sort(), 'the playlist holds every song once');
  for (const l of LEVELS) {
    assert.ok(LEVEL_TRACK[l.id], `${l.id} has its own track`);
    assert.ok(ids.includes(LEVEL_TRACK[l.id]), `${l.id}'s track ${LEVEL_TRACK[l.id]} exists`);
    assert.equal(Music.levelTrack(l.id), LEVEL_TRACK[l.id]);
  }
  assert.equal(Music.levelTrack('nope'), PLAYLIST[0], 'unknown level: the first song');
  // README, controls: "T  Next music track. There are seven; …"
  const readme = fs.readFileSync(new URL('../../README.md', import.meta.url), 'utf8');
  const words = { five: 5, six: 6, seven: 7, eight: 8, nine: 9 };
  const m = /Next music track\. There are (\w+)/.exec(readme);
  assert.ok(m, 'README says how many tracks');
  assert.equal(TRACKS.length, words[m[1]], `README says ${m[1]}`);
  // What the menu's Track picker shows.
  assert.deepEqual(Music.tracks.map((t) => Object.keys(t).sort()), TRACKS.map(() => ['bpm', 'id', 'style', 'title']));
});

for (const T of TRACKS) {
  test(`${T.id}: compiles, and its sections name real parts and patterns`, () => {
    assert.ok(T.title && T.style, 'title and style');
    assert.ok(T.bpm >= 60 && T.bpm <= 200, `bpm ${T.bpm}`);
    const c = compileTrack(T);
    assert.equal(compileTrack(T), c, 'compiled once and cached');
    for (const [k, bars] of Object.entries(c.prog)) {
      for (const bar of bars) for (const ch of bar) assert.match(ch.name, /^[A-G][#b]?[a-z0-9]*(\/[A-G][#b]?)?$/, `chord '${ch.name}' in prog ${k}`);
    }
    for (const [k, lanes] of Object.entries(c.drums)) {
      for (const L of lanes) {
        assert.ok(L.steps.length % 16 === 0, `drums ${k}.${L.voice}: whole bars (${L.steps.length} steps)`);
        for (const ch of L.steps) assert.ok('Xxo.'.includes(ch), `drums ${k}.${L.voice}: step '${ch}'`);
      }
    }
    assert.ok(T.sections.length > 0);
    for (const [i, sec] of T.sections.entries()) {
      const where = `section ${i}`;
      assert.ok(Number.isInteger(sec.bars) && sec.bars > 0, `${where}: bars`);
      assert.ok(c.prog[sec.prog || 'a'], `${where}: progression '${sec.prog || 'a'}'`);
      if (sec.drums) assert.ok(c.drums[sec.drums], `${where}: drum pattern '${sec.drums}'`);
      for (const [part, key] of Object.entries(sec.p || {})) {
        assert.ok(c.parts[part], `${where}: part '${part}'`);
        if (key) assert.ok(c.parts[part].pats[key], `${where}: pattern '${part}.${key}'`);
      }
      if (sec.riser) assert.ok(sec.riser <= sec.bars, `${where}: riser fits`);
      if (sec.gap) assert.ok(sec.gap > 0 && sec.gap < 16, `${where}: gap`);
    }
  });
}

// ── The instruments ───────────────────────────────────────────────
// tracks.js documents patch types saw|square|tri|sine|pulse|fm.
test('every patch plays a valid oscillator type ("saw" is a sawtooth)', () => {
  const { ctx, music } = makeMusic();
  const EXPECT = { saw: 'sawtooth', tri: 'triangle', square: 'square', sine: 'sine', pulse: 'custom' };
  const patches = new Set();
  for (const T of TRACKS) for (const p of Object.values(T.parts)) patches.add(p.inst);
  let n = 0;
  for (const P of patches) {
    if (P.type === 'fm') continue; // FM: sine carriers and modulators, no type set
    assert.ok(EXPECT[P.type], `known patch type '${P.type}'`);
    ctx.oscTypes.length = 0;
    music.note(ctx.createGain(), 1, [60], 0.5, P);
    // One oscillator per voice, then the sub-oscillator if the patch has one.
    const voices = P.voices ?? 1;
    assert.deepEqual(ctx.oscTypes, [...Array(voices).fill(EXPECT[P.type]), ...(P.sub ? [P.subType || 'sine'] : [])], `a '${P.type}' patch`);
    n++;
  }
  assert.ok(n > 10, `${n} patches checked`);
  assert.deepEqual(ctx.problems, []);
});

for (const T of TRACKS) {
  test(`${T.id}: plays start to finish, every part sounds, and the playlist moves on`, () => {
    const { ctx, music } = makeMusic();
    const started = [];
    music.onTrack = (info) => started.push(info.id);
    const played = new Map(); // section index → Set of parts that played notes
    const part = music._part.bind(music);
    music._part = (S, name, ...rest) => {
      if (S.T === T) {
        const sec = music.pos.sec;
        if (!played.has(sec)) played.set(sec, new Set());
        played.get(sec).add(name);
      }
      return part(S, name, ...rest);
    };
    const missing = new Set();
    const voice = music._drumVoice.bind(music);
    music._drumVoice = (S, name) => { const v = voice(S, name); if (!v.buf) missing.add(name); return v; };

    music.play(T.id);
    music.start();
    assert.equal(music.current.id, T.id);
    const bars = T.sections.reduce((a, s) => a + s.bars, 0);
    const length = (bars * 16 * 60) / T.bpm / 4;
    for (let t = 0.5; t < length + 3; t += 0.5) {
      ctx.currentTime = t;
      music.pumpUntil(t + 0.2);
    }
    assert.deepEqual(ctx.problems, [], 'nothing a browser would reject');
    assert.deepEqual([...missing], [], 'every drum hit has a sample');
    for (const [i, sec] of T.sections.entries()) {
      for (const [name, key] of Object.entries(sec.p || {})) {
        if (key) assert.ok(played.get(i)?.has(name), `section ${i}: '${name}' (${key}) played`);
      }
    }
    // At the end the next song in the playlist starts.
    const next = PLAYLIST[(PLAYLIST.indexOf(T.id) + 1) % PLAYLIST.length];
    assert.deepEqual(started, [T.id, next]);
    assert.equal(music.current.id, next);
  });
}

test('next() walks the playlist, and play() keeps a song that is already on', () => {
  const { music } = makeMusic();
  music.play(PLAYLIST[0]);
  assert.equal(music.info.id, PLAYLIST[0], 'queued before music starts');
  assert.equal(music.next().id, PLAYLIST[1], 'next while stopped');
  music.start();
  assert.equal(music.current.id, PLAYLIST[1]);
  const song = music.song;
  music.play(PLAYLIST[1]);
  assert.equal(music.song, song, 'the same song carries on (a restart doesn\'t restart it)');
  let id = PLAYLIST[1];
  for (let i = 0; i < PLAYLIST.length; i++) id = music.next().id;
  assert.equal(id, PLAYLIST[1], 'wraps round');
  music.stop();
  assert.equal(music.on, false);
});
