//! All game audio, synthesized with Web Audio — no sample files
//! (`src/game/Audio.js`, `GameAudio`).
//!
//! Graph:
//!   engine chains ─┐
//!   effects ───────┴─ sfxBus ─ sfxComp ─ sfxVol ─┐
//!                        └─ tunnel reverb ─┘     ├─ master ─ limiter ─ out
//!   music ─ musicMix ─ musicComp ─ gate ─ duck ─ mood lp ─ mood ─ musicVol┘
//!
//! The music itself (songs, sequencer, instruments, drum kit) lives in
//! [`crate::music`] and [`crate::tracks`]; it plays into musicIn. Crash,
//! gravel, scrape, clunk and thud sounds are pre-rendered once
//! ([`crate::samples`]).
//!
//! Music and SFX are separate buses with their own compressors, so a loud
//! engine never pumps the music down; the master only has a peak limiter.
//!
//! The engine is a wavetable: one full 720° four-stroke cycle is drawn with a
//! pressure pulse per cylinder — timed and weighted by the engine's bank
//! layout, which is where a cross-plane V8's lumpy burble comes from — and
//! turned into a band-limited PeriodicWave played at the cycle rate
//! (rpm / 120). On- and off-throttle cycles are separate waves crossfaded by
//! load, run through exhaust formant filters, and doubled on a second,
//! slightly detuned chain for stereo width. A separate rumble layer carries
//! the low end (see rumbleCycle). Continuous layers are built once and only
//! steered in update(); one-shots create short-lived nodes.
//!
//! Port notes (DECISIONS D250–D258): the JS's `setTimeout` callbacks are
//! [`crate::timers::Task`]s its driver runs ([`GameAudio::run_due_timers`],
//! [`GameAudio::poll`]); promises settle in [`GameAudio::settle`]; the
//! platform (`window.AudioContext`, `navigator.audioSession`, `fetch`,
//! `Math.random`) is a [`Platform`]. Every `Math.random` the JS draws is
//! drawn here from the one stream, in the same order.

mod build;
mod exhaust;
mod shots;
mod station;
mod steer;

use crate::dj::DjVoice;
use crate::engine::{self, CarProfile};
use crate::music::{Music, TrackInfo};
use crate::radio::{Bytes, Fetch, RadioVoice, Random};
use crate::session::{AudioSession, ask_for_playback};
use crate::timers::{Task, TimerId, Timers};
use crate::wa::{
    AudioBuffer, AudioBufferSourceNode, AudioContext, AudioParam, ContextOptions, ContextState,
    GainNode, Node, Pending, PeriodicWave,
};
use std::cell::RefCell;
use std::rc::Rc;

pub use exhaust::preset_for as exhaust_preset_for;
pub use shots::{NoiseBurst, PlayOpts};

/// `clamp(v, lo, hi)`: `v < lo ? lo : v > hi ? hi : v` (NaN passes through).
pub(crate) fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// `x || d` for a number that may be missing.
pub(crate) fn or0(x: Option<f64>, d: f64) -> f64 {
    mp_math::js::or_opt(x, d)
}

// ── Sirens ───────────────────────────────────────────────────────
/// A siren pattern: a pitch LFO on the voice, a triangle sweep between lo
/// and hi (wail, yelp) or a soft square between two tones (hi-lo). `period`
/// is a full cycle, so wail spends 1.7 s going up and 1.7 s coming down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SirenPattern {
    pub lo: f64,
    pub hi: f64,
    pub period: f64,
    /// `"tri"` or `"square"`.
    pub shape: &'static str,
}

/// `SIREN_PATTERNS`.
pub const SIREN_PATTERNS: &[(&str, SirenPattern)] = &[
    (
        "wail",
        SirenPattern {
            lo: 650.0,
            hi: 1450.0,
            period: 3.4,
            shape: "tri",
        },
    ),
    (
        "yelp",
        SirenPattern {
            lo: 650.0,
            hi: 1450.0,
            period: 0.6,
            shape: "tri",
        },
    ),
    (
        "hilo",
        SirenPattern {
            lo: 770.0,
            hi: 960.0,
            period: 1.0,
            shape: "square",
        },
    ),
];

/// `SIREN_PATTERNS[mode]`.
pub fn siren_pattern(mode: &str) -> Option<&'static SirenPattern> {
    SIREN_PATTERNS
        .iter()
        .find(|(k, _)| *k == mode)
        .map(|(_, p)| p)
}

const SOUND_SPEED: f64 = 343.0;
const SIREN_RANGE: f64 = 350.0; // m: silent beyond
const SIREN_GAIN: f64 = 0.34; // voice level right behind the player

/// Pitch factor for a source closing on the listener at relSpeed m/s
/// (negative = pulling away), listener taken as still. (`undefined` is 0.)
pub fn siren_doppler(rel_speed: Option<f64>) -> f64 {
    SOUND_SPEED / (SOUND_SPEED - clamp(rel_speed.unwrap_or(0.0), -120.0, 120.0))
}

