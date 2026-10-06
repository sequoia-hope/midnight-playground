//! The noise beds and the tunnel's impulse response (`Audio.js`
//! `_makeNoise`, `_buildTunnel`): the two places the audio build draws from
//! `Math.random`.
//!
//! The JS draws from the one global `Math.random`; the port takes the
//! audio's random stream (`mulberry32(1)` in every parity run, DECISIONS
//! D40) and draws from it in the same order: the noise beds first (one draw
//! per sample), then the tunnel (per channel, one draw per sample, then two
//! per early reflection), before any loop picks its start offset.

use mp_math::Rng;
use mp_math::kernel::exp;

/// The three 2 s mono noise beds (`this.noise`).
#[derive(Clone, Debug, PartialEq)]
pub struct NoiseBeds {
    pub white: Vec<f32>,
    pub pink: Vec<f32>,
    pub brown: Vec<f32>,
}

/// `_makeNoise()`'s sample data: white, pink (Paul Kellet's filter) and
/// brown noise, `floor(sampleRate * 2)` samples each.
pub fn make_noise(sample_rate: f64, rnd: &mut impl Rng) -> NoiseBeds {
    let len = (sample_rate * 2.0).floor() as usize;
    let mut w = vec![0f32; len];
    let mut p = vec![0f32; len];
    let mut b = vec![0f32; len];
    let (mut b0, mut b1, mut b2, mut b3, mut b4, mut b5, mut b6, mut last) =
        (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for i in 0..len {
        let r = rnd.next_f64() * 2.0 - 1.0;
        w[i] = r as f32;
        b0 = 0.99886 * b0 + r * 0.0555179;
        b1 = 0.99332 * b1 + r * 0.0750759;
        b2 = 0.969 * b2 + r * 0.153852;
        b3 = 0.8665 * b3 + r * 0.3104856;
        b4 = 0.55 * b4 + r * 0.5329522;
        b5 = -0.7616 * b5 - r * 0.016898;
        p[i] = ((b0 + b1 + b2 + b3 + b4 + b5 + b6 + r * 0.5362) * 0.11) as f32;
        b6 = r * 0.115926;
        last = (last + 0.02 * r) / 1.02;
        b[i] = (last * 3.5) as f32;
    }
    // Crossfade the loop seam so looping noise doesn't click.
    let fade = 2048;
    for d in [&mut w, &mut p, &mut b] {
        for i in 0..fade {
            let t = i as f64 / fade as f64;
            d[len - fade + i] = (d[len - fade + i] as f64 * (1.0 - t) + d[i] as f64 * t) as f32;
        }
    }
    NoiseBeds {
        white: w,
        pink: p,
        brown: b,
    }
}

/// `_buildTunnel()`'s impulse response: a concrete tube, dense early
/// reflections, ~1.3 s dark tail. Stereo, `floor(sampleRate * 1.4)` samples.
pub fn tunnel_ir(sample_rate: f64, rnd: &mut impl Rng) -> Vec<Vec<f32>> {
    let sr = sample_rate;
    let len = (sr * 1.4).floor() as usize;
    let mut ir = Vec::with_capacity(2);
    for _c in 0..2 {
        let mut d = vec![0f32; len];
        let mut lp = 0.0;
        for (i, v) in d.iter_mut().enumerate() {
            let t = i as f64 / sr;
            lp += 0.35 * ((rnd.next_f64() * 2.0 - 1.0) - lp); // darken
            *v = (lp * exp(-t * 5.2) * (if t < 0.008 { t / 0.008 } else { 1.0 })) as f32;
        }
        for k in 0..14 {
            let at = (sr * (0.006 + rnd.next_f64() * 0.07)).floor() as usize;
            let sign = if rnd.next_f64() < 0.5 { -1.0 } else { 1.0 };
            d[at] = (d[at] as f64 + sign * (0.5 - k as f64 * 0.025)) as f32;
        }
        ir.push(d);
    }
    ir
}
