//! The race's sound (roadmap M5; DECISIONS D513–D517): `mr_audio`'s
//! `GameAudio` driven as `main.js` and `Race.update` drive it, from the
//! session's ticks, their `SimEvent`s and the state they leave.
//!
//! - **Per tick** ([`TickAudio`], made by the session's observer in
//!   [`super::flow`]): the countdown beeps and GO, the player's impacts
//!   with other cars and walls, shifts, landings, the near-miss and
//!   passing whooshes, the finish fanfare, then `update` with the car's
//!   state, the nitro burst, the three nearest rivals' engines and the
//!   tunnel's echo, in `Race.update`'s order. The pans that the JS takes
//!   from the camera's matrix come from a camera rig stepped per tick, read
//!   as three's `matrixWorld` lags its `lookAt`s (D515).
//! - **Per race** (`startRace`): `setPaused(false)`, the wake-up
//!   (`init`, `unlock`), the level's music (`pickMusic`), the volumes
//!   (`applyVolume`) and the car; pause and resume; the music key (M) and
//!   the next track (T).
//! - **The gesture rule** (SPEC 7.4): on the web the context is made
//!   behind the loading screen, suspended, so the graph's build costs no
//!   frame of the race; the page's pointer-down, pointer-up, touch-end,
//!   click and key-down handlers call [`gesture`] (through `web::gesture`),
//!   which runs `init(); unlock()` inside the handler, as the JS's
//!   `wakeAudio`. Nothing plays until the first the browser counts as a
//!   gesture (a tap's lift, a click, a key; not a touch's start). Natively the
//!   context runs from the start (the native backend, an output device
//!   through cpal; no device, no sound).
//!
//! Settings are the JS game's store keys where it has them: `mr.musicVol`,
//! `mr.sfxVol` and `mr.track` in `localStorage` on the web (the menus that
//! set them are M6's); natively the JS defaults.
//!
//! `?audiolog=1` records every call the client makes on the audio as the
//! JS reference's facade recorder writes it (`[tick, "method", ...args]`,
//! `tools/parity/lib/audio-facade.mjs`), on `window.__mr.audioLog`;
//! `tools/parity/rust-audio-race.mjs` compares a scripted race's with the
//! JS drive's.

use super::Play;
use super::camera::{CameraRig, Car, View};
use super::flow::Mode;
use crate::Opts;
use crate::loader::AppState;
use bevy::input::ButtonState;
use bevy::input::keyboard::{KeyCode, KeyboardInput};
use bevy::prelude::*;
use mr_audio::game::{CarState, GameAudio, Platform, Rival, Volume};
use mr_math::{clamp, kernel};
use mr_sim::input::InputFrame;
use mr_sim::physics::{MOTOR_MAX, PhysEvent};
use mr_sim::race::{DT, LevelRuntime, RaceStateKind, SimEvent, SimState};
use mr_track::ROAD_TYPES;
use mr_track::Track;
use std::cell::RefCell;
use std::rc::Rc;

/// Where a one-shot sits left to right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pan {
    Fixed(f64),
    /// A contact at this offset from the player's car (x, z), panned by the
    /// camera's right vector: `((h.x - p.x) * cr[0] + (h.z - p.z) * cr[2]) / 2`.
    At(f64, f64),
}

/// A one-shot of a tick, in `Race.update`'s order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shot {
    Beep(bool),
    Impact(f64, Pan),
    Shift(bool),
    Landing(f64),
    Whoosh(f64, f64),
    FinishFanfare,
}

/// A rival relative to the player: dx, dz, its speed, electric or not.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RivalAt {
    pub dx: f64,
    pub dz: f64,
    pub speed: f64,
    pub electric: bool,
}

/// What the audio hears of one tick: everything of `Race.update`'s audio
/// calls but the camera.
#[derive(Clone, Debug, PartialEq)]
pub struct TickAudio {
    /// The tick (`SimState::tick` after it).
    pub tick: u32,
    pub shots: Vec<Shot>,
    /// `audio.update`'s state.
    pub state: CarState,
    pub nitro: bool,
    pub rivals: Vec<RivalAt>,
    pub in_tunnel: bool,
    /// The player's car, for the per-tick camera (D515).
    pub car: Car,
    /// Still counting down after this tick (the intro swing runs).
    pub countdown: bool,
    /// The camera's shake from this tick's contacts (`cam.bump`).
    pub bumps: Vec<f64>,
}

