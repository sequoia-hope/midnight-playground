//! Port of `src/world/Valley.js` and `src/world/valley/*` (roadmap WP 3.7):
//! Old Mill Valley, zone 1 of Sierra.
//!
//! Farm country between the pass and the city. Weathered farmsteads set
//! back from the road, orchards and windbreaks, hay in the fields,
//! telephone poles along the verge, a creek (and the old mill it is named
//! for) running under the road, and a general store at a crossroads.
//!
//! `valley/Builder.js` is [`crate::builder`] (WP 3.3) and `valley/flora.js`
//! is [`crate::flora`] (WP 3.6); `valley/ground.js` is [`ground`],
//! `valley/parts.js` [`parts`], and the canvas textures [`textures`].
//!
//! The module keeps the JS structure: [`Valley::plan`] runs before the
//! terrain's fields are built (the creek, the farm, store, mill and
//! windmill sites, their pads and fence gaps, the creek's carve);
//! [`Valley::build`] makes the group `valley` after the road, with the
//! night parameters and the updater (the windpump wheels, the waterwheel,
//! the sails, the creek's ripples and tint) as an [`Animator`].

// Index loops stay index loops, and the JS argument lists stay (DECISIONS
// D52, D130); the JS writes 6.28 where it means a turn, and so does the port.
#![allow(
    clippy::needless_range_loop,
    clippy::too_many_arguments,
    clippy::approx_constant
)]

pub mod ground;
pub mod parts;
pub mod textures;

use std::collections::BTreeMap;
use std::f64::consts::PI;
use std::sync::Arc;

use mr_math::{
    Mulberry32, Noise2D, clamp, fbm, hash2, js, kernel, lerp, ridged, rrange, smoothstep,
};
use mr_scene::{MaterialKind, NodeType, three};
use mr_track::{FenceGap, Track};
use serde_json::{Value, json};

use crate::builder::{BuildOpts, Builder, CastShadow};
use crate::color::Color;
use crate::flora::{
    canopy_geometry, flower_geometry, foliage_material, grass_clump_geometry, shrub_geometry,
};
use crate::material::{Material, Param};
use crate::object::{Image, MaterialId, NodeId, SceneGraph, TextureId};
use crate::sky::SkyParams;
use crate::terrain::Terrain;
use crate::textures::Texture;
use crate::three_geom::{
    BufferAttribute, BufferGeometry, Euler, EulerOrder, Matrix4, Quaternion, Vector2, Vector3,
    circle_geometry, cylinder_geometry, lathe_geometry, plane_geometry, torus_geometry,
};
use crate::world::{Change, Edit, Handle, Scenery, SceneryInfo, UpdateCtx, World};

use ground::Ground;
use textures::{WoodSign, corn_texture, neon_texture, water_normal_texture, wood_sign};

/// local +Z → (dx, dz)
fn yaw_z(dx: f64, dz: f64) -> f64 {
    kernel::atan2(dx, dz)
}

/// local +X → (dx, dz)
fn yaw_x(dx: f64, dz: f64) -> f64 {
    kernel::atan2(-dz, dx)
}

// Same parcel grid as TerrainMesh's field colours, so bales land in hay.
const FIELD_A: f64 = 0.38;
const FIELD_U: f64 = 110.0;
const FIELD_V: f64 = 160.0;

fn to_uv(x: f64, z: f64) -> [f64; 2] {
    [
        x * kernel::cos(FIELD_A) - z * kernel::sin(FIELD_A),
        x * kernel::sin(FIELD_A) + z * kernel::cos(FIELD_A),
    ]
}

fn from_uv(u: f64, v: f64) -> [f64; 2] {
    [
        u * kernel::cos(FIELD_A) + v * kernel::sin(FIELD_A),
        -u * kernel::sin(FIELD_A) + v * kernel::cos(FIELD_A),
    ]
}

/// A point of a polyline (`{x, z}`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct P {
    pub x: f64,
    pub z: f64,
}

/// `segDist(px, pz, pts)`: `{ d, i, t }`.
fn seg_dist(px: f64, pz: f64, pts: &[P]) -> (f64, usize, f64) {
    let mut best = f64::INFINITY;
    let mut bi = 0;
    let mut bt = 0.0;
    for i in 0..pts.len().saturating_sub(1) {
        let (a, b) = (pts[i], pts[i + 1]);
        let abx = b.x - a.x;
        let abz = b.z - a.z;
        let t = clamp(
            ((px - a.x) * abx + (pz - a.z) * abz) / js::or(abx * abx + abz * abz, 1.0),
            0.0,
            1.0,
        );
        let dx = px - (a.x + abx * t);
        let dz = pz - (a.z + abz * t);
        let d = dx * dx + dz * dz;
        if d < best {
            best = d;
            bi = i;
            bt = t;
        }
    }
    (best.sqrt(), bi, bt)
}

/// A float read from a typed array.
fn at(a: &[f32], i: usize) -> f64 {
    f64::from(a[i])
}

