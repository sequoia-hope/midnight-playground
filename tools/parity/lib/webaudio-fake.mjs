// A recording fake of the Web Audio API, for the audio reference (roadmap
// WP 0.7, SPEC 7.5). Every call the game makes on it becomes one line of a
// call log (format: parity/golden/audio/README.md), and every array it is
// handed (buffer contents, periodic-wave coefficients, wave-shaper curves)
// goes into an array store keyed by content hash.
//
// It is strict the way a browser is: what Chrome rejects with an exception
// (a second start(), stop() before start(), disconnecting a link that is not
// there, an exponential ramp to zero, a negative time) throws here too, so
// game code that catches it carries on as it would in Chrome. Each throw is
// also logged. Invalid enum values (an oscillator type 'saw') are ignored,
// as browsers do, and counted as problems.
//
// Time comes from a clock object ({ now }) the driver advances; the fake
// never reads a real clock. AudioParam.value reads evaluate the param's
// automation timeline at the current time (Web Audio 1.0 semantics,
// rounded to float32 as Chrome stores params).

import { createHash } from 'node:crypto';

const OSC_TYPES = new Set(['sine', 'square', 'sawtooth', 'triangle']);
const FILTER_TYPES = new Set(['lowpass', 'highpass', 'bandpass', 'lowshelf', 'highshelf', 'peaking', 'notch', 'allpass']);
const OVERSAMPLE = new Set(['none', '2x', '4x']);

// ── Numbers in the log ─────────────────────────────────────────────
// Shortest round-trip decimal (what JS prints), -0 kept, non-finite as
// strings: JSON.stringify would turn -0 into 0 and NaN into null.
export function num(x) {
  if (Object.is(x, -0)) return '-0';
  if (Number.isFinite(x)) return String(x);
  return JSON.stringify(String(x));
}
export function enc(v) {
  if (typeof v === 'number') return num(v);
  if (v === undefined) return '"$undefined"';
  if (v === null || typeof v === 'boolean') return String(v);
  if (typeof v === 'string') return JSON.stringify(v);
  if (Array.isArray(v)) return '[' + v.map(enc).join(',') + ']';
  if (ArrayBuffer.isView(v)) return '[' + Array.from(v, enc).join(',') + ']';
  return '{' + Object.keys(v).filter((k) => v[k] !== undefined).map((k) => JSON.stringify(k) + ':' + enc(v[k])).join(',') + '}';
}
// The inverse for what enc wrote (non-finite numbers come back as strings
// "NaN", "Infinity", "-Infinity", "-0", and "$undefined").
export function dec(text) {
  return JSON.parse(text, (k, v) => {
    if (v === 'NaN') return NaN;
    if (v === 'Infinity') return Infinity;
    if (v === '-Infinity') return -Infinity;
    if (v === '$undefined') return undefined;
    return v;
  });
}

// ── The array store ────────────────────────────────────────────────
// Float32 arrays by content: '#' + the first 16 hex digits of the sha256 of
// their little-endian bytes.
export class ArrayStore {
  constructor() { this.byHash = new Map(); }
  static hash(f32) {
    const bytes = Buffer.from(f32.buffer, f32.byteOffset, f32.byteLength);
    return '#' + createHash('sha256').update(bytes).digest('hex').slice(0, 16);
  }
  add(f32) {
    const a = f32 instanceof Float32Array ? f32 : Float32Array.from(f32);
    const h = ArrayStore.hash(a);
    if (!this.byHash.has(h)) this.byHash.set(h, new Float32Array(a));
    return h;
  }
}

class DOMError extends Error {
  constructor(name, msg) { super(msg); this.name = name; }
}

