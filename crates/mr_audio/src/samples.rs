//! `src/game/audio/samples.js`: one-shot sounds rendered once, with plain
//! sample math, into AudioBuffers: the drum kit for the music and the
//! layered crash / gravel / scrape / clunk sounds for the SFX. A hit then
//! costs one AudioBufferSourceNode and a gain instead of a small graph of
//! oscillators and filters per note, and the sounds can use things WebAudio
//! nodes can't do cheaply (modal resonators, gated reverb tails, per-grain
//! filtering). Everything renders in ~50 ms.
//!
//! Port notes: every inexact `Math` function goes through `mr_math::kernel`,
//! and every store into a `Float32Array` rounds to `f32` at that point, so
//! the buffers are bit-identical to the JS reference (SPEC 7.5). Each
//! function returns its channels as `Vec<Vec<f32>>`; [`render_kit`] and
//! [`render_sfx`] return them by name in the JS object's key order, and
//! [`to_buffer`] makes the AudioBuffer.

use crate::wa::{AudioBuffer, AudioContext};
use mr_math::Mulberry32;
use mr_math::js;
use mr_math::kernel::{exp, pow, sin, tanh};

/// `rngFrom(seed)`: mulberry32, the same generator as `util/math.js`.
pub fn rng_from(seed: u32) -> Mulberry32 {
    Mulberry32::new(seed)
}

/// The JS closures take `rnd` as a function; here a mutable generator.
type Rnd<'a> = &'a mut Mulberry32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BqType {
    Lp,
    Hp,
    Bp,
}

