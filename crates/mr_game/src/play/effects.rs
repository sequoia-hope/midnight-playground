//! `game/Effects.js` without the engine (roadmap WP 4.4): tyre smoke,
//! sparks, skid marks, nitro flames and the fake headlight pools, as the
//! JS keeps them: the two particle ring buffers (700 smoke, 500 sparks)
//! simulated on the CPU in `Float32Array`s (here `f32`, every store rounded
//! as the JS's), the 2,400-quad ring of skid marks, and per car its flames,
//! its last wheel contacts and its pool. [`super::fx`] draws it.
//!
//! The JS draws its randomness from `Math.random`; here from the effects'
//! own `mulberry32` stream, in the JS's order of draws (DECISIONS D801).
//! What the JS does by chance once a frame (smoke from a skid, sparks off a
//! scrape) and the launch puffs it emits once a frame happen at the same
//! rate per second as at 60 frames a second (SPEC 6.5, D802).

use mr_math::{Mulberry32, clamp, js, kernel, lerp};

/// `Particles`: a ring buffer of soft points.
#[derive(Clone, Debug)]
pub struct Particles {
    pub max: usize,
    pub pos: Vec<f32>,
    pub vel: Vec<f32>,
    pub col: Vec<f32>,
    pub size: Vec<f32>,
    pub alpha: Vec<f32>,
    pub life: Vec<f32>,
    pub age: Vec<f32>,
    pub grow: Vec<f32>,
    pub a0: Vec<f32>,
    pub drag: Vec<f32>,
    pub grav: Vec<f32>,
    pub next: usize,
    /// Some particle is alive (or died this frame): the buffers changed.
    pub live: bool,
}

/// A `Float32Array` store.
fn f(x: f64) -> f32 {
    x as f32
}