// ── AudioParam ─────────────────────────────────────────────────────
class FakeParam {
  constructor(ctx, owner, name, value) {
    this._ctx = ctx; this._id = owner._id + '.' + name;
    this._intrinsic = value;
    this._events = []; // { type, v, t, tc }, sorted by t (ties: in insertion order)
  }
  get value() {
    const v = Math.fround(this._at(this._ctx.currentTime));
    this._ctx._log('get', this._id, v);
    return v;
  }
  set value(v) {
    this._ctx._log('value', this._id, v);
    if (!Number.isFinite(v)) this._ctx._throw('TypeError', `${this._id}.value = ${v}`);
    this._intrinsic = v;
    // Web Audio 1.0: setting value is setValueAtTime(value, currentTime).
    this._insert({ type: 'set', v, t: this._ctx.currentTime });
  }
  _check(what, args, times) {
    for (const x of args) if (!Number.isFinite(x)) this._ctx._throw('TypeError', `${this._id}.${what}: non-finite ${x}`);
    for (const t of times) if (t < 0) this._ctx._throw('RangeError', `${this._id}.${what}: negative time ${t}`);
  }
  _insert(e) {
    let i = this._events.length;
    while (i > 0 && this._events[i - 1].t > e.t) i--;
    this._events.splice(i, 0, e);
  }
  setValueAtTime(v, t) {
    this._ctx._log('setValue', this._id, v, t);
    this._check('setValueAtTime', [v, t], [t]);
    this._insert({ type: 'set', v, t });
    return this;
  }
  linearRampToValueAtTime(v, t) {
    this._ctx._log('linRamp', this._id, v, t);
    this._check('linearRampToValueAtTime', [v, t], [t]);
    this._insert({ type: 'lin', v, t });
    return this;
  }
  exponentialRampToValueAtTime(v, t) {
    this._ctx._log('expRamp', this._id, v, t);
    this._check('exponentialRampToValueAtTime', [v, t], [t]);
    if (v === 0) this._ctx._throw('RangeError', `${this._id}.exponentialRampToValueAtTime to 0`);
    this._insert({ type: 'exp', v, t });
    return this;
  }
  setTargetAtTime(v, t, tc) {
    this._ctx._log('setTarget', this._id, v, t, tc);
    this._check('setTargetAtTime', [v, t, tc], [t]);
    if (tc < 0) this._ctx._throw('RangeError', `${this._id}.setTargetAtTime time constant ${tc}`);
    this._insert({ type: 'target', v, t, tc });
    return this;
  }
  cancelScheduledValues(t) {
    this._ctx._log('cancel', this._id, t);
    this._check('cancelScheduledValues', [t], [t]);
    this._events = this._events.filter((e) => e.t < t);
    return this;
  }
  // The automation value at time T. A ramp runs from the previous event's
  // time and value; a target curve runs from the value at its start until
  // the next event. (A ramp right after a target curve starts from the
  // target's start value, the spec's literal reading; the game never reads
  // a param in that state.)
  _at(T) {
    const ev = this._events;
    let v = this._intrinsic, t0 = 0;
    for (let i = 0; i < ev.length; i++) {
      const e = ev[i];
      if (e.t > T) {
        if (e.type === 'lin') return v + (e.v - v) * (T - t0) / (e.t - t0);
        if (e.type === 'exp') return v * e.v > 0 ? v * Math.pow(e.v / v, (T - t0) / (e.t - t0)) : v;
        return v;
      }
      if (e.type === 'target') {
        const next = ev[i + 1];
        const end = next && next.t <= T ? next.t : T;
        const start = v;
        v = e.v + (start - e.v) * Math.exp(-(end - e.t) / e.tc);
        if (end === T) return v;
        // Ramps after a target start from the target's start (see above).
        if (next.type === 'lin' || next.type === 'exp') v = start;
        t0 = e.t;
        continue;
      }
      v = e.v; t0 = e.t;
    }
    return v;
  }
  connect() { this._ctx._throw('TypeError', `${this._id}: an AudioParam has no connect`); }
}

// ── Nodes ──────────────────────────────────────────────────────────
class FakeNode {
  constructor(ctx, kind, args = []) {
    this._ctx = ctx;
    this._id = 'n' + ctx._nextNode++;
    this._kind = kind;
    this._links = []; // [destination id, output, input] strings
    ctx._log('new', this._id, kind, ...args);
  }
  _param(name, value) { this[name] = new FakeParam(this._ctx, this, name, value); }
  _attr(name, value, check) {
    let v = value;
    Object.defineProperty(this, name, {
      enumerable: true,
      get: () => v,
      set: (x) => {
        this._ctx._log('set', this._id, name, x);
        if (check && !check(x)) { this._ctx._bad(`${this._kind}.${name} = ${JSON.stringify(x)}`); return; }
        v = x;
      },
    });
  }
  connect(dest, output, input) {
    const extra = [output, input].filter((x) => x !== undefined);
    if (!dest || !(dest instanceof FakeNode || dest instanceof FakeParam)) {
      this._ctx._log('connect', this._id, String(dest), ...extra);
      this._ctx._throw('TypeError', `${this._id}.connect(${dest})`);
    }
    this._ctx._log('connect', this._id, dest._id, ...extra);
    const key = [dest._id, output ?? 0, input ?? 0].join('/');
    if (!this._links.includes(key)) this._links.push(key);
    return dest;
  }
  disconnect(dest) {
    if (dest === undefined) {
      this._ctx._log('disconnect', this._id);
      this._links = [];
      return;
    }
    this._ctx._log('disconnect', this._id, dest?._id ?? String(dest));
    const n = this._links.length;
    this._links = this._links.filter((k) => !k.startsWith(dest._id + '/'));
    if (this._links.length === n) this._ctx._throw('InvalidAccessError', `${this._id}.disconnect(${dest._id}): not connected`);
  }
}

