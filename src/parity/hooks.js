// Parity hooks for the Rust port's reference runs (roadmap M0, SPEC 12).
// Imported first by main.js, so it runs before any other game module
// evaluates. Each hook changes nothing unless its URL parameter is given.
//
//   ?kernel=1   every inexact Math function from mr_math's kernel (wasm)
//   ?fixeddt=1  the race steps in fixed ticks of 1/120 s, ?ticks=N per
//               rendered frame (default 2), whatever the frame time
//   ?quant=1    the player's input is quantised as the Rust InputFrame is
//   ?seed=N     rivals, police and the pursuit draw from seeded streams
//   ?parity=1   all of the above (seed 1 unless ?seed is given)
//
// Tools reach the state through window.__parity (the `parity` object).

import { KERNEL_WASM, installKernel } from './kernel.js';
import { simStreams, quantiseInput } from './sim.js';

const params = new URLSearchParams(location.search);
const all = params.get('parity') === '1';
const on = (name) => all || params.get(name) === '1';

export const parity = {
  kernel: on('kernel'),
  // Seconds per tick and ticks per rendered frame, or null.
  fixed: on('fixeddt') ? { dt: 1 / 120, ticks: Math.max(1, Number(params.get('ticks') || 2)) } : null,
  quantise: on('quant') ? quantiseInput : null,
  seed: params.has('seed') ? Number(params.get('seed')) : all ? 1 : null,
  // Built per race from the seed (see streamsForRace).
  rngs: null,
  // A recorder sets this: called after every race.update in fixed-dt mode,
  // with the race and the input that tick used.
  onTick: null,
  // Every error the main loop caught (it carries on; a recorder must not).
  errors: [],
};

// Fresh streams for each race, so a restart replays the same draws.
export function streamsForRace() {
  parity.rngs = parity.seed === null ? null : simStreams(parity.seed);
  return parity.rngs;
}

if (parity.kernel) {
  // A synchronous fetch, because the kernel must be in place before the
  // modules that follow this one compute anything at load time.
  const xhr = new XMLHttpRequest();
  xhr.open('GET', KERNEL_WASM, false);
  xhr.overrideMimeType('text/plain; charset=x-user-defined');
  xhr.send();
  if (xhr.status !== 200) throw new Error(`parity kernel: HTTP ${xhr.status} for ${KERNEL_WASM}`);
  const text = xhr.responseText;
  const bytes = new Uint8Array(text.length);
  for (let i = 0; i < text.length; i++) bytes[i] = text.charCodeAt(i) & 0xff;
  installKernel(bytes);
}

if (parity.kernel || parity.fixed || parity.quantise || parity.seed !== null) window.__parity = parity;
