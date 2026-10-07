// The exhaust node's processor in a browser: mp_exhaust's engine as its own
// small wasm (mp_exhaust.wasm, no wasm-bindgen), run on the audio thread.
// The native backend runs the same engine with the same calls in the same
// order (crates/mp_audio/src/wa/exhaust.rs), so both render alike.
//
// The page compiles the wasm and hands the WebAssembly.Module over in
// processorOptions (a worklet scope cannot fetch); it is instantiated here,
// synchronously, once per node.

const BLOCK = 128;

class ExhaustProcessor extends AudioWorkletProcessor {
  // Read once per block (k-rate): rpm, throttle, boost and speed are the
  // engine's targets (it glides to them itself), running fades it in or
  // out, and preset (an index in mp_exhaust::ORDER) rebuilds it for another
  // car when it changes.
  static get parameterDescriptors() {
    const k = (name, defaultValue, minValue, maxValue) =>
      ({ name, defaultValue, minValue, maxValue, automationRate: 'k-rate' });
    const F32_MAX = 3.4028234663852886e38;
    return [
      k('rpm', 800, 0, 30000),
      k('throttle', 0, 0, 1),
      k('boost', 0, 0, 1),
      k('speed', 0, -F32_MAX, F32_MAX),
      k('running', 1, 0, 1),
      k('preset', 0, 0, 7),
    ];
  }

  constructor(options) {
    super();
    const { module, preset, seed } = options.processorOptions;
    this.x = new WebAssembly.Instance(module, {}).exports;
    this.seed = seed;
    this.preset = preset;
    this.w = this.x.mpx_new(preset, sampleRate, seed);
  }

  process(inputs, outputs, params) {
    const x = this.x;
    const rpm = params.rpm[0], throttle = params.throttle[0];
    const boost = params.boost[0], speed = params.speed[0];
    const preset = Math.round(params.preset[0]);
    if (preset !== this.preset) {
      // Another car: a new engine, already at the current state.
      x.mpx_free(this.w);
      this.preset = preset;
      this.w = x.mpx_new(preset, sampleRate, this.seed);
      x.mpx_jump(this.w, rpm, throttle, boost, speed);
    }
    x.mpx_state(this.w, rpm, throttle, boost, speed);
    x.mpx_run(this.w, params.running[0] >= 0.5 ? 1 : 0);
    const ptr = x.mpx_process(this.w);
    // A fresh view every block: the wasm's memory may have grown (and its
    // old buffer detached).
    const lr = new Float32Array(x.memory.buffer, ptr, 2 * BLOCK);
    const out = outputs[0];
    out[0].set(lr.subarray(0, BLOCK));
    if (out.length > 1) out[1].set(lr.subarray(BLOCK, 2 * BLOCK));
    // A source that keeps playing (silence once faded out) until the page
    // lets go of it.
    return true;
  }
}

registerProcessor('mp-exhaust', ExhaustProcessor);
