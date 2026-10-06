//! The session and its tick loop (SPEC 3.3, 4.1; roadmap WP 4.2): the
//! loopback session of single-player. Each rendered frame it runs 0..6 fixed
//! ticks of `mp_sim::race::step` (1/120 s; at most six a frame, the JS's 1/20 s
//! clamp, beyond which the race slows down), keeps the state before the last
//! tick beside the current one, and says how far the frame is between them
//! (`alpha`), so the client draws an interpolation of the last two ticks
//! and reacts to the events those ticks produced. The client never reads
//! the state mid-tick and never writes it.
//!
//! The input layer runs at the tick rate: the session asks for one
//! quantised `InputFrame` per tick.
//!
//! Online (multiplayer, `crate::net`), an `mp_net` client steps the race
//! instead: [`Session::advance_online`] lets it run, then copies its
//! current and previous states and events here, so everything that reads
//! the session reads it the same way. `me` is the local player's index in
//! the race (0 offline).

use std::sync::Arc;

use mp_net::client::Client;
use mp_net::transport::Transport;
use mp_sim::input::InputFrame;
use mp_sim::race::{DT, LevelRuntime, RaceOpts, SimEvent, SimState, step};

/// At most six ticks a frame (1/20 s, SPEC 4.1).
pub const MAX_TICKS: u32 = 6;

/// The JS frame loop's clamp: `Math.min(frameDt, 1 / 20)`.
pub const MAX_FRAME: f64 = 1.0 / 20.0;

pub struct Session {
    pub lr: Arc<LevelRuntime>,
    /// The state before the last tick.
    pub prev: SimState,
    /// The state after the last tick.
    pub curr: SimState,
    /// Time not yet stepped, s (under one tick after `advance`, but for
    /// rounding).
    acc: f64,
    /// The events of the ticks the last `advance` ran, in order.
    pub events: Vec<SimEvent>,
    /// The ticks the last `advance` ran.
    pub ticks: u32,
    /// The inputs of the ticks the last `advance` ran, in order (for the
    /// run recording, `crate::recording`).
    pub inputs: Vec<InputFrame>,
    /// The local player's index in the race (0 offline).
    pub me: usize,
    /// Online: the drawing's alpha, from the network client (it, not the
    /// frame time, decides the ticks).
    pub online: Option<f64>,
}

impl Session {
    pub fn new(lr: LevelRuntime, opts: RaceOpts) -> Session {
        let curr = SimState::new(&lr, opts);
        Session {
            prev: curr.clone(),
            curr,
            lr: Arc::new(lr),
            acc: 0.0,
            events: Vec::new(),
            ticks: 0,
            inputs: Vec::new(),
            me: 0,
            online: None,
        }
    }

    /// A multiplayer race: its first state from the network client, and
    /// this player's index in it.
    pub fn online(lr: Arc<LevelRuntime>, first: SimState, me: usize) -> Session {
        Session {
            prev: first.clone(),
            curr: first,
            lr,
            acc: 0.0,
            events: Vec::new(),
            ticks: 0,
            inputs: Vec::new(),
            me,
            online: Some(0.0),
        }
    }

    /// Online: lets the network client step to where it should be (asking
    /// `input` for this player's controls once per new tick), then takes its
    /// states and events. `observe` hears the frame once, with the newest
    /// state. Returns the new ticks.
    pub fn advance_online<T: Transport>(
        &mut self,
        client: &mut Client<T>,
        now: f64,
        mut input: impl FnMut(&SimState) -> InputFrame,
        mut observe: impl FnMut(&LevelRuntime, &SimState, &[SimEvent], &InputFrame),
    ) -> u32 {
        self.events.clear();
        self.inputs.clear();
        let mut n = 0;
        let mut last = None;
        client.update(now, |st, _me| {
            let f = input(st);
            last = Some(f);
            n += 1;
            f
        });
        if let Some(r) = &mut client.race {
            self.prev.clone_from(r.prev());
            self.curr.clone_from(r.state());
            self.events.append(&mut r.events);
        }
        self.online = Some(client.alpha());
        if let Some(f) = last {
            self.inputs.push(f);
        }
        if last.is_some() || !self.events.is_empty() {
            observe(
                &self.lr,
                &self.curr,
                &self.events,
                &last.unwrap_or_default(),
            );
        }
        self.ticks = n;
        n
    }