/// Voice gain at a distance: near-inverse falloff, faded to zero at the range.
pub fn siren_level(dist: Option<f64>) -> f64 {
    let d = mp_math::js::max(0.0, dist.unwrap_or(0.0));
    let edge = clamp(1.0 - d / SIREN_RANGE, 0.0, 1.0);
    SIREN_GAIN * (20.0 / (20.0 + d)) * edge * edge.sqrt()
}

// Internal trims (calibrated with the analyser measurements in the bench).
const MASTER_TRIM: f64 = 0.64;
const SFX_TRIM: f64 = 1.25;
const MUSIC_TRIM: f64 = 2.4;
const RUMBLE: f64 = 4.0; // engine rumble layer, relative to the exhaust chains

// ── Gates ────────────────────────────────────────────────────────
// Chrome renders every node the destination pulls on, audible or not: an
// engine, siren or noise bed idling behind a zero gain costs the audio thread
// as much as a loud one, and on a phone that is enough to break up the
// music. A gate connects a voice's output only while the voice is in use,
// plus a tail for its fade (or reverb) to die away, so idle voices cost
// nothing. While a gate is shut, leave its voice's params alone: a node that
// isn't rendered never retires its automation events, so they'd pile up.
const RADIO_GAP: f64 = 0.14; // s between the clips of one radio line
const RADIO_VOICE: f64 = 1.6; // recorded voice into the radio bus's crunch: peaks level with the burble
const RADIO_WAIT: f64 = 700.0; // ms a line's clips get to load before the burble stands in
const GATE_TAIL: f64 = 1.2; // s: 8 time constants of the slowest voice fade

/// A gate (`Gate`): links `[node, destination]`; levels: the voice's output
/// gains (hushed to zero when the whole SFX chain comes back, see `_hush`).
pub(crate) struct Gate {
    links: Vec<(Node, Node)>,
    tail: f64,
    levels: Vec<AudioParam>,
    /// Built connected; the first idle check shuts it.
    open: bool,
    /// Shut once the clock passes this (Infinity: in use).
    until: f64,
}

impl Gate {
    fn shut(&mut self) {
        if self.open {
            for (n, d) in &self.links {
                let _ = n.disconnect_from(d);
            }
        }
        self.open = false;
        self.until = 0.0;
    }
}

/// A gate's index in [`GameAudio::gates`]: 0 is `_sfxGate`, the rest are
/// `_gates` in creation order.
pub(crate) type GateId = usize;
const SFX_GATE: GateId = 0;

// ── The state `update` reads ────────────────────────────────────
/// `update(dt, s)`'s `s`. Every field may be missing (`None`), as in the JS.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CarState {
    /// < 100 means the engine is off.
    pub rpm: Option<f64>,
    pub rpm_max: Option<f64>,
    pub throttle: Option<f64>,
    /// m/s (its absolute value is used).
    pub speed: Option<f64>,
    pub on_ground: Option<bool>,
    /// -1 is reverse.
    pub gear: Option<f64>,
    /// Turbo boost, 0..1.
    pub boost: Option<f64>,
    pub skid: Option<f64>,
    /// Slip angle, rad.
    pub slip: Option<f64>,
    /// 0..1, how far onto the verge.
    pub offroad: Option<f64>,
    /// 0..1.
    pub scrape: Option<f64>,
    /// -1 left / 1 right.
    pub scrape_side: Option<f64>,
    pub nitro: Option<bool>,
    /// Electric only: 0..1.05; its presence also means "running".
    pub motor: Option<f64>,
    /// Electric only: kW (/600 = load).
    pub power: Option<f64>,
    pub regen: Option<f64>,
}

/// A rival engine for `setRivalEngines`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rival {
    pub dist: Option<f64>,
    pub pan: Option<f64>,
    pub rpm_norm: Option<f64>,
    pub electric: Option<bool>,
}

/// A police unit for `setSirens`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SirenUnit {
    pub id: Option<f64>,
    pub dist: Option<f64>,
    pub pan: Option<f64>,
    /// m/s, + = closing.
    pub rel_speed: Option<f64>,
    /// `'wail' | 'yelp' | 'hilo' | 'off'` (anything else: no pattern).
    pub mode: Option<String>,
}

/// `setVolume({ master, sfx, music })`: 0..1 each, missing ones unchanged.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Volume {
    pub master: Option<f64>,
    pub sfx: Option<f64>,
    pub music: Option<f64>,
}

/// `init(opts)`.
#[derive(Clone, Default)]
pub struct InitOptions {
    /// Run on a given context (tests render an offline context).
    pub context: Option<AudioContext>,
    /// The new context's buffering. 'balanced' by default: a phone's
    /// smallest buffer ('interactive') leaves the audio thread no slack, and
    /// a late buffer is an audible stutter. The music player, where latency
    /// doesn't matter, asks for 'playback'.
    pub latency_hint: Option<String>,
}

/// Makes a live context (`new AudioContext(options)`), or `None`.
pub type ContextMaker = Box<dyn Fn(&ContextOptions) -> Option<AudioContext>>;