class FakeScheduled extends FakeNode {
  constructor(ctx, kind) { super(ctx, kind); this._started = false; }
  start(...a) {
    this._ctx._log('start', this._id, ...a);
    for (const x of a) if (!(Number.isFinite(x) && x >= 0)) this._ctx._throw('RangeError', `${this._id}.start(${a.join(', ')})`);
    if (this._started) this._ctx._throw('InvalidStateError', `${this._id}.start() twice`);
    this._started = true;
    if (this.buffer) this._ctx._bufferUsed(this.buffer);
  }
  stop(...a) {
    this._ctx._log('stop', this._id, ...a);
    for (const x of a) if (!(Number.isFinite(x) && x >= 0)) this._ctx._throw('RangeError', `${this._id}.stop(${a.join(', ')})`);
    if (!this._started) this._ctx._throw('InvalidStateError', `${this._id}.stop() before start()`);
  }
}

class FakeOscillator extends FakeScheduled {
  constructor(ctx) {
    super(ctx, 'Oscillator');
    this._param('frequency', 440); this._param('detune', 0);
    this._type = 'sine';
  }
  get type() { return this._type; }
  set type(v) {
    this._ctx._log('set', this._id, 'type', v);
    // 'custom' comes only from setPeriodicWave; setting it is an error.
    if (v === 'custom') this._ctx._throw('InvalidStateError', `${this._id}.type = 'custom'`);
    if (OSC_TYPES.has(v)) this._type = v; else this._ctx._bad(`Oscillator.type = ${JSON.stringify(v)}`);
  }
  setPeriodicWave(w) {
    this._ctx._log('setPeriodicWave', this._id, w?._id ?? String(w));
    if (!(w instanceof FakePeriodicWave)) this._ctx._throw('TypeError', `${this._id}.setPeriodicWave(${w})`);
    this._type = 'custom';
  }
}

class FakeBufferSource extends FakeScheduled {
  constructor(ctx) {
    super(ctx, 'BufferSource');
    this._param('playbackRate', 1); this._param('detune', 0);
    let buf = null;
    Object.defineProperty(this, 'buffer', {
      enumerable: true,
      get: () => buf,
      set: (b) => {
        this._ctx._log('set', this._id, 'buffer', b?._id ?? null);
        if (buf && b) this._ctx._throw('InvalidStateError', `${this._id}.buffer set twice`);
        buf = b;
        if (b) this._ctx._bufferUsed(b);
      },
    });
    this._attr('loop', false);
    this._attr('loopStart', 0);
    this._attr('loopEnd', 0);
  }
}

class FakeBuffer {
  constructor(ctx, channels, length, sampleRate, id) {
    this._ctx = ctx;
    this._id = id;
    this.numberOfChannels = channels;
    this.length = length;
    this.sampleRate = sampleRate;
    this.duration = length / sampleRate;
    this._data = Array.from({ length: channels }, () => new Float32Array(length));
    this._hashes = null; // content hashes, taken when the buffer is first used
  }
  getChannelData(c) {
    if (!(c >= 0 && c < this.numberOfChannels)) throw new DOMError('IndexSizeError', `getChannelData(${c})`);
    return this._data[c];
  }
  copyToChannel(src, c, start = 0) { this.getChannelData(c).set(src, start); }
}

class FakePeriodicWave {
  constructor(id) { this._id = id; }
}

