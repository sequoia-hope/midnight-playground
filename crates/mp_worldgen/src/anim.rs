//! Animators that are data (DECISIONS D1215). An [`Animator`] is a closure
//! of `world.updaters`, which cannot leave the thread that built it; a
//! streamed section built in the web's worker (stage 4c) hands its scene
//! over as bytes, so its animators are described here as plain data,
//! serialised beside the scene (`section::pack`) and made into animators
//! again on the other side. The open world's kits (the coast, the beach)
//! register theirs this way; a kit that pushes a closure still animates
//! wherever it is built in the client's own thread, and loses that
//! animator in the worker (the packed log says so).
//!
//! Each variant keeps the closure's state (its clock, its angle) with its
//! handles, so an animator read back carries on where the one written
//! stopped; one freshly built starts at zero, as the closure did.

use serde::{Deserialize, Serialize};

use crate::object::{MaterialId, NodeId};
use crate::three_geom::{Euler, Quaternion};
use crate::world::{Animator, Change, Edit, Handle, UpdateCtx};
use mp_math::{lerp, smoothstep};

/// One animator as data.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AnimatorDesc {
    /// Dew on the tarmac after dark (the road's `setNight`): `uWet` is
    /// `smoothstep(0.55, 1, night) * 0.85`; lamps and headlights glint.
    Dew { mats: Vec<MaterialId> },
    /// The coast's surf and foam: `uTime` runs on one clock and `uBright`
    /// dims to 0.32 at night.
    Surf { mats: Vec<MaterialId>, time: f64 },
    /// The lighthouse: the beam's pivot turns at 0.9 rad/s about `pos`,
    /// the beam's `uStrength` and the glow's opacity and size follow
    /// nightfall.
    Lighthouse {
        pivot: NodeId,
        pos: [f64; 3],
        beam_mat: MaterialId,
        glow_mat: MaterialId,
        glow: NodeId,
        ry: f64,
    },
    /// A material's colour at `rgb * (a + b * night)` (the coffee shack's
    /// string lights, the reflectors).
    NightColor {
        mat: MaterialId,
        rgb: [f64; 3],
        a: f64,
        b: f64,
    },
    /// The fishing boats offshore and their running lights.
    Boats(crate::coast::Boats),
    /// Seabright's Ferris wheel, coaster, surf, light pools and signals.
    Beach(crate::beach::Animate),
}

impl AnimatorDesc {
    /// The animator this describes.
    pub fn into_animator(self) -> Box<dyn Animator> {
        Box::new(self)
    }
}

impl Animator for AnimatorDesc {
    fn update(&mut self, u: &UpdateCtx, out: &mut Vec<Edit>) {
        match self {
            AnimatorDesc::Dew { mats } => {
                let v = smoothstep(0.55, 1.0, u.night) * 0.85;
                for &m in mats.iter() {
                    out.push(Edit {
                        target: Handle::Material(m),
                        change: Change::Number {
                            prop: "uWet",
                            value: v,
                        },
                    });
                }
            }
            AnimatorDesc::Surf { mats, time } => {
                *time += u.dt;
                for &m in mats.iter() {
                    out.push(Edit {
                        target: Handle::Material(m),
                        change: Change::Number {
                            prop: "uTime",
                            value: *time,
                        },
                    });
                    out.push(Edit {
                        target: Handle::Material(m),
                        change: Change::Number {
                            prop: "uBright",
                            value: lerp(1.0, 0.32, u.night),
                        },
                    });
                }
            }
            AnimatorDesc::Lighthouse {
                pivot,
                pos,
                beam_mat,
                glow_mat,
                glow,
                ry,
            } => {
                *ry += u.dt * 0.9;
                let q = Quaternion::from_euler(&Euler::new(0.0, *ry, 0.0));
                out.push(Edit {
                    target: Handle::Node(*pivot),
                    change: Change::Transform {
                        position: *pos,
                        quaternion: [q.x, q.y, q.z, q.w],
                        scale: [1.0, 1.0, 1.0],
                    },
                });
                out.push(Edit {
                    target: Handle::Material(*beam_mat),
                    change: Change::Number {
                        prop: "uStrength",
                        value: 0.03 + 0.32 * smoothstep(0.2, 0.8, u.night),
                    },
                });
                out.push(Edit {
                    target: Handle::Material(*glow_mat),
                    change: Change::Number {
                        prop: "opacity",
                        value: 0.25 + 0.75 * u.night,
                    },
                });
                let sc = 6.0 + 8.0 * u.night;
                out.push(Edit {
                    target: Handle::Node(*glow),
                    change: Change::Transform {
                        position: *pos,
                        quaternion: [0.0, 0.0, 0.0, 1.0],
                        scale: [sc, sc, sc],
                    },
                });
            }
            AnimatorDesc::NightColor { mat, rgb, a, b } => {
                let k = *a + *b * u.night;
                out.push(Edit {
                    target: Handle::Material(*mat),
                    change: Change::Color {
                        prop: "color",
                        rgb: [rgb[0] * k, rgb[1] * k, rgb[2] * k],
                    },
                });
            }
            AnimatorDesc::Boats(b) => b.update(u, out),
            AnimatorDesc::Beach(b) => b.update(u, out),
        }
    }
}

/// Registers a data animator with a build: it runs as an animator, and
/// its description is kept for a packed section.
pub fn push(
    animators: &mut Vec<Box<dyn Animator>>,
    descs: &mut Vec<AnimatorDesc>,
    d: AnimatorDesc,
) {
    animators.push(Box::new(d.clone()));
    descs.push(d);
}
