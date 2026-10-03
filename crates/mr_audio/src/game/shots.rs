//! One-shots, rival engines and the Hot Pursuit sounds (`Audio.js` from
//! `_pop` to `_pursuitReset`).

use super::{
    GameAudio, PendingLine, RADIO_GAP, RADIO_VOICE, RADIO_WAIT, RadioCur, Rival, SirenUnit, clamp,
    siren_doppler, siren_level, siren_pattern,
};
use crate::engine;
use crate::shapes::distortion_curve;
use crate::timers::Task;
use crate::tracks::{FmMod, Patch};
use crate::wa::{
    AudioBuffer, AudioBufferSourceNode, BiquadFilterNode, BiquadFilterType, GainNode, Node,
    OscillatorNode, OscillatorType,
};
use mr_math::js;
use mr_math::kernel::log2;

use BiquadFilterType as B;

/// A source a radio transmission stops when it is cut.
pub(crate) enum Src {
    Buf(AudioBufferSourceNode),
    Osc(OscillatorNode),
}

impl Src {
    fn stop_at(&self, t: f64) {
        // try { s.stop(t) } catch { /* already stopping */ }
        let _ = match self {
            Src::Buf(s) => s.stop_at(t),
            Src::Osc(o) => o.stop_at(t),
        };
    }
}

/// `_play`'s options (`{ time, gain, rate, pan, dest, offset }`).
#[derive(Clone)]
pub struct PlayOpts {
    /// `None`: now.
    pub time: Option<f64>,
    pub gain: f64,
    pub rate: f64,
    pub pan: f64,
    /// `None`: the SFX bus.
    pub dest: Option<Node>,
    pub offset: f64,
}

impl Default for PlayOpts {
    fn default() -> Self {
        PlayOpts {
            time: None,
            gain: 0.5,
            rate: 1.0,
            pan: 0.0,
            dest: None,
            offset: 0.0,
        }
    }
}

/// `_noiseBurst`'s options.
#[derive(Clone)]
pub struct NoiseBurst {
    pub time: f64,
    pub dur: f64,
    pub kind: BiquadFilterType,
    pub freq: f64,
    pub q: f64,
    pub gain: f64,
    pub rate: f64,
    /// `None`: the SFX bus.
    pub dest: Option<Node>,
    /// An exponential frequency sweep to this (`null`: none).
    pub sweep_to: Option<f64>,
    pub pan: f64,
}

impl Default for NoiseBurst {
    fn default() -> Self {
        NoiseBurst {
            time: 0.0,
            dur: 0.0,
            kind: B::Bandpass,
            freq: 1000.0,
            q: 1.0,
            gain: 0.3,
            rate: 1.0,
            dest: None,
            sweep_to: None,
            pan: 0.0,
        }
    }
}

/// What `_noiseBurst` returns.
pub(crate) struct Burst {
    pub f: BiquadFilterNode,
    pub g: GainNode,
}

fn truthy(x: f64) -> bool {
    x != 0.0 && !x.is_nan()
}

fn osc(g: &GameAudio, t: OscillatorType) -> OscillatorNode {
    let o = g.ctx().expect("a context").create_oscillator();
    o.set_type(t);
    o
}

fn biquad(g: &GameAudio, t: BiquadFilterType) -> BiquadFilterNode {
    let f = g.ctx().expect("a context").create_biquad_filter();
    f.set_type(t);
    f
}

fn gain(g: &GameAudio) -> GainNode {
    g.ctx().expect("a context").create_gain()
}

impl GameAudio {
    pub(super) fn pop(&mut self, time: f64, strength: f64) {
        let ctx = self.ctx.clone().expect("a context");
        let g = self.graph();
        let prof = self.prof.unwrap_or(&engine::SPORTS);
        let [f0, f1] = prof.pop_freq;
        let src = ctx.create_buffer_source();
        let _ = src.set_buffer(Some(&g.noise_white));
        src.playback_rate.set_value(0.6 + self.random() * 0.8);
        let bp = biquad(self, B::Bandpass);
        bp.frequency.set_value(f0 + self.random() * (f1 - f0));
        bp.q.set_value(1.4);
        let sh = ctx.create_wave_shaper();
        let curve = self
            .pop_curve
            .get_or_insert_with(|| distortion_curve(6.0, 1024))
            .clone();
        sh.set_curve(Some(&curve));
        let gg = gain(self);
        let peak = 0.5 * strength * js::min(1.3, prof.pops);
        gg.gain.set_value_at_time(0.0, time);
        gg.gain.linear_ramp_to_value_at_time(peak, time + 0.002);
        let dec = time + 0.05 + self.random() * 0.03;
        gg.gain.exponential_ramp_to_value_at_time(0.0005, dec);
        let _ = src.connect(&bp);
        let _ = bp.connect(&sh);
        let _ = sh.connect(&gg);
        let _ = gg.connect(&g.sfx_bus);
        let off = self.random() * 1.5;
        let _ = src.start_with(time, off, Some(0.12));
        // Low thud under the crack.
        let o = osc(self, OscillatorType::Sine);
        o.frequency.set_value_at_time(f0 * 0.18 + 30.0, time);
        o.frequency
            .exponential_ramp_to_value_at_time(40.0, time + 0.06);
        let og = gain(self);
        og.gain.set_value_at_time(0.28 * strength, time);
        og.gain
            .exponential_ramp_to_value_at_time(0.0005, time + 0.07);
        let _ = o.connect(&og);
        let _ = og.connect(&g.sfx_bus);
        let _ = o.start_at(time);
        let _ = o.stop_at(time + 0.09);
    }

    // ── One-shots ─────────────────────────────────────────────────────
    pub fn shift(&mut self, up: bool) {
        if !self.sfx_on() {
            return;
        }
        let t = self.now();
        let g = self.graph();
        if self.gate_open(g.vg.eng) {
            let p = &g.eng.shift_g.gain;
            p.cancel_scheduled_values(t);
            p.set_value_at_time(p.value(), t);
            p.linear_ramp_to_value_at_time(if up { 0.3 } else { 0.55 }, t + 0.025);
            p.set_target_at_time(1.0, t + if up { 0.11 } else { 0.06 }, 0.05);
        }
        if self.electric {
            return;
        }
        // The gearbox: a dog-ring clunk, softer on the way down.
        let rate = 0.9 + self.random() * 0.2;
        self.play(
            g.sfx("clunk"),
            PlayOpts {
                time: Some(t + if up { 0.03 } else { 0.015 }),
                gain: if up { 0.32 } else { 0.24 },
                rate,
                ..Default::default()
            },
        );
        if !up {
            self.pop(t + 0.02, 0.35); // rev-match blip
        } else {
            // Exhaust bark as the ignition cuts and comes back under load.
            if self.prev_throttle > 0.6 {
                self.pop(t + 0.015, 0.5);
                let at = t + 0.06 + self.random() * 0.03;
                self.pop(at, 0.3);
            }
            if self.prof.is_some_and(|p| p.turbo != 0.0) && self.boost > 0.5 {
                self.blow_off(self.boost * 0.6);
            }
        }
    }

    /// Play a pre-rendered buffer once (`_play`).
    pub(crate) fn play(&self, buf: Option<&AudioBuffer>, o: PlayOpts) {
        let Some(buf) = buf else {
            return;
        };
        let ctx = self.ctx.clone().expect("a context");
        let gr = self.graph();
        let time = o.time.unwrap_or_else(|| ctx.current_time());
        let dest = o.dest.clone().unwrap_or_else(|| (*gr.sfx_bus).clone());
        let src = ctx.create_buffer_source();
        let _ = src.set_buffer(Some(buf));
        src.playback_rate.set_value(o.rate);
        let g = ctx.create_gain();
        g.gain.set_value(o.gain);
        let _ = src.connect(&g);
        if truthy(o.pan) {
            let p = ctx.create_stereo_panner();
            p.pan.set_value(clamp(o.pan, -1.0, 1.0));
            let _ = g.connect(&p);
            let _ = p.connect(&dest);
        } else {
            let _ = g.connect(&dest);
        }
        let _ = src.start_with(time, o.offset, None);
    }

