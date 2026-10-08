//! The lab's engine (`tools/music-lab/engine.js`), line for line. Plays a
//! song (a [`Track`], every part carrying a `lab` patch) on the lab's
//! instruments, sample-accurately, and mixes it the way sound.md 3.3 asks:
//! per-part channels, sends, sidechain pump, a song filter, then a bus with
//! glue compression and tape.
//!
//! ```text
//!   instruments ─ channel (drive, hp, pan, level) ─┬─ [pump] ─┐
//!   drum machine (per-lane level, pan) ────────────┤          ├─ song filter ─┐
//!          └ reverb and delay sends ─ ping-pong ───┘──────────┘               │
//!   risers, downlifters ──────────────────────────────────────────────────────┤
//!   reverb return ────────────────────────────────────────────────────────────┤
//!                          energy filter ─ glue ─ tape ─ master ─ limiter ─ out
//! ```
//!
//! Not ported: the page's keyboard and pads (`setAudition`, `noteOn`,
//! `noteOff`, `hit`, the audition instrument and its channel in `process`),
//! which the game does not use, and `kitTweak` (the page's live drum edits;
//! always empty in the game). The `onSection` / `onEnd` callbacks are
//! [`EngineEvent`]s the caller takes with [`Engine::take_events`].
//!
//! Added for the radio's tune-in (radio.md 7, DECISIONS D1151):
//! [`Engine::cue`] starts a song at any 16th with the bar's section state
//! as if it had played from the bar's start.
//!
//! No per-sample allocation: `pending`, `autos` and `fx` grow only when an
//! event fires.

use crate::dsp::{Compressor, Limiter, OnePole, PingPong, Reverb, Rng, Svf, Tape, clamp, tanh};
use crate::instruments::{Drums, Instrument, KitVoice, TriggerOpt, kit_voice, make_instrument};
use crate::js_round;
use crate::seq::{Event, Seq};
use crate::to_u32;
use crate::track::{Ch, Lab, Track, lookup};

/// Reverb sends of the game's default kit (Music.js KIT), per lane
/// (`LANE_REV[name]`; `None` for a lane not in the table).
fn lane_rev(name: &str) -> Option<f64> {
    Some(match name {
        "kick" => 0.0,
        "snare" => 0.25,
        "clap" => 0.3,
        "hat" => 0.0,
        "ohat" => 0.05,
        "ride" => 0.05,
        "crash" => 0.2,
        "revCrash" => 0.2,
        "shaker" => 0.05,
        "rim" => 0.2,
        "snap" => 0.35,
        "tomL" => 0.25,
        "tomM" => 0.25,
        "tomH" => 0.25,
        "boom" => 0.1,
        "cowbell" => 0.15,
        "congaO" => 0.2,
        "congaS" => 0.2,
        "tumba" => 0.15,
        "bongoH" => 0.2,
        "bongoL" => 0.2,
        "timbaleH" => 0.25,
        "timbaleL" => 0.25,
        "cascara" => 0.15,
        "guiroL" => 0.1,
        "guiroS" => 0.1,
        "clave" => 0.25,
        _ => return None,
    })
}

/// Lab mix: drums against the synths. This, the patch gains and the master
/// level were set by soloing every part in A and B and matching their RMS
/// (measure.js); B then plays at the loudness of A, so A/B is fair.
const DRUM_LEVEL: f64 = 0.263;

/// JS truthiness of a numeric field: `if (o.drive)` is false for a missing
/// key, 0 and NaN.
fn tru(o: Option<f64>) -> Option<f64> {
    o.filter(|v| *v != 0.0 && !v.is_nan())
}

/// A part's mixer channel (`Channel`): the instrument adds into `buf`;
/// `run` processes it and clears it for the next sample.
#[derive(Clone, Debug)]
pub struct Channel {
    pub buf: [f64; 2],
    drive: f64,
    dlp: [OnePole; 2],
    hp: Option<[Svf; 2]>,
    gl: f64,
    gr: f64,
    level: f64,
    pub rev: f64,
    pub dly: f64,
    pub pump: bool,
    /// `Math.tanh(drive)`: the real tanh, not dsp's Padé one.
    dn: f64,
}

