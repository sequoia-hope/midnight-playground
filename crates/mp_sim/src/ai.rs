//! Rival driver (port of `src/vehicles/AIDriver.js`). Follows a precomputed
//! racing line at a speed profile scaled by skill, looks ahead for slower
//! cars and oncoming traffic, and picks a side to pass on. Mild
//! rubber-banding keeps the pack close to the player. Once finished, the race
//! hands it a parking spot (`park`: target speed and lane by s) and it
//! cruises there, queueing behind anything in the way.

use mp_math::{Rng, clamp, js, lerp, smoothstep};
use mp_track::{Frame, Track};

use crate::body::{AgentView, Body};
use crate::kinematic::Kinematic;
use crate::park::Park;
use crate::vehicle::Vehicle;

/// The options Race gives a rival (`opts`); `None` takes the JS default.
#[derive(Clone, Copy, Debug, Default)]
pub struct AiOpts {
    pub skill: Option<f64>,
    pub line_factor: Option<f64>,
    pub bias: Option<f64>,
    pub name: &'static str,
    pub power: Option<f64>,
}

/// What `update` reads besides the driver (`ctx`). `cars` is every agent as
/// it is now; `me` is this driver's index in it, if it is there.
pub struct AiCtx<'a> {
    pub cars: &'a [AgentView],
    pub me: Option<usize>,
    pub player_s: f64,
    /// Race progress of the player, on a circuit.
    pub player_prog: Option<f64>,
    pub started: bool,
    pub time: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AiDriver {
    pub k: Kinematic,
    pub skill: f64,
    pub line_factor: f64,
    /// Preferred offset from the line.
    pub bias: f64,
    pub name: &'static str,
    pub power: f64,
    pub color: u32,
    pub avoid: f64,
    pub avoid_timer: f64,
    pub nitro: f64,
    pub nitro_timer: f64,
    pub nitro_active: bool,
    pub finished: bool,
    pub finish_time: Option<f64>,
    pub throttle: f64,
    pub park: Option<Park>,
    /// Hot Pursuit: seconds left pulled over after a bust.
    pub hold: f64,
    pub hold_lat: Option<f64>,
    /// Hot Pursuit: seconds left on shredded tyres.
    pub spiked: f64,
    /// Race progress, set by the grid and the race.
    pub prog: Option<f64>,
}

impl AiDriver {
    /// Random draws (nitro timing) come from `rng`: `Math.random` in the
    /// game, the `ai` stream in the simulation (SPEC 4.3).
    pub fn new(vehicle: Vehicle, opts: AiOpts, rng: &mut dyn Rng) -> AiDriver {
        AiDriver {
            k: Kinematic::new(vehicle),
            skill: opts.skill.unwrap_or(0.95),
            line_factor: opts.line_factor.unwrap_or(0.9),
            bias: opts.bias.unwrap_or(0.0),
            name: if opts.name.is_empty() {
                "AI"
            } else {
                opts.name
            },
            power: opts.power.unwrap_or(500.0),
            color: 0,
            avoid: 0.0,
            avoid_timer: 0.0,
            nitro: 0.0,
            nitro_timer: 4.0 + rng.next_f64() * 10.0,
            nitro_active: false,
            finished: false,
            finish_time: None,
            throttle: 0.0,
            park: None,
            hold: 0.0,
            hold_lat: None,
            spiked: 0.0,
            prog: None,
        }
    }

    pub fn write_pos(&mut self, t: &Track) {
        self.k.write_pos(t);
    }