/// `onTrackChange`: called with the track that starts playing.
pub type TrackCallback = Box<dyn Fn(&TrackInfo)>;

/// What the browser (or the native shell, or a test) provides.
pub struct Platform {
    /// `window.AudioContext || window.webkitAudioContext`: a live context
    /// (`None`: no Web Audio).
    pub new_context: Option<ContextMaker>,
    /// `navigator.audioSession`.
    pub audio_session: Option<Box<dyn AudioSession>>,
    /// Fetches `audio/radio/` files.
    pub radio: Rc<dyn Fetch>,
    /// Fetches `audio/dj/` files (the radio DJs' clips; `None`: no DJ).
    pub dj: Option<Rc<dyn Fetch>>,
    /// `Math.random`.
    pub random: Random,
}

/// A fetcher that has nothing (every fetch fails).
pub struct NoFetch;

impl Fetch for NoFetch {
    fn fetch(&self, _file: &str) -> Pending<Bytes> {
        let p = Pending::new();
        p.resolve(Ok(None));
        p
    }
}

impl Platform {
    /// No Web Audio, no radio clips; `Math.random` is `mulberry32(seed)`.
    pub fn headless(seed: u32) -> Platform {
        Platform {
            new_context: None,
            audio_session: None,
            radio: Rc::new(NoFetch),
            dj: None,
            random: Rc::new(RefCell::new(mp_math::Mulberry32::new(seed))),
        }
    }
}

/// The DJ's duck on the station while a clip plays, and how long its
/// gain takes to settle either way (a time constant).
const DJ_DUCK: f64 = 0.4;
const DJ_DUCK_TC: f64 = 0.05;
const DJ_BACK_TC: f64 = 0.15;

/// The current radio transmission (`_radioCur`).
struct RadioCur {
    g: GainNode,
    end: f64,
    srcs: Vec<shots::Src>,
}

/// A radio line waiting for its clips or its 700 ms (`radioLine`'s race).
struct PendingLine {
    seq: u32,
    voice: Pending<Option<Vec<AudioBuffer>>>,
    pan: f64,
    duration: f64,
}

/// `GameAudio`.
pub struct GameAudio {
    platform: Platform,
    ctx: Option<AudioContext>,
    init_done: bool,
    built: bool,
    vol: (f64, f64, f64),
    car: &'static str,
    env: &'static str,
    prev_throttle: f64,
    pop_cooldown: f64,
    music_on: bool,
    music_wanted: Option<bool>,
    paused: bool,
    limiting: bool,
    /// (info) => void when a music track starts.
    on_track_change: Rc<RefCell<Option<TrackCallback>>>,
    /// Requested track id (None = keep / level default).
    track: Option<String>,
    impact_t: f64,
    impact_s: Option<f64>,
    /// A live AudioContext (not a test's offline one).
    realtime: bool,
    /// It has been running at least once.
    ran: bool,
    /// `_sfxGate` (0) and `_gates`.
    gates: Vec<Gate>,
    gate_timer: Option<TimerId>,
    pub radio_voice: RadioVoice,
    radio_seq: u32,
    radio_lines: Vec<PendingLine>,
    radio_cur: Option<RadioCur>,
    prof: Option<&'static CarProfile>,
    electric: bool,
    rival_wave: Option<PeriodicWave>,
    waves: Vec<(&'static str, CarWaveSet)>,
    pop_curve: Option<Vec<f32>>,
    boost: f64,
    damage: f64,
    misfire_t: f64,
    mood: &'static str,
    mood_to: f64,
    rival_ev: [bool; 3],
    sirens: Vec<SirenState>,
    timers: Timers,
    g: Option<Rc<build::Graph>>,
    pub music: Option<Music>,
    /// The player's engine on the physical model ([`exhaust`]), if asked
    /// for and the platform runs it.
    ex_want: bool,
    ex: exhaust::ExState,
    /// The model voiced the engine at the last update.
    ex_live: bool,
    /// Exhaust nodes made (one per graph, whatever the car changes).
    ex_made: u32,
    /// The car radio ([`station`]): the node's state, the station asked for
    /// (an index in `mp_music::radio::STATIONS`) and the wall time it was
    /// asked at, whether that request still waits on the node, the `tune`
    /// serial, the energy, and nodes made (one per graph).
    station: station::StState,
    station_want: Option<usize>,
    station_wall: f64,
    station_pending: bool,
    station_tune: f64,
    station_energy: f64,
    station_made: u32,
    /// The DJs' clips ([`crate::dj`]), the clips decoding for `dj_say`,
    /// the sources talking (cut on a station change), and when the last
    /// of them ends.
    dj_voice: Option<DjVoice>,
    dj_clips: Vec<Pending<Option<AudioBuffer>>>,
    dj_srcs: Vec<AudioBufferSourceNode>,
    dj_end: f64,
}

struct CarWaveSet {
    on_l: PeriodicWave,
    off_l: PeriodicWave,
    on_r: PeriodicWave,
    off_r: PeriodicWave,
    rumble: PeriodicWave,
}

#[derive(Clone, Debug)]
struct SirenState {
    id: Option<f64>,
    mode: String,
    drift: f64,
}

impl GameAudio {
    pub fn new(platform: Platform) -> GameAudio {
        let radio_voice = RadioVoice::new(platform.radio.clone());
        let dj_voice = platform
            .dj
            .clone()
            .map(|f| DjVoice::new(f, platform.random.clone()));
        GameAudio {
            platform,
            ctx: None,
            init_done: false,
            built: false,
            vol: (1.0, 0.85, 0.7),
            car: "sports",
            env: "open",
            prev_throttle: 0.0,
            pop_cooldown: 0.0,
            music_on: false,
            music_wanted: None,
            paused: false,
            limiting: false,
            on_track_change: Rc::new(RefCell::new(None)),
            track: None,
            impact_t: -1.0,
            impact_s: None,
            realtime: false,
            ran: false,
            gates: Vec::new(),
            gate_timer: None,
            radio_voice,
            radio_seq: 0,
            radio_lines: Vec::new(),
            radio_cur: None,
            prof: None,
            electric: false,
            rival_wave: None,
            waves: Vec::new(),
            pop_curve: None,
            boost: 0.0,
            damage: 0.0,
            misfire_t: 0.0,
            mood: "off",
            mood_to: 20000.0,
            rival_ev: [false; 3],
            sirens: Vec::new(),
            timers: Timers::default(),
            g: None,
            music: None,
            ex_want: false,
            ex: exhaust::ExState::Idle,
            ex_live: false,
            ex_made: 0,
            station: station::StState::Idle,
            station_want: None,
            station_wall: 0.0,
            station_pending: false,
            station_tune: 0.0,
            station_energy: 1.0,
            station_made: 0,
            dj_voice,
            dj_clips: Vec::new(),
            dj_srcs: Vec::new(),
            dj_end: 0.0,
        }
    }

