//! The race's flow without the engine (roadmap WP 4.6): one frame of the
//! session as `main.js`'s `tick` and the presentation half of `Race.update`
//! run it — pause, the input layer at the tick rate, the ticks, and the
//! simulation's events turned into the HUD's centre text and toasts, camera
//! bumps and body-spring kicks — then countdown → race → finish → results.
//! The Bevy systems ([`super`]) draw it; [`smoke_race`] runs it headless.

use mr_sim::autopilot::autopilot;
use mr_sim::input::{InputFrame, RESET};
use mr_sim::physics::PhysEvent;
use mr_sim::race::{
    DT, LevelRuntime, RaceOpts, RaceStateKind, ResultRow, SimEvent, SimState, results,
};

use super::audio::TickAudio;
use super::camera::CameraRig;
use super::input::Input;
use super::pose::Springs;
use super::session::Session;
use super::touch::TouchControls;

/// `fmtTime`: m:ss.cc, rounded to hundredths first (59.996 s reads
/// 1:00.00), `--:--.--` for a missing time.
pub fn fmt_time(t: Option<f64>) -> String {
    let Some(t) = t.filter(|t| t.is_finite()) else {
        return "--:--.--".into();
    };
    let cs = mr_math::js::round(t * 100.0);
    let m = (cs / 6000.0).floor();
    let s = (cs - m * 6000.0) / 100.0;
    format!("{m}:{:0>5.2}", s)
}

/// "1st", "2nd", "3rd", "4th", …
pub fn ordinal(n: usize) -> String {
    let suffix = match n {
        1 => "st",
        2 => "nd",
        3 => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// The HUD's centre text and toast (`hud.center`, `hud.toast`), with their
/// timers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hud {
    pub center: Option<String>,
    pub center_timer: f64,
    pub toast: Option<String>,
    pub toast_timer: f64,
}

impl Hud {
    pub fn center(&mut self, text: impl Into<String>, dur: f64) {
        self.center = Some(text.into());
        self.center_timer = dur;
    }

    pub fn toast(&mut self, text: impl Into<String>, dur: f64) {
        self.toast = Some(text.into());
        self.toast_timer = dur;
    }

    pub fn tick(&mut self, dt: f64) {
        self.center_timer = (self.center_timer - dt).max(0.0);
        if self.center_timer <= 0.0 {
            self.center = None;
        }
        self.toast_timer = (self.toast_timer - dt).max(0.0);
        if self.toast_timer <= 0.0 {
            self.toast = None;
        }
    }
}

/// Every race start in the client gets its own number (`Race::starts`),
/// so a race built afresh from the menu is told from the one before it as
/// a restart is (the audio's `startRace` calls key on it).
fn next_start() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static STARTS: AtomicU32 = AtomicU32::new(0);
    STARTS.fetch_add(1, Ordering::Relaxed) + 1
}

/// Where the session is, for the screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Driving (countdown, race, after the finish until the results).
    Race,
    Paused,
    /// The results are up; the race goes on behind them.
    Results,
}

/// Options of a race in the client.
#[derive(Clone, Copy, Debug)]
pub struct Setup {
    pub opts: RaceOpts,
    /// `?autodrive=1`: the autopilot drives the player.
    pub autodrive: bool,
    /// Touch: the stuck hint names the ↺ button, not R.
    pub touch: bool,
}

/// One race in the client: the session, the input layer, the camera, the
/// springs and the HUD state.
pub struct Race {
    pub session: Session,
    pub setup: Setup,
    pub input: Input,
    pub touch: TouchControls,
    pub rig: CameraRig,
    /// Per car slot (players, rivals, the traffic pool).
    pub springs: Vec<Springs>,
    pub hud: Hud,
    pub mode: Mode,
    pub results: Option<Vec<ResultRow>>,
    /// The events of the ticks of the last frame (for the camera bumps and
    /// the tests).
    pub log: Vec<SimEvent>,
    /// What the audio hears of each tick of the last frame, in order
    /// ([`super::audio`]).
    pub audio_ticks: Vec<TickAudio>,
    /// The music key (M) was pressed this frame.
    pub music_pressed: bool,
    /// Races started on this `Race` (`restart` counts one more).
    pub starts: u32,
}

