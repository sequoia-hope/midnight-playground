//! Scalar math on `libm`: the kernel that both the Rust and the JS reference
//! run use for every inexact `Math` function, the JavaScript-semantics helpers,
//! `mulberry32`, `hash2` and the noise functions (SPEC 4.2; port of
//! `src/util/math.js`).
//!
//! No `f64::sin` and friends anywhere downstream: they use the platform
//! library on native and would break bit-identical results.

#![forbid(unsafe_code)]

pub mod js;
pub mod kernel;

use core::f64::consts::PI;

// Small math helpers shared by every module. Kept dependency-free so the
// track and terrain builders can also run under plain Node for tooling.

pub fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

pub fn inv_lerp(a: f64, b: f64, v: f64) -> f64 {
    clamp((v - a) / (b - a), 0.0, 1.0)
}

pub fn smoothstep(a: f64, b: f64, v: f64) -> f64 {
    let t = inv_lerp(a, b, v);
    t * t * (3.0 - 2.0 * t)
}

/// Frame-rate independent exponential approach of `a` toward `b`.
pub fn damp(a: f64, b: f64, lambda: f64, dt: f64) -> f64 {
    lerp(a, b, 1.0 - kernel::exp(-lambda * dt))
}

pub fn wrap_angle(mut a: f64) -> f64 {
    while a > PI {
        a -= PI * 2.0;
    }
    while a < -PI {
        a += PI * 2.0;
    }
    a
}

pub const DEG: f64 = PI / 180.0;

/// Fastest you can go and still stop within `room` metres braking at `decel`.
pub fn stop_speed(room: f64, decel: f64) -> f64 {
    (2.0 * decel * js::max(0.0, room)).sqrt()
}

/// A source of uniform draws in [0, 1): what the JS passes around as an
/// `rng` function.
pub trait Rng {
    fn next_f64(&mut self) -> f64;
}

/// Deterministic PRNG so the world is identical on every load.
///
/// `mulberry32(seed)` in the JS returns a closure over its state `a`; here the
/// state is the struct, so it can be cloned, hashed and stored in the
/// simulation state. The seed is the JS's `seed >>> 0`: pass a double through
/// [`js::to_uint32`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Mulberry32 {
    pub a: u32,
}

impl Mulberry32 {
    pub fn new(seed: u32) -> Self {
        Mulberry32 { a: seed }
    }

    pub fn next_f64(&mut self) -> f64 {
        // The JS does this on int32 and uint32 values; every step is the same
        // 32 bits in u32 arithmetic (`t + imul(...)` is a double sum of two
        // int32s, which `^` brings back to 32 bits: a wrapping add).
        self.a = self.a.wrapping_add(0x6d2b79f5);
        let mut t = self.a;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        (t ^ (t >> 14)) as f64 / 4294967296.0
    }
}

impl Rng for Mulberry32 {
    fn next_f64(&mut self) -> f64 {
        Mulberry32::next_f64(self)
    }
}

pub fn rrange<R: Rng + ?Sized>(rng: &mut R, a: f64, b: f64) -> f64 {
    a + (b - a) * rng.next_f64()
}

pub fn rpick<'a, T, R: Rng + ?Sized>(rng: &mut R, arr: &'a [T]) -> &'a T {
    &arr[(rng.next_f64() * arr.len() as f64).floor() as usize]
}

/// Integer hash -> [0,1)
///
/// The arguments are numbers, as in the JS: each goes through ToInt32 the way
/// `Math.imul` takes it. The JS default seed is 0.
pub fn hash2(ix: f64, iy: f64, seed: f64) -> f64 {
    let mut h =
        js::imul(ix, 374761393.0) ^ js::imul(iy, 668265263.0) ^ js::imul(seed, 2147483647.0);
    h = (h ^ ((h as u32) >> 13) as i32).wrapping_mul(1274126177);
    h ^= ((h as u32) >> 16) as i32;
    (h as u32) as f64 / 4294967296.0
}

/// 2D simplex noise (Gustavson), seeded permutation.
///
/// `makeNoise2D(seed)` in the JS returns a closure over its permutation; the
/// struct holds the same table and [`Noise2D::noise`] is the closure. The JS
/// default seed is 1.
#[derive(Clone, Debug)]
pub struct Noise2D {
    perm: [u8; 512],
    f2: f64,
    g2: f64,
}

const GRAD: [[f64; 2]; 8] = [
    [1.0, 1.0],
    [-1.0, 1.0],
    [1.0, -1.0],
    [-1.0, -1.0],
    [1.0, 0.0],
    [-1.0, 0.0],
    [0.0, 1.0],
    [0.0, -1.0],
];

