//! Birds: flocks circling over anchors along every level, chosen by the
//! zone's scenery (gulls on the coast and at the raceway, raptors and small
//! birds over the mountains and valley, ravens in the desert, pigeons in
//! the city), and on the Coast Highway a V of pelicans flying along over
//! the sea beside the road. They roost at night.
//!
//! One pool of three instanced meshes (body, right wing, left wing) serves
//! the whole level: each frame the flocks near the camera fill it, up to
//! [`POOL`] birds, and the count is set to what was filled.

use std::f64::consts::PI;

use mp_math::{Mulberry32, kernel, rrange, smoothstep};

use super::{Y, Z, dist2d, ground, heading, instance_count, instance_matrix, rot, v3};
use crate::builder::Builder;
use crate::color::Color;
use crate::material::Material;
use crate::object::NodeId;
use crate::three_geom::{Matrix4, Quaternion, Vector3};
use crate::world::{UpdateCtx, World};

/// Birds drawn at once, at most.
pub const POOL: usize = 160;
/// Flocks farther than this from the camera are not drawn.
const NEAR: f64 = 950.0;

/// A kind of bird: its size, colour and how it flies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Species {
    pub name: &'static str,
    /// Body length, m.
    pub length: f64,
    /// One wing's length, m.
    pub span: f64,
    pub color: u32,
    /// Wingbeats per second.
    pub flap_hz: f64,
    /// Share of the time spent gliding (wings held out).
    pub glide: f64,
    /// Air speed, m/s.
    pub speed: f64,
    /// Flock size (min, max).
    pub flock: (u32, u32),
    /// Height above the ground, m (min, max).
    pub height: (f64, f64),
    /// Circle radius, m (min, max).
    pub radius: (f64, f64),
}

pub const GULL: Species = Species {
    name: "gull",
    length: 0.42,
    span: 0.52,
    color: 0xf1f0ea,
    flap_hz: 2.8,
    glide: 0.55,
    speed: 9.0,
    flock: (4, 9),
    height: (14.0, 36.0),
    radius: (14.0, 34.0),
};
pub const PELICAN: Species = Species {
    name: "pelican",
    length: 1.15,
    span: 1.1,
    color: 0x8b8277,
    flap_hz: 1.35,
    glide: 0.78,
    speed: 13.0,
    flock: (6, 8),
    height: (7.0, 11.0),
    radius: (0.0, 0.0),
};
pub const RAPTOR: Species = Species {
    name: "raptor",
    length: 0.58,
    span: 0.72,
    color: 0x4b3a2a,
    flap_hz: 1.6,
    glide: 0.9,
    speed: 8.0,
    flock: (1, 2),
    height: (45.0, 90.0),
    radius: (35.0, 70.0),
};
pub const SMALL: Species = Species {
    name: "small bird",
    length: 0.15,
    span: 0.13,
    color: 0x3b332b,
    flap_hz: 8.0,
    glide: 0.2,
    speed: 11.0,
    flock: (7, 14),
    height: (6.0, 14.0),
    radius: (8.0, 18.0),
};
pub const RAVEN: Species = Species {
    name: "raven",
    length: 0.55,
    span: 0.52,
    color: 0x17171a,
    flap_hz: 2.2,
    glide: 0.65,
    speed: 9.0,
    flock: (2, 5),
    height: (25.0, 60.0),
    radius: (20.0, 45.0),
};
pub const PIGEON: Species = Species {
    name: "pigeon",
    length: 0.32,
    span: 0.33,
    color: 0x8a8e98,
    flap_hz: 4.5,
    glide: 0.25,
    speed: 10.0,
    flock: (8, 16),
    height: (18.0, 40.0),
    radius: (12.0, 26.0),
};

