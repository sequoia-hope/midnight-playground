//! The physical exhaust engine model: `tools/engine-lab/worklet.js`'s
//! `EngineLab` processor, ported line for line (docs/vision/sound.md 2.1).
//!
//! The lab is the oracle: the golden test renders every preset through a
//! drive cycle here and compares it with the lab's own render
//! (`parity/golden/exhaust/`). The signal flow, per sample (one engine,
//! stereo out):
//!
//! ```text
//!   combustion events ─ pulse ─ header waveguide ┐ (one per cylinder)
//!        │                                       ├─ bank collector ─ X-pipe ─┐
//!        │                                       ┘                           │
//!        │      ┌─────────────────────────────────────────────────────────────┘
//!        │      └─ (turbine) ─ saturation ─ pipe waveguide ─ muffler sections
//!        │           ─ tailpipe waveguide ─ radiation ─ body ─┐  (one per pipe)
//!        ├─ intake pulses + roar ─ intake waveguide ─────────┤
//!        ├─ valve-train ticks, gear whine ───────────────────┤
//!        └─ overrun / limiter pops (into pipe and tail) ─────┤
//!   turbo whistle, hiss, blow-off ────────────────────────────┴─ level ─ DC block
//!                         ─ psychoacoustic bass ─ bus compressor ─ limiter ─ out
//! ```
//!
//! Audio is not simulation: it uses the platform's `f64` math (`sin`, `exp`,
//! `tanh`, ...), as the lab uses `Math.*`. Stores into the lab's
//! `Float32Array`s (the delay lines, the limiter's look-ahead) round to
//! `f32` here too.
//!
//! On the web the model runs as its own small wasm inside an AudioWorklet:
//! see [`wasm`] and `web/exhaust-worklet.js`.

pub mod dsp;
pub mod presets;
#[cfg(all(target_arch = "wasm32", feature = "worklet"))]
pub mod wasm;

use dsp::{Biquad, Guide, Kind, OnePole, Rng, TAU, clamp};
pub use presets::{ORDER, boost_target, preset};

const MAX_CYL: usize = 16;
const MAX_POPS: usize = 10;
/// The limiter's look-ahead (samples, ~1.5 ms at 48 kHz).
const LIM_AHEAD: usize = 72;

/// An engine's parameters (the lab's `P`, with its defaults).
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    pub idle: f64,
    pub redline: f64,
    /// Firing events in order: (bank, header index).
    pub events: Vec<(u32, usize)>,
    /// Crank degrees per event; `None` is even firing.
    pub angles: Option<Vec<f64>>,
    pub bank_gain: [f64; 2],
    pub tau_deg: f64,
    pub sharp: f64,
    pub variation: f64,
    pub jitter_deg: f64,
    pub misfire: f64,
    pub turb_noise: f64,
    pub headers: Vec<f64>,
    pub header_scale: f64,
    pub header_spread: f64,
    pub header_r: f64,
    pub header_rv: f64,
    pub header_loss_hz: f64,
    pub pipes: f64,
    pub xpipe: f64,
    pub pipe_len: f64,
    pub pipe_r: f64,
    pub pipe_rc: f64,
    pub pipe_loss_hz: f64,
    pub muffler_lp: f64,
    /// Muffler and resonator resonances: [Hz, Q, dB].
    pub sections: Vec<[f64; 3]>,
    pub muffler_tune: f64,
    pub muffler_gain: f64,
    pub tail_len: f64,
    pub tail_r: f64,
    pub rad_hz: f64,
    pub rad_low: f64,
    pub body: f64,
    pub drive: f64,
    pub intake: f64,
    pub intake_len: f64,
    pub mech: f64,
    pub pops: f64,
    pub pop_hz: f64,
    pub imbalance: f64,
    pub turbo: f64,
    pub bov: f64,
    pub anti_lag: f64,
    pub level: f64,
    pub width: f64,
    pub comp_thresh: f64,
    pub comp_ratio: f64,
    pub makeup: f64,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            idle: 800.0,
            redline: 7000.0,
            events: vec![(0, 0), (1, 1), (0, 2), (1, 3)],
            angles: None,
            bank_gain: [1.0, 1.0],
            tau_deg: 30.0,
            sharp: 1.5,
            variation: 0.1,
            jitter_deg: 2.0,
            misfire: 0.0,
            turb_noise: 0.3,
            headers: vec![0.6],
            header_scale: 1.0,
            header_spread: 1.0,
            header_r: -0.5,
            header_rv: 0.85,
            header_loss_hz: 3500.0,
            pipes: 2.0,
            xpipe: 0.0,
            pipe_len: 2.5,
            pipe_r: -0.5,
            pipe_rc: 0.3,
            pipe_loss_hz: 2500.0,
            muffler_lp: 2000.0,
            sections: Vec::new(),
            muffler_tune: 1.0,
            muffler_gain: 1.0,
            tail_len: 0.5,
            tail_r: -0.45,
            rad_hz: 80.0,
            rad_low: 0.4,
            body: 3.0,
            drive: 1.5,
            intake: 1.0,
            intake_len: 0.35,
            mech: 0.5,
            pops: 1.0,
            pop_hz: 900.0,
            imbalance: 0.1,
            turbo: 0.0,
            bov: 0.0,
            anti_lag: 0.0,
            level: 1.0,
            width: 0.5,
            comp_thresh: -30.0,
            comp_ratio: 3.0,
            makeup: 9.0,
        }
    }
}