    pub(crate) fn noise_burst(&mut self, o: NoiseBurst) -> Burst {
        let ctx = self.ctx.clone().expect("a context");
        let gr = self.graph();
        let dest = o.dest.clone().unwrap_or_else(|| (*gr.sfx_bus).clone());
        let src = ctx.create_buffer_source();
        let _ = src.set_buffer(Some(&gr.noise_white));
        src.playback_rate.set_value(o.rate);
        let f = ctx.create_biquad_filter();
        f.set_type(o.kind);
        f.frequency.set_value(o.freq);
        f.q.set_value(o.q);
        if let Some(to) = o.sweep_to.filter(|x| truthy(*x)) {
            f.frequency.set_value_at_time(o.freq, o.time);
            f.frequency
                .exponential_ramp_to_value_at_time(to, o.time + o.dur);
        }
        let g = ctx.create_gain();
        g.gain.set_value_at_time(0.0, o.time);
        g.gain
            .linear_ramp_to_value_at_time(o.gain, o.time + js::min(0.01, o.dur * 0.2));
        g.gain
            .exponential_ramp_to_value_at_time(0.0005, o.time + o.dur);
        let _ = src.connect(&f);
        let _ = f.connect(&g);
        if truthy(o.pan) {
            let p = ctx.create_stereo_panner();
            p.pan.set_value(o.pan);
            let _ = g.connect(&p);
            let _ = p.connect(&dest);
        } else {
            let _ = g.connect(&dest);
        }
        let off = self.random() * 1.5;
        let _ = src.start_with(o.time, off, Some(o.dur + 0.05));
        Burst { f, g }
    }

    pub(super) fn blow_off(&mut self, strength: f64) {
        let t = self.now();
        self.noise_burst(NoiseBurst {
            time: t,
            dur: 0.42,
            kind: B::Bandpass,
            freq: 3600.0,
            q: 1.4,
            gain: 0.2 * strength,
            sweep_to: Some(1300.0),
            ..Default::default()
        });
        self.noise_burst(NoiseBurst {
            time: t,
            dur: 0.3,
            kind: B::Highpass,
            freq: 5200.0,
            q: 0.7,
            gain: 0.07 * strength,
            ..Default::default()
        });
        // Compressor surge flutter: "chu-tu-tu".
        for k in 0..3 {
            let k = k as f64;
            self.noise_burst(NoiseBurst {
                time: t + 0.05 + k * 0.055,
                dur: 0.05,
                kind: B::Bandpass,
                freq: 520.0,
                q: 2.0,
                gain: 0.12 * strength * (1.0 - k * 0.25),
                ..Default::default()
            });
        }
    }

    /// Layered crash: a body thud, crumpling sheet metal, a noise crunch for
    /// the attack and, on a hard hit, glass. pan: -1 left .. 1 right.
    pub fn impact(&mut self, strength: f64, pan: f64) {
        if !self.sfx_on() {
            return;
        }
        let t = self.now();
        let s = clamp(strength, 0.0, 1.0);
        // Grinding along a wall fires a stream of small hits: thin them out.
        if t - self.impact_t < 0.07 && s < self.impact_s.unwrap_or(0.0) + 0.2 {
            return;
        }
        self.impact_t = t;
        self.impact_s = Some(s);
        let g = self.graph();
        let rate = 0.85 + self.random() * 0.25;
        self.play(
            g.sfx("thud"),
            PlayOpts {
                gain: 0.55 * (0.3 + 0.7 * s),
                rate,
                pan: pan * 0.4,
                ..Default::default()
            },
        );
        let metal = ["metal1", "metal2", "metal3"][(self.random() * 3.0).floor() as usize];
        // Harder hits ring lower and longer (a bigger panel moves).
        let rate = 1.2 - 0.4 * s + self.random() * 0.15;
        self.play(
            g.sfx(metal),
            PlayOpts {
                gain: 0.5 * (0.2 + 0.8 * s),
                rate,
                pan: pan * 0.6,
                ..Default::default()
            },
        );
        if s > 0.45 {
            let which = if self.random() < 0.5 {
                "metal1"
            } else {
                "metal2"
            };
            let rate = 0.7 + self.random() * 0.15;
            self.play(
                g.sfx(which),
                PlayOpts {
                    time: Some(t + 0.015),
                    gain: 0.3 * s,
                    rate,
                    pan: -pan * 0.3,
                    ..Default::default()
                },
            );
        }
        self.noise_burst(NoiseBurst {
            time: t,
            dur: 0.12 + s * 0.15,
            freq: 900.0 + s * 700.0,
            q: 0.8,
            gain: 0.3 * (0.25 + 0.75 * s),
            rate: 0.7,
            ..Default::default()
        });
        if s > 0.55 {
            let which = if self.random() < 0.5 {
                "glass1"
            } else {
                "glass2"
            };
            let rate = 0.9 + self.random() * 0.25;
            self.play(
                g.sfx(which),
                PlayOpts {
                    time: Some(t + 0.02),
                    gain: 0.45 * (s - 0.35),
                    rate,
                    pan: pan * 0.5,
                    ..Default::default()
                },
            );
        }
    }

    pub fn landing(&mut self, strength: f64) {
        if !self.sfx_on() {
            return;
        }
        let t = self.now();
        let g = self.graph();
        let s = clamp(strength, 0.0, 1.0);
        let o = osc(self, OscillatorType::Sine);
        o.frequency.set_value_at_time(75.0, t);
        o.frequency.exponential_ramp_to_value_at_time(32.0, t + 0.2);
        let og = gain(self);
        og.gain.set_value_at_time(0.0, t);
        og.gain
            .linear_ramp_to_value_at_time(0.45 * (0.3 + 0.7 * s), t + 0.006);
        og.gain.exponential_ramp_to_value_at_time(0.0005, t + 0.28);
        let _ = o.connect(&og);
        let _ = og.connect(&g.sfx_bus);
        let _ = o.start_at(t);
        let _ = o.stop_at(t + 0.3);
        // Suspension bottoming out, then the tyres chirp as they bite again.
        let rate = 0.95 + self.random() * 0.15;
        self.play(
            g.sfx("thud"),
            PlayOpts {
                time: Some(t + 0.01),
                gain: 0.5 * (0.2 + 0.8 * s),
                rate,
                ..Default::default()
            },
        );
        self.noise_burst(NoiseBurst {
            time: t + 0.015,
            dur: 0.07,
            freq: 420.0,
            q: 2.5,
            gain: 0.18 * s,
            rate: 0.5,
            ..Default::default()
        });
        if s > 0.25 {
            self.noise_burst(NoiseBurst {
                time: t + 0.03,
                dur: 0.12,
                freq: 1300.0,
                q: 7.0,
                gain: 0.25 * s,
                sweep_to: Some(1000.0),
                ..Default::default()
            });
            if s > 0.6 {
                self.play(
                    g.sfx("metal3"),
                    PlayOpts {
                        time: Some(t + 0.02),
                        gain: 0.18 * s,
                        rate: 0.8,
                        ..Default::default()
                    },
                );
            }
        }
    }

