//! Arcade driving model for the player car (port of
//! `src/vehicles/CarPhysics.js`).
//!
//! Heading and velocity are separate: steering turns the heading (bicycle
//! model, capped by tyre grip), and tyre grip then pulls the velocity round
//! to match. Normal grip is strong enough that the car goes where it points;
//! the handbrake drops rear grip so the heading outruns the velocity and the
//! car drifts. A drift-angle assist keeps slides catchable on a keyboard.
//!
//! The car is always kept inside the road corridor (track.wallL/wallR), which
//! the scenery dresses as rock faces, guardrails, fences or barriers.
//!
//! The JS object holds its vehicle, track and spec; here the state is the
//! struct and the vehicle and track are passed to each call.

use core::f64::consts::PI;

use mr_math::{clamp, damp, js, kernel, lerp, smoothstep, wrap_angle};
use mr_track::Track;

use crate::input::Input;
use crate::vehicle::Vehicle;

/// power: m²/s³ per kg (accel = power / speed); launch: the traction limit on
/// that accel (m/s²); vmax: a speed limiter (m/s). electric: single-speed
/// motor, regenerative braking charges the boost tank. turbo: boost builds
/// with revs and throttle (lag), and scales the power. gearScale shortens
/// (< 1) or lengthens the gear ratios.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CarSpec {
    pub label: &'static str,
    pub blurb: &'static str,
    pub power: f64,
    pub launch: Option<f64>,
    pub vmax: Option<f64>,
    pub gear_scale: Option<f64>,
    pub turbo: bool,
    pub electric: bool,
    pub grip: f64,
    pub drift_grip: f64,
    pub mass: f64,
    pub color: u32,
}

const fn spec(
    label: &'static str,
    blurb: &'static str,
    power: f64,
    grip: f64,
    drift_grip: f64,
    mass: f64,
    color: u32,
) -> CarSpec {
    CarSpec {
        label,
        blurb,
        power,
        launch: None,
        vmax: None,
        gear_scale: None,
        turbo: false,
        electric: false,
        grip,
        drift_grip,
        mass,
        color,
    }
}

/// `CAR_SPECS`, in the JS order.
pub const CAR_SPECS: [(&str, CarSpec); 5] = [
    (
        "sports",
        spec(
            "Vento GT",
            "Balanced all-rounder",
            480.0,
            13.2,
            8.5,
            1350.0,
            0xd81e36,
        ),
    ),
    (
        "muscle",
        spec(
            "Brawler 69",
            "Big power, loose tail",
            545.0,
            12.0,
            6.8,
            1600.0,
            0x1f4fd8,
        ),
    ),
    (
        "super",
        spec(
            "Stiletto R",
            "Grip and precision",
            515.0,
            14.4,
            9.6,
            1300.0,
            0xf2b705,
        ),
    ),
    (
        "rally",
        CarSpec {
            launch: Some(10.8),
            vmax: Some(71.0),
            gear_scale: Some(0.84),
            turbo: true,
            ..spec(
                "Kestrel RS",
                "Turbo 4WD: launches hard, slides on demand",
                470.0,
                14.0,
                10.6,
                1250.0,
                0xee5a12,
            )
        },
    ),
    (
        "electric",
        CarSpec {
            launch: Some(11.8),
            vmax: Some(71.0),
            electric: true,
            ..spec(
                "Ion Arc",
                "Electric: instant torque, braking charges boost",
                500.0,
                13.6,
                8.8,
                1850.0,
                0xdfe7ee,
            )
        },
    ),
];

pub fn car_spec(kind: &str) -> Option<CarSpec> {
    CAR_SPECS.iter().find(|(k, _)| *k == kind).map(|(_, s)| *s)
}

const G: f64 = 9.81;
const GEAR_TOP: [f64; 7] = [0.0, 15.0, 26.0, 38.0, 51.0, 64.0, 84.0];
const IDLE: f64 = 900.0;
const REDLINE: f64 = 7800.0;
/// Analogue steering: full travel asks for this much of the grip-limited turn.
pub const ANALOG_LOCK: f64 = 1.25;
/// The electric motor: rpm at the speed limiter (single reduction gear).
pub const MOTOR_MAX: f64 = 16000.0;