/// What the engine is doing (the page's `state` message).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct State {
    pub rpm: f64,
    pub throttle: f64,
    /// Turbo boost, 0..1.
    pub boost: f64,
    /// Road speed, m/s (the gear whine).
    pub speed: f64,
}

#[derive(Clone, Debug)]
struct Header {
    g: Guide,
    t: f64,
    tau: f64,
    amp: f64,
    sharp: f64,
    on: bool,
    bank: u32,
    len_m: f64,
    imb: f64,
}

#[derive(Clone, Debug)]
struct Event {
    bank: u32,
    hdr: usize,
    nominal: f64,
    next: f64,
}

#[derive(Clone, Debug)]
struct Pipe {
    dc_in: Biquad,
    turb: OnePole,
    guide: Guide,
    tail: Guide,
    mlp: Biquad,
    mlp2: Biquad,
    secs: [Biquad; 6],
    n_sec: usize,
    rad_lp: OnePole,
    body: Biquad,
    pop_bp: Biquad,
}

#[derive(Clone, Copy, Debug, Default)]
struct Pop {
    t: f64,
    tau: f64,
    amp: f64,
    sharp: f64,
    pipe: u32,
    on: bool,
    at2: f64,
    amp2: f64,
}

/// One engine (`EngineLab`).
#[derive(Clone, Debug)]
pub struct Engine {
    sr: f64,
    p: Params,
    rng: Rng,
    rpm_t: f64,
    thr_t: f64,
    boost_t: f64,
    speed_t: f64,
    rpm: f64,
    thr: f64,
    boost: f64,
    speed: f64,
    overrun: f64,
    load_slow: f64,
    last_thr_t: f64,
    /// Psychoacoustic bass amount (0 = off).
    pub psycho: f64,
    gain: f64,
    gain_t: f64,
    cycle_pos: f64,
    n: usize,
    hdr: Vec<Header>,
    ev: Vec<Event>,
    pipes: [Pipe; 2],
    intake_shape: Biquad,
    turb_lp: OnePole,
    intake_guide: Guide,
    intake_lp: OnePole,
    intake_noise_bp: Biquad,
    tick1: Biquad,
    tick2: Biquad,
    whine_ph: f64,
    turbo_ph: [f64; 2],
    hiss_bp: Biquad,
    bov_bp: Biquad,
    bov_t: f64,
    bov_amp: f64,
    pops: [Pop; MAX_POPS],
    flame_lp: OnePole,
    idle_wob: OnePole,
    idle_wob2: OnePole,
    dc_l: Biquad,
    dc_r: Biquad,
    pb_lp: Biquad,
    pb_lp2: Biquad,
    pb_hp: Biquad,
    pb_out_hp: Biquad,
    pb_out_hp2: Biquad,
    pb_out_lp: Biquad,
    pb_env: f64,
    limit_cut: bool,
    lim_l: [f32; LIM_AHEAD],
    lim_r: [f32; LIM_AHEAD],
    lim_w: usize,
    lim_g: f64,
    lim_hold: f64,
    lim_hold_n: usize,
    intake_kick: f64,
    tick_kick: f64,
    lift_burst: f64,
    comp_env: f64,
}