// ── The context ────────────────────────────────────────────────────
export class RecordingContext {
  // clock: { now } in seconds, advanced by the driver.
  // sink: an array that gets one string per call (null: don't log calls).
  // store: an ArrayStore for buffer, wave and curve contents.
  constructor({ sampleRate = 48000, clock = { now: 0 }, sink = [], store = new ArrayStore(), latencyHint } = {}) {
    this.sampleRate = sampleRate;
    this._clock = clock;
    this._sink = sink;
    this._store = store;
    this._nextNode = 0;
    this._nextBuffer = 1;
    this._nextWave = 1;
    this.problems = [];
    this.throws = [];
    this._buffers = [];
    this._state = 'suspended';
    this._log('context', { sampleRate, latencyHint });
    this.destination = new FakeNode(this, 'Destination');
  }
  get currentTime() { return this._clock.now; }
  get state() { return this._state; }

  _log(op, ...args) {
    if (!this._sink) return;
    this._sink.push('[' + [num(this._clock.now), JSON.stringify(op), ...args.map(enc)].join(',') + ']');
  }
  _bad(msg) { this.problems.push(msg); }
  // With a `sites` map: the game function that made a wave or set a curve
  // (names the arrays no property holds).
  _site(key) {
    if (!this.sites) return;
    const frames = String(new Error().stack).split('\n').slice(2);
    const f = frames.find((l) => !l.includes('webaudio-fake.mjs')) || '';
    this.sites.set(key, (f.match(/at (?:\w+\.)?(\w+) /) || [])[1] || '?');
  }
  _throw(name, msg) {
    this.throws.push(`${name}: ${msg}`);
    this._log('throw', name, msg);
    throw new DOMError(name, msg);
  }
  _bufferUsed(b) {
    if (b._hashes || !(b instanceof FakeBuffer)) return;
    if (b._decoded) { b._hashes = []; return; }
    b._hashes = b._data.map((d) => this._store.add(d));
    this._log('data', b._id, b._hashes);
  }
  // Buffers whose contents changed after they were first used (a reference
  // capture must not have any: the log names the contents by hash).
  mutatedBuffers() {
    return this._buffers.filter((b) => b._hashes && !b._decoded && b._data.some((d, c) => ArrayStore.hash(d) !== b._hashes[c])).map((b) => b._id);
  }