/// The species a zone's scenery has, and how much each is picked.
fn mix(scenery: &str) -> &'static [(Species, f64)] {
    match scenery {
        "Coast" | "Beach" | "Harbor" => &[(GULL, 0.8), (SMALL, 0.2)],
        "Raceway" => &[(GULL, 0.55), (SMALL, 0.35), (RAPTOR, 0.1)],
        "Mountain" => &[(RAPTOR, 0.45), (SMALL, 0.35), (RAVEN, 0.2)],
        "Valley" => &[(SMALL, 0.55), (RAPTOR, 0.25), (RAVEN, 0.2)],
        "Desert" => &[(RAVEN, 0.55), (RAPTOR, 0.45)],
        "City" | "Streets" => &[(PIGEON, 0.85), (GULL, 0.15)],
        _ => &[(SMALL, 0.6), (RAPTOR, 0.4)],
    }
}

fn pick(rng: &mut Mulberry32, m: &[(Species, f64)]) -> Species {
    let total: f64 = m.iter().map(|(_, w)| w).sum();
    let mut u = rng.next_f64() * total;
    for (s, w) in m {
        if u < *w {
            return *s;
        }
        u -= w;
    }
    m[m.len() - 1].0
}

/// One bird's place in its flock.
#[derive(Clone, Copy, Debug)]
struct Member {
    phase: f64,
    r: f64,
    dy: f64,
    flap: f64,
}

#[derive(Clone, Debug)]
enum Path {
    /// Circling over a point (radius per member).
    Circle { c: [f64; 3], dir: f64 },
    /// Flying along the road over the sea (the pelicans): points every
    /// [`CONVOY_STEP`] m from `s0`, and where the V is along them.
    Convoy {
        pts: Vec<[f64; 3]>,
        s0: f64,
        p: Option<f64>,
    },
}

const CONVOY_STEP: f64 = 4.0;

#[derive(Clone, Debug)]
struct Flock {
    sp: Species,
    path: Path,
    /// Along the track, for when there is no camera.
    s: f64,
    members: Vec<Member>,
}

/// The level's birds and their three instanced meshes.
pub struct Birds {
    flocks: Vec<Flock>,
    body: NodeId,
    wing_r: NodeId,
    wing_l: NodeId,
    length: f64,
    /// Drawn last frame (to clear the count once at night).
    drawn: usize,
    /// The colour each pool slot was last given (slots change species as
    /// flocks come and go).
    colors: Vec<u32>,
}

