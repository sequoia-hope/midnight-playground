//! The lab's DSP building blocks (`tools/music-lab/dsp.js`), per sample,
//! line for line. Every struct takes plain numbers and keeps its own state.
//!
//! What is here, and why it sounds less "browser" than OscillatorNode +
//! BiquadFilterNode:
//! - Oscillators are PolyBLEP band-limited and carry their own slow random
//!   drift, so two voices are never exactly in tune (analogue character).
//! - Filters are zero-delay-feedback (topology-preserving transform)
//!   designs: a 4-pole ladder with saturation in the loop (Moog / Juno / 303
//!   family) and a state-variable filter. Resonance stays stable when the
//!   cutoff is swept fast, and pushing them saturates instead of clipping.
//! - The reverb is a feedback delay network with damping and slow
//!   modulation, the delay a tempo-synced ping-pong with a darkening
//!   feedback path, the bus has a glue compressor and tape saturation.
//!
//! The JS reads a module-level sample rate `SR`. There is no global here:
//! a struct that needs the rate after construction stores it (`sr`), and the
//! oscillators and the two filters whose `set` the JS calls with a rate
//! take it as an argument, so the arithmetic is the JS's to the operation.

use crate::{imul, to_i32};
use std::sync::OnceLock;

pub const TAU: f64 = std::f64::consts::PI * 2.0;

pub fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

pub fn mtof(m: f64) -> f64 {
    440.0 * 2f64.powf((m - 69.0) / 12.0)
}

pub fn db_to_gain(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// A cheap tanh (Padé 3/2, clamped): smooth, odd, and 1 at the rails. Not
/// the real `tanh`: the instruments mean this one.
pub fn tanh(x: f64) -> f64 {
    if x < -3.0 {
        return -1.0;
    }
    if x > 3.0 {
        return 1.0;
    }
    let x2 = x * x;
    (x * (27.0 + x2)) / (27.0 + 9.0 * x2)
}

const SIN_N: usize = 4096;

fn sin_table() -> &'static [f64] {
    static SIN: OnceLock<Vec<f64>> = OnceLock::new();
    SIN.get_or_init(|| {
        (0..=SIN_N)
            .map(|i| ((TAU * i as f64) / SIN_N as f64).sin())
            .collect()
    })
}

/// sin(2π·x) from a 4096-point table with linear interpolation (≈ -100 dB
/// error): the FM operators call it millions of times a second. (The index
/// is clamped to the table: `x - floor(x)` can round to exactly 1 for a
/// negative `x` below the last bit, where the JS reads past the end.)
pub fn sin1(x: f64) -> f64 {
    let sin = sin_table();
    let x = x - x.floor();
    let f = x * SIN_N as f64;
    let i = (f as usize).min(SIN_N - 1);
    sin[i] + (sin[i + 1] - sin[i]) * (f - i as f64)
}

/// A seeded generator (mulberry32), so a render is the same every time.
/// The JS `rng(seed)` closure; the seed is `seed >>> 0`.
#[derive(Clone, Copy, Debug)]
pub struct Rng {
    a: u32,
}

impl Rng {
    pub fn new(seed: u32) -> Rng {
        Rng { a: seed }
    }

    /// The next draw in [0, 1). (Named as the JS closure is called; not an
    /// iterator.)
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f64 {
        self.a = self.a.wrapping_add(0x6d2b79f5);
        let mut t = self.a;
        t = imul(t ^ (t >> 15), t | 1);
        t ^= t.wrapping_add(imul(t ^ (t >> 7), t | 61));
        ((t ^ (t >> 14)) as f64) / 4294967296.0
    }
}

// ── Oscillators ──────────────────────────────────────────────────
/// PolyBLEP: subtracts the band-limited step's residual around each
/// discontinuity, which removes most of the aliasing of a naive saw or
/// pulse.
pub fn blep(t: f64, dt: f64) -> f64 {
    if t < dt {
        let t = t / dt;
        return t + t - t * t - 1.0;
    }
    if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        return t * t + t + t + 1.0;
    }
    0.0
}