    pub fn update(&mut self, t: &Track, dt: f64, ctx: &AiCtx, rng: &mut dyn Rng) {
        let f = self.k.frame(t);
        if !ctx.started {
            self.k.write_pos(t);
            return;
        }
        let park = self.park;
        let (s, lat, speed) = (self.k.s, self.k.lat, self.k.speed);

        // Target speed from the profile a little ahead (braking points).
        let look = clamp(speed * 0.5, 6.0, 40.0);
        let i_a = t.idx(s + look);
        let mut v_t = js::min(
            t.speed_profile[t.idx(s)] as f64,
            t.speed_profile[i_a] as f64,
        ) * self.skill;
        if let Some(p) = park {
            v_t = js::min(v_t, p.speed(t, s));
        } else {
            // Rubber band against the player (on a circuit, by race progress).
            let gap = match ctx.player_prog {
                Some(pp) => self.prog.unwrap_or(f64::NAN) - pp,
                None => s - ctx.player_s,
            };
            v_t *= if gap > 120.0 {
                lerp(1.0, 0.9, smoothstep(120.0, 400.0, gap))
            } else if gap < -80.0 {
                lerp(1.0, 1.1, smoothstep(80.0, 350.0, -gap))
            } else {
                1.0
            };
        }
        if self.k.stunned > 0.0 {
            v_t *= 0.6;
        }
        // Circuit run-off slows rivals as it does the player.
        if t.run_l.is_some() && lat.abs() > f.hw + 0.4 {
            let loose = match &t.loose_at {
                Some(l) => l(f.x + f.rx * lat, f.z + f.rz * lat),
                None => 1.0,
            };
            v_t *= 1.0 - 0.2 * loose;
        }
        // Hot Pursuit: shredded tyres after a spike strip.
        if self.spiked > 0.0 {
            self.spiked -= dt;
            v_t *= 0.75;
        }

        // Racing line (or the lane we're parking in).
        let mut lat_t = match park {
            Some(p) => p.lat(t, s),
            None => t.racing_line[t.idx(s + 8.0)] as f64 * self.line_factor + self.bias,
        };

        // Look ahead for cars in the way.
        let my_w = self.k.v.half_w;
        let me = self.seeker();
        let block = find_block(t, &me, ctx.cars, ctx.me, lat_t, &|_| false);
        // Parking: queue behind anything in our lane, but still go round a car
        // we're closing on fast.
        if park.is_some() {
            for (i, o) in ctx.cars.iter().enumerate() {
                let ds = o.s - s;
                if Some(i) == ctx.me
                    || o.dir != 1
                    || ds <= 0.0
                    || ds > 40.0
                    || (o.lat - lat).abs() > my_w + o.half_w + 0.5
                {
                    continue;
                }
                v_t = js::min(v_t, js::max(0.0, o.speed_along + (ds - 10.0) * 0.4));
            }
        }
        if let Some(bi) = block {
            let b = &ctx.cars[bi];
            if !(park.is_some() && speed - b.speed_along < 5.0) {
                match pass_lat(&me, b, &f) {
                    Some(choice) => {
                        self.avoid = choice;
                        self.avoid_timer = 0.9;
                    }
                    None => {
                        if b.dir == 1 {
                            v_t = js::min(v_t, b.speed_along - 0.5);
                        }
                    }
                }
            }
        }
        if self.avoid_timer > 0.0 {
            self.avoid_timer -= dt;
            lat_t = self.avoid;
        }

        // On a circuit the walls stand past the run-off: race on the tarmac.
        let lim = js::min(
            js::min(f.wall_r, f.wall_l) - my_w - 0.35,
            if t.run_l.is_some() {
                f.hw - 0.6
            } else {
                f64::INFINITY
            },
        );
        // Cars alongside (findBlock only sees cars ahead): don't steer into
        // them. Leaning on a car pinned to a wall would shove it along the wall
        // and spin it, so keep off its side, and when there's no room on ours
        // or we're tucked in behind it, drop back instead. (Not roadblock
        // pieces: we aim for their gap, through the sawhorses.)
        for (i, o) in ctx.cars.iter().enumerate() {
            if Some(i) == ctx.me || o.dir != 1 || o.gap_lat.is_some() {
                continue;
            }
            let ds = t.ds(s, o.s); // > 0: o is ahead
            if ds.abs() > self.k.v.half_l + o.half_l + 1.0 {
                continue;
            }
            let dl = o.lat - lat;
            let clear = my_w + o.half_w + 0.4;
            if dl.abs() > clear + 1.5 {
                continue;
            }
            let room = if dl > 0.0 {
                o.lat - clear
            } else {
                o.lat + clear
            };
            lat_t = if dl > 0.0 {
                js::min(lat_t, room)
            } else {
                js::max(lat_t, room)
            };
            if ds > -1.0 && (dl.abs() < clear - 0.3 || room.abs() > lim) {
                v_t = js::min(v_t, o.speed_along - 2.0);
            }
        }

        // Hot Pursuit: busted. Pull over onto the shoulder and wait out the
        // penalty (the race clock keeps running), then rejoin.
        if self.hold > 0.0 {
            self.hold -= dt;
            v_t = 0.0;
            lat_t = self.hold_lat.unwrap_or(lat);
        }

        lat_t = clamp(lat_t, -lim, lim);

        // Lateral controller (critically damped-ish, limited accel).
        let lat_a = clamp((lat_t - lat) * 3.2 - self.k.lat_vel * 2.6, -7.0, 7.0);
        self.k.lat_vel += lat_a * dt;
        self.k.lat_vel = clamp(self.k.lat_vel, -6.0, 6.0);

        // Speed controller.
        self.nitro_timer -= dt;
        if park.is_none()
            && self.nitro_timer < 0.0
            && self.nitro <= 0.0
            && f.kappa.abs() < 0.004
            && speed > 30.0
        {
            self.nitro = 2.5;
            self.nitro_timer = 12.0 + rng.next_f64() * 14.0;
        }
        let nitroing = self.nitro > 0.0;
        if nitroing {
            self.nitro -= dt;
            v_t *= 1.12;
        }
        let acc = js::min(9.0, self.power / js::max(speed, 5.0)) + if nitroing { 4.0 } else { 0.0 };
        let dv = v_t - speed;
        self.throttle = if dv > 0.0 { 1.0 } else { 0.0 };
        // Ease off after the finish.
        let brake = if park.is_some() && block.is_none() {
            4.0
        } else if self.hold > 0.0 {
            9.0
        } else {
            12.0
        };
        self.k.speed += if dv > 0.0 {
            js::min(dv, acc * dt)
        } else {
            js::max(dv, -brake * dt)
        };
        self.k.speed = js::max(0.0, self.k.speed);
        self.k.v.brake_light = if dv < -1.5 { 1.0 } else { 0.0 };
        self.nitro_active = nitroing;
        self.k.v.accel_long = if dv > 0.0 {
            acc * 0.6
        } else {
            -js::min(8.0, brake)
        };
        self.k.v.accel_lat = self.k.speed * self.k.speed * f.kappa;

        self.k.advance(t, dt);
        if !self.finished && self.k.s >= t.finish_s {
            self.finished = true;
            self.finish_time = Some(ctx.time);
        }
    }