    /// Countdown: a rounded square-wave pip; GO is an octave up with a chord.
    pub fn beep(&mut self, final_: bool) {
        if !self.sfx_on() {
            return;
        }
        let t = self.now();
        let g = self.graph();
        let dur = if final_ { 0.7 } else { 0.22 };
        let f0 = if final_ { 880.0 } else { 440.0 };
        let notes: Vec<(f64, f64)> = if final_ {
            vec![
                (f0, 1.0),
                (f0 * 1.26, 0.55),
                (f0 * 1.5, 0.5),
                (f0 / 2.0, 0.6),
            ]
        } else {
            vec![(f0, 1.0), (f0 / 2.0, 0.5)]
        };
        let lp = biquad(self, B::Lowpass);
        lp.q.set_value(0.7);
        lp.frequency
            .set_value_at_time(if final_ { 5200.0 } else { 3200.0 }, t);
        lp.frequency
            .set_target_at_time(if final_ { 2600.0 } else { 1600.0 }, t, dur / 2.0);
        let gg = gain(self);
        let lvl = if final_ { 0.075 } else { 0.085 };
        gg.gain.set_value_at_time(0.0, t);
        gg.gain.linear_ramp_to_value_at_time(lvl, t + 0.006);
        gg.gain.set_target_at_time(lvl * 0.7, t + 0.01, 0.1);
        gg.gain.set_target_at_time(0.0, t + dur - 0.06, 0.025);
        let _ = lp.connect(&gg);
        let _ = gg.connect(&g.sfx_bus);
        for (f, a) in notes {
            for (ty, m, det) in [
                (OscillatorType::Square, 0.6, 0.0),
                (OscillatorType::Sine, 1.0, 4.0),
            ] {
                let o = osc(self, ty);
                o.frequency.set_value(f);
                o.detune.set_value(det);
                let og = gain(self);
                og.gain.set_value(a * m);
                let _ = o.connect(&og);
                let _ = og.connect(&lp);
                let _ = o.start_at(t);
                let _ = o.stop_at(t + dur + 0.1);
            }
        }
        if final_ {
            self.noise_burst(NoiseBurst {
                time: t,
                dur: 0.6,
                kind: B::Bandpass,
                freq: 900.0,
                q: 0.7,
                gain: 0.08,
                sweep_to: Some(5000.0),
                ..Default::default()
            });
        }
    }

    /// A car flashing past: an air rush that sweeps across the stereo field
    /// plus its engine note dropping in pitch (doppler).
    pub fn whoosh(&mut self, pan: f64, strength: f64) {
        if !self.sfx_on() {
            return;
        }
        let ctx = self.ctx.clone().expect("a context");
        let t = self.now();
        let gr = self.graph();
        let s = clamp(strength, 0.0, 1.0);
        let p = clamp(pan, -1.0, 1.0);
        let b = self.noise_burst(NoiseBurst {
            time: t,
            dur: 0.6,
            freq: 350.0,
            q: 1.4,
            gain: 0.35 * s,
            ..Default::default()
        });
        b.f.frequency.cancel_scheduled_values(t);
        b.f.frequency.set_value_at_time(380.0, t);
        b.f.frequency
            .exponential_ramp_to_value_at_time(1700.0, t + 0.15);
        b.f.frequency
            .exponential_ramp_to_value_at_time(280.0, t + 0.6);
        b.g.gain.cancel_scheduled_values(t);
        b.g.gain.set_value_at_time(0.0, t);
        b.g.gain.linear_ramp_to_value_at_time(0.35 * s, t + 0.14);
        b.g.gain.exponential_ramp_to_value_at_time(0.0005, t + 0.6);
        let pn = ctx.create_stereo_panner();
        pn.pan.set_value_at_time(p * 0.5, t);
        pn.pan.linear_ramp_to_value_at_time(p, t + 0.14);
        pn.pan.linear_ramp_to_value_at_time(p * 0.3, t + 0.6);
        let _ = pn.connect(&gr.sfx_bus);
        b.g.disconnect();
        let _ = b.g.connect(&pn);
        let o = osc(self, OscillatorType::Sawtooth);
        let f = 70.0 + self.random() * 40.0;
        o.frequency.set_value_at_time(f * 1.25, t);
        o.frequency.linear_ramp_to_value_at_time(f * 1.2, t + 0.12);
        o.frequency
            .exponential_ramp_to_value_at_time(f * 0.8, t + 0.3);
        let lp = biquad(self, B::Lowpass);
        lp.frequency.set_value(700.0);
        let og = gain(self);
        og.gain.set_value_at_time(0.0, t);
        og.gain.linear_ramp_to_value_at_time(0.09 * s, t + 0.12);
        og.gain.exponential_ramp_to_value_at_time(0.0005, t + 0.5);
        let _ = o.connect(&lp);
        let _ = lp.connect(&og);
        let _ = og.connect(&pn);
        let _ = o.start_at(t);
        let _ = o.stop_at(t + 0.55);
    }

    pub fn nitro_burst(&mut self) {
        if !self.sfx_on() {
            return;
        }
        let t = self.now();
        let gr = self.graph();
        if self.electric {
            // Overboost: a rising electric zap and a crackle of discharge.
            let o = osc(self, OscillatorType::Sawtooth);
            o.frequency.set_value_at_time(180.0, t);
            o.frequency
                .exponential_ramp_to_value_at_time(2600.0, t + 0.28);
            let bp = biquad(self, B::Bandpass);
            bp.q.set_value(3.0);
            bp.frequency.set_value_at_time(600.0, t);
            bp.frequency
                .exponential_ramp_to_value_at_time(5200.0, t + 0.3);
            let g = gain(self);
            g.gain.set_value_at_time(0.0, t);
            g.gain.linear_ramp_to_value_at_time(0.16, t + 0.02);
            g.gain.exponential_ramp_to_value_at_time(0.0005, t + 0.45);
            let _ = o.connect(&bp);
            let _ = bp.connect(&g);
            let _ = g.connect(&gr.sfx_bus);
            let _ = o.start_at(t);
            let _ = o.stop_at(t + 0.5);
            for k in 0..6 {
                let time = t + k as f64 * 0.03 + self.random() * 0.02;
                self.noise_burst(NoiseBurst {
                    time,
                    dur: 0.025,
                    kind: B::Highpass,
                    freq: 3500.0,
                    q: 0.8,
                    gain: 0.08,
                    ..Default::default()
                });
            }
            return;
        }
        // The solenoid's "pssh", the flame lighting ("whump") and a rising rush.
        self.noise_burst(NoiseBurst {
            time: t,
            dur: 0.18,
            kind: B::Highpass,
            freq: 4500.0,
            q: 0.7,
            gain: 0.16,
            ..Default::default()
        });
        self.noise_burst(NoiseBurst {
            time: t + 0.04,
            dur: 0.65,
            kind: B::Bandpass,
            freq: 600.0,
            q: 0.9,
            gain: 0.26,
            sweep_to: Some(4500.0),
            ..Default::default()
        });
        let o = osc(self, OscillatorType::Sine);
        o.frequency.set_value_at_time(95.0, t + 0.04);
        o.frequency
            .exponential_ramp_to_value_at_time(38.0, t + 0.35);
        let g = gain(self);
        g.gain.set_value_at_time(0.0, t + 0.04);
        g.gain.linear_ramp_to_value_at_time(0.4, t + 0.05);
        g.gain.exponential_ramp_to_value_at_time(0.0005, t + 0.4);
        let _ = o.connect(&g);
        let _ = g.connect(&gr.sfx_bus);
        let _ = o.start_at(t + 0.04);
        let _ = o.stop_at(t + 0.45);
        let o2 = osc(self, OscillatorType::Sawtooth);
        o2.frequency.set_value_at_time(55.0, t);
        o2.frequency
            .exponential_ramp_to_value_at_time(110.0, t + 0.4);
        let lp = biquad(self, B::Lowpass);
        lp.frequency.set_value(300.0);
        let g2 = gain(self);
        g2.gain.set_value_at_time(0.0, t);
        g2.gain.linear_ramp_to_value_at_time(0.14, t + 0.03);
        g2.gain.exponential_ramp_to_value_at_time(0.0005, t + 0.5);
        let _ = o2.connect(&lp);
        let _ = lp.connect(&g2);
        let _ = g2.connect(&gr.sfx_bus);
        let _ = o2.start_at(t);
        let _ = o2.stop_at(t + 0.55);
        self.pop(t + 0.03, 0.6);
    }

