//! Simple autopilot (`?autodrive=1`): follows the racing line at 93 % of the
//! speed profile (port of `src/game/autopilot.js`, SPEC 4.3 "Autodrive").
//! Tests, the reference runs, the headless runner and RL baselines drive
//! with it.

use mp_math::{clamp, kernel, wrap_angle};
use mp_track::Track;

use crate::input::Input;
use crate::vehicle::Vehicle;

/// Writes the autopilot's controls into `inp` (steer, throttle, brake,
/// handbrake, nitro); the other fields are left as they are.
pub fn autopilot(inp: &mut Input, v: &Vehicle, t: &Track) {
    let la = 10.0 + kernel::hypot(v.vx, v.vz) * 0.35;
    let p = t.point_at(v.s + la, t.racing_line[t.idx(v.s + la)] as f64 * 0.6);
    let want = kernel::atan2(p.z - v.z, p.x - v.x);
    let err = wrap_angle(want - v.yaw);
    let target = t.speed_profile[t.idx(v.s + 15.0)] as f64 * 0.93;
    let sp = kernel::hypot(v.vx, v.vz);
    inp.steer = clamp(err * 2.2, -1.0, 1.0);
    inp.throttle = if sp < target { 1.0 } else { 0.0 };
    inp.brake = if sp > target + 3.0 { 1.0 } else { 0.0 };
    inp.handbrake = false;
    inp.nitro = sp < target - 8.0 && err.abs() < 0.1;
}