pub fn build(w: &mut World, level: &str) -> Result<Option<Birds>, String> {
    let Some(t) = w.track.as_ref() else {
        return Ok(None);
    };
    let length = t.length;
    let mut rng = Mulberry32::new(0x5eed_b1d5 ^ level.len() as u32);
    let sea = w.level.sea_y;
    let mut flocks = Vec::new();
    // Circling flocks every ~380 m, on alternating sides (the sea side on
    // the coast), some way off the road.
    let spacing = if t.is_loop { 300.0 } else { 380.0 };
    let mut s = 120.0;
    while s < t.length - 60.0 {
        let f = t.frame(s);
        let scenery = w.level.zones.get(f.zone as usize).map_or("", |z| z.scenery);
        let sp = pick(&mut rng, mix(scenery));
        let coastal = matches!(scenery, "Coast" | "Beach" | "Harbor");
        // Over the sea (the left) three times in four on the coast.
        let sea_side = if coastal { 0.75 } else { 0.5 };
        let side = if rng.next_f64() < sea_side { -1.0 } else { 1.0 };
        let wall = if side < 0.0 { f.wall_l } else { f.wall_r };
        let lat = side * (wall + rrange(&mut rng, 25.0, 120.0));
        let p = t.point_at(s, lat);
        let mut gy = ground(w, p.x, p.z, f.y);
        if let Some(sy) = sea {
            gy = gy.max(sy);
        }
        let y = gy.max(f.y) + rrange(&mut rng, sp.height.0, sp.height.1);
        let n = sp.flock.0 + (rng.next_f64() * f64::from(sp.flock.1 - sp.flock.0 + 1)) as u32;
        let r = rrange(&mut rng, sp.radius.0, sp.radius.1);
        let members = (0..n)
            .map(|_| Member {
                phase: rng.next_f64() * PI * 2.0,
                r: r * rrange(&mut rng, 0.7, 1.25),
                dy: rrange(&mut rng, -3.0, 3.0),
                flap: rng.next_f64(),
            })
            .collect();
        flocks.push(Flock {
            sp,
            path: Path::Circle {
                c: [p.x, y, p.z],
                dir: if rng.next_f64() < 0.5 { -1.0 } else { 1.0 },
            },
            s,
            members,
        });
        s += spacing * rrange(&mut rng, 0.8, 1.2);
    }
    // The pelicans: along the coast road where the sea is on the left.
    if let Some(sy) = sea {
        let zones: Vec<usize> = w
            .level
            .zones
            .iter()
            .enumerate()
            .filter(|(_, z)| matches!(z.scenery, "Coast" | "Beach"))
            .map(|(i, _)| i)
            .collect();
        if let (Some(&a), Some(&b)) = (zones.first(), zones.last()) {
            let s0 = t.zone_start[a] as f64 + 150.0;
            let s1 = if b + 1 < t.zone_start.len() {
                t.zone_start[b + 1] as f64
            } else {
                t.length
            } - 100.0;
            let mut pts = Vec::new();
            let mut s = s0;
            while s < s1 {
                let f = t.frame(s);
                let p = t.point_at(s, -(f.wall_l + 34.0));
                let y = sy.max(ground(w, p.x, p.z, sy)) + 9.0;
                pts.push([p.x, y, p.z]);
                s += CONVOY_STEP;
            }
            if pts.len() > 100 {
                let members = (0..7)
                    .map(|k| {
                        // A V: the leader, then pairs behind on either side.
                        let rank = f64::from((k + 1) / 2);
                        let side = if k % 2 == 0 { 1.0 } else { -1.0 };
                        Member {
                            phase: rng.next_f64() * PI * 2.0,
                            r: rank * 3.6,
                            dy: side * rank * 2.9,
                            flap: rng.next_f64(),
                        }
                    })
                    .collect();
                flocks.push(Flock {
                    sp: PELICAN,
                    path: Path::Convoy { pts, s0, p: None },
                    s: s0,
                    members,
                });
            }
        }
    }
    if flocks.is_empty() {
        return Ok(None);
    }

    // The meshes: a body along +z, and a wing on either side from the
    // shoulder, each 1 long (scaled per species).
    let mut b = Builder::new();
    b.cbox("b", 0.2, 0.15, 0.75, 0.0, 0.0, 0.0, [0.0; 3]);
    b.cbox("b", 0.13, 0.12, 0.2, 0.0, 0.03, 0.45, [0.0; 3]);
    b.cbox("b", 0.22, 0.03, 0.26, 0.0, 0.01, -0.5, [0.0; 3]);
    let body_geo = b.merge_all().ok_or("the bird's body")?;
    let mut b = Builder::new();
    b.cbox("w", 1.0, 0.025, 0.42, 0.5, 0.0, 0.0, [0.0; 3]);
    let wing_r_geo = b.merge_all().ok_or("a wing")?;
    let mut b = Builder::new();
    b.cbox("w", 1.0, 0.025, 0.42, -0.5, 0.0, 0.0, [0.0; 3]);
    let wing_l_geo = b.merge_all().ok_or("a wing")?;
    let mat = w.graph.add_material(
        Material::standard()
            .set("color", 0xffffff)
            .set("roughness", 0.85),
    );
    let mut node = |geo| {
        let g = w.graph.add_geometry(geo);
        let n = w.graph.instanced_mesh(g, mat, POOL as u32);
        let o = w.graph.get_mut(n);
        o.frustum_culled = false;
        o.cast_shadow = false;
        o.receive_shadow = false;
        o.instances.as_mut().expect("instanced").count = 0;
        n
    };
    let body = node(body_geo);
    let wing_r = node(wing_r_geo);
    let wing_l = node(wing_l_geo);
    let group = w.graph.group("life:birds");
    for n in [body, wing_r, wing_l] {
        w.graph.add(group, n);
    }
    let root = w.root;
    w.graph.add(root, group);
    Ok(Some(Birds {
        flocks,
        body,
        wing_r,
        wing_l,
        length,
        drawn: 0,
        colors: vec![u32::MAX; POOL],
    }))
}

