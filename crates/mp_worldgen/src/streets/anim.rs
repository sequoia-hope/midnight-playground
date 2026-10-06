//! The closures Streets pushes onto `world.updaters`, as [`Animator`]s in
//! the JS order: the neon clock (`materials()`), the traffic signals
//! (`buildSignals`, when there are any), the barrier flashers
//! (`buildBarriers`), the crowd's phone flashes (`buildCrowds`), the
//! elevated train (`buildElevated`), the steam clock and point scale
//! (`buildSteam`) and the aircraft warning lights (`buildAircraftLights`).

use std::f64::consts::PI;

use mp_math::{js, kernel};

use super::dressing::SignalLamp;
use super::ground;
use crate::object::{GeoId, MaterialId, NodeId};
use crate::world::{Animator, Change, Edit, Handle, UpdateCtx};

/// `(dt) => { this.neonTime.value += dt; }`: the one `uNTime` uniform both
/// neon materials share; the port writes it into each.
pub struct NeonClock {
    pub time: f64,
    pub mats: [MaterialId; 2],
}

impl Animator for NeonClock {
    fn update(&mut self, u: &UpdateCtx, out: &mut Vec<Edit>) {
        self.time += u.dt;
        for m in self.mats {
            out.push(Edit {
                target: Handle::Material(m),
                change: Change::Number {
                    prop: "uNTime",
                    value: self.time,
                },
            });
        }
    }
}

/// The traffic signals' lenses: amber flashing where the race runs, a
/// red/green cycle elsewhere; the `color` attribute is rewritten when
/// either phase changes.
pub struct Signals {
    pub time: f64,
    pub last_a: f64,
    pub last_p: f64,
    pub lamps: Vec<SignalLamp>,
    pub geo: GeoId,
}

impl Animator for Signals {
    fn update(&mut self, u: &UpdateCtx, out: &mut Vec<Edit>) {
        self.time += u.dt;
        let amber = (self.time * 1.4).floor() % 2.0;
        let phase = (self.time / 9.0).floor() % 2.0;
        if amber == self.last_a && phase == self.last_p {
            return;
        }
        self.last_a = amber;
        self.last_p = phase;
        let mut values = Vec::with_capacity(self.lamps.len() * 3);
        for l in &self.lamps {
            let c: [f32; 3] = if l.flash {
                if amber != 0.0 {
                    [3.4, 1.7, 0.1]
                } else {
                    [0.25, 0.12, 0.01]
                }
            } else if l.phase == phase {
                [0.2, 3.0, 1.2]
            } else {
                [3.4, 0.25, 0.15]
            };
            values.extend_from_slice(&c);
        }
        out.push(Edit {
            target: Handle::Geometry(self.geo),
            change: Change::Attribute {
                name: "color",
                offset: 0,
                values,
            },
        });
    }
}

/// The barriers' amber flashers.
pub struct Blink {
    pub time: f64,
    pub mat: MaterialId,
}

impl Animator for Blink {
    fn update(&mut self, u: &UpdateCtx, out: &mut Vec<Edit>) {
        self.time += u.dt;
        let on = if kernel::sin(self.time * 7.0) > 0.0 {
            1.0
        } else {
            0.15
        };
        out.push(Edit {
            target: Handle::Material(self.mat),
            change: Change::Color {
                prop: "color",
                rgb: [3.5 * on, 1.8 * on, 0.2 * on],
            },
        });
    }
}

/// Phone cameras twinkling in the crowd: the points' size.
pub struct Flashes {
    pub time: f64,
    pub mat: MaterialId,
}

impl Animator for Flashes {
    fn update(&mut self, u: &UpdateCtx, out: &mut Vec<Edit>) {
        self.time += u.dt;
        out.push(Edit {
            target: Handle::Material(self.mat),
            change: Change::Number {
                prop: "size",
                value: 0.25 + 0.2 * kernel::sin(self.time * 3.1).abs(),
            },
        });
    }
}

/// The elevated train: scripted near the crossing, so it meets the player
/// overhead; elsewhere it runs on at 16 m/s and wraps.
pub struct Train {
    pub node: NodeId,
    /// `train.position.x`.
    pub x: f64,
    /// The viaduct's centre line (`deckY` reads the ground there).
    pub deck_x: f64,
    pub z0: f64,
    pub z1: f64,
    pub len: f64,
    pub s_cross: f64,
    pub zc: f64,
    pub zt: f64,
    pub free: bool,
}

impl Animator for Train {
    fn update(&mut self, u: &UpdateCtx, out: &mut Vec<Edit>) {
        // Scripted near the crossing: the train meets the player overhead.
        let ds = u.s - self.s_cross;
        if ds > -380.0 && ds < 160.0 {
            self.zt = self.zc - self.len * 0.55 + ds * 0.85;
            self.free = false;
        } else {
            if !self.free {
                self.free = true;
            }
            self.zt += 16.0 * u.dt;
            if self.zt > self.z1 {
                self.zt = self.z0 - self.len;
            }
        }
        let zz = js::min(self.z1, js::max(self.z0, self.zt + self.len / 2.0));
        let y = ground(self.deck_x, zz) + 8.5 - 0.2;
        out.push(Edit {
            target: Handle::Node(self.node),
            change: Change::Transform {
                position: [self.x, y, self.zt],
                quaternion: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0, 1.0, 1.0],
            },
        });
    }
}

/// The steam's clock, and its point scale from the viewport height.
pub struct Steam {
    pub time: f64,
    pub mat: MaterialId,
}

impl Animator for Steam {
    fn update(&mut self, u: &UpdateCtx, out: &mut Vec<Edit>) {
        self.time += u.dt;
        out.push(Edit {
            target: Handle::Material(self.mat),
            change: Change::Number {
                prop: "uTime",
                value: self.time,
            },
        });
        // Point size in pixels needs the viewport height.
        if let Some(cam) = &u.camera {
            let h = js::or(cam.viewport_height, 720.0);
            let fov = js::or(cam.fov, 60.0);
            out.push(Edit {
                target: Handle::Material(self.mat),
                change: Change::Number {
                    prop: "uScale",
                    value: 0.5 * h / kernel::tan(fov * PI / 360.0),
                },
            });
        }
    }
}

/// The aircraft warning lights on the tall towers: a slow blink.
pub struct Aircraft {
    pub time: f64,
    pub mat: MaterialId,
}

impl Animator for Aircraft {
    fn update(&mut self, u: &UpdateCtx, out: &mut Vec<Edit>) {
        self.time += u.dt;
        let on = if kernel::sin(self.time * PI * 1.6) > 0.2 {
            1.0
        } else {
            0.08
        };
        out.push(Edit {
            target: Handle::Material(self.mat),
            change: Change::Color {
                prop: "color",
                rgb: [6.0 * on, 0.35 * on, 0.25 * on],
            },
        });
    }
}