  createGain() { const n = new FakeNode(this, 'Gain'); n._param('gain', 1); return n; }
  createDelay(max = 1) { const n = new FakeNode(this, 'Delay', [max]); n._param('delayTime', 0); return n; }
  createStereoPanner() { const n = new FakeNode(this, 'StereoPanner'); n._param('pan', 0); return n; }
  createChannelMerger(inputs = 6) { return new FakeNode(this, 'ChannelMerger', [inputs]); }
  createDynamicsCompressor() {
    const n = new FakeNode(this, 'DynamicsCompressor');
    n._param('threshold', -24); n._param('knee', 30); n._param('ratio', 12); n._param('attack', 0.003); n._param('release', 0.25);
    return n;
  }
  createConvolver() {
    const n = new FakeNode(this, 'Convolver');
    let buf = null;
    Object.defineProperty(n, 'buffer', {
      enumerable: true,
      get: () => buf,
      set: (b) => { this._log('set', n._id, 'buffer', b?._id ?? null); buf = b; if (b) this._bufferUsed(b); },
    });
    n._attr('normalize', true);
    return n;
  }
  createWaveShaper() {
    const n = new FakeNode(this, 'WaveShaper');
    let curve = null;
    Object.defineProperty(n, 'curve', {
      enumerable: true,
      get: () => curve,
      set: (c) => {
        if (c && !(c instanceof Float32Array)) this._bad('WaveShaper.curve is not a Float32Array');
        const h = c ? this._store.add(c) : null;
        this._site(n._id + '.curve');
        this._log('set', n._id, 'curve', h);
        curve = c;
      },
    });
    n._attr('oversample', 'none', (v) => OVERSAMPLE.has(v));
    return n;
  }
  createBiquadFilter() {
    const n = new FakeNode(this, 'BiquadFilter');
    n._param('frequency', 350); n._param('detune', 0); n._param('Q', 1); n._param('gain', 0);
    n._attr('type', 'lowpass', (v) => FILTER_TYPES.has(v));
    return n;
  }
  createOscillator() { return new FakeOscillator(this); }
  createBufferSource() { return new FakeBufferSource(this); }
  createPeriodicWave(real, imag, opts = {}) {
    const id = 'w' + this._nextWave++;
    if (!real || !imag || real.length !== imag.length || real.length < 2) {
      this._log('wave', id, null, null, opts);
      this._throw('IndexSizeError', 'createPeriodicWave: arrays of different or too short lengths');
    }
    this._site(id);
    const re = Float32Array.from(real), im = Float32Array.from(imag);
    this._log('wave', id, this._store.add(re), this._store.add(im), opts);
    const w = new FakePeriodicWave(id);
    w._real = re; w._imag = im; w._opts = opts;
    return w;
  }
  createBuffer(channels, length, sampleRate) {
    const id = 'b' + this._nextBuffer++;
    this._log('buffer', id, channels, length, sampleRate);
    if (!(channels >= 1 && length >= 1 && sampleRate >= 3000)) this._throw('NotSupportedError', `createBuffer(${channels}, ${length}, ${sampleRate})`);
    const b = new FakeBuffer(this, channels, length, sampleRate, id);
    this._buffers.push(b);
    return b;
  }
  // A decoded clip: its length from a table (the driver's `decoded`
  // callback gets the bytes and returns { length, channels, name }).
  decodeAudioData(bytes) {
    const id = 'b' + this._nextBuffer++;
    const info = this.decoder ? this.decoder(bytes) : null;
    if (!info) {
      this._log('decode', id, null);
      return Promise.reject(new DOMError('EncodingError', 'decodeAudioData: unknown clip'));
    }
    this._log('decode', id, info.name, info.channels, info.length, this.sampleRate);
    const b = new FakeBuffer(this, info.channels, info.length, this.sampleRate, id);
    b._decoded = true;
    return Promise.resolve(b);
  }
  // State changes land a microtask later, as a browser's do (before the
  // promise settles).
  resume() {
    this._log('resume');
    return Promise.resolve().then(() => { if (this._state !== 'closed') this._state = 'running'; });
  }
  suspend() {
    this._log('suspend');
    return Promise.resolve().then(() => { if (this._state !== 'closed') this._state = 'suspended'; });
  }
  close() {
    this._log('close');
    return Promise.resolve().then(() => { this._state = 'closed'; });
  }
}

// ── Virtual timers ─────────────────────────────────────────────────
// setTimeout and friends on the fake clock: a callback runs when the driver
// advances the clock past its due time, with the clock set to that time.
// Due times are now + ms / 1000; ties run in the order they were set.
export class VirtualTimers {
  constructor(clock) {
    this.clock = clock;
    this.queue = []; // { id, due, seq, fn, args }
    this.seq = 0;
    this.saved = null;
  }
  install() {
    this.saved = { setTimeout: globalThis.setTimeout, clearTimeout: globalThis.clearTimeout, setInterval: globalThis.setInterval, clearInterval: globalThis.clearInterval };
    globalThis.setTimeout = (fn, ms = 0, ...args) => {
      const id = ++this.seq;
      this.queue.push({ id, due: this.clock.now + Math.max(0, Number(ms) || 0) / 1000, seq: id, fn, args });
      return id;
    };
    globalThis.clearTimeout = (id) => { this.queue = this.queue.filter((x) => x.id !== id); };
    globalThis.setInterval = () => { throw new Error('setInterval: not supported by the virtual timers'); };
    globalThis.clearInterval = globalThis.clearTimeout;
  }
  uninstall() { if (this.saved) Object.assign(globalThis, this.saved); this.saved = null; }
  // Run every timer due at or before t (each followed by `settle`), then
  // leave the clock at t.
  async advanceTo(t, settle) {
    for (;;) {
      let best = null;
      for (const x of this.queue) if (x.due <= t && (!best || x.due < best.due || (x.due === best.due && x.seq < best.seq))) best = x;
      if (!best) break;
      this.queue.splice(this.queue.indexOf(best), 1);
      if (best.due > this.clock.now) this.clock.now = best.due;
      best.fn(...best.args);
      await settle();
    }
    this.clock.now = t;
  }
}

// Let every pending promise reaction run (a macrotask turn: Node drains the
// microtask queue before it).
export const settle = () => new Promise((r) => setImmediate(r));

// mulberry32, as src/util/math.js and the audio code's rngFrom.
export function mulberry32(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
