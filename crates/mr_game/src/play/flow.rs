//! The race's flow without the engine (roadmap WP 4.6): one frame of the
//! session as `main.js`'s `tick` and the presentation half of `Race.update`
//! run it — pause, the input layer at the tick rate, the ticks, and the
//! simulation's events turned into the HUD's centre text and toasts, camera
//! bumps and body-spring kicks — then countdown → race → finish → results.
//! The Bevy systems ([`super`]) draw it; [`smoke_race`] runs it headless.

use mr_sim::autopilot::autopilot;
use mr_sim::input::{InputFrame, RESET};
use mr_sim::physics::PhysEvent;
use mr_sim::pursuit::PursuitEvent;
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
    /// `center()` and `toast()` calls so far: each restarts the HUD's pop
    /// or toast (`play::hud`).
    pub centers: u32,
    pub toasts: u32,
}

impl Hud {
    pub fn center(&mut self, text: impl Into<String>, dur: f64) {
        self.center = Some(text.into());
        self.center_timer = dur;
        self.centers += 1;
    }

    pub fn toast(&mut self, text: impl Into<String>, dur: f64) {
        self.toast = Some(text.into());
        self.toast_timer = dur;
        self.toasts += 1;
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
    /// The pads should quiet what is held (`input.pads.hush()` at the
    /// start and on resume); the client's pad system does it.
    pub hush_pads: bool,
    /// Rumble asked for this frame (`pads.kick`): strong, weak, ms.
    pub kicks: Vec<(f64, f64, f64)>,
    /// The steady buzz this frame (`pads.feel`, `Race.rumble`); `None`
    /// while paused.
    pub feel: Option<(f64, f64)>,
    /// What the reset action is on the pad in hand, while one is connected
    /// (`pads.label('reset')`, for the stuck hint).
    pub pad_reset: Option<String>,
    /// Events the test bridge adds to the next frame's, after its ticks'
    /// (`race.phys.events.push(…)` in the JS suites: a wall impact for the
    /// rumble).
    pub inject: Vec<SimEvent>,
    /// Dispatch: the lines said this frame (`play::radio`, WP 8.4).
    pub radio: super::radio::Radio,
    /// Lines the test bridge says (`race.pv.say(line, now)`), said in the
    /// next frame after the radio's new frame.
    pub stage_say: Vec<(mr_audio::radio::lines::Line, bool)>,
    /// Hot Pursuit's options the simulation leaves to the page
    /// (`?cops=N`, the flash setting), set on every new field.
    pub pursuit_opts: PursuitOpts,
    /// Frames run (not paused): tells a frame's `radio.said` from the
    /// last one's.
    pub frames: u32,
    was_nitro: bool,
    offroad: f64,
}

/// `new PursuitView(race, { cops, flash })`: the units' cap (`?cops=N`, 6
/// by default) and whether the lights strobe (the menu's option).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PursuitOpts {
    pub cops: f64,
    pub flash: bool,
}

impl Default for PursuitOpts {
    fn default() -> PursuitOpts {
        PursuitOpts {
            cops: 6.0,
            flash: true,
        }
    }
}