impl Particles {
    pub fn new(max: usize) -> Particles {
        let z = |n: usize| vec![0f32; n];
        Particles {
            max,
            pos: z(max * 3),
            vel: z(max * 3),
            col: z(max * 3),
            size: z(max),
            alpha: z(max),
            life: z(max),
            age: z(max),
            grow: z(max),
            a0: z(max),
            drag: z(max),
            grav: z(max),
            next: 0,
            live: false,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn emit(
        &mut self,
        [x, y, z]: [f64; 3],
        [vx, vy, vz]: [f64; 3],
        life: f64,
        size: f64,
        grow: f64,
        [r, g, b]: [f64; 3],
        alpha: f64,
        drag: f64,
        grav: f64,
    ) {
        let i = self.next;
        self.next = (self.next + 1) % self.max;
        self.pos[i * 3..i * 3 + 3].copy_from_slice(&[f(x), f(y), f(z)]);
        self.vel[i * 3..i * 3 + 3].copy_from_slice(&[f(vx), f(vy), f(vz)]);
        self.col[i * 3..i * 3 + 3].copy_from_slice(&[f(r), f(g), f(b)]);
        self.size[i] = f(size);
        self.grow[i] = f(grow);
        self.life[i] = f(life);
        self.age[i] = 0.0;
        self.a0[i] = f(alpha);
        self.alpha[i] = f(alpha);
        self.drag[i] = f(drag);
        self.grav[i] = f(grav);
        self.live = true;
    }

    /// One frame: drag, gravity, growth, and the alpha's fade in over the
    /// first tenth of the life and out over the rest.
    pub fn update(&mut self, dt: f64) {
        let mut live = false;
        for i in 0..self.max {
            if self.age[i] >= self.life[i] {
                self.alpha[i] = 0.0;
                continue;
            }
            live = true;
            self.age[i] = f(f64::from(self.age[i]) + dt);
            let k = kernel::exp(-f64::from(self.drag[i]) * dt);
            let v = &mut self.vel[i * 3..i * 3 + 3];
            v[0] = f(f64::from(v[0]) * k);
            v[1] = f(f64::from(v[1]) * k - f64::from(self.grav[i]) * dt);
            v[2] = f(f64::from(v[2]) * k);
            for (p, v) in self.pos[i * 3..i * 3 + 3].iter_mut().zip(v.iter()) {
                *p = f(f64::from(*p) + f64::from(*v) * dt);
            }
            self.size[i] = f(f64::from(self.size[i]) + f64::from(self.grow[i]) * dt);
            let fr = f64::from(self.age[i]) / f64::from(self.life[i]);
            self.alpha[i] = f(f64::from(self.a0[i])
                * if fr < 0.1 {
                    fr / 0.1
                } else {
                    1.0 - (fr - 0.1) / 0.9
                });
        }
        // The frame the last one died still uploads its zero alpha.
        self.live = live;
    }
}

/// A point on the road.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct P3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// `SkidMarks`: a ring buffer of `max` quads.
#[derive(Clone, Debug)]
pub struct SkidMarks {
    pub max: usize,
    /// Four corners a quad, three floats each.
    pub pos: Vec<f32>,
    /// One alpha a corner.
    pub alpha: Vec<f32>,
    pub next: usize,
    pub dirty: bool,
}

impl SkidMarks {
    pub const MAX: usize = 2400;

    pub fn new(max: usize) -> SkidMarks {
        SkidMarks {
            max,
            pos: vec![0.0; max * 12],
            alpha: vec![0.0; max * 4],
            next: 0,
            dirty: false,
        }
    }

    /// A quad across the direction of travel from `a` to `b`.
    pub fn add(&mut self, a: P3, b: P3, width: f64, alpha: f64) {
        let (dx, dz) = (b.x - a.x, b.z - a.z);
        let l = kernel::hypot(dx, dz);
        if !(0.05..=4.0).contains(&l) {
            return;
        }
        let nx = -dz / l * width * 0.5;
        let nz = dx / l * width * 0.5;
        let i = self.next;
        self.next = (self.next + 1) % self.max;
        let o = i * 12;
        let q = [
            a.x + nx,
            a.y + 0.03,
            a.z + nz,
            a.x - nx,
            a.y + 0.03,
            a.z - nz,
            b.x + nx,
            b.y + 0.03,
            b.z + nz,
            b.x - nx,
            b.y + 0.03,
            b.z - nz,
        ];
        for (k, v) in q.iter().enumerate() {
            self.pos[o + k] = f(*v);
        }
        self.alpha[i * 4..i * 4 + 4].fill(f(alpha));
        self.dirty = true;
    }

    /// `flush`: whether the buffers are to be sent again.
    pub fn flush(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }
}

/// What the effects read of a car this frame (`v.x`, `v.yaw`, ...,
/// `v.model.root.visible`, `v.model.dims`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CarIn {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f64,
    pub visual_yaw: f64,
    pub vx: f64,
    pub vz: f64,
    pub on_ground: bool,
    /// `model.root.visible`: off for a traffic car off the road.
    pub visible: bool,
    /// `model.dims.wheelBase`, `.track` (0 when missing).
    pub wheel_base: f64,
    pub track: f64,
}

/// `Race.extras`' entry for a car (`{ nitro, skid, launch }`; a car
/// without one reads as all off).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Extras {
    pub nitro: bool,
    pub skid: f64,
    pub launch: bool,
}

/// The headlight pool's place: `pool.position`, `pool.rotation.y`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pool {
    pub visible: bool,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub rot_y: f64,
    /// `scale.x`, `scale.z`: 9 × 16 m for the player, 7 × 12 for the rest.
    pub sx: f64,
    pub sz: f64,
}

/// A registered car (`addCar`'s entry).
#[derive(Clone, Debug)]
pub struct CarFx {
    pub player: bool,
    /// Its flames are shown, and each one's length (`scale.z`).
    pub flames_on: bool,
    pub flames: Vec<f64>,
    pub last_l: Option<P3>,
    pub last_r: Option<P3>,
    pub pool: Pool,
    /// Launch puffs owed (one per 1/60 s, D802).
    launch_due: f64,
}

