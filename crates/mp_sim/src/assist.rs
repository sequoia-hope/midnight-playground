//! Driving aids the Rust build adds for touch screens (the owner's request
//! of 2026-10-05; the JS game has neither, DECISIONS D1080–D1083):
//!
//! - [`steer_assist`]: blends the player's steering toward the racing line
//!   when the car is heading off the road or is far off the line. The
//!   client applies it to the player's controls before it quantises them
//!   into the tick's `InputFrame`, so a recorded run carries the assisted
//!   steering and replays exactly; the race itself never calls it. Off, it
//!   is not called at all (and is the identity if it is).
//! - [`brake_urgency`]: the guide line's colour rule: for a point ahead
//!   with its target speed, how soon a car going this fast has to brake
//!   for it (green, orange, red).
//!
//! Both are pure functions of the state, through `mp_math`'s kernel only.

use mp_math::{clamp, js, kernel, lerp, smoothstep, wrap_angle};
use mp_track::Track;

use crate::input::Input;
use crate::physics::{ANALOG_LOCK, CarPhysics};
use crate::vehicle::Vehicle;

// ── The car's braking, as physics applies it ──────────────────────────

/// Full brake: physics' `a -= 15 * inp.brake` (moving forward over 0.8
/// m/s). The same for every car.
pub const BRAKE_DECEL: f64 = 15.0;
/// Physics' drag: `a -= 0.00115 * v * |v| + 0.01 * v`.
pub const DRAG_V2: f64 = 0.00115;
pub const DRAG_V: f64 = 0.01;

/// The deceleration of a car going `v` m/s forward on a flat road with
/// the brake fully on: the brake and the drag.
pub fn full_brake_decel(v: f64) -> f64 {
    BRAKE_DECEL + DRAG_V2 * v * v + DRAG_V * v
}

/// Metres a car needs to slow from `v0` to `v1` braking at `frac` of the
/// brake pedal (the drag always whole), on a flat road: ∫ u / a(u) du,
/// by Simpson's rule over eight steps (the integrand is smooth; exact to
/// well under a millimetre at racing speeds). 0 when `v0 <= v1`.
pub fn brake_distance(v0: f64, v1: f64, frac: f64) -> f64 {
    if v0 <= v1 {
        return 0.0;
    }
    let v1 = js::max(v1, 0.0);
    let f = |u: f64| u / (BRAKE_DECEL * frac + DRAG_V2 * u * u + DRAG_V * u);
    const N: usize = 8;
    let h = (v0 - v1) / N as f64;
    let mut sum = f(v1) + f(v0);
    for k in 1..N {
        let u = v1 + h * k as f64;
        sum += f(u) * if k % 2 == 1 { 4.0 } else { 2.0 };
    }
    sum * h / 3.0
}

// ── The guide line's colour ───────────────────────────────────────────

/// The share of the brakes the colour rule plans with: firm braking, with
/// a quarter in hand (the track's speed profile assumes 11 m/s², about
/// this much).
pub const FIRM: f64 = 0.75;
/// Seconds before the firm braking point where the line starts to turn
/// from green to orange, where it is fully orange, and where it starts to
/// turn red; it is fully red at the braking point and past it.
pub const WARN_T: f64 = 1.5;
pub const ORANGE_T: f64 = 0.75;
pub const RED_T: f64 = 0.25;

/// How soon a car going `v` m/s has to brake for a point `d` metres ahead
/// whose target speed is `vt` (the track's speed profile there):
///
/// t = (d − D) / v, where D = [`brake_distance`]`(v, vt, FIRM)` is how far
/// the car needs to slow to `vt` braking firmly; t is the time left before
/// it must start braking. The urgency is 0 (green) from t ≥ 1.5 s, rises
/// to 1 (orange) by t = 0.75 s, stays orange to 0.25 s, and reaches 2
/// (red) at t = 0: brake now. A point the car is already at or under the
/// speed for is green; so is everything when the car is (nearly) still.
pub fn brake_urgency(v: f64, vt: f64, d: f64) -> f64 {
    if v <= vt || v < 1.0 {
        return 0.0;
    }
    let t = (d - brake_distance(v, vt, FIRM)) / v;
    clamp((WARN_T - t) / (WARN_T - ORANGE_T), 0.0, 1.0) + clamp((RED_T - t) / RED_T, 0.0, 1.0)
}

// ── Steering assist ───────────────────────────────────────────────────

/// The settings' choice: off, light, strong.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Assist {
    #[default]
    Off,
    Light,
    Strong,
}