    /// The DJs' clips, for prefetching (`None`: the platform has none).
    pub fn dj(&self) -> Option<&DjVoice> {
        self.dj_voice.as_ref()
    }

    /// `_radioCur.srcs`: the sources of the radio transmission on the air
    /// (or the last one), its hiss, its clips or its burble, in order. The
    /// test bridge reads them (`__audio._radioCur`).
    pub fn radio_cur_srcs(&self) -> Option<Vec<crate::wa::Node>> {
        let cur = self.radio_cur.as_ref()?;
        Some(
            cur.srcs
                .iter()
                .map(|s| match s {
                    shots::Src::Buf(b) => (**b).clone(),
                    shots::Src::Osc(o) => (**o).clone(),
                })
                .collect(),
        )
    }

    /// The context, once `init` made or took one.
    pub fn ctx(&self) -> Option<&AudioContext> {
        self.ctx.as_ref()
    }

    pub fn ready(&self) -> bool {
        self.ctx.is_some() && self.built
    }

    pub fn car(&self) -> &'static str {
        self.car
    }

    pub fn environment(&self) -> &'static str {
        self.env
    }

    /// `_vol`: (master, sfx, music).
    pub fn volumes(&self) -> (f64, f64, f64) {
        self.vol
    }

    pub fn music_on(&self) -> bool {
        self.music_on
    }

    /// `musicGate.gain.value` (the music's fade in and out), 0 before the
    /// graph is built: what the e2e tests read (`__mp.audio.musicGate`).
    pub fn music_gate(&self) -> f64 {
        self.g.as_ref().map_or(0.0, |g| g.music_gate.gain.value())
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    /// `onTrackChange`.
    pub fn set_on_track_change(&self, f: Option<TrackCallback>) {
        *self.on_track_change.borrow_mut() = f;
    }

    pub(crate) fn graph(&self) -> Rc<build::Graph> {
        self.g.clone().expect("the graph is built")
    }

    pub(crate) fn now(&self) -> f64 {
        self.ctx.as_ref().map_or(0.0, |c| c.current_time())
    }

    pub(crate) fn random(&self) -> f64 {
        self.platform.random.borrow_mut().next_f64()
    }

    /// Memoised: a second call does nothing. Never waits for the context to
    /// start. One made outside a user gesture (on a phone, a tap's
    /// pointerdown doesn't count) starts suspended, and a resume() made
    /// there doesn't settle until a later one succeeds. The graph builds
    /// fine on a suspended context; unlock() starts it from a gesture.
    pub fn init(&mut self, opts: InitOptions) {
        if self.init_done {
            return;
        }
        self.init_done = true;
        let has_ac = self.platform.new_context.is_some();
        if !has_ac && opts.context.is_none() {
            return;
        }
        if opts.context.is_none() {
            ask_for_playback(self.platform.audio_session.as_deref());
        }
        let ctx = match &opts.context {
            Some(c) => c.clone(),
            None => {
                let make = self.platform.new_context.as_ref().expect("a context maker");
                let co = ContextOptions {
                    latency_hint: Some(
                        opts.latency_hint
                            .clone()
                            .unwrap_or_else(|| "balanced".into()),
                    ),
                };
                match make(&co) {
                    Some(c) => c,
                    None => return,
                }
            }
        };
        self.ctx = Some(ctx);
        self.realtime = opts.context.is_none();
        self.build();
        self.built = true;
        self.set_car(self.car);
        self.set_environment(self.env, true);
        if let Some(t) = self.track.clone() {
            let m = self.music.as_mut().expect("music");
            m.play(&t, 0, true, &mut self.timers);
        }
        if self.music_wanted == Some(true) {
            self.set_music(true);
        }
        // A station asked for before init: tuned now, as a fresh request.
        if self.station_pending {
            let (want, wall) = (self.station_want.take(), self.station_wall);
            self.set_station(want, wall);
        }
        if opts.context.is_none() {
            self.unlock();
        }
    }

    /// Start the context if it isn't running (it can also be interrupted
    /// later, e.g. by a phone call on iOS). Call it from inside a user
    /// gesture: a tap's pointerup, touchend or click, or a key. While paused
    /// it stays suspended.
    pub fn unlock(&mut self) {
        let Some(ctx) = &self.ctx else {
            return;
        };
        let st = ctx.state();
        if self.paused || st == ContextState::Running || st == ContextState::Closed {
            return;
        }
        let _ = ctx.resume();
    }

    /// Independent 0..1 volumes. sfx covers the engine and every effect.
    pub fn set_volume(&mut self, v: Volume) {
        if let Some(m) = v.master {
            self.vol.0 = clamp(m, 0.0, 1.0);
        }
        if let Some(s) = v.sfx {
            self.vol.1 = clamp(s, 0.0, 1.0);
        }
        if let Some(m) = v.music {
            self.vol.2 = clamp(m, 0.0, 1.0);
        }
        if self.ready() {
            self.apply_volumes(0.03);
        }
    }

    fn apply_volumes(&mut self, tc: f64) {
        let t = self.now();
        let g = self.graph();
        g.master
            .gain
            .set_target_at_time(MASTER_TRIM * self.vol.0, t, tc);
        // SFX at zero (the music player): the whole effects graph stops rendering.
        let was_open = self.gates[SFX_GATE].open;
        if self.gate_set(SFX_GATE, self.vol.1 > 0.001, t) && !was_open {
            self.hush(t);
        }
        g.sfx_vol
            .gain
            .set_target_at_time(SFX_TRIM * self.vol.1, t, tc);
        g.music_vol
            .gain
            .set_target_at_time(MUSIC_TRIM * self.vol.2, t, tc);
    }

    // ── Gates (see Gate) ─────────────────────────────────────────────
    pub(crate) fn add_gate(
        &mut self,
        links: Vec<(Node, Node)>,
        tail: f64,
        levels: Vec<AudioParam>,
    ) -> GateId {
        self.gates.push(Gate {
            links,
            tail,
            levels,
            open: true,
            until: 0.0,
        });
        self.gates.len() - 1
    }

    /// `gate.set(active, t)`: report whether the voice is in use this
    /// frame; returns whether it is connected (worth steering).
    pub(crate) fn gate_set(&mut self, id: GateId, active: bool, t: f64) -> bool {
        let g = &mut self.gates[id];
        if active {
            g.until = f64::INFINITY;
            if !g.open {
                for (n, d) in &g.links {
                    let _ = n.connect(d);
                }
                g.open = true;
            }
        } else if g.open {
            if g.until == f64::INFINITY {
                g.until = t + g.tail;
                let tail = g.tail;
                self.gate_later(tail);
            } else if t >= g.until {
                g.shut();
            }
        }
        self.gates[id].open
    }

    pub(crate) fn gate_open(&self, id: GateId) -> bool {
        self.gates[id].open
    }

    // A live context shuts gates whose tails ran out on a timer too, so voices
    // left fading when nothing calls update() (the menu) still stop rendering.
    // Offline renders call gate_tick() themselves if they want it.
    fn gate_later(&mut self, s: f64) {
        if !self.realtime || self.gate_timer.is_some() {
            return;
        }
        let now = self.now();
        self.gate_timer = Some(self.timers.set_timeout(
            now,
            (mp_math::js::max(0.25, s) + 0.05) * 1000.0,
            Task::GateTick,
        ));
    }

    pub fn gate_tick(&mut self) {
        if !self.ready() {
            return;
        }
        let t = self.now();
        let mut wait: f64 = 0.0;
        for g in self.gates.iter_mut() {
            if !g.open || g.until == f64::INFINITY {
                continue;
            }
            if t >= g.until {
                g.shut();
            } else {
                wait = mp_math::js::max(wait, g.until - t);
            }
        }
        if wait != 0.0 {
            self.gate_later(wait);
        }
    }

    // The SFX chain is back after being shut: voices inside it kept whatever
    // level they had (their updates were skipped), so silence them all and
    // let the next update() bring back the ones in use.
    fn hush(&mut self, t: f64) {
        for g in self.gates.iter_mut().skip(1) {
            for p in &g.levels {
                p.cancel_scheduled_values(t);
                p.set_value_at_time(0.0, t);
            }
            if !g.levels.is_empty() {
                g.shut();
            }
        }
        self.radio_cut(t);
    }

    // Per-frame steering needs a context that has run: until the first unlock
    // nothing renders, so every automation event would pile up (headless
    // Chrome; a phone that never got its tap). Offline contexts always steer.
    pub(crate) fn running(&mut self) -> bool {
        if !self.ready() {
            return false;
        }
        if !self.ran {
            self.ran = !self.realtime
                || self.ctx.as_ref().expect("a context").state() == ContextState::Running;
        }
        self.ran
    }

    /// Continuous SFX voices: steered only while the SFX chain renders.
    pub(crate) fn steer(&mut self) -> bool {
        self.running() && self.gates[SFX_GATE].open
    }

    /// One-shots: nothing to play into while the SFX chain is shut (muted).
    pub(crate) fn sfx_on(&self) -> bool {
        self.ready() && self.gates[SFX_GATE].open
    }

    // ── Timers and promises (the JS's event loop) ─────────────────────
    /// When the next timer is due.
    pub fn next_timer(&self) -> Option<f64> {
        self.timers.next_due()
    }

    /// Run every timer due at or before `t`, in order, each followed by
    /// [`GameAudio::settle`]. `set_clock(due)` runs before each: a driver on
    /// a virtual clock moves it there (`VirtualTimers.advanceTo`).
    pub fn run_due_timers(&mut self, t: f64, mut set_clock: impl FnMut(f64)) {
        while let Some((due, task)) = self.timers.pop_due(t) {
            set_clock(due);
            self.run_task(task);
            self.settle();
        }
    }

    /// A live client's frame: the timers due by the audio clock, then the
    /// promises.
    pub fn poll(&mut self) {
        let now = self.now();
        self.run_due_timers(now, |_| {});
        self.settle();
        self.station_poll();
    }

    fn run_task(&mut self, task: Task) {
        match task {
            Task::GateTick => {
                self.gate_timer = None;
                self.gate_tick();
            }
            Task::MusicTick => {
                if let Some(m) = self.music.as_mut() {
                    m.tick(&mut self.timers);
                }
            }
            Task::MusicOnTrack { serial, info } => {
                if let Some(m) = &self.music {
                    m.fire_on_track(serial, &info);
                }
            }
            Task::MusicKill(k) => Music::kill(k),
            Task::RadioWait(seq) => self.radio_wait(seq),
        }
    }

    /// Let every pending promise settle: the context's (resume, suspend,
    /// decoding), then the radio lines and DJ clips waiting on them.
    pub fn settle(&mut self) {
        loop {
            if let Some(ctx) = &self.ctx {
                ctx.settle();
            }
            let radio = self.poll_radio();
            let dj = self.poll_dj();
            if !radio && !dj {
                break;
            }
        }
    }

    // ── The radio DJ ──────────────────────────────────────────────────
    /// The DJ says clip `id` (take `take`, 1-based; 0 for a random one):
    /// decoded and played once into the music bus, ducking the station
    /// while it talks. A clip that is missing or fails to load plays
    /// nothing, silently, as the police radio copes.
    pub fn dj_say(&mut self, id: &str, take: u32) {
        if !self.ready() {
            return;
        }
        let Some(dj) = &self.dj_voice else {
            return;
        };
        let ctx = self.ctx.clone().expect("a context");
        self.dj_clips.push(dj.buffer(&ctx, id, take));
    }

    /// The DJ clips whose buffers are in (or known to be missing) play.
    /// Returns whether one settled.
    fn poll_dj(&mut self) -> bool {
        let Some(i) = self.dj_clips.iter().position(Pending::is_settled) else {
            return false;
        };
        let p = self.dj_clips.remove(i);
        if let Some(Ok(Some(buf))) = p.result() {
            self.dj_play(&buf);
        }
        true
    }

    /// One DJ clip, after the one still talking if there is one; the
    /// station comes down to `DJ_DUCK` under it and back after.
    fn dj_play(&mut self, buf: &AudioBuffer) {
        let ctx = self.ctx.clone().expect("a context");
        let g = self.graph();
        let now = self.now();
        let t0 = if self.dj_end > now {
            self.dj_end + 0.1
        } else {
            // Everything before has finished talking.
            self.dj_srcs.clear();
            now + 0.01
        };
        let end = t0 + buf.duration();
        let vg = ctx.create_gain();
        let src = ctx.create_buffer_source();
        let _ = src.set_buffer(Some(buf));
        let _ = src.connect(&vg);
        let _ = vg.connect(&g.music_in);
        let _ = src.start_at(t0);
        if let Some(out) = self.station_out() {
            out.gain.cancel_scheduled_values(t0);
            out.gain.set_target_at_time(DJ_DUCK, t0, DJ_DUCK_TC);
            out.gain.set_target_at_time(1.0, end, DJ_BACK_TC);
        }
        self.dj_srcs.push(src);
        self.dj_end = end;
    }

    /// Cuts the DJ off: the clips talking or queued stop at once, the ones
    /// still decoding are dropped, and the station comes back up. A
    /// station change (or off): a DJ does not follow the listener.
    pub(super) fn dj_cut(&mut self) {
        self.dj_clips.clear();
        if self.dj_srcs.is_empty() {
            return;
        }
        for src in self.dj_srcs.drain(..) {
            let _ = src.stop();
        }
        let now = self.now();
        if let Some(out) = self.station_out().filter(|_| self.dj_end > now) {
            out.gain.cancel_scheduled_values(now);
            out.gain.set_target_at_time(1.0, now, DJ_DUCK_TC);
        }
        self.dj_end = 0.0;
    }

    // ── Car and environment ──────────────────────────────────────────
    /// Engine character: 'sports' (flat-plane V8), 'muscle' (cross-plane V8),
    /// 'super' (V10), 'rally' (turbo four), 'electric' (motor, no engine).
    /// Can be switched live.
    pub fn set_car(&mut self, kind: &str) {
        let prof = engine::car(kind).unwrap_or(&engine::SPORTS);
        self.car = prof.key;
        if !self.ready() {
            return;
        }
        self.pursuit_reset();
        self.prof = Some(prof);
        self.electric = prof.electric;
        let ctx = self.ctx.clone().expect("a context");
        let g = self.graph();
        // Rivals are all combustion cars, whatever the player drives.
        if self.rival_wave.is_none() {
            let w = engine::rival_wave();
            self.rival_wave = ctx.create_periodic_wave(&w.real, &w.imag, None).ok();
        }
        let rw = self.rival_wave.clone().expect("the rival wave");
        for (i, r) in g.rivals.iter().enumerate() {
            if !self.rival_ev[i] {
                r.osc.set_periodic_wave(&rw);
            }
        }
        if self.electric {
            return;
        }
        let t = ctx.current_time();
        let key = prof.key;
        if !self.waves.iter().any(|(k, _)| *k == key) {
            let w = engine::car_waves(prof).expect("a combustion car");
            let mk = |f: &engine::Fourier| {
                ctx.create_periodic_wave(&f.real, &f.imag, None)
                    .expect("an engine wave")
            };
            let set = CarWaveSet {
                on_l: mk(&w.on_l),
                off_l: mk(&w.off_l),
                on_r: mk(&w.on_r),
                off_r: mk(&w.off_r),
                rumble: mk(&w.rumble),
            };
            self.waves.push((key, set));
        }
        let w = &self.waves.iter().find(|(k, _)| *k == key).expect("waves").1;
        let e = &g.eng;
        e.l.on.set_periodic_wave(&w.on_l);
        e.l.off.set_periodic_wave(&w.off_l);
        e.r.on.set_periodic_wave(&w.on_r);
        e.r.off.set_periodic_wave(&w.off_r);
        e.rum.set_periodic_wave(&w.rumble);
        for (side, spread) in [(&e.l, 1.0), (&e.r, 1.04)] {
            for (i, f) in side.f.iter().enumerate() {
                let [freq, gain, q] = prof.formants[i];
                f.frequency.set_target_at_time(freq * spread, t, 0.05);
                f.gain.set_target_at_time(gain, t, 0.05);
                f.q.set_target_at_time(q, t, 0.05);
            }
        }
    }

    /// 'tunnel' blends in a short, dense concrete-box reverb on all SFX.
    pub fn set_environment(&mut self, env: &str, immediate: bool) {
        self.env = if env == "tunnel" { "tunnel" } else { "open" };
        if !self.ready() {
            return;
        }
        let t = self.now();
        let tc = if immediate { 0.001 } else { 0.25 };
        let on = self.env == "tunnel";
        let g = self.graph();
        self.gate_set(g.vg.tunnel, on, t);
        g.env_send
            .gain
            .set_target_at_time(if on { 0.75 } else { 0.0 }, t, tc);
        g.env_low
            .gain
            .set_target_at_time(if on { 3.0 } else { 0.0 }, t, tc);
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        let Some(ctx) = &self.ctx else {
            return;
        };
        if paused {
            let _ = ctx.suspend();
        } else {
            let _ = ctx.resume();
            if let Some(m) = self.music.as_mut() {
                m.on_resume();
            }
        }
    }

    // ── Music ─────────────────────────────────────────────────────────
    /// The songs and the playlist: `[{ id, title, style, bpm }]`.
    pub fn tracks() -> Vec<TrackInfo> {
        Music::tracks()
    }

    /// The track that suits a level (its default when nothing else is picked).
    pub fn level_track(level_id: &str) -> &'static str {
        Music::level_track(level_id)
    }

    pub fn set_music(&mut self, on: bool) {
        if !self.ready() {
            self.music_wanted = Some(on);
            return;
        }
        let t = self.now();
        let g = self.graph();
        if on && !self.music_on {
            self.music_on = true;
            g.music_gate.gain.cancel_scheduled_values(t);
            g.music_gate.gain.set_target_at_time(1.0, t, 0.3);
            // With a station tuned the gate opens for it; the playlist
            // stays stopped until the radio is turned off.
            if self.station().is_none() {
                let m = self.music.as_mut().expect("music");
                m.start(&mut self.timers);
            }
        } else if !on && self.music_on {
            self.music_on = false;
            g.music_gate.gain.cancel_scheduled_values(t);
            g.music_gate.gain.set_target_at_time(0.0, t, 0.25);
            let m = self.music.as_mut().expect("music");
            m.stop(&mut self.timers);
        }
    }

    /// Play a track by id (a quick fade if another is playing). If it is
    /// already the one playing, it just carries on.
    pub fn play_track(&mut self, id: &str) {
        self.track = Some(id.to_owned());
        if self.ready() {
            let m = self.music.as_mut().expect("music");
            m.play(id, 0, true, &mut self.timers);
        }
    }

    /// Skip to the next track in the playlist; returns its info.
    pub fn next_track(&mut self) -> Option<TrackInfo> {
        if !self.ready() {
            return None;
        }
        let m = self.music.as_mut().expect("music");
        let info = m.next(&mut self.timers);
        if let Some(i) = &info {
            self.track = Some(i.id.to_owned());
        }
        info
    }

    /// `{ id, title, style, bpm }` of the playing (or queued) track.
    pub fn track_info(&self) -> Option<TrackInfo> {
        if self.ready() {
            self.music.as_ref().and_then(Music::info)
        } else {
            None
        }
    }

    /// The music's `pumpUntil` (offline renders drive it).
    pub fn music_pump_until(&mut self, until: f64) {
        if let Some(m) = self.music.as_mut() {
            m.pump_until(until, &mut self.timers);
        }
    }

    /// The music's `play(id)` (offline renders).
    pub fn music_play(&mut self, id: &str) {
        if let Some(m) = self.music.as_mut() {
            m.play(id, 0, true, &mut self.timers);
        }
    }
}