/// Every car slot of a state: the players, the rivals, the traffic pool
/// (whether it is on the road), in a fixed order.
pub fn slots(st: &SimState) -> impl Iterator<Item = (&mr_sim::vehicle::Vehicle, bool)> {
    st.players
        .iter()
        .map(|p| (&p.v, true))
        .chain(st.rivals.iter().map(|r| (&r.k.v, true)))
        .chain(
            st.traffic
                .cars
                .iter()
                .map(|c| (&c.k.v, c.active && c.k.v.alive)),
        )
}

impl Race {
    pub fn new(lr: LevelRuntime, setup: Setup, touch: TouchControls) -> Race {
        let session = Session::new(lr, setup.opts);
        let n = slots(&session.curr).count();
        Race {
            session,
            setup,
            input: Input::new(),
            touch,
            rig: CameraRig::default(),
            springs: vec![Springs::default(); n],
            hud: Hud::default(),
            mode: Mode::Race,
            results: None,
            log: Vec::new(),
            audio_ticks: Vec::new(),
            music_pressed: false,
            starts: next_start(),
        }
    }

    /// `startRace` again on the same level: a new field on the grid, keys
    /// pressed before it do not carry in.
    pub fn restart(&mut self, opts: RaceOpts) {
        self.setup.opts = opts;
        let curr = SimState::new(&self.session.lr, opts);
        self.session.prev = curr.clone();
        self.session.curr = curr;
        self.session.events.clear();
        self.rig = CameraRig::default();
        self.springs = vec![Springs::default(); slots(&self.session.curr).count()];
        self.hud = Hud::default();
        self.mode = Mode::Race;
        self.results = None;
        self.input.pressed.clear();
        self.input.enabled = true;
        self.starts = next_start();
    }

    pub fn state(&self) -> RaceStateKind {
        self.session.curr.race.state
    }

    /// `pause(on)`: only a race in progress pauses.
    pub fn pause(&mut self, on: bool) {
        if on && self.mode != Mode::Race {
            return;
        }
        if !on && self.mode != Mode::Paused {
            return;
        }
        self.mode = if on { Mode::Paused } else { Mode::Race };
        if !on {
            // Keys pressed while paused don't carry into the race.
            self.input.pressed.clear();
        }
    }

    /// One rendered frame: pause and the one-shot actions, then the ticks
    /// that came due (the input layer and the autopilot once per tick), then
    /// the events. `dt` is the frame's time (already scaled by
    /// `?timescale`).
    pub fn frame(&mut self, dt: f64) {
        self.log.clear();
        self.audio_ticks.clear();
        if self.input.consume("pause") {
            match self.mode {
                Mode::Race => self.pause(true),
                Mode::Paused => self.pause(false),
                Mode::Results => {}
            }
        }
        self.music_pressed = self.input.consume("music");
        if self.mode == Mode::Paused {
            return;
        }
        if self.input.consume("camera") {
            self.rig.cycle();
        }
        let Race {
            session,
            input,
            touch,
            setup,
            audio_ticks,
            ..
        } = self;
        let autodrive = setup.autodrive;
        let track = session.lr.track.clone();
        let mut first = true;
        session.advance_observed(
            dt,
            |st| {
                let s = input.update(DT, Some(touch));
                let mut inp = s.sim();
                if autodrive {
                    autopilot(&mut inp, &st.players[0].v, &track);
                }
                let mut f = InputFrame::quantise(&inp);
                // The reset key is read once a frame, by its first tick.
                if first && input.consume("reset") {
                    f.flags |= RESET;
                }
                first = false;
                f
            },
            |lr, st, ev, f| audio_ticks.push(TickAudio::of(lr, st, ev, f)),
        );
        self.touch.tick(dt);
        let events = std::mem::take(&mut self.session.events);
        for e in &events {
            self.on_event(e);
        }
        self.log = events;
        self.stuck_hint();
        self.hud.tick(dt);
    }