pub const OSC_SAW: u32 = 1;
pub const OSC_PULSE: u32 = 2;
pub const OSC_TRI: u32 = 4;
pub const OSC_SUB: u32 = 8;

/// One analogue-style oscillator: saw, pulse (with width) and triangle, all
/// from one phase, plus a sub an octave down (the Juno's square sub). The
/// caller mixes the outputs.
#[derive(Clone, Copy, Debug, Default)]
pub struct Osc {
    pub ph: f64,
    pub sub_ph: u32,
    pub tri: f64,
    pub saw: f64,
    pub pulse: f64,
    pub sub: f64,
}

impl Osc {
    pub fn new(phase: f64) -> Osc {
        Osc {
            ph: phase,
            ..Osc::default()
        }
    }

    /// `mask`: which outputs to compute (`OSC_SAW | OSC_PULSE | OSC_TRI |
    /// OSC_SUB`). The JS defaults are `width = 0.5, mask = 1`.
    pub fn run(&mut self, hz: f64, width: f64, mask: u32, sr: f64) -> f64 {
        let dt = clamp(hz / sr, 0.0, 0.45);
        let mut ph = self.ph;
        if mask & 1 != 0 {
            self.saw = 2.0 * ph - 1.0 - blep(ph, dt);
        }
        if mask & 2 != 0 {
            let mut p = if ph < width { 1.0 } else { -1.0 };
            p += blep(ph, dt);
            let mut q = ph - width;
            if q < 0.0 {
                q += 1.0;
            }
            self.pulse = p - blep(q, dt);
        }
        if mask & 4 != 0 {
            // Triangle: the integrated square (leaky, so it can't drift off).
            let sq =
                (if ph < 0.5 { 1.0 } else { -1.0 }) + blep(ph, dt) - blep((ph + 0.5) % 1.0, dt);
            self.tri = self.tri * 0.9995 + sq * 4.0 * dt;
        }
        ph += dt;
        if ph >= 1.0 {
            ph -= 1.0;
            self.sub_ph ^= 1;
        }
        self.ph = ph;
        if mask & 8 != 0 {
            // The sub flips on every wrap: a rising edge when it turns 1. Just
            // after a wrap the edge was the one that set subPh; just before,
            // the next one.
            let s = if self.sub_ph != 0 { 1.0 } else { -1.0 };
            let rising = if ph < 0.5 { s > 0.0 } else { s < 0.0 };
            self.sub = s + (if rising { 1.0 } else { -1.0 }) * blep(ph, dt);
        }
        self.saw
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Sine {
    pub ph: f64,
}

impl Sine {
    pub fn new(phase: f64) -> Sine {
        Sine { ph: phase }
    }

    pub fn run(&mut self, hz: f64, sr: f64) -> f64 {
        let y = (TAU * self.ph).sin();
        self.ph += hz / sr;
        if self.ph >= 1.0 {
            self.ph -= self.ph.floor();
        }
        y
    }
}

/// Slow analogue drift: a smoothed random walk, in cents. Each voice owns
/// one. The JS closes over the voice's `rng`; here the voice passes its
/// [`Rng`] to `run`, so the draws happen in the same order.
#[derive(Clone, Copy, Debug)]
pub struct Drift {
    c: f64,
    pub v: f64,
    t: f64,
    a: f64,
    n: i64,
    sr: f64,
}

impl Drift {
    /// The JS defaults are `cents = 4, hz = 0.6`.
    pub fn new(r: &mut Rng, cents: f64, hz: f64, sr: f64) -> Drift {
        let v = (r.next() * 2.0 - 1.0) * cents;
        Drift {
            c: cents,
            v,
            t: v,
            a: 1.0 - ((-TAU * hz) / sr).exp(),
            n: 0,
            sr,
        }
    }

    /// Called every `n` samples (control rate).
    pub fn run(&mut self, r: &mut Rng, n: i64) -> f64 {
        self.n -= n;
        if self.n <= 0 {
            self.t = (r.next() * 2.0 - 1.0) * self.c;
            self.n = to_i32(self.sr * (0.2 + r.next() * 0.6)) as i64;
        }
        self.v += self.a * n as f64 * (self.t - self.v);
        self.v
    }
}

// ── Filters ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct OnePole {
    y: f64,
    a: f64,
    sr: f64,
}

impl OnePole {
    pub fn new(hz: f64, sr: f64) -> OnePole {
        let mut p = OnePole { y: 0.0, a: 0.0, sr };
        p.set_hz(hz);
        p
    }

