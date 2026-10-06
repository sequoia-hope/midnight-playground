// Engine Sound Lab presets: each engine is one parameter set for the model in
// worklet.js (see its header). Lengths are metres; sections are muffler and
// resonator resonances as [Hz, Q, dB]; events list the firing order as
// [bank, header] pairs (even firing unless `angles` gives crank degrees).
//
// gameCar: the closest car in the current game (src/game/Audio.js CARS), for
// the A/B button.

export const PRESETS = {
  crossV8: {
    name: 'Cross-plane V8 (American muscle)',
    gameCar: 'muscle',
    idle: 720, redline: 6400,
    // GM 1-8-4-3-6-5-7-2: L R R L R L L R. Two same-bank pulses in a row,
    // through cast manifolds of quite different lengths: the burble.
    events: [[0, 0], [1, 1], [1, 2], [0, 3], [1, 4], [0, 5], [0, 6], [1, 7]],
    bankGain: [1, 0.86],
    headers: [0.42, 0.66, 0.38, 0.74, 0.55, 0.82, 0.48, 0.62],
    headerR: -0.5, headerRv: 0.85, headerLossHz: 3000,
    imbalance: 0.12, tauDeg: 36, sharp: 1.3, variation: 0.14, jitterDeg: 2.6, misfire: 0.4, turbNoise: 0.3,
    pipes: 2, xpipe: 0.08, pipeLen: 2.7, pipeR: -0.55, pipeRc: 0.3, pipeLossHz: 2200,
    mufflerLp: 1300, sections: [[92, 2.5, 6], [185, 3, 3], [410, 4, -4], [760, 3, 2]],
    tailLen: 0.6, tailR: -0.5, radHz: 70, radLow: 0.45, body: 4,
    drive: 2.0, intake: 0.6, intakeLen: 0.4, mech: 0.6, pops: 1.4, popHz: 700,
    level: 1.25, width: 0.55,
    revUp: 9000, revDown: 4200,
  },
  flatV8: {
    name: 'Flat-plane V8 (Italian)',
    gameCar: 'sports',
    idle: 1000, redline: 9000,
    // Banks alternate evenly; equal 4-into-1 headers per bank: each bank is
    // an even-firing four, so it screams instead of burbling.
    events: [[0, 0], [1, 1], [0, 2], [1, 3], [0, 4], [1, 5], [0, 6], [1, 7]],
    headers: [0.72],
    headerR: -0.45, headerRv: 0.85, headerLossHz: 4500,
    imbalance: 0.15, tauDeg: 26, sharp: 1.8, variation: 0.07, jitterDeg: 1.2, misfire: 0.1, turbNoise: 0.35,
    pipes: 2, xpipe: 0, pipeLen: 2.0, pipeR: -0.5, pipeRc: 0.3, pipeLossHz: 3500,
    mufflerLp: 2600, sections: [[160, 3, 4], [520, 4, 3], [1300, 3, 4], [2600, 4, -3]],
    tailLen: 0.4, tailR: -0.45, radHz: 90, radLow: 0.35, body: 2,
    drive: 1.6, intake: 1.0, intakeLen: 0.3, mech: 0.6, pops: 0.9, popHz: 1100,
    level: 1.12, width: 0.5,
    revUp: 14000, revDown: 5500,
  },
  v10: {
    name: 'V10 (supercar)',
    gameCar: 'super',
    idle: 1000, redline: 8700,
    events: [[0, 0], [1, 1], [0, 2], [1, 3], [0, 4], [1, 5], [0, 6], [1, 7], [0, 8], [1, 9]],
    headers: [0.64],
    headerR: -0.45, headerRv: 0.85, headerLossHz: 5000,
    imbalance: 0.15, tauDeg: 24, sharp: 2.0, variation: 0.06, jitterDeg: 1.0, misfire: 0.1, turbNoise: 0.35,
    pipes: 2, xpipe: 0.1, pipeLen: 1.8, pipeR: -0.5, pipeRc: 0.3, pipeLossHz: 4000,
    mufflerLp: 3200, sections: [[230, 3, 4], [700, 4, 4], [1700, 3, 4]],
    tailLen: 0.35, tailR: -0.45, radHz: 100, radLow: 0.35, body: 1.5,
    drive: 1.5, intake: 1.2, intakeLen: 0.28, mech: 0.6, pops: 0.7, popHz: 1500,
    level: 1.12, width: 0.5,
    revUp: 15000, revDown: 6000,
  },
  v12: {
    name: 'V12 (60°)',
    gameCar: 'sports',
    idle: 950, redline: 8500,
    // Each bank is an even-firing straight six; banks alternate every 60°.
    events: [[0, 0], [1, 1], [0, 2], [1, 3], [0, 4], [1, 5], [0, 6], [1, 7], [0, 8], [1, 9], [0, 10], [1, 11]],
    headers: [0.76],
    headerR: -0.45, headerRv: 0.85, headerLossHz: 5000,
    imbalance: 0.14, tauDeg: 22, sharp: 2.1, variation: 0.05, jitterDeg: 0.8, misfire: 0.05, turbNoise: 0.3,
    pipes: 2, xpipe: 0.05, pipeLen: 2.2, pipeR: -0.5, pipeRc: 0.3, pipeLossHz: 4200,
    mufflerLp: 3600, sections: [[250, 3, 4], [900, 4, 4], [2100, 3, 4]],
    tailLen: 0.38, tailR: -0.45, radHz: 100, radLow: 0.35, body: 1.5,
    drive: 1.4, intake: 1.1, intakeLen: 0.3, mech: 0.5, pops: 0.6, popHz: 1500,
    level: 1.05, width: 0.5,
    revUp: 13000, revDown: 5500,
  },
  v6tt: {
    name: 'Twin-turbo V6 (3000GT VR-4 style)',
    gameCar: 'rally',
    idle: 800, redline: 7000,
    // 60° V6, 1-2-3-4-5-6, banks alternate every 120°. Short manifolds into
    // a turbo per bank, which smooths the pulses; one pipe after the turbos.
    events: [[0, 0], [1, 1], [0, 2], [1, 3], [0, 4], [1, 5]],
    headers: [0.34, 0.42, 0.38, 0.46, 0.36, 0.44],
    headerR: -0.45, headerRv: 0.8, headerLossHz: 3500,
    imbalance: 0.12, tauDeg: 30, sharp: 1.6, variation: 0.09, jitterDeg: 1.6, misfire: 0.2, turbNoise: 0.3,
    pipes: 1, xpipe: 0, pipeLen: 3.2, pipeR: -0.5, pipeRc: 0.3, pipeLossHz: 2600,
    mufflerLp: 1900, sections: [[130, 3, 4], [360, 4, 3], [900, 3, 2]],
    tailLen: 0.5, tailR: -0.45, radHz: 80, radLow: 0.4, body: 3,
    drive: 1.5, intake: 0.7, intakeLen: 0.45, mech: 0.6, pops: 0.6, popHz: 900,
    turbo: 2, bov: 1, level: 1.5, width: 0.4,
    revUp: 9500, revDown: 4500,
  },
  audiV8: {
    name: 'Audi 4.2 V8 (S4 / RS4 style)',
    gameCar: 'muscle',
    idle: 700, redline: 7600,
    // 1-5-4-8-6-3-7-2, cross-plane: A B A B B A B A. Equal, tidy headers and
    // an X-pipe: still a burble, but a smoother and higher one.
    events: [[0, 0], [1, 1], [0, 2], [1, 3], [1, 4], [0, 5], [1, 6], [0, 7]],
    headers: [0.6, 0.62, 0.58, 0.61, 0.6, 0.59, 0.62, 0.6],
    headerR: -0.48, headerRv: 0.85, headerLossHz: 3600,
    imbalance: 0.1, tauDeg: 30, sharp: 1.5, variation: 0.08, jitterDeg: 1.5, misfire: 0.15, turbNoise: 0.3,
    pipes: 2, xpipe: 0.25, pipeLen: 2.8, pipeR: -0.5, pipeRc: 0.3, pipeLossHz: 2800,
    mufflerLp: 2200, sections: [[118, 3, 4], [260, 4, 3], [600, 4, 3], [1500, 3, 1]],
    tailLen: 0.5, tailR: -0.45, radHz: 75, radLow: 0.42, body: 3,
    drive: 1.7, intake: 0.9, intakeLen: 0.38, mech: 0.6, pops: 0.9, popHz: 850,
    level: 1.25, width: 0.5,
    revUp: 11000, revDown: 5000,
  },
  i4turbo: {
    name: 'Turbo inline-4 (rally)',
    gameCar: 'rally',
    idle: 950, redline: 7500,
    // 1-3-4-2 into one 4-2-1 manifold and turbo. Anti-lag is a hint, not a
    // machine gun: the owner found the full rat-a-tat harsh, "tin cans", so
    // the pops are fewer, lower and darker (popHz 1200 -> 500).
    events: [[0, 0], [0, 1], [0, 2], [0, 3]],
    headers: [0.5, 0.52, 0.5, 0.52],
    headerR: -0.45, headerRv: 0.85, headerLossHz: 3500,
    imbalance: 0.15, tauDeg: 32, sharp: 1.7, variation: 0.1, jitterDeg: 1.8, misfire: 0.3, turbNoise: 0.45,
    pipes: 1, xpipe: 0, pipeLen: 2.4, pipeR: -0.5, pipeRc: 0.3, pipeLossHz: 3000,
    mufflerLp: 2600, sections: [[180, 3, 3], [540, 4, 4], [1400, 3, 3]],
    tailLen: 0.45, tailR: -0.45, radHz: 90, radLow: 0.38, body: 2,
    drive: 2.2, intake: 1.2, intakeLen: 0.3, mech: 0.7, pops: 0.5, popHz: 500,
    turbo: 1, bov: 1, antiLag: 0.2, level: 1.88, width: 0.3,
    revUp: 12000, revDown: 5500,
  },
};