    /// The HUD, camera and spring reactions to an event (`Race.update`'s
    /// calls to `hud`, `cam` and the vehicle).
    fn on_event(&mut self, e: &SimEvent) {
        let laps = self.session.curr.race.laps;
        match e {
            SimEvent::Countdown(n) => self.hud.center(n.to_string(), 1.0),
            SimEvent::Go => self.hud.center("GO!", 1.0),
            SimEvent::PerfectStart => self.hud.toast("PERFECT START", 1.6),
            SimEvent::Bonus {
                text,
                nitro,
                points,
                ..
            } => {
                let t = if self.session.curr.race.cruise && *points > 0.0 {
                    format!("{text}  +{}", mr_math::js::round(*points))
                } else if *nitro > 0.0 {
                    format!("{text}  +N2O")
                } else {
                    text.clone()
                };
                self.hud.toast(t, 1.6);
            }
            SimEvent::Lap {
                lap, time, best, ..
            } => {
                let c = if *lap == laps as i32 {
                    "FINAL LAP".to_string()
                } else {
                    format!("LAP {lap}/{laps}")
                };
                self.hud.center(c, 1.4);
                self.hud.toast(
                    format!(
                        "LAP {}{}",
                        fmt_time(Some(*time)),
                        if *best { "  BEST" } else { "" }
                    ),
                    2.2,
                );
            }
            SimEvent::WrongWay { .. } => {
                if self.hud.center_timer <= 0.0 {
                    self.hud.center("WRONG WAY", 1.0);
                }
            }
            SimEvent::Finished { place, .. } => {
                let t = if *place == 1 {
                    "WINNER!".to_string()
                } else {
                    format!("{} PLACE", ordinal(*place))
                };
                self.hud.center(t, 2.0);
            }
            SimEvent::Results => {
                self.results = Some(results(&self.session.curr));
                self.mode = Mode::Results;
            }
            SimEvent::CarHit {
                hit,
                player: Some(_),
            } => self.rig.bump(hit.strength * 1.2),
            SimEvent::Phys { e, player } => match e {
                PhysEvent::Impact { strength, .. } => self.rig.bump(*strength),
                PhysEvent::Land { strength, .. } => self.rig.bump(strength * 0.8),
                PhysEvent::Touchdown { impact } => {
                    if let Some(s) = self.springs.get_mut(*player) {
                        s.touchdown(*impact);
                    }
                }
                PhysEvent::Shift { .. } => {}
            },
            _ => {}
        }
    }

    /// Stuck? Offer the reset key (`Race.update`, after the wrong-way check).
    fn stuck_hint(&mut self) {
        let st = &self.session.curr;
        let p = &st.players[0];
        let held = st
            .pv
            .as_ref()
            .is_some_and(|pv| pv.held() || pv.pursuit.bust > 0.0);
        if p.rules.stuck.is_some_and(|s| s > 3.0) && self.hud.toast_timer <= 0.0 && !held {
            let t = if self.setup.touch {
                "STUCK? TAP R TO RESET"
            } else {
                "STUCK? PRESS R TO RESET"
            };
            self.hud.toast(t, 2.0);
        }
    }
}

/// What [`smoke_race`] saw.
#[derive(Clone, Debug)]
pub struct Smoke {
    pub results: Vec<ResultRow>,
    /// Centre texts in the order they showed.
    pub centers: Vec<String>,
    pub ticks: u32,
    pub race_time: f64,
    pub hash: u64,
}