impl Channel {
    pub fn new(o: &Ch, sr: f64) -> Channel {
        let drive = o.drive.unwrap_or(0.0);
        let dlp = [OnePole::new(o.drive_lp.unwrap_or(5000.0), sr); 2];
        let hp = tru(o.hp).map(|hz| [Svf::with(hz, 0.707, sr); 2]);
        let pan = o.pan.unwrap_or(0.0);
        Channel {
            buf: [0.0, 0.0],
            drive,
            dlp,
            hp,
            gl: (1.0 - pan).min(1.0),
            gr: (1.0 + pan).min(1.0),
            level: o.level.unwrap_or(1.0),
            rev: o.rev.unwrap_or(0.0),
            dly: o.dly.unwrap_or(0.0),
            pump: o.pump == Some(true),
            dn: if tru(Some(drive)).is_some() {
                drive.tanh()
            } else {
                1.0
            },
        }
    }

    /// Processes `buf` in place and clears it for the next sample; sets
    /// `out` to `[l, r]`.
    pub fn run(&mut self, out: &mut [f64; 2]) {
        let mut l = self.buf[0];
        let mut r = self.buf[1];
        self.buf[0] = 0.0;
        self.buf[1] = 0.0;
        if tru(Some(self.drive)).is_some() {
            l = self.dlp[0].lp(tanh(self.drive * l) / self.dn);
            r = self.dlp[1].lp(tanh(self.drive * r) / self.dn);
        }
        if let Some(hp) = &mut self.hp {
            hp[0].run(l);
            hp[1].run(r);
            l = hp[0].hp;
            r = hp[1].hp;
        }
        out[0] = l * self.gl * self.level;
        out[1] = r * self.gr * self.level;
    }
}

/// A noise sweep for risers and downlifters. The noise is the engine's own
/// generator (`this.r`), passed to `run`.
#[derive(Clone, Debug)]
struct Sweep {
    f: Svf,
    f0: f64,
    f1: f64,
    g0: f64,
    g1: f64,
    /// Samples (`Math.round(dur * SR)`).
    len: f64,
    q: f64,
    t: u64,
    sr: f64,
}

impl Sweep {
    #[allow(clippy::too_many_arguments)]
    fn new(f0: f64, f1: f64, g0: f64, g1: f64, len: f64, q: f64, sr: f64) -> Sweep {
        Sweep {
            f: Svf::new(),
            f0,
            f1,
            g0,
            g1,
            len,
            q,
            t: 0,
            sr,
        }
    }

    fn done(&self) -> bool {
        self.t as f64 >= self.len
    }

    fn run(&mut self, r: &mut Rng) -> f64 {
        let u = self.t as f64 / self.len;
        self.t += 1;
        if (self.t & 31) == 1 {
            self.f
                .set(self.f0 * (self.f1 / self.f0).powf(u), self.q, self.sr);
        }
        self.f.run(r.next() * 2.0 - 1.0);
        let g =
            self.g0 * (self.g1 / self.g0).powf(u) * (if u > 0.98 { (1.0 - u) / 0.02 } else { 1.0 });
        self.f.bp * g
    }
}

/// The bus settings (`this.mix`), with the JS defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mix {
    pub glue: f64,
    pub tape: f64,
    pub reverb: f64,
    pub rev_size: f64,
    pub rev_tone: f64,
    pub master: f64,
}

impl Default for Mix {
    fn default() -> Mix {
        Mix {
            glue: 0.5,
            tape: 0.35,
            reverb: 1.0,
            rev_size: 2.4,
            rev_tone: 0.5,
            master: 1.07,
        }
    }
}

/// A part of the song being played: its instrument (which owns the live
/// `lab` patch the automation writes into) and its channel.
#[derive(Clone, Debug)]
pub struct PartState {
    pub inst: Instrument,
    pub ch: Channel,
}