impl Birds {
    pub fn update(
        &mut self,
        time: f64,
        u: &UpdateCtx,
        cam: Option<[f64; 3]>,
        out: &mut Vec<crate::world::Edit>,
    ) {
        // Roosting: none at night.
        if u.night > 0.8 {
            if self.drawn > 0 {
                for n in [self.body, self.wing_r, self.wing_l] {
                    out.push(instance_count(n, 0));
                }
                self.drawn = 0;
            }
            return;
        }
        let mut i = 0;
        let len = self.length;
        for f in &mut self.flocks {
            if i >= POOL {
                break;
            }
            let poses = match &mut f.path {
                Path::Circle { c, dir } => {
                    let near = cam.map_or_else(
                        || {
                            let d = (f.s - u.s).rem_euclid(len);
                            d.min(len - d) < NEAR
                        },
                        |p| dist2d(p, *c) < NEAR,
                    );
                    if !near {
                        continue;
                    }
                    circle_poses(&f.sp, &f.members, *c, *dir, time)
                }
                Path::Convoy { pts, s0, p } => match convoy_step(pts, *s0, p, u) {
                    Some(at) => convoy_poses(&f.sp, &f.members, pts, at),
                    None => continue,
                },
            };
            for (m, (pos, yaw, bank)) in f.members.iter().zip(poses) {
                if i >= POOL {
                    break;
                }
                if self.colors[i] != f.sp.color {
                    self.colors[i] = f.sp.color;
                    let c = Color::hex(f.sp.color);
                    let rgb = [c.r as f32, c.g as f32, c.b as f32];
                    for n in [self.body, self.wing_r, self.wing_l] {
                        out.push(crate::world::Edit {
                            target: crate::world::Handle::Node(n),
                            change: crate::world::Change::InstanceColor {
                                index: i as u32,
                                rgb,
                            },
                        });
                    }
                }
                let flap = flap_angle(&f.sp, m, time);
                let [b, r, l] = bird_matrices(&f.sp, pos, yaw, bank, flap);
                out.push(instance_matrix(self.body, i, &b));
                out.push(instance_matrix(self.wing_r, i, &r));
                out.push(instance_matrix(self.wing_l, i, &l));
                i += 1;
            }
        }
        if i != self.drawn || i > 0 {
            for n in [self.body, self.wing_r, self.wing_l] {
                out.push(instance_count(n, i));
            }
        }
        self.drawn = i;
    }
}

/// Where each member of a circling flock is: position, heading, bank.
fn circle_poses(
    sp: &Species,
    members: &[Member],
    c: [f64; 3],
    dir: f64,
    t: f64,
) -> Vec<([f64; 3], f64, f64)> {
    members
        .iter()
        .map(|m| {
            // Small birds tighten and widen their circle as they go.
            let r = if sp.name == "small bird" {
                m.r * (0.75 + 0.25 * kernel::sin(0.6 * t + m.phase))
            } else {
                m.r
            };
            let a = m.phase + dir * (sp.speed / r.max(1.0)) * t;
            let (sa, ca) = (kernel::sin(a), kernel::cos(a));
            let pos = [
                c[0] + r * ca,
                c[1] + m.dy + 1.5 * kernel::sin(0.35 * t + m.phase),
                c[2] + r * sa,
            ];
            // Moving along the circle: d/da (cos a, sin a) times dir.
            let yaw = heading(-sa * dir, ca * dir);
            let bank = -dir * kernel::atan(sp.speed * sp.speed / (r.max(1.0) * 9.81)).min(0.6);
            (pos, yaw, bank)
        })
        .collect()
}