    /// This driver as `findBlock` and `passLat` read it.
    pub fn seeker(&self) -> Seeker {
        Seeker {
            s: self.k.s,
            lat: self.k.lat,
            speed: self.k.speed,
            half_w: self.k.v.half_w,
        }
    }
}

impl Body for AiDriver {
    fn v(&self) -> &Vehicle {
        &self.k.v
    }
    fn mass(&self) -> f64 {
        self.k.v.mass
    }
    fn velocity(&self) -> (f64, f64) {
        self.k.velocity()
    }
    fn set_velocity(&mut self, vx: f64, vz: f64) {
        self.k.set_velocity(vx, vz)
    }
    fn translate(&mut self, t: &Track, dx: f64, dz: f64) {
        self.k.translate(t, dx, dz)
    }
    fn add_spin(&mut self, w: f64) {
        self.k.add_spin(w)
    }
    fn view(&self) -> AgentView {
        self.k.view()
    }
}

/// The driver doing the looking (`self` in `findBlock` and `passLat`).
#[derive(Clone, Copy, Debug)]
pub struct Seeker {
    pub s: f64,
    pub lat: f64,
    pub speed: f64,
    pub half_w: f64,
}

/// The nearest car ahead that we'd hit on our current lane or on the lane we
/// want (latT) within a few seconds: slower cars, stopped cars and anything
/// oncoming. Shared by the rivals and the police. `skip(i)` → true ignores
/// car i. Returns its index in `cars`.
pub fn find_block(
    t: &Track,
    me: &Seeker,
    cars: &[AgentView],
    me_idx: Option<usize>,
    lat_t: f64,
    skip: &dyn Fn(usize) -> bool,
) -> Option<usize> {
    let my_w = me.half_w;
    let mut block = None;
    let mut block_gap = f64::INFINITY;
    for (i, o) in cars.iter().enumerate() {
        if Some(i) == me_idx || skip(i) {
            continue;
        }
        let ds = t.ds(me.s, o.s);
        let oncoming = o.dir == -1;
        let closing = me.speed
            - if oncoming {
                -o.speed_along
            } else {
                o.speed_along
            };
        // Far enough ahead to get round it at speed (a stopped car at 60 m/s
        // needs ~3 s of warning, not 45 m).
        let range = js::max(if oncoming { 110.0 } else { 45.0 }, closing * 3.2);
        if ds < 2.0 || ds > range {
            continue;
        }
        if closing < 0.5 && !oncoming {
            continue;
        }
        let t_hit = ds / js::max(closing, 1.0);
        if t_hit > 3.2 {
            continue;
        }
        if ((o.lat - me.lat).abs() < my_w + o.half_w + 0.7
            || (o.lat - lat_t).abs() < my_w + o.half_w + 0.7)
            && ds < block_gap
        {
            block_gap = ds;
            block = Some(i);
        }
    }
    block
}

/// A lane that clears `block` on the nearer side, inside the walls, or None
/// when neither side fits. A roadblock car points at its gap instead.
pub fn pass_lat(me: &Seeker, block: &AgentView, f: &Frame) -> Option<f64> {
    if let Some(g) = block.gap_lat {
        return Some(g);
    }
    let my_w = me.half_w;
    let need = my_w + block.half_w + 1.1;
    let wall_r = f.wall_r - my_w - 0.4;
    let wall_l = -(f.wall_l - my_w - 0.4);
    let right = block.lat + need;
    let left = block.lat - need;
    let ok_r = right < wall_r;
    let ok_l = left > wall_l;
    if ok_r && ok_l {
        return Some(if (right - me.lat).abs() < (left - me.lat).abs() {
            right
        } else {
            left
        });
    }
    if ok_r {
        return Some(right);
    }
    if ok_l {
        return Some(left);
    }
    None
}