/// A running section automation (`this.autos[i]`): the part (an index
/// into `parts`), the parameter's JS name, the ramp, and samples run.
#[derive(Clone, Debug)]
struct Auto {
    part: usize,
    param: String,
    from: f64,
    to: f64,
    len: f64,
    t: f64,
}

/// What the JS `onSection` / `onEnd` callbacks reported, in firing order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineEvent {
    /// A section started (its index).
    Section(usize),
    /// The song wrapped past its last bar.
    End,
}

/// Where the song is (`engine.position`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Position {
    pub sec: usize,
    pub bar: u32,
    pub step: u32,
    pub bar_index: u32,
    pub bars: u32,
    pub playing: bool,
}

/// The engine: `new Engine({ seed, rate })`.
pub struct Engine {
    seed: u32,
    sr: f64,
    r: Rng,
    drums: Drums,
    kit: String,
    parts: Vec<(String, PartState)>,
    seq: Option<Seq>,
    playing: bool,
    /// Samples until the next 16th.
    next_step: i64,
    /// `[samplesLeft, event]`.
    pending: Vec<(i64, Event)>,
    autos: Vec<Auto>,
    fx: Vec<Sweep>,
    pump_g: f64,
    pump_t: u64,
    pump_on: bool,
    song_lp: [Svf; 2],
    lp_from: f64,
    lp_to: f64,
    lp_len: f64,
    lp_t: u64,
    energy: f64,
    en_lp: [Svf; 2],
    en_hz: f64,
    delay: PingPong,
    reverb: Reverb,
    glue: Compressor,
    tape: [Tape; 2],
    lim: Limiter,
    mix: Mix,
    t: u64,
    gr_sum: f64,
    gr_n: u64,
    events: Vec<EngineEvent>,
    /// The per-lane kit voice cache (`laneK`).
    lane_k: Vec<(String, KitVoice)>,
}

impl Engine {
    /// The JS defaults are `seed = 1, rate = 48000`.
    pub fn new(seed: u32, sr: f64) -> Engine {
        let mut e = Engine {
            seed,
            sr,
            r: Rng::new(seed),
            drums: Drums::new(to_u32(seed as f64 + 3.0), sr),
            kit: "tr909".to_owned(),
            parts: Vec::new(),
            seq: None,
            playing: false,
            next_step: 0,
            pending: Vec::new(),
            autos: Vec::new(),
            fx: Vec::new(),
            pump_g: 1.0,
            pump_t: 0,
            pump_on: false,
            song_lp: [Svf::new(); 2],
            lp_from: 20000.0,
            lp_to: 20000.0,
            lp_len: 0.0,
            lp_t: 0,
            energy: 1.0,
            en_lp: [Svf::new(); 2],
            en_hz: 20000.0,
            delay: PingPong::new(sr),
            reverb: Reverb::new(to_u32(seed as f64 + 11.0), sr),
            glue: Compressor::new(sr),
            tape: [Tape::new(sr); 2],
            lim: Limiter::new(0.95, sr),
            mix: Mix::default(),
            t: 0,
            gr_sum: 0.0,
            gr_n: 0,
            events: Vec::new(),
            lane_k: Vec::new(),
        };
        e.set_mix(Mix::default());
        e
    }

    pub fn sr(&self) -> f64 {
        self.sr
    }

    pub fn mix(&self) -> Mix {
        self.mix
    }

    /// `setMix(m)`: the whole mix (the JS merges a partial object).
    pub fn set_mix(&mut self, m: Mix) {
        self.mix = m;
        let x = &self.mix;
        self.glue
            .set(-10.0 - x.glue * 10.0, 1.0 + x.glue * 2.5, 0.012, 0.18);
        for t in &mut self.tape {
            t.set(x.tape);
        }
        self.reverb.set(x.rev_size, x.rev_tone);
    }

    pub fn set_kit(&mut self, name: &str) {
        self.kit = name.to_owned();
        self.lane_k.clear();
    }