    /// Short UI sounds for the menus: 'click' (buttons, tabs), 'start' (go racing).
    pub fn ui_click(&mut self, kind: &str) {
        if !self.sfx_on() {
            return;
        }
        let t = self.now();
        let gr = self.graph();
        let start = kind == "start";
        let blips: &[(f64, f64)] = if start {
            &[(880.0, 0.0), (1318.5, 0.06), (1760.0, 0.12)]
        } else {
            &[(1500.0, 0.0)]
        };
        for &(f, dt) in blips {
            let o = osc(self, OscillatorType::Triangle);
            o.frequency.set_value_at_time(f * 1.3, t + dt);
            o.frequency
                .exponential_ramp_to_value_at_time(f, t + dt + 0.02);
            let g = gain(self);
            g.gain.set_value_at_time(0.0, t + dt);
            g.gain.linear_ramp_to_value_at_time(0.09, t + dt + 0.002);
            g.gain.exponential_ramp_to_value_at_time(
                0.0005,
                t + dt + if start { 0.16 } else { 0.06 },
            );
            let _ = o.connect(&g);
            let _ = g.connect(&gr.sfx_bus);
            let _ = o.start_at(t + dt);
            let _ = o.stop_at(t + dt + 0.2);
        }
        self.noise_burst(NoiseBurst {
            time: t,
            dur: 0.015,
            kind: B::Highpass,
            freq: 6000.0,
            q: 0.7,
            gain: 0.05,
            ..Default::default()
        });
    }

    /// A brass-and-bells lift over a crash; the music dips under it.
    pub fn finish_fanfare(&mut self) {
        if !self.sfx_on() {
            return;
        }
        let t0 = self.now() + 0.05;
        let gr = self.graph();
        let d = &gr.music_duck.gain;
        d.cancel_scheduled_values(t0);
        d.set_target_at_time(0.35, t0, 0.08);
        d.set_target_at_time(1.0, t0 + 2.4, 0.6);
        const BRASS: Patch = Patch {
            kind: "saw",
            voices: Some(3.0),
            detune: Some(14.0),
            width: Some(0.5),
            cutoff: Some(2600.0),
            q: Some(1.2),
            fenv: Some(1.2),
            fdec: Some(0.2),
            a: Some(0.015),
            d: Some(0.3),
            s: Some(0.8),
            r: Some(0.25),
            vib: Some(10.0),
            vib_delay: Some(0.2),
            gain: Some(0.11),
            ..Patch::NONE
        };
        const BELL: Patch = Patch {
            kind: "fm",
            mods: Some(&[FmMod {
                ratio: 3.5,
                index: 2.0,
                dec: Some(0.6),
                sus: Some(0.05),
            }]),
            a: Some(0.002),
            d: Some(1.0),
            s: Some(0.1),
            r: Some(0.8),
            gain: Some(0.07),
            ..Patch::NONE
        };
        let out = gain(self);
        out.gain.set_value(1.0);
        let _ = out.connect(&gr.sfx_bus);
        let m = self.music.as_ref().expect("music");
        for (n, dt) in [
            (69.0, 0.0),
            (72.0, 0.14),
            (76.0, 0.28),
            (81.0, 0.42),
            (76.0, 0.62),
            (81.0, 0.76),
        ] {
            m.note(&out, t0 + dt, &[n], 0.18, &BRASS, 0.9, None);
        }
        m.note(
            &out,
            t0 + 0.95,
            &[57.0, 64.0, 69.0, 73.0, 76.0],
            1.6,
            &Patch {
                a: Some(0.04),
                gain: Some(0.16),
                ..BRASS
            },
            1.0,
            None,
        );
        for (n, dt) in [(81.0, 0.95), (85.0, 1.05), (88.0, 1.15), (93.0, 1.25)] {
            m.note(&out, t0 + dt, &[n], 0.3, &BELL, 0.9, None);
        }
        let crash = m.kit_buffer("crash").cloned();
        let kick = m.kit_buffer("kickPunch").cloned();
        self.play(
            crash.as_ref(),
            PlayOpts {
                time: Some(t0 + 0.95),
                gain: 0.3,
                ..Default::default()
            },
        );
        self.play(
            kick.as_ref(),
            PlayOpts {
                time: Some(t0 + 0.95),
                gain: 0.5,
                ..Default::default()
            },
        );
    }

    pub fn set_rival_engines(&mut self, list: &[Rival]) {
        if !self.steer() {
            return;
        }
        let t = self.now();
        let gr = self.graph();
        for (i, v) in gr.rivals.iter().enumerate() {
            let r = list.get(i);
            let near = match r {
                Some(r) => clamp(1.0 - r.dist.unwrap_or(100.0) / 70.0, 0.0, 1.0),
                None => 0.0,
            };
            if !self.gate_set(v.gate, near > 0.0, t) {
                continue;
            }
            let Some(r) = r else {
                v.g.gain.set_target_at_time(0.0, t, 0.1);
                continue;
            };
            let rn = clamp(r.rpm_norm.unwrap_or(0.5), 0.0, 1.0);
            // Electric rivals whine instead of burbling.
            let ev = r.electric == Some(true);
            if ev != self.rival_ev[i] {
                self.rival_ev[i] = ev;
                if ev {
                    v.osc.set_type(OscillatorType::Sawtooth);
                } else if let Some(w) = &self.rival_wave {
                    v.osc.set_periodic_wave(w);
                }
            }
            v.osc.frequency.set_target_at_time(
                if ev {
                    40.0 + rn * 700.0
                } else {
                    (1200.0 + rn * 6000.0) / 120.0
                },
                t,
                0.05,
            );
            v.lp.frequency.set_target_at_time(
                if ev {
                    900.0 + rn * 2600.0
                } else {
                    500.0 + rn * 1600.0
                },
                t,
                0.05,
            );
            v.g.gain
                .set_target_at_time((if ev { 0.035 } else { 0.09 }) * near * near, t, 0.08);
            v.p.pan
                .set_target_at_time(clamp(r.pan.unwrap_or(0.0), -1.0, 1.0), t, 0.05);
        }
    }