impl Engine {
    /// A new engine for `params`, already running at `state` (the
    /// processor's options: `params`, `state`, `seed`).
    pub fn new(params: Params, state: State, seed: u32, sample_rate: f64) -> Engine {
        let sr = sample_rate;
        let pipe = || Pipe {
            dc_in: Biquad::default(),
            turb: OnePole::new(6000.0, sr),
            guide: Guide::new(sr),
            tail: Guide::new(sr),
            mlp: Biquad::default(),
            mlp2: Biquad::default(),
            secs: Default::default(),
            n_sec: 0,
            rad_lp: OnePole::new(80.0, sr),
            body: Biquad::default(),
            pop_bp: Biquad::default(),
        };
        let header = || Header {
            g: Guide::new(sr),
            t: 0.0,
            tau: 1.0,
            amp: 0.0,
            sharp: 1.0,
            on: false,
            bank: 0,
            len_m: 0.6,
            imb: 1.0,
        };
        let mut e = Engine {
            sr,
            p: Params::default(),
            rng: Rng::new(seed),
            rpm_t: 800.0,
            thr_t: 0.0,
            boost_t: 0.0,
            speed_t: 0.0,
            rpm: 800.0,
            thr: 0.0,
            boost: 0.0,
            speed: 0.0,
            overrun: 0.0,
            load_slow: 0.0,
            last_thr_t: 0.0,
            psycho: 0.0,
            gain: 0.0,
            gain_t: 1.0,
            cycle_pos: 0.0,
            n: 0,
            hdr: (0..MAX_CYL).map(|_| header()).collect(),
            ev: Vec::new(),
            pipes: [pipe(), pipe()],
            intake_shape: Biquad::default(),
            turb_lp: OnePole::new(1600.0, sr),
            intake_guide: Guide::new(sr),
            intake_lp: OnePole::new(2000.0, sr),
            intake_noise_bp: Biquad::default(),
            tick1: Biquad::default(),
            tick2: Biquad::default(),
            whine_ph: 0.0,
            turbo_ph: [0.0; 2],
            hiss_bp: Biquad::default(),
            bov_bp: Biquad::default(),
            bov_t: -1.0,
            bov_amp: 0.0,
            pops: [Pop {
                tau: 1.0,
                at2: -1.0,
                ..Pop::default()
            }; MAX_POPS],
            flame_lp: OnePole::new(2500.0, sr),
            idle_wob: OnePole::new(1.5, sr),
            idle_wob2: OnePole::new(0.4, sr),
            dc_l: Biquad::new(Kind::Highpass, 18.0, 0.6, 0.0, sr),
            dc_r: Biquad::new(Kind::Highpass, 18.0, 0.6, 0.0, sr),
            // Psychoacoustic bass: isolate the sub band, normalise it by its
            // own envelope, distort it into harmonics, keep the 2nd..5th,
            // restore level.
            pb_lp: Biquad::new(Kind::Lowpass, 140.0, 0.7, 0.0, sr),
            pb_lp2: Biquad::new(Kind::Lowpass, 140.0, 0.7, 0.0, sr),
            pb_hp: Biquad::new(Kind::Highpass, 30.0, 0.7, 0.0, sr),
            pb_out_hp: Biquad::new(Kind::Highpass, 150.0, 0.7, 0.0, sr),
            pb_out_hp2: Biquad::new(Kind::Highpass, 150.0, 0.7, 0.0, sr),
            pb_out_lp: Biquad::new(Kind::Lowpass, 700.0, 0.7, 0.0, sr),
            pb_env: 0.0,
            limit_cut: false,
            lim_l: [0.0; LIM_AHEAD],
            lim_r: [0.0; LIM_AHEAD],
            lim_w: 0,
            lim_g: 1.0,
            lim_hold: 0.0,
            lim_hold_n: 0,
            intake_kick: 0.0,
            tick_kick: 0.0,
            lift_burst: 0.0,
            comp_env: 0.0,
        };
        e.set_params(params);
        e.set_state(state, true);
        e
    }

    pub fn params(&self) -> &Params {
        &self.p
    }

    /// The limiter's gain now (1 = not limiting).
    pub fn limiter_gain(&self) -> f64 {
        self.lim_g
    }

    /// Starts (fades in) or stops (fades out) the engine (`run`).
    pub fn set_running(&mut self, on: bool) {
        self.gain_t = if on { 1.0 } else { 0.0 };
    }

    /// New targets; `jump` sets them at once (`setState`).
    pub fn set_state(&mut self, s: State, jump: bool) {
        self.rpm_t = s.rpm;
        self.thr_t = clamp(s.throttle, 0.0, 1.0);
        self.boost_t = clamp(s.boost, 0.0, 1.0);
        self.speed_t = s.speed;
        if jump {
            self.rpm = self.rpm_t;
            self.thr = self.thr_t;
            self.boost = self.boost_t;
            self.speed = self.speed_t;
            self.last_thr_t = self.thr_t;
            self.load_slow = self.thr_t;
            self.overrun = if self.thr_t < 0.08 && self.rpm > self.p.idle * 1.6 {
                1.0
            } else {
                0.0
            };
        }
    }

