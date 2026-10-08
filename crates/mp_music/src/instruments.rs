//! The lab's instruments (`tools/music-lab/instruments.js`), line for line.
//!
//!   tb303  the acid bassline: one saw/square oscillator into an 18 dB
//!          diode-leaning ladder, a filter envelope whose decay the knob
//!          sets, accent (shorter, harder envelope; louder; and the accent
//!          capacitor that builds up over consecutive accents: the "wow"),
//!          slide (no retrigger, fixed-time portamento).
//!   juno   a Juno-style polysynth: DCO saw + PWM pulse + square sub + noise
//!          per voice, a high-pass, a saturating 4-pole ladder, ADSRs, then
//!          the BBD chorus (I, II, I+II) on the whole instrument. `unison`
//!          up to 7 detuned oscillators per voice (the JP-8000 supersaw,
//!          with its spread: the outer pairs further apart than the inner),
//!          and `vowel` a formant bank after the voices (a choir).
//!   mono   a Moog-style monosynth: up to three drifting oscillators, ladder
//!          with drive, filter and amp ADSRs, glide, vibrato, bend.
//!   fm     a DX-style 4-operator FM voice with a few algorithms (EP, bell,
//!          stack, organ), per-operator envelopes, velocity to index,
//!          feedback.
//!   string a plucked string (Karplus-Strong as Jaffe and Smith extended
//!          it): a pick burst into a tuned delay loop with damping, a pickup
//!          or body resonance, then the amp: drive, a pedal wah, tremolo.
//!          Chicha's surf guitars first; country's telecaster, acoustic,
//!          banjo and upright bass and the harpsichord are parameter sets of
//!          the same model (sound.md 3.5).
//!   drums  TR-808 and TR-909 kits synthesised per hit (kick, snare, clap,
//!          hats, ride, crash, rim, cowbell, toms, shaker, snap, boom,
//!          swell), and a latin kit (congas, bongos, timbales and their
//!          cáscara, campana, güiro, maracas, clave) for chicha.
//!
//! Every voice adds into a stereo pair; instruments own their voices and
//! gate timing (a note knows its length in samples), so the sequencer only
//! fires.
//!
//! An instrument owns its [`Lab`] patch and reads it live, as the JS
//! instrument holds the object the engine mutates: section automation
//! writes through [`Instrument::lab_mut`]. The kit tables (`KITS`,
//! `SAMPLE_TWEAK`) are generated into [`crate::kits`]; `BPATCH` is
//! [`crate::patches`]. Not ported: the page's knob tables (`KNOBS`,
//! `FM_OP_KNOBS`, `DRUM_KNOBS`) and `patchName` / `voiceTrack`, which map
//! the JS game's tracks to patches (this crate does not hold those tracks
//! yet).

use crate::dsp::{
    Adsr, Chorus, Decay, Drift, Ladder, OSC_PULSE, OSC_SAW, OSC_SUB, OSC_TRI, OnePole, Osc, Rng,
    Sine, Svf, TAU, clamp, mtof, sin1, tanh,
};
use crate::json::Val;
use crate::track::{Lab, OscSpec};
use crate::{js_round, to_i32, to_u32};
use std::sync::OnceLock;

fn cents(c: f64) -> f64 {
    2f64.powf(c / 1200.0)
}

/// JS truthiness of a numeric parameter: `if (p.drive)` is false for a
/// missing key, 0 and NaN.
fn tru(o: Option<f64>) -> Option<f64> {
    o.filter(|v| *v != 0.0 && !v.is_nan())
}

fn truthy(o: Option<f64>) -> bool {
    tru(o).is_some()
}

/// `Math.sign`: 0 for 0 (Rust's `signum` gives 1).
fn js_sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        x
    }
}

/// `trigger`'s options (`opt.glideFrom`, `opt.accent`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TriggerOpt {
    pub glide_from: Option<f64>,
    pub accent: bool,
}

// ── TB-303 ───────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct Tb303 {
    p: Lab,
    osc: Osc,
    f: Ladder,
    fenv: Decay,
    /// The accent capacitor.
    acc: f64,
    amp: f64,
    gate: bool,
    left: i32,
    hz: f64,
    target: f64,
    r: Rng,
    drift: Drift,
    hp_in: OnePole,
    dc: OnePole,
    accent: f64,
    k: u32,
    hzc: Option<f64>,
    /// Set by `trigger`, read by nothing (as in the JS).
    pub amp_target: f64,
    vel: f64,
    sr: f64,
}

impl Tb303 {
    /// The JS default is `seed = 1`.
    pub fn new(patch: Lab, seed: u32, sr: f64) -> Tb303 {
        let mut r = Rng::new(seed);
        let drift = Drift::new(&mut r, 2.0, 0.6, sr);
        Tb303 {
            p: patch,
            osc: Osc::new(0.0),
            f: Ladder::new(),
            fenv: Decay::new(0.3, sr),
            acc: 0.0,
            amp: 0.0,
            gate: false,
            left: 0,
            hz: 110.0,
            target: 110.0,
            r,
            drift,
            hp_in: OnePole::new(30.0, sr),
            dc: OnePole::new(20.0, sr),
            accent: 0.0,
            k: 0,
            hzc: None,
            amp_target: 0.0,
            vel: 0.0,
            sr,
        }
    }

    pub fn trigger(&mut self, midis: &[f64], vel: f64, dur_s: f64, opt: &TriggerOpt) {
        let Some(&m0) = midis.first() else { return };
        let p = &self.p;
        let m = m0 + 12.0 * p.oct.unwrap_or(0.0);
        self.target = mtof(m);
        // A slide carries the gate over: no new envelope, the pitch glides.
        let sliding = opt.glide_from.is_some() && self.gate;
        if !sliding {
            self.hz = self.target;
            self.accent = if opt.accent { 1.0 } else { 0.0 };
            self.fenv.set(if self.accent != 0.0 {
                0.2
            } else {
                p.decay.unwrap_or(0.4)
            });
            self.fenv.hit(1.0);
            self.amp_target = 1.0;
        }
        self.gate = true;
        self.left = to_i32(dur_s).max(1);
        self.vel = vel;
    }

    /// Adds into `out`.
    pub fn run(&mut self, out: &mut [f64; 2]) {
        let sr = self.sr;
        let p = &self.p;
        if !self.gate && self.amp < 1e-5 {
            return;
        }
        if self.left > 0 {
            self.left -= 1;
            if self.left == 0 {
                self.gate = false;
            }
        }
        let env = self.fenv.run();
        let acc_amt = p.accent.unwrap_or(0.6) * self.accent;
        // Control rate (every 16 samples): pitch, the accent capacitor,
        // cutoff.
        self.k = (self.k + 1) & 15;
        if self.k == 0 || self.hzc.is_none() {
            // Fixed-time slide (about 60 ms on the hardware), exponential.
            self.hz += (self.target - self.hz)
                * (1.0 - (-16.0 / (p.glide.unwrap_or(0.06) * sr / 3.0)).exp());
            self.hzc = Some(self.hz * cents(self.drift.run(&mut self.r, 16)));
            // The accent capacitor charges from the accented envelope and
            // leaks slowly.
            self.acc += (env * acc_amt - self.acc)
                * (if env * acc_amt > self.acc {
                    0.014
                } else {
                    0.002
                });
            let cut = p.cutoff.unwrap_or(400.0)
                * 2f64
                    .powf(p.env.unwrap_or(0.6) * 5.5 * env + acc_amt * 1.2 * env + self.acc * 3.0);
            self.f.set(
                cut.min(16000.0),
                clamp(p.res.unwrap_or(0.7) * 0.97 + self.acc * 0.1, 0.0, 1.0),
                sr,
            );
        }
        let hz = self.hzc.unwrap_or(self.hz);
        let x = if p.wave.as_deref() == Some("square") {
            self.osc.run(hz, 0.5, OSC_PULSE, sr);
            self.osc.pulse
        } else {
            self.osc.run(hz, 0.5, 1, sr)
        };
        let y = self
            .f
            .run(self.hp_in.hp(x) * 0.7, 1.0 + p.drive.unwrap_or(0.6), 1);
        // VCA: fast attack, slow droop while the gate is held, 8 ms release.
        let tgt = if self.gate { 1.0 + acc_amt * 0.7 } else { 0.0 };
        let k = if self.gate {
            if self.amp < tgt { 0.02 } else { 0.00003 }
        } else {
            1.0 - (-1.0 / (0.008 * sr / 4.0)).exp()
        };
        self.amp += (tgt - self.amp) * k;
        let mut s = y * self.amp * p.gain.unwrap_or(0.3) * (0.75 + 0.25 * self.vel);
        s = tanh(s * 1.4) / 1.4;
        s -= self.dc.lp(s);
        out[0] += s;
        out[1] += s;
    }