impl Assist {
    /// The most of the line's steering the blend takes, when the car is
    /// heading off the road and the player is not steering hard.
    pub fn strength(self) -> f64 {
        match self {
            Assist::Off => 0.0,
            Assist::Light => 0.5,
            Assist::Strong => 0.85,
        }
    }

    /// `0`, `1`, `2` (the `assist=` query) or `off`, `light`, `strong`
    /// (the store).
    pub fn parse(s: &str) -> Option<Assist> {
        match s {
            "0" | "off" => Some(Assist::Off),
            "1" | "light" => Some(Assist::Light),
            "2" | "strong" => Some(Assist::Strong),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Assist::Off => "off",
            Assist::Light => "light",
            Assist::Strong => "strong",
        }
    }
}

/// Seconds ahead the car's drift across the road is projected to judge
/// whether it is heading off.
const PREVIEW_T: f64 = 0.7;
/// How much of the blend a hard steer of the player's own takes away.
const DELIBERATE_YIELD: f64 = 0.8;

/// The steering lock physics allows for these controls at this speed
/// (`CarPhysics::step`'s `steerMax`).
fn steer_max(ph: &CarPhysics, inp: &Input, speed: f64) -> f64 {
    let mut m = lerp(0.6, 0.15, smoothstep(0.0, 65.0, speed));
    if inp.analog && !ph.drifting && !(inp.handbrake && speed > 9.0) {
        let grip = ph.spec.grip * if ph.spiked > 0.0 { 0.7 } else { 1.0 };
        let yaw_max = ANALOG_LOCK * grip * 1.5 / js::max(speed, 4.0);
        m = js::min(
            m,
            kernel::atan(yaw_max * ph.wheel_base / js::max(speed, 1.0)),
        );
    }
    m
}

/// The steering that follows the racing line from here: the turn that
/// arcs the car onto the line a speed-dependent distance ahead (the
/// circle through the car, tangent to its heading, and that point), as a
/// steering input for these controls.
pub fn line_steer(inp: &Input, v: &Vehicle, ph: &CarPhysics, t: &Track) -> f64 {
    let speed = kernel::hypot(v.vx, v.vz);
    let la = 8.0 + speed * 0.5;
    let s = v.s + la;
    let p = t.point_at(s, t.racing_line[t.idx(s)] as f64);
    let (dx, dz) = (p.x - v.x, p.z - v.z);
    let l = js::max(kernel::hypot(dx, dz), 1.0);
    let alpha = wrap_angle(kernel::atan2(dz, dx) - v.yaw);
    // Curvature 2 sin α / L, as a yaw rate, then the wheel angle for it.
    let yaw_rate = 2.0 * speed * kernel::sin(alpha) / l;
    let delta = kernel::atan(yaw_rate * ph.wheel_base / js::max(speed, 1.0));
    clamp(delta / steer_max(ph, inp, speed), -1.0, 1.0)
}

/// How much the car needs help, 0..1: heading off the road (its edge
/// within 2 m of the road's at the projected place, or already past it),
/// or far off the line (from 4 to 8 m off, counted half: in the middle of
/// a two-lane road with the line on one side the car is left alone).
pub fn assist_need(v: &Vehicle, t: &Track) -> f64 {
    let f = t.frame(v.s);
    let across = v.vx * f.rx + v.vz * f.rz;
    let lat_p = v.lat + across * PREVIEW_T;
    let room = |lat: f64| f.hw - lat.abs() - v.half_w;
    let edge = 1.0 - smoothstep(0.2, 2.0, js::min(room(lat_p), room(v.lat) + 0.8));
    let off = (v.lat - t.racing_line[t.idx(v.s)] as f64).abs();
    js::max(edge, 0.5 * smoothstep(4.0, 8.0, off))
}