/// Moves the pelicans' V along the coast. They fly at their own speed;
/// when the player has left them far behind (or not reached them yet),
/// they set off again ahead of the player, so a drive along the coast
/// keeps meeting them. `None` when the player is off this stretch.
fn convoy_step(pts: &[[f64; 3]], s0: f64, p: &mut Option<f64>, u: &UpdateCtx) -> Option<f64> {
    let span = (pts.len() - 1) as f64 * CONVOY_STEP;
    let me = u.s - s0;
    if me < -400.0 || me > span + 200.0 {
        *p = None;
        return None;
    }
    let at = match *p {
        Some(at) if at > me - 450.0 && at < me + 1200.0 && at < span - 10.0 => {
            at + PELICAN.speed * u.dt
        }
        _ => (me + 650.0).clamp(0.0, (span - 300.0).max(0.0)),
    };
    *p = Some(at);
    Some(at)
}

fn convoy_point(pts: &[[f64; 3]], at: f64) -> ([f64; 3], [f64; 3]) {
    let k = (at / CONVOY_STEP).max(0.0);
    let i = (k as usize).min(pts.len() - 2);
    let f = (k - i as f64).clamp(0.0, 1.0);
    let (a, b) = (pts[i], pts[i + 1]);
    let p = [
        a[0] + (b[0] - a[0]) * f,
        a[1] + (b[1] - a[1]) * f,
        a[2] + (b[2] - a[2]) * f,
    ];
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    (p, d)
}

fn convoy_poses(
    sp: &Species,
    members: &[Member],
    pts: &[[f64; 3]],
    at: f64,
) -> Vec<([f64; 3], f64, f64)> {
    let _ = sp;
    members
        .iter()
        .map(|m| {
            // r: how far back in the V; dy: how far to the side.
            let (p, d) = convoy_point(pts, at - m.r);
            let n = (d[0] * d[0] + d[2] * d[2]).sqrt().max(1e-9);
            let (dx, dz) = (d[0] / n, d[2] / n);
            let pos = [
                p[0] + dz * m.dy,
                p[1] + 0.6 * kernel::sin(at * 0.05 + m.phase),
                p[2] - dx * m.dy,
            ];
            (pos, heading(dx, dz), 0.0)
        })
        .collect()
}

/// The wings' angle: beating, or held out in a glide.
fn flap_angle(sp: &Species, m: &Member, t: f64) -> f64 {
    // A four-second cycle: a glide, then a run of beats, eased in and
    // out over a few tenths of a second.
    let cycle = (t / 4.0 + m.flap).rem_euclid(1.0);
    let amp = smoothstep(sp.glide, sp.glide + 0.05, cycle) * (1.0 - smoothstep(0.95, 1.0, cycle));
    0.1 + amp * 0.55 * kernel::sin(2.0 * PI * sp.flap_hz * t + m.phase)
}

/// The body's and the two wings' matrices.
fn bird_matrices(sp: &Species, pos: [f64; 3], yaw: f64, bank: f64, flap: f64) -> [Matrix4; 3] {
    let q: Quaternion = rot(Y, yaw).multiply(rot(Z, bank));
    let at = v3(pos[0], pos[1], pos[2]);
    let l = sp.length;
    let body = Matrix4::compose(at, q, Vector3::splat(l));
    let frame = Matrix4::compose(at, q, Vector3::splat(1.0));
    let wing = |side: f64| {
        frame.multiply(&Matrix4::compose(
            v3(side * 0.08 * l, 0.03 * l, 0.06 * l),
            rot(Z, side * flap),
            v3(sp.span, l, l),
        ))
    };
    [body, wing(1.0), wing(-1.0)]
}