    /// Advances by a frame's time: as many whole ticks as have come due (at
    /// most [`MAX_TICKS`]), asking `input` for each tick's controls just
    /// before it runs. Returns the ticks run.
    pub fn advance(&mut self, frame_dt: f64, input: impl FnMut(&SimState) -> InputFrame) -> u32 {
        self.advance_observed(frame_dt, input, |_, _, _, _| {})
    }

    /// [`Session::advance`], with `observe` called after each tick with the
    /// state it left, that tick's events and its input (the audio's
    /// per-tick calls, `Race.update`'s tail).
    pub fn advance_observed(
        &mut self,
        frame_dt: f64,
        mut input: impl FnMut(&SimState) -> InputFrame,
        mut observe: impl FnMut(&LevelRuntime, &SimState, &[SimEvent], &InputFrame),
    ) -> u32 {
        self.events.clear();
        self.inputs.clear();
        self.acc += frame_dt.clamp(0.0, MAX_FRAME);
        let mut n = 0;
        // A hair of tolerance, so 1/20 s is six ticks whatever the rounding.
        while self.acc >= DT - 1e-9 && n < MAX_TICKS {
            let frame = input(&self.curr);
            self.prev.clone_from(&self.curr);
            let from = self.events.len();
            step(&self.lr, &mut self.curr, &[frame], &mut self.events);
            observe(&self.lr, &self.curr, &self.events[from..], &frame);
            self.inputs.push(frame);
            self.acc -= DT;
            n += 1;
        }
        self.ticks = n;
        n
    }