/// The steering assist: `inp.steer` blended toward [`line_steer`] by the
/// assist's strength × [`assist_need`], less where the player steers hard
/// (from half to full lock, the blend loses up to four fifths). Nothing
/// else in `inp` changes; nothing happens off, slower than 4 m/s, in the
/// air, or facing more than about 70° off the road (spun, or going the
/// wrong way).
pub fn steer_assist(inp: &mut Input, v: &Vehicle, ph: &CarPhysics, t: &Track, a: Assist) {
    let k = a.strength();
    if k <= 0.0 {
        return;
    }
    let speed = kernel::hypot(v.vx, v.vz);
    if speed < 4.0 || !v.on_ground || ph.locked {
        return;
    }
    let f = t.frame(v.s);
    if wrap_angle(v.yaw - kernel::atan2(f.fz, f.fx)).abs() > 1.2 {
        return;
    }
    let need = assist_need(v, t);
    if need <= 0.0 {
        return;
    }
    let deliberate = smoothstep(0.5, 0.95, inp.steer.abs());
    let w = k * need * (1.0 - DELIBERATE_YIELD * deliberate);
    let want = line_steer(inp, v, ph, t);
    inp.steer = clamp(inp.steer + (want - inp.steer) * w, -1.0, 1.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::race::{LevelRuntime, PlayerCar};

    fn sierra() -> LevelRuntime {
        LevelRuntime::new(mp_levels::level_by_id("sierra")).unwrap()
    }

    /// A car on the road at (s, lat), pointing along it at `speed`, turned
    /// `yaw` off the road's heading.
    fn car_at(t: &Track, s: f64, lat: f64, speed: f64, yaw: f64) -> PlayerCar {
        let mut p = PlayerCar::new("sports");
        p.phys.reset(&mut p.v, t, s, lat);
        p.v.yaw += yaw;
        p.v.vx = kernel::cos(p.v.yaw) * speed;
        p.v.vz = kernel::sin(p.v.yaw) * speed;
        p
    }

    /// The constant is the brake physics applies: one tick of full brake on
    /// the road decelerates by it plus the drag and the slope.
    #[test]
    fn full_brake_is_physics_brake() {
        let lr = sierra();
        let t = &*lr.track;
        let mut p = car_at(t, 400.0, 0.0, 30.0, 0.0);
        let before = kernel::hypot(p.v.vx, p.v.vz);
        let inp = Input {
            brake: 1.0,
            ..Input::default()
        };
        let dt = 1.0 / 120.0;
        let f = t.frame(p.v.s);
        p.phys.step(&mut p.v, t, dt, &inp);
        let after = kernel::hypot(p.v.vx, p.v.vz);
        let grade = f.grade;
        let slope = 9.81 * grade / (1.0 + grade * grade).sqrt();
        let a = (before - after) / dt;
        assert!(
            (a - full_brake_decel(before) - slope).abs() < 0.05,
            "{a} against {}",
            full_brake_decel(before) + slope
        );
    }

    #[test]
    fn brake_distance_matches_constant_decel_and_grows_with_speed() {
        // Without drag it is v² / 2a; the drag only shortens it.
        let d = brake_distance(40.0, 10.0, 1.0);
        let no_drag = (40.0 * 40.0 - 10.0 * 10.0) / (2.0 * BRAKE_DECEL);
        assert!(d < no_drag && d > no_drag * 0.9, "{d} {no_drag}");
        assert_eq!(brake_distance(10.0, 20.0, 1.0), 0.0);
        assert!(brake_distance(50.0, 10.0, FIRM) > brake_distance(40.0, 10.0, FIRM));
        assert!(brake_distance(40.0, 10.0, FIRM) > d);
    }

    #[test]
    fn the_colour_rule() {
        // At or under the point's speed: green, however close.
        assert_eq!(brake_urgency(20.0, 20.0, 1.0), 0.0);
        assert_eq!(brake_urgency(15.0, 20.0, 0.0), 0.0);
        // 40 → 15 m/s braking firmly takes D metres.
        let (v, vt) = (40.0, 15.0);
        let d = brake_distance(v, vt, FIRM);
        let at = |t: f64| brake_urgency(v, vt, d + t * v);
        assert_eq!(at(3.0), 0.0, "comfortably far: green");
        assert_eq!(at(WARN_T), 0.0);
        assert!((at(1.125) - 0.5).abs() < 1e-9, "turning orange");
        assert_eq!(at(ORANGE_T), 1.0, "orange: brake soon");
        assert_eq!(at(0.5), 1.0);
        assert_eq!(at(RED_T), 1.0);
        assert!((at(0.125) - 1.5).abs() < 1e-9, "turning red");
        assert_eq!(at(0.0), 2.0, "red: brake now");
        assert_eq!(at(-1.0), 2.0, "red: too late to make it");
        // Monotonic in distance and in speed.
        let mut last = 2.0;
        for k in 0..200 {
            let u = brake_urgency(v, vt, k as f64);
            assert!(u <= last + 1e-12);
            last = u;
        }
        assert!(brake_urgency(45.0, vt, 80.0) >= brake_urgency(40.0, vt, 80.0));
    }

    #[test]
    fn assist_off_is_the_identity() {
        let lr = sierra();
        let t = &*lr.track;
        let p = car_at(t, 900.0, 3.0, 30.0, 0.3);
        for steer in [-1.0, -0.37, 0.0, 0.2, 1.0] {
            let mut inp = Input {
                steer,
                throttle: 0.6,
                analog: true,
                ..Input::default()
            };
            let before = inp;
            steer_assist(&mut inp, &p.v, &p.phys, t, Assist::Off);
            assert_eq!(inp, before);
        }
    }

    /// Heading for the right-hand edge with the stick at rest, the assist
    /// steers left, back toward the line, and Strong more than Light.
    #[test]
    fn assist_pulls_toward_the_line() {
        let lr = sierra();
        let t = &*lr.track;
        let s = 600.0;
        let hw = t.frame(s).hw;
        for side in [1.0, -1.0] {
            let p = car_at(t, s, side * (hw - 1.4), 30.0, side * 0.12);
            assert!(assist_need(&p.v, t) > 0.9, "{}", assist_need(&p.v, t));
            let run = |a| {
                let mut inp = Input {
                    analog: true,
                    ..Input::default()
                };
                steer_assist(&mut inp, &p.v, &p.phys, t, a);
                inp.steer
            };
            let (l, st) = (run(Assist::Light), run(Assist::Strong));
            assert!(l * side < -0.1, "{side}: light {l}");
            assert!(st * side < l * side, "{side}: strong {st} light {l}");
            // The input is otherwise untouched.
            let mut inp = Input {
                throttle: 0.7,
                brake: 0.1,
                nitro: true,
                analog: true,
                ..Input::default()
            };
            steer_assist(&mut inp, &p.v, &p.phys, t, Assist::Strong);
            assert_eq!(
                (inp.throttle, inp.brake, inp.nitro, inp.analog),
                (0.7, 0.1, true, true)
            );
        }
    }

    /// On the line, pointing along the road: nothing to do.
    #[test]
    fn on_the_line_the_assist_leaves_the_steering() {
        let lr = sierra();
        let t = &*lr.track;
        let s = 600.0;
        let p = car_at(t, s, t.racing_line[t.idx(s)] as f64, 25.0, 0.0);
        if assist_need(&p.v, t) == 0.0 {
            let mut inp = Input {
                steer: 0.3,
                analog: true,
                ..Input::default()
            };
            steer_assist(&mut inp, &p.v, &p.phys, t, Assist::Strong);
            assert_eq!(inp.steer, 0.3);
        }
    }

    /// A hard steer of the player's own wins over the assist: the blend
    /// moves full lock much less than it moves a resting stick.
    #[test]
    fn deliberate_input_wins() {
        let lr = sierra();
        let t = &*lr.track;
        let s = 600.0;
        let hw = t.frame(s).hw;
        let p = car_at(t, s, hw - 1.4, 30.0, 0.12);
        let out = |steer: f64| {
            let mut inp = Input {
                steer,
                analog: true,
                ..Input::default()
            };
            steer_assist(&mut inp, &p.v, &p.phys, t, Assist::Strong);
            inp.steer
        };
        let want = line_steer(
            &Input {
                analog: true,
                ..Input::default()
            },
            &p.v,
            &p.phys,
            t,
        );
        // Full lock to the right (toward the edge) stays mostly the
        // player's: the share moved is at most a fifth of Strong's.
        let moved_full = (out(1.0) - 1.0) / (want - 1.0);
        let moved_rest = out(0.0) / want;
        assert!(moved_full <= 0.2 * 0.85 + 1e-9, "{moved_full}");
        assert!(moved_rest > 0.6, "{moved_rest}");
        assert!(out(1.0) > 0.6, "{}", out(1.0));
    }

    /// Spun round, crawling, or in the air: the assist keeps out.
    #[test]
    fn no_assist_spun_slow_or_airborne() {
        let lr = sierra();
        let t = &*lr.track;
        let s = 600.0;
        let hw = t.frame(s).hw;
        let base = Input {
            steer: 0.1,
            analog: true,
            ..Input::default()
        };
        let check = |p: &PlayerCar| {
            let mut inp = base;
            steer_assist(&mut inp, &p.v, &p.phys, t, Assist::Strong);
            assert_eq!(inp, base);
        };
        check(&car_at(t, s, hw - 1.4, 30.0, 2.0));
        check(&car_at(t, s, hw - 1.4, 3.0, 0.1));
        let mut p = car_at(t, s, hw - 1.4, 30.0, 0.1);
        p.v.on_ground = false;
        check(&p);
    }
}