    pub fn active(&self) -> bool {
        self.gate || self.amp > 1e-5
    }
}

// ── Juno-style poly voice ────────────────────────────────────────
#[derive(Clone, Debug)]
struct JunoVoice {
    r: Rng,
    osc: [Osc; 7],
    drift: [Drift; 7],
    f: Ladder,
    hp: OnePole,
    env: Adsr,
    fenv: Adsr,
    left: i32,
    age: u64,
    midi: f64,
    on: bool,
    /// Per-voice component spread.
    cut_var: f64,
    inc: [f64; 7],
    k: u32,
    pw: f64,
    vel: f64,
    hz: f64,
    hz0: f64,
    glide_k: f64,
    cur: f64,
    vib_t: f64,
    sr: f64,
}

impl JunoVoice {
    fn new(seed: u32, sr: f64) -> JunoVoice {
        let mut r = Rng::new(seed);
        let mut osc = [Osc::default(); 7];
        for o in &mut osc {
            *o = Osc::new(r.next());
        }
        let mut drift = [Drift::new(&mut Rng::new(0), 0.0, 0.6, sr); 7];
        for d in &mut drift {
            *d = Drift::new(&mut r, 3.5, 0.6, sr);
        }
        let cut_var = 1.0 + (r.next() * 2.0 - 1.0) * 0.06;
        JunoVoice {
            r,
            osc,
            drift,
            f: Ladder::new(),
            hp: OnePole::new(20.0, sr),
            env: Adsr::new(sr),
            fenv: Adsr::new(sr),
            left: 0,
            age: 0,
            midi: -1.0,
            on: false,
            cut_var,
            inc: [0.0; 7],
            k: 0,
            pw: 0.5,
            vel: 0.0,
            hz: 0.0,
            hz0: 0.0,
            glide_k: 0.0,
            cur: 0.0,
            vib_t: 0.0,
            sr,
        }
    }

    fn start(&mut self, p: &Lab, midi: f64, vel: f64, dur_s: f64, glide_from: Option<f64>, t: u64) {
        let sr = self.sr;
        self.midi = midi;
        self.vel = vel;
        self.left = to_i32(dur_s).max(1);
        self.age = t;
        self.on = true;
        self.hz = mtof(midi + 12.0 * p.oct.unwrap_or(0.0));
        self.hz0 = if let Some(g) = glide_from {
            mtof(g + 12.0 * p.oct.unwrap_or(0.0))
        } else if let Some(b) = tru(p.bend) {
            self.hz * 2f64.powf(-b / 12.0)
        } else {
            self.hz
        };
        let gt = if glide_from.is_some() {
            p.glide
        } else {
            p.bend_t
        };
        self.glide_k = 1.0 - (-1.0 / (gt.unwrap_or(0.06) * sr / 3.0)).exp();
        self.cur = self.hz0;
        self.env.set_all(
            p.a.unwrap_or(0.005),
            p.d.unwrap_or(0.3),
            p.s.unwrap_or(0.7),
            p.r.unwrap_or(0.2),
        );
        self.env.on();
        self.fenv.set_all(
            p.fa.or(p.a).unwrap_or(0.005),
            p.fd.or(p.d).unwrap_or(0.3),
            p.fs.or(p.s).unwrap_or(0.7),
            p.fr.or(p.r).unwrap_or(0.2),
        );
        self.fenv.on();
        self.hp.set_hz(p.hpf.unwrap_or(20.0));
        self.vib_t = 0.0;
        self.k = 0;
    }

    fn done(&self) -> bool {
        !self.on
    }

    fn run(&mut self, p: &Lab, lfo: f64, vib: f64) -> f64 {
        let sr = self.sr;
        if self.left > 0 {
            self.left -= 1;
            if self.left == 0 {
                self.env.off();
                self.fenv.off();
            }
        }
        let a = self.env.run();
        if self.env.done() {
            self.on = false;
            return 0.0;
        }
        // `n` stays a double: the JS loops `i < n` and divides by `n - 1`.
        let n = p.unison.unwrap_or(1.0).min(7.0);
        let fe = self.fenv.run();
        self.k = (self.k + 1) & 15;
        if self.k == 1 {
            self.cur += (self.hz - self.cur) * (1.0 - (1.0 - self.glide_k).powf(16.0));
            self.vib_t += 16.0 / sr;
            let vd = p.vib_delay.unwrap_or(0.3);
            let vib_amt = if vib != 0.0 && !vib.is_nan() && self.vib_t > vd {
                ((self.vib_t - vd) / 0.25).min(1.0) * vib
            } else {
                0.0
            };
            let det = p.detune.unwrap_or(0.0);
            let mut i = 0usize;
            while (i as f64) < n {
                // Spread over ±detune/2, the outer oscillators further apart.
                let u = if n > 1.0 {
                    (i as f64 / (n - 1.0)) * 2.0 - 1.0
                } else {
                    0.0
                };
                let c = js_sign(u) * u.abs().powf(1.3) * det * 0.5
                    + self.drift[i].run(&mut self.r, 16)
                    + vib_amt;
                self.inc[i] = self.cur * cents(c);
                i += 1;
            }
            let kt = 2f64.powf(((self.midi - 60.0) / 12.0) * p.keytrack.unwrap_or(0.5));
            let cut = p.cutoff.unwrap_or(2000.0)
                * self.cut_var
                * kt
                * 2f64.powf(p.fenv.unwrap_or(0.0) * fe * (0.5 + 0.5 * self.vel));
            self.f.set(cut.min(18000.0), p.res.unwrap_or(0.1), sr);
            self.pw = clamp(
                p.pw.unwrap_or(0.5) + p.pwm.unwrap_or(0.0) * lfo * 0.45,
                0.05,
                0.95,
            );
        }
        let mut x = 0.0;
        let pw = self.pw;
        let saw = p.saw.unwrap_or(1.0);
        let m = (if saw != 0.0 && !saw.is_nan() {
            OSC_SAW
        } else {
            0
        }) | (if truthy(p.pulse) { OSC_PULSE } else { 0 });
        let sub = truthy(p.sub);
        let mut i = 0usize;
        while (i as f64) < n {
            let o = &mut self.osc[i];
            o.run(
                self.inc[i],
                pw,
                if i == 0 && sub { m | OSC_SUB } else { m },
                sr,
            );
            x += o.saw * saw
                + o.pulse * p.pulse.unwrap_or(0.0)
                + (if i == 0 {
                    o.sub * p.sub.unwrap_or(0.0)
                } else {
                    0.0
                });
            i += 1;
        }
        x /= n.sqrt();
        if let Some(noise) = tru(p.noise) {
            x += (self.r.next() * 2.0 - 1.0) * noise;
        }
        x = self.hp.hp(x);
        let y = self.f.run(x * 0.5, 1.0 + p.drive.unwrap_or(0.2), 0);
        y * a * self.vel
    }
}

// ── Moog-style mono voice ────────────────────────────────────────
#[derive(Clone, Debug)]
struct MonoVoice {
    r: Rng,
    osc: [Osc; 3],
    sine: Sine,
    drift: [Drift; 3],
    f: Ladder,
    env: Adsr,
    fenv: Adsr,
    on: bool,
    left: i32,
    age: u64,
    cur: f64,
    hz: f64,
    vib_t: f64,
    breath: OnePole,
    inc: [f64; 3],
    k: u32,
    midi: f64,
    vel: f64,
    glide_k: f64,
    sr: f64,
}

/// `p.osc ?? [{ w: 'saw' }]`.
fn default_oscs() -> &'static [OscSpec] {
    static D: OnceLock<Vec<OscSpec>> = OnceLock::new();
    D.get_or_init(|| {
        vec![OscSpec {
            w: "saw".into(),
            ..Default::default()
        }]
    })
}