/// RBJ-cookbook biquad for offline use (direct form I).
#[derive(Clone, Debug)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    fn new(ty: BqType, f: f64, q: f64, sr: f64) -> Self {
        let mut b = Biquad {
            b0: 0.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        };
        b.set(ty, f, q, sr);
        b
    }

    fn set(&mut self, ty: BqType, f: f64, q: f64, sr: f64) -> &mut Self {
        let w = (2.0 * std::f64::consts::PI * js::min(f, sr * 0.45)) / sr;
        let c = mr_math::kernel::cos(w);
        let al = sin(w) / (2.0 * q);
        let (b0, b1, b2);
        match ty {
            BqType::Lp => {
                b0 = (1.0 - c) / 2.0;
                b1 = 1.0 - c;
                b2 = b0;
            }
            BqType::Hp => {
                b0 = (1.0 + c) / 2.0;
                b1 = -(1.0 + c);
                b2 = b0;
            }
            // band-pass, 0 dB peak
            BqType::Bp => {
                b0 = al;
                b1 = 0.0;
                b2 = -al;
            }
        }
        let a0 = 1.0 + al;
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = (-2.0 * c) / a0;
        self.a2 = (1.0 - al) / a0;
        self
    }

    fn p(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

const TAU: f64 = std::f64::consts::PI * 2.0;

fn sat(x: f64, d: f64) -> f64 {
    tanh(x * d) / tanh(d)
}

/// `Math.floor(secs * sr)` as a length.
fn len_of(secs: f64, sr: f64) -> usize {
    (secs * sr).floor() as usize
}

/// `rnd() * 2 - 1`.
fn bipolar(r: Rnd) -> f64 {
    r.next_f64() * 2.0 - 1.0
}

/// `Float32Array` element store.
fn f32s(x: f64) -> f32 {
    x as f32
}

/// Short fades at both ends so nothing clicks, then peak-normalise to `peak`.
fn finish(
    mut chs: Vec<Vec<f32>>,
    sr: f64,
    peak: f64,
    fade_in: f64,
    fade_out: f64,
) -> Vec<Vec<f32>> {
    let mut m = 1e-9;
    for d in &chs {
        for &v in d.iter() {
            m = js::max(m, (v as f64).abs());
        }
    }
    let fi = js::max(1.0, (fade_in * sr).floor());
    let fo = js::max(1.0, (fade_out * sr).floor());
    for d in chs.iter_mut() {
        let n = d.len() as f64;
        for (i, v) in d.iter_mut().enumerate() {
            let i = i as f64;
            let mut g = peak / m;
            if i < fi {
                g *= i / fi;
            }
            if i > n - fo {
                g *= (n - i) / fo;
            }
            *v = f32s(*v as f64 * g);
        }
    }
    chs
}

fn finish_default(chs: Vec<Vec<f32>>, sr: f64, peak: f64) -> Vec<Vec<f32>> {
    finish(chs, sr, peak, 0.0005, 0.01)
}

// ── Drum voices ──────────────────────────────────────────────────

/// `kick`'s options, with the JS defaults.
#[derive(Clone, Copy, Debug)]
pub struct KickOpts {
    pub f0: f64,
    pub f1: f64,
    pub pt: f64,
    pub decay: f64,
    pub len: f64,
    pub click: f64,
    pub drive: f64,
    pub tone: f64,
}

impl Default for KickOpts {
    fn default() -> Self {
        KickOpts {
            f0: 170.0,
            f1: 48.0,
            pt: 0.035,
            decay: 0.3,
            len: 0.6,
            click: 0.5,
            drive: 1.6,
            tone: 0.0,
        }
    }
}

/// Sine body with a pitch drop, a click transient and a little saturation.
fn kick(sr: f64, o: KickOpts, rnd: Rnd) -> Vec<Vec<f32>> {
    let n = len_of(o.len, sr);
    let mut d = vec![0f32; n];
    let mut hp = Biquad::new(BqType::Hp, 1800.0, 0.7, sr);
    let mut lp = Biquad::new(BqType::Lp, 7000.0, 0.7, sr);
    let mut ph = 0.0;
    for (i, out) in d.iter_mut().enumerate() {
        let t = i as f64 / sr;
        let f = o.f1 + (o.f0 - o.f1) * exp(-t / o.pt);
        ph += (TAU * f) / sr;
        // The body holds a touch longer than a plain exponential, then falls away.
        let env = exp(-t / o.decay) * (1.0 - exp(-t / 0.0012));
        let mut s = sin(ph) * env + o.tone * sin(ph * 2.0) * env * exp(-t / 0.05);
        let cl = lp.p(hp.p(bipolar(rnd))) * exp(-t / 0.004) * o.click;
        s = sat(s * 1.1 + cl, o.drive);
        *out = f32s(s);
    }
    finish(vec![d], sr, 0.95, 0.0003, 0.02)
}

/// `snare`'s options, with the JS defaults.
#[derive(Clone, Copy, Debug)]
pub struct SnareOpts {
    pub body: f64,
    pub decay: f64,
    pub noise: f64,
    pub bright: f64,
    pub gate: f64,
    pub gate_len: f64,
    pub crack: f64,
    pub len: f64,
}

impl Default for SnareOpts {
    fn default() -> Self {
        SnareOpts {
            body: 190.0,
            decay: 0.16,
            noise: 1.0,
            bright: 5200.0,
            gate: 0.0,
            gate_len: 0.26,
            crack: 0.6,
            len: 0.5,
        }
    }
}

/// Tuned body, noise snares, an optional gated "plate" tail (the 80s sound).
fn snare(sr: f64, o: SnareOpts, rnd: Rnd, stereo: bool) -> Vec<Vec<f32>> {
    let n = len_of(o.len, sr);
    let mut out = Vec::new();
    for _c in 0..(if stereo { 2 } else { 1 }) {
        let mut d = vec![0f32; n];
        let mut hp = Biquad::new(BqType::Hp, 900.0, 0.7, sr);
        let mut lp = Biquad::new(BqType::Lp, o.bright, 0.6, sr);
        let mut cr = Biquad::new(BqType::Bp, 3800.0, 1.2, sr);
        let mut tl = Biquad::new(BqType::Lp, 5200.0, 0.5, sr);
        let mut th = Biquad::new(BqType::Hp, 350.0, 0.5, sr);
        let (mut p1, mut p2) = (0.0, 0.0);
        for (i, out) in d.iter_mut().enumerate() {
            let t = i as f64 / sr;
            let pd = 1.0 + 0.25 * exp(-t / 0.012);
            p1 += (TAU * o.body * pd) / sr;
            p2 += (TAU * o.body * 1.72 * pd) / sr;
            let tone = (sin(p1) * 0.8 + sin(p2) * 0.45) * exp(-t / 0.055);
            let w = bipolar(rnd);
            let nz = lp.p(hp.p(w)) * exp(-t / o.decay) * o.noise;
            let ck = cr.p(w) * exp(-t / 0.005) * o.crack * 3.0;
            let mut tail = 0.0;
            if o.gate != 0.0 {
                // A dense reverb tail held flat, then chopped: the gate.
                let g = if t < 0.012 {
                    t / 0.012
                } else if t < o.gate_len {
                    1.0 - 0.35 * (t / o.gate_len)
                } else {
                    js::max(0.0, 1.0 - (t - o.gate_len) / 0.03) * 0.65
                };
                tail = th.p(tl.p(bipolar(rnd))) * g * o.gate;
            }
            *out = f32s(sat(tone + nz + ck + tail, 1.3));
        }
        out.push(d);
    }
    finish_default(out, sr, 0.9)
}

/// `clap`'s options, with the JS defaults.
#[derive(Clone, Copy, Debug)]
pub struct ClapOpts {
    pub tail: f64,
    pub freq: f64,
    pub len: f64,
}

impl Default for ClapOpts {
    fn default() -> Self {
        ClapOpts {
            tail: 0.14,
            freq: 1150.0,
            len: 0.45,
        }
    }
}

fn clap(sr: f64, o: ClapOpts, rnd: Rnd) -> Vec<Vec<f32>> {
    let n = len_of(o.len, sr);
    let mut out = Vec::new();
    for c in 0..2 {
        let c = c as f64;
        let mut d = vec![0f32; n];
        let mut bp = Biquad::new(BqType::Bp, o.freq, 1.1, sr);
        let mut hp = Biquad::new(BqType::Hp, 600.0, 0.7, sr);
        let hits = [0.0, 0.0105 + c * 0.0015, 0.021, 0.0325 - c * 0.001];
        for (i, out) in d.iter_mut().enumerate() {
            let t = i as f64 / sr;
            let mut e = 0.0;
            for (k, &h) in hits.iter().enumerate() {
                let u = t - h;
                if (0.0..0.02).contains(&u) {
                    e = js::max(e, exp(-u / 0.0038) * (if k == 3 { 1.0 } else { 0.8 }));
                }
            }
            let last = t - hits[3];
            if last > 0.0 {
                e = js::max(e, exp(-last / o.tail) * 0.55);
            }
            *out = f32s(hp.p(bp.p(bipolar(rnd))) * e * 3.0);
        }
        out.push(d);
    }
    finish_default(out, sr, 0.85)
}

/// 808-style metal: six detuned square waves, band-limited to the top end,
/// plus some noise. Used for hats, the ride and the crash.
const METAL: [f64; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];

/// `metal`'s options, with the JS defaults.
#[derive(Clone, Copy, Debug)]
pub struct MetalOpts {
    pub decay: f64,
    pub len: f64,
    pub hp_f: f64,
    pub mix: f64,
    pub scale: f64,
    pub attack: f64,
    pub darken: f64,
    pub bell: f64,
}

impl Default for MetalOpts {
    fn default() -> Self {
        MetalOpts {
            decay: 0.03,
            len: 0.15,
            hp_f: 7000.0,
            mix: 0.45,
            scale: 1.0,
            attack: 0.0005,
            darken: 0.0,
            bell: 0.0,
        }
    }
}

fn metal(sr: f64, o: MetalOpts, rnd: Rnd, stereo: bool) -> Vec<Vec<f32>> {
    let n = len_of(o.len, sr);
    let mut out = Vec::new();
    for c in 0..(if stereo { 2 } else { 1 }) {
        let mut d = vec![0f32; n];
        let mut hp1 = Biquad::new(BqType::Hp, o.hp_f, 0.7, sr);
        let mut hp2 = Biquad::new(BqType::Hp, o.hp_f, 0.7, sr);
        let mut lp = Biquad::new(BqType::Lp, 15000.0, 0.6, sr);
        let mut bp = Biquad::new(BqType::Bp, o.hp_f * 1.4, 0.9, sr);
        let mut ph: Vec<f64> = METAL.iter().map(|_| rnd.next_f64()).collect();
        let det = 1.0 + (if c != 0 { 0.004 } else { -0.004 });
        let fr: Vec<f64> = METAL.iter().map(|f| f * o.scale * det).collect();
        for (i, out) in d.iter_mut().enumerate() {
            let t = i as f64 / sr;
            let mut sq = 0.0;
            for k in 0..6 {
                ph[k] += fr[k] / sr;
                sq += if ph[k] % 1.0 < 0.5 { 1.0 } else { -1.0 };
            }
            let mut s = sq / 6.0 * (1.0 - o.mix) + bipolar(rnd) * o.mix;
            if o.darken != 0.0 {
                lp.set(
                    BqType::Lp,
                    15000.0 - o.darken * js::min(1.0, t / o.len) * 10000.0,
                    0.6,
                    sr,
                );
            }
            let a = hp2.p(hp1.p(s));
            s = lp.p(a + bp.p(s) * 0.5);
            let mut b = 0.0;
            if o.bell != 0.0 {
                b = (sin(TAU * 1240.0 * o.scale * t)
                    + 0.6 * sin(TAU * 3170.0 * o.scale * t)
                    + 0.35 * sin(TAU * 5120.0 * o.scale * t))
                    * exp(-t / (o.decay * 0.6))
                    * o.bell;
            }
            let env = (if t < o.attack { t / o.attack } else { 1.0 }) * exp(-t / o.decay);
            *out = f32s(s * env + b * 0.2);
        }
        out.push(d);
    }
    finish_default(out, sr, 0.8)
}

/// `tom`'s options, with the JS defaults.
#[derive(Clone, Copy, Debug)]
pub struct TomOpts {
    pub f: f64,
    pub decay: f64,
    pub len: f64,
    pub pan: f64,
}

impl Default for TomOpts {
    fn default() -> Self {
        TomOpts {
            f: 110.0,
            decay: 0.28,
            len: 0.7,
            pan: 0.0,
        }
    }
}

fn tom(sr: f64, o: TomOpts, rnd: Rnd) -> Vec<Vec<f32>> {
    let n = len_of(o.len, sr);
    let mut l = vec![0f32; n];
    let mut r = vec![0f32; n];
    let mut bp = Biquad::new(BqType::Bp, o.f * 4.0, 1.0, sr);
    let mut ph = 0.0;
    for i in 0..n {
        let t = i as f64 / sr;
        ph += (TAU * o.f * (1.0 + 0.6 * exp(-t / 0.03))) / sr;
        let s = sat(
            sin(ph) * exp(-t / o.decay) + bp.p(bipolar(rnd)) * exp(-t / 0.02) * 0.6,
            1.5,
        );
        l[i] = f32s(s * (1.0 - js::max(0.0, o.pan)));
        r[i] = f32s(s * (1.0 + js::min(0.0, o.pan)));
    }
    finish_default(vec![l, r], sr, 0.85)
}

fn shaker(sr: f64, rnd: Rnd) -> Vec<Vec<f32>> {
    let n = len_of(0.12, sr);
    let mut out = Vec::new();
    for c in 0..2 {
        let mut d = vec![0f32; n];
        let mut bp = Biquad::new(BqType::Bp, 6800.0 + c as f64 * 400.0, 1.1, sr);
        let mut hp = Biquad::new(BqType::Hp, 3500.0, 0.7, sr);
        for (i, out) in d.iter_mut().enumerate() {
            let t = i as f64 / sr;
            let e = if t < 0.012 {
                t / 0.012
            } else {
                exp(-(t - 0.012) / 0.03)
            };
            *out = f32s(hp.p(bp.p(bipolar(rnd))) * e);
        }
        out.push(d);
    }
    finish(out, sr, 0.8, 0.001, 0.01)
}

/// `rim`'s options, with the JS defaults.
#[derive(Clone, Copy, Debug)]
pub struct RimOpts {
    pub f: f64,
    pub body: f64,
    pub len: f64,
    pub q: f64,
}

impl Default for RimOpts {
    fn default() -> Self {
        RimOpts {
            f: 1750.0,
            body: 480.0,
            len: 0.08,
            q: 4.0,
        }
    }
}

/// Rim-shot / finger snap: a short resonant click.
fn rim(sr: f64, o: RimOpts, rnd: Rnd) -> Vec<Vec<f32>> {
    let n = len_of(o.len, sr);
    let mut d = vec![0f32; n];
    let mut bp = Biquad::new(BqType::Bp, o.f, o.q, sr);
    for (i, out) in d.iter_mut().enumerate() {
        let t = i as f64 / sr;
        *out = f32s(
            bp.p(bipolar(rnd)) * exp(-t / 0.01) * 4.0 + sin(TAU * o.body * t) * exp(-t / 0.012),
        );
    }
    finish(vec![d], sr, 0.8, 0.0002, 0.005)
}

/// Sub drop for the downbeat of a drop: a long falling sine and a noise swell.
fn boom(sr: f64, rnd: Rnd) -> Vec<Vec<f32>> {
    let n = len_of(2.4, sr);
    let mut l = vec![0f32; n];
    let mut r = vec![0f32; n];
    let mut lp = Biquad::new(BqType::Lp, 300.0, 0.7, sr);
    let mut lp2 = Biquad::new(BqType::Lp, 300.0, 0.7, sr);
    let mut ph = 0.0;
    for i in 0..n {
        let t = i as f64 / sr;
        ph += (TAU * (32.0 + 40.0 * exp(-t / 0.25))) / sr;
        let s = sin(ph) * exp(-t / 0.7);
        let e = exp(-t / 0.35);
        l[i] = f32s(sat(s + lp.p(bipolar(rnd)) * e * 1.5, 1.4));
        r[i] = f32s(sat(s + lp2.p(bipolar(rnd)) * e * 1.5, 1.4));
    }
    finish_default(vec![l, r], sr, 0.9)
}

fn reversed(chs: &[Vec<f32>]) -> Vec<Vec<f32>> {
    chs.iter()
        .map(|d| d.iter().rev().copied().collect())
        .collect()
}

/// `toBuffer(ctx, chs)`: an AudioBuffer with these channels.
pub fn to_buffer(ctx: &AudioContext, chs: &[Vec<f32>]) -> AudioBuffer {
    let b = ctx.create_buffer(chs.len() as u32, chs[0].len() as u32, ctx.sample_rate());
    for (c, d) in chs.iter().enumerate() {
        b.copy_to_channel(d, c as u32);
    }
    b
}

/// Named channel sets, in the JS object's key order.
pub type Named = Vec<(&'static str, Vec<Vec<f32>>)>;

/// The whole drum kit's sample data, keyed by name (`renderKit` before
/// `toBuffer`).
pub fn kit_data(sr: f64) -> Named {
    let r = &mut rng_from(1234);
    let crash = metal(
        sr,
        MetalOpts {
            decay: 0.85,
            len: 2.6,
            hp_f: 3600.0,
            mix: 0.7,
            scale: 1.9,
            attack: 0.002,
            darken: 0.6,
            ..Default::default()
        },
        r,
        true,
    );
    let kick_o = |f0, f1, pt, decay, len, click, drive, tone| KickOpts {
        f0,
        f1,
        pt,
        decay,
        len,
        click,
        drive,
        tone,
    };
    // (Each entry is rendered in the JS object literal's order: they all
    // draw from `r`.)
    let mut kit: Named = vec![
        (
            "kickPunch",
            kick(
                sr,
                kick_o(175.0, 49.0, 0.032, 0.26, 0.55, 0.55, 1.8, 0.0),
                r,
            ),
        ),
        (
            "kickBoom",
            kick(sr, kick_o(150.0, 42.0, 0.05, 0.55, 1.1, 0.35, 2.2, 0.2), r),
        ),
        (
            "kickTight",
            kick(sr, kick_o(230.0, 56.0, 0.022, 0.16, 0.4, 0.8, 2.0, 0.0), r),
        ),
        (
            "kickSoft",
            kick(sr, kick_o(120.0, 50.0, 0.04, 0.24, 0.5, 0.12, 1.1, 0.0), r),
        ),
        (
            "kickHouse",
            kick(sr, kick_o(160.0, 52.0, 0.028, 0.3, 0.6, 0.45, 2.4, 0.1), r),
        ),
    ];
    let snare_o = |body, decay, noise, bright, gate, gate_len, crack, len| SnareOpts {
        body,
        decay,
        noise,
        bright,
        gate,
        gate_len,
        crack,
        len,
    };
    kit.push((
        "snareGated",
        snare(
            sr,
            snare_o(180.0, 0.12, 0.9, 7000.0, 0.75, 0.27, 0.5, 0.42),
            r,
            true,
        ),
    ));
    kit.push((
        "snareCrisp",
        snare(
            sr,
            snare_o(225.0, 0.09, 1.0, 9500.0, 0.0, 0.26, 1.0, 0.3),
            r,
            true,
        ),
    ));
    kit.push((
        "snareFat",
        snare(
            sr,
            snare_o(170.0, 0.2, 1.1, 6000.0, 0.4, 0.18, 0.6, 0.5),
            r,
            true,
        ),
    ));
    kit.push((
        "snareSoft",
        snare(
            sr,
            snare_o(200.0, 0.08, 0.55, 4200.0, 0.0, 0.26, 0.2, 0.25),
            r,
            true,
        ),
    ));
    kit.push(("clap", clap(sr, ClapOpts::default(), r)));
    kit.push((
        "clapBig",
        clap(
            sr,
            ClapOpts {
                tail: 0.3,
                freq: 1000.0,
                len: 0.8,
            },
            r,
        ),
    ));
    let metal_o = |decay, len, hp_f, mix| MetalOpts {
        decay,
        len,
        hp_f,
        mix,
        ..Default::default()
    };
    kit.push(("hat", metal(sr, metal_o(0.022, 0.1, 7500.0, 0.4), r, true)));
    kit.push((
        "hatSoft",
        metal(sr, metal_o(0.018, 0.08, 8500.0, 0.7), r, true),
    ));
    kit.push(("ohat", metal(sr, metal_o(0.2, 0.55, 6500.0, 0.45), r, true)));
    kit.push((
        "ride",
        metal(
            sr,
            MetalOpts {
                scale: 1.4,
                bell: 0.8,
                ..metal_o(0.7, 1.8, 4200.0, 0.3)
            },
            r,
            true,
        ),
    ));
    let rev_crash = reversed(&crash);
    kit.push(("crash", crash));
    kit.push(("revCrash", rev_crash));
    kit.push(("shaker", shaker(sr, r)));
    kit.push(("rim", rim(sr, RimOpts::default(), r)));
    kit.push((
        "snap",
        rim(
            sr,
            RimOpts {
                f: 2300.0,
                body: 900.0,
                q: 2.5,
                len: 0.07,
            },
            r,
        ),
    ));
    let tom_o = |f, decay, pan| TomOpts {
        f,
        decay,
        pan,
        ..Default::default()
    };
    kit.push(("tomL", tom(sr, tom_o(82.0, 0.32, 0.35), r)));
    kit.push(("tomM", tom(sr, tom_o(116.0, 0.28, 0.0), r)));
    kit.push(("tomH", tom(sr, tom_o(158.0, 0.24, -0.35), r)));
    kit.push(("boom", boom(sr, r)));
    kit
}

/// `renderKit(ctx)`: the whole drum kit, as AudioBuffers keyed by name.
pub fn render_kit(ctx: &AudioContext) -> Vec<(&'static str, AudioBuffer)> {
    kit_data(ctx.sample_rate())
        .into_iter()
        .map(|(k, v)| (k, to_buffer(ctx, &v)))
        .collect()
}

// ── SFX one-shots ────────────────────────────────────────────────

/// `metalHit`'s options, with the JS defaults.
#[derive(Clone, Copy, Debug)]
pub struct MetalHitOpts {
    pub len: f64,
    pub lo: f64,
    pub hi: f64,
    pub crumple: f64,
}

impl Default for MetalHitOpts {
    fn default() -> Self {
        MetalHitOpts {
            len: 1.0,
            lo: 160.0,
            hi: 4200.0,
            crumple: 0.25,
        }
    }
}

struct Mode {
    f: f64,
    d: f64,
    a: f64,
}

/// Crumpled sheet metal: a handful of inharmonic panel modes rung by a noisy
/// excitation that keeps crackling (the crumple) for a moment.
fn metal_hit(sr: f64, seed: u32, o: MetalHitOpts) -> Vec<Vec<f32>> {
    let r = &mut rng_from(seed);
    let n = len_of(o.len, sr);
    let mut out = Vec::new();
    let mut modes = Vec::new();
    for _k in 0..14 {
        let f = o.lo * pow(o.hi / o.lo, r.next_f64());
        let d = 0.04 + 0.5 * pow(o.lo / f, 0.6) * r.next_f64();
        let a = 0.4 + r.next_f64();
        modes.push(Mode { f, d, a });
    }
    for c in 0..2 {
        let mut d = vec![0f32; n];
        let det = 1.0 + (if c != 0 { 0.006 } else { -0.006 });
        let mut bps: Vec<Biquad> = modes
            .iter()
            .map(|m| Biquad::new(BqType::Bp, m.f * det, 30.0 + m.f / 60.0, sr))
            .collect();
        let mut grit = Biquad::new(BqType::Bp, 2400.0, 0.8, sr);
        let mut crk = 0.0;
        for (i, out) in d.iter_mut().enumerate() {
            let t = i as f64 / sr;
            // Excitation: a hard hit, then random crackle bursts while it crumples.
            if t < o.crumple && r.next_f64() < 0.0025 {
                crk = 0.6 + r.next_f64();
            }
            crk *= 0.993;
            let w = bipolar(r);
            let fade = if 1.0 - t / o.crumple > 0.0 {
                1.0 - t / o.crumple
            } else {
                0.0
            };
            let ex = w * (exp(-t / 0.006) * 3.0 + crk * fade);
            let mut s = 0.0;
            for (k, m) in modes.iter().enumerate() {
                s += bps[k].p(ex) * m.a * exp(-t / m.d) * 6.0;
            }
            s += grit.p(w) * (exp(-t / 0.05) + crk * 0.3) * 0.7;
            *out = f32s(sat(s, 1.6));
        }
        out.push(d);
    }
    finish(out, sr, 0.9, 0.0002, 0.05)
}

/// Glass: a sharp shatter and then pieces landing — dozens of tiny high
/// ringing tinkles thinning out over half a second.
fn glass(sr: f64, seed: u32, len: f64) -> Vec<Vec<f32>> {
    let r = &mut rng_from(seed);
    let n = len_of(len, sr);
    let mut l = vec![0f32; n];
    let mut rr = vec![0f32; n];
    let mut hp = Biquad::new(BqType::Hp, 3500.0, 0.7, sr);
    for i in 0..len_of(0.08, sr) {
        let t = i as f64 / sr;
        let s = hp.p(bipolar(r)) * exp(-t / 0.02);
        l[i] = f32s(l[i] as f64 + s);
        rr[i] = f32s(rr[i] as f64 + s * 0.8);
    }
    for _k in 0..55 {
        let t0 = pow(r.next_f64(), 1.8) * (len * 0.7);
        let f = 2800.0 + r.next_f64() * 6500.0;
        let dec = 0.01 + r.next_f64() * 0.05;
        let a = (0.15 + r.next_f64() * 0.5) * (1.0 - t0 / len);
        let pan = bipolar(r);
        let i0 = (t0 * sr).floor() as usize;
        let m = (dec * 6.0 * sr).floor() as usize;
        let mut i = 0;
        while i < m && i0 + i < n {
            let t = i as f64 / sr;
            let s = (sin(TAU * f * t) + 0.5 * sin(TAU * f * 2.37 * t)) * exp(-t / dec) * a;
            l[i0 + i] = f32s(l[i0 + i] as f64 + s * (1.0 - pan) * 0.5);
            rr[i0 + i] = f32s(rr[i0 + i] as f64 + s * (1.0 + pan) * 0.5);
            i += 1;
        }
    }
    finish_default(vec![l, rr], sr, 0.8)
}

/// Seamless looping textures: generate twice as long, filter it all so the
/// filters are settled, and keep the second half (whose end runs straight
/// back into its start).
fn loop_texture(
    sr: f64,
    secs: f64,
    seed: u32,
    mut gen_: impl FnMut(usize, Rnd, usize) -> Vec<f32>,
) -> Vec<Vec<f32>> {
    let n = len_of(secs, sr);
    let r = &mut rng_from(seed);
    let mut out = Vec::new();
    for c in 0..2 {
        let full = gen_(n, r, c);
        out.push(full[n..2 * n].to_vec());
    }
    // Cross-fade the seam anyway; grains straddling it would otherwise click.
    let f = 1024;
    for d in out.iter_mut() {
        for i in 0..f {
            let t = i as f64 / f as f64;
            d[n - f + i] = f32s(d[n - f + i] as f64 * (1.0 - t) + d[i] as f64 * t);
        }
    }
    finish(out, sr, 0.8, 0.0, 0.0)
}

/// Gravel under the tyres: dense random stone clicks of different sizes over a
/// bed of low crunch. Played faster at speed.
fn gravel(sr: f64) -> Vec<Vec<f32>> {
    loop_texture(sr, 2.0, 77, |n, r, _c| {
        let mut x = vec![0f32; 2 * n];
        // Grains land in the kept (second) half and wrap around its end.
        let grains = (n as f64 / sr * 900.0).floor() as usize;
        for _k in 0..grains {
            let at = (r.next_f64() * n as f64).floor() as usize;
            let size = r.next_f64();
            let f = 700.0 + (1.0 - size) * 4200.0;
            let dec = 0.001 + size * 0.004;
            let a = 0.2 + size * 0.8;
            let mut bp = Biquad::new(BqType::Bp, f, 2.5, sr);
            let m = (dec * 5.0 * sr).floor() as usize;
            for i in 0..m {
                let j = n + ((at + i) % n);
                x[j] = f32s(x[j] as f64 + bp.p(bipolar(r)) * exp(-(i as f64) / sr / dec) * a);
            }
        }
        let mut lp = Biquad::new(BqType::Lp, 380.0, 0.7, sr);
        let mut br = 0.0;
        for v in x.iter_mut() {
            br = br * 0.97 + bipolar(r) * 0.03;
            *v = f32s(*v as f64 * 1.4 + lp.p(br) * 5.0);
        }
        x
    })
}

/// Metal on concrete: a grinding excitation through a few ringing panel modes.
fn scrape(sr: f64) -> Vec<Vec<f32>> {
    loop_texture(sr, 1.5, 91, |n, r, c| {
        let mut x = vec![0f32; 2 * n];
        let mut modes: Vec<Biquad> = [1450.0, 2330.0, 3170.0, 4480.0, 6100.0]
            .iter()
            .map(|f| Biquad::new(BqType::Bp, f * (1.0 + c as f64 * 0.01), 25.0, sr))
            .collect();
        let mut grit = Biquad::new(BqType::Bp, 2800.0, 0.7, sr);
        let mut lp_a = Biquad::new(BqType::Lp, 30.0, 0.7, sr);
        for v in x.iter_mut() {
            // Grind intensity wobbles quickly and randomly (the car bounces along the wall).
            let a = js::max(0.0, 0.6 + lp_a.p(bipolar(r) * 40.0));
            let w = bipolar(r) * a;
            let mut s = grit.p(w) * 0.9;
            for m in modes.iter_mut() {
                s += m.p(w) * 3.0;
            }
            *v = f32s(sat(s, 1.8));
        }
        x
    })
}

/// Gearbox clunk: the dog rings engaging — a woody "thock" with a metal tick.
fn clunk(sr: f64) -> Vec<Vec<f32>> {
    let r = &mut rng_from(5);
    let n = len_of(0.16, sr);
    let mut d = vec![0f32; n];
    let mut tick = Biquad::new(BqType::Bp, 3400.0, 3.0, sr);
    for (i, out) in d.iter_mut().enumerate() {
        let t = i as f64 / sr;
        let s = sin(TAU * 165.0 * t) * exp(-t / 0.03)
            + 0.6 * sin(TAU * 430.0 * t) * exp(-t / 0.018)
            + 0.35 * sin(TAU * 960.0 * t) * exp(-t / 0.012)
            + tick.p(bipolar(r)) * exp(-t / 0.003) * 4.0;
        *out = f32s(sat(s, 1.3));
    }
    finish(vec![d], sr, 0.9, 0.0002, 0.01)
}

/// Suspension bottoming out: a deep thump with a spring's metallic ring.
fn thud(sr: f64) -> Vec<Vec<f32>> {
    let r = &mut rng_from(9);
    let n = len_of(0.5, sr);
    let mut d = vec![0f32; n];
    let mut lp = Biquad::new(BqType::Lp, 240.0, 0.8, sr);
    let mut ring = Biquad::new(BqType::Bp, 780.0, 18.0, sr);
    let mut ph = 0.0;
    for (i, out) in d.iter_mut().enumerate() {
        let t = i as f64 / sr;
        ph += (TAU * (46.0 + 70.0 * exp(-t / 0.03))) / sr;
        let w = bipolar(r);
        *out = f32s(sat(
            sin(ph) * exp(-t / 0.16)
                + lp.p(w) * exp(-t / 0.05) * 2.0
                + ring.p(w) * exp(-t / 0.12) * 0.5,
            1.4,
        ));
    }
    finish(vec![d], sr, 0.9, 0.0005, 0.03)
}

/// The SFX sample data, keyed by name (`renderSfx` before `toBuffer`).
pub fn sfx_data(sr: f64) -> Named {
    vec![
        ("metal1", metal_hit(sr, 3, MetalHitOpts::default())),
        (
            "metal2",
            metal_hit(
                sr,
                8,
                MetalHitOpts {
                    lo: 120.0,
                    hi: 3000.0,
                    crumple: 0.35,
                    ..Default::default()
                },
            ),
        ),
        (
            "metal3",
            metal_hit(
                sr,
                13,
                MetalHitOpts {
                    lo: 260.0,
                    hi: 5200.0,
                    crumple: 0.15,
                    len: 0.8,
                },
            ),
        ),
        ("glass1", glass(sr, 4, 1.0)),
        ("glass2", glass(sr, 17, 0.8)),
        ("gravel", gravel(sr)),
        ("scrape", scrape(sr)),
        ("clunk", clunk(sr)),
        ("thud", thud(sr)),
    ]
}

/// `renderSfx(ctx)`: the SFX one-shots and loops, as AudioBuffers keyed by
/// name.
pub fn render_sfx(ctx: &AudioContext) -> Vec<(&'static str, AudioBuffer)> {
    sfx_data(ctx.sample_rate())
        .into_iter()
        .map(|(k, v)| (k, to_buffer(ctx, &v)))
        .collect()
}
