import { clamp, wrapAngle } from '../util/math.js';

// Simple autopilot for automated tests (?autodrive=1): follows the racing
// line at 93 % of the speed profile. A module of its own so the Rust port's
// reference runs (tools/parity/) drive with exactly this code; the port
// carries it into mr_sim as an input generator (SPEC 4.3).
// v: the player's Vehicle, t: the Track. Writes into and returns inp.
export function autopilot(inp, v, t) {
  const la = 10 + Math.hypot(v.vx, v.vz) * 0.35;
  const p = t.pointAt(v.s + la, t.racingLine[t.idx(v.s + la)] * 0.6);
  const want = Math.atan2(p.z - v.z, p.x - v.x);
  const err = wrapAngle(want - v.yaw);
  const target = t.speedProfile[t.idx(v.s + 15)] * 0.93;
  const sp = Math.hypot(v.vx, v.vz);
  inp.steer = clamp(err * 2.2, -1, 1);
  inp.throttle = sp < target ? 1 : 0;
  inp.brake = sp > target + 3 ? 1 : 0;
  inp.handbrake = false;
  inp.nitro = sp < target - 8 && Math.abs(err) < 0.1;
  return inp;
}