    /// New parameters, keeping the engine's phase (`setParams`).
    pub fn set_params(&mut self, p: Params) {
        let sr = self.sr;
        self.p = p;
        let p = &self.p;
        // Firing events: (bank, header); angles default to even firing.
        let n = p.events.len().min(MAX_CYL);
        let same = self.ev.len() == n;
        let prev = std::mem::take(&mut self.ev);
        let base = self.cycle_pos.floor();
        // Cylinder imbalance: no two cylinders breathe and burn quite alike,
        // a fixed difference in strength and firing angle per cylinder. It
        // is what puts energy at the half orders (below the firing rate) of
        // an even-firing engine: its lope.
        let mut ir = Rng::new(4242);
        for k in 0..n {
            let (bank, hi) = p.events[k];
            let ang = match &p.angles {
                Some(a) => a[k],
                None => (k as f64 * 720.0) / n as f64,
            } + p.imbalance * 12.0 * ir.gauss();
            let h = hi % MAX_CYL;
            self.hdr[h].imb = (1.0 + p.imbalance * ir.gauss()).max(0.3);
            let (nominal, next) = if same {
                (prev[k].nominal, prev[k].next)
            } else {
                let nominal = base
                    + ang / 720.0
                    + if ang / 720.0 < self.cycle_pos - base {
                        1.0
                    } else {
                        0.0
                    };
                (nominal, nominal)
            };
            self.ev.push(Event {
                bank: bank & 1,
                hdr: h,
                nominal,
                next,
            });
            self.hdr[h].bank = bank & 1;
        }
        self.n = n;
        // Header lengths: around their mean, with the spread scaled.
        let hl = &p.headers;
        let mut mean = 0.0;
        for i in 0..n {
            mean += hl[i % hl.len()];
        }
        mean /= n as f64;
        for i in 0..MAX_CYL {
            let raw = hl[i % hl.len()];
            self.hdr[i].len_m =
                ((mean + (raw - mean) * p.header_spread) * p.header_scale).max(0.08);
            self.hdr[i].g.loss.set_hz(p.header_loss_hz, sr);
        }
        for pp in self.pipes.iter_mut() {
            pp.dc_in.set(Kind::Highpass, 12.0, 0.6, 0.0, sr);
            pp.turb
                .set_hz(if p.turbo != 0.0 { 3800.0 } else { 20000.0 }, sr);
            pp.guide.loss.set_hz(p.pipe_loss_hz, sr);
            pp.tail.loss.set_hz(1400.0, sr);
            pp.mlp.set(Kind::Lowpass, p.muffler_lp, 0.6, 0.0, sr);
            pp.mlp2.set(Kind::Lowpass, p.muffler_lp * 1.6, 0.5, 0.0, sr);
            pp.n_sec = p.sections.len().min(6);
            for (i, &[f, q, db]) in p.sections.iter().take(6).enumerate() {
                pp.secs[i].set(
                    Kind::Peaking,
                    f * p.muffler_tune,
                    q,
                    db * p.muffler_gain,
                    sr,
                );
            }
            pp.rad_lp.set_hz(p.rad_hz, sr);
            pp.body.set(Kind::Lowshelf, 120.0, 0.7, p.body, sr);
            pp.pop_bp.set(Kind::Highpass, p.pop_hz, 0.6, 0.0, sr);
        }
        self.intake_guide.loss.set_hz(3000.0, sr);
        self.tick1.set(Kind::Bandpass, 4300.0, 9.0, 0.0, sr);
        self.tick2.set(Kind::Bandpass, 7400.0, 12.0, 0.0, sr);
        self.retune();
    }

    /// Waveguide lengths in samples from metres and the gas temperature.
    fn retune(&mut self) {
        let (p, sr) = (&self.p, self.sr);
        let c = 460.0 + 120.0 * self.load_slow; // m/s in the hot exhaust
        for h in self.hdr.iter_mut() {
            h.g.len = ((h.len_m / c) * sr).max(1.5);
        }
        for pp in self.pipes.iter_mut() {
            pp.guide.len = ((p.pipe_len / (c * 0.92)) * sr).max(2.0); // cooler downstream
            pp.tail.len = ((p.tail_len / (c * 0.85)) * sr).max(2.0);
        }
        self.intake_guide.len = ((p.intake_len / 343.0) * sr).max(1.5);
    }

    /// A pop: unburnt charge lighting in the hot pipe. A pressure pulse (a
    /// small explosion), not noise: a fast rise and a few milliseconds'
    /// decay, which then rings the pipe, muffler and tail like a firing
    /// pulse does. Most are small, short and bright (the crackle); now and
    /// then one is big and long and so carries the bass (the bang). About a
    /// third come with a second, smaller flame front a few ms later.
    fn spawn_pop(&mut self, amp: f64, pipe: u32) {
        let sr = self.sr;
        for i in 0..MAX_POPS {
            if self.pops[i].on {
                continue;
            }
            let r = &mut self.rng;
            let size = r.next().powf(2.2); // 0 small .. 1 big; mostly small
            let p = &mut self.pops[i];
            p.on = true;
            p.t = 0.0;
            p.pipe = pipe;
            p.amp = amp * (0.35 + 1.0 * size);
            p.tau = sr * (0.0003 + 0.0011 * size + 0.0002 * r.next());
            p.sharp = 1.6 - 0.6 * size;
            p.at2 = if r.next() < 0.35 {
                sr * (0.0015 + 0.004 * r.next())
            } else {
                -1.0
            };
            p.amp2 = p.amp * (0.3 + 0.35 * r.next());
            return;
        }
    }