    pub fn set_hz(&mut self, hz: f64) -> &mut Self {
        self.a = 1.0 - ((-TAU * clamp(hz, 1.0, self.sr * 0.49)) / self.sr).exp();
        self
    }

    /// `this.y = 0`: the string's pluck filter starts each burst clean.
    pub fn reset(&mut self) {
        self.y = 0.0;
    }

    pub fn lp(&mut self, x: f64) -> f64 {
        self.y += self.a * (x - self.y);
        self.y
    }

    pub fn hp(&mut self, x: f64) -> f64 {
        x - self.lp(x)
    }
}

/// ZDF 4-pole ladder (Zavalishin's TPT form) with a tanh on the input of
/// the feedback sum. res 0..1 (self-oscillates near 1); drive pushes the
/// input. `mode` 0 = 24 dB low-pass (Moog / Juno IR3109), 1 = an 18 dB tap
/// mix that leans toward the 303's diode ladder.
#[derive(Clone, Copy, Debug)]
pub struct Ladder {
    s: [f64; 4],
    out: [f64; 4],
    g: f64,
    big_g: f64,
    k: f64,
    hz: f64,
    res: f64,
}

impl Default for Ladder {
    fn default() -> Self {
        Ladder::new()
    }
}

impl Ladder {
    pub fn new() -> Ladder {
        Ladder {
            s: [0.0; 4],
            out: [0.0; 4],
            g: 0.0,
            big_g: 0.0,
            k: 0.0,
            hz: -1.0,
            res: -1.0,
        }
    }

    pub fn set(&mut self, hz: f64, res: f64, sr: f64) {
        if hz != self.hz {
            self.hz = hz;
            let g = ((std::f64::consts::PI * clamp(hz, 10.0, sr * 0.45)) / sr).tan();
            self.g = g;
            self.big_g = g / (1.0 + g);
        }
        if res != self.res {
            self.res = res;
            self.k = 4.0 * clamp(res, 0.0, 1.05);
        }
    }

    /// The JS defaults are `drive = 1, mode = 0`.
    pub fn run(&mut self, x: f64, drive: f64, mode: u32) -> f64 {
        let s = &mut self.s;
        let big_g = self.big_g;
        let k = self.k;
        let g2 = big_g * big_g;
        let g3 = g2 * big_g;
        let g4 = g3 * big_g;
        // Estimate of the loop output without the input (the "zero-delay"
        // solve).
        let big_s = (g3 * s[0] + g2 * s[1] + big_g * s[2] + s[3]) * (1.0 - big_g);
        let u = tanh((x * drive - k * big_s) / (1.0 + k * g4));
        let mut y = u;
        let out = &mut self.out;
        for i in 0..4 {
            let v = (y - s[i]) * big_g;
            let lp = v + s[i];
            s[i] = lp + v;
            out[i] = lp;
            y = lp;
        }
        if mode == 1 {
            return out[2] * 1.25 - out[3] * 0.3;
        }
        // Make up some of the pass-band loss resonance costs (as the hardware
        // does).
        out[3] * (1.0 + k * 0.25)
    }

