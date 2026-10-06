//! Civilian traffic (port of `src/vehicles/Traffic.js`). A fixed pool of cars
//! is kept around the player: cars that fall far behind are recycled to a
//! spot ahead that suits the zone — oncoming cars on two-lane roads (plus the
//! odd tractor), both directions on the beach boulevard, dense
//! same-direction traffic on freeways, and headlights streaming the other way
//! on the far carriageway where a level has one. Rules per zone come from the
//! level (level.traffic).
//!
//! The far-model switch (`farLod`) is presentation and lives in the client.

use mp_levels::world::OppositeCarriageway;
use mp_math::{Rng, clamp, js, rpick, rrange, stop_speed};
use mp_track::{Frame, Level, ROAD_TYPES, Track, TrafficRule};

use crate::body::{AgentView, Body};
use crate::dims::dims;
use crate::kinematic::{Kinematic, Surface};
use crate::vehicle::Vehicle;

const PAINT: [u32; 10] = [
    0xc9ccd1, 0x2b2f36, 0x8a1c1c, 0x1d3f75, 0xe8e6df, 0x4d5a3a, 0x6b4a2e, 0x9aa3ad, 0x2e6d8e,
    0xb5a27a,
];

fn mass_of(kind: &str) -> f64 {
    match kind {
        "sedan" => 1500.0,
        "hatch" => 1200.0,
        "van" => 2200.0,
        "pickup" => 2100.0,
        "boxtruck" => 6500.0,
        "tractor" => 3500.0,
        _ => f64::NAN,
    }
}

/// Past FAR_OUT metres from the camera a car switches to its far model, and
/// back inside FAR_IN (read by the client).
pub const FAR_OUT: f64 = 95.0;
pub const FAR_IN: f64 = 85.0;

#[derive(Clone, Debug, PartialEq)]
pub struct TrafficCar {
    pub k: Kinematic,
    pub kind_name: &'static str,
    pub active: bool,
    pub cruise: f64,
    pub lane_lat: f64,
    pub crashed: f64,
    pub opposite: bool,
    /// Missing until the first spawn.
    pub lane: Option<i32>,
}

impl Body for TrafficCar {
    fn v(&self) -> &Vehicle {
        &self.k.v
    }
    fn mass(&self) -> f64 {
        self.k.v.mass
    }
    fn kinematic_only(&self) -> bool {
        true
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
        AgentView {
            kinematic_only: true,
            ..self.k.view()
        }
    }
}