/// `Effects`.
#[derive(Clone, Debug)]
pub struct Effects {
    pub smoke: Particles,
    pub sparks: Particles,
    pub skids: SkidMarks,
    pub cars: Vec<CarFx>,
    /// The shared pool material's opacity (`poolMat.opacity = 0.3 ×
    /// night`, which every pool draws with: D800).
    pub pool_opacity: f64,
    /// `uScale` of both particle materials (`resize`).
    pub scale: f64,
    rng: Mulberry32,
}

/// The chance of something that happens with chance `p` a frame at 60
/// frames a second, over a frame of `dt` seconds (D802). Exactly `p` at
/// 1/60 s.
pub fn rate60(p: f64, dt: f64) -> f64 {
    let n = dt * 60.0;
    if (n - 1.0).abs() < 1e-9 {
        return p;
    }
    if p >= 1.0 {
        return 1.0;
    }
    if p <= 0.0 || n <= 0.0 {
        return 0.0;
    }
    1.0 - kernel::pow(1.0 - p, n)
}

impl Effects {
    /// `new Effects(...)`, its random stream seeded with `seed`.
    pub fn new(seed: u32) -> Effects {
        Effects {
            smoke: Particles::new(700),
            sparks: Particles::new(500),
            skids: SkidMarks::new(SkidMarks::MAX),
            cars: Vec::new(),
            pool_opacity: 0.0,
            scale: 400.0,
            rng: Mulberry32::new(seed),
        }
    }

    /// Restarts the random stream (the staged scenes' `mulberry32(seed)`).
    pub fn reseed(&mut self, seed: u32) {
        self.rng = Mulberry32::new(seed);
    }

    /// `Math.random()`.
    pub fn random(&mut self) -> f64 {
        self.rng.next_f64()
    }

    /// `resize(heightPx, fovDeg)`: the particles' size scale.
    pub fn resize(&mut self, height_px: f64, fov_deg: f64) {
        self.scale = height_px / (2.0 * kernel::tan(fov_deg * std::f64::consts::PI / 360.0));
    }

    /// `addCar(vehicle, { player })` for a model with `exhausts` flames.
    pub fn add_car(&mut self, player: bool, exhausts: usize) -> usize {
        self.cars.push(CarFx {
            player,
            flames_on: false,
            flames: vec![1.0; exhausts],
            last_l: None,
            last_r: None,
            pool: Pool {
                sx: if player { 9.0 } else { 7.0 },
                sz: if player { 16.0 } else { 12.0 },
                ..Pool::default()
            },
            launch_due: 0.0,
        });
        self.cars.len() - 1
    }

    /// `wheelWorld(v, side)`: a rear wheel's contact point (side -1, 1; 0
    /// the middle of the rear axle).
    pub fn wheel_world(v: &CarIn, side: f64) -> P3 {
        let fx = kernel::cos(v.yaw);
        let fz = kernel::sin(v.yaw);
        let (rx, rz) = (-fz, fx);
        let back = -js::or(v.wheel_base, 2.6) / 2.0;
        let across = js::or(v.track, 1.6) / 2.0 * side;
        P3 {
            x: v.x + fx * back + rx * across,
            z: v.z + fz * back + rz * across,
            y: v.y,
        }
    }

    /// `sparksAt(x, y, z, n, vx, vz)`.
    #[allow(clippy::too_many_arguments)]
    pub fn sparks_at(&mut self, x: f64, y: f64, z: f64, n: f64, vx: f64, vz: f64) {
        let mut i = 0.0;
        while i < n {
            // The arguments' draws in the JS's order.
            let a = self.random();
            let b = self.random();
            let c = self.random();
            let d = self.random();
            let e = self.random();
            let g = self.random();
            self.sparks.emit(
                [x, y, z],
                [
                    vx * 0.6 + (a - 0.5) * 9.0,
                    2.0 + b * 5.0,
                    vz * 0.6 + (c - 0.5) * 9.0,
                ],
                0.3 + d * 0.5,
                0.18 + e * 0.12,
                -0.2,
                [1.0, 0.65 + g * 0.3, 0.25],
                1.0,
                1.5,
                14.0,
            );
            i += 1.0;
        }
    }