// ── Plan records ────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pad {
    pub x: f64,
    pub z: f64,
    pub r: f64,
    pub falloff: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Farm {
    pub s: f64,
    pub side: f64,
    pub lat: f64,
    pub x: f64,
    pub z: f64,
    pub yaw: f64,
    pub y: f64,
    pub seed: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StoreSite {
    pub s: f64,
    pub side: f64,
    pub x: f64,
    pub z: f64,
    pub yaw: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MillSite {
    pub x: f64,
    pub z: f64,
    pub i: usize,
    pub nx: f64,
    pub nz: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindmillSite {
    pub s: f64,
    pub side: f64,
    pub x: f64,
    pub z: f64,
    pub y: f64,
    pub yaw: f64,
}

/// Where the page's `Math.random` stood (draws from the scene export's
/// seed, 0x5eed) when each wood sign drew its plank noise: the valley
/// sign, the store's, the mill's. Measured from the export's pixels
/// (DECISIONS D332).
pub const SIGN_RANDOM_AT: [u64; 3] = [11880, 12132, 19468];

/// The scene export's `Math.random` seed (`tools/parity/lib/seed-random.mjs`).
pub const PAGE_RANDOM_SEED: u32 = 0x5eed;

/// The page's `Math.random` after `n` draws.
pub fn page_random(n: u64) -> Mulberry32 {
    let mut r = Mulberry32::new(PAGE_RANDOM_SEED);
    for _ in 0..n {
        r.next_f64();
    }
    r
}

/// `new Valley({ zone, key, level })`, planned and then built.
pub struct Valley {
    pub label: &'static str,
    pub rng: Mulberry32,
    /// The page's `Math.random` stream at each wood sign
    /// ([`SIGN_RANDOM_AT`]).
    pub sign_random: [u64; 3],
    pub v0: usize,
    pub v1: usize,
    pub pads: Vec<Pad>,
    pub creek: Vec<P>,
    /// Index of the crossing point.
    pub creek_cross: usize,
    pub creek_s: f64,
    pub creek_width: f64,
    pub creek_depth: f64,
    /// `this._fy`: the road height `approxLand` last saw.
    fy: Option<f64>,
    pub farms: Vec<Farm>,
    pub store: Option<StoreSite>,
    pub side_roads: Vec<Vec<P>>,
    pub mill: Option<MillSite>,
    pub windmill: Option<WindmillSite>,
    /// What the build counted (`this.stats`).
    pub stats: Stats,
}

/// `this.stats` (without the timings).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub farms: usize,
    pub trees: usize,
    pub bales: usize,
    pub cows: usize,
    pub meshes: usize,
}

impl Valley {
    pub fn new(_info: &SceneryInfo) -> Valley {
        Valley {
            label: "Planting the valley",
            rng: Mulberry32::new(4242),
            sign_random: SIGN_RANDOM_AT,
            v0: 0,
            v1: 0,
            pads: Vec::new(),
            creek: Vec::new(),
            creek_cross: 0,
            creek_s: 0.0,
            creek_width: 0.0,
            creek_depth: 0.0,
            fy: None,
            farms: Vec::new(),
            store: None,
            side_roads: Vec::new(),
            mill: None,
            windmill: None,
            stats: Stats::default(),
        }
    }

    // ── Planning (before terrain heights exist) ─────────────────────

    /// `plan(world)`.
    pub fn plan_world(&mut self, t: &mut Track, tt: &mut Terrain) {
        self.v0 = t.zone_start[1];
        self.v1 = t.zone_start[2];
        self.pads.clear();
        self.plan_creek(t, tt);
        self.plan_farms(t, tt);
        self.plan_store(t, tt);
        self.plan_mill(t, tt);
        self.plan_windmill(t, tt);
        for p in &self.pads {
            tt.add_flatten(p.x, p.z, p.r, p.falloff, Some(p.y));
        }
        // Open the roadside fence at driveway mouths and the crossroads.
        for f in &self.farms {
            t.fence_gaps.push(FenceGap {
                s0: f.s - 2.4,
                s1: f.s + 2.4,
                side: f.side,
            });
        }
        if let Some(st) = self.store {
            for side in [-1.0, 1.0] {
                t.fence_gaps.push(FenceGap {
                    s0: st.s - 3.6,
                    s1: st.s + 3.6,
                    side,
                });
            }
        }
        tt.add_carve(
            self.creek.iter().map(|p| [p.x, p.z]).collect(),
            self.creek_width,
            self.creek_depth,
            true,
        );
    }

    /// Distance from a point to the nearest road, and whether that road is
    /// the stretch around s (so we can tell the inside of a bend from open
    /// land). The JS default window is 60.
    fn clear_of_road(t: &Track, x: f64, z: f64, margin: f64, s: Option<f64>, window: f64) -> bool {
        let k = t.nearest(x, z, margin + 20.0);
        if k < 0 {
            return true;
        }
        let p = t.project_window(x, z, k as f64, 6);
        let wall = if p.lat > 0.0 {
            at(&t.wall_r, p.i)
        } else {
            at(&t.wall_l, p.i)
        };
        if p.lat.abs() - wall < margin {
            return false;
        }
        if let Some(s) = s
            && (p.s - s).abs() > window
            && p.lat.abs() < margin + wall + 10.0
        {
            return false;
        }
        true
    }

    fn in_valley(tt: &Terrain, x: f64, need: f64) -> bool {
        tt.zone_weights(x).w[1] >= need
    }

    /// Approximate valley landform, usable before the terrain fields exist
    /// (mirrors Terrain.landform's valley branch). Used to route the creek
    /// along low ground so its channel can actually hold water.
    fn approx_land(&mut self, t: &Track, tt: &Terrain, x: f64, z: f64) -> f64 {
        let k = t.nearest(x, z, 360.0);
        let mut d = 360.0;
        let mut fy = match self.fy {
            Some(v) => v,
            None => at(&t.py, self.creek_s as usize),
        };
        if k >= 0 {
            d = kernel::hypot(x - at(&t.px, k as usize), z - at(&t.pz, k as usize));
            fy = at(&t.py, k as usize);
            self.fy = Some(fy);
        }
        let roll = fbm(&tt.noise, x / 380.0, z / 380.0, 3) * 7.0 * smoothstep(15.0, 90.0, d);
        let hills = smoothstep(260.0, 1500.0, d)
            * (40.0 + 380.0 * ridged(&tt.noise, x / 1700.0 + 9.1, z / 1700.0, 5));
        fy - 1.2 + roll + hills
    }

    fn plan_creek(&mut self, t: &Track, tt: &Terrain) {
        let br = t.tag("bridge")[0].clone();
        let sc = js::round((br.s0 + br.s1) / 2.0);
        self.creek_s = sc;
        let f = t.frame(sc);
        const STEP: f64 = 12.0;
        let along = |this: &mut Valley, sgn: f64| -> Vec<P> {
            let mut out: Vec<P> = Vec::new();
            let mut heading = kernel::atan2(f.rz * sgn, f.rx * sgn); // leave perpendicular to the road
            let (mut x, mut z) = (f.x, f.z);
            for k in 1..70 {
                let dist = f64::from(k) * STEP;
                if dist > 72.0 {
                    // Greedy: of the headings in a forward cone, take the lowest ground
                    // (looking two steps ahead), with a mild penalty for turning.
                    let mut best = heading;
                    let mut best_h = f64::INFINITY;
                    for da in [-0.6, -0.4, -0.2, 0.0, 0.2, 0.4, 0.6] {
                        let h2 = heading + da;
                        let x1 = x + kernel::cos(h2) * STEP;
                        let z1 = z + kernel::sin(h2) * STEP;
                        let x2 = x1 + kernel::cos(h2) * STEP;
                        let z2 = z1 + kernel::sin(h2) * STEP;
                        let v = this.approx_land(t, tt, x1, z1)
                            + 0.6 * this.approx_land(t, tt, x2, z2)
                            + f64::abs(da) * 0.8;
                        if v < best_h {
                            best_h = v;
                            best = h2;
                        }
                    }
                    heading = heading + (best - heading) * 0.7;
                }
                x += kernel::cos(heading) * STEP;
                z += kernel::sin(heading) * STEP;
                if dist > 60.0 {
                    let kk = t.nearest(x, z, 70.0);
                    if kk >= 0 && (kk as f64 - sc).abs() > 120.0 {
                        break;
                    }
                }
                if !Valley::in_valley(tt, x, 0.75) {
                    break;
                }
                // Don't loop back on ourselves.
                let upto = out.len().saturating_sub(4);
                if out[..upto]
                    .iter()
                    .any(|p| kernel::hypot(p.x - x, p.z - z) < 36.0)
                {
                    break;
                }
                out.push(P { x, z });
            }
            out
        };
        let mut a = along(self, -1.0);
        a.reverse();
        let b = along(self, 1.0);
        self.creek_cross = a.len(); // index of the crossing point
        self.creek = a;
        self.creek.push(P { x: f.x, z: f.z });
        self.creek.extend(b);
        self.creek_width = 13.0;
        self.creek_depth = 3.4;
    }

    fn creek_dist(&self, x: f64, z: f64) -> f64 {
        seg_dist(x, z, &self.creek).0
    }

    fn pad_ok(&self, tt: &Terrain, x: f64, z: f64, r: f64) -> bool {
        for p in &self.pads {
            if kernel::hypot(x - p.x, z - p.z) < r + p.r + 12.0 {
                return false;
            }
        }
        self.creek_dist(x, z) > r + 22.0 && Valley::in_valley(tt, x, 0.7)
    }

    fn plan_farms(&mut self, t: &Track, tt: &Terrain) {
        let farm_tags: Vec<f64> = t
            .tag("farm")
            .iter()
            .map(|g| js::round((g.s0 + g.s1) / 2.0))
            .collect();
        let crest = t.tag("crest").first().map(|c| (*c).clone());
        let extra = crest.map(|c| c.s1 + 110.0);
        let list: Vec<f64> = match extra {
            Some(e) if e != 0.0 => {
                let mut l = vec![farm_tags[0], e];
                l.extend_from_slice(&farm_tags[1..]);
                l
            }
            _ => farm_tags,
        };
        self.farms.clear();
        let mut prefer = 1.0;
        for s in list {
            let mut placed: Option<Farm> = None;
            'sides: for side in [prefer, -prefer] {
                for lat in [60.0, 70.0, 82.0, 96.0, 112.0] {
                    let c = t.point_at(s, side * lat);
                    if !Valley::clear_of_road(t, c.x, c.z, lat - 12.0, Some(s), 80.0) {
                        continue;
                    }
                    if !self.pad_ok(tt, c.x, c.z, 34.0) {
                        continue;
                    }
                    placed = Some(Farm {
                        s,
                        side,
                        lat,
                        x: c.x,
                        z: c.z,
                        yaw: 0.0,
                        y: 0.0,
                        seed: 0,
                    });
                    break 'sides;
                }
            }
            let Some(mut placed) = placed else { continue };
            let road = t.point_at(s, 0.0);
            placed.yaw = yaw_z(road.x - placed.x, road.z - placed.z);
            placed.y = t.surface_y(s, 0.0) - 0.7;
            placed.seed = (self.rng.next_f64() * 1e9).floor() as u32;
            self.pads.push(Pad {
                x: placed.x,
                z: placed.z,
                r: 34.0,
                falloff: 26.0,
                y: placed.y,
            });
            self.farms.push(placed);
            prefer = -placed.side;
        }
    }

    fn plan_store(&mut self, t: &Track, tt: &Terrain) {
        let Some(g) = t.tag("fields").first().map(|g| (*g).clone()) else {
            return;
        };
        let sx = js::round((g.s0 + g.s1) / 2.0);
        let si = sx as usize;
        for side in [1.0, -1.0] {
            let lat = at(&t.wall_r, si) + 17.0;
            let c = t.point_at(sx + 18.0, side * lat);
            if !self.pad_ok(tt, c.x, c.z, 20.0) {
                continue;
            }
            let road = t.point_at(sx + 18.0, 0.0);
            let st = StoreSite {
                s: sx,
                side,
                x: c.x,
                z: c.z,
                yaw: yaw_z(road.x - c.x, road.z - c.z),
                y: t.surface_y(sx, 0.0) - 0.35,
            };
            self.store = Some(st);
            self.pads.push(Pad {
                x: c.x,
                z: c.z,
                r: 20.0,
                falloff: 16.0,
                y: st.y,
            });
            break;
        }
        // The side road crosses the main road here, running out both ways.
        self.side_roads = Vec::new();
        if self.store.is_none() {
            return;
        }
        let f = t.frame(sx);
        for sgn in [1.0, -1.0] {
            let mut pts = Vec::new();
            let mut d = at(&t.wall_r, si) + 0.7;
            while d < 420.0 {
                let x = f.x + f.rx * d * sgn;
                let z = f.z + f.rz * d * sgn;
                if d > 40.0 && !Valley::clear_of_road(t, x, z, 30.0, Some(sx), 40.0) {
                    break;
                }
                if self.creek_dist(x, z) < 14.0 || !Valley::in_valley(tt, x, 0.8) {
                    break;
                }
                pts.push(P { x, z });
                d += 6.0;
            }
            if pts.len() > 3 {
                self.side_roads.push(pts);
            }
        }
    }

    fn plan_mill(&mut self, t: &Track, _tt: &Terrain) {
        let c = self.creek_cross as i64;
        for dir in [1i64, -1] {
            let i = c + dir * 8;
            if i < 1 || i >= self.creek.len() as i64 - 1 {
                continue;
            }
            let i = i as usize;
            let (a, b) = (self.creek[i - 1], self.creek[i + 1]);
            let dx = b.x - a.x;
            let dz = b.z - a.z;
            let l = kernel::hypot(dx, dz);
            let nx = -dz / l;
            let nz = dx / l;
            for side in [1.0, -1.0] {
                let x = self.creek[i].x + nx * side * 13.5;
                let z = self.creek[i].z + nz * side * 13.5;
                if !Valley::clear_of_road(t, x, z, 30.0, None, 60.0) {
                    continue;
                }
                let mut bad = false;
                for p in &self.pads {
                    if kernel::hypot(x - p.x, z - p.z) < p.r + 24.0 {
                        bad = true;
                    }
                }
                if bad {
                    continue;
                }
                let m = MillSite {
                    x,
                    z,
                    i,
                    nx: nx * side,
                    nz: nz * side,
                    y: t.surface_y(self.creek_s, 0.0) - 1.2,
                };
                self.mill = Some(m);
                self.pads.push(Pad {
                    x,
                    z,
                    r: 7.0,
                    falloff: 7.0,
                    y: m.y,
                });
                return;
            }
        }
    }

    /// The valley's windmill: on a rise near the road, where it stands out
    /// against the sky as a landmark on the way down from the pass.
    fn plan_windmill(&mut self, t: &Track, tt: &Terrain) {
        let mut cands = Vec::new();
        if let Some(crest) = t.tag("crest").first() {
            let mut s = crest.s0;
            while s <= crest.s1 + 120.0 {
                cands.push(s);
                s += 30.0;
            }
        }
        let mut s = self.v0 as f64 + 350.0;
        while s < self.v1 as f64 - 350.0 {
            cands.push(s);
            s += 40.0;
        }
        for s in cands {
            for side in [-1.0, 1.0] {
                for lat in [40.0, 48.0, 58.0] {
                    let c = t.point_at(s, side * lat);
                    if !Valley::clear_of_road(t, c.x, c.z, lat - 12.0, Some(s), 80.0) {
                        continue;
                    }
                    if !self.pad_ok(tt, c.x, c.z, 10.0) {
                        continue;
                    }
                    let road = t.point_at(s, 0.0);
                    let w = WindmillSite {
                        s,
                        side,
                        x: c.x,
                        z: c.z,
                        y: t.surface_y(s, 0.0) - 0.4,
                        yaw: yaw_z(road.x - c.x, road.z - c.z),
                    };
                    self.windmill = Some(w);
                    self.pads.push(Pad {
                        x: c.x,
                        z: c.z,
                        r: 9.0,
                        falloff: 12.0,
                        y: w.y,
                    });
                    return;
                }
            }
        }
    }
}

impl Scenery for Valley {
    fn name(&self) -> &str {
        "Valley"
    }

    fn label(&self) -> Option<&str> {
        Some(self.label)
    }

    fn plan(&mut self, w: &mut World) -> Result<(), String> {
        let t = w.track.as_mut().ok_or("the route is surveyed first")?;
        let tt = w.terrain.as_mut().ok_or("no terrain")?;
        self.plan_world(t, tt);
        Ok(())
    }

    fn build(&mut self, w: &mut World) -> Result<(), String> {
        build(self, w)
    }
}

// ── Build ───────────────────────────────────────────────────────────────

/// `{p: [x, z], r, creek?}`: keep trees off buildings and lanes.
#[derive(Clone, Copy, Debug)]
struct Avoid {
    p: [f64; 2],
    r: f64,
    creek: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TreeKind {
    Shade,
    Orchard,
    Poplar,
    Willow,
    Hill,
}

#[derive(Clone, Copy, Debug)]
struct Tree {
    x: f64,
    y: f64,
    z: f64,
    kind: TreeKind,
    r: f64,
    sy: f64,
    th: f64,
    col: [f64; 3],
    yaw: f64,
}

#[derive(Clone, Copy, Debug)]
struct Hedge {
    x: f64,
    z: f64,
    y: f64,
    sx: f64,
    sy: f64,
    sz: f64,
    yaw: f64,
    c: f64,
}

#[derive(Clone, Copy, Debug)]
struct Bale {
    x: f64,
    z: f64,
    y: f64,
    yaw: f64,
    k: f64,
}

#[derive(Clone, Copy, Debug)]
struct Cow {
    x: f64,
    z: f64,
    yaw: f64,
    c: f64,
}

/// A windpump wheel: where, which way, how fast and its angle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wheel {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f64,
    pub speed: f64,
    pub a: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Crop {
    Corn,
    Lav,
}

#[derive(Clone, Copy, Debug)]
struct CropPt {
    x: f64,
    z: f64,
    y: f64,
    r: f64,
}

#[derive(Clone, Debug)]
struct CropRun {
    kind: Crop,
    pts: Vec<CropPt>,
}

#[derive(Clone, Copy, Debug)]
struct Grass {
    s: f64,
    x: f64,
    y: f64,
    z: f64,
    sx: f64,
    sy: f64,
    sz: f64,
    yaw: f64,
    lush: f64,
    b: f64,
}

#[derive(Clone, Copy, Debug)]
struct Flower {
    s: f64,
    x: f64,
    y: f64,
    z: f64,
    sx: f64,
    sy: f64,
    sz: f64,
    yaw: f64,
    hue: usize,
}

/// A record for `instances()`: `{x, y, z, sx?, sy?, sz?, yaw?, col?}`.
#[derive(Clone, Copy, Debug)]
struct Inst {
    x: f64,
    y: f64,
    z: f64,
    sx: f64,
    sy: f64,
    sz: f64,
    yaw: f64,
    col: Option<Color>,
}

impl Inst {
    fn at(x: f64, y: f64, z: f64, yaw: f64) -> Inst {
        Inst {
            x,
            y,
            z,
            sx: 1.0,
            sy: 1.0,
            sz: 1.0,
            yaw,
            col: None,
        }
    }
}

/// `this.M`: the materials by key, in the order made.
#[derive(Default)]
struct Mats(Vec<(&'static str, MaterialId)>);

impl Mats {
    fn get(&self, k: &str) -> MaterialId {
        self.0
            .iter()
            .find(|(n, _)| *n == k)
            .map(|&(_, m)| m)
            .unwrap_or_else(|| panic!("no material {k}"))
    }
}

/// The paints: plain colours baked into vertex colours, sharing a few
/// material-class buckets (see the paint builder): `[bucket, colour]`.
const PAINTS: &[(&str, (&str, u32))] = &[
    ("wallCream", ("p:siding", 0xd6caa9)),
    ("wallWhite", ("p:siding", 0xe2ddd0)),
    ("wallYellow", ("p:siding", 0xcdb27a)),
    ("wallBlue", ("p:siding", 0x8e9ea3)),
    ("wallGreen", ("p:siding", 0x9aa88a)),
    ("woodGray", ("p:boards", 0x857a6b)),
    ("woodDark", ("p:boards", 0x4d3c2d)),
    ("barnRed", ("p:boards", 0x8e3327)),
    ("barnRed2", ("p:boards", 0x6d3a2c)),
    ("barnDoor", ("p:boards", 0x5a2921)),
    ("barnWhite", ("p:boards", 0xd8d2c4)),
    ("roofShingle", ("p:roof", 0x3e3a38)),
    ("roofRust", ("p:roof", 0x7c4a2f)),
    ("roofSlate", ("p:roof", 0x4a4e56)),
    ("roofGreen", ("p:roof", 0x3f5a48)),
    ("roofMetal", ("p:metal", 0x8b8f91)),
    ("siloMetal", ("p:metal", 0xb6babd)),
    ("siloDome", ("p:metal", 0x9ea4a8)),
    ("siloBand", ("p:metal", 0x6c6f72)),
    ("steelOld", ("p:metal", 0x6c6158)),
    ("mailbox", ("p:metal", 0x3c4146)),
    ("pumpRed", ("p:metal", 0xb2271d)),
    ("trim", ("p:paint", 0xefe9dc)),
    ("stone", ("p:paint", 0x77716a)),
    ("stoneLight", ("p:paint", 0x9a9387)),
    ("brick", ("p:paint", 0x7b4636)),
    ("black", ("p:paint", 0x141414)),
    ("hay", ("p:paint", 0xd2ae62)),
    ("concrete", ("p:paint", 0xb7b0a2)),
    ("vane", ("p:paint", 0x8c3024)),
    ("shutterGreen", ("p:paint", 0x2f4a36)),
    ("shutterBlack", ("p:paint", 0x222426)),
    ("shutterRed", ("p:paint", 0x6e2a22)),
    ("shutterBlue", ("p:paint", 0x34506a)),
    ("sail", ("p:paint", 0xd9d0bc)),
];

/// `S(color, o)`: a standard material, rough and not metallic.
fn s_mat(color: u32, o: &[(&str, Param)]) -> Material {
    let mut m = Material::standard()
        .set("color", color)
        .set("roughness", 0.9)
        .set("metalness", 0.0);
    for (k, v) in o {
        m.set_value(k, v.clone());
    }
    m
}

/// `V(o)`: white with vertex colours.
fn v_mat(o: &[(&str, Param)]) -> Material {
    let mut all = vec![("vertexColors", Param::Bool(true))];
    all.extend(o.iter().cloned());
    s_mat(0xffffff, &all)
}

fn n(v: f64) -> Param {
    Param::Num(v)
}

const DOUBLE: f64 = three::DOUBLE_SIDE as f64;

/// `surfaceDetail(mat, mode)`: world-space surface detail for the painted
/// building buckets, so walls read as clapboard or barn boards and roofs
/// as courses of shingles without a texture per building (kind `Siding`;
/// the GLSL is the renderer's).
fn surface_detail(m: Material, mode: &str) -> Material {
    m.kind(MaterialKind::Siding, Some(json!({ "mode": mode })))
        .program_key(&format!("valley-{mode}"))
        .uniform("clippingPlanes", Value::Null)
}

/// A canvas texture in the graph.
fn add_tex(g: &mut SceneGraph, t: Texture, wrap_s_repeat: Option<bool>) -> TextureId {
    let mut desc = t.desc("", 0);
    if let Some(r) = wrap_s_repeat {
        desc.wrap_s = if r {
            three::REPEAT_WRAPPING
        } else {
            three::CLAMP_TO_EDGE_WRAPPING
        };
    }
    g.add_texture(Image::Own(Arc::new(t)), desc)
}

struct Bld<'a> {
    v: &'a Valley,
    t: &'a Track,
    tt: &'a Terrain,
    ground: Ground<'a>,
    g: &'a mut SceneGraph,
    group: NodeId,
    m: Mats,
    b: Builder,
    trees: Vec<Tree>,
    bales: Vec<Bale>,
    cows: Vec<Cow>,
    wheels: Vec<Wheel>,
    avoid: Vec<Avoid>,
    hedges: Vec<Hedge>,
    crop_runs: Vec<CropRun>,
    grass: Vec<Grass>,
    flowers: Vec<Flower>,
    water: Option<Vec<f64>>,
    water_mat: Option<MaterialId>,
    normal_map: Option<TextureId>,
    mill_wheel: Option<(f64, f64, f64, f64)>,
    sails: Option<(NodeId, Vector3, f64)>,
    waterwheel: Option<(NodeId, Vector3, f64)>,
    wheel_mesh: Option<NodeId>,
    poles: usize,
}

/// `build(world)`.
fn build(v: &mut Valley, w: &mut World) -> Result<(), String> {
    let World {
        track,
        terrain,
        graph,
        root,
        sky,
        animators,
        ..
    } = w;
    let t = track.as_ref().ok_or("the route is surveyed first")?;
    let tt = terrain.as_ref().ok_or("no terrain")?;
    let group = graph.group("valley");
    let mut s = Bld {
        v,
        t,
        tt,
        ground: Ground::new(tt),
        g: graph,
        group,
        m: Mats::default(),
        b: Builder::new_paint(PAINTS),
        trees: Vec::new(),
        bales: Vec::new(),
        cows: Vec::new(),
        wheels: Vec::new(),
        avoid: Vec::new(),
        hedges: Vec::new(),
        crop_runs: Vec::new(),
        grass: Vec::new(),
        flowers: Vec::new(),
        water: None,
        water_mat: None,
        normal_map: None,
        mill_wheel: None,
        sails: None,
        waterwheel: None,
        wheel_mesh: None,
        poles: 0,
    };
    s.make_materials();
    for f in s.v.farms.clone() {
        s.build_farm(&f);
    }
    s.build_store();
    s.build_creek();
    s.build_mill();
    s.build_windmill();
    s.build_poles();
    s.build_entry_sign();
    s.scatter_hedgerows();
    s.scatter_creek_trees();
    s.scatter_hill_trees();
    s.scatter_bales();
    s.scatter_crops();
    s.scatter_verge();

    let mats: Vec<(&str, MaterialId)> = s.m.0.clone();
    let opts = BuildOpts {
        cast_shadow: CastShadow::Keys(
            ["p:siding", "p:boards", "p:roof", "p:metal", "p:paint"]
                .iter()
                .map(|k| k.to_string())
                .collect(),
        ),
        receive_shadow: true,
    };
    for mesh in s.b.build(s.g, &mats, &opts) {
        s.g.add(s.group, mesh);
    }
    s.build_trees();
    s.build_bales_mesh();
    s.build_cows();
    s.build_wheels();
    s.build_crops_mesh();
    s.build_verge_mesh();
    s.g.add(*root, s.group);

    let m = |k: &str| Some(s.m.get(k));
    s.g.add_night(m("window"), "emissiveIntensity", 0.0, 2.4);
    s.g.add_night(m("lamp"), "emissiveIntensity", 0.3, 7.0);
    s.g.add_night(m("pumpGlobe"), "emissiveIntensity", 0.4, 4.0);
    s.g.add_night(m("neon"), "emissiveIntensity", 1.2, 5.0);
    s.g.add_night(m("signValley"), "emissiveIntensity", 0.0, 0.25);
    s.g.add_night(m("signStore"), "emissiveIntensity", 0.0, 0.35);

    let horizon = sky.as_ref().map(|k| (k.params.clone(), k.override_p));
    let anim = Wheels {
        wheels: s.wheels.clone(),
        wheel_mesh: s.wheel_mesh,
        waterwheel: s.waterwheel.map(|(n, p, yaw)| (n, p, yaw, 0.0)),
        sails: s.sails.map(|(n, p, yaw)| (n, p, yaw, 0.0)),
        water: s.water_mat.zip(s.normal_map).map(|(m, t)| (m, t, [0.0; 2])),
        horizon,
    };
    let stats = Stats {
        farms: s.v.farms.len(),
        trees: s.trees.len(),
        bales: s.bales.len(),
        cows: s.cows.len(),
        meshes: s.g.get(s.group).children.len(),
    };
    animators.push(Box::new(anim));
    v.stats = stats;
    Ok(())
}

impl Bld<'_> {
    fn make_materials(&mut self) {
        let mut add = |g: &mut SceneGraph, k: &'static str, m: Material| {
            let id = g.add_material(m);
            self.m.0.push((k, id));
        };
        let g = &mut *self.g;
        add(
            g,
            "p:siding",
            surface_detail(v_mat(&[("roughness", n(0.85))]), "siding"),
        );
        add(
            g,
            "p:boards",
            surface_detail(v_mat(&[("roughness", n(0.92))]), "boards"),
        );
        add(
            g,
            "p:roof",
            surface_detail(
                v_mat(&[("roughness", n(0.85)), ("metalness", n(0.12))]),
                "roof",
            ),
        );
        add(
            g,
            "p:metal",
            v_mat(&[
                ("metalness", n(0.5)),
                ("roughness", n(0.48)),
                ("side", n(DOUBLE)),
            ]),
        );
        add(g, "p:paint", v_mat(&[("side", n(DOUBLE))]));
        add(
            g,
            "window",
            s_mat(
                0x2b2721,
                &[
                    ("emissive", n(f64::from(0xffbb66))),
                    ("emissiveIntensity", n(0.0)),
                    ("roughness", n(0.3)),
                ],
            ),
        );
        add(
            g,
            "windowDark",
            s_mat(0x1c2126, &[("roughness", n(0.25)), ("metalness", n(0.4))]),
        );
        add(
            g,
            "lamp",
            s_mat(
                0xfff1d0,
                &[
                    ("emissive", n(f64::from(0xffd49a))),
                    ("emissiveIntensity", n(0.3)),
                ],
            ),
        );
        add(
            g,
            "pumpGlobe",
            s_mat(
                0xffffff,
                &[
                    ("emissive", n(f64::from(0xfff0d8))),
                    ("emissiveIntensity", n(0.4)),
                ],
            ),
        );
        let offset = [
            ("roughness", n(1.0)),
            ("polygonOffset", Param::Bool(true)),
            ("polygonOffsetFactor", n(-1.0)),
            ("polygonOffsetUnits", n(-2.0)),
        ];
        add(g, "dirt", s_mat(0x94806a, &offset));
        add(g, "gravelRoad", s_mat(0x9b9384, &offset));
        add(g, "hay", s_mat(0xd2ae62, &[("roughness", n(1.0))]));
        let mut rv = page_random(self.v.sign_random[0]);
        let vs = add_tex(
            g,
            wood_sign(
                &["OLD MILL VALLEY", "Pop. 312  ·  Est. 1887"],
                WoodSign::DEFAULT,
                &mut rv,
            ),
            None,
        );
        add(
            g,
            "signValley",
            s_mat(
                0xffffff,
                &[
                    ("map", Param::Texture(vs)),
                    ("emissive", n(f64::from(0xffffff))),
                    ("emissiveMap", Param::Texture(vs)),
                    ("emissiveIntensity", n(0.0)),
                ],
            ),
        );
        let mut rs = page_random(self.v.sign_random[1]);
        let st = add_tex(
            g,
            wood_sign(
                &["GENERAL STORE", "MILL VALLEY  ·  FEED · SEED · GAS"],
                WoodSign {
                    w: 768,
                    h: 160,
                    bg: "#7a2e22",
                    font: "bold 74px Georgia, serif",
                    sub: "bold 30px Georgia, serif",
                    ..WoodSign::DEFAULT
                },
                &mut rs,
            ),
            None,
        );
        add(
            g,
            "signStore",
            s_mat(
                0xffffff,
                &[
                    ("map", Param::Texture(st)),
                    ("emissive", n(f64::from(0xffffff))),
                    ("emissiveMap", Param::Texture(st)),
                    ("emissiveIntensity", n(0.0)),
                ],
            ),
        );
        let neon = add_tex(g, neon_texture(), None);
        add(
            g,
            "neon",
            Material::standard()
                .set("color", 0x000000)
                .set("emissive", 0xffffff)
                .set("emissiveMap", neon)
                .set("emissiveIntensity", 1.2)
                .set("transparent", true)
                .set("alphaMap", neon)
                .set("depthWrite", false),
        );
    }

    fn gy(&self, x: f64, z: f64) -> f64 {
        self.ground.height(x, z)
    }

    // ── Farmsteads ─────────────────────────────────────────────────

    fn build_farm(&mut self, f: &Farm) {
        let t = self.t;
        let mut rng = Mulberry32::new(f.seed);
        let rng = &mut rng;
        let y = f.y;
        let to_world = |lx: f64, lz: f64| -> [f64; 2] {
            let c = kernel::cos(f.yaw);
            let s = kernel::sin(f.yaw);
            [f.x + lx * c + lz * s, f.z - lx * s + lz * c]
        };
        self.b.set_frame(f.x, y, f.z, f.yaw);

        // Farmhouse, left-front of the yard.
        let hx = -11.0 + rrange(rng, -2.0, 2.0);
        let hz = 7.0;
        let ry = rrange(rng, -0.08, 0.08);
        self.b.push_frame(hx, 0.0, hz, ry);
        let house = parts::farmhouse(&mut self.b, rng);
        self.b.pop_frame();
        self.avoid.push(Avoid {
            p: to_world(hx, hz),
            r: 12.0,
            creek: false,
        });

        // Barn, right-back; sometimes turned side-on.
        let bx = 13.0 + rrange(rng, -2.0, 2.0);
        let bz = -7.0;
        let turned = rng.next_f64() < 0.4;
        let ry = if turned {
            -PI / 2.0
        } else {
            rrange(rng, -0.1, 0.1)
        };
        self.b.push_frame(bx, 0.0, bz, ry);
        let b = parts::barn(&mut self.b, rng);
        self.b.pop_frame();
        self.avoid.push(Avoid {
            p: to_world(bx, bz),
            r: 15.0,
            creek: false,
        });

        // Silos beside the barn.
        let n_silo = if rng.next_f64() < 0.25 {
            0
        } else if rng.next_f64() < 0.6 {
            1
        } else {
            2
        };
        for k in 0..n_silo {
            let kf = f64::from(k);
            let sx = bx
                + (if turned {
                    6.0 + kf * 6.5
                } else {
                    b.w / 2.0 + 4.0 + kf * 6.2
                });
            let sz = bz + (if turned { -b.w / 2.0 - 4.0 } else { -4.0 });
            let metal = if k == 0 { rng.next_f64() < 0.5 } else { true };
            parts::silo(&mut self.b, rng, sx, sz, metal);
            self.avoid.push(Avoid {
                p: to_world(sx, sz),
                r: 4.0,
                creek: false,
            });
        }
        // Shed and a stack of square bales.
        let ry = rrange(rng, -0.3, 0.3);
        self.b.push_frame(-18.0, 0.0, -13.0, ry);
        parts::shed(&mut self.b, rng);
        self.b.pop_frame();
        for r in 0..3 {
            for c in 0..4 - r {
                let (r, c) = (f64::from(r), f64::from(c));
                self.b.box_(
                    "hay",
                    1.2,
                    0.5,
                    0.9,
                    1.0 + c * 1.25 + r * 0.6,
                    r * 0.5,
                    -18.5,
                    0.0,
                    0.0,
                    0.0,
                );
            }
        }
        // Windpump near the house.
        if rng.next_f64() < 0.8 {
            let (wx, wz) = (-26.0, 15.0);
            self.b.push_frame(wx, 0.0, wz, 0.0);
            // Tower in the frame; wheel faces the prevailing wind (+Z world-ish).
            let h = 10.0 + rng.next_f64() * 2.0;
            let hub = parts::windpump_tower(&mut self.b, h);
            self.b.pop_frame();
            let [ex, ez] = to_world(wx + hub.x, wz + hub.z);
            let speed = 1.6 + rng.next_f64() * 1.4;
            let a = rng.next_f64() * 6.0;
            self.wheels.push(Wheel {
                x: ex,
                y: y + hub.y,
                z: ez,
                yaw: f.yaw,
                speed,
                a,
            });
            self.avoid.push(Avoid {
                p: to_world(wx, wz),
                r: 4.0,
                creek: false,
            });
        }
        // Yard light pole.
        self.b
            .box_("woodDark", 0.18, 6.5, 0.18, 2.0, 0.0, 12.0, 0.0, 0.0, 0.0);
        self.b
            .cbox("lamp", 0.5, 0.25, 0.35, 2.0, 6.4, 12.35, [0.0; 3]);
        // Paddock with cows behind the barn.
        let (px, pz, pw, pd) = (8.0, -27.0, 30.0, 16.0);
        self.b.push_frame(px, 0.0, pz, 0.0);
        parts::paddock(&mut self.b, pw, pd, 3.5);
        self.b.pop_frame();
        let n_cows = 3 + (rng.next_f64() * 5.0).floor() as usize;
        for _ in 0..n_cows {
            let lx = px + rrange(rng, -pw / 2.0 + 2.0, pw / 2.0 - 2.0);
            let lz = pz + rrange(rng, -pd / 2.0 + 2.0, pd / 2.0 - 2.0);
            let [cx, cz] = to_world(lx, lz);
            let yaw = rng.next_f64() * PI * 2.0;
            let c = rng.next_f64();
            self.cows.push(Cow {
                x: cx,
                z: cz,
                yaw,
                c,
            });
        }
        self.avoid.push(Avoid {
            p: to_world(px, pz),
            r: 17.0,
            creek: false,
        });

        // Driveway from the road edge to the porch.
        let hw = t.frame(f.s);
        let wall = if f.side > 0.0 { hw.wall_r } else { hw.wall_l };
        let a = t.point_at(f.s, f.side * (wall + 0.7));
        let a2 = t.point_at(f.s, f.side * (wall + 12.0));
        let [dx, dz] = to_world(hx, hz + house.d / 2.0 + 3.2);
        let [mx, mz] = to_world(hx + 4.0, hz + house.d / 2.0 + 14.0);
        let mut path = Vec::new();
        // Quadratic from road edge, leaving straight out from the road.
        let p0 = [a.x, a.z];
        let p1 = [a2.x, a2.z];
        let p2 = [mx, mz];
        let p3 = [dx, dz];
        let l = kernel::hypot(p3[0] - p0[0], p3[1] - p0[1]);
        let nn = js::max(6.0, js::round(l / 3.0)) as usize;
        for i in 0..=nn {
            let u = i as f64 / nn as f64;
            let v = 1.0 - u;
            path.push(P {
                x: v * v * v * p0[0]
                    + 3.0 * v * v * u * p1[0]
                    + 3.0 * v * u * u * p2[0]
                    + u * u * u * p3[0],
                z: v * v * v * p0[1]
                    + 3.0 * v * v * u * p1[1]
                    + 3.0 * v * u * u * p2[1]
                    + u * u * u * p3[1],
            });
        }
        self.ribbon(&path, 2.8, "dirt", 0.09);
        self.avoid_path(&path, 5.0);
        // Mailbox beside the driveway mouth, just outside the fence line.
        let mb = t.point_at(f.s + 3.2, f.side * (wall + 1.6));
        let gy = self.gy(mb.x, mb.z);
        self.b
            .set_frame(mb.x, gy, mb.z, yaw_z(-hw.rx * f.side, -hw.rz * f.side));
        parts::mailbox(&mut self.b, 0.0, 0.0, 0.0);

        // Big shade trees around the house.
        let nt = 2 + (rng.next_f64() * 3.0).floor() as usize;
        for _ in 0..nt {
            let lx = hx + rrange(rng, -16.0, 8.0);
            let r = rrange(rng, 10.0, 20.0);
            let lz = hz + r * (if rng.next_f64() < 0.5 { 1.0 } else { -0.9 });
            let [tx, tz] = to_world(lx, lz);
            self.add_tree(tx, tz, TreeKind::Shade, rng, 0.0);
        }
        // Orchard behind the house.
        if rng.next_f64() < 0.75 {
            let ox = -36.0 + rrange(rng, -4.0, 4.0);
            let oz = -30.0;
            let cols = 6 + (rng.next_f64() * 3.0).floor() as usize;
            let rows = 6 + (rng.next_f64() * 4.0).floor() as usize;
            for i in 0..cols {
                for j in 0..rows {
                    let [tx, tz] = to_world(ox - i as f64 * 6.5, oz - j as f64 * 6.5 + 20.0);
                    self.add_tree(tx, tz, TreeKind::Orchard, rng, 1.5);
                }
            }
        }
        // Windbreak row of poplars along one side.
        let wb_side = if rng.next_f64() < 0.5 { -1.0 } else { 1.0 };
        for k in 0..14 {
            let [tx, tz] = to_world(wb_side * 44.0, 20.0 - f64::from(k) * 5.2);
            self.add_tree(tx, tz, TreeKind::Poplar, rng, 1.5);
        }
    }

    fn avoid_path(&mut self, path: &[P], r: f64) {
        for p in path.iter().step_by(2) {
            self.avoid.push(Avoid {
                p: [p.x, p.z],
                r,
                creek: false,
            });
        }
    }

    /// Flat ribbon draped on the rendered ground (the JS default lift is
    /// 0.09).
    fn ribbon(&mut self, path: &[P], width: f64, mat_key: &str, lift: f64) {
        let mut pos = Vec::new();
        let mut uv = Vec::new();
        let mut idx: Vec<u32> = Vec::new();
        let mut along = 0.0;
        for i in 0..path.len() {
            let p = path[i];
            let a = path[i.saturating_sub(1)];
            let b = path[(i + 1).min(path.len() - 1)];
            let mut dx = b.x - a.x;
            let mut dz = b.z - a.z;
            let l = js::or(kernel::hypot(dx, dz), 1.0);
            dx /= l;
            dz /= l;
            if i > 0 {
                along += kernel::hypot(p.x - path[i - 1].x, p.z - path[i - 1].z);
            }
            for sgn in [-1.0, 1.0] {
                let x = p.x - dz * sgn * width / 2.0;
                let z = p.z + dx * sgn * width / 2.0;
                pos.extend([x, self.gy(x, z) + lift, z]);
                uv.extend([sgn * 0.5 + 0.5, along / 4.0]);
            }
            if i > 0 {
                let k = (i * 2) as u32;
                idx.extend([k - 2, k, k - 1, k - 1, k, k + 1]);
            }
        }
        let mut g = BufferGeometry::new();
        g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
        g.set_attribute("uv", BufferAttribute::from_f64(&uv, 2));
        g.set_index(&idx);
        g.compute_vertex_normals();
        // Make sure normals point up whatever the winding.
        let nrm = g.get_attribute_mut("normal").expect("normals");
        for i in 0..nrm.count() {
            if nrm.get_y(i) < 0.0 {
                let (x, y, z) = (nrm.get_x(i), nrm.get_y(i), nrm.get_z(i));
                nrm.set_xyz(i, -x, -y, -z);
            }
        }
        let mat = self.m.get(mat_key);
        self.g.material_mut(mat).set_value("side", n(DOUBLE));
        let geo = self.g.add_geometry(g);
        let m = self.g.mesh(geo, mat);
        let o = self.g.get_mut(m);
        o.receive_shadow = true;
        o.matrix_auto_update = false;
        self.g.add(self.group, m);
    }

    // ── Crossroads store ───────────────────────────────────────────

    fn build_store(&mut self) {
        let Some(st) = self.v.store else { return };
        let t = self.t;
        for path in self.v.side_roads.clone() {
            self.ribbon(&path, 5.5, "gravelRoad", 0.1);
            self.avoid_path(&path, 7.0);
        }
        // mulberry32(99): generalStore draws nothing from it.
        self.b.set_frame(st.x, st.y, st.z, st.yaw);
        let s = parts::general_store(&mut self.b);
        // Sign board on the false front.
        self.b.put_at(
            "signStore",
            &plane_geometry(12.0, 2.5, 1.0, 1.0),
            s.sign.x,
            s.sign.y,
            s.sign.z + 0.02,
        );
        self.b.put_at(
            "neon",
            &plane_geometry(1.6, 0.6, 1.0, 1.0),
            s.neon.x,
            s.neon.y - 0.95,
            s.neon.z + 0.02,
        );
        // Pumps on a gravel apron between the store and the road.
        for px in [-2.2, 2.2] {
            parts::gas_pump(&mut self.b, px, 12.5);
        }
        self.b
            .box_("roofRust", 8.0, 0.2, 4.5, 0.0, 4.6, 12.5, 0.0, 0.0, 0.0);
        for px in [-3.6, 3.6] {
            self.b
                .box_("woodDark", 0.2, 4.6, 0.2, px, 0.0, 12.5, 0.0, 0.0, 0.0);
        }
        self.avoid.push(Avoid {
            p: [st.x, st.z],
            r: 22.0,
            creek: false,
        });
        // Stop signs at the side-road mouths.
        let f = t.frame(st.s);
        for sgn in [1.0, -1.0] {
            let wall = if sgn > 0.0 { f.wall_r } else { f.wall_l };
            let p = t.point_at(st.s - 4.0 * sgn, sgn * (wall + 1.8));
            let face = yaw_z(f.rx * sgn, f.rz * sgn);
            let gy = self.gy(p.x, p.z);
            self.b.set_frame(p.x, gy, p.z, face);
            self.b
                .box_("steelOld", 0.07, 2.2, 0.07, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
            self.b.put(
                "pumpRed",
                &cylinder_geometry(0.38, 0.38, 0.04, 8.0, 1.0, false, 0.0, 2.0 * PI),
                0.0,
                2.4,
                0.05,
                [PI / 2.0, 0.0, PI / 8.0],
                [1.0; 3],
            );
        }
    }

    // ── Mill and creek ─────────────────────────────────────────────

    fn build_mill(&mut self) {
        let Some(m) = self.v.mill else { return };
        // Building's +X (wheel side) faces the creek.
        let yaw = yaw_x(-m.nx, -m.nz);
        self.b.set_frame(m.x, m.y, m.z, yaw);
        let h = parts::mill_house(&mut self.b, &mut Mulberry32::new(7));
        let c = kernel::cos(yaw);
        let s = kernel::sin(yaw);
        let wx = m.x + h.wheel.x * c + h.wheel.z * s;
        let wz = m.z - h.wheel.x * s + h.wheel.z * c;
        let water = self.water_level_at(wx, wz);
        // Paddles dip ~0.7 m into the stream.
        self.mill_wheel = Some((wx, water + 3.2 - 0.7, wz, yaw));
        self.avoid.push(Avoid {
            p: [m.x, m.z],
            r: 13.0,
            creek: false,
        });
        // A sign by the road pointing at it.
        let t = self.t;
        let sc = self.v.creek_s;
        let f = t.frame(sc - 30.0);
        let side = if (m.x - f.x) * f.rx + (m.z - f.z) * f.rz > 0.0 {
            1.0
        } else {
            -1.0
        };
        let wall = if side > 0.0 { f.wall_r } else { f.wall_l };
        let p = t.point_at(sc - 30.0, side * (wall + 2.2));
        let mut rm = page_random(self.v.sign_random[2]);
        let tex = add_tex(
            self.g,
            wood_sign(
                &["THE OLD MILL", "Grist since 1887"],
                WoodSign::DEFAULT,
                &mut rm,
            ),
            None,
        );
        let mat = self
            .g
            .add_material(Material::standard().set("map", tex).set("roughness", 0.9));
        self.m.0.push(("signMill", mat));
        let gy = self.gy(p.x, p.z);
        self.b.set_frame(p.x, gy, p.z, yaw_z(-f.fx, -f.fz));
        parts::board_sign(&mut self.b, "signMill", 2.4, 0.9, 0.0, 0.0, 0.0, 0.9);
    }

    fn build_windmill(&mut self) {
        let Some(w) = self.v.windmill else { return };
        self.b.set_frame(w.x, w.y, w.z, w.yaw);
        let hub = parts::tower_mill(&mut self.b, &mut Mulberry32::new(31));
        self.b
            .box_("stone", 1.8, 0.3, 1.2, 0.0, -0.1, 4.1, 0.0, 0.0, 0.0); // doorstep
        let geo = parts::mill_sails_geometry(&mut Builder::new_paint(&[
            ("wood", ("a", 0x4a3a2c)),
            ("cloth", ("a", 0xd9d0bc)),
        ]))
        .expect("the sails merge");
        let mat = self.g.add_material(
            Material::standard()
                .set("vertexColors", true)
                .set("roughness", 0.9)
                .set("side", DOUBLE),
        );
        let c = kernel::cos(w.yaw);
        let s = kernel::sin(w.yaw);
        let geo = self.g.add_geometry(geo);
        let m = self.g.mesh(geo, mat);
        let pos = Vector3::new(
            w.x + hub.x * c + hub.z * s,
            w.y + hub.y,
            w.z - hub.x * s + hub.z * c,
        );
        let o = self.g.get_mut(m);
        o.position = pos;
        // The windshaft tilted up, as on real mills.
        o.set_rotation(&Euler::with_order(-0.08, w.yaw, 0.0, EulerOrder::YXZ));
        o.cast_shadow = true;
        self.sails = Some((m, pos, w.yaw));
        self.g.add(self.group, m);
        self.avoid.push(Avoid {
            p: [w.x, w.z],
            r: 13.0,
            creek: false,
        });
    }

    fn water_level_at(&self, x: f64, z: f64) -> f64 {
        let Some(water) = &self.water else {
            return self.tt.height_at(x, z);
        };
        let (_, i, t) = seg_dist(x, z, &self.v.creek);
        lerp(water[i], water[(water.len() - 1).min(i + 1)], t)
    }

    fn build_creek(&mut self) {
        let pts = self.v.creek.clone();
        let (tt, t) = (self.tt, self.t);
        // Bed height along the creek, ignoring where the road embankment sits.
        let mut bed: Vec<f64> = pts.iter().map(|p| tt.height_at(p.x, p.z)).collect();
        let valid: Vec<bool> = pts.iter().map(|p| t.nearest(p.x, p.z, 40.0) < 0).collect();
        // Fill invalid spans by interpolation.
        let mut last: i64 = -1;
        for i in 0..pts.len() {
            if !valid[i] {
                continue;
            }
            if last >= 0 && i as i64 - last > 1 {
                let l = last as usize;
                for k in l + 1..i {
                    bed[k] = lerp(bed[l], bed[i], (k - l) as f64 / (i - l) as f64);
                }
            }
            if last < 0 {
                for k in 0..i {
                    bed[k] = bed[i];
                }
            }
            last = i as i64;
        }
        if last >= 0 {
            let l = last as usize;
            for k in l + 1..pts.len() {
                bed[k] = bed[l];
            }
        }
        // Water sits ~1 m above the bed, but always well below the banks: near
        // the road the channel is filled by the embankment, and there the
        // water must stay hidden underneath it.
        let n_pts = pts.len();
        let bank: Vec<f64> = (0..n_pts)
            .map(|i| {
                let p = pts[i];
                let a = pts[i.saturating_sub(1)];
                let b = pts[(i + 1).min(n_pts - 1)];
                let mut dx = b.x - a.x;
                let mut dz = b.z - a.z;
                let l = js::or(kernel::hypot(dx, dz), 1.0);
                dx /= l;
                dz /= l;
                let mut m = f64::INFINITY;
                for off in [-14.0, -11.0, 11.0, 14.0] {
                    let (x, z) = (p.x - dz * off, p.z + dx * off);
                    m = js::min_n(&[m, self.ground.height(x, z), tt.height_at(x, z)]);
                }
                m
            })
            .collect();
        let mut water: Vec<f64> = bed
            .iter()
            .enumerate()
            .map(|(i, &b)| js::min(b + 1.1, bank[i] - 0.4))
            .collect();
        for _ in 0..3 {
            for i in 1..water.len().saturating_sub(1) {
                water[i] = js::min(
                    bank[i] - 0.4,
                    (water[i - 1] + water[i] + water[i + 1]) / 3.0,
                );
            }
        }

        let mut pos = Vec::new();
        let mut uv = Vec::new();
        let mut idx: Vec<u32> = Vec::new();
        const W: f64 = 9.0;
        let mut along = 0.0;
        for i in 0..n_pts {
            let a = pts[i.saturating_sub(1)];
            let b = pts[(i + 1).min(n_pts - 1)];
            let mut dx = b.x - a.x;
            let mut dz = b.z - a.z;
            let l = js::or(kernel::hypot(dx, dz), 1.0);
            dx /= l;
            dz /= l;
            if i > 0 {
                along += kernel::hypot(pts[i].x - pts[i - 1].x, pts[i].z - pts[i - 1].z);
            }
            for sgn in [-1.0, 1.0] {
                pos.extend([pts[i].x - dz * sgn * W, water[i], pts[i].z + dx * sgn * W]);
                uv.extend([(sgn * W) / 10.0, along / 10.0]);
            }
            if i > 0 {
                let k = (i * 2) as u32;
                idx.extend([k - 2, k - 1, k, k - 1, k + 1, k]);
            }
        }
        self.water = Some(water);
        let mut g = BufferGeometry::new();
        g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
        g.set_attribute("uv", BufferAttribute::from_f64(&uv, 2));
        g.set_index(&idx);
        g.compute_vertex_normals();
        let nrm = g.get_attribute_mut("normal").expect("normals");
        for i in 0..nrm.count() {
            nrm.set_xyz(i, 0.0, 1.0, 0.0);
        }
        let mut nm = water_normal_texture();
        nm.repeat = true;
        nm.srgb = false;
        let nm = add_tex(self.g, nm, None);
        let mat = self.g.add_material(
            Material::standard()
                .set("color", 0x1b4a5c)
                .set("roughness", 0.05)
                .set("metalness", 0.0)
                .set("normalMap", nm)
                .set("normalScale", Param::Vec2(0.4, 0.4))
                .set("transparent", true)
                .set("opacity", 0.86)
                .set("emissive", 0x000000)
                .set("side", DOUBLE)
                .set("envMapIntensity", 0.8),
        );
        self.water_mat = Some(mat);
        self.normal_map = Some(nm);
        let geo = self.g.add_geometry(g);
        let mesh = self.g.mesh(geo, mat);
        let o = self.g.get_mut(mesh);
        o.receive_shadow = true;
        o.matrix_auto_update = false;
        self.g.add(self.group, mesh);
        for p in &pts {
            self.avoid.push(Avoid {
                p: [p.x, p.z],
                r: 11.0,
                creek: true,
            });
        }

        self.build_crossing();
    }

    /// Where the creek meets the road: stone parapets, and either a proper
    /// bridge (if the terrain dips under the deck) or culvert headwalls.
    fn build_crossing(&mut self) {
        let (t, tt) = (self.t, self.tt);
        let sc = self.v.creek_s;
        let f = t.frame(sc);
        let road_y = t.surface_y(sc, 0.0);
        let gap = road_y - tt.height_at(f.x, f.z);
        let span = 12.0;
        let b = &mut self.b;
        for side in [-1.0, 1.0] {
            let wall = if side > 0.0 { f.wall_r } else { f.wall_l };
            // Parapet: a stone wall with a capping, just outside the fence.
            let mut s = sc - span;
            while s < sc + span {
                let p = t.point_at(s + 1.0, side * (wall + 1.1));
                let fr = t.frame(s + 1.0);
                b.set_frame(
                    p.x,
                    t.surface_y(s + 1.0, side * (wall + 1.1)) - 0.2,
                    p.z,
                    yaw_z(fr.fx, fr.fz),
                );
                b.box_("stoneLight", 0.5, 1.15, 2.05, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
                b.box_("stone", 0.62, 0.14, 2.05, 0.0, 1.15, 0.0, 0.0, 0.0, 0.0);
                s += 2.0;
            }
            for e in [-1.0, 1.0] {
                let s = sc + e * (span + 0.6);
                let p = t.point_at(s, side * (wall + 1.1));
                let fr = t.frame(s);
                b.set_frame(
                    p.x,
                    t.surface_y(s, side * (wall + 1.1)) - 0.2,
                    p.z,
                    yaw_z(fr.fx, fr.fz),
                );
                b.box_("stone", 0.8, 1.6, 0.8, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
                b.box_("stoneLight", 0.9, 0.2, 0.9, 0.0, 1.6, 0.0, 0.0, 0.0, 0.0);
            }
        }
        // Creek direction at the crossing.
        let creek = &self.v.creek;
        let i = self.v.creek_cross;
        let a = creek[i.saturating_sub(1)];
        let bb = creek[(i + 1).min(creek.len() - 1)];
        let cdx = bb.x - a.x;
        let cdz = bb.z - a.z;
        let cl = kernel::hypot(cdx, cdz);
        let ux = cdx / cl;
        let uz = cdz / cl;
        if gap > 2.0 {
            // Open bridge: abutments at the banks and beams under the deck.
            for e in [-1.0, 1.0] {
                let s = sc + e * (span - 1.0);
                let fr = t.frame(s);
                b.set_frame(
                    fr.x,
                    tt.height_at(fr.x, fr.z) - 1.0,
                    fr.z,
                    yaw_z(fr.fx, fr.fz),
                );
                b.box_(
                    "stone",
                    fr.hw * 2.0 + 4.0,
                    road_y - tt.height_at(fr.x, fr.z) + 0.6,
                    2.2,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                );
            }
            for lat in [-4.0, 0.0, 4.0] {
                let p0 = t.point_at(sc - span, lat);
                let p1 = t.point_at(sc + span, lat);
                b.set_frame(0.0, 0.0, 0.0, 0.0);
                b.beam(
                    "steelOld",
                    Vector3::new(p0.x, p0.y - 0.9, p0.z),
                    Vector3::new(p1.x, p1.y - 0.9, p1.z),
                    0.8,
                );
            }
        } else {
            // Culvert headwalls where the embankment meets the water.
            let water = self.water.as_ref().expect("the creek's water");
            let mut walls = Vec::new();
            for dir in [-1i64, 1] {
                // Walk out along the creek until the water surfaces above the bed;
                // the culvert mouth sits just road-side of that.
                let (mut hx, mut hz) = (f.x, f.z);
                let (mut hdx, mut hdz) = (ux * dir as f64, uz * dir as f64);
                let mut found = false;
                for k in 1..12i64 {
                    let j = i as i64 + dir * k;
                    if j < 0 || j >= creek.len() as i64 {
                        break;
                    }
                    let j = j as usize;
                    if water[j] - tt.height_at(creek[j].x, creek[j].z) > 0.25 {
                        let pj = creek[j];
                        let pp = creek[(j as i64 - dir) as usize];
                        let back = 3.0 / kernel::hypot(pj.x - pp.x, pj.z - pp.z);
                        hx = pj.x + (pp.x - pj.x) * back;
                        hz = pj.z + (pp.z - pj.z) * back;
                        let l = kernel::hypot(pj.x - pp.x, pj.z - pp.z);
                        hdx = (pj.x - pp.x) / l;
                        hdz = (pj.z - pp.z) / l;
                        found = true;
                        break;
                    }
                }
                if found {
                    walls.push((hx, hz, hdx, hdz));
                }
            }
            for (hx, hz, hdx, hdz) in walls {
                let wl = self.water_level_at(hx, hz);
                let base = wl - 1.4;
                let top = js::max(wl + 2.4, tt.height_at(hx, hz) + 0.6);
                let b = &mut self.b;
                b.set_frame(hx, base, hz, yaw_z(hdx, hdz));
                b.box_(
                    "stoneLight",
                    12.0,
                    top - base,
                    0.9,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                );
                b.box_(
                    "stone",
                    12.4,
                    0.25,
                    1.1,
                    0.0,
                    top - base,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                );
                // Arch opening: dark half-disc on the outer face.
                b.put_at(
                    "black",
                    &circle_geometry(1.9, 16.0, 0.0, PI),
                    0.0,
                    wl - base - 0.3,
                    0.47,
                );
                b.put_at(
                    "stone",
                    &torus_geometry(2.0, 0.22, 4.0, 16.0, PI),
                    0.0,
                    wl - base - 0.3,
                    0.5,
                );
            }
        }
    }

    // ── Telephone poles along the verge ─────────────────────────────

    fn build_poles(&mut self) {
        let t = self.t;
        let mut skip: Vec<[f64; 2]> = Vec::new();
        for f in &self.v.farms {
            skip.push([f.s - 8.0, f.s + 12.0]);
        }
        if let Some(st) = self.v.store {
            skip.push([st.s - 10.0, st.s + 10.0]);
        }
        skip.push([self.v.creek_s - 22.0, self.v.creek_s + 22.0]);
        let side = 1.0;
        struct Pole {
            x: f64,
            y: f64,
            z: f64,
            rx: f64,
            rz: f64,
            yaw: f64,
        }
        let mut poles: Vec<Pole> = Vec::new();
        let mut s = self.v.v0 as f64 + 60.0;
        while s < self.v.v1 as f64 - 40.0 {
            let k = at(&t.k_smooth, t.idx(s)).abs();
            let spacing = if k > 0.004 { 30.0 } else { 42.0 };
            if !skip.iter().any(|&[a, b]| s > a && s < b) {
                let f = t.frame(s);
                let lat = side * (f.wall_r + 4.2);
                let p = t.point_at(s, lat);
                if Valley::in_valley(self.tt, p.x, 0.5)
                    && Valley::clear_of_road(t, p.x, p.z, 3.5, None, 60.0)
                {
                    poles.push(Pole {
                        x: p.x,
                        y: self.gy(p.x, p.z) - 0.3,
                        z: p.z,
                        rx: f.rx,
                        rz: f.rz,
                        yaw: 0.0,
                    });
                }
            }
            s += spacing;
        }
        if poles.is_empty() {
            return;
        }
        let geo = parts::pole_geometry(&mut Builder::new()).expect("the pole merges");
        let mat = self.g.add_material(
            Material::standard()
                .set("color", 0x5a4a3b)
                .set("roughness", 0.95),
        );
        let geo = self.g.add_geometry(geo);
        let im = self.g.instanced_mesh(geo, mat, poles.len() as u32);
        for (k, p) in poles.iter_mut().enumerate() {
            let kf = k as f64;
            p.yaw = yaw_x(p.rx, p.rz) + (hash2(kf, 3.0, 0.0) - 0.5) * 0.06;
            let e = Euler::new(
                (hash2(kf, 5.0, 0.0) - 0.5) * 0.04,
                p.yaw,
                (hash2(kf, 7.0, 0.0) - 0.5) * 0.04,
            );
            let q = Quaternion::from_euler(&e);
            let m4 = Matrix4::compose(Vector3::new(p.x, p.y, p.z), q, Vector3::splat(1.0));
            self.instances_of(im).set_matrix_at(k, &m4);
        }
        {
            let o = self.g.get_mut(im);
            o.cast_shadow = true;
            o.receive_shadow = true;
        }
        self.g.compute_instance_bounding_sphere(im);
        self.g.add(self.group, im);
        // Wires with a catenary sag between neighbouring poles.
        let mut wpos = Vec::new();
        for k in 0..poles.len() - 1 {
            let (a, b) = (&poles[k], &poles[k + 1]);
            let span = kernel::hypot(b.x - a.x, b.z - a.z);
            if span > 80.0 {
                continue;
            }
            let sag = 0.45 + span * 0.01;
            for off in [-0.95, 0.0, 0.95] {
                let ax = a.x + kernel::cos(a.yaw) * off;
                let az = a.z - kernel::sin(a.yaw) * off;
                let ay = a.y + 8.85;
                let bx = b.x + kernel::cos(b.yaw) * off;
                let bz = b.z - kernel::sin(b.yaw) * off;
                let by = b.y + 8.85;
                const N: usize = 10;
                for i in 0..N {
                    for u in [i as f64 / N as f64, (i + 1) as f64 / N as f64] {
                        wpos.extend([
                            lerp(ax, bx, u),
                            lerp(ay, by, u) - sag * 4.0 * u * (1.0 - u),
                            lerp(az, bz, u),
                        ]);
                    }
                }
            }
        }
        let mut wg = BufferGeometry::new();
        wg.set_attribute("position", BufferAttribute::from_f64(&wpos, 3));
        let wmat = self
            .g
            .add_material(Material::line_basic().set("color", 0x1a1816));
        let wgeo = self.g.add_geometry(wg);
        let wires = self.g.drawable(NodeType::LineSegments, wgeo, wmat);
        self.g.get_mut(wires).matrix_auto_update = false;
        self.g.add(self.group, wires);
        self.poles = poles.len();
    }

    fn instances_of(&mut self, id: NodeId) -> &mut crate::object::Instances {
        self.g
            .get_mut(id)
            .instances
            .as_mut()
            .expect("an instanced mesh")
    }

    fn build_entry_sign(&mut self) {
        let t = self.t;
        let s = self.v.v0 as f64 + 70.0;
        let f = t.frame(s);
        let p = t.point_at(s, f.wall_r + 3.2);
        let gy = self.gy(p.x, p.z);
        self.b.set_frame(p.x, gy, p.z, yaw_z(-f.fx, -f.fz) + 0.25);
        parts::board_sign(&mut self.b, "signValley", 3.6, 1.35, 0.0, 0.0, 0.0, 1.0);
    }

    // ── Trees ──────────────────────────────────────────────────────

    /// The JS default r is 2.
    fn tree_ok(&self, x: f64, z: f64, r: f64) -> bool {
        if !Valley::clear_of_road(self.t, x, z, r + 3.0, None, 60.0) {
            return false;
        }
        for a in &self.avoid {
            let dx = x - a.p[0];
            let dz = z - a.p[1];
            if dx * dx + dz * dz < (a.r + r) * (a.r + r) {
                return false;
            }
        }
        true
    }

    /// `addTree(x, z, kind, rng, r0 = 0)`.
    fn add_tree(&mut self, x: f64, z: f64, kind: TreeKind, rng: &mut Mulberry32, r0: f64) -> bool {
        if !self.tree_ok(x, z, js::or(r0, 2.5)) {
            return false;
        }
        if !Valley::in_valley(self.tt, x, 0.55) {
            return false;
        }
        let y = self.gy(x, z);
        let h = hash2((x * 3.0).floor(), (z * 3.0).floor(), 9.0);
        let (r, sy, th, mut col) = match kind {
            TreeKind::Poplar => (
                rrange(rng, 1.3, 1.8),
                rrange(rng, 3.2, 4.3),
                1.4,
                [0.19, 0.3, 0.12],
            ),
            TreeKind::Orchard => (rrange(rng, 1.6, 2.2), 0.85, 1.1, [0.3, 0.42, 0.16]),
            TreeKind::Willow => (rrange(rng, 3.5, 5.2), 0.8, 1.8, [0.33, 0.42, 0.18]),
            TreeKind::Hill => (
                rrange(rng, 3.0, 5.0),
                rrange(rng, 1.0, 1.5),
                1.5,
                [0.16, 0.24, 0.1],
            ),
            TreeKind::Shade => {
                let r = rrange(rng, 2.8, 4.8);
                let sy = rrange(rng, 0.9, 1.2);
                let th = rrange(rng, 1.8, 3.0);
                (r, sy, th, [0.24, 0.35, 0.13])
            }
        };
        // Late-summer variety: some yellowing, some darker.
        let v = rng.next_f64();
        let k = 0.8 + h * 0.4;
        col = col.map(|c| c * k);
        if v < 0.15 && kind != TreeKind::Poplar {
            col[0] *= 1.5;
            col[1] *= 1.15;
        }
        let yaw = rng.next_f64() * PI * 2.0;
        self.trees.push(Tree {
            x,
            y,
            z,
            kind,
            r,
            sy,
            th,
            col,
            yaw,
        });
        true
    }

    /// The valley road's bounds (every tenth sample), unpadded.
    fn road_bounds(&self) -> [f64; 4] {
        let t = self.t;
        let (mut min_x, mut max_x, mut min_z, mut max_z) = (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        );
        let mut i = self.v.v0;
        while i < self.v.v1 {
            min_x = js::min(min_x, at(&t.px, i));
            max_x = js::max(max_x, at(&t.px, i));
            min_z = js::min(min_z, at(&t.pz, i));
            max_z = js::max(max_z, at(&t.pz, i));
            i += 10;
        }
        [min_x, max_x, min_z, max_z]
    }

    fn scatter_hedgerows(&mut self) {
        let mut rng = Mulberry32::new(1717);
        let tt = self.tt;
        let n = Noise2D::new(313);
        // Bounds of the valley road, padded.
        let [mut min_x, mut max_x, mut min_z, mut max_z] = self.road_bounds();
        min_x -= 700.0;
        max_x += 700.0;
        min_z -= 800.0;
        max_z += 800.0;
        let corners = [
            [min_x, min_z],
            [max_x, min_z],
            [min_x, max_z],
            [max_x, max_z],
        ]
        .map(|[x, z]| to_uv(x, z));
        let u0 = js::min_n(&corners.map(|c| c[0]));
        let u1 = js::max_n(&corners.map(|c| c[0]));
        let v0 = js::min_n(&corners.map(|c| c[1]));
        let v1 = js::max_n(&corners.map(|c| c[1]));
        let try_at = |s: &mut Bld, rng: &mut Mulberry32, u: f64, v: f64| {
            let [x, z] = from_uv(u, v);
            if x < min_x || x > max_x || z < min_z || z > max_z {
                return;
            }
            if n.noise(x / 90.0, z / 90.0) < 0.05 {
                return;
            }
            if rng.next_f64() > 0.6 {
                return;
            }
            if !Valley::in_valley(tt, x, 0.8) {
                return;
            }
            let info = tt.road_info(x, z);
            if info.d < 22.0 || info.d > 820.0 {
                return;
            }
            if tt.slope_at(x, z, 3.0) > 0.22 {
                return;
            }
            let tx = x + rrange(rng, -1.5, 1.5);
            let tz = z + rrange(rng, -1.5, 1.5);
            s.add_tree(tx, tz, TreeKind::Shade, rng, 0.0);
        };
        let mut u = (u0 / FIELD_U).ceil() * FIELD_U;
        while u < u1 {
            let mut v = v0;
            while v < v1 {
                try_at(self, &mut rng, u, v);
                v += 8.5;
            }
            u += FIELD_U;
        }
        let mut v = (v0 / FIELD_V).ceil() * FIELD_V;
        while v < v1 {
            let mut u = u0;
            while u < u1 {
                try_at(self, &mut rng, u, v);
                u += 8.5;
            }
            v += FIELD_V;
        }
        // Continuous hedge (hawthorn, bramble) along the same boundaries as the
        // trees, so field edges read as lines of green rather than dotted trees.
        self.hedges = Vec::new();
        let hedge_at = |s: &mut Bld, rng: &mut Mulberry32, u: f64, v: f64| {
            let du = rrange(rng, -0.8, 0.8);
            let dv = rrange(rng, -0.8, 0.8);
            let [x, z] = from_uv(u + du, v + dv);
            if x < min_x || x > max_x || z < min_z || z > max_z {
                return;
            }
            if n.noise(x / 90.0, z / 90.0) < 0.0 {
                return;
            }
            if !Valley::in_valley(tt, x, 0.8) {
                return;
            }
            let info = tt.road_info(x, z);
            if info.d < 16.0 || info.d > 450.0 {
                return;
            }
            if !s.tree_ok(x, z, 1.0) {
                return;
            }
            let k = rrange(rng, 1.1, 1.9);
            let y = s.gy(x, z);
            let sx = k * rrange(rng, 1.2, 1.7);
            let sy = k * rrange(rng, 0.8, 1.2);
            let sz = k * rrange(rng, 1.2, 1.7);
            let yaw = rng.next_f64() * 6.28;
            let c = rng.next_f64();
            s.hedges.push(Hedge {
                x,
                z,
                y,
                sx,
                sy,
                sz,
                yaw,
                c,
            });
        };
        let mut u = (u0 / FIELD_U).ceil() * FIELD_U;
        while u < u1 {
            let mut v = v0;
            while v < v1 {
                hedge_at(self, &mut rng, u, v);
                v += 3.2;
            }
            u += FIELD_U;
        }
        let mut v = (v0 / FIELD_V).ceil() * FIELD_V;
        while v < v1 {
            let mut u = u0;
            while u < u1 {
                hedge_at(self, &mut rng, u, v);
                u += 3.2;
            }
            v += FIELD_V;
        }
    }

    fn scatter_creek_trees(&mut self) {
        let mut rng = Mulberry32::new(2323);
        let rng = &mut rng;
        let pts = self.v.creek.clone();
        for i in 1..pts.len().saturating_sub(1) {
            let (a, b) = (pts[i - 1], pts[i + 1]);
            let dx = b.x - a.x;
            let dz = b.z - a.z;
            let l = kernel::hypot(dx, dz);
            for side in [-1.0, 1.0] {
                if rng.next_f64() > 0.55 {
                    continue;
                }
                let off = rrange(rng, 12.5, 18.0);
                let x = pts[i].x - (dz / l) * off * side + rrange(rng, -3.0, 3.0);
                let z = pts[i].z + (dx / l) * off * side + rrange(rng, -3.0, 3.0);
                // Creek banks are in the avoid list; allow these by testing road/buildings only.
                if !Valley::clear_of_road(self.t, x, z, 6.0, None, 60.0) {
                    continue;
                }
                if !Valley::in_valley(self.tt, x, 0.6) {
                    continue;
                }
                let kind = if rng.next_f64() < 0.5 {
                    TreeKind::Willow
                } else {
                    TreeKind::Shade
                };
                let tr = self.make_tree_record(x, z, kind, rng);
                self.trees.push(tr);
            }
        }
    }

    fn make_tree_record(&mut self, x: f64, z: f64, kind: TreeKind, rng: &mut Mulberry32) -> Tree {
        let save_len = self.trees.len();
        let save_avoid = std::mem::take(&mut self.avoid);
        self.avoid = save_avoid.iter().filter(|a| !a.creek).copied().collect();
        self.add_tree(x, z, kind, rng, 0.0);
        self.avoid = save_avoid;
        if self.trees.len() > save_len {
            self.trees.pop().expect("pushed")
        } else {
            Tree {
                x,
                z,
                y: self.gy(x, z),
                kind,
                r: 3.0,
                sy: 1.0,
                th: 2.0,
                col: [0.25, 0.35, 0.13],
                yaw: 0.0,
            }
        }
    }

    fn scatter_hill_trees(&mut self) {
        let mut rng = Mulberry32::new(5151);
        let rng = &mut rng;
        let tt = self.tt;
        let [min_x, max_x, min_z, max_z] = self.road_bounds();
        let mut placed = 0;
        let mut k = 0;
        while k < 9000 && placed < 520 {
            k += 1;
            let x = rrange(rng, min_x - 1300.0, max_x + 1300.0);
            let z = rrange(rng, min_z - 1300.0, max_z + 1300.0);
            if !Valley::in_valley(tt, x, 0.85) {
                continue;
            }
            let info = tt.road_info(x, z);
            if info.d < 480.0 || info.d > 1500.0 {
                continue;
            }
            let mask = smoothstep(0.05, 0.3, fbm(&tt.noise2, x / 300.0 + 3.0, z / 300.0, 3))
                * smoothstep(500.0, 900.0, info.d);
            if mask < 0.55 {
                continue;
            }
            // Clusters: plant a few around the seed point.
            let nc = 3 + (rng.next_f64() * 4.0).floor() as usize;
            for _ in 0..nc {
                let cx = x + rrange(rng, -14.0, 14.0);
                let cz = z + rrange(rng, -14.0, 14.0);
                if tt.slope_at(cx, cz, 4.0) > 0.7 {
                    continue;
                }
                if self.add_tree(cx, cz, TreeKind::Hill, rng, 0.0) {
                    placed += 1;
                }
            }
        }
    }

    // ── Hay bales in the hay/wheat parcels ─────────────────────────

    fn parcel_range(&self, pad: f64) -> ([f64; 4], i64, i64, i64, i64) {
        let [mut min_x, mut max_x, mut min_z, mut max_z] = self.road_bounds();
        min_x -= pad;
        max_x += pad;
        min_z -= pad;
        max_z += pad;
        let corners = [
            [min_x, min_z],
            [max_x, min_z],
            [min_x, max_z],
            [max_x, max_z],
        ]
        .map(|[x, z]| to_uv(x, z));
        let fu0 = (js::min_n(&corners.map(|c| c[0])) / FIELD_U).floor() as i64;
        let fu1 = (js::max_n(&corners.map(|c| c[0])) / FIELD_U).ceil() as i64;
        let fv0 = (js::min_n(&corners.map(|c| c[1])) / FIELD_V).floor() as i64;
        let fv1 = (js::max_n(&corners.map(|c| c[1])) / FIELD_V).ceil() as i64;
        ([min_x, max_x, min_z, max_z], fu0, fu1, fv0, fv1)
    }

    fn scatter_bales(&mut self) {
        let mut rng = Mulberry32::new(8080);
        let rng = &mut rng;
        let tt = self.tt;
        let ([min_x, max_x, min_z, max_z], fu0, fu1, fv0, fv1) = self.parcel_range(600.0);
        for fu in fu0..=fu1 {
            for fv in fv0..=fv1 {
                let (fuf, fvf) = (fu as f64, fv as f64);
                let h = hash2(fuf, fvf, 3.0);
                let col = (h * 8.0).floor() as i64;
                if col != 3 && col != 0 && col != 6 {
                    continue;
                }
                if rng.next_f64() < (if col == 3 { 0.2 } else { 0.6 }) {
                    continue;
                }
                let rows = 2 + (rng.next_f64() * 3.0).floor() as i64;
                for r in 0..rows {
                    let vv =
                        (fvf + 0.2 + (r as f64 / js::max(1.0, (rows - 1) as f64)) * 0.6) * FIELD_V;
                    for q in 0..6 {
                        if rng.next_f64() < 0.3 {
                            continue;
                        }
                        let uu =
                            (fuf + 0.15 + f64::from(q) * 0.13 + rrange(rng, -0.03, 0.03)) * FIELD_U;
                        let [x, z] = from_uv(uu, vv + rrange(rng, -3.0, 3.0));
                        if x < min_x || x > max_x || z < min_z || z > max_z {
                            continue;
                        }
                        if !Valley::in_valley(tt, x, 0.85) {
                            continue;
                        }
                        let info = tt.road_info(x, z);
                        if info.d < 30.0 || info.d > 650.0 {
                            continue;
                        }
                        if tt.slope_at(x, z, 2.0) > 0.06 {
                            continue;
                        }
                        if !self.tree_ok(x, z, 1.0) {
                            continue;
                        }
                        let y = self.gy(x, z);
                        let yaw = rng.next_f64() * PI;
                        let k = 0.85 + rng.next_f64() * 0.25;
                        self.bales.push(Bale { x, z, y, yaw, k });
                    }
                }
            }
        }
    }

    // ── Crop rows ──────────────────────────────────────────────────
    // Parcels the terrain paints as a green crop get rows of corn, and the
    // lavender parcels get low mounded rows, following the painted furrows.
    // Only parcels near the road: that's where rows read as rows.

    fn scatter_crops(&mut self) {
        let tt = self.tt;
        let mut rng = Mulberry32::new(4545);
        let rng = &mut rng;
        self.crop_runs = Vec::new();
        // Slope on a coarse 10 m lattice is plenty to keep rows off banks, and
        // far cheaper than sampling the terrain at every plant. (A pure
        // function, memoised; the map is only looked up.)
        let mut slopes: BTreeMap<(i64, i64), f64> = BTreeMap::new();
        let mut slope_at = |x: f64, z: f64| -> f64 {
            let (rx, rz) = (js::round(x / 10.0), js::round(z / 10.0));
            *slopes
                .entry((rx as i64, rz as i64))
                .or_insert_with(|| tt.slope_at(rx * 10.0, rz * 10.0, 3.0))
        };
        let (_, fu0, fu1, fv0, fv1) = self.parcel_range(400.0);
        for fu in fu0..=fu1 {
            for fv in fv0..=fv1 {
                let (fuf, fvf) = (fu as f64, fv as f64);
                let h = hash2(fuf, fvf, 3.0);
                let col = (h * 8.0).floor() as i64;
                let kind = if col == 2 || col == 5 {
                    Crop::Corn
                } else if col == 7 {
                    Crop::Lav
                } else {
                    continue;
                };
                let [cx, cz] = from_uv((fuf + 0.5) * FIELD_U, (fvf + 0.5) * FIELD_V);
                if !Valley::in_valley(tt, cx, 0.6) || tt.road_info(cx, cz).d > 260.0 {
                    continue;
                }
                // Obstacles near this parcel only.
                let avoid: Vec<Avoid> = self
                    .avoid
                    .iter()
                    .filter(|a| !a.creek && kernel::hypot(a.p[0] - cx, a.p[1] - cz) < 120.0 + a.r)
                    .copied()
                    .collect();
                let near_creek = self.v.creek_dist(cx, cz) < 120.0;
                let along_v = h > 0.5; // matches the painted furrow direction
                let (aa, bn) = if along_v {
                    (FIELD_U, FIELD_V)
                } else {
                    (FIELD_V, FIELD_U)
                };
                let fa = if along_v { fuf } else { fvf };
                let fb = if along_v { fvf } else { fuf };
                let gap = if kind == Crop::Corn { 2.0 } else { 2.2 };
                let step = if kind == Crop::Corn { 3.0 } else { 3.5 };
                let mut a = fa * aa + 5.0 + 1.1;
                while a < (fa + 1.0) * aa - 5.0 {
                    let mut run: Vec<CropPt> = Vec::new();
                    let mut b = fb * bn + 5.0;
                    while b < (fb + 1.0) * bn - 5.0 {
                        let [x, z] = if along_v {
                            from_uv(a, b)
                        } else {
                            from_uv(b, a)
                        };
                        let mut ok = Valley::in_valley(tt, x, 0.85);
                        if ok {
                            let d = tt.road_info(x, z).d;
                            ok = d > 15.0 && d < 200.0;
                        }
                        if ok {
                            for o in &avoid {
                                if kernel::pow(x - o.p[0], 2.0) + kernel::pow(z - o.p[1], 2.0)
                                    < kernel::pow(o.r + 1.5, 2.0)
                                {
                                    ok = false;
                                    break;
                                }
                            }
                        }
                        if ok && near_creek {
                            ok = self.v.creek_dist(x, z) > 14.0;
                        }
                        if ok {
                            ok = slope_at(x, z) < 0.2;
                        }
                        if !ok {
                            if run.len() > 2 {
                                self.crop_runs.push(CropRun {
                                    kind,
                                    pts: std::mem::take(&mut run),
                                });
                            }
                            run.clear();
                            b += step;
                            continue;
                        }
                        let y = self.gy(x, z);
                        run.push(CropPt {
                            x,
                            z,
                            y,
                            r: rng.next_f64(),
                        });
                        b += step;
                    }
                    if run.len() > 2 {
                        self.crop_runs.push(CropRun { kind, pts: run });
                    }
                    a += gap;
                }
            }
        }
    }

    /// Grass tussocks and wildflowers on the verge between the road and the
    /// fence line, chunked along the road.
    fn scatter_verge(&mut self) {
        let t = self.t;
        let mut rng = Mulberry32::new(6262);
        let rng = &mut rng;
        let n = Noise2D::new(99);
        self.grass = Vec::new();
        self.flowers = Vec::new();
        let gaps = &t.fence_gaps;
        let mut s = self.v.v0 as f64 - 200.0;
        while s < self.v.v1 as f64 + 100.0 {
            let f = t.frame(s);
            for side in [-1.0, 1.0] {
                if gaps
                    .iter()
                    .any(|g| g.side == side && s > g.s0 - 2.0 && s < g.s1 + 2.0)
                {
                    continue;
                }
                let wall = if side > 0.0 { f.wall_r } else { f.wall_l };
                for _ in 0..3 {
                    let r1 = rng.next_f64();
                    let r2 = rng.next_f64();
                    let lat = side * (wall + 0.9 + r1 * r2 * 7.0);
                    let x = f.x + f.rx * lat + f.fx * (rng.next_f64() - 0.5) * 2.0;
                    let z = f.z + f.rz * lat + f.fz * (rng.next_f64() - 0.5) * 2.0;
                    if !Valley::in_valley(self.tt, x, 0.4)
                        || !Valley::clear_of_road(t, x, z, 0.4, None, 60.0)
                    {
                        continue;
                    }
                    let y = self.gy(x, z);
                    let lush = n.noise(x / 50.0, z / 50.0);
                    let sc = rrange(rng, 0.65, 1.15);
                    let sy = sc * rrange(rng, 0.8, 1.4);
                    let yaw = rng.next_f64() * 6.28;
                    let b = rrange(rng, 0.8, 1.1);
                    self.grass.push(Grass {
                        s,
                        x,
                        y: y - 0.05,
                        z,
                        sx: sc,
                        sy,
                        sz: sc,
                        yaw,
                        lush,
                        b,
                    });
                    if lush > -0.1 && rng.next_f64() < 0.3 {
                        let hue = ((n.noise(x / 18.0 + 7.0, z / 18.0) * 0.5 + 0.5) * 6.99).floor()
                            as usize;
                        let fx = x + rrange(rng, -0.5, 0.5);
                        let fz = z + rrange(rng, -0.5, 0.5);
                        let sy = rrange(rng, 0.8, 1.3);
                        let yaw = rng.next_f64() * 6.28;
                        self.flowers.push(Flower {
                            s,
                            x: fx,
                            y: y - 0.02,
                            z: fz,
                            sx: 1.0,
                            sy,
                            sz: 1.0,
                            yaw,
                            hue,
                        });
                    }
                }
            }
            s += 2.2;
        }
    }

    // ── Instanced meshes ───────────────────────────────────────────

    /// One InstancedMesh from records (the JS defaults: `cast = false`,
    /// `receive = true`).
    fn instances(
        &mut self,
        geo: crate::object::GeoId,
        mat: MaterialId,
        list: &[Inst],
        cast: bool,
        receive: bool,
    ) -> Option<NodeId> {
        if list.is_empty() {
            return None;
        }
        let im = self.g.instanced_mesh(geo, mat, list.len() as u32);
        for (k, r) in list.iter().enumerate() {
            let q = Quaternion::from_euler(&Euler::new(0.0, js::or(r.yaw, 0.0), 0.0));
            let m4 = Matrix4::compose(
                Vector3::new(r.x, r.y, r.z),
                q,
                Vector3::new(r.sx, r.sy, r.sz),
            );
            let inst = self.instances_of(im);
            inst.set_matrix_at(k, &m4);
            if let Some(c) = r.col {
                inst.set_color_at(k, c);
            }
        }
        {
            let o = self.g.get_mut(im);
            o.cast_shadow = cast;
            o.receive_shadow = receive;
        }
        self.g.compute_instance_bounding_sphere(im);
        self.g.add(self.group, im);
        Some(im)
    }

    fn build_trees(&mut self) {
        if self.trees.is_empty() {
            return;
        }
        // A crown shape per kind: round orchard heads, columnar poplars, broad
        // lobed shade trees and drooping willows. Vertex colours bake the
        // occlusion; the instance colour is the tree's own green.
        let geos: Vec<(&str, BufferGeometry)> = vec![
            ("orchard", canopy_geometry("orchard", 5, 0)),
            ("poplar", canopy_geometry("poplar", 6, 0)),
            ("shade", canopy_geometry("shade", 7, 0)),
            ("willow", canopy_geometry("willow", 8, 0)),
            ("shadeFar", canopy_geometry("shade", 7, 1)),
        ];
        let canopy_mat = self.g.add_material(foliage_material(&[
            ("side", n(f64::from(three::FRONT_SIDE))),
            ("roughness", n(0.95)),
        ]));
        let mut trunk_geo = cylinder_geometry(0.6, 1.0, 1.0, 6.0, 1.0, true, 0.0, 2.0 * PI);
        trunk_geo.translate(0.0, 0.5, 0.0);
        let trunk_mat = self.g.add_material(
            Material::standard()
                .set("color", 0x4e3b2a)
                .set("roughness", 1.0),
        );
        let mut groups: Vec<(&'static str, Vec<Inst>)> = Vec::new();
        for tr in &self.trees {
            let kind = match tr.kind {
                TreeKind::Hill | TreeKind::Shade => "shade",
                TreeKind::Orchard => "orchard",
                TreeKind::Poplar => "poplar",
                TreeKind::Willow => "willow",
            };
            // Shade trees are the most numerous: only those near the road cast shadows.
            let key = if kind == "shade" {
                if self.tt.road_info(tr.x, tr.z).d < 70.0 {
                    "shade:near"
                } else {
                    "shade:far"
                }
            } else {
                kind
            };
            let col = Color::new(tr.col[0], tr.col[1], tr.col[2]);
            let rec = Inst {
                x: tr.x,
                y: tr.y + tr.th + tr.r * tr.sy * 0.8,
                z: tr.z,
                sx: tr.r,
                sy: tr.r * tr.sy,
                sz: tr.r,
                yaw: tr.yaw,
                col: Some(col),
            };
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, l)) => l.push(rec),
                None => groups.push((key, vec![rec])),
            }
        }
        let mut geo_ids: Vec<(&str, crate::object::GeoId)> = Vec::new();
        for (key, list) in &groups {
            let kind = if *key == "shade:far" {
                "shadeFar"
            } else {
                key.split(':').next().expect("a kind")
            };
            let gid = match geo_ids.iter().find(|(k, _)| *k == kind) {
                Some(&(_, g)) => g,
                None => {
                    let g = geos
                        .iter()
                        .find(|(k, _)| *k == kind)
                        .expect("a canopy")
                        .1
                        .clone();
                    let g = self.g.add_geometry(g);
                    geo_ids.push((kind, g));
                    g
                }
            };
            self.instances(gid, canopy_mat, list, *key != "shade:far", true);
        }
        let trunk_geo = self.g.add_geometry(trunk_geo);
        let tim = self
            .g
            .instanced_mesh(trunk_geo, trunk_mat, self.trees.len() as u32);
        for (k, tr) in self.trees.clone().iter().enumerate() {
            let q = Quaternion::from_euler(&Euler::new(0.0, tr.yaw, 0.0));
            let v = Vector3::new(tr.x, tr.y - 0.3, tr.z);
            let tw = 0.12 + tr.r * 0.045;
            let sc = Vector3::new(tw, tr.th + tr.r * tr.sy * 0.6 + 0.3, tw);
            let m4 = Matrix4::compose(v, q, sc);
            self.instances_of(tim).set_matrix_at(k, &m4);
        }
        self.g.get_mut(tim).cast_shadow = true;
        self.g.compute_instance_bounding_sphere(tim);
        self.g.add(self.group, tim);

        // Hedges, in a few spatial cells so off-screen ones are culled.
        if !self.hedges.is_empty() {
            let hedge_geo = self.g.add_geometry(shrub_geometry(44, 0.0));
            let mut cells: Vec<((i64, i64), Vec<Inst>)> = Vec::new();
            let tints = [
                [0.16, 0.22, 0.08],
                [0.2, 0.25, 0.09],
                [0.14, 0.2, 0.09],
                [0.24, 0.24, 0.1],
            ];
            for h in &self.hedges {
                let key = ((h.x / 900.0).floor() as i64, (h.z / 900.0).floor() as i64);
                let tc = tints[(h.c * tints.len() as f64).floor() as usize];
                let rec = Inst {
                    x: h.x,
                    y: h.y,
                    z: h.z,
                    sx: h.sx,
                    sy: h.sy,
                    sz: h.sz,
                    yaw: h.yaw,
                    col: Some(Color::new(tc[0], tc[1], tc[2])),
                };
                match cells.iter_mut().find(|(k, _)| *k == key) {
                    Some((_, l)) => l.push(rec),
                    None => cells.push((key, vec![rec])),
                }
            }
            for (_, list) in &cells {
                self.instances(hedge_geo, canopy_mat, list, false, true);
            }
        }
    }

    /// Round bales: a lathed drum with rounded shoulders, the rolled-up
    /// spiral on the ends painted as alternating rings in vertex colour.
    /// Some are wrapped in white or green plastic (silage).
    fn build_bales_mesh(&mut self) {
        if self.bales.is_empty() {
            return;
        }
        let (r, l) = (0.78, 1.25);
        let mut prof = Vec::new();
        for k in 0..=4 {
            prof.push(Vector2::new((f64::from(k) / 4.0) * (r - 0.12), l / 2.0));
        }
        for k in 1..=2 {
            let a = (f64::from(k) / 2.0) * PI / 2.0;
            prof.push(Vector2::new(
                r - 0.12 + kernel::sin(a) * 0.12,
                l / 2.0 - 0.12 + kernel::cos(a) * 0.12,
            ));
        }
        let n0 = prof.len();
        for k in (0..n0).rev() {
            prof.push(Vector2::new(prof[k].x, -prof[k].y));
        }
        let mut g = lathe_geometry(&prof, 11.0, 0.0, PI * 2.0);
        g.delete_attribute("uv");
        let p = g.position().clone();
        let mut col = vec![0f32; p.count() * 3];
        let mut rng = Mulberry32::new(3);
        for i in 0..p.count() {
            let rr = kernel::hypot(p.get_x(i), p.get_z(i));
            let y = p.get_y(i);
            let v = if y.abs() > l / 2.0 - 0.02 && rr < r - 0.12 {
                0.72 + 0.3 * (js::round(rr / ((r - 0.12) / 4.0)) % 2.0) // spiral rings
            } else {
                0.85 + rng.next_f64() * 0.25 // straw streaks
            };
            col[i * 3] = v as f32;
            col[i * 3 + 1] = (v * 0.97) as f32;
            col[i * 3 + 2] = (v * 0.9) as f32;
        }
        g.set_attribute("color", BufferAttribute::from_f32(col, 3));
        g.compute_vertex_normals();
        g.rotate_z(PI / 2.0);
        g.translate(0.0, r - 0.04, 0.0);
        let mat = self.g.add_material(
            Material::standard()
                .set("color", 0xffffff)
                .set("vertexColors", true)
                .set("roughness", 0.95)
                .set("side", DOUBLE),
        );
        let hay = Color::hex(0xd2ae62);
        let wrap_w = Color::hex(0xeeeeea);
        let wrap_g = Color::hex(0x4f6a3c);
        let list: Vec<Inst> = self
            .bales
            .iter()
            .map(|b| {
                let w = hash2(b.x.floor(), b.z.floor(), 11.0);
                let col = if w < 0.12 {
                    wrap_w
                } else if w < 0.18 {
                    wrap_g
                } else {
                    let mut c = hay;
                    c.multiply_scalar(b.k);
                    c.multiply(Color::new(1.0, 0.95, 0.85));
                    c
                };
                Inst {
                    col: Some(col),
                    ..Inst::at(b.x, b.y - 0.05, b.z, b.yaw)
                }
            })
            .collect();
        let geo = self.g.add_geometry(g);
        self.instances(geo, mat, &list, true, true);
    }

    /// Crop rows, draped on the ground; one merged mesh per 600 m cell and
    /// crop so they cull with the camera. Corn is a pair of back-to-back
    /// cut-out strips painted with stalks, leaves and tassels (a ragged,
    /// see-through silhouette for two triangles a side); lavender is a low
    /// mounded ridge, purple on top.
    fn build_crops_mesh(&mut self) {
        if self.crop_runs.is_empty() {
            return;
        }
        #[derive(Default)]
        struct Cell {
            pos: Vec<f64>,
            nrm: Vec<f64>,
            col: Vec<f64>,
            uv: Vec<f64>,
            idx: Vec<u32>,
            corn: bool,
        }
        let mut cells: Vec<(String, Cell)> = Vec::new();
        for run in &self.crop_runs {
            let key = format!(
                "{}:{},{}",
                if run.kind == Crop::Corn {
                    "corn"
                } else {
                    "lav"
                },
                (run.pts[0].x / 600.0).floor(),
                (run.pts[0].z / 600.0).floor()
            );
            let gi = match cells.iter().position(|(k, _)| *k == key) {
                Some(i) => i,
                None => {
                    cells.push((
                        key,
                        Cell {
                            corn: run.kind == Crop::Corn,
                            ..Cell::default()
                        },
                    ));
                    cells.len() - 1
                }
            };
            let cg = &mut cells[gi].1;
            let pts = &run.pts;
            let mut along = 0.0;
            let last = pts.len() - 1;
            for i in 0..pts.len() {
                let a = pts[i.saturating_sub(1)];
                let b = pts[(i + 1).min(last)];
                let mut dx = b.x - a.x;
                let mut dz = b.z - a.z;
                let l = js::or(kernel::hypot(dx, dz), 1.0);
                dx /= l;
                dz /= l;
                let nx = -dz;
                let nz = dx;
                let p = pts[i];
                if i > 0 {
                    along += kernel::hypot(p.x - pts[i - 1].x, p.z - pts[i - 1].z);
                }
                let v0 = (cg.pos.len() / 3) as u32;
                if run.kind == Crop::Corn {
                    let h = lerp(2.0, 2.5, p.r) * (if i == 0 || i == last { 0.8 } else { 1.0 });
                    // Two coincident sheets, wound opposite ways, each lit as if it
                    // faced up-and-out so neither side goes black against the sun.
                    for sd in [1.0, -1.0] {
                        cg.pos.extend([p.x, p.y - 0.15, p.z, p.x, p.y + h, p.z]);
                        cg.nrm.extend([
                            nx * sd * 0.5,
                            0.85,
                            nz * sd * 0.5,
                            nx * sd * 0.5,
                            0.85,
                            nz * sd * 0.5,
                        ]);
                        cg.col.extend([0.55, 0.55, 0.5, 1.0, 1.0, 1.0]);
                        cg.uv.extend([along / 2.6, 0.0, along / 2.6, 1.0]);
                    }
                    if i > 0 {
                        let u0 = v0 - 4;
                        cg.idx.extend([u0, v0, u0 + 1, u0 + 1, v0, v0 + 1]);
                        cg.idx
                            .extend([u0 + 2, u0 + 3, v0 + 2, u0 + 3, v0 + 3, v0 + 2]);
                    }
                } else {
                    let w = 1.2;
                    let h = if i == 0 || i == last {
                        0.35
                    } else {
                        lerp(0.5, 0.75, p.r)
                    };
                    cg.pos.extend([
                        p.x - nx * w / 2.0,
                        p.y - 0.1,
                        p.z - nz * w / 2.0,
                        p.x,
                        p.y + h,
                        p.z,
                        p.x + nx * w / 2.0,
                        p.y - 0.1,
                        p.z + nz * w / 2.0,
                    ]);
                    cg.nrm.extend([
                        -nx * 0.8,
                        0.6,
                        -nz * 0.8,
                        0.0,
                        1.0,
                        0.0,
                        nx * 0.8,
                        0.6,
                        nz * 0.8,
                    ]);
                    let top = if p.r < 0.5 {
                        [0.2, 0.12, 0.38]
                    } else {
                        [0.26, 0.16, 0.42]
                    };
                    cg.col.extend([0.06, 0.08, 0.05]);
                    cg.col.extend(top);
                    cg.col.extend([0.06, 0.08, 0.05]);
                    cg.uv.extend([0.0; 6]);
                    if i > 0 {
                        let u0 = v0 - 3;
                        cg.idx.extend([u0, v0, u0 + 1, u0 + 1, v0, v0 + 1]);
                        cg.idx
                            .extend([u0 + 1, v0 + 1, u0 + 2, u0 + 2, v0 + 1, v0 + 2]);
                    }
                }
            }
        }
        let corn_tex = add_tex(self.g, corn_texture(), Some(true));
        let corn_mat = self.g.add_material(
            Material::standard()
                .set("map", corn_tex)
                .set("vertexColors", true)
                .set("alphaTest", 0.45)
                .set("roughness", 0.9)
                .set("color", 0xffffff),
        );
        let lav_mat = self
            .g
            .add_material(foliage_material(&[("roughness", n(0.95))]));
        for (_, c) in cells {
            let mut g = BufferGeometry::new();
            g.set_attribute("position", BufferAttribute::from_f64(&c.pos, 3));
            g.set_attribute("normal", BufferAttribute::from_f64(&c.nrm, 3));
            g.set_attribute("color", BufferAttribute::from_f64(&c.col, 3));
            g.set_attribute("uv", BufferAttribute::from_f64(&c.uv, 2));
            g.set_index(&c.idx);
            g.compute_bounding_sphere();
            let geo = self.g.add_geometry(g);
            let m = self.g.mesh(geo, if c.corn { corn_mat } else { lav_mat });
            let o = self.g.get_mut(m);
            o.receive_shadow = true;
            o.matrix_auto_update = false;
            self.g.add(self.group, m);
        }
    }

    fn build_verge_mesh(&mut self) {
        let mat = self.g.add_material(foliage_material(&[]));
        let g_geo = self.g.add_geometry(grass_clump_geometry(11, 15));
        let f_geo = self.g.add_geometry(flower_geometry(6, 19));
        let green = [0x9cc070, 0xb8cc80, 0x86ad5e].map(Color::hex);
        let straw = [0xffffff, 0xf4e2b8, 0xe8d8a8].map(Color::hex);
        let bloom = [
            0xff4a2a, 0xffd23a, 0xfff4e0, 0x8a6ee0, 0xff8a2a, 0x6e8ef0, 0xe86aa0,
        ]
        .map(Color::hex);
        const CH: f64 = 600.0;
        fn chunk<T>(list: &[T], s: impl Fn(&T) -> f64, f: impl Fn(&T) -> Inst) -> Vec<Vec<Inst>> {
            let mut out: Vec<(i64, Vec<Inst>)> = Vec::new();
            for r in list {
                let k = (s(r) / CH).floor() as i64;
                match out.iter_mut().find(|(x, _)| *x == k) {
                    Some((_, l)) => l.push(f(r)),
                    None => out.push((k, vec![f(r)])),
                }
            }
            out.into_iter().map(|(_, l)| l).collect()
        }
        let grass = chunk(
            &self.grass,
            |r| r.s,
            |r| {
                let mut c = (if r.lush > -0.2 { green } else { straw })
                    [((r.b * 10.0).floor() as usize) % 3];
                c.multiply_scalar(r.b);
                Inst {
                    x: r.x,
                    y: r.y,
                    z: r.z,
                    sx: r.sx,
                    sy: r.sy,
                    sz: r.sz,
                    yaw: r.yaw,
                    col: Some(c),
                }
            },
        );
        for l in grass {
            self.instances(g_geo, mat, &l, false, true);
        }
        let flowers = chunk(
            &self.flowers,
            |r| r.s,
            |r| Inst {
                x: r.x,
                y: r.y,
                z: r.z,
                sx: r.sx,
                sy: r.sy,
                sz: r.sz,
                yaw: r.yaw,
                col: Some(bloom[r.hue % bloom.len()]),
            },
        );
        for l in flowers {
            self.instances(f_geo, mat, &l, false, true);
        }
    }

    fn build_cows(&mut self) {
        if self.cows.is_empty() {
            return;
        }
        let geo = parts::cow_geometry(&mut Builder::new()).expect("the cow merges");
        let mat = self.g.add_material(
            Material::standard()
                .set("color", 0xffffff)
                .set("roughness", 0.9),
        );
        let geo = self.g.add_geometry(geo);
        let im = self.g.instanced_mesh(geo, mat, self.cows.len() as u32);
        let palette = [0x5b3a26, 0x1d1b1a, 0xd9d2c4, 0x8a5a36, 0x2a2624];
        for (k, c) in self.cows.clone().iter().enumerate() {
            let q = Quaternion::from_euler(&Euler::new(0.0, c.yaw, 0.0));
            let m4 = Matrix4::compose(
                Vector3::new(c.x, self.gy(c.x, c.z), c.z),
                q,
                Vector3::splat(1.0),
            );
            let col = Color::hex(palette[(c.c * palette.len() as f64).floor() as usize]);
            let inst = self.instances_of(im);
            inst.set_matrix_at(k, &m4);
            inst.set_color_at(k, col);
        }
        self.g.get_mut(im).cast_shadow = true;
        self.g.compute_instance_bounding_sphere(im);
        self.g.add(self.group, im);
    }

    fn build_wheels(&mut self) {
        if !self.wheels.is_empty() {
            let geo =
                parts::windpump_wheel_geometry(&mut Builder::new()).expect("the wheel merges");
            let mat = self.g.add_material(
                Material::standard()
                    .set("color", 0x8f8a82)
                    .set("metalness", 0.5)
                    .set("roughness", 0.55)
                    .set("side", DOUBLE),
            );
            let geo = self.g.add_geometry(geo);
            let im = self.g.instanced_mesh(geo, mat, self.wheels.len() as u32);
            self.g.get_mut(im).cast_shadow = true;
            self.g.add(self.group, im);
            self.wheel_mesh = Some(im);
            // updateWheels(0)
            for (k, w) in self.wheels.clone().iter().enumerate() {
                let m4 = wheel_matrix(w);
                self.instances_of(im).set_matrix_at(k, &m4);
            }
            self.g.compute_instance_bounding_sphere(im);
        }
        if let Some((x, y, z, yaw)) = self.mill_wheel {
            let geo = parts::waterwheel_geometry(&mut Builder::new(), 3.2, 1.4)
                .expect("the waterwheel merges");
            let mat = self.g.add_material(
                Material::standard()
                    .set("color", 0x5c4634)
                    .set("roughness", 0.95),
            );
            let geo = self.g.add_geometry(geo);
            let m = self.g.mesh(geo, mat);
            let o = self.g.get_mut(m);
            o.position = Vector3::new(x, y, z);
            o.set_rotation(&Euler::with_order(0.0, yaw, 0.0, EulerOrder::YXZ));
            o.cast_shadow = true;
            self.waterwheel = Some((m, o.position, yaw));
            self.g.add(self.group, m);
        }
    }
}

/// A windpump wheel's instance matrix: Euler (0, yaw, a) in YXZ order.
fn wheel_matrix(w: &Wheel) -> Matrix4 {
    let q = Quaternion::from_euler(&Euler::with_order(0.0, w.yaw, w.a, EulerOrder::YXZ));
    Matrix4::compose(Vector3::new(w.x, w.y, w.z), q, Vector3::splat(1.0))
}

/// The updater Valley registers: `updateWheels(dt)` (the windpump wheels
/// turn, the waterwheel and the sails rotate), the creek's ripples scroll
/// and its tint follows the sky's horizon.
struct Wheels {
    wheels: Vec<Wheel>,
    wheel_mesh: Option<NodeId>,
    /// The node, its position, its yaw and the angle about X so far.
    waterwheel: Option<(NodeId, Vector3, f64, f64)>,
    /// The node, its position, its yaw and the angle about Z so far.
    sails: Option<(NodeId, Vector3, f64, f64)>,
    /// The water's material, its normal map and the map's offset.
    water: Option<(MaterialId, TextureId, [f64; 2])>,
    /// `world.sky.uniforms.uHorizon` (the sky's time of day and its `?t=`
    /// override): the horizon is the sky's at the player's s.
    horizon: Option<(SkyParams, Option<f64>)>,
}

impl crate::world::Animator for Wheels {
    fn update(&mut self, u: &UpdateCtx, out: &mut Vec<Edit>) {
        let dt = u.dt;
        if let Some(im) = self.wheel_mesh {
            for (k, w) in self.wheels.iter_mut().enumerate() {
                w.a += dt * w.speed;
                let m4 = wheel_matrix(w);
                out.push(Edit {
                    target: Handle::Node(im),
                    change: Change::InstanceMatrix {
                        index: k as u32,
                        matrix: m4.elements.map(|v| v as f32),
                    },
                });
            }
        }
        let transform = |node: NodeId, p: Vector3, e: Euler| {
            let q = Quaternion::from_euler(&e);
            Edit {
                target: Handle::Node(node),
                change: Change::Transform {
                    position: [p.x, p.y, p.z],
                    quaternion: [q.x, q.y, q.z, q.w],
                    scale: [1.0; 3],
                },
            }
        };
        if let Some((node, p, yaw, rx)) = &mut self.waterwheel {
            *rx -= dt * 0.6;
            out.push(transform(
                *node,
                *p,
                Euler::with_order(*rx, *yaw, 0.0, EulerOrder::YXZ),
            ));
        }
        if let Some((node, p, yaw, rz)) = &mut self.sails {
            *rz -= dt * 0.5;
            out.push(transform(
                *node,
                *p,
                Euler::with_order(-0.08, *yaw, *rz, EulerOrder::YXZ),
            ));
        }
        if let Some((mat, tex, off)) = &mut self.water {
            off[0] += dt * 0.02;
            off[1] -= dt * 0.035;
            out.push(Edit {
                target: Handle::Texture(*tex),
                change: Change::TextureOffset(*off),
            });
            if let Some((params, over)) = &self.horizon {
                let mut c = params.frame_at(u.s, *over).horizon;
                c.multiply_scalar(0.05);
                c.add(Color::hex(0x06222c));
                out.push(Edit {
                    target: Handle::Material(*mat),
                    change: Change::Color {
                        prop: "emissive",
                        rgb: [c.r, c.g, c.b],
                    },
                });
            }
        }
    }
}