impl MonoVoice {
    fn new(seed: u32, sr: f64) -> MonoVoice {
        let mut r = Rng::new(seed);
        let mut osc = [Osc::default(); 3];
        for o in &mut osc {
            *o = Osc::new(r.next());
        }
        let mut drift = [Drift::new(&mut Rng::new(0), 0.0, 0.6, sr); 3];
        for d in &mut drift {
            *d = Drift::new(&mut r, 2.5, 0.6, sr);
        }
        MonoVoice {
            r,
            osc,
            sine: Sine::new(0.0),
            drift,
            f: Ladder::new(),
            env: Adsr::new(sr),
            fenv: Adsr::new(sr),
            on: false,
            left: 0,
            age: 0,
            cur: 0.0,
            hz: 0.0,
            vib_t: 0.0,
            breath: OnePole::new(2500.0, sr),
            inc: [0.0; 3],
            k: 0,
            midi: 0.0,
            vel: 0.0,
            glide_k: 0.0,
            sr,
        }
    }

    fn start(&mut self, p: &Lab, midi: f64, vel: f64, dur_s: f64, glide_from: Option<f64>) {
        let sr = self.sr;
        let legato = glide_from.is_some() && self.on && !self.env.done();
        self.midi = midi;
        self.vel = vel;
        self.left = to_i32(dur_s).max(1);
        self.on = true;
        self.hz = mtof(midi + 12.0 * p.oct.unwrap_or(0.0));
        if !legato {
            self.cur = if let Some(b) = tru(p.bend) {
                self.hz * 2f64.powf(-b / 12.0)
            } else {
                self.hz
            };
            self.glide_k = 1.0 - (-1.0 / (p.bend_t.unwrap_or(0.06) * sr / 3.0)).exp();
            self.env.set_all(
                p.a.unwrap_or(0.005),
                p.d.unwrap_or(0.3),
                p.s.unwrap_or(0.8),
                p.r.unwrap_or(0.15),
            );
            self.env.on();
            self.fenv.set_all(
                p.fa.unwrap_or(0.003),
                p.fd.unwrap_or(0.3),
                p.fs.unwrap_or(0.3),
                p.fr.or(p.r).unwrap_or(0.15),
            );
            self.fenv.on();
            self.vib_t = 0.0;
            self.k = 0;
        } else {
            // `p.glide || 0.06`: a glide of 0 is missing too.
            self.glide_k = 1.0 - (-1.0 / (tru(p.glide).unwrap_or(0.06) * sr / 3.0)).exp();
            if self.env.st == 4 {
                self.env.st = 2;
                self.fenv.st = 2;
            }
        }
    }

    fn done(&self) -> bool {
        !self.on
    }

    fn run(&mut self, p: &Lab) -> f64 {
        let sr = self.sr;
        if self.left > 0 {
            self.left -= 1;
            if self.left == 0 {
                self.env.off();
                self.fenv.off();
            }
        }
        let a = self.env.run();
        if self.env.done() {
            self.on = false;
            return 0.0;
        }
        let oscs: &[OscSpec] = match &p.osc {
            Some(o) => o,
            None => default_oscs(),
        };
        let fe = self.fenv.run();
        self.k = (self.k + 1) & 15;
        if self.k == 1 {
            self.cur += (self.hz - self.cur) * (1.0 - (1.0 - self.glide_k).powf(16.0));
            self.vib_t += 16.0 / sr;
            let vd = p.vib_delay.unwrap_or(0.3);
            let vib = match tru(p.vib) {
                Some(v) if self.vib_t > vd => {
                    ((self.vib_t - vd) / 0.3).min(1.0)
                        * v
                        * (TAU * p.vib_rate.unwrap_or(5.5) * self.vib_t).sin()
                }
                _ => 0.0,
            };
            for i in 0..oscs.len() {
                self.inc[i] = self.cur
                    * cents(oscs[i].det.unwrap_or(0.0) + self.drift[i].run(&mut self.r, 16) + vib)
                    * 2f64.powf(oscs[i].oct.unwrap_or(0.0));
            }
            let kt = 2f64.powf(((self.midi - 60.0) / 12.0) * p.keytrack.unwrap_or(0.4));
            let cut = p.cutoff.unwrap_or(1500.0)
                * kt
                * 2f64.powf(p.fenv.unwrap_or(1.0) * fe * (0.6 + 0.4 * self.vel));
            self.f.set(cut.min(18000.0), p.res.unwrap_or(0.15), sr);
        }
        let mut x = 0.0;
        let sub = truthy(p.sub);
        for i in 0..oscs.len() {
            let spec = &oscs[i];
            let hz = self.inc[i];
            if spec.w == "sine" {
                x += self.sine.run(hz, sr) * spec.lvl.unwrap_or(1.0);
                continue;
            }
            let o = &mut self.osc[i];
            let wave = if spec.w == "pulse" {
                OSC_PULSE
            } else if spec.w == "tri" {
                OSC_TRI
            } else {
                OSC_SAW
            };
            o.run(
                hz,
                spec.pw.unwrap_or(0.5),
                wave | (if i == 0 && sub { OSC_SUB } else { 0 }),
                sr,
            );
            x += (if spec.w == "pulse" {
                o.pulse
            } else if spec.w == "tri" {
                o.tri
            } else {
                o.saw
            }) * spec.lvl.unwrap_or(1.0);
        }
        if let Some(s) = tru(p.sub) {
            x += self.osc[0].sub * s;
        }
        if let Some(nz) = tru(p.noise) {
            let w = self.r.next() * 2.0 - 1.0;
            x += self.breath.lp(w) * nz * (0.4 + a);
        }
        self.f.run(x * 0.45, 1.0 + p.drive.unwrap_or(0.5), 0) * a * self.vel
    }
}

// ── DX-style 4-op FM voice ───────────────────────────────────────
/// Algorithms: `mods[i]` lists the operators that modulate operator i (ops
/// are 0-based here; the DX names are 1-based), `car` the carriers that are
/// heard. Operators run from 3 down to 0, so a modulator's sample is ready
/// for the operator below it.
struct Algo {
    mods: [&'static [usize]; 4],
    car: &'static [usize],
}

const ALGO_EP: Algo = Algo {
    mods: [&[1], &[], &[3], &[]],
    car: &[0, 2],
}; // 2→1, 4→3
const ALGO_PAIR: Algo = Algo {
    mods: [&[1], &[], &[], &[]],
    car: &[0],
}; // 2→1
const ALGO_BELL: Algo = Algo {
    mods: [&[1], &[], &[3], &[]],
    car: &[0, 2],
};
const ALGO_STACK: Algo = Algo {
    mods: [&[1], &[2], &[3], &[]],
    car: &[0],
}; // 4→3→2→1
const ALGO_ORGAN: Algo = Algo {
    mods: [&[], &[], &[], &[]],
    car: &[0, 1, 2, 3],
};

/// `ALGOS[p.algo ?? 'ep']`. An unknown name would throw in the JS; it is
/// the EP here.
fn algo(name: Option<&str>) -> &'static Algo {
    match name.unwrap_or("ep") {
        "pair" => &ALGO_PAIR,
        "bell" => &ALGO_BELL,
        "stack" => &ALGO_STACK,
        "organ" => &ALGO_ORGAN,
        _ => &ALGO_EP,
    }
}

#[derive(Clone, Debug)]
struct FmVoice {
    r: Rng,
    ph: [f64; 4],
    out: [f64; 4],
    env: [Adsr; 4],
    on: bool,
    left: i32,
    fb: f64,
    age: u64,
    k: u32,
    base: f64,
    drift: Drift,
    midi: f64,
    vel: f64,
    hz: f64,
    cur: f64,
    glide_k: f64,
    sr: f64,
}

impl FmVoice {
    fn new(seed: u32, sr: f64) -> FmVoice {
        let mut r = Rng::new(seed);
        let drift = Drift::new(&mut r, 1.5, 0.6, sr);
        FmVoice {
            r,
            ph: [0.0; 4],
            out: [0.0; 4],
            env: [Adsr::new(sr); 4],
            on: false,
            left: 0,
            fb: 0.0,
            age: 0,
            k: 0,
            base: 0.0,
            drift,
            midi: 0.0,
            vel: 0.0,
            hz: 0.0,
            cur: 0.0,
            glide_k: 0.0,
            sr,
        }
    }