    /// The kit voice of a lane, with the track's per-lane tweaks (`T.kit`):
    /// the sample, a gain on `lvl`, a reverb send (else the default kit's).
    /// `None` for a lane the kit lacks.
    fn lane(&mut self, name: &str) -> Option<KitVoice> {
        if let Some(k) = lookup(&self.lane_k, name) {
            return Some(*k);
        }
        let ko = self
            .seq
            .as_ref()
            .and_then(|s| s.track().kit.as_ref())
            .and_then(|k| lookup(k, name));
        let base = kit_voice(&self.kit, name, ko.and_then(|o| o.s.as_deref()))?;
        let mut k = base;
        k.lvl = Some(k.lvl.unwrap_or(1.0) * ko.and_then(|o| o.g).unwrap_or(1.0));
        k.rev = Some(ko.and_then(|o| o.rev).or(lane_rev(name)).unwrap_or(0.0));
        self.lane_k.push((name.to_owned(), k));
        Some(k)
    }

    /// A song: the game's format with `lab` patches on every part. The kit
    /// is the track's `kitName` if it has one.
    pub fn set_track(&mut self, t: &Track) {
        if let Some(kit) = &t.kit_name {
            self.kit = kit.clone();
        }
        self.lane_k.clear();
        self.parts.clear();
        for (i, (name, p)) in t.parts.iter().enumerate() {
            let lab = p.lab.clone();
            // `this.seed * 31 + i++`: a double in the JS.
            let inst = make_instrument(lab, to_u32(self.seed as f64 * 31.0 + i as f64), self.sr);
            self.parts.push((
                name.clone(),
                PartState {
                    inst,
                    ch: Channel::new(&p.ch, self.sr),
                },
            ));
        }
        self.delay.time = t.delay * (60.0 / t.bpm);
        self.delay.fb = t.delay_fb;
        self.pending.clear();
        self.autos.clear();
        self.fx.clear();
        self.lp_from = 20000.0;
        self.lp_to = 20000.0;
        self.lp_len = 0.0;
        self.next_step = 0;
        // A new `Seq` starts at energy 1 whatever the engine's, as in the
        // JS: call `set_energy` after `set_track` (the lab and the goldens
        // do).
        self.seq = Some(Seq::new(t.clone()));
    }

    /// `setPatch(part, lab)`: `Object.assign(P.lab, lab)`, so every member
    /// `lab` has replaces the part's; the rest stay.
    pub fn set_patch(&mut self, part: &str, lab: Lab) {
        let Some((_, p)) = self.parts.iter_mut().find(|(n, _)| n == part) else {
            return;
        };
        let mut merged = p.inst.lab().clone();
        merge_lab(&mut merged, &lab);
        p.inst.set_patch(merged);
    }

    /// Starts at a bar of the song; the first 16th fires after `delay_s`
    /// (the JS default is 0.02).
    pub fn play(&mut self, bar: u32, delay_s: f64) {
        let Some(seq) = &mut self.seq else { return };
        seq.seek_bar(bar);
        self.playing = true;
        self.next_step = js_round(delay_s * self.sr).max(1.0) as i64;
        self.pending.clear();
    }

