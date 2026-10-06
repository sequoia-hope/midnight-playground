// Runs worklet.js under Node for tests: just enough of the AudioWorklet
// global scope (sampleRate, AudioWorkletProcessor with a port,
// registerProcessor) to build the processors and pull blocks from them.

const processors = {};

export async function load(rate = 48000) {
  if (!globalThis.__labLoaded) {
    globalThis.sampleRate = rate;
    globalThis.AudioWorkletProcessor = class {
      constructor() { this.port = { onmessage: null, postMessage() {} }; }
    };
    globalThis.registerProcessor = (name, cls) => { processors[name] = cls; };
    await import('../worklet.js');
    globalThis.__labLoaded = true;
  }
  return processors;
}

// The page's messages, as the worklet receives them.
export function send(node, data) { node.port.onmessage({ data }); }

// A new engine processor for `params`, already running at `state`.
export async function engine(params, state = { rpm: 800, throttle: 0 }, seed = 12345) {
  const P = await load();
  return new P['engine-lab']({ processorOptions: { params, state, seed } });
}

// Renders `secs` of stereo audio in 128-sample blocks. `control(t)` is
// called once per 60 Hz frame (as the page's driver is) and may return a
// state message's fields.
export function render(node, secs, control = null, rate = 48000) {
  const n = Math.round(secs * rate / 128) * 128;
  const L = new Float32Array(n), R = new Float32Array(n);
  let nextFrame = 0;
  for (let i = 0; i < n; i += 128) {
    if (control && i >= nextFrame) {
      const s = control(i / rate);
      if (s) send(node, { type: 'state', ...s });
      nextFrame += rate / 60;
    }
    node.process([], [[L.subarray(i, i + 128), R.subarray(i, i + 128)]]);
  }
  return { L, R };
}