/// One entry of the agent list Traffic.update reads: a car of this pool
/// (read live, as it moves during the update) or another body as it is.
#[derive(Clone, Copy, Debug)]
pub enum Agent {
    Traffic(usize),
    Other(AgentView),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Traffic {
    pub rules: Vec<TrafficRule>,
    pub opposite_carriageway: Option<OppositeCarriageway>,
    pub cars: Vec<TrafficCar>,
    /// `pool`: per kind, in the JS insertion order, the indices in `cars`.
    pub pool: Vec<(&'static str, Vec<usize>)>,
    pub opp_pool: Vec<usize>,
    pub max_active: usize,
    pub next_spawn_s: f64,
    pub next_opp_s: f64,
}

impl Traffic {
    /// `rng` is Traffic's own stream (mulberry32 seed 99 in the game).
    pub fn new(
        level: &Level,
        world: Option<OppositeCarriageway>,
        count: usize,
        rng: &mut dyn Rng,
    ) -> Traffic {
        let rules = level.traffic.clone();
        let mut cars = Vec::new();
        let has_tractors = rules
            .iter()
            .any(|r| r.mix.iter().any(|(k, _)| *k == "tractor"));
        let mut per: Vec<(&'static str, usize)> = vec![
            ("sedan", 8),
            ("hatch", 6),
            ("van", 4),
            ("pickup", 5),
            ("boxtruck", 5),
            ("tractor", if has_tractors { 2 } else { 0 }),
        ];
        if rules.iter().any(|r| r.gap[0] < 40.0) {
            for (k, n) in per.iter_mut() {
                if ["sedan", "hatch", "van", "boxtruck"].contains(k) {
                    *n += 3;
                }
            }
        }
        let paint: Vec<u32> = level
            .traffic_paint
            .clone()
            .unwrap_or_else(|| PAINT.to_vec());
        let mut make = |k: &'static str, rng: &mut dyn Rng| -> usize {
            let color = if k == "tractor" {
                *rpick(rng, &[0x9c2a1c, 0x3d6b2a])
            } else {
                *rpick(rng, &paint)
            };
            let v = Vehicle::new(dims(k).expect("traffic kind"), k, mass_of(k), "", color);
            cars.push(TrafficCar {
                k: Kinematic::new(v),
                kind_name: k,
                active: false,
                cruise: 20.0,
                lane_lat: 0.0,
                crashed: 0.0,
                opposite: false,
                lane: None,
            });
            cars.len() - 1
        };
        let mut pool = Vec::new();
        for (k, n) in per {
            let mut ids = Vec::new();
            for _ in 0..n {
                ids.push(make(k, rng));
            }
            pool.push((k, ids));
        }
        // Oncoming traffic on the far carriageway (visual only, behind the median).
        let mut opp_pool = Vec::new();
        if rules.iter().any(|r| r.opposite.is_some_and(|o| o != 0.0)) {
            for _ in 0..14 {
                let kind = *rpick(
                    rng,
                    &["sedan", "sedan", "hatch", "van", "boxtruck", "pickup"],
                );
                opp_pool.push(make(kind, rng));
            }
        }
        for &i in &opp_pool {
            cars[i].opposite = true;
            cars[i].k.free_lat = true;
        }
        Traffic {
            rules,
            opposite_carriageway: world,
            cars,
            pool,
            opp_pool,
            max_active: count,
            next_spawn_s: 0.0,
            next_opp_s: 0.0,
        }
    }

    pub fn pool_of(&self, kind: &str) -> Option<&Vec<usize>> {
        self.pool.iter().find(|(k, _)| *k == kind).map(|(_, v)| v)
    }

    fn lane_for(&self, t: &Track, dir: i32, f: &Frame, s: f64, rng: &mut dyn Rng) -> (f64, i32) {
        let ty = ROAD_TYPES[t.road_type[t.idx(s)] as usize].key;
        let d = dir as f64;
        if ty == "freeway" && f.hw > 8.5 {
            let left = -f.hw + 1.2;
            let right = f.hw - 2.0;
            let lw = (right - left) / 4.0;
            let lane = (rng.next_f64() * 4.0).floor();
            return (left + lw * (lane + 0.5), lane as i32);
        }
        if ty == "street" {
            // Two lanes each way between the kerbs.
            let lw = f.hw / 2.0;
            let lane = if rng.next_f64() < 0.5 { 0 } else { 1 };
            return (
                d * lw * (lane as f64 + 0.5),
                if dir > 0 { 2 + lane } else { 1 - lane },
            );
        }
        if ty == "boulevard" {
            let lw = (f.hw - 0.45) / 2.0;
            let lane = if rng.next_f64() < 0.5 { 0 } else { 1 };
            return (
                d * (lw * (lane as f64 + 0.5) + 0.1),
                if dir > 0 { 2 + lane } else { 1 - lane },
            );
        }
        let off = f.hw * 0.5;
        (
            if dir > 0 { off } else { -off },
            if dir > 0 { 1 } else { 0 },
        )
    }

    /// Put a car somewhere ahead of the player that suits the zone.
    pub fn spawn(&mut self, t: &Track, s_raw: f64, rng: &mut dyn Rng) -> bool {
        let s = t.wrap(s_raw);
        let f = t.frame(s);
        if !t.is_loop && (s < t.start_s + 250.0 || s > t.finish_s + 100.0) {
            return false;
        }
        let Some(rule) = self.rules.get(f.zone as usize).cloned() else {
            return false;
        };
        if rule.mix.is_empty() {
            return false; // an empty mix: no traffic in this zone
        }
        let mut r = rng.next_f64();
        let mut kind = rule.mix[0].0;
        for (k, w) in &rule.mix {
            r -= w;
            if r <= 0.0 {
                kind = *k;
                break;
            }
        }
        let Some(ci) = self
            .pool_of(kind)
            .and_then(|p| p.iter().copied().find(|&i| !self.cars[i].active))
        else {
            return false;
        };
        let mut dir = if rng.next_f64() < rule.oncoming {
            -1
        } else {
            1
        };
        if kind == "tractor" {
            dir = 1;
        }
        let (lat, lane) = self.lane_for(t, dir, &f, s, rng);
        for o in &self.cars {
            if o.active && t.ds(o.k.s, s).abs() < 25.0 && (o.k.lat - lat).abs() < 3.0 {
                return false;
            }
        }
        activate(&mut self.cars[ci], s, lat, dir);
        let car = &mut self.cars[ci];
        car.lane = Some(lane);
        let [a, b] = rule.speed;
        car.cruise = if kind == "tractor" {
            8.0
        } else if kind == "boxtruck" {
            a * 0.9
        } else {
            rrange(rng, a, b)
        };
        if ROAD_TYPES[t.road_type[t.idx(s)] as usize].key == "freeway" {
            car.cruise += (3 - lane) as f64 * 1.6; // left lanes faster
        }
        car.k.speed = car.cruise;
        car.k.write_pos(t);
        true
    }

    pub fn spawn_opposite(&mut self, t: &Track, s_raw: f64, rng: &mut dyn Rng) -> bool {
        let s = t.wrap(s_raw);
        let Some(oc) = &self.opposite_carriageway else {
            return false;
        };
        if oc.lanes.is_empty() {
            return false;
        }
        if !t.is_loop && (s < oc.s0 + 20.0 || s > oc.s1 - 20.0) {
            return false;
        }
        let Some(rule) = self.rules.get(t.zone[t.idx(s)] as usize) else {
            return false;
        };
        let Some(opp) = rule.opposite.filter(|&o| o != 0.0) else {
            return false;
        };
        if rng.next_f64() > opp {
            return false;
        }
        let Some(ci) = self
            .opp_pool
            .iter()
            .copied()
            .find(|&i| !self.cars[i].active)
        else {
            return false;
        };
        let lat = *rpick(rng, &oc.lanes);
        for &oi in &self.opp_pool {
            let o = &self.cars[oi];
            if o.active && t.ds(o.k.s, s).abs() < 30.0 && (o.k.lat - lat).abs() < 2.0 {
                return false;
            }
        }
        activate(&mut self.cars[ci], s, lat, -1);
        let car = &mut self.cars[ci];
        car.k.surface = Surface::Opposite;
        car.cruise = rrange(rng, 22.0, 31.0);
        car.k.speed = car.cruise;
        car.k.write_pos(t);
        true
    }

    pub fn despawn(&mut self, i: usize) {
        self.cars[i].active = false;
    }

    /// `playerS`: position on the track; `player_dist`: unwrapped distance
    /// driven (same as playerS on point-to-point tracks). `agents` is the
    /// tick's agent list.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        t: &Track,
        dt: f64,
        player_s: f64,
        agents: &[Agent],
        _night: f64,
        player_dist: f64,
        rng: &mut dyn Rng,
    ) {
        let mut n = 0;
        for i in 0..self.cars.len() {
            let c = &self.cars[i];
            if !c.active {
                continue;
            }
            let ds = t.ds(player_s, c.k.s);
            let off_deck = c.opposite
                && !t.is_loop
                && self
                    .opposite_carriageway
                    .as_ref()
                    .is_some_and(|oc| c.k.s < oc.s0 - 5.0 || c.k.s > oc.s1);
            if ds < (if c.opposite { -120.0 } else { -260.0 })
                || ds > 1400.0
                || c.crashed > 25.0
                || off_deck
            {
                self.despawn(i);
            } else if !c.opposite {
                n += 1;
            }
        }
        // Keep the road ahead populated.
        if self.next_spawn_s < player_dist + 350.0 {
            self.next_spawn_s = player_dist + 350.0 + rng.next_f64() * 200.0;
        }
        let mut tries = 0;
        while n < self.max_active && self.next_spawn_s < player_dist + 1100.0 && {
            tries += 1;
            tries - 1 < 6
        } {
            let gap = self.rules[t.zone[t.idx(self.next_spawn_s)] as usize].gap;
            if self.spawn(t, self.next_spawn_s, rng) {
                n += 1;
            }
            self.next_spawn_s += rrange(rng, gap[0], gap[1]);
        }
        if !self.opp_pool.is_empty() {
            if self.next_opp_s < player_dist + 500.0 {
                self.next_opp_s = player_dist + 500.0 + rng.next_f64() * 200.0;
            }
            let mut k = 0;
            while k < 4 && self.next_opp_s < player_dist + 1200.0 {
                self.spawn_opposite(t, self.next_opp_s, rng);
                self.next_opp_s += rrange(rng, 40.0, 110.0);
                k += 1;
            }
        }

        for ci in 0..self.cars.len() {
            if !self.cars[ci].active {
                continue;
            }
            self.cars[ci].k.frame(t);
            if self.cars[ci].crashed > 0.0 {
                let c = &mut self.cars[ci];
                c.crashed += dt;
                c.k.speed = js::max(0.0, c.k.speed - 9.0 * dt);
                c.k.lat_vel *= 1.0 - 2.0 * dt;
            } else if !self.cars[ci].opposite {
                // Car-following: slow for anything ahead in the lane.
                let c = &self.cars[ci];
                let mut v_t = c.cruise;
                for a in agents {
                    let o = match a {
                        Agent::Traffic(j) if *j == ci => continue,
                        Agent::Traffic(j) => self.cars[*j].view(),
                        Agent::Other(v) => *v,
                    };
                    let ds = t.ds(c.k.s, o.s) * c.k.dir as f64;
                    if ds > 0.0 && ds < 35.0 && (o.lat - c.k.lat).abs() < 2.4 {
                        let ov = if o.dir == c.k.dir { o.speed_along } else { 0.0 };
                        v_t = js::min(v_t, ov + (ds - 10.0) * 0.4);
                    }
                }
                // The road runs out past the finish: pull up short of its end,
                // ahead of where the racers park.
                if !t.is_loop && c.k.dir > 0 {
                    v_t = js::min(v_t, stop_speed(t.road_end() - 200.0 - c.k.s, 2.5));
                }
                v_t = js::max(0.0, v_t);
                let c = &mut self.cars[ci];
                c.k.speed += clamp(v_t - c.k.speed, -8.0 * dt, 3.0 * dt);
                // Return to lane after a shove.
                let la = clamp((c.lane_lat - c.k.lat) * 1.5 - c.k.lat_vel * 2.0, -3.0, 3.0);
                c.k.lat_vel += la * dt;
                if c.k.stunned > 0.6 {
                    c.crashed = 0.01;
                }
            }
            let c = &mut self.cars[ci];
            c.k.v.brake_light = if c.k.speed < c.cruise - 2.0 { 1.0 } else { 0.0 };
            c.k.advance(t, dt);
        }
    }
}

pub fn activate(car: &mut TrafficCar, s: f64, lat: f64, dir: i32) {
    car.active = true;
    car.k.s = s;
    car.k.lat = lat;
    car.lane_lat = lat;
    car.k.dir = dir;
    car.k.lat_vel = 0.0;
    car.k.spin = 0.0;
    car.k.spin_rate = 0.0;
    car.crashed = 0.0;
    car.k.stunned = 0.0;
}