    /// Starts at a 16th of the song (`bar = step / 16`, `s = step % 16`),
    /// for the radio's tune-in (radio.md 7, D1151): seeks to the bar, then
    /// fast-forwards the bar's first `s` steps so the section state is as
    /// if the bar had played from its start: each skipped step is stepped
    /// through the sequencer and only its `Section`, `Lp` and `Auto` events
    /// fire (no drums, notes, risers, downlifters or end), then the song
    /// filter's and the automations' sample counters advance by the skipped
    /// samples (`s * round(step_dur * sr)`): the automations apply their
    /// value at the next 64-sample tick, the filter recomputes at its next
    /// 32-sample tick. The next real step fires after `delay_s`, as `play`
    /// does; `cue(16 * bar, d)` is `play(bar, d)`.
    pub fn cue(&mut self, step: u32, delay_s: f64) {
        let Some(seq) = &mut self.seq else { return };
        let bar = step / 16;
        let s = step % 16;
        seq.seek_bar(bar);
        self.playing = true;
        self.pending.clear();
        let first_auto = self.autos.len();
        let mut lp_fired = false;
        for _ in 0..s {
            let evs = self.seq.as_mut().expect("a sequencer").step();
            for e in evs {
                match e {
                    Event::Section { .. } | Event::Auto { .. } => self.fire(e),
                    Event::Lp { .. } => {
                        lp_fired = true;
                        self.fire(e);
                    }
                    _ => {}
                }
            }
        }
        let step_dur = self.seq.as_ref().expect("a sequencer").step_dur;
        let skipped = s as f64 * js_round(step_dur * self.sr);
        if lp_fired {
            self.lp_t += skipped as u64;
        }
        for a in &mut self.autos[first_auto..] {
            a.t += skipped;
        }
        self.next_step = js_round(delay_s * self.sr).max(1.0) as i64;
    }

    pub fn stop(&mut self) {
        self.playing = false;
        self.pending.clear();
    }

    pub fn playing(&self) -> bool {
        self.playing
    }

    pub fn set_energy(&mut self, e: f64) {
        self.energy = clamp(e, 0.0, 1.0);
        if let Some(seq) = &mut self.seq {
            seq.energy = self.energy;
        }
    }

    pub fn energy(&self) -> f64 {
        self.energy
    }

    pub fn seq(&self) -> Option<&Seq> {
        self.seq.as_ref()
    }

    pub fn seq_mut(&mut self) -> Option<&mut Seq> {
        self.seq.as_mut()
    }

    /// The parts being played, in the track's order.
    pub fn parts(&self) -> &[(String, PartState)] {
        &self.parts
    }

    /// A part's instrument's live patch (section automation writes into
    /// it).
    pub fn part_lab(&self, name: &str) -> Option<&Lab> {
        lookup(&self.parts, name).map(|p| p.inst.lab())
    }

    fn drum(&mut self, lane: &str, vel: f64, len: Option<f64>) {
        let Some(k) = self.lane(lane) else { return };
        let dur = match tru(len) {
            Some(len) => js_round(len * self.sr) as usize,
            None => 0,
        };
        self.drums.hit(lane, &k, vel, 1.0, dur);
        // `this.T?.pump`: every grammar sets the pump, so a track has one.
        if lane == "kick" && self.seq.is_some() && self.playing {
            self.pump_t = 0;
            self.pump_on = true;
        }
    }

    fn fire(&mut self, e: Event) {
        let sr = self.sr;
        match e {
            Event::Drum { lane, vel, len, .. } => self.drum(&lane, vel, len),
            Event::Note {
                part,
                midis,
                vel,
                dur,
                glide_from,
                accent,
                ..
            } => {
                if let Some(p) = self.parts.iter_mut().find(|(n, _)| *n == part) {
                    p.1.inst.trigger(
                        &midis,
                        vel,
                        js_round(dur * sr),
                        &TriggerOpt {
                            glide_from: glide_from.flatten(),
                            accent: accent == Some(true),
                        },
                    );
                }
            }
            Event::Lp { from, to, dur } => {
                self.lp_from = from;
                self.lp_to = to;
                self.lp_len = js_round(dur * sr);
                self.lp_t = 0;
            }
            Event::Auto {
                target,
                from,
                to,
                dur,
            } => {
                let mut it = target.split('.');
                let part = it.next().unwrap_or("");
                let param = it.next().unwrap_or("");
                if let Some(pi) = self.parts.iter().position(|(n, _)| n == part) {
                    self.autos.push(Auto {
                        part: pi,
                        param: param.to_owned(),
                        from,
                        to,
                        len: js_round(dur * sr),
                        t: 0.0,
                    });
                }
            }
            Event::Riser { dur } => self.fx.push(Sweep::new(
                350.0,
                7500.0,
                0.002,
                0.16,
                js_round(dur * sr),
                1.4,
                sr,
            )),
            Event::Down { dur } => self.fx.push(Sweep::new(
                6000.0,
                250.0,
                0.12,
                0.0005,
                js_round(dur * sr),
                1.2,
                sr,
            )),
            Event::Section { sec } => self.events.push(EngineEvent::Section(sec)),
            Event::End => self.events.push(EngineEvent::End),
        }
    }