#[cfg(test)]
mod tests {
    //! `test/unit/pursuit-audio.test.js`.
    use super::*;

    #[test]
    fn siren_patterns() {
        let wail = siren_pattern("wail").unwrap();
        let yelp = siren_pattern("yelp").unwrap();
        let hilo = siren_pattern("hilo").unwrap();
        for p in [wail, yelp] {
            assert_eq!((p.lo, p.hi, p.shape), (650.0, 1450.0, "tri"));
        }
        assert_eq!(wail.period / 2.0, 1.7, "wail takes 1.7 s each way");
        assert!(
            yelp.period < wail.period / 4.0,
            "yelp is much faster than wail"
        );
        assert_eq!(
            (hilo.lo, hilo.hi, hilo.shape, hilo.period / 2.0),
            (770.0, 960.0, "square", 0.5)
        );
    }

    #[test]
    fn doppler() {
        assert_eq!(siren_doppler(Some(0.0)), 1.0);
        assert!((siren_doppler(Some(30.0)) - 343.0 / 313.0).abs() < 1e-9);
        assert!((siren_doppler(Some(-30.0)) - 343.0 / 373.0).abs() < 1e-9);
        assert!(siren_doppler(Some(1e6)) < 2.0 && siren_doppler(Some(-1e6)) > 0.5);
        assert_eq!(siren_doppler(None), 1.0);
    }