/// A race from the grid to the results with the autopilot driving, through
/// the client's own frame loop (input layer, session, events, HUD), at a
/// steady `frame_dt`, headless. For `--smoke-race` and the tests.
pub fn smoke_race(lr: LevelRuntime, opts: RaceOpts, frame_dt: f64) -> Result<Smoke, String> {
    let mut race = Race::new(
        lr,
        Setup {
            opts,
            autodrive: true,
            touch: false,
        },
        TouchControls::default(),
    );
    let mut centers: Vec<String> = Vec::new();
    // Fifteen minutes of race at most (the JS recordings' cap).
    let limit = (15.0 * 60.0 / frame_dt) as u64;
    for _ in 0..limit {
        race.frame(frame_dt);
        if let Some(c) = &race.hud.center
            && centers.last() != Some(c)
            && race.hud.center_timer > 0.0
        {
            centers.push(c.clone());
        }
        if let Some(r) = &race.results {
            return Ok(Smoke {
                results: r.clone(),
                centers,
                ticks: race.session.curr.tick,
                race_time: race.session.curr.race.time,
                hash: mr_sim::race::hash(&race.session.curr),
            });
        }
    }
    Err(format!(
        "no results after {:.0} s of race (state {:?})",
        race.session.curr.race.time,
        race.state()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mr_sim::input::Input as SimInput;
    use mr_sim::race::step;

    #[test]
    fn fmt_time_as_the_hud() {
        assert_eq!(fmt_time(None), "--:--.--");
        assert_eq!(fmt_time(Some(f64::NAN)), "--:--.--");
        assert_eq!(fmt_time(Some(59.996)), "1:00.00");
        assert_eq!(fmt_time(Some(83.456)), "1:23.46");
        assert_eq!(fmt_time(Some(5.0)), "0:05.00");
    }

    /// The whole flow, headless: countdown 3, 2, 1, GO!, the finish, the
    /// results, and the results are the simulation's own (the same race
    /// stepped directly with the autopilot: no drift through the client's
    /// frame loop).
    #[test]
    fn smoke_race_sierra() {
        let opts = RaceOpts {
            car: "sports",
            seed: 1,
            pursuit: false,
            heat: 1.0,
        };
        let lr = LevelRuntime::new(mr_levels::level_by_id("sierra")).unwrap();
        let smoke = smoke_race(lr, opts, 1.0 / 60.0).unwrap();
        assert_eq!(&smoke.centers[..4], ["3", "2", "1", "GO!"]);
        assert!(
            smoke
                .centers
                .iter()
                .any(|c| c == "WINNER!" || c.ends_with(" PLACE")),
            "{:?}",
            smoke.centers
        );
        assert_eq!(smoke.results.len(), 6);
        assert_eq!(smoke.results.iter().filter(|r| r.player).count(), 1);

        let lr = LevelRuntime::new(mr_levels::level_by_id("sierra")).unwrap();
        let mut st = SimState::new(&lr, opts);
        let mut ev = Vec::new();
        while st.tick < smoke.ticks {
            let mut inp = SimInput::default();
            autopilot(&mut inp, &st.players[0].v, &lr.track);
            step(&lr, &mut st, &[InputFrame::quantise(&inp)], &mut ev);
        }
        assert_eq!(mr_sim::race::hash(&st), smoke.hash);
        assert_eq!(results(&st), smoke.results);
    }

    #[test]
    fn pause_holds_the_race() {
        let lr = LevelRuntime::new(mr_levels::level_by_id("sierra")).unwrap();
        let mut race = Race::new(
            lr,
            Setup {
                opts: RaceOpts {
                    car: "sports",
                    seed: 1,
                    pursuit: false,
                    heat: 1.0,
                },
                autodrive: false,
                touch: false,
            },
            TouchControls::default(),
        );
        race.frame(0.1);
        let t = race.session.curr.tick;
        race.input.key_down("Escape", false);
        race.frame(0.1);
        assert_eq!(race.mode, Mode::Paused);
        race.frame(0.1);
        assert_eq!(race.session.curr.tick, t);
        race.input.key_up("Escape");
        race.input.key_down("KeyP", false);
        race.frame(0.05);
        assert_eq!(race.mode, Mode::Race);
        assert_eq!(race.session.curr.tick, t + 6);
    }
}