impl TickAudio {
    /// The tick just stepped: its state, events and input.
    pub fn of(lr: &LevelRuntime, st: &SimState, events: &[SimEvent], f: &InputFrame) -> TickAudio {
        let t = &*lr.track;
        let p = &st.players[0];
        let (v, ph) = (&p.v, &p.phys);
        let inp = f.input();
        let mut ctrl_throttle = inp.throttle;
        let mut shots = Vec::new();
        let mut bumps = Vec::new();
        for e in events {
            match e {
                SimEvent::CarHit {
                    hit,
                    player: Some(_),
                } => bumps.push(hit.strength * 1.2),
                SimEvent::Phys { player: 0, e } => match *e {
                    PhysEvent::Impact { strength, .. } => bumps.push(strength),
                    PhysEvent::Land { strength, .. } => bumps.push(strength * 0.8),
                    _ => {}
                },
                _ => {}
            }
            match e {
                SimEvent::Countdown(_) => shots.push(Shot::Beep(false)),
                SimEvent::Go => shots.push(Shot::Beep(true)),
                SimEvent::CarHit {
                    hit,
                    player: Some(_),
                } => shots.push(Shot::Impact(
                    hit.strength,
                    Pan::At(hit.x - v.x, hit.z - v.z),
                )),
                SimEvent::Phys { player: 0, e } => match *e {
                    PhysEvent::Impact { strength, side, .. } => {
                        shots.push(Shot::Impact(strength, Pan::Fixed(f64::from(side) * 0.6)))
                    }
                    PhysEvent::Shift { up } => shots.push(Shot::Shift(up)),
                    PhysEvent::Land { strength, .. } => shots.push(Shot::Landing(strength)),
                    PhysEvent::Touchdown { .. } => {}
                },
                SimEvent::NearMiss { lat, rel, .. } => shots.push(Shot::Whoosh(
                    clamp(lat / 3.0, -1.0, 1.0),
                    clamp(rel / 50.0, 0.3, 1.0),
                )),
                SimEvent::Whoosh { lat, .. } => {
                    shots.push(Shot::Whoosh(clamp(lat / 3.0, -1.0, 1.0), 0.4))
                }
                SimEvent::Finished { .. } => shots.push(Shot::FinishFanfare),
                SimEvent::Controls { throttle, .. } => ctrl_throttle = *throttle,
                _ => {}
            }
        }
        let psp = kernel::hypot(v.vx, v.vz);
        // Tyres past the edge of the tarmac: gravel instead of squeal.
        // Freeway shoulders, boulevards and kerbed streets are paved to the
        // wall.
        let fr = t.frame(v.s);
        let edge = ROAD_TYPES
            .get(usize::from(t.road_type[t.idx(v.s)]))
            .map(|r| r.edge);
        let paved = matches!(edge, Some("jersey" | "rail" | "curb"));
        let mut offroad = if paved {
            0.0
        } else {
            clamp((v.lat.abs() + v.half_w * 0.6 - fr.hw) / 1.2, 0.0, 1.0)
        };
        if offroad > 0.0
            && let Some(loose) = &t.loose_at
        {
            offroad *= loose(v.x, v.z); // paved run-off
        }
        let electric = ph.electric;
        let state = CarState {
            rpm: Some(ph.rpm),
            rpm_max: Some(7800.0),
            throttle: Some(if ph.locked {
                inp.throttle
            } else {
                ctrl_throttle
            }),
            gear: Some(f64::from(ph.gear)),
            speed: Some(psp),
            skid: Some(ph.skid),
            nitro: Some(ph.nitro_active),
            on_ground: Some(v.on_ground),
            scrape: Some(ph.scrape),
            boost: Some(ph.boost),
            slip: Some(ph.slip),
            offroad: Some(offroad),
            scrape_side: ph.scrape_side.map(f64::from),
            motor: electric.then(|| ph.rpm / MOTOR_MAX),
            power: electric.then_some(ph.power_out),
            regen: electric.then_some(ph.regen),
        };
        let rivals = st
            .rivals
            .iter()
            .map(|a| RivalAt {
                dx: a.k.v.x - v.x,
                dz: a.k.v.z - v.z,
                speed: a.k.speed,
                electric: a.k.v.kind == "electric",
            })
            .collect();
        // Tunnels get a concrete echo.
        let in_tunnel = t
            .tags
            .iter()
            .filter(|g| g.tag == "tunnel")
            .any(|g| v.s > g.s0 - 10.0 && v.s < g.s1 + 10.0);
        TickAudio {
            tick: st.tick,
            shots,
            state,
            nitro: ph.nitro_active,
            rivals,
            in_tunnel,
            car: Car::of(v),
            countdown: st.race.state == RaceStateKind::Countdown,
            bumps,
        }
    }
}

/// The settings the audio reads (`settings.music`, `.sfx`, `.track`).
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub music: f64,
    pub sfx: f64,
    /// `'auto'` (the level's own) or a track id.
    pub track: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            music: 0.7,
            sfx: 0.85,
            track: "auto".into(),
        }
    }
}