    // ── Hot Pursuit ───────────────────────────────────────────────────
    /// The nearest police units' sirens, every frame (like
    /// setRivalEngines): mode 'wail' | 'yelp' | 'hilo' | 'off'. Up to 3
    /// sound. Items with an `id` keep their voice from frame to frame;
    /// without one, voices go by list order. An empty list fades them all
    /// out.
    pub fn set_sirens(&mut self, list: &[SirenUnit]) {
        if !self.steer() {
            return;
        }
        let t = self.now();
        let gr = self.graph();
        let nv = gr.sirens.len();
        let mut slots: Vec<Option<&SirenUnit>> = vec![None; nv];
        let mut rest = Vec::new();
        for it in list.iter().take(nv) {
            let k = match it.id {
                None => None,
                Some(id) => (0..nv).find(|&i| slots[i].is_none() && self.sirens[i].id == Some(id)),
            };
            match k {
                Some(k) => slots[k] = Some(it),
                None => rest.push(it),
            }
        }
        for it in rest {
            // A new unit takes a voice nobody holds, if there is one.
            let k = (0..nv)
                .find(|&i| slots[i].is_none() && self.sirens[i].id.is_none())
                .or_else(|| (0..nv).find(|&i| slots[i].is_none()))
                .expect("a free voice");
            slots[k] = Some(it);
        }
        for (i, v) in gr.sirens.iter().enumerate() {
            let it = slots[i];
            let pat = it.and_then(|it| it.mode.as_deref().and_then(siren_pattern));
            self.sirens[i].id = it.and_then(|it| it.id);
            let dist = js::max(0.0, it.and_then(|it| it.dist).unwrap_or(100.0));
            let lvl = if pat.is_some() {
                siren_level(Some(dist))
            } else {
                0.0
            };
            // Shut: its pattern LFOs keep the last mode applied (off: none), so a
            // unit coming back in range picks up any change once it reconnects.
            if !self.gate_set(v.gate, lvl > 0.0, t) {
                if pat.is_none() {
                    self.sirens[i].mode = "off".into();
                }
                continue;
            }
            let (Some(it), Some(pat)) = (it, pat) else {
                v.g.gain.set_target_at_time(0.0, t, 0.12);
                self.sirens[i].mode = "off".into();
                continue;
            };
            let mode = it.mode.clone().unwrap_or_default();
            if mode != self.sirens[i].mode {
                self.sirens[i].mode = mode;
                let depth = 1200.0 * log2(pat.hi / (pat.lo * pat.hi).sqrt()); // cents either side
                let tri = pat.shape == "tri";
                let drift = self.sirens[i].drift;
                v.tri_d
                    .gain
                    .set_target_at_time(if tri { depth } else { 0.0 }, t, 0.05);
                v.sq_d
                    .gain
                    .set_target_at_time(if tri { 0.0 } else { depth }, t, 0.05);
                (if tri { &v.tri } else { &v.sq })
                    .frequency
                    .set_target_at_time(1.0 / (pat.period * drift), t, 0.05);
            }
            let f = (pat.lo * pat.hi).sqrt() * siren_doppler(Some(it.rel_speed.unwrap_or(0.0)));
            v.a.frequency.set_target_at_time(f, t, 0.06);
            v.b.frequency.set_target_at_time(f, t, 0.06);
            v.lp.frequency
                .set_target_at_time(900.0 + 9000.0 * (30.0 / (30.0 + dist)), t, 0.08);
            v.g.gain.set_target_at_time(lvl, t, 0.06);
            v.p.pan
                .set_target_at_time(clamp(it.pan.unwrap_or(0.0), -1.0, 1.0), t, 0.05);
        }
    }

    /// A siren chirp over an air-horn blast (the pursuit starts).
    pub fn siren_horn(&mut self, pan: f64) {
        if !self.sfx_on() {
            return;
        }
        let t = self.now();
        self.siren_blip(t, pan, 1.0);
    }

    fn siren_blip(&mut self, t: f64, pan: f64, level: f64) {
        let ctx = self.ctx.clone().expect("a context");
        let gr = self.graph();
        let out = gain(self);
        out.gain.set_value(level);
        if truthy(pan) {
            let p = ctx.create_stereo_panner();
            p.pan.set_value(clamp(pan, -1.0, 1.0));
            let _ = out.connect(&p);
            let _ = p.connect(&gr.sfx_bus);
        } else {
            let _ = out.connect(&gr.sfx_bus);
        }
        // Two quick whoops up the siren's range.
        let bp = biquad(self, B::Bandpass);
        bp.frequency.set_value(1150.0);
        bp.q.set_value(0.9);
        let wg = gain(self);
        wg.gain.set_value_at_time(0.0, t);
        for (dt, len) in [(0.0, 0.2), (0.24, 0.3)] {
            wg.gain.set_target_at_time(0.16, t + dt, 0.01);
            wg.gain.set_target_at_time(0.0, t + dt + len - 0.05, 0.02);
        }
        let _ = bp.connect(&wg);
        let _ = wg.connect(&out);
        for (ty, det, gv) in [
            (OscillatorType::Square, 0.0, 0.55),
            (OscillatorType::Sawtooth, 18.0, 0.45),
        ] {
            let o = osc(self, ty);
            o.detune.set_value(det);
            o.frequency.set_value_at_time(600.0, t);
            o.frequency
                .exponential_ramp_to_value_at_time(1450.0, t + 0.16);
            o.frequency.set_value_at_time(700.0, t + 0.24);
            o.frequency
                .exponential_ramp_to_value_at_time(1500.0, t + 0.44);
            o.frequency
                .exponential_ramp_to_value_at_time(1100.0, t + 0.54);
            let og = gain(self);
            og.gain.set_value(gv);
            let _ = o.connect(&og);
            let _ = og.connect(&bp);
            let _ = o.start_at(t);
            let _ = o.stop_at(t + 0.6);
        }
        // Air horn: two low, buzzy reeds a minor third apart.
        let lp = biquad(self, B::Lowpass);
        lp.frequency.set_value(2200.0);
        lp.q.set_value(1.5);
        let hg = gain(self);
        hg.gain.set_value_at_time(0.0, t);
        hg.gain.linear_ramp_to_value_at_time(0.1, t + 0.02);
        hg.gain.set_target_at_time(0.0, t + 0.3, 0.04);
        let _ = lp.connect(&hg);
        let _ = hg.connect(&out);
        for f in [277.0, 330.0] {
            let o = osc(self, OscillatorType::Sawtooth);
            o.frequency.set_value_at_time(f * 0.94, t);
            o.frequency.exponential_ramp_to_value_at_time(f, t + 0.04);
            let _ = o.connect(&lp);
            let _ = o.start_at(t);
            let _ = o.stop_at(t + 0.5);
        }
    }

    /// A line of police radio (its spoken parts, see radioLines.js) in the
    /// recorded voice, or the burble if its clips aren't in within a moment:
    /// the words land with the text on screen or not at all.
    pub fn radio_line(&mut self, parts: &[String], pan: f64) {
        if !self.sfx_on() {
            return;
        }
        self.radio_seq += 1;
        let n = self.radio_seq;
        let ctx = self.ctx.clone().expect("a context");
        let voice = self
            .radio_voice
            .buffers(&ctx, parts, self.platform.random.clone());
        let now = self.now();
        self.timers.set_timeout(now, RADIO_WAIT, Task::RadioWait(n));
        let len = parts.join(" ").encode_utf16().count() as f64;
        self.radio_lines.push(PendingLine {
            seq: n,
            voice,
            pan,
            duration: 1.0 + js::min(2.0, len / 30.0),
        });
        // The race resolves when the promises settle (GameAudio::settle).
    }

    /// The radio lines whose clips are in (or known to be missing) go out.
    /// Returns whether one did.
    pub(super) fn poll_radio(&mut self) -> bool {
        let Some(i) = self.radio_lines.iter().position(|l| l.voice.is_settled()) else {
            return false;
        };
        let l = self.radio_lines.remove(i);
        let voice = l.voice.result().and_then(|r| r.ok()).flatten();
        // A newer line has the channel.
        if l.seq == self.radio_seq {
            self.radio(l.duration, l.pan, voice);
        }
        true
    }

    /// The 700 ms are up: a line still waiting gets the burble.
    pub(super) fn radio_wait(&mut self, seq: u32) {
        let Some(i) = self.radio_lines.iter().position(|l| l.seq == seq) else {
            return;
        };
        let l = self.radio_lines.remove(i);
        if l.seq == self.radio_seq {
            self.radio(l.duration, l.pan, None);
        }
    }

