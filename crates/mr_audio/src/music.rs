//! `src/game/audio/Music.js`. So far the arrays `Music.build()` makes (roadmap
//! WP 5.2): the pulse wave of the square-ish leads and the shared hall
//! reverb's impulse response. The sequencer and instruments are WP 5.6.

use crate::engine::Fourier;
use mr_math::kernel::{exp, sin};
use std::f64::consts::PI;

/// Pulse waves for the square-ish leads (25 % duty: hollow and nasal).
/// Cosine terms: a sine-only series of the same magnitudes is a spiky, quiet
/// wave. 64 coefficients (`this.pulse`).
pub fn pulse_wave() -> Fourier {
    let hh = 64;
    let mut re = vec![0f32; hh];
    let im = vec![0f32; hh];
    for (h, r) in re.iter_mut().enumerate().skip(1) {
        let h = h as f64;
        *r = ((2.0 / (h * PI)) * sin(h * PI * 0.25)) as f32;
    }
    Fourier { real: re, imag: im }
}

/// Shared hall reverb: stereo, 2.6 s, darkening as it decays, with a short
/// pre-delay and a few early reflections (`this.reverb.buffer`). Its noise
/// is a fixed xorshift per channel, not `Math.random`.
pub fn hall_ir(sample_rate: f64) -> Vec<Vec<f32>> {
    let sr = sample_rate;
    let len = (sr * 2.6).floor() as usize;
    let mut ir = Vec::with_capacity(2);
    for c in 0..2 {
        let mut d = vec![0f32; len];
        let mut lp = 0.0;
        // The JS keeps `seed` as an int32 after the first `^=`; every step is
        // a 32-bit operation, so u32 with wrapping shifts is the same bits.
        let mut seed: u32 = if c != 0 { 0x9e3779b9 } else { 0x7f4a7c15 };
        let mut rand = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed as f64 / 4294967296.0) * 2.0 - 1.0
        };
        let pre = (0.018 * sr).floor() as usize;
        for i in pre..len {
            let t = (i - pre) as f64 / sr;
            let u = i as f64 / len as f64;
            let k = 0.75 - 0.6 * u; // one-pole low-pass that closes over the tail
            lp += k * (rand() - lp);
            d[i] = (lp * exp((-6.9 * t) / 2.3) * (if t < 0.03 { t / 0.03 } else { 1.0 })) as f32;
        }
        for k in 0..10 {
            let kf = k as f64;
            let at = pre
                + (sr * (0.005 + 0.06 * ((kf * 0.37 + c as f64 * 0.19) % 1.0))).floor() as usize;
            let sign = if k % 2 != 0 { -1.0 } else { 1.0 };
            d[at] = (d[at] as f64 + sign * 0.35 * (1.0 - kf / 12.0)) as f32;
        }
        ir.push(d);
    }
    ir
}