    pub fn reset(&mut self) {
        self.s = [0.0; 4];
    }
}

/// ZDF state-variable filter: low, band and high at once.
#[derive(Clone, Copy, Debug)]
pub struct Svf {
    ic1: f64,
    ic2: f64,
    pub lp: f64,
    pub bp: f64,
    pub hp: f64,
    pub hz: f64,
    pub q: f64,
    a1: f64,
    a2: f64,
    a3: f64,
    k: f64,
}

impl Default for Svf {
    fn default() -> Self {
        Svf::new()
    }
}

impl Svf {
    pub fn new() -> Svf {
        Svf {
            ic1: 0.0,
            ic2: 0.0,
            lp: 0.0,
            bp: 0.0,
            hp: 0.0,
            hz: -1.0,
            q: -1.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            k: 0.0,
        }
    }

    /// `new SVF().set(hz, q)`.
    pub fn with(hz: f64, q: f64, sr: f64) -> Svf {
        let mut f = Svf::new();
        f.set(hz, q, sr);
        f
    }

    /// The JS default is `q = 0.707`.
    pub fn set(&mut self, hz: f64, q: f64, sr: f64) -> &mut Self {
        if hz == self.hz && q == self.q {
            return self;
        }
        self.hz = hz;
        self.q = q;
        let g = ((std::f64::consts::PI * clamp(hz, 10.0, sr * 0.45)) / sr).tan();
        let k = 1.0 / q;
        self.a1 = 1.0 / (1.0 + g * (g + k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
        self.k = k;
        self
    }

    pub fn run(&mut self, x: f64) -> f64 {
        let v3 = x - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        self.lp = v2;
        self.bp = v1;
        self.hp = x - self.k * v1 - v2;
        v2
    }
}

// ── Envelopes ────────────────────────────────────────────────────
/// Analogue-style ADSR: exponential segments (RC curves), retriggerable
/// from wherever it is (no click), times in seconds. `st`: 0 idle, 1
/// attack, 2/3 decay, 4 release.
#[derive(Clone, Copy, Debug)]
pub struct Adsr {
    pub v: f64,
    pub st: u32,
    ka: f64,
    kd: f64,
    s: f64,
    kr: f64,
    sr: f64,
}

impl Adsr {
    /// `new ADSR()`: the JS defaults (0.005, 0.2, 0.7, 0.2).
    pub fn new(sr: f64) -> Adsr {
        let mut e = Adsr {
            v: 0.0,
            st: 0,
            ka: 0.0,
            kd: 0.0,
            s: 0.0,
            kr: 0.0,
            sr,
        };
        e.set_all(0.005, 0.2, 0.7, 0.2);
        e
    }

    pub fn set_all(&mut self, a: f64, d: f64, s: f64, r: f64) -> &mut Self {
        let sr = self.sr;
        let c = |t: f64| 1.0 - (-1.0 / (t.max(0.0005) * sr / 4.0)).exp();
        self.ka = 1.0 - (-1.0 / (a.max(0.0005) * sr / 1.6)).exp();
        self.kd = c(d);
        self.s = s;
        self.kr = c(r);
        self
    }

    pub fn on(&mut self) {
        self.st = 1;
    }

    pub fn off(&mut self) {
        if self.st != 0 && self.st < 4 {
            self.st = 4;
        }
    }

    pub fn done(&self) -> bool {
        self.st == 0 || (self.st == 4 && self.v < 5e-4)
    }

    pub fn run(&mut self) -> f64 {
        match self.st {
            1 => {
                self.v += self.ka * (1.25 - self.v);
                if self.v >= 1.0 {
                    self.v = 1.0;
                    self.st = 2;
                }
            }
            2 | 3 => self.v += self.kd * (self.s - self.v),
            4 => {
                self.v += self.kr * (0.0 - self.v);
                if self.v < 5e-4 {
                    self.v = 0.0;
                    self.st = 0;
                }
            }
            _ => {}
        }
        self.v
    }
}

/// A one-shot decay (drums, the 303's filter envelope): jumps to 1, decays.
#[derive(Clone, Copy, Debug)]
pub struct Decay {
    pub v: f64,
    pub k: f64,
    sr: f64,
}

impl Decay {
    /// The JS default is `t = 0.2`.
    pub fn new(t: f64, sr: f64) -> Decay {
        let mut d = Decay { v: 0.0, k: 0.0, sr };
        d.set(t);
        d
    }

    pub fn set(&mut self, t: f64) -> &mut Self {
        self.k = (-1.0 / (t.max(0.0005) * self.sr / 4.6)).exp();
        self
    }

    /// The JS default is `v = 1`.
    pub fn hit(&mut self, v: f64) {
        self.v = v;
    }

    pub fn run(&mut self) -> f64 {
        self.v *= self.k;
        self.v
    }
}

// ── Effects ──────────────────────────────────────────────────────
/// A `Float32Array` ring: writes round to `f32`.
#[derive(Clone, Debug)]
pub struct Delay {
    b: Vec<f32>,
    w: usize,
}

impl Delay {
    pub fn new(max_secs: f64, sr: f64) -> Delay {
        Delay {
            b: vec![0.0; (max_secs * sr).ceil() as usize + 4],
            w: 0,
        }
    }

    pub fn write(&mut self, x: f64) {
        self.b[self.w] = x as f32;
        self.w = (self.w + 1) % self.b.len();
    }

    /// Read `d` samples back (fractional, linear interpolation).
    pub fn read(&self, d: f64) -> f64 {
        let n = self.b.len();
        let mut p = self.w as f64 - d - 1.0;
        while p < 0.0 {
            p += n as f64;
        }
        let i = p as usize;
        let f = p - i as f64;
        self.b[i] as f64 * (1.0 - f) + self.b[(i + 1) % n] as f64 * f
    }
}

/// Stereo ping-pong delay with high-pass in, low-pass in the loop.
#[derive(Clone, Debug)]
pub struct PingPong {
    l: Delay,
    r: Delay,
    hp: OnePole,
    lp_l: OnePole,
    lp_r: OnePole,
    pub time: f64,
    pub fb: f64,
    sr: f64,
}

impl PingPong {
    pub fn new(sr: f64) -> PingPong {
        PingPong {
            l: Delay::new(2.1, sr),
            r: Delay::new(2.1, sr),
            hp: OnePole::new(280.0, sr),
            lp_l: OnePole::new(3200.0, sr),
            lp_r: OnePole::new(3200.0, sr),
            time: 0.3,
            fb: 0.38,
            sr,
        }
    }

    /// Adds into `out`.
    pub fn run(&mut self, x: f64, out: &mut [f64; 2]) {
        let d = self.time * self.sr;
        let yl = self.l.read(d);
        let yr = self.r.read(d);
        self.l.write(self.hp.hp(x) + self.lp_r.lp(yr) * self.fb);
        self.r.write(self.lp_l.lp(yl));
        out[0] += yl * 0.5;
        out[1] += yr * 0.5;
    }
}

/// An 8-line feedback delay network (Householder mix), damped and slowly
/// modulated: a smooth hall with no metallic ring, cheap enough for phones.
const FDN_LEN: [f64; 8] = [
    1031.0, 1327.0, 1523.0, 1801.0, 2053.0, 2311.0, 2633.0, 2971.0,
];

#[derive(Clone, Debug)]
pub struct Reverb {
    n: Vec<Delay>,
    len: [f64; 8],
    damp: [OnePole; 8],
    ph: [f64; 8],
    pre: Delay,
    in_lp: OnePole,
    in_hp: OnePole,
    y: [f64; 8],
    k: u32,
    mod_: [f64; 8],
    pub rt: f64,
    g: [f64; 8],
    sr: f64,
}

impl Reverb {
    /// The JS default is `seed = 7`.
    pub fn new(seed: u32, sr: f64) -> Reverb {
        let mut r = Rng::new(seed);
        let n = FDN_LEN
            .iter()
            .map(|l| Delay::new((l * (sr / 48000.0) + 32.0) / sr, sr))
            .collect();
        let len = FDN_LEN.map(|l| l * (sr / 48000.0));
        let damp = [OnePole::new(5200.0, sr); 8];
        let mut ph = [0.0; 8];
        for p in &mut ph {
            *p = r.next();
        }
        let mut rv = Reverb {
            n,
            len,
            damp,
            ph,
            pre: Delay::new(0.1, sr),
            in_lp: OnePole::new(9000.0, sr),
            in_hp: OnePole::new(120.0, sr),
            y: [0.0; 8],
            k: 0,
            mod_: len,
            rt: 0.0,
            g: [0.0; 8],
            sr,
        };
        rv.set(2.4, 0.5);
        rv
    }

    /// size: decay time (RT60, s); tone 0 (dark) .. 1 (bright). The JS
    /// default is `tone = 0.5`.
    pub fn set(&mut self, rt60: f64, tone: f64) {
        self.rt = rt60;
        self.g = self.len.map(|l| 10f64.powf((-3.0 * l) / (rt60 * self.sr)));
        for d in &mut self.damp {
            d.set_hz(2500.0 + tone * 7000.0);
        }
    }

    /// Adds into `out`.
    pub fn run(&mut self, in_l: f64, in_r: f64, out: &mut [f64; 2]) {
        let sr = self.sr;
        self.pre.write((in_l + in_r) * 0.5);
        let mut x = self.pre.read(0.018 * sr);
        x = self.in_hp.hp(self.in_lp.lp(x));
        let y = &mut self.y;
        let mut sum = 0.0;
        self.k = (self.k + 1) & 31;
        if self.k == 0 {
            for i in 0..8 {
                self.ph[i] += (32.0 * (0.07 + i as f64 * 0.013)) / sr;
                if self.ph[i] > 1.0 {
                    self.ph[i] -= 1.0;
                }
                self.mod_[i] = self.len[i] + (TAU * self.ph[i]).sin() * 6.0;
            }
        }
        for i in 0..8 {
            y[i] = self.damp[i].lp(self.n[i].read(self.mod_[i])) * self.g[i];
            sum += y[i];
        }
        let h = sum * 0.25; // Householder: y - 2/N * sum
        for i in 0..8 {
            self.n[i].write(y[i] - h + x * (if i & 1 != 0 { -0.35 } else { 0.35 }));
        }
        out[0] += (y[0] - y[2] + y[4] - y[6]) * 0.6;
        out[1] += (y[1] - y[3] + y[5] - y[7]) * 0.6;
    }
}

/// Stereo-linked feed-forward compressor (RMS-ish detector), for glue.
#[derive(Clone, Copy, Debug)]
pub struct Compressor {
    env: f64,
    pub gr: f64,
    th: f64,
    ratio: f64,
    ka: f64,
    kr: f64,
    sr: f64,
}

impl Compressor {
    pub fn new(sr: f64) -> Compressor {
        let mut c = Compressor {
            env: 0.0,
            gr: 1.0,
            th: 0.0,
            ratio: 1.0,
            ka: 0.0,
            kr: 0.0,
            sr,
        };
        c.set(-14.0, 2.5, 0.01, 0.15);
        c
    }

    pub fn set(&mut self, thresh_db: f64, ratio: f64, att: f64, rel: f64) -> &mut Self {
        self.th = thresh_db;
        self.ratio = ratio;
        self.ka = (-1.0 / (att * self.sr)).exp();
        self.kr = (-1.0 / (rel * self.sr)).exp();
        self
    }

    /// Returns the gain reduction to apply.
    pub fn run(&mut self, l: f64, r: f64) -> f64 {
        let x = l.abs().max(r.abs());
        let k = if x > self.env { self.ka } else { self.kr };
        self.env = k * self.env + (1.0 - k) * x;
        let db = 20.0 * (self.env + 1e-9).log10();
        let over = db - self.th;
        self.gr = if over > 0.0 {
            10f64.powf((-over * (1.0 - 1.0 / self.ratio)) / 20.0)
        } else {
            1.0
        };
        self.gr
    }
}

/// Tape-ish saturation: asymmetric soft clip with a little head bump and a
/// high-frequency roll-off that grows with drive.
#[derive(Clone, Copy, Debug)]
pub struct Tape {
    lp: OnePole,
    bump: Svf,
    dc: OnePole,
    pub drive: f64,
}

impl Tape {
    pub fn new(sr: f64) -> Tape {
        Tape {
            lp: OnePole::new(16000.0, sr),
            bump: Svf::with(90.0, 0.9, sr),
            dc: OnePole::new(12.0, sr),
            drive: 0.0,
        }
    }

    pub fn set(&mut self, drive: f64) -> &mut Self {
        self.drive = drive;
        self.lp.set_hz(18000.0 - drive * 6000.0);
        self
    }

    pub fn run(&mut self, x: f64) -> f64 {
        if self.drive <= 0.0 {
            return x;
        }
        // Unity gain for small signals; drive sets how early the curve bends.
        let d = 1.0 + self.drive * 1.5;
        self.bump.run(x);
        let y = tanh((x + self.bump.bp * 0.15 * self.drive) * d + 0.05 * self.drive) / d;
        let dc = self.dc.lp(y);
        self.lp.lp(y - dc)
    }
}

/// A brickwall-ish peak limiter at the very end, so nothing ever clips.
#[derive(Clone, Copy, Debug)]
pub struct Limiter {
    pub ceil: f64,
    g: f64,
    kr: f64,
}

impl Limiter {
    /// The JS default is `ceil = 0.95`.
    pub fn new(ceil: f64, sr: f64) -> Limiter {
        Limiter {
            ceil,
            g: 1.0,
            kr: (-1.0 / (0.12 * sr)).exp(),
        }
    }

    /// Sets `out`.
    pub fn run(&mut self, l: f64, r: f64, out: &mut [f64; 2]) {
        let p = l.abs().max(r.abs()) * self.g;
        if p > self.ceil {
            self.g *= self.ceil / p;
        } else {
            self.g = self.kr * self.g + (1.0 - self.kr);
        }
        out[0] = l * self.g;
        out[1] = r * self.g;
    }
}

/// Juno-style BBD chorus: two delay taps swept by one triangle LFO in
/// opposite directions, wet only on the sides (mode I 0.51 Hz, II 0.86 Hz).
#[derive(Clone, Debug)]
pub struct Chorus {
    d: Delay,
    ph: f64,
    rate: f64,
    depth: f64,
    lp: OnePole,
    /// Unset (false) until `set`, as the JS's `this.on` is undefined.
    pub on: bool,
    sr: f64,
}

impl Chorus {
    pub fn new(sr: f64) -> Chorus {
        Chorus {
            d: Delay::new(0.03, sr),
            ph: 0.0,
            rate: 0.513,
            depth: 1.0,
            lp: OnePole::new(9000.0, sr),
            on: false,
            sr,
        }
    }

    /// `mode`: 0 off, 1 (I), 2 (II), 3 (I+II, the fast one); a number, as
    /// the patch's `chorus` is.
    pub fn set(&mut self, mode: f64) {
        self.rate = if mode == 2.0 {
            0.863
        } else if mode == 3.0 {
            9.75
        } else {
            0.513
        };
        self.depth = if mode == 3.0 { 0.25 } else { 1.0 };
        self.on = mode > 0.0;
    }

    /// Adds into `out`. The JS default is `gain = 1`.
    pub fn run(&mut self, x: f64, out: &mut [f64; 2], gain: f64) {
        if !self.on {
            out[0] += x * gain;
            out[1] += x * gain;
            return;
        }
        let sr = self.sr;
        let w = self.lp.lp(x);
        self.d.write(w);
        self.ph += self.rate / sr;
        if self.ph > 1.0 {
            self.ph -= 1.0;
        }
        let tri = if self.ph < 0.5 {
            self.ph * 4.0 - 1.0
        } else {
            3.0 - self.ph * 4.0
        };
        let base = 0.0035 * sr;
        let sw = 0.0017 * sr * self.depth;
        let a = self.d.read(base + sw * tri);
        let b = self.d.read(base - sw * tri);
        out[0] += (x * 0.7 + a * 0.7) * gain;
        out[1] += (x * 0.7 + b * 0.7) * gain;
    }
}