    fn start(&mut self, p: &Lab, midi: f64, vel: f64, dur_s: f64, glide_from: Option<f64>) {
        let sr = self.sr;
        self.midi = midi;
        self.vel = vel;
        self.left = to_i32(dur_s).max(1);
        self.on = true;
        self.hz = mtof(midi + 12.0 * p.oct.unwrap_or(0.0));
        self.cur = if let Some(g) = glide_from {
            mtof(g + 12.0 * p.oct.unwrap_or(0.0))
        } else if let Some(b) = tru(p.bend) {
            self.hz * 2f64.powf(-b / 12.0)
        } else {
            self.hz
        };
        let gt = if glide_from.is_some() {
            p.glide
        } else {
            p.bend_t
        };
        self.glide_k = 1.0 - (-1.0 / (gt.unwrap_or(0.06) * sr / 3.0)).exp();
        self.k = 0;
        if let Some(ops) = &p.ops {
            for i in 0..4 {
                let Some(Some(o)) = ops.get(i) else { continue };
                // Higher notes decay faster (rate scaling).
                let rs = 2f64.powf(-((midi - 60.0) / 12.0) * o.rs.unwrap_or(0.3));
                self.env[i].set_all(
                    o.a.unwrap_or(0.002),
                    o.d.unwrap_or(0.5) * rs,
                    o.s.unwrap_or(0.2),
                    o.r.or(p.r).unwrap_or(0.3),
                );
                self.env[i].on();
                self.ph[i] = 0.0;
            }
        }
    }

    fn done(&self) -> bool {
        !self.on
    }

    fn run(&mut self, p: &Lab) -> f64 {
        let sr = self.sr;
        if self.left > 0 {
            self.left -= 1;
            if self.left == 0 {
                for e in &mut self.env {
                    e.off();
                }
            }
        }
        let al = algo(p.algo.as_deref());
        self.k = (self.k + 1) & 15;
        if self.k == 1 {
            self.cur += (self.hz - self.cur) * (1.0 - (1.0 - self.glide_k).powf(16.0));
            self.base = self.cur * cents(self.drift.run(&mut self.r, 16));
        }
        let base = self.base;
        let mut y = 0.0;
        let mut live = false;
        for i in (0..4).rev() {
            let o = match &p.ops {
                Some(ops) => ops.get(i).and_then(|o| o.as_ref()),
                None => None,
            };
            let Some(o) = o else {
                self.out[i] = 0.0;
                continue;
            };
            let e = self.env[i].run();
            if !self.env[i].done() {
                live = true;
            }
            let mut pm = 0.0;
            for &m in al.mods[i] {
                pm += self.out[m];
            }
            if i == 3 {
                pm += self.fb * p.fbk.unwrap_or(0.0);
            }
            self.ph[i] += (base * o.r.unwrap_or(1.0) * cents(o.det.unwrap_or(0.0))
                + o.fix.unwrap_or(0.0))
                / sr;
            if self.ph[i] > 1.0 {
                self.ph[i] -= self.ph[i].floor();
            }
            let s = sin1(self.ph[i] + pm / TAU);
            let is_car = al.car.contains(&i);
            let lvl = if is_car {
                o.l.unwrap_or(1.0)
            } else {
                o.l.unwrap_or(1.0) * (1.0 - o.v.unwrap_or(0.6) + o.v.unwrap_or(0.6) * self.vel)
            };
            self.out[i] = s * e * lvl;
            if i == 3 {
                self.fb = self.out[i];
            }
            if is_car {
                y += self.out[i];
            }
        }
        if !live {
            self.on = false;
            return 0.0;
        }
        (y / (al.car.len() as f64).sqrt()) * self.vel
    }
}

// ── Plucked string ───────────────────────────────────────────────
/// A noise burst one period long goes into a delay loop whose length is the
/// period; a one-zero average in the loop damps the upper partials faster
/// than the lower (`damp`), and a loss per period sets the decay time. The
/// string is read through a resonance (`body`: the pickup's peak on an
/// electric, the top on an acoustic) and a tone control. What a note does:
/// `pick` is how hard the pick is (brighter burst), `decay` the ring time
/// in seconds, `r` how fast it is muted when the note ends, `bend` /
/// `bendT` a slide up into the note, `glide` a slide from the last note
/// when legato, `strum` seconds between the notes of a chord. The amp
/// (drive, wah, tremolo) is the instrument's, in [`Poly`].
#[derive(Clone, Debug)]
struct StringVoice {
    r: Rng,
    /// A `Float32Array`: stores round to `f32`.
    buf: Vec<f32>,
    w: usize,
    d: f64,
    g: f64,
    g_rel: f64,
    damp: f64,
    lp: f64,
    on: bool,
    left: i32,
    age: u64,
    wait: i64,
    rel: bool,
    exc: i64,
    exc_lp: OnePole,
    body: Svf,
    tone: OnePole,
    midi: f64,
    cur: f64,
    hz: f64,
    glide_k: f64,
    k: u32,
    amp: f64,
    vel: f64,
    sr: f64,
}

impl StringVoice {
    fn new(seed: u32, sr: f64) -> StringVoice {
        StringVoice {
            r: Rng::new(seed),
            buf: vec![0.0; (sr / 20.0).ceil() as usize + 8], // down to 20 Hz
            w: 0,
            d: 2.0,
            g: 1.0,
            g_rel: 1.0,
            damp: 0.4,
            lp: 0.0,
            on: false,
            left: 0,
            age: 0,
            wait: 0,
            rel: false,
            exc: 0,
            exc_lp: OnePole::new(4000.0, sr),
            body: Svf::new(),
            tone: OnePole::new(6000.0, sr),
            midi: -1.0,
            cur: 110.0,
            hz: 110.0,
            glide_k: 0.0,
            k: 0,
            amp: 0.0,
            vel: 0.0,
            sr,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn start(
        &mut self,
        p: &Lab,
        midi: f64,
        vel: f64,
        dur_s: f64,
        glide_from: Option<f64>,
        t: u64,
        wait: i64,
    ) {
        let sr = self.sr;
        self.midi = midi;
        self.vel = vel;
        self.left = to_i32(dur_s).max(1);
        self.on = true;
        self.age = t;
        self.wait = wait;
        self.hz = mtof(midi + 12.0 * p.oct.unwrap_or(0.0));
        let legato = glide_from.is_some();
        self.cur = if let Some(g) = glide_from {
            mtof(g + 12.0 * p.oct.unwrap_or(0.0))
        } else if let Some(b) = tru(p.bend) {
            self.hz * 2f64.powf(-b / 12.0)
        } else {
            self.hz
        };
        let gt = if legato { p.glide } else { p.bend_t };
        self.glide_k = 1.0 - (-1.0 / (gt.unwrap_or(0.05) * sr / 3.0)).exp();
        // The pluck: a burst one period long, low-passed by the pick's
        // softness, brighter when hit harder. The loop is cleared: a new
        // pluck on a ringing string restarts it (the voice would be another
        // one if not).
        self.exc = js_round(sr / self.cur).max(2.0) as i64;
        self.exc_lp
            .set_hz(1200.0 + p.pick.unwrap_or(0.5) * 9000.0 * (0.4 + 0.6 * vel));
        self.body
            .set(p.body.unwrap_or(2500.0), p.body_q.unwrap_or(1.2), sr);
        self.tone.set_hz(p.tone.unwrap_or(6000.0));
        self.rel = false;
        self.k = 0;
        self.amp = 1.0;
        self.lp = 0.0;
        self.buf.fill(0.0);
        self.tune(p);
    }

    /// Loop delay: the period less the damping filter's half sample, read
    /// fractionally. Loss per period from the decay time (RT60), and the
    /// release's extra loss once the note has ended.
    fn tune(&mut self, p: &Lab) {
        let sr = self.sr;
        self.d = (sr / self.cur - 0.5).max(2.0);
        self.g = 10f64.powf(-3.0 / (p.decay.unwrap_or(1.5).max(0.02) * self.cur));
        self.g_rel = 10f64.powf(-3.0 / (p.r.unwrap_or(0.3).max(0.01) * self.cur));
        self.damp = clamp(p.damp.unwrap_or(0.4), 0.0, 0.95);
    }

    fn done(&self) -> bool {
        !self.on
    }

    fn run(&mut self, p: &Lab) -> f64 {
        if self.wait > 0 {
            self.wait -= 1;
            return 0.0;
        }
        if self.left > 0 {
            self.left -= 1;
            if self.left == 0 {
                self.rel = true;
            }
        }
        self.k = (self.k + 1) & 15;
        if self.k == 1 && self.cur != self.hz {
            self.cur += (self.hz - self.cur) * (1.0 - (1.0 - self.glide_k).powf(16.0));
            if (self.cur - self.hz).abs() < 1e-3 {
                self.cur = self.hz;
            }
            self.tune(p);
        }
        let n = self.buf.len();
        let mut rp = self.w as f64 - self.d;
        while rp < 0.0 {
            rp += n as f64;
        }
        let i = rp as usize;
        let f = rp - i as f64;
        let y = self.buf[i] as f64 * (1.0 - f) + self.buf[(i + 1) % n] as f64 * f;
        let avg = y * (1.0 - self.damp) + self.lp * self.damp;
        self.lp = y;
        let mut x = avg
            * (if self.rel {
                self.g * self.g_rel
            } else {
                self.g
            });
        if self.exc > 0 {
            self.exc -= 1;
            let w = self.r.next() * 2.0 - 1.0;
            x += self.exc_lp.lp(w) * self.vel * 0.9;
        }
        self.buf[self.w] = x as f32;
        self.w = (self.w + 1) % n;
        self.body.run(x);
        let out = self.tone.lp(x + self.body.bp * p.body_mix.unwrap_or(0.6));
        // Quiet for a while after the pluck: the voice is free.
        let a = out.abs();
        self.amp = if a > self.amp { a } else { self.amp * 0.9995 };
        if self.exc == 0 && self.amp < 2e-4 {
            self.on = false;
        }
        out
    }
}

/// Vowel formants (a tenor's first three), for the choir: centre
/// frequencies and relative levels of three band-passes on the summed
/// voices.
fn vowel(name: &str) -> Option<&'static [[f64; 2]; 3]> {
    const A: [[f64; 2]; 3] = [[650.0, 1.0], [1080.0, 0.5], [2650.0, 0.25]];
    const E: [[f64; 2]; 3] = [[400.0, 1.0], [1700.0, 0.4], [2600.0, 0.25]];
    const I: [[f64; 2]; 3] = [[290.0, 1.0], [1870.0, 0.3], [2800.0, 0.25]];
    const O: [[f64; 2]; 3] = [[400.0, 1.0], [800.0, 0.6], [2600.0, 0.15]];
    const U: [[f64; 2]; 3] = [[350.0, 1.0], [600.0, 0.5], [2700.0, 0.1]];
    match name {
        "a" => Some(&A),
        "e" => Some(&E),
        "i" => Some(&I),
        "o" => Some(&O),
        "u" => Some(&U),
        _ => None,
    }
}

/// The voice a [`Poly`] of a kind holds (the JS picks the class by `kind`).
/// The variants differ in size (the Juno's seven oscillators, the string's
/// buffer); voices are allocated once per instrument, so no boxing.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
enum Voice {
    Juno(JunoVoice),
    Mono(MonoVoice),
    Fm(FmVoice),
    Str(StringVoice),
}

impl Voice {
    fn done(&self) -> bool {
        match self {
            Voice::Juno(v) => v.done(),
            Voice::Mono(v) => v.done(),
            Voice::Fm(v) => v.done(),
            Voice::Str(v) => v.done(),
        }
    }