    fn fire(&mut self, k: usize) {
        let sr = self.sr;
        let (bank, hi) = (self.ev[k].bank, self.ev[k].hdr);
        let rpm = self.rpm;
        let idleness = clamp(1.0 - (rpm - self.p.idle) / 1300.0, 0.0, 1.0);
        let load = self.thr.max(0.3 * idleness) + 0.25 * self.boost * self.thr;
        let over = self.overrun;
        let rn = clamp(
            (rpm - self.p.idle) / (self.p.redline - self.p.idle),
            0.0,
            1.0,
        );
        // Rev limiter: ignition cut on about half the events; unburnt charge
        // sometimes lights in the pipe.
        if self.limit_cut && self.rng.next() < 0.55 {
            let h = &mut self.hdr[hi];
            h.on = true;
            h.t = 0.0;
            h.amp = 0.12;
            h.tau = (sr * 0.0003).max(((self.p.tau_deg * 1.4) / (6.0 * rpm)) * sr);
            h.sharp = 0.9;
            if self.rng.next() < 0.06 * self.p.pops {
                let a = 0.5 + 0.5 * self.rng.next();
                self.spawn_pop(a, bank);
            }
            return;
        }
        let p = &self.p;
        let r = &mut self.rng;
        // Cycle-to-cycle variation: largest at idle and on overrun, smallest
        // under load.
        let v = p.variation * (0.45 + 1.4 * idleness + 1.8 * over) * (1.0 - 0.6 * load.min(1.0));
        let mut amp = (0.2 + 0.8 * load.min(1.3).powf(0.8)) * (1.0 + v * r.gauss());
        if r.next() < p.misfire * (idleness + over) * 0.03 {
            amp *= 0.15;
        }
        amp = amp.max(0.02) * p.bank_gain[bank as usize] * self.hdr[hi].imb;
        let tau_deg = p.tau_deg * (1.15 - 0.3 * load.min(1.0)) * (1.0 + 0.3 * over);
        let h = &mut self.hdr[hi];
        h.on = true;
        h.t = 0.0;
        h.amp = amp;
        h.tau = (sr * 0.00025).max((tau_deg / (6.0 * rpm)) * sr); // deg → s at this rpm
        h.sharp = p.sharp * (0.7 + 0.55 * load.min(1.0)) * if over != 0.0 { 0.8 } else { 1.0 };
        // Intake: a suction pulse into the intake shaper, scaled by the
        // throttle opening.
        self.intake_kick -= (0.25 + 0.75 * self.thr) * (0.6 + 0.4 * r.next());
        // Valve train: a faint tick per event.
        self.tick_kick += p.mech * (0.3 + 0.7 * r.next()) * (0.4 + 0.6 * rn);
        // Overrun crackle: unburnt fuel lighting in the hot pipe,
        // off-throttle at high rpm. Pops per second: an occasional one on a
        // long overrun, a few just after a lift, more with anti-lag; spread
        // over the firing events. Kept sparse on purpose: the owner found a
        // steady crackle harsh, accurate or not.
        let per_sec = p.pops
            * (over * clamp((rn - 0.3) / 0.4, 0.0, 1.0) * (1.0 + 5.0 * self.lift_burst)
                + p.anti_lag * if self.thr < 0.15 { 1.0 } else { 0.0 } * rn * 1.5);
        if r.next() < per_sec / ((rpm / 120.0) * self.n as f64) {
            let a = (0.4 + 0.6 * r.next()) * (0.5 + 0.5 * rn);
            self.spawn_pop(a, bank);
        }
    }