/// One call on the audio, as the game makes it.
#[derive(Clone, Debug)]
enum Call {
    SetPaused(bool),
    Init,
    Unlock,
    PlayTrack(String),
    SetVolume { music: f64, sfx: f64 },
    SetMusic(bool),
    SetCar(&'static str),
    NextTrack,
    UiClick(&'static str),
    Update(f64, Box<CarState>),
    NitroBurst,
    SetRivalEngines(Vec<Rival>),
    SetEnvironment(&'static str),
    Shot(Shot, f64),
}

/// A number as the facade recorder writes it: `-0` kept, non-finite as
/// strings.
fn num(x: f64) -> String {
    if x == 0.0 && x.is_sign_negative() {
        "-0".into()
    } else if x.is_finite() {
        mr_audio::wa::null::fmt_num(x)
    } else {
        format!("\"{}\"", mr_audio::wa::null::fmt_num(x))
    }
}

/// `{"k":v,...}` with the missing fields left out, in this order.
fn object(fields: &[(&str, Option<String>)]) -> String {
    let body: Vec<String> = fields
        .iter()
        .filter_map(|(k, v)| v.as_ref().map(|v| format!("\"{k}\":{v}")))
        .collect();
    format!("{{{}}}", body.join(","))
}

fn car_state_json(s: &CarState) -> String {
    let n = |x: Option<f64>| x.map(num);
    let b = |x: Option<bool>| x.map(|b| b.to_string());
    object(&[
        ("rpm", n(s.rpm)),
        ("rpmMax", n(s.rpm_max)),
        ("throttle", n(s.throttle)),
        ("gear", n(s.gear)),
        ("speed", n(s.speed)),
        ("skid", n(s.skid)),
        ("nitro", b(s.nitro)),
        ("onGround", b(s.on_ground)),
        ("scrape", n(s.scrape)),
        ("boost", n(s.boost)),
        ("slip", n(s.slip)),
        ("offroad", n(s.offroad)),
        ("scrapeSide", n(s.scrape_side)),
        ("motor", n(s.motor)),
        ("power", n(s.power)),
        ("regen", n(s.regen)),
    ])
}

impl Call {
    /// The facade recorder's line for this call.
    fn line(&self, tick: u32) -> String {
        let (method, args): (&str, Vec<String>) = match self {
            Call::SetPaused(on) => ("setPaused", vec![on.to_string()]),
            Call::Init => ("init", vec![]),
            Call::Unlock => ("unlock", vec![]),
            Call::PlayTrack(id) => ("playTrack", vec![format!("\"{id}\"")]),
            Call::SetVolume { music, sfx } => (
                "setVolume",
                vec![object(&[
                    ("music", Some(num(*music))),
                    ("sfx", Some(num(*sfx))),
                ])],
            ),
            Call::SetMusic(on) => ("setMusic", vec![on.to_string()]),
            Call::SetCar(k) => ("setCar", vec![format!("\"{k}\"")]),
            Call::NextTrack => ("nextTrack", vec![]),
            Call::UiClick(k) => ("uiClick", vec![format!("\"{k}\"")]),
            Call::Update(dt, s) => ("update", vec![num(*dt), car_state_json(s)]),
            Call::NitroBurst => ("nitroBurst", vec![]),
            Call::SetRivalEngines(list) => {
                let items: Vec<String> = list
                    .iter()
                    .map(|r| {
                        object(&[
                            ("dist", r.dist.map(num)),
                            ("pan", r.pan.map(num)),
                            ("rpmNorm", r.rpm_norm.map(num)),
                            ("electric", r.electric.map(|b| b.to_string())),
                        ])
                    })
                    .collect();
                ("setRivalEngines", vec![format!("[{}]", items.join(","))])
            }
            Call::SetEnvironment(e) => ("setEnvironment", vec![format!("\"{e}\"")]),
            Call::Shot(s, pan) => match *s {
                Shot::Beep(f) => ("beep", vec![f.to_string()]),
                Shot::Impact(strength, _) => ("impact", vec![num(strength), num(*pan)]),
                Shot::Shift(up) => ("shift", vec![up.to_string()]),
                Shot::Landing(strength) => ("landing", vec![num(strength)]),
                Shot::Whoosh(p, strength) => ("whoosh", vec![num(p), num(strength)]),
                Shot::FinishFanfare => ("finishFanfare", vec![]),
            },
        };
        let mut out = format!("[{tick},\"{method}\"");
        for a in args {
            out.push(',');
            out.push_str(&a);
        }
        out.push(']');
        out
    }

    fn apply(self, a: &mut GameAudio) {
        match self {
            Call::SetPaused(on) => a.set_paused(on),
            Call::Init => a.init(Default::default()),
            Call::Unlock => a.unlock(),
            Call::PlayTrack(id) => a.play_track(&id),
            Call::SetVolume { music, sfx } => a.set_volume(Volume {
                master: None,
                sfx: Some(sfx),
                music: Some(music),
            }),
            Call::SetMusic(on) => a.set_music(on),
            Call::SetCar(k) => a.set_car(k),
            Call::NextTrack => {
                a.next_track();
            }
            Call::UiClick(k) => a.ui_click(k),
            Call::Update(dt, s) => a.update(dt, &s),
            Call::NitroBurst => a.nitro_burst(),
            Call::SetRivalEngines(list) => a.set_rival_engines(&list),
            Call::SetEnvironment(e) => a.set_environment(e, false),
            Call::Shot(s, pan) => match s {
                Shot::Beep(f) => a.beep(f),
                Shot::Impact(strength, _) => a.impact(strength, pan),
                Shot::Shift(up) => a.shift(up),
                Shot::Landing(strength) => a.landing(strength),
                Shot::Whoosh(p, strength) => a.whoosh(p, strength),
                Shot::FinishFanfare => a.finish_fanfare(),
            },
        }
    }
}

/// The race's audio driver: `main.js`'s and `Race`'s side of the calls.
pub struct RaceAudio {
    pub audio: GameAudio,
    pub settings: Settings,
    /// `?audiolog=1`: the calls, as the facade recorder writes them.
    pub log: Option<Vec<String>>,
    /// The tick calls made now are logged at (`parity.onTick`'s count).
    tick: u32,
    /// `pickMusic`'s key: the level and the track choice it last played.
    music_key: Option<String>,
    /// The race (`Race::starts`) whose `startRace` calls were made.
    started: u32,
    /// The graph was built behind the loading screen.
    prepared: bool,
    was_nitro: bool,
    in_tunnel: bool,
    mode: Mode,
    /// The camera as the JS game's audio reads it (D515): a rig stepped
    /// per tick, the orientation its last `lookAt` gave (`q`) and the one
    /// `camera.matrixWorld` holds (`mw`), each a right vector (x, z).
    rig: CameraRig,
    q: (f64, f64),
    mw: (f64, f64),
    /// What the audio costs the main thread: the graph's build behind the
    /// loading screen, and the per-frame calls (ms).
    pub prepare_ms: f64,
    pub frame_ms_max: f64,
    /// The tick of the costliest frame.
    pub frame_ms_max_tick: u32,
    pub frame_ms_sum: f64,
    pub frames: u32,
}

impl RaceAudio {
    pub fn new(platform: Platform, settings: Settings, record: bool) -> RaceAudio {
        RaceAudio {
            audio: GameAudio::new(platform),
            settings,
            log: record.then(Vec::new),
            tick: 0,
            music_key: None,
            started: 0,
            prepared: false,
            was_nitro: false,
            in_tunnel: false,
            mode: Mode::Race,
            rig: CameraRig::default(),
            q: (1.0, 0.0),
            mw: (1.0, 0.0),
            prepare_ms: 0.0,
            frame_ms_max: 0.0,
            frame_ms_max_tick: 0,
            frame_ms_sum: 0.0,
            frames: 0,
        }
    }

    fn call(&mut self, c: Call) {
        if let Some(log) = &mut self.log {
            log.push(c.line(self.tick));
        }
        c.apply(&mut self.audio);
    }

    /// `wakeAudio`: `init(); unlock()`. Inside a user gesture on the web.
    pub fn wake(&mut self) {
        self.call(Call::Init);
        self.call(Call::Unlock);
        self.audio.settle();
    }

    /// Before the race, behind the loading screen: the car, then the graph
    /// on a context that stays suspended until a gesture (D514).
    pub fn prepare(&mut self, car: &'static str) {
        if self.prepared {
            return;
        }
        self.prepared = true;
        self.call(Call::SetCar(car));
        self.call(Call::Init);
        self.audio.settle();
    }

    /// `pickMusic`: only switch when the level or the choice changed, so a
    /// restart (or a skipped-to track) keeps playing.
    fn pick_music(&mut self, level: &str) {
        let key = format!("{level}|{}", self.settings.track);
        if self.music_key.as_deref() == Some(key.as_str()) {
            return;
        }
        self.music_key = Some(key);
        let id = if self.settings.track == "auto" {
            GameAudio::level_track(level).to_owned()
        } else {
            self.settings.track.clone()
        };
        self.call(Call::PlayTrack(id));
    }

    /// `applyVolume`.
    fn apply_volume(&mut self) {
        let (music, sfx) = (self.settings.music, self.settings.sfx);
        self.call(Call::SetVolume { music, sfx });
        self.call(Call::SetMusic(music > 0.001));
    }

    /// `startRace`'s audio: restart from the pause screen, wake, the
    /// level's music, the volumes, the car.
    pub fn start_race(&mut self, level: &str, car: &'static str) {
        self.call(Call::SetPaused(false));
        self.wake();
        self.pick_music(level);
        self.apply_volume();
        self.call(Call::SetCar(car));
        self.was_nitro = false;
        self.in_tunnel = false;
        self.mode = Mode::Race;
        self.rig = CameraRig::default();
    }

    /// `pause(on)`.
    pub fn pause(&mut self, on: bool) {
        self.call(Call::SetPaused(on));
    }

    /// The music key: off, or back on at 0.7 (`mr.musicVol`).
    pub fn toggle_music(&mut self) {
        if !self.audio.ready() {
            return;
        }
        self.settings.music = if self.settings.music > 0.001 {
            0.0
        } else {
            0.7
        };
        store_set("musicVol", &num(self.settings.music));
        self.apply_volume();
    }

    /// `nextTrack` (T).
    pub fn next_track(&mut self) {
        if self.audio.ready() {
            self.call(Call::NextTrack);
        }
    }

    /// A menu button's click (the results' race-again tap).
    pub fn ui_click(&mut self, kind: &'static str) {
        self.call(Call::UiClick(kind));
    }

    /// The menus' volume sliders and track picker (`crate::ui`):
    /// `applyVolume` when the sound is up, and `pickMusic` for a new track
    /// choice.
    pub fn menu_settings(&mut self, s: Settings, level: &str) {
        let track = s.track != self.settings.track;
        self.settings = s;
        if self.audio.ready() {
            self.apply_volume();
            if track {
                self.pick_music(level);
            }
        }
    }

    /// A level tab: `if (audio.ready) pickMusic()`.
    pub fn menu_level(&mut self, level: &str) {
        if self.audio.ready() {
            self.pick_music(level);
        }
    }

    /// `toMenu`: the sound unpaused, the engine idle, no rivals, the open
    /// road's acoustics.
    pub fn to_menu(&mut self) {
        self.call(Call::SetPaused(false));
        self.mode = Mode::Race;
        self.call(Call::Update(
            0.016,
            Box::new(CarState {
                rpm: Some(0.0),
                rpm_max: Some(7800.0),
                throttle: Some(0.0),
                gear: Some(0.0),
                speed: Some(0.0),
                skid: Some(0.0),
                nitro: Some(false),
                on_ground: Some(true),
                scrape: Some(0.0),
                ..CarState::default()
            }),
        ));
        self.call(Call::SetRivalEngines(Vec::new()));
        self.call(Call::SetEnvironment("open"));
    }

    /// The ticks of a frame, in order: each one's one-shots, then
    /// `update`, the nitro burst, the rivals and the tunnel (`Race.update`).
    /// The pans come from the camera as the JS reads it: `camera.matrixWorld`
    /// is brought up to date by the next `lookAt` (with the orientation the
    /// one before gave) and by the frame's render, so a contact hears the
    /// camera of the previous tick or two, and the rivals hear the chase
    /// camera's orientation during the countdown's swing and the previous
    /// tick's after it. `mode` is the drawn rig's camera mode (C cycles it),
    /// `look_back` the frame's look-back key.
    pub fn ticks(&mut self, ticks: &[TickAudio], track: &Track, mode: usize, look_back: bool) {
        while self.rig.mode != mode {
            self.rig.cycle();
        }
        for ta in ticks {
            // `parity.onTick` counts after each update: calls in tick k's
            // update are logged at k - 1.
            self.tick = ta.tick.saturating_sub(1);
            let (crx, crz) = self.mw;
            for s in &ta.shots {
                let pan = match *s {
                    Shot::Impact(_, Pan::At(dx, dz)) => {
                        clamp((dx * crx + dz * crz) / 2.0, -1.0, 1.0)
                    }
                    Shot::Impact(_, Pan::Fixed(p)) => p,
                    _ => 0.0,
                };
                self.call(Call::Shot(*s, pan));
            }
            for &b in &ta.bumps {
                self.rig.bump(b);
            }
            // `cam.update`, then during the countdown `introCamera`: each
            // ends in a `lookAt`.
            let view = self
                .rig
                .update(DT, track, &ta.car, look_back, ta.nitro, 1.6);
            self.look_at(&view);
            if ta.countdown {
                let view = self.rig.intro(DT, &ta.car, view);
                self.look_at(&view);
            }
            let (crx, crz) = self.mw;
            if self.audio.ready() {
                self.call(Call::Update(mr_sim::race::DT, Box::new(ta.state.clone())));
                if ta.nitro && !self.was_nitro {
                    self.call(Call::NitroBurst);
                }
                let mut near: Vec<Rival> = ta
                    .rivals
                    .iter()
                    .map(|r| {
                        let d = kernel::hypot(r.dx, r.dz);
                        Rival {
                            dist: Some(d),
                            pan: Some(clamp(
                                (r.dx * crx + r.dz * crz) / mr_math::js::max(d, 1.0),
                                -1.0,
                                1.0,
                            )),
                            rpm_norm: Some(clamp(r.speed / 70.0, 0.2, 1.0)),
                            electric: Some(r.electric),
                        }
                    })
                    .collect();
                // `.sort((a, b) => a.dist - b.dist)`: stable.
                near.sort_by(|a, b| {
                    let d = a.dist.unwrap_or(0.0) - b.dist.unwrap_or(0.0);
                    d.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
                });
                near.truncate(3);
                self.call(Call::SetRivalEngines(near));
            }
            self.was_nitro = ta.nitro;
            if ta.in_tunnel != self.in_tunnel {
                self.in_tunnel = ta.in_tunnel;
                self.call(Call::SetEnvironment(if ta.in_tunnel {
                    "tunnel"
                } else {
                    "open"
                }));
            }
            self.tick = ta.tick;
        }
        // The frame's render brings the matrix up to date.
        self.mw = self.q;
    }

    /// `camera.lookAt(target)`: the matrix takes the orientation the last
    /// one gave, then the new one is three's `Matrix4.lookAt` right vector,
    /// `normalize(up × normalize(eye - target))`.
    fn look_at(&mut self, v: &View) {
        self.mw = self.q;
        let z = v.eye - v.target;
        let zl = (z.x * z.x + z.y * z.y + z.z * z.z).sqrt();
        if zl == 0.0 {
            return;
        }
        let (zx, zz) = (z.x / zl, z.z / zl);
        let xl = (zz * zz + zx * zx).sqrt();
        if xl == 0.0 {
            return;
        }
        self.q = (zz / xl, -zx / xl);
    }

    /// The race's mode, each frame: pause and resume.
    pub fn mode(&mut self, m: Mode) {
        let was = self.mode;
        self.mode = m;
        match (was, m) {
            (Mode::Race, Mode::Paused) => self.pause(true),
            (Mode::Paused, Mode::Race) => self.pause(false),
            // End run (a cruise, from the pause screen):
            // `audio.setPaused(false); showResults(...)`.
            (Mode::Paused, Mode::Results) => self.pause(false),
            _ => {}
        }
    }

    /// The frame's timers (the music's scheduler, the gates' tails) and
    /// promises.
    pub fn poll(&mut self) {
        self.audio.poll();
    }
}

/// The client's audio, shared with the page's gesture handlers (web).
pub struct Shared(pub Rc<RefCell<RaceAudio>>);

#[cfg(target_arch = "wasm32")]
thread_local! {
    static GESTURE: RefCell<Option<Rc<RefCell<RaceAudio>>>> = const { RefCell::new(None) };
}

/// The page's gesture handlers (pointer-down, pointer-up, touch-end, click,
/// key-down), inside the handler: `wakeAudio`.
#[cfg(target_arch = "wasm32")]
pub fn gesture() {
    GESTURE.with(|g| {
        if let Some(a) = g.borrow().as_ref()
            && let Ok(mut a) = a.try_borrow_mut()
        {
            a.wake();
        }
    });
}

// ── Platform ─────────────────────────────────────────────────────────

/// `Math.random` for the audio's variations (not a parity stream).
#[cfg(target_arch = "wasm32")]
struct JsRandom;

#[cfg(target_arch = "wasm32")]
impl mr_math::Rng for JsRandom {
    fn next_f64(&mut self) -> f64 {
        js_sys::Math::random()
    }
}

/// The radio clips beside the page: `audio/radio/` at the repository's
/// root, two levels up from `dist/next/` (as Seaside's survey is fetched).
#[cfg(target_arch = "wasm32")]
const RADIO_BASE: &str = "../../audio/radio/";

#[cfg(target_arch = "wasm32")]
fn platform() -> Platform {
    use mr_audio::session::AudioSession;
    Platform {
        new_context: Some(Box::new(|o| {
            mr_audio::wa::web::context(o.latency_hint.as_deref()).ok()
        })),
        audio_session: mr_audio::session::web::navigator_session()
            .map(|s| Box::new(s) as Box<dyn AudioSession>),
        radio: Rc::new(mr_audio::radio::web::WebFetch {
            base: RADIO_BASE.into(),
        }),
        random: Rc::new(RefCell::new(JsRandom)),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn platform() -> Platform {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.subsec_nanos());
    Platform {
        new_context: Some(Box::new(|o| {
            mr_audio::wa::native::try_context(o.latency_hint.as_deref())
        })),
        audio_session: None,
        radio: Rc::new(mr_audio::radio::DirFetch(
            crate::native::repo_root().join("audio/radio"),
        )),
        random: Rc::new(RefCell::new(mr_math::Mulberry32::new(seed))),
    }
}

// ── The JS game's store (`mr.<key>`, JSON) ──────────────────────────

#[cfg(target_arch = "wasm32")]
fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

#[cfg(target_arch = "wasm32")]
fn store_get(k: &str) -> Option<String> {
    storage()?.get_item(&format!("mr.{k}")).ok().flatten()
}

#[cfg(not(target_arch = "wasm32"))]
fn store_get(_k: &str) -> Option<String> {
    None
}

/// `store.set(k, v)`, `v` already JSON.
#[cfg(target_arch = "wasm32")]
fn store_set(k: &str, json: &str) {
    if let Some(s) = storage() {
        let _ = s.set_item(&format!("mr.{k}"), json);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn store_set(_k: &str, _json: &str) {}

/// `settings.music`, `.sfx` and `.track` as `main.js` reads them.
fn settings() -> Settings {
    let d = Settings::default();
    let number = |k: &str, d: f64| {
        store_get(k)
            .and_then(|v| serde_json::from_str::<serde_json::Value>(&v).ok())
            .and_then(|v| v.as_f64())
            .unwrap_or(d)
    };
    let track = store_get("track")
        .and_then(|v| serde_json::from_str::<String>(&v).ok())
        .filter(|t| t == "auto" || GameAudio::tracks().iter().any(|i| i.id == t))
        .unwrap_or(d.track);
    Settings {
        music: number("musicVol", d.music),
        sfx: number("sfxVol", d.sfx),
        track,
    }
}

// ── The Bevy side ────────────────────────────────────────────────────

pub fn plugin(app: &mut App) {
    let record = app.world().resource::<Opts>().o.param("audiolog") == Some("1");
    let shared = Rc::new(RefCell::new(RaceAudio::new(platform(), settings(), record)));
    #[cfg(target_arch = "wasm32")]
    GESTURE.with(|g| *g.borrow_mut() = Some(shared.clone()));
    app.insert_non_send(Shared(shared)).add_systems(
        Update,
        frame.after(super::draw).run_if(in_state(AppState::Running)),
    );
}

/// The test bridge's context calls (`__audio.ctx.suspend()`, as iOS
/// interrupts a context on a call), made at the next frame.
static CTX_OPS: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());

/// `__mr.stage({cmd: 'audio', op})`: `suspend` or `resume` the context.
pub fn stage_ctx(op: &str) {
    let op = match op {
        "suspend" => "suspend",
        "resume" => "resume",
        _ => return,
    };
    CTX_OPS.lock().unwrap_or_else(|e| e.into_inner()).push(op);
}

/// After the frame's ticks and the camera: the race's audio calls.
fn frame(shared: NonSend<Shared>, mut play: ResMut<Play>, mut keys: MessageReader<KeyboardInput>) {
    let mut a = shared.0.borrow_mut();
    for op in std::mem::take(&mut *CTX_OPS.lock().unwrap_or_else(|e| e.into_inner())) {
        if let Some(c) = a.audio.ctx() {
            let _ = if op == "suspend" {
                c.suspend()
            } else {
                c.resume()
            };
        }
    }
    let next = keys
        .read()
        .any(|k| k.state == ButtonState::Pressed && !k.repeat && k.key_code == KeyCode::KeyT);
    let started = play.started;
    let Some(race) = play.race.as_mut() else {
        // The menus (`crate::ui`): the music plays on, and the page still
        // sees the sound's state.
        if next {
            a.next_track();
        }
        a.poll();
        publish(&mut a);
        return;
    };
    let t0 = now_ms();
    if !started {
        if !a.prepared {
            a.prepare(race.setup.opts.car);
            a.prepare_ms = now_ms() - t0;
        }
        a.poll();
        publish(&mut a);
        return;
    }
    if a.started != race.starts {
        if a.started != 0 {
            a.ui_click("start"); // the results' "race again"
        }
        a.started = race.starts;
        // Calls before the race's first tick are logged at the tick count
        // so far (0 for the first race).
        a.tick = race
            .audio_ticks
            .first()
            .map_or(race.session.curr.tick, |t| t.tick - 1);
        a.start_race(race.session.lr.level.id, race.setup.opts.car);
    }
    if race.music_pressed {
        a.toggle_music();
    }
    if next {
        a.next_track();
    }
    a.mode(race.mode);
    let track = race.session.lr.track.clone();
    let (mode, look_back) = (race.rig.mode, race.input.state.look_back);
    a.ticks(&race.audio_ticks, &track, mode, look_back);
    race.audio_ticks.clear();
    a.poll();
    let ms = now_ms() - t0;
    if ms > a.frame_ms_max {
        a.frame_ms_max = ms;
        a.frame_ms_max_tick = a.tick;
    }
    a.frame_ms_sum += ms;
    a.frames += 1;
    publish(&mut a);
}

/// A millisecond clock for the cost figures.
#[cfg(target_arch = "wasm32")]
fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map_or(0.0, |p| p.now())
}

#[cfg(not(target_arch = "wasm32"))]
fn now_ms() -> f64 {
    use std::sync::OnceLock;
    static T0: OnceLock<std::time::Instant> = OnceLock::new();
    T0.get_or_init(std::time::Instant::now)
        .elapsed()
        .as_secs_f64()
        * 1000.0
}

/// `window.__mr.audio` (the context's state, ready, the calls so far) and,
/// with `?audiolog=1`, the new lines onto `window.__mr.audioLog`.
#[cfg(target_arch = "wasm32")]
fn publish(a: &mut RaceAudio) {
    use js_sys::{Array, Object, Reflect};
    use wasm_bindgen::{JsCast, JsValue};
    let Some(w) = web_sys::window() else { return };
    let Some(mr) = Reflect::get(&w, &JsValue::from_str("__mr"))
        .ok()
        .and_then(|v| v.dyn_into::<Object>().ok())
    else {
        return;
    };
    let o = Object::new();
    let state = a.audio.ctx().map_or("none", |c| match c.state() {
        mr_audio::wa::ContextState::Running => "running",
        mr_audio::wa::ContextState::Suspended => "suspended",
        mr_audio::wa::ContextState::Closed => "closed",
    });
    let set = |o: &Object, k: &str, v: JsValue| {
        let _ = Reflect::set(o, &JsValue::from_str(k), &v);
    };
    set(&o, "context", JsValue::from_str(state));
    set(&o, "ready", JsValue::from_bool(a.audio.ready()));
    set(
        &o,
        "currentTime",
        JsValue::from_f64(a.audio.ctx().map_or(0.0, |c| c.current_time())),
    );
    set(&o, "music", JsValue::from_f64(a.settings.music));
    set(&o, "sfx", JsValue::from_f64(a.settings.sfx));
    set(&o, "track", JsValue::from_str(&a.settings.track));
    set(
        &o,
        "playing",
        a.audio
            .track_info()
            .map_or(JsValue::NULL, |t| JsValue::from_str(t.id)),
    );
    // What the e2e suites read off `__audio` (`_musicOn`,
    // `musicGate.gain.value`, `_vol`).
    set(&o, "musicOn", JsValue::from_bool(a.audio.music_on()));
    set(&o, "musicGate", JsValue::from_f64(a.audio.music_gate()));
    let (master, sfx, music) = a.audio.volumes();
    let vol = Object::new();
    set(&vol, "master", JsValue::from_f64(master));
    set(&vol, "sfx", JsValue::from_f64(sfx));
    set(&vol, "music", JsValue::from_f64(music));
    set(&o, "vol", vol.into());
    set(&o, "prepareMs", JsValue::from_f64(a.prepare_ms));
    set(&o, "frameMsMax", JsValue::from_f64(a.frame_ms_max));
    set(
        &o,
        "frameMsMaxTick",
        JsValue::from_f64(f64::from(a.frame_ms_max_tick)),
    );
    set(
        &o,
        "frameMsMean",
        JsValue::from_f64(a.frame_ms_sum / f64::from(a.frames.max(1))),
    );
    set(&mr, "audio", o.into());
    if let Some(log) = &mut a.log
        && !log.is_empty()
    {
        let arr = Reflect::get(&mr, &JsValue::from_str("audioLog"))
            .ok()
            .and_then(|v| v.dyn_into::<Array>().ok())
            .unwrap_or_else(|| {
                let arr = Array::new();
                set(&mr, "audioLog", arr.clone().into());
                arr
            });
        for l in log.drain(..) {
            arr.push(&JsValue::from_str(&l));
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn publish(_a: &mut RaceAudio) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::play::flow::{Race, Setup};
    use crate::play::touch::TouchControls;
    use mr_audio::wa::ContextState;
    use mr_audio::wa::null::{self, NullOptions};
    use mr_sim::race::RaceOpts;
    use std::io::Read;

    /// A null-backend context: the calls are made, nothing is heard.
    fn null_platform() -> Platform {
        Platform {
            new_context: Some(Box::new(|o| {
                Some(
                    null::context(
                        NullOptions {
                            sample_rate: 48000.0,
                            log: false,
                            state: ContextState::Suspended,
                        },
                        o.clone(),
                    )
                    .0,
                )
            })),
            ..Platform::headless(1)
        }
    }

    /// The scripted race of the JS reference (`drive-race.jsonl.gz`: Sierra,
    /// the sports car, seed 1, the autopilot, 45 s), through the client's
    /// own frame loop at 60 frames a second: the calls the client makes on
    /// the audio are the JS game's, line for line (the browser check is
    /// `tools/parity/rust-audio-race.mjs`).
    #[test]
    fn the_scripted_race_makes_the_js_calls() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../parity/golden/audio/drive-race.jsonl.gz");
        let mut js = String::new();
        flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap())
            .read_to_string(&mut js)
            .unwrap();
        let js: Vec<&str> = js.lines().filter(|l| !l.is_empty()).collect();

        let opts = RaceOpts {
            car: "sports",
            seed: 1,
            pursuit: false,
            heat: 1.0,
        };
        let lr = LevelRuntime::new(mr_levels::level_by_id("sierra")).unwrap();
        let mut race = Race::new(
            lr,
            Setup {
                opts,
                autodrive: true,
                touch: false,
            },
            TouchControls::default(),
        );
        let mut a = RaceAudio::new(null_platform(), Settings::default(), true);
        a.started = race.starts;
        a.start_race("sierra", "sports");
        while race.session.curr.tick < 5400 {
            race.frame(1.0 / 60.0);
            let ticks: Vec<TickAudio> = race
                .audio_ticks
                .iter()
                .filter(|t| t.tick <= 5400)
                .cloned()
                .collect();
            a.ticks(&ticks, &race.session.lr.track, 0, false);
            a.poll();
        }
        compare(&a.log.take().unwrap(), &js);
    }

    /// Line by line: every argument bit for bit but the pans from the
    /// camera, which come out of three's quaternion round trip in the JS
    /// and are held to 1e-9.
    fn compare(ours: &[String], js: &[&str]) {
        use serde_json::Value;
        let mut pans = Vec::new();
        let mask = |l: &str, pans: &mut Vec<f64>| -> Value {
            let mut v: Value = serde_json::from_str(l).unwrap();
            let m = v[1].as_str().unwrap().to_owned();
            if m == "impact" {
                pans.push(v[3].as_f64().unwrap());
                v[3] = Value::Null;
            }
            if m == "setRivalEngines"
                && let Some(list) = v[2].as_array_mut()
            {
                for r in list {
                    pans.push(r["pan"].as_f64().unwrap());
                    r["pan"] = Value::Null;
                }
            }
            v
        };
        let mut worst = 0f64;
        for (i, (a, b)) in ours.iter().zip(js).enumerate() {
            let (mut pa, mut pb) = (Vec::new(), Vec::new());
            assert_eq!(
                mask(a, &mut pa),
                mask(b, &mut pb),
                "line {}:\n  rust {a}\n  js   {b}",
                i + 1
            );
            for (x, y) in pa.iter().zip(&pb) {
                let d = (x - y).abs();
                worst = worst.max(d);
                assert!(d <= 1e-9, "line {}: pan {x} against {y}", i + 1);
            }
            pans.extend(pa);
        }
        println!(
            "{} calls, {} pans, the largest pan difference {worst:e}",
            ours.len(),
            pans.len()
        );
        assert_eq!(ours.len(), js.len(), "call count");
    }
}