    /// Police radio chatter between squelch clicks, on its own crunchy bus:
    /// voice (AudioBuffers, played one after another) or, without it, a burst
    /// of band-limited gibberish. One channel: a new call cuts off one that is
    /// still talking (the newest message always gets through).
    pub fn radio(&mut self, duration: f64, pan: f64, voice: Option<Vec<AudioBuffer>>) {
        if !self.sfx_on() {
            return;
        }
        let ctx = self.ctx.clone().expect("a context");
        let gr = self.graph();
        let t0 = self.now() + 0.01;
        self.gate_set(gr.vg.radio, true, t0);
        let talk = match &voice {
            Some(v) => {
                v.iter().fold(0.0, |a, b| a + b.duration())
                    + RADIO_GAP * (v.len() as f64 - 1.0)
                    + 0.12
            }
            None => clamp(duration, 0.4, 6.0),
        };
        let end = t0 + talk;
        self.radio_cut(t0);
        let rd = &gr.radio_bus;
        rd.p.pan.set_value_at_time(clamp(pan, -1.0, 1.0), t0);
        let g = gain(self);
        g.gain.set_value(1.0);
        let _ = g.connect(&rd.input);
        let gn: Node = (*g).clone();
        let mut srcs = Vec::new();
        // Squelch open: a key-up click and a short rush.
        self.noise_burst(NoiseBurst {
            time: t0,
            dur: 0.012,
            kind: B::Highpass,
            freq: 1200.0,
            q: 0.7,
            gain: 0.7,
            dest: Some(gn.clone()),
            ..Default::default()
        });
        self.noise_burst(NoiseBurst {
            time: t0,
            dur: 0.07,
            kind: B::Bandpass,
            freq: 1800.0,
            q: 0.5,
            gain: 0.25,
            dest: Some(gn.clone()),
            ..Default::default()
        });
        // Carrier hiss under the voice.
        let hiss = ctx.create_buffer_source();
        let _ = hiss.set_buffer(Some(&gr.noise_white));
        hiss.set_loop(true);
        let hf = biquad(self, B::Bandpass);
        hf.frequency.set_value(1700.0);
        hf.q.set_value(0.4);
        let hg = gain(self);
        hg.gain.set_value(0.045);
        let _ = hiss.connect(&hf);
        let _ = hf.connect(&hg);
        let _ = hg.connect(&g);
        let off = self.random() * 1.5;
        let _ = hiss.start_with(t0, off, None);
        let _ = hiss.stop_at(end + 0.2);
        srcs.push(Src::Buf(hiss));
        if let Some(v) = &voice {
            let vg = gain(self);
            vg.gain.set_value(RADIO_VOICE);
            let _ = vg.connect(&g);
            let mut at = t0 + 0.08;
            for b in v {
                let s = ctx.create_buffer_source();
                let _ = s.set_buffer(Some(b));
                let _ = s.connect(&vg);
                let _ = s.start_at(at);
                srcs.push(Src::Buf(s));
                at += b.duration() + RADIO_GAP;
            }
        } else {
            self.burble(&g, t0, end, &mut srcs);
        }
        // Squelch close: the "kssht" as the carrier drops.
        self.noise_burst(NoiseBurst {
            time: end,
            dur: 0.16,
            kind: B::Bandpass,
            freq: 2000.0,
            q: 0.4,
            gain: 0.45,
            dest: Some(gn.clone()),
            ..Default::default()
        });
        self.noise_burst(NoiseBurst {
            time: end + 0.005,
            dur: 0.01,
            kind: B::Highpass,
            freq: 1500.0,
            q: 0.7,
            gain: 0.5,
            dest: Some(gn),
            ..Default::default()
        });
        self.radio_cur = Some(RadioCur {
            g,
            end: end + 0.2,
            srcs,
        });
    }

    // Radio gibberish: a buzz (and some breath) through three moving formants.
    fn burble(&mut self, g: &GainNode, t0: f64, end: f64, srcs: &mut Vec<Src>) {
        let ctx = self.ctx.clone().expect("a context");
        let gr = self.graph();
        let dur = end - t0;
        let buzz = osc(self, OscillatorType::Sawtooth);
        let breath = ctx.create_buffer_source();
        let _ = breath.set_buffer(Some(&gr.noise_white));
        breath.set_loop(true);
        let br_g = gain(self);
        br_g.gain.set_value(0.12);
        let vox = gain(self);
        vox.gain.set_value(0.0);
        let mut ff = Vec::new();
        for (q, a) in [(5.0, 1.0), (8.0, 0.7), (10.0, 0.35)] {
            let f = biquad(self, B::Bandpass);
            f.q.set_value(q);
            let fg = gain(self);
            fg.gain.set_value(a * 6.0);
            let _ = buzz.connect(&f);
            let _ = br_g.connect(&f);
            let _ = f.connect(&fg);
            let _ = fg.connect(&vox);
            ff.push(f);
        }
        let _ = breath.connect(&br_g);
        let _ = vox.connect(g);
        const VOWELS: [[f64; 3]; 7] = [
            [730.0, 1090.0, 2440.0],
            [530.0, 1840.0, 2480.0],
            [270.0, 2290.0, 3010.0],
            [570.0, 840.0, 2410.0],
            [300.0, 870.0, 2240.0],
            [490.0, 1350.0, 1690.0],
            [660.0, 1720.0, 2410.0],
        ];
        let f0 = 95.0 + self.random() * 85.0;
        let mut at = t0 + 0.08;
        while at < end - 0.12 {
            let len = 0.06 + self.random() * 0.15;
            let prog = (at - t0) / dur;
            let [a1, a2, a3] = VOWELS[(self.random() * VOWELS.len() as f64).floor() as usize];
            let s = 0.9 + self.random() * 0.2; // speaker's vocal tract
            ff[0]
                .frequency
                .set_target_at_time(js::max(380.0, a1 * s), at, 0.015); // the handset loses anything lower
            ff[1].frequency.set_target_at_time(a2 * s, at, 0.02);
            ff[2].frequency.set_target_at_time(a3 * s, at, 0.02);
            // Pitch sags across the phrase, with a lift or a drop on each syllable.
            let p = f0 * (1.08 - 0.18 * prog) * (1.0 + (self.random() - 0.5) * 0.14);
            buzz.frequency.set_target_at_time(p, at, 0.03);
            let p2 = p * (0.94 + self.random() * 0.1);
            buzz.frequency.set_target_at_time(p2, at + len * 0.5, 0.05);
            let peak = 0.5 + self.random() * 0.5;
            vox.gain.set_target_at_time(peak, at, 0.012);
            vox.gain.set_target_at_time(0.0, at + len * 0.75, 0.02);
            // Consonants: a hiss or a stop burst before some syllables.
            if self.random() < 0.35 {
                let d = 0.04 + self.random() * 0.04;
                let freq = 2200.0 + self.random() * 700.0;
                self.noise_burst(NoiseBurst {
                    time: at - 0.02,
                    dur: d,
                    kind: B::Bandpass,
                    freq,
                    q: 1.2,
                    gain: 0.25,
                    dest: Some((**g).clone()),
                    ..Default::default()
                });
            }
            at += len
                + if self.random() < 0.18 {
                    0.1 + self.random() * 0.12
                } else {
                    self.random() * 0.04
                };
        }
        buzz.frequency.set_value(f0);
        let _ = buzz.start_at(t0);
        let _ = buzz.stop_at(end + 0.05);
        srcs.push(Src::Osc(buzz));
        let off = self.random() * 1.5;
        let _ = breath.start_with(t0, off, None);
        let _ = breath.stop_at(end + 0.05);
        srcs.push(Src::Buf(breath));
    }

    pub(super) fn radio_cut(&mut self, t: f64) {
        let cur = self.radio_cur.take();
        let Some(cur) = cur else {
            return;
        };
        if cur.end <= t {
            return;
        }
        cur.g.gain.cancel_scheduled_values(t);
        cur.g.gain.set_target_at_time(0.0, t, 0.012);
        for s in &cur.srcs {
            s.stop_at(t + 0.08);
        }
    }