    fn clock(&mut self) {
        if !self.playing {
            return;
        }
        self.next_step -= 1;
        if self.next_step > 0 {
            // From the end, as the JS splices while it walks.
            let mut i = self.pending.len();
            while i > 0 {
                i -= 1;
                self.pending[i].0 -= 1;
                if self.pending[i].0 <= 0 {
                    let (_, e) = self.pending.remove(i);
                    self.fire(e);
                }
            }
            return;
        }
        let seq = self.seq.as_mut().expect("playing needs a sequencer");
        self.next_step += js_round(seq.step_dur * self.sr) as i64;
        let evs = seq.step();
        for e in evs {
            let dt = match &e {
                Event::Drum { dt, .. } | Event::Note { dt, .. } => *dt,
                _ => 0.0,
            };
            let d = js_round(dt * self.sr) as i64;
            if d > 0 {
                self.pending.push((d, e));
            } else {
                self.fire(e);
            }
        }
    }

    /// Renders `l.len()` stereo samples into `l` and `r` (overwritten).
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        let sr = self.sr;
        let mut s = [0.0f64; 2];
        let mut o = [0.0f64; 2];
        let mut rv = [0.0f64; 2];
        let mut dl = [0.0f64; 2];
        let (pump_depth, pump_rel, gain) = match &self.seq {
            Some(seq) => {
                let t = seq.track();
                (t.pump.depth, t.pump.release / 3.0, t.gain)
            }
            None => (0.6, 0.16 / 3.0, 1.0),
        };
        let k_pump = 1.0 - (-1.0 / (pump_rel * sr)).exp();
        let en_hz = 250.0 * 2f64.powf(self.energy * 6.3);
        for i in 0..n {
            self.clock();
            // Automation (once every 64 samples is plenty).
            if (self.t & 63) == 0 {
                let mut a = self.autos.len();
                while a > 0 {
                    a -= 1;
                    let au = &mut self.autos[a];
                    au.t += 64.0;
                    let u = (au.t / au.len).min(1.0);
                    let v = au.from + (au.to - au.from) * u;
                    let pi = au.part;
                    self.parts[pi].1.inst.lab_mut().set(&au.param, v);
                    if u >= 1.0 {
                        self.autos.remove(a);
                    }
                }
                self.en_hz += (en_hz - self.en_hz) * 0.05;
            }
            self.t += 1;
            // Pump: down in 6 ms, held to 30 ms, then back with the release.
            if self.pump_on {
                let ms = (self.pump_t as f64 / sr) * 1000.0;
                self.pump_t += 1;
                if ms < 6.0 {
                    self.pump_g = 1.0 - pump_depth * (ms / 6.0);
                } else if ms > 30.0 {
                    self.pump_g += (1.0 - self.pump_g) * k_pump;
                    if self.pump_g > 0.999 {
                        self.pump_on = false;
                    }
                }
            }
            let mut pl = 0.0;
            let mut pr = 0.0;
            let mut ul = 0.0;
            let mut ur = 0.0;
            rv[0] = 0.0;
            rv[1] = 0.0;
            dl[0] = 0.0;
            dl[1] = 0.0;
            for (_, p) in &mut self.parts {
                p.inst.run(&mut p.ch.buf);
                p.ch.run(&mut o);
                if p.ch.pump {
                    pl += o[0];
                    pr += o[1];
                } else {
                    ul += o[0];
                    ur += o[1];
                }
                if tru(Some(p.ch.rev)).is_some() {
                    rv[0] += o[0] * p.ch.rev;
                    rv[1] += o[1] * p.ch.rev;
                }
                if tru(Some(p.ch.dly)).is_some() {
                    dl[0] += (o[0] + o[1]) * 0.5 * p.ch.dly;
                }
            }
            let mut d_l = 0.0;
            let mut d_r = 0.0;
            self.drums.run(|h, y| {
                let g = y * DRUM_LEVEL;
                let p = h.pan;
                let l = g * (1.0 - p).min(1.0);
                let r = g * (1.0 + p).min(1.0);
                d_l += l;
                d_r += r;
                if let Some(rev) = tru(h.k.rev) {
                    rv[0] += l * rev;
                    rv[1] += r * rev;
                }
            });
            // Song filter over the instruments, drums and the delay return.
            s[0] = 0.0;
            s[1] = 0.0;
            self.delay.run(dl[0], &mut s);
            let mut ml = pl * self.pump_g + ul + d_l + s[0];
            let mut mr = pr * self.pump_g + ur + d_r + s[1];
            if self.lp_from < 19999.0 || self.lp_to < 19999.0 {
                if (self.lp_t & 31) == 0 {
                    let u = if self.lp_len != 0.0 {
                        (self.lp_t as f64 / self.lp_len).min(1.0)
                    } else {
                        1.0
                    };
                    let hz = self.lp_from * (self.lp_to / self.lp_from).powf(u);
                    self.song_lp[0].set(hz, 0.9, sr);
                    self.song_lp[1].set(hz, 0.9, sr);
                }
                self.lp_t += 1;
                ml = self.song_lp[0].run(ml);
                mr = self.song_lp[1].run(mr);
            }
            let mut f = self.fx.len();
            while f > 0 {
                f -= 1;
                let y = self.fx[f].run(&mut self.r);
                ml += y;
                mr += y;
                rv[0] += y * 0.5;
                rv[1] += y * 0.5;
                if self.fx[f].done() {
                    self.fx.remove(f);
                }
            }
            s[0] = 0.0;
            s[1] = 0.0;
            self.reverb
                .run(rv[0] * self.mix.reverb, rv[1] * self.mix.reverb, &mut s);
            ml += s[0] * 0.42;
            mr += s[1] * 0.42;
            // Energy: the whole mix closes down as the energy falls.
            if self.en_hz < 18000.0 {
                self.en_lp[0].set(self.en_hz, 0.8, sr);
                self.en_lp[1].set(self.en_hz, 0.8, sr);
                ml = self.en_lp[0].run(ml);
                mr = self.en_lp[1].run(mr);
            }
            ml *= gain;
            mr *= gain;
            // Bus: glue, tape, master, limiter.
            let gr = if self.mix.glue > 0.0 {
                self.glue.run(ml, mr)
            } else {
                1.0
            };
            self.gr_sum += gr;
            self.gr_n += 1;
            ml = self.tape[0].run(ml * gr) * self.mix.master;
            mr = self.tape[1].run(mr * gr) * self.mix.master;
            self.lim.run(ml, mr, &mut o);
            l[i] = o[0] as f32;
            r[i] = o[1] as f32;
        }
    }

    /// Where the song is; `None` before a track is set.
    pub fn position(&self) -> Option<Position> {
        let seq = self.seq.as_ref()?;
        let p = seq.pos;
        Some(Position {
            sec: p.sec,
            bar: p.bar,
            step: p.step,
            bar_index: seq.bar_index(),
            bars: seq.bars,
            playing: self.playing,
        })
    }

    /// The glue's mean gain reduction since the last take (1 if nothing
    /// ran).
    pub fn take_gr(&mut self) -> f64 {
        let g = if self.gr_n != 0 {
            self.gr_sum / self.gr_n as f64
        } else {
            1.0
        };
        self.gr_sum = 0.0;
        self.gr_n = 0;
        g
    }

    /// The section starts and song ends since the last take, in order.
    pub fn take_events(&mut self) -> Vec<EngineEvent> {
        std::mem::take(&mut self.events)
    }
}

