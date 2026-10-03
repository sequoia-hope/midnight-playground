// Parity helpers for the simulation's reference runs (Rust port, roadmap
// WP 0.3; SPEC 4.3). Pure functions, used by the in-game hooks
// (src/parity/hooks.js) and by the Node tools in tools/parity/.

import { mulberry32 } from '../util/math.js';

// The named random streams of SPEC 4.3. In the live game rivals, police and
// the pursuit all draw from Math.random, which effects and audio share, so
// a run cannot be reproduced. Here each gets its own mulberry32, seeded from
// the race seed: stream k starts at (seed + 0x9E3779B9 * (k + 1)) as an
// unsigned 32-bit value. Traffic already has its own stream, seed 99; it is
// made here too only so its draws can be counted.
export const STREAMS = ['ai', 'police', 'pursuit', 'traffic'];
export const TRAFFIC_SEED = 99; // Traffic's own default
export function simStreams(seed) {
  const out = {};
  STREAMS.forEach((name, k) => {
    const rng = mulberry32(name === 'traffic' ? TRAFFIC_SEED : (seed + 0x9e3779b9 * (k + 1)) >>> 0);
    let draws = 0;
    const next = () => { draws++; return rng(); };
    // Counting draws lets a trace record where each stream is.
    Object.defineProperty(next, 'draws', { get: () => draws });
    out[name] = next;
  });
  return out;
}

// One player's controls for one tick as the Rust InputFrame carries them:
// steer as i16 (±32767), throttle and brake as u8, the rest as flags. The
// reference run feeds the simulation these values, so both sides integrate
// the same numbers. Other fields (lookBack, ...) are presentation and pass
// through unchanged.
export function quantiseInput(inp) {
  const c = (x, lo, hi) => (x < lo ? lo : x > hi ? hi : x);
  return {
    ...inp,
    steer: Math.round(c(inp.steer, -1, 1) * 32767) / 32767,
    throttle: Math.round(c(inp.throttle, 0, 1) * 255) / 255,
    brake: Math.round(c(inp.brake, 0, 1) * 255) / 255,
    handbrake: !!inp.handbrake,
    nitro: !!inp.nitro,
    analog: !!inp.analog,
  };
}

// Random controls for the fuzz runs (SPEC 4.6 test 5), from mulberry32(seed):
// every 30 ticks (a quarter second) a new set, held until the next. The
// draws, in order: steer, then throttle (a 70 % chance of a random amount),
// brake (20 %), handbrake (10 %), nitro (20 %). The Rust fuzz test replays
// exactly this sequence.
export function fuzzer(seed) {
  const rng = mulberry32(seed >>> 0);
  let k = 0, cur = null;
  return (inp) => {
    if (k++ % 30 === 0) {
      const steer = rng() * 2 - 1;
      const throttle = rng() < 0.7 ? rng() : 0;
      const brake = rng() < 0.2 ? rng() : 0;
      const handbrake = rng() < 0.1;
      const nitro = rng() < 0.2;
      cur = { steer, throttle, brake, handbrake, nitro };
    }
    return { ...inp, ...cur };
  };
}