    #[test]
    fn siren_level_falls_to_silence() {
        let d = [0.0, 5.0, 20.0, 50.0, 100.0, 200.0, 300.0, 349.0];
        for i in 1..d.len() {
            assert!(siren_level(Some(d[i])) < siren_level(Some(d[i - 1])));
        }
        assert!(siren_level(Some(0.0)) > 0.2 && siren_level(Some(0.0)) < 0.5);
        assert!(siren_level(Some(100.0)) > 0.02);
        assert_eq!(siren_level(Some(350.0)), 0.0);
        assert_eq!(siren_level(Some(1000.0)), 0.0);
    }

    #[test]
    fn pursuit_methods_are_no_ops_before_init() {
        let mut a = GameAudio::new(Platform::headless(1));
        assert!(!a.ready());
        a.set_sirens(&[SirenUnit {
            dist: Some(5.0),
            pan: Some(0.0),
            rel_speed: Some(10.0),
            mode: Some("wail".into()),
            ..Default::default()
        }]);
        a.set_sirens(&[]);
        a.siren_horn(0.5);
        a.radio(2.0, -0.5, None);
        a.radio_line(&["Suspect in custody.".into()], 0.0);
        a.busted();
        a.escaped();
        a.takedown(1.0, 0.0);
        a.spike_pop(0.0);
        a.wrecked();
        a.set_spiked_tyres(true, 30.0);
        a.set_pursuit_mood("cooldown");
        a.set_damage(0.9);
        assert!(a.ctx().is_none());
        // init without a context maker does nothing either.
        a.init(InitOptions::default());
        assert!(a.ctx().is_none());
    }
}