impl Noise2D {
    pub fn new(seed: u32) -> Self {
        let mut rng = Mulberry32::new(seed);
        let mut p = [0u8; 256];
        for (i, v) in p.iter_mut().enumerate() {
            *v = i as u8;
        }
        for i in (1..256).rev() {
            let j = (rng.next_f64() * (i + 1) as f64).floor() as usize;
            p.swap(i, j);
        }
        let mut perm = [0u8; 512];
        for (i, v) in perm.iter_mut().enumerate() {
            *v = p[i & 255];
        }
        Noise2D {
            perm,
            f2: 0.5 * (3.0f64.sqrt() - 1.0),
            g2: (3.0 - 3.0f64.sqrt()) / 6.0,
        }
    }

    pub fn noise(&self, xin: f64, yin: f64) -> f64 {
        let (f2, g2, perm) = (self.f2, self.g2, &self.perm);
        let s = (xin + yin) * f2;
        let i = (xin + s).floor();
        let j = (yin + s).floor();
        let t = (i + j) * g2;
        let x0 = xin - (i - t);
        let y0 = yin - (j - t);
        let i1 = if x0 > y0 { 1 } else { 0 };
        let j1 = if x0 > y0 { 0 } else { 1 };
        let x1 = x0 - i1 as f64 + g2;
        let y1 = y0 - j1 as f64 + g2;
        let x2 = x0 - 1.0 + 2.0 * g2;
        let y2 = y0 - 1.0 + 2.0 * g2;
        // `i & 255` in the JS takes ToInt32 of the double first.
        let ii = (js::to_int32(i) & 255) as usize;
        let jj = (js::to_int32(j) & 255) as usize;
        let mut n = 0.0;
        let mut t0 = 0.5 - x0 * x0 - y0 * y0;
        if t0 > 0.0 {
            let g = GRAD[(perm[ii + perm[jj] as usize] & 7) as usize];
            t0 *= t0;
            n += t0 * t0 * (g[0] * x0 + g[1] * y0);
        }
        let mut t1 = 0.5 - x1 * x1 - y1 * y1;
        if t1 > 0.0 {
            let g = GRAD[(perm[ii + i1 + perm[jj + j1] as usize] & 7) as usize];
            t1 *= t1;
            n += t1 * t1 * (g[0] * x1 + g[1] * y1);
        }
        let mut t2 = 0.5 - x2 * x2 - y2 * y2;
        if t2 > 0.0 {
            let g = GRAD[(perm[ii + 1 + perm[jj + 1] as usize] & 7) as usize];
            t2 *= t2;
            n += t2 * t2 * (g[0] * x2 + g[1] * y2);
        }
        70.0 * n // ~[-1,1]
    }
}

impl Default for Noise2D {
    fn default() -> Self {
        Noise2D::new(1)
    }
}

/// `fbm(noise, x, y, octaves)` with the JS defaults `lac = 2.0`, `gain = 0.5`.
pub fn fbm(noise: &Noise2D, x: f64, y: f64, octaves: u32) -> f64 {
    fbm_with(noise, x, y, octaves, 2.0, 0.5)
}

pub fn fbm_with(noise: &Noise2D, x: f64, y: f64, octaves: u32, lac: f64, gain: f64) -> f64 {
    let (mut amp, mut freq, mut sum, mut norm) = (1.0, 1.0, 0.0, 0.0);
    for _ in 0..octaves {
        sum += amp * noise.noise(x * freq, y * freq);
        norm += amp;
        amp *= gain;
        freq *= lac;
    }
    sum / norm
}

/// Ridged multifractal — sharp crests, good for rocky mountains. ~[0,1]
///
/// `ridged(noise, x, y, octaves)` with the JS defaults `lac = 2.0`,
/// `gain = 0.5`.
pub fn ridged(noise: &Noise2D, x: f64, y: f64, octaves: u32) -> f64 {
    ridged_with(noise, x, y, octaves, 2.0, 0.5)
}

pub fn ridged_with(noise: &Noise2D, x: f64, y: f64, octaves: u32, lac: f64, gain: f64) -> f64 {
    let (mut amp, mut freq, mut sum, mut prev) = (0.5, 1.0, 0.0, 1.0);
    for _ in 0..octaves {
        let mut n = 1.0 - noise.noise(x * freq, y * freq).abs();
        n *= n;
        sum += n * amp * prev;
        prev = n;
        freq *= lac;
        amp *= gain;
    }
    sum
}