/// `new Pursuit({ maxUnits: cops, flash })` on a field the simulation made
/// with its defaults (6 units, flashing).
fn apply_pursuit_opts(st: &mut SimState, o: PursuitOpts) {
    if let Some(pv) = st.pv.as_mut() {
        pv.pursuit.max_units = mr_math::clamp(o.cops, 0.0, 6.0) as usize;
        pv.pursuit.flash = o.flash;
    }
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
            // Nor does the A that started it.
            hush_pads: true,
            kicks: Vec::new(),
            feel: None,
            pad_reset: None,
            inject: Vec::new(),
            radio: super::radio::Radio::default(),
            stage_say: Vec::new(),
            pursuit_opts: PursuitOpts::default(),
            frames: 0,
            was_nitro: false,
            offroad: 0.0,
        }
    }

    /// Hot Pursuit's page options, for this field and every restart.
    pub fn set_pursuit_opts(&mut self, o: PursuitOpts) {
        self.pursuit_opts = o;
        apply_pursuit_opts(&mut self.session.curr, o);
        apply_pursuit_opts(&mut self.session.prev, o);
    }

    /// `startRace` again on the same level: a new field on the grid, keys
    /// pressed before it do not carry in.
    pub fn restart(&mut self, opts: RaceOpts) {
        self.setup.opts = opts;
        let mut curr = SimState::new(&self.session.lr, opts);
        apply_pursuit_opts(&mut curr, self.pursuit_opts);
        self.session.prev = curr.clone();
        self.session.curr = curr;
        self.session.events.clear();
        self.rig = CameraRig::default();
        self.springs = vec![Springs::default(); slots(&self.session.curr).count()];
        self.hud = Hud::default();
        self.radio = super::radio::Radio::default();
        self.mode = Mode::Race;
        self.results = None;
        self.input.pressed.clear();
        self.input.enabled = true;
        self.hush_pads = true;
        self.was_nitro = false;
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
            self.hush_pads = true;
        }
    }

    /// One rendered frame: pause and the one-shot actions, then the ticks
    /// that came due (the input layer and the autopilot once per tick), then
    /// the events. `dt` is the frame's time (already scaled by
    /// `?timescale`).
    pub fn frame(&mut self, dt: f64) {
        self.log.clear();
        self.audio_ticks.clear();
        self.kicks.clear();
        self.feel = None;
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
        // PursuitView.events' `radioT -= dt`, once a frame; the bridge's
        // lines first.
        self.frames += 1;
        self.radio.frame(dt);
        for (line, force) in std::mem::take(&mut self.stage_say) {
            self.radio.say(line, force);
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
        let mut events = std::mem::take(&mut self.session.events);
        events.append(&mut self.inject);
        for e in &events {
            self.on_event(e);
        }
        // WP 8.4: pursuit events → radio lines here.
        self.log = events;
        self.rumble();
        self.stuck_hint();
        self.hud.tick(dt);
    }

    /// The HUD, camera and spring reactions to an event (`Race.update`'s
    /// calls to `hud`, `cam` and the vehicle).
    fn on_event(&mut self, e: &SimEvent) {
        let laps = self.session.curr.race.laps;
        match e {
            SimEvent::Countdown(n) => {
                self.hud.center(n.to_string(), 1.0);
                self.kicks.push((0.0, 0.25, 90.0));
            }
            SimEvent::Go => {
                self.hud.center("GO!", 1.0);
                self.kicks.push((0.3, 0.6, 200.0));
            }
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
            } => {
                self.jolt(hit.strength);
                self.rig.bump(hit.strength * 1.2);
            }
            SimEvent::Phys { e, player } => match e {
                PhysEvent::Impact { strength, .. } => {
                    if *player == 0 {
                        self.jolt(*strength);
                    }
                    self.rig.bump(*strength)
                }
                PhysEvent::Land { strength, .. } => {
                    if *player == 0 {
                        self.kicks.push((0.7 * strength, 0.5 * strength, 180.0));
                    }
                    self.rig.bump(strength * 0.8)
                }
                PhysEvent::Touchdown { impact } => {
                    if let Some(s) = self.springs.get_mut(*player) {
                        s.touchdown(*impact);
                    }
                }
                PhysEvent::Shift { .. } => {
                    if *player == 0 {
                        self.kicks.push((0.0, 0.2, 70.0));
                    }
                }
            },
            // PursuitView's texts and camera jolts, then its rumble.
            SimEvent::Pursuit(pe) => {
                super::hud::pursuit::event(&mut self.hud, &mut self.rig, pe);
                self.pursuit_kick(pe);
            }
            _ => {}
        }
    }

    /// PursuitView's jolts for the player.
    fn pursuit_kick(&mut self, pe: &PursuitEvent) {
        match pe {
            PursuitEvent::Takedown {
                by_player: true, ..
            } => self.kicks.push((0.8, 0.7, 400.0)),
            PursuitEvent::Spiked { player: true, .. } => self.kicks.push((0.6, 0.8, 350.0)),
            PursuitEvent::Barrier { player: true, .. } => self.kicks.push((0.5, 0.6, 250.0)),
            PursuitEvent::Busted { player: true, .. } => self.kicks.push((0.9, 0.9, 700.0)),
            _ => {}
        }
    }

    /// `Race.jolt`: a hit, as a kick in the pad.
    fn jolt(&mut self, strength: f64) {
        if strength > 0.03 {
            self.kicks.push((
                0.25 + 0.75 * strength,
                0.5 + 0.5 * strength,
                120.0 + 280.0 * strength,
            ));
        }
    }

    /// `Race.rumble(speed, offroad)` and the nitro's kick, once a frame
    /// after the ticks: the nitro firing up (per tick, where the JS looked
    /// once a frame), then the steady buzz of a scrape, gravel, a skid, the
    /// nitro or spiked tyres. The client hands `feel` to the pads only
    /// while one is connected (`if (!pads?.state.connected) return`).
    fn rumble(&mut self) {
        for t in &self.audio_ticks {
            if t.nitro && !self.was_nitro {
                self.kicks.push((0.45, 0.6, 260.0));
            }
            self.was_nitro = t.nitro;
            if let Some(o) = t.state.offroad {
                self.offroad = o;
            }
        }
        let p = &self.session.curr.players[0];
        let ph = &p.phys;
        let speed = mr_math::kernel::hypot(p.v.vx, p.v.vz);
        let sp = mr_math::clamp(speed / 30.0, 0.0, 1.0);
        let spiked = if ph.spiked > 0.0 { 0.3 * sp } else { 0.0 };
        let max = mr_math::js::max_n;
        self.feel = Some((
            max(&[ph.scrape * 0.55, self.offroad * 0.35 * sp, spiked]),
            max(&[
                ph.scrape * 0.45,
                self.offroad * 0.45 * sp,
                ph.skid * 0.15,
                if ph.nitro_active { 0.15 } else { 0.0 },
                spiked,
            ]),
        ));
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
                "STUCK? TAP R TO RESET".to_string()
            } else {
                format!(
                    "STUCK? PRESS {} TO RESET",
                    self.pad_reset
                        .as_deref()
                        .map_or("R".into(), str::to_uppercase)
                )
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

    fn pursuit_race(level: &str) -> Race {
        let lr = LevelRuntime::new(mr_levels::level_by_id(level)).unwrap();
        Race::new(
            lr,
            Setup {
                opts: RaceOpts {
                    car: "sports",
                    seed: 1,
                    pursuit: true,
                    heat: 2.0,
                },
                autodrive: true,
                touch: false,
            },
            TouchControls::default(),
        )
    }

    /// `?cops=N` and the flash option reach the pursuit, and stay over a
    /// restart.
    #[test]
    fn pursuit_options_reach_the_pursuit() {
        let mut race = pursuit_race("sierra");
        assert_eq!(race.session.curr.pv.as_ref().unwrap().pursuit.max_units, 6);
        race.set_pursuit_opts(PursuitOpts {
            cops: 2.0,
            flash: false,
        });
        let pu = &race.session.curr.pv.as_ref().unwrap().pursuit;
        assert_eq!((pu.max_units, pu.flash), (2, false));
        let opts = race.setup.opts;
        race.restart(opts);
        let pu = &race.session.curr.pv.as_ref().unwrap().pursuit;
        assert_eq!((pu.max_units, pu.flash), (2, false));
    }

    /// A line the bridge says goes through the radio in the next frame,
    /// and is gone the frame after; a second, unforced line within the
    /// gap is not said.
    #[test]
    fn staged_lines_go_through_the_radio() {
        use mr_audio::radio::lines::Line;
        let mut race = pursuit_race("sierra");
        let line = |t: &str| Line {
            text: t.into(),
            parts: vec![t.into()],
        };
        race.stage_say.push((line("one"), true));
        race.stage_say.push((line("two"), false));
        race.frame(1.0 / 60.0);
        let said: Vec<_> = race.radio.said.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(said, ["one"]);
        race.frame(1.0 / 60.0);
        assert!(race.radio.said.is_empty());
    }

    /// A pursuit race, headless: the police give chase and the HUD says
    /// so (PURSUIT), as PursuitView.events shows it.
    #[test]
    fn a_pursuit_shows_on_the_hud() {
        let mut race = pursuit_race("desert");
        let mut centers = Vec::new();
        let mut toasts = Vec::new();
        for _ in 0..(120.0 * 60.0) as u32 {
            race.frame(1.0 / 60.0);
            for e in &race.log {
                if let SimEvent::Pursuit(_) = e {
                    if let Some(c) = &race.hud.center {
                        centers.push(c.clone());
                    }
                    if let Some(t) = &race.hud.toast {
                        toasts.push(t.clone());
                    }
                }
            }
            if centers.iter().any(|c| c == "PURSUIT") {
                break;
            }
        }
        assert!(
            centers.iter().any(|c| c == "PURSUIT"),
            "{centers:?} {toasts:?}"
        );
    }
}