    fn age(&self) -> u64 {
        match self {
            Voice::Juno(v) => v.age,
            Voice::Mono(v) => v.age,
            Voice::Fm(v) => v.age,
            Voice::Str(v) => v.age,
        }
    }

    fn set_age(&mut self, t: u64) {
        match self {
            Voice::Juno(v) => v.age = t,
            Voice::Mono(v) => v.age = t,
            Voice::Fm(v) => v.age = t,
            Voice::Str(v) => v.age = t,
        }
    }

    /// `v.start(p, midi, vel, durS, glideFrom, t, wait)`: each class takes
    /// the arguments it declares.
    #[allow(clippy::too_many_arguments)]
    fn start(
        &mut self,
        p: &Lab,
        midi: f64,
        vel: f64,
        dur_s: f64,
        glide_from: Option<f64>,
        t: u64,
        wait: i64,
    ) {
        match self {
            Voice::Juno(v) => v.start(p, midi, vel, dur_s, glide_from, t),
            Voice::Mono(v) => v.start(p, midi, vel, dur_s, glide_from),
            Voice::Fm(v) => v.start(p, midi, vel, dur_s, glide_from),
            Voice::Str(v) => v.start(p, midi, vel, dur_s, glide_from, t, wait),
        }
    }
}

/// The poly kinds; a `kind` that is none of these gets Juno voices, as in
/// the JS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Juno,
    Mono,
    Fm,
    Str,
}

/// A polyphonic instrument around any of the poly voices, with an LFO, the
/// chorus and voice stealing. (The JS's unused `this.out` is not kept.)
#[derive(Clone, Debug)]
pub struct Poly {
    p: Lab,
    kind: Kind,
    voices: Vec<Voice>,
    ch: Chorus,
    lfo_ph: f64,
    vib_ph: f64,
    t: u64,
    formant: [Svf; 3],
    /// The string's amp: a pedal wah and the tremolo.
    wah_f: Svf,
    wah_ph: f64,
    trem_ph: f64,
    sr: f64,
}

impl Poly {
    /// The JS defaults are `seed = 1, nVoices = 8`; the mono kind has one
    /// voice whatever `n_voices`.
    pub fn new(kind: &str, patch: Lab, seed: u32, n_voices: usize, sr: f64) -> Poly {
        let k = match kind {
            "fm" => Kind::Fm,
            "mono" => Kind::Mono,
            "string" => Kind::Str,
            _ => Kind::Juno,
        };
        let n = if k == Kind::Mono { 1 } else { n_voices };
        let voices = (0..n)
            .map(|i| {
                let s = to_u32(seed as f64 * 101.0 + i as f64 * 7919.0);
                match k {
                    Kind::Fm => Voice::Fm(FmVoice::new(s, sr)),
                    Kind::Mono => Voice::Mono(MonoVoice::new(s, sr)),
                    Kind::Str => Voice::Str(StringVoice::new(s, sr)),
                    Kind::Juno => Voice::Juno(JunoVoice::new(s, sr)),
                }
            })
            .collect();
        let mut ch = Chorus::new(sr);
        ch.set(patch.chorus.unwrap_or(0.0));
        Poly {
            p: patch,
            kind: k,
            voices,
            ch,
            lfo_ph: 0.0,
            vib_ph: 0.0,
            t: 0,
            formant: [Svf::new(); 3],
            wah_f: Svf::new(),
            wah_ph: 0.0,
            trem_ph: 0.0,
            sr,
        }
    }

    pub fn set_patch(&mut self, p: Lab) {
        self.ch.set(p.chorus.unwrap_or(0.0));
        self.p = p;
    }

    /// The guitar amp: drive (a soft clip), a wah pedal rocked by its own
    /// slow LFO (`wahRate` Hz, a sweep over `wah` octaves from `wahHz`),
    /// then the amp's tremolo (depth `trem`, `tremRate` Hz).
    fn amp(&mut self, mut x: f64) -> f64 {
        let sr = self.sr;
        let p = &self.p;
        if let Some(drive) = tru(p.drive) {
            x = tanh(x * (1.0 + drive)) / (1.0 + drive * 0.35);
        }
        if let Some(wah) = tru(p.wah) {
            self.wah_ph += p.wah_rate.unwrap_or(1.5) / sr;
            if self.wah_ph > 1.0 {
                self.wah_ph -= 1.0;
            }
            if (self.t & 15) == 0 || self.wah_f.hz < 0.0 {
                self.wah_f.set(
                    p.wah_hz.unwrap_or(450.0)
                        * 2f64.powf(wah * (0.5 - 0.5 * (TAU * self.wah_ph).cos())),
                    p.wah_q.unwrap_or(4.0),
                    sr,
                );
            }
            self.wah_f.run(x);
            x = x * 0.3 + self.wah_f.bp * 0.9;
        }
        if let Some(trem) = tru(p.trem) {
            self.trem_ph += p.trem_rate.unwrap_or(5.5) / sr;
            if self.trem_ph > 1.0 {
                self.trem_ph -= 1.0;
            }
            x *= 1.0 - trem * (0.5 - 0.5 * (TAU * self.trem_ph).cos());
        }
        x
    }

    /// The vowel bank: the LFO drifts the formants a little, as a mouth
    /// does.
    fn vowel(&mut self, x: f64, lfo: f64) -> f64 {
        let sr = self.sr;
        let Some(v) = self.p.vowel.as_deref().and_then(vowel) else {
            return x;
        };
        let mut y = 0.0;
        for i in 0..3 {
            let f = &mut self.formant[i];
            if (self.t & 7) == i as u64 || f.hz < 0.0 {
                f.set(
                    v[i][0] * (1.0 + lfo * 0.04),
                    self.p.vowel_q.unwrap_or(9.0),
                    sr,
                );
            }
            f.run(x);
            y += f.bp * v[i][1];
        }
        let mix = self.p.vowel_mix.unwrap_or(0.8);
        x * (1.0 - mix) + y * mix * 2.2
    }