    /// `smokeAt(x, y, z, amount, vx, vz, night)`.
    #[allow(clippy::too_many_arguments)]
    pub fn smoke_at(&mut self, x: f64, y: f64, z: f64, amount: f64, vx: f64, vz: f64, night: f64) {
        let shade = lerp(0.85, 0.35, night);
        let a = self.random();
        let b = self.random();
        let c = self.random();
        let d = self.random();
        self.smoke.emit(
            [x, y + 0.3, z],
            [
                vx * 0.3 + (a - 0.5) * 1.5,
                0.6 + b,
                vz * 0.3 + (c - 0.5) * 1.5,
            ],
            1.2 + d * 1.0,
            0.8 + amount * 0.6,
            2.4,
            [shade; 3],
            0.07 + amount * 0.13,
            1.2,
            -0.3,
        );
    }

    /// `update(dt, night, extras)`: `cars` and `extras` per registered car,
    /// in the order they were added.
    pub fn update(&mut self, dt: f64, night: f64, cars: &[CarIn], extras: &[Extras]) {
        for (k, v) in cars.iter().enumerate().take(self.cars.len()) {
            let ex = extras.get(k).copied().unwrap_or_default();
            let visible = v.visible;
            // Nitro flames.
            let nit = ex.nitro && visible;
            let player = self.cars[k].player;
            self.cars[k].flames_on = nit;
            for fi in 0..self.cars[k].flames.len() {
                if nit {
                    let fl = 0.7 + self.random() * 0.6;
                    self.cars[k].flames[fi] = fl * if player { 1.3 } else { 1.0 };
                }
            }
            // Headlight pool on the road ahead at night.
            let pool = &mut self.cars[k].pool;
            pool.visible = visible && night > 0.2;
            if pool.visible {
                let yaw = v.yaw + js::or(v.visual_yaw, 0.0);
                let (fx, fz) = (kernel::cos(yaw), kernel::sin(yaw));
                let ahead = if player { 10.0 } else { 7.5 };
                pool.x = v.x + fx * ahead;
                pool.y = v.y + 0.06;
                pool.z = v.z + fz * ahead;
                pool.rot_y = -kernel::atan2(fz, fx) + std::f64::consts::PI / 2.0;
                // `c.pool.material.opacity = 0.35 × night`: the shared
                // material's, overwritten below (D800).
            }
            // Skids and smoke from the rear wheels.
            let skid = ex.skid;
            if skid > 0.3 && v.on_ground && visible {
                let l = Effects::wheel_world(v, -1.0);
                let r = Effects::wheel_world(v, 1.0);
                let w = clamp(skid * 0.6, 0.0, 0.55);
                if let Some(a) = self.cars[k].last_l {
                    self.skids.add(a, l, 0.26, w);
                }
                if let Some(a) = self.cars[k].last_r {
                    self.skids.add(a, r, 0.26, w);
                }
                self.cars[k].last_l = Some(l);
                self.cars[k].last_r = Some(r);
                if self.random() < rate60(skid * 0.55, dt) {
                    let src = if self.random() < 0.5 { l } else { r };
                    self.smoke_at(src.x, src.y, src.z, skid, v.vx, v.vz, night);
                }
            } else {
                self.cars[k].last_l = None;
                self.cars[k].last_r = None;
            }
            // Exhaust smoke puffs at launch: one a frame at 60 frames a
            // second.
            if ex.launch && visible {
                let due = self.cars[k].launch_due + dt * 60.0;
                let n = (due + 1e-6).floor();
                self.cars[k].launch_due = due - n;
                let w = Effects::wheel_world(v, 0.0);
                for _ in 0..n as u32 {
                    self.smoke_at(w.x, w.y, w.z, 0.2, 0.0, 0.0, night);
                }
            } else {
                self.cars[k].launch_due = 0.0;
            }
        }
        self.smoke.update(dt);
        self.sparks.update(dt);
        self.pool_opacity = 0.3 * night;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particles_fade_in_then_out_and_die() {
        let mut p = Particles::new(4);
        p.emit(
            [0.0; 3],
            [1.0, 0.0, 0.0],
            1.0,
            1.0,
            0.0,
            [1.0; 3],
            0.5,
            0.0,
            0.0,
        );
        p.update(0.05);
        assert!((p.alpha[0] - 0.25).abs() < 1e-6, "{}", p.alpha[0]);
        assert!((p.pos[0] - 0.05).abs() < 1e-6);
        for _ in 0..30 {
            p.update(0.05);
        }
        assert_eq!(p.alpha[0], 0.0);
        assert!(!p.live);
    }

    #[test]
    fn the_rings_wrap() {
        let mut p = Particles::new(3);
        for _ in 0..4 {
            p.emit([0.0; 3], [0.0; 3], 1.0, 1.0, 0.0, [1.0; 3], 1.0, 1.0, 0.0);
        }
        assert_eq!(p.next, 1);
        let mut s = SkidMarks::new(2);
        let a = P3::default();
        let b = P3 {
            x: 1.0,
            ..P3::default()
        };
        s.add(a, b, 0.26, 0.5);
        s.add(a, b, 0.26, 0.5);
        s.add(a, b, 0.26, 0.5);
        assert_eq!(s.next, 1);
        // Too short and too long segments are dropped.
        s.add(a, P3 { x: 0.01, ..a }, 0.26, 0.5);
        s.add(a, P3 { x: 5.0, ..a }, 0.26, 0.5);
        assert_eq!(s.next, 1);
        assert!(s.flush() && !s.flush());
        // The quad sits across the travel, 3 cm above the road.
        assert!((s.pos[1] - 0.03).abs() < 1e-6 && (s.pos[2] - 0.13).abs() < 1e-6);
    }

    #[test]
    fn rates_hold_at_sixty_and_scale_with_the_frame() {
        assert_eq!(rate60(0.3, 1.0 / 60.0), 0.3);
        let two = rate60(0.3, 1.0 / 120.0);
        // Two 120 Hz frames give the 60 Hz frame's chance.
        assert!((1.0 - (1.0 - two) * (1.0 - two) - 0.3).abs() < 1e-12);
        assert_eq!(rate60(1.2, 1.0 / 120.0), 1.0);
    }

    #[test]
    fn pools_follow_the_car_and_the_night() {
        let mut fx = Effects::new(1);
        fx.add_car(true, 2);
        fx.add_car(false, 0);
        let car = CarIn {
            x: 10.0,
            y: 2.0,
            z: 5.0,
            yaw: 0.0,
            visible: true,
            on_ground: true,
            ..CarIn::default()
        };
        let hidden = CarIn {
            visible: false,
            ..car
        };
        fx.update(1.0 / 60.0, 0.1, &[car, car], &[]);
        assert!(!fx.cars[0].pool.visible);
        fx.update(1.0 / 60.0, 1.0, &[car, hidden], &[]);
        let p = fx.cars[0].pool;
        assert!(p.visible && !fx.cars[1].pool.visible);
        assert_eq!((p.x, p.y, p.z), (20.0, 2.06, 5.0));
        assert!((p.rot_y - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert!((fx.pool_opacity - 0.3).abs() < 1e-12);
    }

    #[test]
    fn a_skid_lays_marks_and_smoke_and_nitro_lights_flames() {
        let mut fx = Effects::new(7);
        fx.add_car(true, 2);
        let ex = [Extras {
            nitro: true,
            skid: 1.0,
            launch: false,
        }];
        let mut smoked = false;
        for i in 0..60 {
            let car = CarIn {
                x: i as f64 * 0.4,
                visible: true,
                on_ground: true,
                wheel_base: 2.5,
                track: 1.5,
                ..CarIn::default()
            };
            fx.update(1.0 / 60.0, 1.0, &[car], &ex);
            smoked |= fx.smoke.live;
        }
        assert!(smoked);
        assert_eq!(fx.skids.next, 2 * 59);
        assert!(fx.cars[0].flames_on);
        assert!(
            fx.cars[0]
                .flames
                .iter()
                .all(|&l| (0.91..=1.69).contains(&l))
        );
    }
}