    /// BUSTED: a low brass stab ("dun... DUN") that climbs a semitone onto a
    /// dark minor chord, a kick under it, and a siren chirp. The music dips.
    pub fn busted(&mut self) {
        if !self.sfx_on() {
            return;
        }
        let t0 = self.now() + 0.03;
        let gr = self.graph();
        self.duck_music(t0, 0.3, 2.2);
        const BRASS: Patch = Patch {
            kind: "saw",
            voices: Some(3.0),
            detune: Some(16.0),
            width: Some(0.5),
            cutoff: Some(1500.0),
            q: Some(1.3),
            fenv: Some(1.6),
            fdec: Some(0.25),
            a: Some(0.01),
            d: Some(0.5),
            s: Some(0.65),
            r: Some(0.5),
            vib: Some(0.0),
            gain: Some(0.2),
            ..Patch::NONE
        };
        let out = gain(self);
        out.gain.set_value(1.0);
        let _ = out.connect(&gr.sfx_bus);
        let m = self.music.as_ref().expect("music");
        m.note(&out, t0, &[37.0, 44.0, 49.0], 0.13, &BRASS, 0.8, None);
        m.note(
            &out,
            t0 + 0.22,
            &[38.0, 45.0, 50.0, 53.0, 57.0],
            1.3,
            &Patch {
                cutoff: Some(2000.0),
                gain: Some(0.26),
                ..BRASS
            },
            1.0,
            None,
        );
        let kick = m.kit_buffer("kickPunch").cloned();
        let crash = m.kit_buffer("crash").cloned();
        self.play(
            kick.as_ref(),
            PlayOpts {
                time: Some(t0 + 0.22),
                gain: 0.6,
                ..Default::default()
            },
        );
        self.play(
            crash.as_ref(),
            PlayOpts {
                time: Some(t0 + 0.22),
                gain: 0.18,
                rate: 0.8,
                ..Default::default()
            },
        );
        let o = osc(self, OscillatorType::Sine);
        o.frequency.set_value_at_time(70.0, t0 + 0.22);
        o.frequency
            .exponential_ramp_to_value_at_time(34.0, t0 + 0.9);
        let og = gain(self);
        og.gain.set_value_at_time(0.0, t0 + 0.22);
        og.gain.linear_ramp_to_value_at_time(0.35, t0 + 0.23);
        og.gain.exponential_ramp_to_value_at_time(0.0005, t0 + 1.0);
        let _ = o.connect(&og);
        let _ = og.connect(&gr.sfx_bus);
        let _ = o.start_at(t0 + 0.22);
        let _ = o.stop_at(t0 + 1.05);
        self.siren_blip(t0 + 0.95, 0.0, 0.6);
    }

    /// ESCAPED: a suspended chord that resolves to major, bells on top and a
    /// falling breath of air. The music dips a little under it.
    pub fn escaped(&mut self) {
        if !self.sfx_on() {
            return;
        }
        let t0 = self.now() + 0.03;
        let gr = self.graph();
        self.duck_music(t0, 0.5, 2.0);
        const PAD: Patch = Patch {
            kind: "saw",
            voices: Some(3.0),
            detune: Some(12.0),
            width: Some(0.7),
            cutoff: Some(2400.0),
            q: Some(0.8),
            fenv: Some(0.8),
            fdec: Some(0.4),
            a: Some(0.06),
            d: Some(0.6),
            s: Some(0.8),
            r: Some(0.8),
            vib: Some(6.0),
            vib_delay: Some(0.3),
            gain: Some(0.12),
            ..Patch::NONE
        };
        const BELL: Patch = Patch {
            kind: "fm",
            mods: Some(&[FmMod {
                ratio: 3.5,
                index: 2.0,
                dec: Some(0.6),
                sus: Some(0.05),
            }]),
            a: Some(0.002),
            d: Some(1.0),
            s: Some(0.1),
            r: Some(0.8),
            gain: Some(0.06),
            ..Patch::NONE
        };
        let out = gain(self);
        out.gain.set_value(1.0);
        let _ = out.connect(&gr.sfx_bus);
        let m = self.music.as_ref().expect("music");
        m.note(&out, t0, &[50.0, 55.0, 57.0, 62.0], 0.5, &PAD, 0.85, None);
        m.note(
            &out,
            t0 + 0.5,
            &[50.0, 54.0, 57.0, 62.0, 66.0],
            1.7,
            &PAD,
            1.0,
            None,
        );
        for (n, dt) in [(74.0, 0.5), (78.0, 0.62), (81.0, 0.74), (86.0, 0.9)] {
            m.note(&out, t0 + dt, &[n], 0.4, &BELL, 0.9, None);
        }
        self.noise_burst(NoiseBurst {
            time: t0 + 0.4,
            dur: 1.4,
            kind: B::Bandpass,
            freq: 3200.0,
            q: 0.8,
            gain: 0.05,
            sweep_to: Some(500.0),
            ..Default::default()
        });
    }

    fn duck_music(&self, t: f64, depth: f64, hold: f64) {
        let gr = self.graph();
        let d = &gr.music_duck.gain;
        d.cancel_scheduled_values(t);
        d.set_target_at_time(depth, t, 0.08);
        d.set_target_at_time(1.0, t + hold, 0.6);
    }

    /// A police car taken out: the crash (impact) plus heavy crumpling metal
    /// and a distorted low crunch. strength 0..1.
    pub fn takedown(&mut self, strength: f64, pan: f64) {
        if !self.sfx_on() {
            return;
        }
        let ctx = self.ctx.clone().expect("a context");
        let t = self.now();
        let gr = self.graph();
        let s = clamp(strength, 0.0, 1.0);
        self.impact_t = -1.0; // never thinned out as wall grinding is
        self.impact(js::max(0.5, s), pan);
        let rate = 0.5 + self.random() * 0.1;
        self.play(
            gr.sfx("metal2"),
            PlayOpts {
                time: Some(t + 0.01),
                gain: 0.45 * (0.4 + 0.6 * s),
                rate,
                pan: pan * 0.5,
                ..Default::default()
            },
        );
        let rate = 0.62 + self.random() * 0.1;
        self.play(
            gr.sfx("metal1"),
            PlayOpts {
                time: Some(t + 0.05),
                gain: 0.35 * s,
                rate,
                pan: -pan * 0.3,
                ..Default::default()
            },
        );
        self.play(
            gr.sfx("metal3"),
            PlayOpts {
                time: Some(t + 0.12),
                gain: 0.2 * s,
                rate: 0.55,
                pan: pan * 0.3,
                ..Default::default()
            },
        );
        let src = ctx.create_buffer_source();
        let _ = src.set_buffer(Some(&gr.noise_brown));
        src.playback_rate.set_value(1.5);
        let bp = biquad(self, B::Bandpass);
        bp.frequency.set_value(380.0);
        bp.q.set_value(0.9);
        let sh = ctx.create_wave_shaper();
        let curve = self
            .pop_curve
            .get_or_insert_with(|| distortion_curve(6.0, 1024))
            .clone();
        sh.set_curve(Some(&curve));
        let g = gain(self);
        g.gain.set_value_at_time(0.0, t);
        g.gain
            .linear_ramp_to_value_at_time(0.3 * (0.4 + 0.6 * s), t + 0.005);
        g.gain.exponential_ramp_to_value_at_time(0.0005, t + 0.35);
        let _ = src.connect(&bp);
        let _ = bp.connect(&sh);
        let _ = sh.connect(&g);
        let _ = g.connect(&gr.sfx_bus);
        let off = self.random() * 1.5;
        let _ = src.start_with(t, off, Some(0.4));
    }