    pub fn trigger(&mut self, midis: &[f64], vel: f64, dur_s: f64, opt: &TriggerOpt) {
        self.t += 1;
        if self.kind == Kind::Mono {
            let Some(&m) = midis.last() else { return };
            self.voices[0].start(&self.p, m, vel, dur_s, opt.glide_from, 0, 0);
            return;
        }
        let gain = 1.0 / (midis.len() as f64).sqrt();
        // A strummed chord: each string a little after the one below it.
        let strum = if self.kind == Kind::Str {
            js_round(self.p.strum.unwrap_or(0.0) * self.sr) as i64
        } else {
            0
        };
        for (i, &m) in midis.iter().enumerate() {
            // A free voice, else the oldest (the first of the oldest).
            let vi = match self.voices.iter().position(|x| x.done()) {
                Some(vi) => vi,
                None => {
                    let mut best = 0;
                    for j in 1..self.voices.len() {
                        if self.voices[best].age() > self.voices[j].age() {
                            best = j;
                        }
                    }
                    best
                }
            };
            let v = &mut self.voices[vi];
            v.start(
                &self.p,
                m,
                vel * gain,
                dur_s,
                opt.glide_from,
                self.t,
                strum * i as i64,
            );
            v.set_age(self.t);
        }
    }

    pub fn active(&self) -> bool {
        self.voices.iter().any(|v| !v.done())
    }

    /// Adds into `out`.
    pub fn run(&mut self, out: &mut [f64; 2]) {
        let sr = self.sr;
        self.lfo_ph += self.p.lfo_rate.unwrap_or(0.6) / sr;
        if self.lfo_ph > 1.0 {
            self.lfo_ph -= 1.0;
        }
        let lfo = if self.lfo_ph < 0.5 {
            self.lfo_ph * 4.0 - 1.0
        } else {
            3.0 - self.lfo_ph * 4.0
        };
        let mut x = 0.0;
        match self.kind {
            Kind::Juno => {
                self.vib_ph += self.p.vib_rate.unwrap_or(5.5) / sr;
                if self.vib_ph > 1.0 {
                    self.vib_ph -= 1.0;
                }
                let vib = match tru(self.p.vib) {
                    Some(v) => v * (TAU * self.vib_ph).sin(),
                    None => 0.0,
                };
                for v in &mut self.voices {
                    if let Voice::Juno(j) = v
                        && !j.done()
                    {
                        x += j.run(&self.p, lfo, vib);
                    }
                }
                if self.p.vowel.as_deref().is_some_and(|s| !s.is_empty()) {
                    self.t += 1;
                    x = self.vowel(x, lfo);
                }
            }
            Kind::Str => {
                for v in &mut self.voices {
                    if let Voice::Str(s) = v
                        && !s.done()
                    {
                        x += s.run(&self.p);
                    }
                }
                self.t += 1;
                x = self.amp(x);
            }
            Kind::Mono | Kind::Fm => {
                for v in &mut self.voices {
                    match v {
                        Voice::Mono(m) if !m.done() => x += m.run(&self.p),
                        Voice::Fm(f) if !f.done() => x += f.run(&self.p),
                        _ => {}
                    }
                }
            }
        }
        self.ch.run(x * self.p.gain.unwrap_or(0.2), out, 1.0);
    }
}

// ── Drums ────────────────────────────────────────────────────────
/// 808 metallic oscillator frequencies (six squares), shared by hats,
/// cymbal and (two of them) the cowbell.
const METAL: [f64; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];
const COWBELL: [f64; 2] = [540.0, 800.0];

/// A kit voice's type (`k.t`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitType {
    Kick808,
    Kick909,
    Tom,
    Boom,
    Snare808,
    Snare909,
    Clap,
    Metal,
    Cymbal,
    Cowbell,
    Rim,
    Shaker,
    Snap,
    Swell,
    Conga,
    Guiro,
}

impl HitType {
    /// The JS name.
    pub fn name(self) -> &'static str {
        match self {
            HitType::Kick808 => "kick808",
            HitType::Kick909 => "kick909",
            HitType::Tom => "tom",
            HitType::Boom => "boom",
            HitType::Snare808 => "snare808",
            HitType::Snare909 => "snare909",
            HitType::Clap => "clap",
            HitType::Metal => "metal",
            HitType::Cymbal => "cymbal",
            HitType::Cowbell => "cowbell",
            HitType::Rim => "rim",
            HitType::Shaker => "shaker",
            HitType::Snap => "snap",
            HitType::Swell => "swell",
            HitType::Conga => "conga",
            HitType::Guiro => "guiro",
        }
    }

    pub fn from_name(s: &str) -> Option<HitType> {
        Some(match s {
            "kick808" => HitType::Kick808,
            "kick909" => HitType::Kick909,
            "tom" => HitType::Tom,
            "boom" => HitType::Boom,
            "snare808" => HitType::Snare808,
            "snare909" => HitType::Snare909,
            "clap" => HitType::Clap,
            "metal" => HitType::Metal,
            "cymbal" => HitType::Cymbal,
            "cowbell" => HitType::Cowbell,
            "rim" => HitType::Rim,
            "shaker" => HitType::Shaker,
            "snap" => HitType::Snap,
            "swell" => HitType::Swell,
            "conga" => HitType::Conga,
            "guiro" => HitType::Guiro,
            _ => return None,
        })
    }
}

/// Declares `KitVoice`'s numeric fields with their JS names, and `get` /
/// `set` / `to_val` over them.
macro_rules! kit_nums {
    ($( $field:ident : $js:literal ),* $(,)?) => {
        /// A kit voice (`KITS[kit][lane]`): its type and the parameters
        /// the hit reads; `None` is a missing key. `rev` is not in the
        /// table: the engine fills it in from the track's kit tweaks.
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct KitVoice {
            pub t: HitType,
            $( pub $field: Option<f64>, )*
            pub rev: Option<f64>,
        }

        impl KitVoice {
            /// Every parameter missing (the generated table spreads over it).
            pub const NONE: KitVoice = KitVoice {
                t: HitType::Kick808,
                $( $field: None, )*
                rev: None,
            };

            /// A numeric parameter by its JS name.
            pub fn get(&self, name: &str) -> Option<f64> {
                match name {
                    $( $js => self.$field, )*
                    _ => None,
                }
            }

            /// Sets a numeric parameter by its JS name.
            pub fn set(&mut self, name: &str, v: f64) {
                match name {
                    $( $js => self.$field = Some(v), )*
                    _ => {}
                }
            }

            /// The lab's object, for the canonical JSON (`rev` omitted: it
            /// is not in the table).
            pub fn to_val(&self) -> Val {
                let mut pairs: Vec<(&str, Option<Val>)> = vec![("t", Val::str(self.t.name()))];
                $( pairs.push(($js, Val::onum(self.$field))); )*
                Val::obj(pairs)
            }
        }
    };
}

kit_nums! {
    tune: "tune", pitch: "pitch", pt: "pt", decay: "decay", click: "click",
    click_hz: "clickHz", drive: "drive", lvl: "lvl", tone: "tone", snappy: "snappy",
    nhp: "nhp", nlp: "nlp", bp: "bp", tail: "tail", bursts: "bursts", spacing: "spacing",
    q: "q", hp: "hp", noise: "noise", bp2: "bp2", decay2: "decay2", f1: "f1", f2: "f2",
    attack: "attack", pan: "pan", gate: "gate", len: "len", rate0: "rate0", rate1: "rate1",
    slap: "slap",
}

/// The tweak keys that replace the base value instead of scaling it.
const ABSOLUTE: &[&str] = &["gate"];

/// `kitVoice(kitName, lane, sample)`: the kit's voice for a lane, tweaked
/// by the game's sample name (`SAMPLE_TWEAK`): a tweak multiplies the base
/// value (1 for `lvl`, 0 for anything else when the base lacks the key);
/// `gate` is absolute. An unknown sample gives the base.
pub fn kit_voice(kit_name: &str, lane: &str, sample: Option<&str>) -> Option<KitVoice> {
    let base = crate::kits::kit(kit_name)?
        .iter()
        .find(|(l, _)| *l == lane)
        .map(|(_, v)| *v)?;
    let Some(tw) = sample.and_then(crate::kits::sample_tweak) else {
        return Some(base);
    };
    let mut out = base;
    for &(k, v) in tw {
        if ABSOLUTE.contains(&k) {
            out.set(k, v);
        } else {
            let cur = out.get(k).unwrap_or(if k == "lvl" { 1.0 } else { 0.0 });
            out.set(k, cur * v);
        }
    }
    Some(out)
}

