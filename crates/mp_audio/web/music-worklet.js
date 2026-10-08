// The radio node's processor in a browser: mp_music's station player (the
// lab's engine with the station schedule and the tuner) as its own small
// wasm (mp_music.wasm, no wasm-bindgen), run on the audio thread. The
// native backend runs the same player with the same calls in the same
// order (crates/mp_audio/src/wa/music.rs), so both render alike.
//
// The page compiles the wasm and hands the WebAssembly.Module over in
// processorOptions (a worklet scope cannot fetch); it is instantiated here,
// synchronously, once per node.

const BLOCK = 128;

class RadioProcessor extends AudioWorkletProcessor {
  // Read once per block (k-rate). `station` is an index in
  // mp_music::radio::STATIONS (-1: off); `wallDay` and `wallSec` are the
  // wall time at the moment of tuning (days since the Unix epoch, and
  // seconds into the day: an f32 cannot hold Unix seconds); `tune` is a
  // serial that re-syncs when it changes; `energy` the game's intensity.
  static get parameterDescriptors() {
    const k = (name, defaultValue, minValue, maxValue) =>
      ({ name, defaultValue, minValue, maxValue, automationRate: 'k-rate' });
    return [
      k('station', -1, -1, 63),
      k('wallDay', 0, 0, 1e6),
      k('wallSec', 0, 0, 86400),
      k('tune', 0, 0, 1e9),
      k('energy', 1, 0, 1),
    ];
  }

  constructor(options) {
    super();
    const { module, seed } = options.processorOptions;
    this.x = new WebAssembly.Instance(module, {}).exports;
    this.w = this.x.mpm_new(sampleRate, seed);
  }

  process(inputs, outputs, params) {
    const x = this.x;
    x.mpm_params(this.w, params.station[0], params.wallDay[0], params.wallSec[0], params.tune[0], params.energy[0]);
    const ptr = x.mpm_process(this.w);
    // A fresh view every block: the wasm's memory may have grown (and its
    // old buffer detached).
    const lr = new Float32Array(x.memory.buffer, ptr, 2 * BLOCK);
    const out = outputs[0];
    out[0].set(lr.subarray(0, BLOCK));
    if (out.length > 1) out[1].set(lr.subarray(BLOCK, 2 * BLOCK));
    // A source that keeps playing (silence while off) until the page lets
    // go of it.
    return true;
  }
}

registerProcessor('mp-radio', RadioProcessor);