    /// How far the frame is from the previous tick (0) to the current (1).
    pub fn alpha(&self) -> f64 {
        match self.online {
            Some(a) => a,
            None => (self.acc / DT).clamp(0.0, 1.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp_sim::autopilot::autopilot;
    use mp_sim::input::Input;
    use mp_sim::race::hash;

    fn sierra() -> LevelRuntime {
        LevelRuntime::new(mp_levels::level_by_id("sierra")).unwrap()
    }

    const OPTS: RaceOpts = RaceOpts {
        car: "sports",
        seed: 1,
        pursuit: false,
        heat: 1.0,
    };

    fn auto(st: &SimState, lr: &LevelRuntime) -> InputFrame {
        let mut inp = Input::default();
        autopilot(&mut inp, &st.players[0].v, &lr.track);
        InputFrame::quantise(&inp)
    }

    /// Ragged frame times (60, 144 and 30 Hz, a stall) step exactly the
    /// ticks that came due, with one input each, so the session's state is
    /// the simulation's own, tick for tick: no drift.
    #[test]
    fn frames_of_any_length_step_the_same_race() {
        let mut s = Session::new(sierra(), OPTS);
        let lr = sierra();
        let mut direct = SimState::new(&lr, OPTS);
        let mut ev = Vec::new();
        let frames: [f64; 6] = [1.0 / 60.0, 1.0 / 144.0, 1.0 / 30.0, 0.0, 0.25, 1.0 / 120.0];
        let mut total = 0.0;
        for i in 0..900 {
            let dt = frames[i % frames.len()];
            total += dt.min(MAX_FRAME);
            let n = s.advance(dt, |st| auto(st, &lr));
            assert!(n <= MAX_TICKS);
            assert!(s.alpha() >= 0.0 && s.alpha() <= 1.0);
            for _ in 0..n {
                let f = auto(&direct, &lr);
                step(&lr, &mut direct, &[f], &mut ev);
            }
            assert_eq!(s.curr.tick, direct.tick);
            if n > 0 {
                assert_eq!(s.prev.tick + 1, s.curr.tick);
            }
        }
        assert_eq!(hash(&s.curr), hash(&direct));
        // Every tick that came due ran (the clamp is per frame).
        assert!((f64::from(s.curr.tick) * DT - total).abs() < DT + 1e-9);
    }

    /// A long frame runs six ticks and drops the rest (the race slows down).
    #[test]
    fn a_frame_runs_at_most_six_ticks() {
        let mut s = Session::new(sierra(), OPTS);
        assert_eq!(s.advance(1.0, |_| InputFrame::default()), 6);
        assert!(s.alpha() < 1.0);
        assert_eq!(s.advance(0.0, |_| InputFrame::default()), 0);
    }

    /// Offline, alpha is the part of a tick the frames have not stepped.
    #[test]
    fn alpha_is_the_unstepped_part_of_a_tick() {
        let mut s = Session::new(sierra(), OPTS);
        assert_eq!(s.alpha(), 0.0);
        assert_eq!(s.advance(DT / 2.0, |_| InputFrame::default()), 0);
        assert!((s.alpha() - 0.5).abs() < 1e-9);
        assert_eq!(s.advance(DT, |_| InputFrame::default()), 1);
        assert!((s.alpha() - 0.5).abs() < 1e-9);
        // A negative frame time (a clock that went back) steps nothing.
        assert_eq!(s.advance(-1.0, |_| InputFrame::default()), 0);
        assert!((s.alpha() - 0.5).abs() < 1e-9);
        assert_eq!(s.prev.tick + 1, s.curr.tick);
    }

    /// Online the network client decides the ticks: the session copies its
    /// states and events, asks for one input per new tick, hears the frame
    /// once, and draws at the client's alpha.
    #[test]
    fn online_the_client_steps_and_the_session_copies() {
        use crate::net::tests::Lan;
        use mp_net::proto::{AiFill, Settings};
        let mut lan = Lan::new(&["Ann", "Bob"]);
        let mut races = lan.start(
            Settings {
                ai: AiFill::None,
                ..Settings::default()
            },
            false,
        );
        let first: Vec<u32> = races.iter().map(|r| r.session.curr.tick).collect();
        for r in &races {
            assert_eq!(r.session.alpha(), 0.0, "before the first frame");
            assert!(r.online_now());
        }
        let mut asked = [0u32; 2];
        let mut ran = [0u32; 2];
        let mut events = [0usize; 2];
        for _ in 0..(5 * 60) {
            lan.host_frame();
            let now = lan.now;
            for (i, (c, r)) in lan.clients.iter_mut().zip(races.iter_mut()).enumerate() {
                let s = &mut r.session;
                let mut heard = 0;
                let mut seen = None;
                let n = s.advance_online(
                    c,
                    now,
                    |_| {
                        asked[i] += 1;
                        InputFrame::default()
                    },
                    |_, st, ev, _| {
                        heard += 1;
                        seen = Some(st.tick);
                        events[i] += ev.len();
                    },
                );
                ran[i] += n;
                assert_eq!(s.ticks, n);
                assert_eq!(s.inputs.len(), usize::from(n > 0));
                assert!(heard <= 1, "the frame is heard once");
                assert_eq!(heard == 1, n > 0 || !s.events.is_empty());
                if let Some(t) = seen {
                    assert_eq!(t, s.curr.tick, "it hears the newest state");
                }
                let cr = c.race.as_ref().unwrap();
                assert_eq!(hash(&s.curr), hash(cr.state()));
                assert_eq!(hash(&s.prev), hash(cr.prev()));
                assert!(cr.events.is_empty(), "the session took the events");
                assert_eq!(s.alpha(), c.alpha());
                assert!((0.0..=1.0).contains(&s.alpha()));
            }
        }
        for i in 0..2 {
            assert!(ran[i] > 300, "five seconds of race ran: {}", ran[i]);
            assert_eq!(asked[i], ran[i], "one input per new tick");
            assert_eq!(races[i].session.curr.tick - first[i], ran[i]);
            assert!(events[i] > 0, "the countdown's events came through");
        }
    }
}