/// One drum hit. The JS class holds the fields its type needs; here every
/// part is present and only the type's are set up and run.
#[derive(Clone, Debug)]
pub struct Hit {
    pub lane: String,
    /// The JS `type`.
    pub ty: HitType,
    pub k: KitVoice,
    pub vel: f64,
    /// Samples run.
    pub t: u64,
    pub done: bool,
    pub rate: f64,
    pub amp: Decay,
    pub pan: f64,
    sine: Sine,
    click: Svf,
    nz: Decay,
    s1: Sine,
    s2: Sine,
    ndec: Decay,
    nlp: Svf,
    nhp: Svf,
    bp: Svf,
    tail: Decay,
    ph: [f64; 6],
    hp: Svf,
    lo: Svf,
    amp2: Decay,
    att: f64,
    a: Svf,
    b: Svf,
    len: f64,
    nbp: Svf,
    /// The güiro's scalar `this.ph`.
    ph1: f64,
    ex: i64,
    sr: f64,
}

impl Hit {
    /// The JS defaults are `rate = 1, durS = 0`. Draws from `r` for the
    /// metallic voices' six phases.
    fn new(
        ty: HitType,
        k: KitVoice,
        vel: f64,
        r: &mut Rng,
        rate: f64,
        dur_s: usize,
        sr: f64,
    ) -> Hit {
        let mut h = Hit {
            lane: String::new(),
            ty,
            k,
            vel,
            t: 0,
            done: false,
            rate,
            amp: Decay::new(k.decay.unwrap_or(0.3), sr),
            pan: k.pan.unwrap_or(0.0),
            sine: Sine::new(0.0),
            click: Svf::new(),
            nz: Decay::new(0.004, sr),
            s1: Sine::new(0.0),
            s2: Sine::new(0.0),
            ndec: Decay::new(0.15, sr),
            nlp: Svf::new(),
            nhp: Svf::new(),
            bp: Svf::new(),
            tail: Decay::new(0.2, sr),
            ph: [0.0; 6],
            hp: Svf::new(),
            lo: Svf::new(),
            amp2: Decay::new(0.15, sr),
            att: 0.0,
            a: Svf::new(),
            b: Svf::new(),
            len: 0.0,
            nbp: Svf::new(),
            ph1: 0.0,
            ex: 0,
            sr,
        };
        h.amp.hit(1.0);
        match ty {
            HitType::Kick808 | HitType::Kick909 | HitType::Tom | HitType::Boom => {
                h.sine = Sine::new(0.25);
                h.click = Svf::with(k.click_hz.unwrap_or(3000.0), 0.7, sr);
                h.nz = Decay::new(0.004, sr);
                h.nz.hit(1.0);
            }
            HitType::Snare808 | HitType::Snare909 => {
                h.s1 = Sine::new(0.25);
                h.s2 = Sine::new(0.1);
                h.ndec = Decay::new(k.snappy.unwrap_or(0.15), sr);
                h.ndec.hit(1.0);
                h.nlp = Svf::with(k.nlp.unwrap_or(7000.0), 0.6, sr);
                h.nhp = Svf::with(k.nhp.unwrap_or(1200.0), 0.6, sr);
            }
            HitType::Clap => {
                h.bp = Svf::with(k.bp.unwrap_or(1100.0), 1.6, sr);
                h.tail = Decay::new(k.tail.unwrap_or(0.2), sr);
            }
            HitType::Metal | HitType::Cymbal | HitType::Cowbell => {
                for p in &mut h.ph {
                    *p = r.next();
                }
                h.bp = Svf::with(k.bp.unwrap_or(10000.0), k.q.unwrap_or(0.9), sr);
                h.hp = Svf::with(k.hp.unwrap_or(7000.0), 0.7, sr);
                if ty == HitType::Cymbal {
                    h.lo = Svf::with(k.bp2.unwrap_or(4000.0), 1.2, sr);
                    h.amp2 = Decay::new(k.decay2.unwrap_or(0.15), sr);
                    h.amp2.hit(1.0);
                }
                if truthy(k.attack) {
                    h.att = 0.0;
                }
            }
            HitType::Rim => {
                h.a = Svf::with(k.f1.unwrap_or(455.0), 12.0, sr);
                h.b = Svf::with(k.f2.unwrap_or(1667.0), 14.0, sr);
                h.hp = Svf::with(300.0, 0.7, sr);
            }
            HitType::Shaker | HitType::Snap => {
                h.bp = Svf::with(k.bp.unwrap_or(6000.0), k.q.unwrap_or(1.4), sr);
                h.att = 0.0;
            }
            HitType::Swell => {
                h.len = (dur_s as f64).max(1.0);
                h.bp = Svf::with(8000.0, 0.8, sr);
                h.hp = Svf::with(2500.0, 0.7, sr);
            }
            HitType::Conga => {
                // The head's tone and a second partial, the open tone
                // ringing; a slap is a bright burst that chokes the tone
                // (k.slap).
                let slap = truthy(k.slap);
                h.s1 = Sine::new(0.25);
                h.s2 = Sine::new(0.1);
                h.nz = Decay::new(if slap { 0.02 } else { 0.004 }, sr);
                h.nz.hit(1.0);
                h.nbp = Svf::with(if slap { 2200.0 } else { 900.0 }, 1.2, sr);
            }
            HitType::Guiro => {
                // Scraped ridges: band-passed noise, a rasp per ridge as the
                // stick passes it, the stroke speeding up over its length.
                h.len = js_round(k.len.unwrap_or(0.15) * sr).max(1.0);
                h.bp = Svf::with(k.bp.unwrap_or(3000.0), k.q.unwrap_or(1.5), sr);
                h.ph1 = 0.0;
            }
        }
        h.ex = match tru(k.gate) {
            Some(g) => to_i32(g * sr) as i64,
            None => 0,
        };
        h
    }