export const ORDER = ['crossV8', 'flatV8', 'v10', 'v12', 'v6tt', 'audiV8', 'i4turbo'];

// Live tweak sliders: [key, label, min, max, step]. headerScale, headerSpread,
// mufflerTune and mufflerGain are multipliers on the preset's own numbers.
export const TWEAKS = [
  ['headerScale', 'Header length ×', 0.4, 2, 0.01],
  ['headerSpread', 'Header inequality ×', 0, 2.5, 0.01],
  ['pipeLen', 'Pipe length (m)', 0.5, 6, 0.05],
  ['tailLen', 'Tailpipe length (m)', 0.1, 1.5, 0.01],
  ['mufflerTune', 'Muffler resonance ×', 0.5, 2, 0.01],
  ['mufflerGain', 'Muffler resonance depth ×', 0, 2.5, 0.01],
  ['mufflerLp', 'Muffler low-pass (Hz)', 400, 8000, 10],
  ['xpipe', 'X-pipe crossover', 0, 0.5, 0.01],
  ['variation', 'Cycle variation', 0, 0.5, 0.005],
  ['imbalance', 'Cylinder imbalance', 0, 0.4, 0.005],
  ['jitterDeg', 'Timing jitter (°)', 0, 8, 0.1],
  ['tauDeg', 'Pulse width (°)', 10, 60, 0.5],
  ['sharp', 'Pulse sharpness', 0.6, 3, 0.05],
  ['drive', 'Drive / saturation', 1, 4, 0.05],
  ['body', 'Body low shelf (dB)', -6, 10, 0.5],
  ['radHz', 'Tailpipe radiation corner (Hz)', 20, 250, 1],
  ['intake', 'Intake', 0, 2.5, 0.01],
  ['mech', 'Mechanical', 0, 2.5, 0.01],
  ['pops', 'Overrun pops', 0, 3, 0.01],
  ['level', 'Output level', 0.05, 2, 0.01],
];

export const TWEAK_DEFAULTS = { headerScale: 1, headerSpread: 1, mufflerTune: 1, mufflerGain: 1 };

// A working copy of a preset with the multiplier fields filled in.
export function presetParams(key) {
  return { ...TWEAK_DEFAULTS, ...structuredClone(PRESETS[key]) };
}

// Boost the turbo would make at this rpm and throttle, once spooled.
export function boostTarget(p, rpm, thr) {
  if (!p.turbo) return 0;
  return thr * Math.max(0, Math.min(1, (rpm - 0.3 * p.redline) / (0.35 * p.redline)));
}