    /// Renders one block (any length; the worklet's is 128).
    pub fn process(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        let n_block = out_l.len();
        let sr = self.sr;
        let nb = n_block as f64;
        // Per-block control work.
        self.load_slow += (self.thr - self.load_slow) * (nb / sr) * 2.0;
        self.retune();
        let kc = 1.0 - (-1.0 / (0.008 * sr)).exp();
        let kt = 1.0 - (-1.0 / (0.012 * sr)).exp();
        let kb = 1.0 - (-1.0 / (0.05 * sr)).exp();
        let p_turbo = self.p.turbo;
        if p_turbo != 0.0
            && self.last_thr_t > 0.5
            && self.thr_t < 0.2
            && self.boost > 0.35
            && self.p.bov > 0.0
        {
            self.bov_t = 0.0;
            self.bov_amp = self.boost;
        }
        if self.last_thr_t > 0.5 && self.thr_t < 0.15 {
            self.lift_burst = 1.0;
        }
        self.lift_burst *= (-(nb / sr) / 0.35).exp();
        self.last_thr_t = self.thr_t;
        let over_t = if self.thr_t < 0.08 && self.rpm_t > self.p.idle * 1.6 {
            1.0
        } else {
            0.0
        };
        self.overrun += (over_t - self.overrun) * ((nb / sr) * 12.0).min(1.0);
        self.limit_cut = self.rpm >= self.p.redline * 0.985 && self.thr > 0.5;
        let nf = self.n as f64;
        self.intake_shape.set(
            Kind::Lowpass,
            clamp((self.rpm * nf) / 120.0 * 1.5, 30.0, 4000.0),
            0.55,
            0.0,
            sr,
        );
        self.intake_lp.set_hz(300.0 + 2200.0 * self.thr, sr);
        self.intake_noise_bp.set(
            Kind::Bandpass,
            200.0 + 900.0 * (self.rpm / self.p.redline),
            0.8,
            0.0,
            sr,
        );
        self.hiss_bp
            .set(Kind::Bandpass, 1400.0 + 2600.0 * self.boost, 1.5, 0.0, sr);
        let sat = self.p.drive;
        let sat_n = 1.0 / sat;
        let hr = self.p.header_r;
        let hrv = self.p.header_rv;
        let ht = 1.0 + hr;
        let width = self.p.width;
        let turbo_amt = if p_turbo != 0.0 {
            0.55 + 0.45 * self.boost
        } else {
            0.0
        };
        let rn = clamp(
            (self.rpm - self.p.idle) / (self.p.redline - self.p.idle),
            0.0,
            1.0,
        );

        for i in 0..n_block {
            self.rpm += (self.rpm_t - self.rpm) * kc;
            self.thr += (self.thr_t - self.thr) * kt;
            self.boost += (self.boost_t - self.boost) * kb;
            self.speed += (self.speed_t - self.speed) * kb;
            self.gain += (self.gain_t - self.gain) * 0.0005;
            // Idle hunting: a slow wander in speed, only near idle.
            let idleness = clamp(1.0 - (self.rpm - self.p.idle) / 1300.0, 0.0, 1.0);
            let wob_in = self.rng.bi() * 30.0;
            let wob = self.idle_wob2.run(self.idle_wob.run(wob_in));
            let rpm_now = (self.rpm * (1.0 + 0.02 * idleness * wob)).max(200.0);
            self.cycle_pos += rpm_now / 120.0 / sr;
            for k in 0..self.n {
                let mut guard = 0;
                while self.cycle_pos >= self.ev[k].next && guard < 2 {
                    guard += 1;
                    self.fire(k);
                    self.ev[k].nominal += 1.0;
                    let jit = (self.p.jitter_deg / 720.0)
                        * (0.5 + 1.5 * idleness + self.overrun)
                        * self.rng.gauss();
                    let lim = 0.4 / nf;
                    self.ev[k].next = self.ev[k].nominal + clamp(jit, -lim, lim);
                }
            }

            // Combustion pulses into the headers; headers into the bank
            // collectors. Turbulent flow noise rides on each pulse; its
            // spectrum falls off.
            let tn = self.rng.bi();
            let turb = self.p.turb_noise * (0.4 + 0.6 * self.thr) * self.turb_lp.run(tn) * 2.5;
            let (mut b0, mut b1) = (0.0, 0.0);
            for k in 0..self.n {
                let h = &mut self.hdr[self.ev[k].hdr];
                let mut p = 0.0;
                if h.on {
                    let u = h.t / h.tau;
                    if u > 12.0 {
                        h.on = false;
                    } else {
                        let g = u * (1.0 - u).exp();
                        p = h.amp * g.powf(h.sharp) * (1.0 + turb);
                        h.t += 1.0;
                    }
                }
                let y = h.g.step(p, hr, hrv) * ht;
                if h.bank != 0 {
                    b1 += y;
                } else {
                    b0 += y;
                }
            }
            if self.p.pipes < 2.0 {
                b0 += b1;
                b1 = 0.0;
            } else if self.p.xpipe > 0.0 {
                let x = self.p.xpipe;
                let m0 = b0;
                b0 = (1.0 - x) * b0 + x * b1;
                b1 = (1.0 - x) * b1 + x * m0;
            }

            // Pops: pressure pulses (see spawn_pop), roughened by flame
            // turbulence, into the pipe; their bright edge also leaves at
            // the tail.
            let (mut pop0, mut pop1) = (0.0, 0.0);
            let fl = self.rng.bi();
            let flame = self.flame_lp.run(fl);
            for pp in self.pops.iter_mut() {
                if !pp.on {
                    continue;
                }
                let u = pp.t / pp.tau;
                let mut s = pp.amp * (u * (1.0 - u).exp()).powf(pp.sharp);
                if pp.at2 >= 0.0 && pp.t >= pp.at2 {
                    let u2 = (pp.t - pp.at2) / pp.tau;
                    s += pp.amp2 * (u2 * (1.0 - u2).exp()).powf(pp.sharp);
                }
                s *= 1.0 + 1.2 * flame;
                if pp.pipe != 0 && self.p.pipes >= 2.0 {
                    pop1 += s;
                } else {
                    pop0 += s;
                }
                pp.t += 1.0;
                if pp.t > pp.tau * 14.0 + pp.at2.max(0.0) {
                    pp.on = false;
                }
            }

            let (mut o_l, mut o_r) = (0.0, 0.0);
            let np = if self.p.pipes >= 2.0 { 2 } else { 1 };
            for j in 0..np {
                let p = &self.p;
                let pp = &mut self.pipes[j];
                let mut s = if j != 0 { b1 } else { b0 };
                s = pp.dc_in.run(s);
                if turbo_amt != 0.0 {
                    s = pp.turb.run(s) * (1.0 - 0.3 * turbo_amt);
                }
                // Pops light in the pipe, so they take the same steepening as
                // the firing pulses rather than standing over the mix and
                // ducking it.
                let pop = if j != 0 { pop1 } else { pop0 };
                s += pop;
                // High-level pressure waves steepen and compress: asymmetric
                // soft clip.
                let d = s * sat;
                s = (if d >= 0.0 {
                    d.tanh()
                } else {
                    (0.75 * d).tanh() / 0.75
                }) * sat_n;
                let mut m = pp.guide.step(s, p.pipe_r, p.pipe_rc) * (1.0 + p.pipe_r);
                m = pp.mlp.run(m);
                m = pp.mlp2.run(m);
                for q in 0..pp.n_sec {
                    m = pp.secs[q].run(m);
                }
                m += pp.pop_bp.run(pop) * 0.25;
                let tl = pp.tail.step(m, p.tail_r, 0.25);
                // Open-end radiation: the low end radiates less (high-pass-ish
                // shelf).
                let mut rad = tl - (1.0 - p.rad_low) * pp.rad_lp.run(tl);
                rad = pp.body.run(rad);
                if np == 1 {
                    o_l += rad;
                    o_r += rad;
                } else if j == 0 {
                    o_l += rad * (0.5 + 0.5 * width);
                    o_r += rad * (0.5 - 0.5 * width);
                } else {
                    o_l += rad * (0.5 - 0.5 * width);
                    o_r += rad * (0.5 + 0.5 * width);
                }
            }

            // Intake: suction pulses shaped by a low-pass, plus roar noise;
            // the throttle is the opening (low-pass and level).
            let ik = self.intake_kick;
            self.intake_kick = 0.0;
            let shaped = self.intake_shape.run(ik * 6.0);
            let inoise = self.rng.bi();
            let isrc = shaped + self.intake_noise_bp.run(inoise) * 0.05 * self.thr * (0.3 + rn);
            let mut iv = self.intake_guide.step(isrc, -0.75, 0.6);
            iv = self.intake_lp.run(iv) * self.p.intake * (0.15 + 0.85 * self.thr) * 0.5;

            // Mechanical: valve-train ticks and a faint gear whine with road
            // speed.
            let tk = self.tick_kick;
            self.tick_kick = 0.0;
            let t1 = self.tick1.run(tk);
            let t2 = self.tick2.run(tk);
            let mut mech = (t1 + 0.6 * t2) * 0.012;
            if self.speed > 0.5 {
                self.whine_ph += (self.speed * 26.0) / sr;
                self.whine_ph -= self.whine_ph.floor();
                mech += (self.whine_ph * TAU).sin()
                    * 0.0025
                    * self.p.mech
                    * (self.speed / 30.0).min(1.0)
                    * (0.3 + 0.7 * self.thr);
            }

            // Turbo: shaft whistle (blade pass), intake hiss, blow-off valve.
            let mut tb = 0.0;
            if p_turbo != 0.0 {
                let b = self.boost;
                let units = if p_turbo < 2.0 { p_turbo } else { 2.0 };
                let mut t = 0;
                while (t as f64) < units {
                    let f = (1900.0 + b * 5200.0 + rn * 700.0) * if t != 0 { 1.031 } else { 1.0 };
                    self.turbo_ph[t] += f / sr;
                    self.turbo_ph[t] -= self.turbo_ph[t].floor();
                    let ph = self.turbo_ph[t] * TAU;
                    tb += (ph.sin() + 0.3 * (ph * 1.5).sin())
                        * 0.008
                        * b
                        * b
                        * (0.4 + 0.6 * self.thr);
                    t += 1;
                }
                let hn = self.rng.bi();
                tb += self.hiss_bp.run(hn) * 0.025 * b * self.thr;
                if self.bov_t >= 0.0 {
                    let t = self.bov_t / sr;
                    if (self.bov_t as i64) & 31 == 0 {
                        self.bov_bp.set(
                            Kind::Bandpass,
                            1300.0 + 2300.0 * (-t / 0.15).exp(),
                            1.3,
                            0.0,
                            sr,
                        );
                    }
                    let env = (t / 0.006).min(1.0) * (-t / 0.13).exp();
                    let bn = self.rng.bi();
                    tb += self.bov_bp.run(bn) * env * 0.25 * self.p.bov * self.bov_amp;
                    self.bov_t += 1.0;
                    if self.bov_t > sr * 0.8 {
                        self.bov_t = -1.0;
                    }
                }
            }

            let c = iv + mech + tb;
            let mut l = (o_l + c) * self.p.level * self.gain;
            let mut rr = (o_r + c) * self.p.level * self.gain;
            l = self.dc_l.run(l);
            rr = self.dc_r.run(rr);

            // Psychoacoustic bass: harmonics of the sub band so a small
            // speaker still implies the fundamental.
            if self.psycho > 0.0 {
                let x = (l + rr) * 0.5;
                let sub = self.pb_hp.run(self.pb_lp2.run(self.pb_lp.run(x)));
                let a = sub.abs();
                self.pb_env += (a - self.pb_env) * if a > self.pb_env { 0.004 } else { 0.0004 };
                let nrm = sub / (self.pb_env + 1e-4);
                let hgen = 0.5 * nrm.abs() + 0.5 * (2.5 * nrm).tanh();
                let hb = self
                    .pb_out_lp
                    .run(self.pb_out_hp2.run(self.pb_out_hp.run(hgen)))
                    * self.pb_env
                    * 7.0
                    * self.psycho;
                l += hb;
                rr += hb;
            }
            // Bus compressor: RMS-ish detector, soft ratio.
            let lev = (l * l + rr * rr) * 0.5;
            self.comp_env +=
                (lev - self.comp_env) * if lev > self.comp_env { 0.0042 } else { 0.00014 };
            let env_db = 10.0 * (self.comp_env + 1e-12).log10();
            let over = env_db - self.p.comp_thresh;
            let cg = if over > 0.0 {
                10f64.powf((-over * (1.0 - 1.0 / self.p.comp_ratio)) / 20.0)
            } else {
                1.0
            };
            l *= cg * self.p.makeup;
            rr *= cg * self.p.makeup;
            // Peak limiter with look-ahead (~60 ms release). The output is
            // delayed LIM_AHEAD samples and the gain glides down over that
            // time, so a peak meets a gain already lowered.
            let pk = l.abs().max(rr.abs());
            if pk >= self.lim_hold {
                self.lim_hold = pk;
                self.lim_hold_n = LIM_AHEAD;
            } else if self.lim_hold_n > 0 {
                self.lim_hold_n -= 1;
            } else {
                self.lim_hold += (pk - self.lim_hold) * 0.00035;
            }
            let lt = if self.lim_hold > 0.84 {
                0.84 / self.lim_hold
            } else {
                1.0
            };
            let lim_attack = 1.0 - (-7.0 / LIM_AHEAD as f64).exp();
            self.lim_g += (lt - self.lim_g) * if lt < self.lim_g { lim_attack } else { 0.00035 };
            let w = self.lim_w;
            let (dl, dr) = (self.lim_l[w] as f64, self.lim_r[w] as f64);
            self.lim_l[w] = l as f32;
            self.lim_r[w] = rr as f32;
            self.lim_w = (w + 1) % LIM_AHEAD;
            l = dl * self.lim_g;
            rr = dr * self.lim_g;
            // Safety: soft knee above 0.9 so a bad tweak can't blast.
            out_l[i] = knee(l) as f32;
            out_r[i] = knee(rr) as f32;
        }
    }
}

fn knee(x: f64) -> f64 {
    if x.abs() > 0.9 {
        x.signum() * (0.9 + 0.1 * ((x.abs() - 0.9) * 10.0).tanh())
    } else {
        x
    }
}