    /// One sample; draws the hit's noise from `r`.
    fn run(&mut self, r: &mut Rng) -> f64 {
        let k = self.k;
        let sr = self.sr;
        let t = self.t as f64 / sr;
        self.t += 1;
        let noise = r.next() * 2.0 - 1.0;
        let mut y;
        match self.ty {
            HitType::Kick808 | HitType::Kick909 | HitType::Tom | HitType::Boom => {
                // A pitch that starts high and falls quickly onto the tuned
                // note.
                let f0 = k.tune.unwrap_or(50.0) * self.rate;
                let sweep = k.pitch.unwrap_or(2.0);
                let hz = f0 * (1.0 + (sweep - 1.0) * (-t / k.pt.unwrap_or(0.012)).exp());
                let mut s = self.sine.run(hz, sr);
                if self.ty == HitType::Kick909 {
                    s = tanh(s * 1.8) / 0.95; // the 909's shaped triangle
                }
                y = s * self.amp.run();
                // Click: a short filtered noise burst (909) or a tiny pulse
                // (808).
                let c = self.nz.run();
                if let Some(click) = tru(k.click) {
                    self.click.run(noise);
                    y += self.click.bp * c * click * 3.0;
                }
                if let Some(nz) = tru(k.noise) {
                    y += noise * nz * self.amp.v;
                }
                y = tanh(y * k.drive.unwrap_or(1.0)) / k.drive.unwrap_or(1.0).min(1.5);
            }
            HitType::Snare808 | HitType::Snare909 => {
                let f = k.tune.unwrap_or(180.0) * self.rate;
                let bend = if self.ty == HitType::Snare909 {
                    1.0 + 0.5 * (-t / 0.01).exp()
                } else {
                    1.0
                };
                let tone =
                    self.s1.run(f * bend, sr) * 0.65 + self.s2.run(f * 1.85 * bend, sr) * 0.35;
                let mut nenv = self.ndec.run();
                if self.ex != 0 {
                    nenv = if (self.t as i64) < self.ex {
                        nenv.max(0.35)
                    } else {
                        nenv * (-(self.t as f64 - self.ex as f64) / (0.004 * sr)).exp()
                    };
                }
                self.nhp.run(self.nlp.run(noise));
                let tn = k.tone.unwrap_or(0.5);
                y = tone * self.amp.run() * (1.0 - tn) + self.nhp.hp * nenv * tn * 2.2;
            }
            HitType::Clap => {
                // Three or four quick bursts, then the diffuse tail.
                let sp = k.spacing.unwrap_or(0.011);
                let nb = k.bursts.unwrap_or(3.0);
                let e = if t < sp * nb {
                    let u = (t % sp) / sp;
                    (-u * 6.0).exp()
                } else {
                    self.tail.run() * 0.8
                };
                if t < sp * nb {
                    self.tail.hit(1.0);
                }
                y = self.bp.run(noise) * e * 2.2;
                self.amp.v = e;
            }
            HitType::Metal | HitType::Cymbal | HitType::Cowbell => {
                let tune = k.tune.unwrap_or(1.0) * self.rate;
                let mut s = 0.0;
                let freqs: &[f64] = if self.ty == HitType::Cowbell {
                    &COWBELL
                } else {
                    &METAL
                };
                for i in 0..freqs.len() {
                    self.ph[i] += (freqs[i] * tune) / sr;
                    if self.ph[i] > 1.0 {
                        self.ph[i] -= 1.0;
                    }
                    s += if self.ph[i] < 0.5 { 1.0 } else { -1.0 };
                }
                s /= freqs.len() as f64;
                if let Some(nz) = tru(k.noise) {
                    s = s * (1.0 - nz) + noise * nz;
                }
                let mut e = self.amp.run();
                if let Some(att) = tru(k.attack) {
                    self.att = (self.att + 1.0 / (att * sr)).min(1.0);
                    e *= self.att;
                }
                self.bp.run(s);
                if self.ty == HitType::Cowbell {
                    y = self.bp.bp * e * 3.0;
                } else {
                    self.hp.run(self.bp.bp);
                    y = self.hp.hp * e * 2.5;
                    if self.ty == HitType::Cymbal {
                        y += self.lo.run(s) * self.amp2.run() * 0.6;
                    }
                }
            }
            HitType::Rim => {
                let ex = if self.t < 8 { 1.0 } else { 0.0 };
                self.a.run(ex);
                self.b.run(ex);
                self.hp.run(self.a.bp + self.b.bp);
                y = self.hp.hp * 4.0 * self.amp.run();
            }
            HitType::Shaker | HitType::Snap => {
                self.att = (self.att + 1.0 / (k.attack.unwrap_or(0.008) * sr)).min(1.0);
                self.bp.run(noise);
                y = (if self.ty == HitType::Snap {
                    self.bp.hp
                } else {
                    self.bp.bp
                }) * self.amp.run()
                    * self.att
                    * 1.6;
            }
            HitType::Swell => {
                // A reverse cymbal: noise that swells over its length, then
                // stops dead.
                let u = self.t as f64 / self.len;
                if u >= 1.0 {
                    self.done = true;
                    return 0.0;
                }
                self.bp.run(noise);
                self.hp.run(self.bp.bp + noise * 0.3);
                y = self.hp.hp * u * u * u * 1.4;
                self.amp.v = 1.0;
            }
            HitType::Conga => {
                let f0 = k.tune.unwrap_or(190.0) * self.rate;
                let hz = f0 * (1.0 + 0.35 * (-t / 0.008).exp());
                let tone = self.s1.run(hz, sr) * 0.8
                    + self.s2.run(hz * 1.51, sr) * 0.35 * (-t / 0.06).exp();
                self.nbp.run(noise);
                y = tone * self.amp.run()
                    + self.nbp.bp * self.nz.run() * (if truthy(k.slap) { 2.5 } else { 0.6 });
                y = tanh(y * 1.3) / 1.3;
            }
            HitType::Guiro => {
                let u = self.t as f64 / self.len;
                if u >= 1.0 {
                    self.done = true;
                    return 0.0;
                }
                let r0 = k.rate0.unwrap_or(70.0);
                self.ph1 += (r0 * (k.rate1.unwrap_or(160.0) / r0).powf(u)) / sr;
                if self.ph1 >= 1.0 {
                    self.ph1 -= 1.0;
                }
                let ridge = (1.0 - self.ph1) * (1.0 - self.ph1) * (1.0 - self.ph1);
                self.bp.run(noise);
                let env = (self.t as f64 / (0.003 * sr)).min(1.0)
                    * (if u > 0.85 { (1.0 - u) / 0.15 } else { 1.0 });
                y = self.bp.bp * ridge * env * 2.4;
                self.amp.v = 1.0;
            }
        }
        if self.amp.v < 1e-4 && self.t > 64 {
            self.done = true;
        }
        y * self.vel * k.lvl.unwrap_or(1.0)
    }
}

/// A drum machine: any number of overlapping hits; the open hat is choked
/// by the closed one, as on both machines.
#[derive(Clone, Debug)]
pub struct Drums {
    r: Rng,
    hits: Vec<Hit>,
    sr: f64,
}

impl Drums {
    /// The JS default is `seed = 3`.
    pub fn new(seed: u32, sr: f64) -> Drums {
        Drums {
            r: Rng::new(seed),
            hits: Vec::new(),
            sr,
        }
    }

    /// Starts a hit (the JS defaults are `rate = 1, durS = 0`) and returns
    /// its index in [`Drums::hits`]. The JS returns nothing for a missing
    /// voice; the caller resolves the `Option` here.
    pub fn hit(&mut self, lane: &str, k: &KitVoice, vel: f64, rate: f64, dur_s: usize) -> usize {
        if lane == "hat" {
            for h in &mut self.hits {
                if h.lane == "ohat" {
                    h.amp.k = (-1.0 / (0.01 * self.sr / 4.6)).exp();
                }
            }
        }
        let mut h = Hit::new(k.t, *k, vel, &mut self.r, rate, dur_s, self.sr);
        h.lane = lane.to_owned();
        self.hits.push(h);
        self.hits.len() - 1
    }

    /// The live and finished hits, in trigger order (until pruned).
    pub fn hits(&self) -> &[Hit] {
        &self.hits
    }

    /// Runs every live hit; `each(hit, y)` lets the engine route lanes.
    pub fn run(&mut self, mut each: impl FnMut(&Hit, f64)) {
        let mut alive = 0usize;
        for i in 0..self.hits.len() {
            if self.hits[i].done {
                continue;
            }
            let y = self.hits[i].run(&mut self.r);
            each(&self.hits[i], y);
            alive += 1;
        }
        if self.hits.len() > 32 && (alive as f64) < self.hits.len() as f64 / 2.0 {
            self.hits.retain(|h| !h.done);
        }
    }
}

// ── Instruments ──────────────────────────────────────────────────
/// A lab instrument: `makeInstrument(lab, seed)`.
#[derive(Clone, Debug)]
pub enum Instrument {
    Tb303(Tb303),
    Poly(Poly),
}

/// Builds the instrument for a lab patch.
pub fn make_instrument(lab: Lab, seed: u32, sr: f64) -> Instrument {
    if lab.kind == "tb303" {
        Instrument::Tb303(Tb303::new(lab, seed, sr))
    } else {
        let kind = lab.kind.clone();
        Instrument::Poly(Poly::new(&kind, lab, seed, 8, sr))
    }
}

impl Instrument {
    /// `dur_s` is the note's length in samples (the engine passes
    /// `Math.round(e.dur * SR)`; it is `| 0`'d here).
    pub fn trigger(&mut self, midis: &[f64], vel: f64, dur_s: f64, opt: &TriggerOpt) {
        match self {
            Instrument::Tb303(i) => i.trigger(midis, vel, dur_s, opt),
            Instrument::Poly(i) => i.trigger(midis, vel, dur_s, opt),
        }
    }

    /// Adds into `out`.
    pub fn run(&mut self, out: &mut [f64; 2]) {
        match self {
            Instrument::Tb303(i) => i.run(out),
            Instrument::Poly(i) => i.run(out),
        }
    }

    pub fn active(&self) -> bool {
        match self {
            Instrument::Tb303(i) => i.active(),
            Instrument::Poly(i) => i.active(),
        }
    }

    /// The patch the instrument reads live.
    pub fn lab(&self) -> &Lab {
        match self {
            Instrument::Tb303(i) => &i.p,
            Instrument::Poly(i) => &i.p,
        }
    }

    /// Section automation's `lab[param] = v` goes through here.
    pub fn lab_mut(&mut self) -> &mut Lab {
        match self {
            Instrument::Tb303(i) => &mut i.p,
            Instrument::Poly(i) => &mut i.p,
        }
    }

    /// `setPatch`: replaces the patch (and re-applies the chorus mode on a
    /// `Poly`).
    pub fn set_patch(&mut self, lab: Lab) {
        match self {
            Instrument::Tb303(i) => i.p = lab,
            Instrument::Poly(i) => i.set_patch(lab),
        }
    }
}