/// `Object.assign(dst, src)` over lab patches: every member `src` has
/// (its canonical JSON drops the missing ones) replaces `dst`'s.
fn merge_lab(dst: &mut Lab, src: &Lab) {
    use crate::json::Val;
    dst.kind = src.kind.clone();
    if src.name.is_some() {
        dst.name = src.name.clone();
    }
    if src.wave.is_some() {
        dst.wave = src.wave.clone();
    }
    if src.algo.is_some() {
        dst.algo = src.algo.clone();
    }
    if src.vowel.is_some() {
        dst.vowel = src.vowel.clone();
    }
    if src.osc.is_some() {
        dst.osc = src.osc.clone();
    }
    if src.ops.is_some() {
        dst.ops = src.ops.clone();
    }
    if let Val::Obj(pairs) = src.to_val() {
        for (k, v) in pairs {
            if let Val::Num(x) = v {
                dst.set(&k, x);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genres::genre;

    const SR: f64 = 48000.0;

    fn render(e: &mut Engine, secs: f64) -> Vec<f32> {
        let n = (js_round((secs * SR) / 128.0) * 128.0) as usize;
        let mut l = vec![0f32; n];
        let mut r = vec![0f32; n];
        let mut i = 0;
        while i < n {
            e.process(&mut l[i..i + 128], &mut r[i..i + 128]);
            i += 128;
        }
        l
    }

    /// The first section with an lp sweep, and the bar it starts at.
    fn lp_section(t: &Track) -> (usize, u32) {
        let mut bar = 0;
        for (i, s) in t.sections.iter().enumerate() {
            if s.lp.is_some() {
                return (i, bar);
            }
            bar += s.bars;
        }
        panic!("no lp section");
    }

    #[test]
    fn cue_sets_the_bars_section_state_mid_bar() {
        let t = (genre("house").expect("house").make)(1, None);
        let (sec, bar) = lp_section(&t);
        let lp = t.sections[sec].lp.expect("an lp");
        let mut e = Engine::new(5, SR);
        e.set_track(&t);
        e.cue(16 * bar + 8, 0.02);
        let step_dur = e.seq().expect("a sequencer").step_dur;
        // The lp fired at the bar's start, and its clock has run the eight
        // skipped steps.
        assert_eq!((e.lp_from, e.lp_to), (lp[0], lp[1]));
        assert_eq!(e.lp_t, 8 * js_round(step_dur * SR) as u64);
        assert_eq!(
            e.lp_len,
            js_round(step_dur * 16.0 * t.sections[sec].bars as f64 * SR)
        );
        assert_eq!(e.take_events(), vec![EngineEvent::Section(sec)]);
        let p = e.position().expect("a position");
        assert_eq!((p.sec, p.bar, p.step, p.bar_index), (sec, 0, 8, bar));
        assert!(p.playing);
        // Nothing sounded yet: the skipped drums and notes were not fired.
        assert!(e.drums.hits().is_empty());
        assert!(e.parts.iter().all(|(_, p)| !p.inst.active()));
        let l = render(&mut e, 1.0);
        assert!(l.iter().all(|v| v.is_finite()));
        assert!(l.iter().any(|v| v.abs() > 0.01));
    }

    #[test]
    fn cue_advances_the_automations() {
        let t = (genre("psytrance").expect("psytrance").make)(4, None);
        let mut bar = 0;
        let mut found = None;
        for (i, s) in t.sections.iter().enumerate() {
            if s.auto
                .as_ref()
                .is_some_and(|a| lookup(a, "acid.cutoff").is_some())
            {
                found = Some((i, bar));
                break;
            }
            bar += s.bars;
        }
        let (_, bar) = found.expect("an acid.cutoff auto");
        let mut e = Engine::new(5, SR);
        e.set_track(&t);
        e.cue(16 * bar + 12, 0.02);
        let step_dur = e.seq().expect("a sequencer").step_dur;
        assert!(!e.autos.is_empty());
        for a in &e.autos {
            assert_eq!(a.t, 12.0 * js_round(step_dur * SR));
        }
    }
}
