//! An engine and gearbox (SPEC 4.5), the first form of the drivetrain
//! graph: a torque curve with idle, a limiter and engine braking, the
//! flywheel's inertia reflected onto the driven wheels, an automatic
//! clutch that slips to launch, a gearbox with a shift time, and an open
//! differential (equal torque to each driven wheel, by the axles' shares).
//!
//! The rigid shafts are collapsed (SPEC 4.5): with the clutch closed, the
//! engine turns at the driven wheels' mean speed times the overall ratio,
//! and its inertia joins theirs. Below the launch speed the clutch slips
//! and passes the engine's torque at its slipping speed.

use core::f64::consts::PI;

/// rad/s to rpm.
const RPM: f64 = 60.0 / (2.0 * PI);

#[derive(Clone, Debug, PartialEq)]
pub struct EngineDef {
    /// Full-throttle torque at the crank over rpm, ascending (rpm, N·m).
    pub torque: Vec<(f64, f64)>,
    pub idle: f64,
    /// The rev limiter cuts the fuel here.
    pub limiter: f64,
    /// Engine and flywheel inertia (kg·m²).
    pub inertia: f64,
    /// Engine braking at the limiter with the throttle shut (N·m at the
    /// crank), proportional to rpm.
    pub braking: f64,
    /// Forward ratios, first gear first; the reverse ratio (positive); the
    /// final drive; the driveline's efficiency.
    pub ratios: Vec<f64>,
    pub reverse: f64,
    pub final_drive: f64,
    pub efficiency: f64,
    /// Seconds the clutch is open for a shift.
    pub shift_time: f64,
    /// The automatic gearbox shifts up past `up_rpm`, down under `down_rpm`.
    pub up_rpm: f64,
    pub down_rpm: f64,
    /// The automatic clutch slips the engine up to this speed at full
    /// throttle (idle at none) and closes once the wheels catch up.
    pub launch_rpm: f64,
}

impl EngineDef {
    /// Full-throttle torque at `rpm`, interpolated, flat past the table.
    pub fn torque_at(&self, rpm: f64) -> f64 {
        let t = &self.torque;
        if rpm <= t[0].0 {
            return t[0].1;
        }
        for k in 1..t.len() {
            if rpm <= t[k].0 {
                let (r0, t0) = t[k - 1];
                let (r1, t1) = t[k];
                return t0 + (t1 - t0) * (rpm - r0) / (r1 - r0);
            }
        }
        t[t.len() - 1].1
    }

    /// Overall ratio from crank to wheels in `gear` (negative in reverse,
    /// 0 in neutral).
    pub fn ratio(&self, gear: i32) -> f64 {
        if gear > 0 {
            self.ratios[(gear as usize - 1).min(self.ratios.len() - 1)] * self.final_drive
        } else if gear < 0 {
            -self.reverse * self.final_drive
        } else {
            0.0
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EngineState {
    /// −1 reverse, 0 neutral, 1.. forward.
    pub gear: i32,
    pub rpm: f64,
    /// Seconds left of a shift (the clutch is open).
    pub shift_timer: f64,
    /// The last tick's shift: +1 up, −1 down, 0 none (derived).
    pub shifted: i32,
    /// Traction control's share of the throttle it lets through, 0..1.
    pub tc: f64,
}

impl EngineState {
    pub fn new(def: &EngineDef) -> EngineState {
        EngineState {
            gear: 1,
            rpm: def.idle,
            shift_timer: 0.0,
            shifted: 0,
            tc: 1.0,
        }
    }

    pub fn hash_into(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.gear.to_le_bytes());
        for v in [self.rpm, self.shift_timer, self.tc] {
            out.extend_from_slice(&v.to_bits().to_le_bytes());
        }
    }
}

/// What the engine gives the driven wheels for one substep.
pub struct EngineOut {
    /// Total torque at the wheels (N·m), before the axles' shares.
    pub wheel_torque: f64,
    /// Inertia the closed clutch adds to the driven wheels, in all (kg·m²).
    pub wheel_inertia: f64,
}

/// One substep of the engine at the driven wheels' mean spin `omega`.
pub fn engine_step(def: &EngineDef, st: &mut EngineState, throttle: f64, omega: f64) -> EngineOut {
    let g = def.ratio(st.gear);
    let throttle = if throttle > 0.0 { throttle } else { 0.0 };
    if g == 0.0 || st.shift_timer > 0.0 {
        // Clutch open: the engine falls toward idle (or revs under throttle).
        let free = def.idle + throttle * (def.launch_rpm - def.idle);
        st.rpm += (free - st.rpm) * 0.05;
        return EngineOut {
            wheel_torque: 0.0,
            wheel_inertia: 0.0,
        };
    }
    let rpm_wheels = omega * g * RPM;
    let slip_rpm = def.idle + throttle * (def.launch_rpm - def.idle);
    if rpm_wheels >= slip_rpm {
        // Clutch closed.
        st.rpm = rpm_wheels;
        let drag = def.braking * rpm_wheels / def.limiter;
        let te = if rpm_wheels >= def.limiter {
            -drag
        } else {
            throttle * def.torque_at(rpm_wheels) - (1.0 - throttle) * drag
        };
        EngineOut {
            wheel_torque: te * g * def.efficiency,
            wheel_inertia: def.inertia * g * g,
        }
    } else {
        // Slipping: the engine holds its launch speed and the clutch passes
        // its torque there.
        st.rpm = slip_rpm;
        EngineOut {
            wheel_torque: throttle * def.torque_at(slip_rpm) * g * def.efficiency,
            wheel_inertia: 0.0,
        }
    }
}

/// Once a tick: the shift timer, a requested shift (`shift` +1 up, −1
/// down), and the automatic gearbox's choice when `auto` is on.
pub fn gearbox_tick(def: &EngineDef, st: &mut EngineState, shift: i32, auto: bool, dt: f64) {
    st.shift_timer = (st.shift_timer - dt).max(0.0);
    st.shifted = 0;
    let top = def.ratios.len() as i32;
    let mut want = st.gear;
    if shift > 0 && st.gear < top {
        want = st.gear + 1;
    } else if shift < 0 && st.gear > -1 {
        want = st.gear - 1;
    } else if auto && st.gear >= 1 && st.shift_timer == 0.0 {
        if st.rpm > def.up_rpm && st.gear < top {
            want = st.gear + 1;
        } else if st.rpm < def.down_rpm && st.gear > 1 {
            want = st.gear - 1;
        }
    }
    if want != st.gear {
        st.shifted = if want > st.gear { 1 } else { -1 };
        st.gear = want;
        st.shift_timer = def.shift_time;
    }
}
