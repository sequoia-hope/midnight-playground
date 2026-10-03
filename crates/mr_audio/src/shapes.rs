//! Wave-shaper curves and the small periodic waves of `Audio.js`
//! (`distortionCurve`, `exhaustCurve`, `pulseWave`, `softSquareWave`).
//! Bit-identical to the JS arrays.

use crate::engine::Fourier;
use mr_math::js;
use mr_math::kernel::{cos, sin, tanh};
use std::f64::consts::PI;

/// `distortionCurve(amount, n = 1024)`: a symmetric tanh soft clip.
pub fn distortion_curve(amount: f64, n: usize) -> Vec<f32> {
    let mut c = vec![0f32; n];
    for (i, v) in c.iter_mut().enumerate() {
        let x = (i as f64 * 2.0) / (n as f64 - 1.0) - 1.0;
        *v = (tanh(amount * x) / tanh(amount)) as f32;
    }
    c
}

/// Asymmetric soft clip (`exhaustCurve(amount, n = 2048)`): exhaust pressure
/// pulses compress harder on the positive swing, which adds even harmonics
/// (warmth).
pub fn exhaust_curve(amount: f64, n: usize) -> Vec<f32> {
    let mut c = vec![0f32; n];
    for (i, v) in c.iter_mut().enumerate() {
        let x = (i as f64 * 2.0) / (n as f64 - 1.0) - 1.0;
        let y = if x >= 0.0 {
            tanh(amount * x)
        } else {
            tanh(amount * 0.6 * x) / 0.8
        };
        *v = (y / tanh(amount)) as f32;
    }
    c
}

/// `pulseWave`'s coefficients: a narrow unipolar pulse train (Hann-tapered
/// harmonics, so it doesn't ring), scaled to peak 1. PeriodicWaves carry no
/// DC, so the wave dips to `floor` between pulses; add -floor to whatever it
/// drives to sit the gaps at zero. The JS makes the wave with
/// `{ disableNormalization: true }`.
#[derive(Clone, Debug, PartialEq)]
pub struct Pulse {
    pub wave: Fourier,
    pub floor: f64,
}

/// `pulseWave(ctx, harmonics = 24)` without the context.
pub fn pulse_wave(harmonics: usize) -> Pulse {
    let mut real = vec![0f32; harmonics + 1];
    let imag = vec![0f32; harmonics + 1];
    for h in 1..=harmonics {
        // `Math.cos(...) ** 2`: the one `**` in the audio, which V8 computes
        // as `x * x` (DECISIONS D46).
        let c = cos((PI * h as f64) / (2.0 * (harmonics as f64 + 1.0)));
        real[h] = (c * c) as f32;
    }
    let (mut peak, mut floor) = (f64::NEG_INFINITY, f64::INFINITY);
    for i in 0..512 {
        let mut x = 0.0;
        for (h, &r) in real.iter().enumerate().skip(1) {
            x += r as f64 * cos((2.0 * PI * h as f64 * i as f64) / 512.0);
        }
        peak = js::max(peak, x);
        floor = js::min(floor, x);
    }
    for r in real.iter_mut().skip(1) {
        *r = (*r as f64 / peak) as f32;
    }
    Pulse {
        wave: Fourier { real, imag },
        floor: floor / peak,
    }
}

/// A square with softened edges (`softSquareWave(ctx, harmonics = 15)`,
/// sigma-smoothed odd harmonics): the hi-lo siren's two-tone switch without
/// the Gibbs overshoot in pitch.
pub fn soft_square_wave(harmonics: usize) -> Fourier {
    let real = vec![0f32; harmonics + 1];
    let mut imag = vec![0f32; harmonics + 1];
    let mut h = 1;
    while h <= harmonics {
        let x = (PI * h as f64) / (harmonics as f64 + 1.0);
        imag[h] = ((4.0 / (PI * h as f64)) * (sin(x) / x)) as f32;
        h += 2;
    }
    Fourier { real, imag }
}