/// What physics reports to the rest of the game (`phys.events`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PhysEvent {
    Shift {
        up: bool,
    },
    /// A landing hard enough to report (impact over 2.5 m/s).
    Land {
        strength: f64,
        air: f64,
    },
    /// A wall hit at more than 1.5 m/s into it.
    Impact {
        strength: f64,
        x: f64,
        y: f64,
        z: f64,
        side: i32,
    },
    /// Every landing: the kick the JS gives the body's pitch spring
    /// (`v.pitchV -= impact * 0.02`), for the client's springs.
    Touchdown {
        impact: f64,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct CarPhysics {
    pub spec: CarSpec,
    /// The spec on shredded tyres, made at the first spiked step.
    pub spiked_spec: Option<CarSpec>,
    pub gear: i32,
    pub rpm: f64,
    pub shift_timer: f64,
    pub nitro: f64,
    pub nitro_active: bool,
    pub drifting: bool,
    pub drift_time: f64,
    /// Slip angle (rad).
    pub slip: f64,
    /// 0..1 for audio/fx.
    pub skid: f64,
    /// 0..1 wall contact.
    pub scrape: f64,
    pub air_time: f64,
    pub events: Vec<PhysEvent>,
    /// During countdown.
    pub locked: bool,
    pub wheel_base: f64,
    pub electric: bool,
    pub gear_top: [f64; 7],
    /// Turbo boost 0..1.
    pub boost: f64,
    /// Electric: drive power (+) or regen (−), kW.
    pub power_out: f64,
    /// Electric: 0..1 regen braking strength.
    pub regen: f64,
    /// Hot Pursuit. damage 0..1: past half, the engine loses power (a wreck
    /// at 1 is the race's call). spiked: seconds left on shredded tyres
    /// after a spike strip: less grip and a lower top speed. Both are 0
    /// outside Hot Pursuit, which leaves the car exactly as it was.
    pub damage: f64,
    pub spiked: f64,
    /// Missing until the first step on the ground.
    pub off_track: Option<f64>,
    /// Missing until the first scrape.
    pub scrape_side: Option<i32>,
    pub on_ground: bool,
}

impl CarPhysics {
    pub fn new(vehicle: &Vehicle, spec: CarSpec) -> CarPhysics {
        let electric = spec.electric;
        let scale = spec.gear_scale.unwrap_or(1.0);
        CarPhysics {
            spec,
            spiked_spec: None,
            gear: 1,
            rpm: if electric { 0.0 } else { IDLE },
            shift_timer: 0.0,
            nitro: 0.5,
            nitro_active: false,
            drifting: false,
            drift_time: 0.0,
            slip: 0.0,
            skid: 0.0,
            scrape: 0.0,
            air_time: 0.0,
            events: Vec::new(),
            locked: false,
            wheel_base: js::or(vehicle.dims.wheel_base, 2.6),
            electric,
            gear_top: GEAR_TOP.map(|v| v * scale),
            boost: 0.0,
            power_out: 0.0,
            regen: 0.0,
            damage: 0.0,
            spiked: 0.0,
            off_track: None,
            scrape_side: None,
            on_ground: false,
        }
    }

    pub fn reset(&mut self, v: &mut Vehicle, track: &Track, s: f64, lat: f64) {
        v.place(track, s, lat, 0.0);
        self.gear = 1;
        self.rpm = if self.electric { 0.0 } else { IDLE };
        self.drifting = false;
        self.on_ground = true;
        v.on_ground = true;
    }

    /// Equal steps of about 1/120 s that add up to exactly the frame's time. A
    /// fixed step with a carried-over remainder took 1, 2 or 3 steps per 60 Hz
    /// frame (and 0 or 1 at 144 Hz) with nothing drawn in between, so at speed
    /// the car lurched against the camera, the traffic and the rivals, which all
    /// move by the frame's own time. A step may run 10 % long, so a 60 Hz frame
    /// a hair over 1/60 s is still two steps, not three.
    pub fn update(&mut self, v: &mut Vehicle, t: &Track, frame_dt: f64, inp: &Input) {
        let h = 1.0 / 120.0;
        // `!(frameDt > 0)`: NaN steps nothing, as in the JS.
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(frame_dt > 0.0) {
            return;
        }
        let n = js::min(12.0, (frame_dt / (h * 1.1)).ceil());
        let hh = js::min(h * 1.1, frame_dt / n);
        for _ in 0..n as usize {
            self.step(v, t, hh, inp);
        }
    }

    pub fn engine_accel(&self, v: f64) -> f64 {
        let spec = &self.spec;
        let mut a = js::min(spec.launch.unwrap_or(9.5), spec.power / js::max(v, 5.0));
        if spec.turbo {
            a *= 0.8 + 0.2 * self.boost;
        }
        if let Some(vmax) = spec.vmax.filter(|&m| m != 0.0) {
            a *= 1.0 - smoothstep(vmax - 5.0, vmax, v);
        }
        if self.damage > 0.5 {
            a *= 1.0 - 0.35 * smoothstep(0.5, 1.0, self.damage);
        }
        if self.spiked > 0.0 {
            let vm = spec.vmax.unwrap_or(76.0) * 0.75;
            a *= 1.0 - smoothstep(vm - 5.0, vm, v);
        }
        a
    }

    pub fn step(&mut self, v: &mut Vehicle, t: &Track, dt: f64, inp: &Input) {
        let mut spec = self.spec;
        if self.spiked > 0.0 {
            self.spiked = js::max(0.0, self.spiked - dt);
            let base = self.spec;
            spec = *self.spiked_spec.get_or_insert(CarSpec {
                grip: base.grip * 0.7,
                drift_grip: base.drift_grip * 0.7,
                ..base
            });
        }
        let p = t.project(v.x, v.z, v.s);
        v.s = p.s;
        v.lat = p.lat;
        let f = t.frame(v.s);

        let mut fx = kernel::cos(v.yaw);
        let mut fz = kernel::sin(v.yaw);
        let mut rx = -fz;
        let mut rz = fx;
        let mut v_long = v.vx * fx + v.vz * fz;
        let mut v_lat = v.vx * rx + v.vz * rz;
        let speed = kernel::hypot(v.vx, v.vz);

        // Steering: less lock at speed. Analogue steering (a thumb stick, tilt,
        // a gamepad) is scaled to the grip instead: full travel asks for a
        // quarter more turn than the tyres hold at this speed, so the whole
        // stick steers. With a fixed lock, past about 100 km/h the first fifth
        // of a phone's stick (a centimetre of thumb) already turned as hard as
        // the tyres allow, and the rest did nothing. Not in a slide, which
        // needs the lock to catch.
        let mut steer_max = lerp(0.6, 0.15, smoothstep(0.0, 65.0, speed));
        if inp.analog && !self.drifting && !(inp.handbrake && speed > 9.0) {
            let yaw_max = ANALOG_LOCK * spec.grip * 1.5 / js::max(speed, 4.0);
            steer_max = js::min(
                steer_max,
                kernel::atan(yaw_max * self.wheel_base / js::max(speed, 1.0)),
            );
        }
        let target = inp.steer * steer_max;
        v.steer_angle += clamp(target - v.steer_angle, -3.2 * dt, 3.2 * dt);

        // Gearbox (automatic). Mostly for sound and the shift cut. inp.cruise
        // short-shifts and holds taller gears, like an automatic at part throttle.
        self.shift_timer = js::max(0.0, self.shift_timer - dt);
        let up_rpm = if inp.cruise { 3800.0 } else { 7350.0 };
        let down_k = if inp.cruise { 0.45 } else { 0.7 };
        let gt = self.gear_top;
        if v_long < -0.5 && inp.brake > 0.0 {
            self.gear = -1;
        } else if self.gear == -1 && v_long > -0.2 {
            self.gear = 1;
        }
        if self.electric {
            // One fixed reduction: motor speed follows the wheels, no shifts.
            if self.gear != -1 {
                self.gear = 1;
            }
            let want = if self.locked {
                0.0
            } else {
                js::min(
                    MOTOR_MAX * 1.02,
                    (v_long.abs() / spec.vmax.unwrap_or(f64::NAN)) * MOTOR_MAX,
                )
            };
            self.rpm = damp(self.rpm, want, 30.0, dt);
        } else if self.gear >= 1 {
            let g = self.gear as usize;
            let ratio = v_long.abs() / gt[g];
            let mut rpm = IDLE + ratio * (REDLINE - IDLE);
            rpm = js::max(
                rpm,
                IDLE + inp.throttle * 3800.0 * (1.0 - smoothstep(4.0, 14.0, speed)),
            );
            if self.locked {
                rpm = IDLE + inp.throttle * 6200.0;
            }
            self.rpm = damp(self.rpm, js::min(rpm, REDLINE + 150.0), 18.0, dt);
            if !self.locked
                && self.rpm > up_rpm
                && self.gear < 6
                && self.shift_timer == 0.0
                && v_long.abs() > gt[g] * (down_k + 0.08)
            {
                self.gear += 1;
                self.shift_timer = 0.2;
                self.events.push(PhysEvent::Shift { up: true });
            } else if self.gear > 1 && v_long.abs() < gt[g - 1] * down_k && self.shift_timer == 0.0
            {
                self.gear -= 1;
                self.shift_timer = 0.12;
                self.events.push(PhysEvent::Shift { up: false });
            }
        } else {
            self.rpm = damp(self.rpm, IDLE + v_long.abs() / 12.0 * 4000.0, 10.0, dt);
        }

        // Turbo: boost follows revs under throttle, with lag; lifting dumps it
        // (the audio plays the blow-off from the drop).
        if spec.turbo {
            let want = inp.throttle
                * smoothstep(2600.0, 5200.0, self.rpm)
                * (if self.shift_timer > 0.08 { 0.3 } else { 1.0 });
            self.boost = damp(
                self.boost,
                want,
                if want > self.boost { 2.2 } else { 9.0 },
                dt,
            );
        }

        if self.locked {
            v.vx = 0.0;
            v.vz = 0.0;
            v.yaw_rate = 0.0;
            self.nitro_active = false;
            self.skid = 0.0;
            self.power_out = 0.0;
            return;
        }

        let on_ground = v.on_ground;
        self.nitro_active = false;
        if on_ground {
            // ── Longitudinal ─────────────────────────────────────────
            let mut a = 0.0;
            let throttle = if self.shift_timer > 0.08 {
                inp.throttle * 0.2
            } else {
                inp.throttle
            };
            if v_long > -0.5 {
                if inp.brake > 0.0 && v_long > 0.8 {
                    a -= 15.0 * inp.brake;
                } else if inp.brake > 0.0 && v_long > -12.0 {
                    a -= 7.0 * inp.brake;
                }
                a += throttle * self.engine_accel(v_long.abs());
            } else if inp.throttle > 0.0 {
                a += 14.0 * inp.throttle;
            } else if inp.brake > 0.0 && v_long > -13.0 {
                a -= 6.0 * inp.brake;
            }
            if inp.nitro && self.nitro > 0.0 && v_long > 4.0 && inp.throttle > 0.1 {
                a += 6.0;
                self.nitro = js::max(0.0, self.nitro - dt / 6.0);
                self.nitro_active = true;
            }
            if self.electric {
                // Regenerative braking: the motor does some of the slowing and
                // tops up the boost; lifting off regens gently too.
                let moving = smoothstep(3.0, 18.0, v_long);
                let coast = if inp.throttle < 0.05 && inp.brake < 0.05 {
                    0.25
                } else {
                    0.0
                };
                let want = if v_long > 3.0 {
                    js::max(inp.brake, coast) * moving
                } else {
                    0.0
                };
                self.regen = damp(self.regen, want, 12.0, dt);
                self.nitro = js::min(1.0, self.nitro + dt * 0.075 * self.regen);
                let drive = if v_long > -0.5 {
                    throttle * self.engine_accel(v_long.abs())
                        + (if self.nitro_active { 6.0 } else { 0.0 })
                } else {
                    0.0
                };
                self.power_out = damp(
                    self.power_out,
                    (spec.mass * (drive - self.regen * 7.0) * js::max(0.0, v_long)) / 1000.0,
                    14.0,
                    dt,
                );
            }
            a -= 0.00115 * v_long * v_long.abs() + 0.01 * v_long;
            // Circuit run-off (surveyed tracks carry run-off grades): off the
            // tarmac the tyres plough through dirt and dry grass. Paved run-off
            // drives like the road.
            let mut off = if t.run_l.is_some() {
                clamp((v.lat.abs() - f.hw - 0.4) / 1.5, 0.0, 1.0)
            } else {
                0.0
            };
            if off > 0.0
                && let Some(loose_at) = &t.loose_at
            {
                off *= loose_at(v.x, v.z);
            }
            self.off_track = Some(off);
            if off > 0.0 {
                a -= off
                    * js::sign(v_long)
                    * js::min(v_long.abs() * 4.0, 1.2 + 0.0025 * v_long * v_long);
            }
            if inp.handbrake {
                a -= js::sign(v_long) * 3.0;
            }
            if inp.throttle < 0.05 && inp.brake < 0.05 {
                a -= js::sign(v_long) * js::min(0.9, v_long.abs());
            }
            let grade_along = f.grade * (fx * f.fx + fz * f.fz);
            a -= G * grade_along / (1.0 + grade_along * grade_along).sqrt();
            let before = v_long;
            v_long += a * dt;
            // Braking or coasting through zero stops the car rather than rolling
            // it the other way; holding the brake from a standstill reverses.
            let reversing = inp.brake > 0.05 && before.abs() < 0.05;
            if inp.throttle < 0.05
                && !reversing
                && js::sign(before) != js::sign(v_long)
                && before.abs() < 1.0
            {
                v_long = 0.0;
            }
            v.accel_long = a;
            v.brake_light = if inp.brake > 0.0 && v_long > 0.5 {
                1.0
            } else {
                0.0
            };

            // ── Yaw ──────────────────────────────────────────────────
            let slip = kernel::atan2(v_lat, js::max(v_long.abs(), 0.5));
            // Nearly stopped (e.g. nose in a wall): wheelspin still lets you pivot.
            let v_yaw = if v_long.abs() < 2.5 && (inp.throttle > 0.3 || inp.brake > 0.3) {
                js::sign(js::or(v_long, 1.0))
                    * 2.5
                    * (if inp.brake > 0.3 && v_long <= 0.5 {
                        -1.0
                    } else {
                        1.0
                    })
            } else {
                v_long
            };
            let mut yaw_target = v_yaw * kernel::tan(v.steer_angle) / self.wheel_base;
            let hb = inp.handbrake && speed > 9.0;
            if hb {
                yaw_target = yaw_target * 1.6
                    + js::sign(v.steer_angle) * 0.35 * smoothstep(9.0, 20.0, speed);
            }
            // Arcade grip: ~2 g of cornering before the tyres give up.
            let grip_lim = (if self.drifting || hb {
                spec.grip * 1.8
            } else {
                spec.grip * 1.5
            }) / js::max(speed, 4.0);
            yaw_target = clamp(yaw_target, -grip_lim, grip_lim);
            // Drift assist: stop the slide angle running away past ~35°.
            if self.drifting || hb {
                let max_slip = 0.62;
                let room = max_slip - slip.abs();
                if js::sign(yaw_target) == -js::sign(slip) && slip != 0.0 {
                    let omega_v = spec.drift_grip / js::max(speed, 5.0);
                    let cap = js::max(0.0, omega_v + 3.0 * room);
                    yaw_target = js::sign(yaw_target) * js::min(yaw_target.abs(), cap);
                }
            }
            let resp = if self.drifting { 5.0 } else { 10.0 };
            v.yaw_rate = damp(v.yaw_rate, yaw_target, resp, dt);
            // Rotate heading, keep the world velocity, re-express it.
            let wvx = v_long * fx + v_lat * rx;
            let wvz = v_long * fz + v_lat * rz;
            v.yaw += v.yaw_rate * dt;
            fx = kernel::cos(v.yaw);
            fz = kernel::sin(v.yaw);
            rx = -fz;
            rz = fx;
            v_long = wvx * fx + wvz * fz;
            v_lat = wvx * rx + wvz * rz;

            // ── Lateral grip ─────────────────────────────────────────
            let grip_a = (if hb {
                4.5
            } else if self.drifting {
                spec.drift_grip * 1.15
            } else {
                spec.grip * 2.2
            }) * (1.0 - 0.3 * off);
            let dv = js::min(v_lat.abs(), grip_a * dt);
            v_lat -= js::sign(v_lat) * dv;
            if hb || self.drifting {
                v_long += js::sign(v_long) * dv * 0.3; // keep some momentum
            }
            v.accel_lat = v.yaw_rate * speed;

            self.slip = kernel::atan2(v_lat, js::max(v_long.abs(), 0.5));
            if !self.drifting && speed > 12.0 && self.slip.abs() > 0.18 {
                self.drifting = true;
            }
            if self.drifting && (self.slip.abs() < 0.06 || speed < 7.0) {
                self.drifting = false;
            }
            if self.drifting {
                self.drift_time += dt;
                // (The electric car tops up mostly from regen, so drifting pays half.)
                self.nitro = js::min(
                    1.0,
                    self.nitro
                        + dt * 0.1
                            * (if self.electric { 0.5 } else { 1.0 })
                            * clamp(self.slip.abs() / 0.4, 0.3, 1.2)
                            * smoothstep(12.0, 30.0, speed),
                );
            } else {
                self.drift_time = 0.0;
            }
            self.skid = clamp(
                js::max_n(&[
                    v_lat.abs() / 5.0,
                    if hb { speed / 25.0 } else { 0.0 },
                    if inp.brake > 0.5 && speed > 18.0 {
                        0.35
                    } else {
                        0.0
                    },
                ]),
                0.0,
                1.0,
            ) * smoothstep(3.0, 8.0, speed);

            v.vx = v_long * fx + v_lat * rx;
            v.vz = v_long * fz + v_lat * rz;
        } else {
            // Airborne: ballistic, keep spin, a hint of air control.
            v.yaw_rate *= 1.0 - 0.8 * dt;
            v.yaw += (v.yaw_rate + inp.steer * 0.25) * dt;
            v.vx *= 1.0 - 0.02 * dt;
            v.vz *= 1.0 - 0.02 * dt;
            self.skid = 0.0;
            self.air_time += dt;
            v.accel_long = 0.0;
            v.accel_lat = 0.0;
        }
        v.speed = v.vx * kernel::cos(v.yaw) + v.vz * kernel::sin(v.yaw);

        // ── Integrate position ─────────────────────────────────────
        let prev_ground = t.surface_y(v.s, v.lat);
        v.x += v.vx * dt;
        v.z += v.vz * dt;
        let p = t.project(v.x, v.z, v.s);
        v.s = p.s;
        v.lat = p.lat;
        self.collide_walls(v, t, dt);

        // Vertical: follow the road unless it falls away faster than gravity.
        let ground = t.surface_y(v.s, v.lat);
        let ground_vy = (ground - prev_ground) / dt;
        v.vy -= G * dt;
        let y_new = v.y + v.vy * dt;
        if y_new <= ground + 0.001 {
            if !v.on_ground {
                let impact = ground_vy - v.vy;
                if impact > 2.5 {
                    self.events.push(PhysEvent::Land {
                        strength: clamp(impact / 12.0, 0.0, 1.0),
                        air: self.air_time,
                    });
                }
                self.events.push(PhysEvent::Touchdown { impact });
                self.air_time = 0.0;
            }
            v.y = ground;
            v.vy = ground_vy;
            v.on_ground = true;
        } else {
            // Road falling away faster than gravity. Suspension travel keeps the
            // tyres on the road for the first 30 cm; past that we're flying.
            v.y = y_new;
            if v.on_ground && y_new - ground > 0.3 {
                v.on_ground = false;
            }
        }
        v.vis_y = Some(if v.on_ground { ground } else { v.y });
    }

    pub fn collide_walls(&mut self, v: &mut Vehicle, t: &Track, dt: f64) {
        let f = t.frame(v.s);
        let track_yaw = kernel::atan2(f.fz, f.fx);
        let rel = wrap_angle(v.yaw - track_yaw);
        let ext = v.half_w * kernel::cos(rel).abs() + v.half_l * kernel::sin(rel).abs();
        let lim_r = f.wall_r - ext;
        let lim_l = -(f.wall_l - ext);
        let mut side = 0i32;
        let mut pen = 0.0;
        if v.lat > lim_r {
            side = 1;
            pen = v.lat - lim_r;
        } else if v.lat < lim_l {
            side = -1;
            pen = v.lat - lim_l;
        }
        self.scrape = js::max(0.0, self.scrape - dt * 4.0);
        if side != 0 {
            let sd = side as f64;
            v.x -= f.rx * pen;
            v.z -= f.rz * pen;
            v.lat -= pen;
            let vn = (v.vx * f.rx + v.vz * f.rz) * sd; // into the wall
            if vn > 0.0 {
                let e = 0.25;
                v.vx -= f.rx * sd * vn * (1.0 + e);
                v.vz -= f.rz * sd * vn * (1.0 + e);
                // Scrub speed along the wall and swing the nose parallel.
                let scrub = 1.0 - clamp(0.05 + vn * 0.025, 0.0, 0.35);
                v.vx *= scrub;
                v.vz *= scrub;
                let points_in = js::sign(rel) == sd && rel.abs() < PI / 2.0;
                if points_in {
                    v.yaw -= rel * clamp(vn * 0.06, 0.05, 0.5);
                }
                // The wall stops the touching corner swinging further into it (a
                // tail slap kills the spin), but never the rotation back toward
                // parallel: steering a nose-in car off the wall.
                if v.yaw_rate * rel > 0.0 {
                    v.yaw_rate *= 0.5;
                }
                if vn > 1.5 {
                    let px = v.x + f.rx * sd * ext;
                    let pz = v.z + f.rz * sd * ext;
                    self.events.push(PhysEvent::Impact {
                        strength: clamp(vn / 18.0, 0.0, 1.0),
                        x: px,
                        y: v.y + 0.5,
                        z: pz,
                        side,
                    });
                }
            }
            // Continuous scraping friction.
            let sp = kernel::hypot(v.vx, v.vz);
            if sp > 1.0 {
                let k = js::max(0.0, 1.0 - 4.0 * dt / sp);
                v.vx *= k;
                v.vz *= k;
                self.scrape = js::min(1.0, 0.4 + sp / 60.0);
                self.scrape_side = Some(side);
            }
        }
        // Ends of the road (loops have none).
        if t.is_loop {
            return;
        }
        if v.s < 1.0 {
            v.x += f.fx * (1.0 - v.s);
            v.z += f.fz * (1.0 - v.s);
            let vf = v.vx * f.fx + v.vz * f.fz;
            if vf < 0.0 {
                v.vx -= f.fx * vf;
                v.vz -= f.fz * vf;
            }
        }
        if v.s > t.road_end() - 3.0 {
            let vf = v.vx * f.fx + v.vz * f.fz;
            if vf > 0.0 {
                v.vx -= f.fx * vf;
                v.vz -= f.fz * vf;
            }
        }
    }

    pub fn speed_kmh(&self, v: &Vehicle) -> f64 {
        v.speed.abs() * 3.6
    }
}
