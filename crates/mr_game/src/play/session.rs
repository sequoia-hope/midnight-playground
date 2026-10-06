//! The session and its tick loop (SPEC 3.3, 4.1; roadmap WP 4.2): the
//! loopback session of single-player. Each rendered frame it runs 0..6 fixed
//! ticks of `mr_sim::race::step` (1/120 s; at most six a frame, the JS's 1/20 s
//! clamp, beyond which the race slows down), keeps the state before the last
//! tick beside the current one, and says how far the frame is between them
//! (`alpha`), so the client draws an interpolation of the last two ticks
//! and reacts to the events those ticks produced. The client never reads
//! the state mid-tick and never writes it.
//!
//! The input layer runs at the tick rate: the session asks for one
//! quantised `InputFrame` per tick.
//!
//! It lives here until `mr_net` has its session types (M10), where the
//! loopback becomes one transport among the others (DECISIONS D430).

use mr_sim::input::InputFrame;
use mr_sim::race::{DT, LevelRuntime, RaceOpts, SimEvent, SimState, step};

/// At most six ticks a frame (1/20 s, SPEC 4.1).
pub const MAX_TICKS: u32 = 6;

/// The JS frame loop's clamp: `Math.min(frameDt, 1 / 20)`.
pub const MAX_FRAME: f64 = 1.0 / 20.0;

pub struct Session {
    pub lr: LevelRuntime,
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
}

impl Session {
    pub fn new(lr: LevelRuntime, opts: RaceOpts) -> Session {
        let curr = SimState::new(&lr, opts);
        Session {
            prev: curr.clone(),
            curr,
            lr,
            acc: 0.0,
            events: Vec::new(),
            ticks: 0,
            inputs: Vec::new(),
        }
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
        (self.acc / DT).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mr_sim::autopilot::autopilot;
    use mr_sim::input::Input;
    use mr_sim::race::hash;

    fn sierra() -> LevelRuntime {
        LevelRuntime::new(mr_levels::level_by_id("sierra")).unwrap()
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
}
