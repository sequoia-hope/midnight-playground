// Recording the game's calls on its audio facade (GameAudio), and playing
// them back into a GameAudio on the recording Web Audio fake. Part of the
// audio reference (tools/parity/audio-ref.mjs; format in
// parity/golden/audio/README.md).

import { RecordingContext, ArrayStore, VirtualTimers, settle, mulberry32, enc, dec } from './webaudio-fake.mjs';

// ── In the page ───────────────────────────────────────────────────
// Runs before the game's scripts (page.evaluateOnNewDocument). Wraps every
// public method of the GameAudio the game publishes as window.__audio, and
// radioVoice.prefetch, and logs each call the game makes from outside the
// audio code, tagged with the race tick it happens in (the number of
// parity.onTick calls so far). Calls the audio code makes on itself (init
// calling setCar, radioLine calling radio, ...) are not logged: playing the
// log back makes them again. Self-contained: it is serialised into the page.
export function facadeRecorder() {
  const log = [];
  let tick = 0;
  window.__mpFacade = { log, get tick() { return tick; } };
  const num = (x) => (Object.is(x, -0) ? '-0' : Number.isFinite(x) ? String(x) : JSON.stringify(String(x)));
  const enc = (v) => {
    if (typeof v === 'number') return num(v);
    if (v === undefined) return '"$undefined"';
    if (v === null || typeof v === 'boolean') return String(v);
    if (typeof v === 'string') return JSON.stringify(v);
    if (Array.isArray(v)) return '[' + v.map(enc).join(',') + ']';
    return '{' + Object.keys(v).filter((k) => v[k] !== undefined).map((k) => JSON.stringify(k) + ':' + enc(v[k])).join(',') + '}';
  };
  // The caller's frame: the audio modules calling themselves don't count.
  const internal = () => {
    const lines = String(new Error().stack).split('\n');
    const caller = lines[3] || '';
    return /\/src\/game\/(Audio\.js|audio\/)/.test(caller);
  };
  const wrap = (obj, name, label) => {
    const fn = obj[name];
    obj[name] = function (...args) {
      if (!internal()) log.push('[' + [tick, JSON.stringify(label), ...args.map(enc)].join(',') + ']');
      return fn.apply(this, args);
    };
  };
  const hook = (prop, onSet) => {
    let v;
    Object.defineProperty(window, prop, {
      configurable: true,
      get: () => v,
      set: (x) => { v = x; onSet(x); },
    });
  };
  hook('__audio', (a) => {
    const proto = Object.getPrototypeOf(a);
    for (const [name, d] of Object.entries(Object.getOwnPropertyDescriptors(proto))) {
      if (name === 'constructor' || name.startsWith('_') || typeof d.value !== 'function') continue;
      wrap(a, name, name);
    }
    wrap(a.radioVoice, 'prefetch', 'radioVoice.prefetch');
  });
  hook('__parity', (p) => {
    const prev = p.onTick;
    p.onTick = (...args) => { prev?.(...args); tick++; };
  });
}

// ── Playing a facade log back ─────────────────────────────────────
// The clock: a call in tick k happens at k / 120 s; setTimeout callbacks
// (the music scheduler, gate tails, the radio's wait) run on that clock at
// their due times, before the tick that passes them. Every Math.random draw
// comes from one mulberry32 stream. The radio clips' bytes are read from
// audio/radio/ and "decoded" to buffers of the length Chrome decodes them to
// (radio-clips.json); the decode always beats the radio's 700 ms wait.
export const TICK = 120;

export async function replay(lines, { GameAudio, sampleRate = 48000, seed = 1, clips, readFile, store = new ArrayStore() }) {
  const clock = { now: 0 };
  const sink = [];
  const timers = new VirtualTimers(clock);
  const saved = { random: Math.random, window: globalThis.window, fetch: globalThis.fetch };
  let ctx = null;
  globalThis.window = {
    AudioContext: class extends RecordingContext {
      constructor(opts = {}) {
        super({ sampleRate, clock, sink, store, latencyHint: opts.latencyHint });
        ctx = this;
        this.decoder = (bytes) => {
          const info = clips.byHash(bytes);
          return info && { name: info.file, channels: info.channels, length: info.length };
        };
      }
    },
  };
  globalThis.fetch = async (url) => {
    const bytes = readFile(url);
    return bytes
      ? { ok: true, status: 200, json: async () => JSON.parse(bytes.toString('utf8')), arrayBuffer: async () => bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) }
      : { ok: false, status: 404, json: async () => null, arrayBuffer: async () => null };
  };
  Math.random = mulberry32(seed);
  timers.install();
  const audio = new GameAudio();
  const errors = [];
  let n = 0;
  try {
    for (const line of lines) {
      const [tick, method, ...args] = dec(line);
      await timers.advanceTo(tick / TICK, settle);
      sink.push('[' + [enc(clock.now), '"api"', JSON.stringify(method), ...args.map(enc)].join(',') + ']');
      const [obj, name] = method === 'radioVoice.prefetch' ? [audio.radioVoice, 'prefetch'] : [audio, method];
      try {
        const r = obj[name](...args);
        if (r && typeof r.then === 'function') r.catch((e) => errors.push(`${method}: ${e.stack || e}`));
      } catch (e) {
        errors.push(`line ${n + 1} ${method}: ${e.stack || e}`);
      }
      await settle();
      n++;
    }
  } finally {
    timers.uninstall();
    Math.random = saved.random;
    globalThis.window = saved.window;
    globalThis.fetch = saved.fetch;
  }
  return {
    sink, store, errors,
    problems: ctx ? ctx.problems : [],
    throws: ctx ? ctx.throws : [],
    mutated: ctx ? ctx.mutatedBuffers() : [],
    end: clock.now,
  };
}
