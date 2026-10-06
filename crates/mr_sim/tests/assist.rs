//! The steering assist's effect on a poor driver (DECISIONS D1082): scripted
//! "bad drivers" race with the assist off, Light and Strong, and the
//! time off the road and the race time are compared. The quick test runs
//! one driver for a minute of Sierra; the full table (Sierra and Coast, or
//! the levels in `ASSIST_LEVELS=sierra,coast,desert,streets`) is
//!
//!   cargo test --release -p mr_sim --test assist -- --ignored --nocapture

use mr_math::clamp;
use mr_sim::assist::{Assist, steer_assist};
use mr_sim::autopilot::autopilot;
use mr_sim::input::{Input, InputFrame, RESET};
use mr_sim::race::{LevelRuntime, RaceOpts, RaceStateKind, SimState, step};

/// How the scripted driver steers, given the autopilot's steering.
#[derive(Clone, Copy, Debug)]
enum Driver {
    /// The autopilot's own steering (a good driver, for reference).
    Good,
    /// Steers half as much as a corner needs.
    Lazy,
    /// The right steering plus a random ±0.7 held for a quarter second.
    Noisy,
    /// Steers 70 % of what is needed and pulls 0.2 to the right.
    Pulling,
}

#[derive(Default)]
struct Run {
    race_time: f64,
    finished: bool,
    off_road: f64,
    resets: u32,
}

struct Lcg(u32);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        f64::from(self.0 >> 8) / f64::from(1u32 << 24)
    }
}

fn run(level: &str, driver: Driver, assist: Assist, limit_s: f64) -> Run {
    let lr = LevelRuntime::new(mr_levels::level_by_id(level)).unwrap();
    let t = &*lr.track;
    let opts = RaceOpts {
        car: "sports",
        seed: 1,
        pursuit: false,
        heat: 1.0,
    };
    let mut st = SimState::new(&lr, opts);
    let mut ev = Vec::new();
    let mut rng = Lcg(7);
    let mut noise = 0.0;
    let mut out = Run::default();
    let dt = 1.0 / 120.0;
    let mut stuck_reset = false;
    while st.race.time < limit_s && st.race.state != RaceStateKind::Finished {
        if st.tick.is_multiple_of(30) {
            noise = (rng.next() * 2.0 - 1.0) * 0.7;
        }
        let p = &st.players[0];
        let mut inp = Input::default();
        autopilot(&mut inp, &p.v, t);
        inp.analog = true;
        inp.nitro = false;
        inp.steer = clamp(
            match driver {
                Driver::Good => inp.steer,
                Driver::Lazy => inp.steer * 0.5,
                Driver::Noisy => inp.steer + noise,
                Driver::Pulling => inp.steer * 0.7 + 0.2,
            },
            -1.0,
            1.0,
        );
        steer_assist(&mut inp, &p.v, &p.phys, t, assist);
        let mut f = InputFrame::quantise(&inp);
        // A player presses reset when stuck (the race offers it after 3 s).
        if p.rules.stuck.is_some_and(|s| s > 3.0) && !stuck_reset {
            f.flags |= RESET;
            out.resets += 1;
            stuck_reset = true;
        } else if p.rules.stuck.is_none_or(|s| s < 1.0) {
            stuck_reset = false;
        }
        step(&lr, &mut st, &[f], &mut ev);
        ev.clear();
        let v = &st.players[0].v;
        if st.race.state == RaceStateKind::Racing && v.lat.abs() > t.frame(v.s).hw {
            out.off_road += dt;
        }
        if st.players[0].rules.finished && !out.finished {
            out.finished = true;
            out.race_time = st.race.time;
            break;
        }
    }
    if !out.finished {
        out.race_time = st.race.time;
    }
    out
}

/// A lazy driver on Sierra's first minute: the assist cuts the time off
/// the road, Strong more than Light.
#[test]
fn the_assist_keeps_a_lazy_driver_on_the_road() {
    let off = run("sierra", Driver::Lazy, Assist::Off, 60.0);
    let light = run("sierra", Driver::Lazy, Assist::Light, 60.0);
    let strong = run("sierra", Driver::Lazy, Assist::Strong, 60.0);
    eprintln!(
        "off-road s: off {:.1}, light {:.1}, strong {:.1}",
        off.off_road, light.off_road, strong.off_road
    );
    assert!(off.off_road > 1.0, "{}", off.off_road);
    assert!(light.off_road < off.off_road * 0.7);
    assert!(strong.off_road <= light.off_road);
}

#[test]
#[ignore]
fn assist_table() {
    let levels: Vec<String> = std::env::var("ASSIST_LEVELS")
        .map_or(vec!["sierra".into(), "coast".into()], |v| {
            v.split(',').map(String::from).collect()
        });
    for level in levels.iter().map(String::as_str) {
        for driver in [Driver::Good, Driver::Lazy, Driver::Noisy, Driver::Pulling] {
            for assist in [Assist::Off, Assist::Light, Assist::Strong] {
                let r = run(level, driver, assist, 600.0);
                println!(
                    "{level:<7} {:<8} {:<7} race {:>7.2} s{} off-road {:>6.1} s ({:>4.1} %) resets {}",
                    format!("{driver:?}"),
                    assist.name(),
                    r.race_time,
                    if r.finished { " " } else { "+" },
                    r.off_road,
                    100.0 * r.off_road / r.race_time.max(1.0),
                    r.resets
                );
            }
        }
    }
}