    /// Driving over a spike strip: the tyre bursts (a crack and a thump),
    /// then the air rushes out.
    pub fn spike_pop(&mut self, pan: f64) {
        if !self.sfx_on() {
            return;
        }
        let t = self.now();
        let gr = self.graph();
        self.noise_burst(NoiseBurst {
            time: t,
            dur: 0.05,
            kind: B::Highpass,
            freq: 400.0,
            q: 0.7,
            gain: 0.55,
            pan,
            ..Default::default()
        });
        self.noise_burst(NoiseBurst {
            time: t,
            dur: 0.1,
            kind: B::Bandpass,
            freq: 900.0,
            q: 1.0,
            gain: 0.3,
            pan,
            ..Default::default()
        });
        let o = osc(self, OscillatorType::Sine);
        o.frequency.set_value_at_time(120.0, t);
        o.frequency
            .exponential_ramp_to_value_at_time(40.0, t + 0.12);
        let og = gain(self);
        og.gain.set_value_at_time(0.45, t);
        og.gain.exponential_ramp_to_value_at_time(0.0005, t + 0.16);
        let _ = o.connect(&og);
        let _ = og.connect(&gr.sfx_bus);
        let _ = o.start_at(t);
        let _ = o.stop_at(t + 0.18);
        let h = self.noise_burst(NoiseBurst {
            time: t + 0.02,
            dur: 1.4,
            kind: B::Bandpass,
            freq: 6500.0,
            q: 0.9,
            gain: 0.12,
            sweep_to: Some(2200.0),
            pan,
            ..Default::default()
        });
        h.g.gain.cancel_scheduled_values(t + 0.02);
        h.g.gain.set_value_at_time(0.0, t + 0.02);
        h.g.gain.linear_ramp_to_value_at_time(0.12, t + 0.06);
        h.g.gain.exponential_ramp_to_value_at_time(0.0005, t + 1.42);
    }

    /// The player's car is done: a full crash, steam pouring out and the
    /// engine running down and dying.
    pub fn wrecked(&mut self) {
        if !self.sfx_on() {
            return;
        }
        let ctx = self.ctx.clone().expect("a context");
        let t = self.now();
        let gr = self.graph();
        self.impact_t = -1.0;
        self.impact(1.0, 0.0);
        self.play(
            gr.sfx("thud"),
            PlayOpts {
                time: Some(t + 0.02),
                gain: 0.6,
                rate: 0.7,
                ..Default::default()
            },
        );
        self.play(
            gr.sfx("metal3"),
            PlayOpts {
                time: Some(t + 0.18),
                gain: 0.25,
                rate: 0.5,
                ..Default::default()
            },
        );
        // Steam.
        let st = self.noise_burst(NoiseBurst {
            time: t + 0.15,
            dur: 3.2,
            kind: B::Highpass,
            freq: 3000.0,
            q: 0.7,
            gain: 0.1,
            ..Default::default()
        });
        st.g.gain.cancel_scheduled_values(t + 0.15);
        st.g.gain.set_value_at_time(0.0, t + 0.15);
        st.g.gain.linear_ramp_to_value_at_time(0.1, t + 0.5);
        st.g.gain.set_target_at_time(0.05, t + 0.6, 0.6);
        st.g.gain
            .exponential_ramp_to_value_at_time(0.0005, t + 3.35);
        // The engine runs down, stumbling, and stalls.
        let o = ctx.create_oscillator();
        let wave = if self.electric {
            None
        } else {
            self.waves
                .iter()
                .find(|(k, _)| *k == self.car)
                .map(|(_, w)| w.on_l.clone())
                .or_else(|| self.rival_wave.clone())
        };
        match &wave {
            Some(w) => o.set_periodic_wave(w),
            None => o.set_type(OscillatorType::Sawtooth),
        }
        let (fa, fb) = if wave.is_some() {
            (2600.0 / 120.0, 500.0 / 120.0)
        } else {
            (700.0, 40.0)
        };
        o.frequency.set_value_at_time(fa, t);
        o.frequency.exponential_ramp_to_value_at_time(fb, t + 2.2);
        let lp = biquad(self, B::Lowpass);
        lp.q.set_value(0.8);
        lp.frequency.set_value_at_time(1600.0, t);
        lp.frequency
            .exponential_ramp_to_value_at_time(250.0, t + 2.2);
        let am = gain(self);
        am.gain.set_value(0.6);
        let lfo = osc(self, OscillatorType::Square);
        lfo.frequency.set_value_at_time(9.0, t);
        lfo.frequency
            .exponential_ramp_to_value_at_time(2.5, t + 2.2);
        let lg = gain(self);
        lg.gain.set_value(0.4);
        let _ = lfo.connect(&lg);
        let _ = lg.connect(&am.gain);
        let g = gain(self);
        g.gain.set_value_at_time(0.0, t);
        g.gain
            .linear_ramp_to_value_at_time(if wave.is_some() { 0.5 } else { 0.08 }, t + 0.05);
        g.gain.set_target_at_time(0.0, t + 1.4, 0.35);
        let _ = o.connect(&lp);
        let _ = lp.connect(&am);
        let _ = am.connect(&g);
        let _ = g.connect(&gr.sfx_bus);
        let _ = o.start_at(t);
        let _ = o.stop_at(t + 3.0);
        let _ = lfo.start_at(t);
        let _ = lfo.stop_at(t + 3.0);
    }

    /// Shredded tyres flapping: on while the car runs on spiked tyres, the
    /// flap rate following road speed (m/s). Call it every frame, or on each
    /// change.
    pub fn set_spiked_tyres(&mut self, on: bool, speed: f64) {
        if !self.steer() {
            return;
        }
        let t = self.now();
        let gr = self.graph();
        let ty = &gr.tyres;
        let sp = if speed.is_nan() { 0.0 } else { speed.abs() };
        let lvl = if on {
            0.3 * clamp(sp / 4.0, 0.0, 1.0) * (0.6 + 0.4 * clamp(sp / 40.0, 0.0, 1.0))
        } else {
            0.0
        };
        if !self.gate_set(gr.vg.tyres, lvl > 0.0, t) {
            return;
        }
        // Two strips on a 0.33 m wheel: about one flap per metre travelled.
        ty.pulse
            .frequency
            .set_target_at_time(clamp(sp * 0.95, 1.0, 90.0), t, 0.05);
        ty.out
            .gain
            .set_target_at_time(lvl, t, if on { 0.06 } else { 0.1 });
    }

    /// Music under a pursuit: 'cooldown' muffles it (a ~900 Hz low-pass,
    /// eased over a second) and brings it down a little; 'pursuit' and
    /// 'off' open it up.
    pub fn set_pursuit_mood(&mut self, mood: &str) {
        self.mood = match mood {
            "cooldown" => "cooldown",
            "pursuit" => "pursuit",
            _ => "off",
        };
        if !self.running() {
            return;
        }
        let t = self.now();
        let gr = self.graph();
        let f = &gr.music_mood_lp.frequency;
        let cool = self.mood == "cooldown";
        let to = if cool { 900.0 } else { 20000.0 };
        if self.mood_to != to {
            // called every frame: only a change starts a ramp
            self.mood_to = to;
            f.cancel_scheduled_values(t);
            f.set_value_at_time(f.value(), t);
            f.exponential_ramp_to_value_at_time(to, t + 1.0);
        }
        gr.music_mood
            .gain
            .set_target_at_time(if cool { 0.75 } else { 1.0 }, t, 0.3);
    }

    /// Engine distress, 0 (fine) .. 1 (wrecked): silent below 0.5.
    pub fn set_damage(&mut self, d: f64) {
        self.damage = clamp(if d.is_nan() { 0.0 } else { d }, 0.0, 1.0);
    }

    /// Everything a pursuit leaves running goes quiet (menu, a new car).
    pub(super) fn pursuit_reset(&mut self) {
        if !self.ready() {
            return;
        }
        self.set_sirens(&[]);
        self.set_spiked_tyres(false, 0.0);
        if self.mood != "off" {
            self.set_pursuit_mood("off");
        }
        self.damage = 0.0;
        let t = self.now();
        self.radio_cut(t);
    }
}
