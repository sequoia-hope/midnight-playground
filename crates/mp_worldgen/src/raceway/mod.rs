//! `src/world/Raceway.js` (roadmap WP 7.4): Seaside Raceway, Level 5:
//! Laguna Seca, from survey data.
//!
//! The land, the racing line and the barrier positions are real (see
//! tools/seaside/build.py and src/levels/seaside/). This dresses it: red and
//! white kerbs through the corners, concrete walls with catch fencing where
//! OpenStreetMap has walls and tyre walls on the outside of the fast
//! corners, the start gantry and its lights, the grid, the pit lane, every
//! building and grandstand from OpenStreetMap, the bridges over the track,
//! the infield lake, and coast live oaks wherever the aerial photo shows a
//! crown.
//!
//! The port keeps the JS structure: [`Raceway::plan`] finds the corners;
//! [`Raceway::build`] runs the JS `build*` methods in the JS order on a
//! [`Bld`] (the JS `this` while it builds) into the group `raceway`. The
//! survey (`world.level.data`) is [`World::level_data`]. Raceway registers
//! no updater; its start lights follow the countdown through
//! `world.onCountdown` ([`World::on_countdown`]), which the race calls
//! every frame. `raceway/textures.js` is [`textures`].

// Index loops stay index loops, and the JS signatures stay (D52, D130).
#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

pub mod textures;

use std::f64::consts::PI;
use std::sync::Arc;

use mp_levels::survey::{Pt, SeasideData};
use mp_math::{Mulberry32, clamp, js, kernel, lerp, smoothstep};
use mp_track::{Frame, Track};

use crate::color::Color;
use crate::flora::canopy_geometry;
use crate::geom::{GeoBuilder, P2, P3, PrismOpts, StaticOpts, static_mesh};
use crate::material::Material;
use crate::mountain::kit::SurfaceSampler;
use crate::object::{Layer, MaterialId, NodeId, SceneGraph};
use crate::road::{ExtrudeOpts, Lat, ProfilePoint, extrude, runs};
use crate::textures::TextureCache;
use crate::three_geom::{
    BufferAttribute, BufferGeometry, Euler, Matrix4, Quaternion, Vector2, Vector3, circle_geometry,
    cylinder_geometry, icosahedron_geometry, triangulate_shape,
};
use crate::world::{Change, Edit, Handle, Scenery, SceneryInfo, World};

use textures::{
    banner_atlas, banner_rect, crowd_texture, fence_texture, kerb_texture, tyre_texture,
};

/// Concrete wall height.
const WALL_H: f64 = 1.05;
/// Catch fence above the wall.
const FENCE_H: f64 = 2.7;
/// Tyre wall depth.
const TYRE_W: f64 = 1.1;
/// Bridge deck underside above the road.
const CLEAR: f64 = 5.6;

const DOUBLE_SIDE: f64 = mp_scene::three::DOUBLE_SIDE as f64;

/// A corner: a run of road tighter than a 260 m radius, with the apex (the
/// tightest point) and which way it turns (+1 right, -1 left).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Corner {
    pub s0: usize,
    pub s1: usize,
    pub apex: usize,
    pub sign: f64,
    pub peak: f64,
}

/// A bridge over the track (`this.bridges`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bridge {
    pub s: f64,
    pub road: bool,
    pub deck_y: f64,
}

/// The Seaside Raceway scenery module.
pub struct Raceway {
    pub label: &'static str,
    pub zone: usize,
    /// `this.corners`.
    pub corners: Vec<Corner>,
    /// `this.kerbs`: the kerbs' ranges on the left (-1) and right (+1).
    pub kerbs: [Vec<[f64; 2]>; 2],
    /// `this.tyreRuns`, left and right.
    pub tyre_runs: [Vec<[f64; 2]>; 2],
    pub bridges: Vec<Bridge>,
    pub tree_count: usize,
    /// `this.lampMats`: the start lights' five columns.
    pub lamp_mats: Vec<MaterialId>,
    /// The group `raceway`, once built.
    pub group: Option<NodeId>,
}

/// Index of a side (-1 left, +1 right) in the per-side arrays.
fn si(side: f64) -> usize {
    usize::from(side > 0.0)
}

impl Raceway {
    /// `new Raceway({ zone })`.
    pub fn new(info: &SceneryInfo) -> Raceway {
        Raceway {
            label: "Building the raceway",
            zone: info.zone,
            corners: Vec::new(),
            kerbs: [Vec::new(), Vec::new()],
            tyre_runs: [Vec::new(), Vec::new()],
            bridges: Vec::new(),
            tree_count: 0,
            lamp_mats: Vec::new(),
            group: None,
        }
    }
}

/// `findCorners()`: runs of road tighter than a 260 m radius, with the apex
/// (the tightest point) and which way they turn (+1 right, -1 left).
pub fn find_corners(t: &Track) -> Vec<Corner> {
    let n = t.n;
    let k = |i: usize| f64::from(t.k_smooth[i]);
    let mut out: Vec<Corner> = Vec::new();
    let mut i = 0;
    while i < n {
        if k(i).abs() < 1.0 / 260.0 {
            i += 1;
            continue;
        }
        let sign = js::sign(k(i));
        let mut j = i;
        let mut apex = i;
        while j < n && js::sign(k(j)) == sign && k(j).abs() >= 1.0 / 260.0 {
            if k(j).abs() > k(apex).abs() {
                apex = j;
            }
            j += 1;
        }
        match out.last_mut() {
            Some(prev) if prev.sign == sign && (i as f64) - (prev.s1 as f64) < 15.0 => {
                prev.s1 = j;
                if k(apex).abs() > prev.peak {
                    prev.apex = apex;
                    prev.peak = k(apex).abs();
                }
            }
            _ => {
                if j - i > 8 {
                    out.push(Corner {
                        s0: i,
                        s1: j,
                        apex,
                        sign,
                        peak: k(apex).abs(),
                    });
                }
            }
        }
        i = j;
    }
    out
}

impl Scenery for Raceway {
    fn name(&self) -> &str {
        "Raceway"
    }

    fn label(&self) -> Option<&str> {
        Some(self.label)
    }

    fn plan(&mut self, world: &mut World) -> Result<(), String> {
        let t = world.track.as_ref().ok_or("the route is surveyed first")?;
        self.corners = find_corners(t);
        Ok(())
    }

    fn build(&mut self, world: &mut World) -> Result<(), String> {
        let data = world
            .level_data
            .clone()
            .ok_or("Seaside Raceway: prepare() first (no survey data)")?;
        let World {
            track,
            terrain,
            graph,
            textures,
            root,
            on_countdown,
            ..
        } = world;
        let t = track.as_ref().ok_or("the route is surveyed first")?;
        let terrain = terrain.as_ref().ok_or("no terrain")?;
        let group = graph.group("raceway");
        graph.add(*root, group);
        let mats = Mats::make(graph, textures);
        let mut b = Bld {
            t,
            ta: Arc::new(t.clone()),
            d: &data,
            graph,
            root: group,
            s: SurfaceSampler::new(terrain),
            m: mats,
            corners: self.corners.clone(),
            kerbs: [Vec::new(), Vec::new()],
            tyre_runs: [Vec::new(), Vec::new()],
            bridges: Vec::new(),
            tree_count: 0,
            lamp_mats: Vec::new(),
        };
        b.build_verges();
        b.build_kerbs();
        b.build_barriers();
        b.build_start();
        b.build_pit_lane();
        b.build_buildings();
        b.build_grandstands();
        b.build_bridges();
        b.build_lake();
        b.build_boards();
        b.build_trees();
        self.kerbs = b.kerbs;
        self.tyre_runs = b.tyre_runs;
        self.bridges = b.bridges;
        self.tree_count = b.tree_count;
        self.lamp_mats = b.lamp_mats.clone();
        self.group = Some(group);
        let lamps = b.lamp_mats;
        *on_countdown = Some(Box::new(move |cd: f64, out: &mut Vec<Edit>| {
            countdown(&lamps, cd, out)
        }));
        Ok(())
    }
}

/// `world.onCountdown = (cd, racing) => ...`: the start lights light one
/// column a second through the countdown and go out at GO. `cd` is the
/// seconds left (3.999 → 0), or -1 once racing.
pub fn countdown(lamps: &[MaterialId], cd: f64, out: &mut Vec<Edit>) {
    let lit = if cd > 0.0 {
        clamp(((4.0 - cd) * 5.0 / 4.0).ceil(), 0.0, 5.0)
    } else {
        0.0
    };
    for (i, &m) in lamps.iter().enumerate() {
        out.push(Edit {
            target: Handle::Material(m),
            change: Change::Number {
                prop: "emissiveIntensity",
                value: if (i as f64) < lit { 6.0 } else { 0.0 },
            },
        });
    }
}

/// `this.mats`.
struct Mats {
    concrete: MaterialId,
    verge: MaterialId,
    kerb: MaterialId,
    tyre: MaterialId,
    fence: MaterialId,
    steel: MaterialId,
    solid: MaterialId,
    paint: MaterialId,
    banner: MaterialId,
    crowd: MaterialId,
}

impl Mats {
    fn make(graph: &mut SceneGraph, textures: &mut TextureCache) -> Mats {
        let concrete_tex = graph.cached_texture(&textures.concrete_texture(), Layer::Main, "");
        let concrete = graph.add_material(
            Material::standard()
                .set("map", concrete_tex)
                .set("color", 0xe4e0d8)
                .set("roughness", 0.9)
                .set("side", DOUBLE_SIDE),
        );
        let concrete_tex = graph.cached_texture(&textures.concrete_texture(), Layer::Main, "");
        let verge = graph.add_material(
            Material::standard()
                .set("map", concrete_tex)
                .set("color", 0x5a5854)
                .set("roughness", 0.95),
        );
        let kt = graph.cached_texture(&kerb_texture(textures), Layer::Main, "");
        let kerb = graph.add_material(
            Material::standard()
                .set("map", kt)
                .set("roughness", 0.7)
                .set("polygonOffset", true)
                .set("polygonOffsetFactor", -2.0)
                .set("polygonOffsetUnits", -2.0),
        );
        let tt = graph.cached_texture(&tyre_texture(textures), Layer::Main, "");
        let tyre = graph.add_material(
            Material::standard()
                .set("map", tt)
                .set("roughness", 0.85)
                .set("side", DOUBLE_SIDE),
        );
        let ft = graph.cached_texture(&fence_texture(textures), Layer::Main, "");
        let fence = graph.add_material(
            Material::standard()
                .set("map", ft)
                .set("alphaTest", 0.35)
                .set("transparent", false)
                .set("roughness", 0.6)
                .set("metalness", 0.4)
                .set("side", DOUBLE_SIDE),
        );
        let steel = graph.add_material(
            Material::standard()
                .set("color", 0x8c9196)
                .set("roughness", 0.5)
                .set("metalness", 0.5),
        );
        let solid = graph.add_material(
            Material::standard()
                .set("vertexColors", true)
                .set("roughness", 0.85),
        );
        let paint = graph.add_material(
            Material::standard()
                .set("vertexColors", true)
                .set("roughness", 0.6)
                .set("polygonOffset", true)
                .set("polygonOffsetFactor", -3.0)
                .set("polygonOffsetUnits", -3.0),
        );
        let bt = graph.cached_texture(&banner_atlas(textures), Layer::Main, "");
        let banner = graph.add_material(Material::standard().set("map", bt).set("roughness", 0.7));
        let ct = graph.cached_texture(&crowd_texture(textures), Layer::Main, "");
        let crowd = graph.add_material(Material::standard().set("map", ct).set("roughness", 0.95));
        Mats {
            concrete,
            verge,
            kerb,
            tyre,
            fence,
            steel,
            solid,
            paint,
            banner,
            crowd,
        }
    }
}

/// A tree to plant: `{ x, z, y, r, hi }`.
#[derive(Clone, Copy, Debug)]
struct Tree {
    x: f64,
    z: f64,
    y: f64,
    r: f64,
    hi: bool,
}

/// The JS `this` while Raceway builds.
struct Bld<'a> {
    t: &'a Track,
    /// The track again, for the extrusion profiles' closures.
    ta: Arc<Track>,
    d: &'a SeasideData,
    graph: &'a mut SceneGraph,
    /// `this.root`: the group `raceway`.
    root: NodeId,
    /// `this.S`: the rendered-surface sampler (its height cache is shared
    /// by every builder, in the JS order).
    s: SurfaceSampler<'a>,
    m: Mats,
    corners: Vec<Corner>,
    kerbs: [Vec<[f64; 2]>; 2],
    tyre_runs: [Vec<[f64; 2]>; 2],
    bridges: Vec<Bridge>,
    tree_count: usize,
    lamp_mats: Vec<MaterialId>,
}

/// `wall(f, side)`: the wall distance on a side.
fn wall(f: &Frame, side: f64) -> f64 {
    if side < 0.0 { f.wall_l } else { f.wall_r }
}

/// A quad's four corners' uvs, `[[u0, v0], [u1, v0], [u1, v1], [u0, v1]]`.
fn rect_uvs(r: [f64; 4]) -> [P2; 4] {
    let [u0, v0, u1, v1] = r;
    [[u0, v0], [u1, v0], [u1, v1], [u0, v1]]
}

impl Bld<'_> {
    /// `this.add(mesh)`.
    fn add(&mut self, node: NodeId) -> NodeId {
        self.graph.add(self.root, node);
        node
    }

    /// `new THREE.Mesh(g, mat)`, added.
    fn mesh(&mut self, g: BufferGeometry, mat: MaterialId) -> NodeId {
        let geo = self.graph.add_geometry(g);
        let m = self.graph.mesh(geo, mat);
        self.add(m)
    }

    /// `staticMesh(g, mat, { cast })`, added.
    fn static_mesh(&mut self, g: BufferGeometry, mat: MaterialId, cast: bool) -> NodeId {
        let geo = self.graph.add_geometry(g);
        let m = static_mesh(
            self.graph,
            geo,
            mat,
            &StaticOpts {
                cast,
                ..StaticOpts::default()
            },
        );
        self.add(m)
    }

    // ── Verges: a strip of old tarmac past each white line, stepping down
    // to the run-off, so the road's edge never floats over the ground. ──
    fn build_verges(&mut self) {
        let len = self.t.length;
        for side in [-1.0, 1.0] {
            // Heights from the run-off surface (it leaves the road's plane at
            // the edge), and the outer lip tucked under the ground.
            let at = |ta: &Arc<Track>, lat: f64, dy: f64, u: f64| {
                let ta = ta.clone();
                ProfilePoint::new(Lat::f(move |f, _| side * (f.hw + lat)))
                    .abs()
                    .dy(Lat::f(move |f, s| {
                        ta.surface_y(s, side * (f.hw + lat)) + dy
                    }))
                    .u(u)
            };
            let mut prof = vec![
                at(&self.ta, -0.05, -0.004, 0.0),
                at(&self.ta, 1.2, -0.03, 0.5),
                at(&self.ta, 1.6, -0.45, 1.0),
            ];
            if side < 0.0 {
                prof.reverse();
            }
            let mut s0 = 0.0;
            while s0 < len {
                let s1 = js::min(len, s0 + 600.0) + if s0 + 600.0 >= len { 1.0 } else { 0.0 };
                let g = extrude(
                    self.t,
                    &[[s0, s1]],
                    &prof,
                    &ExtrudeOpts {
                        v_scale: 5.0,
                        ..ExtrudeOpts::default()
                    },
                );
                let verge = self.m.verge;
                let m = self.mesh(g, verge);
                self.graph.get_mut(m).receive_shadow = true;
                s0 += 600.0;
            }
        }
    }

    // ── Kerbs: on the inside through each apex, on the outside out of the
    // exit, and on the outside of the entry of the slow corners. ──
    fn build_kerbs(&mut self) {
        let mut spans: [Vec<[f64; 2]>; 2] = [Vec::new(), Vec::new()];
        for c in &self.corners {
            let inside = c.sign; // a right-hander's inside is the right
            let len = c.s1 as f64 - c.s0 as f64;
            let r = 1.0 / c.peak;
            let half = clamp(len * 0.3, 10.0, 40.0);
            let apex = c.apex as f64;
            spans[si(inside)].push([apex - half, apex + half]);
            spans[si(-inside)].push([apex, js::min(c.s1 as f64 + 18.0, apex + len)]);
            if r < 60.0 {
                spans[si(-inside)].push([c.s0 as f64 - 12.0, c.s0 as f64 + 10.0]);
            }
        }
        for side in [-1.0, 1.0] {
            // Merge overlaps.
            let mut list = std::mem::take(&mut spans[si(side)]);
            list.sort_by(|a, b| (a[0] - b[0]).partial_cmp(&0.0).expect("finite"));
            let mut merged: Vec<[f64; 2]> = Vec::new();
            for r in &list {
                match merged.last_mut() {
                    Some(last) if r[0] <= last[1] + 6.0 => last[1] = js::max(last[1], r[1]),
                    _ => merged.push(*r),
                }
            }
            // Whole blocks so the paint ends on a join.
            let ranges: Vec<[f64; 2]> = merged
                .iter()
                .map(|&[a, b]| [js::round(a / 2.0) * 2.0, js::round(b / 2.0) * 2.0])
                .collect();
            spans[si(side)] = ranges.clone();
            let mut prof = vec![
                ProfilePoint::new(Lat::f(move |f, _| side * (f.hw - 0.25)))
                    .dy(0.004)
                    .u(0.0),
                ProfilePoint::new(Lat::f(move |f, _| side * (f.hw + 0.05)))
                    .dy(0.055)
                    .u(0.25),
                ProfilePoint::new(Lat::f(move |f, _| side * (f.hw + 0.95)))
                    .dy(0.035)
                    .u(0.95),
                ProfilePoint::new(Lat::f(move |f, _| side * (f.hw + 1.1)))
                    .dy(-0.06)
                    .u(1.0),
            ];
            if side < 0.0 {
                prof.reverse();
            }
            let g = extrude(
                self.t,
                &ranges,
                &prof,
                &ExtrudeOpts {
                    step: 1.0,
                    v_scale: 2.4,
                    color: None,
                },
            );
            let kerb = self.m.kerb;
            let m = self.mesh(g, kerb);
            self.graph.get_mut(m).receive_shadow = true;
        }
        self.kerbs = spans;
    }

    // ── Barriers: concrete wall and catch fence along every surveyed wall;
    // tyre walls in front of it on the outside of corners with room to run
    // wide, and wherever the run-off is open (no wall in OpenStreetMap). ──
    fn build_barriers(&mut self) {
        let t = self.t;
        let corners = self.corners.clone();
        let outside_at = move |s: f64, side: f64| {
            corners
                .iter()
                .any(|c| c.sign == -side && s > c.s0 as f64 - 25.0 && s < c.s1 as f64 + 40.0)
        };
        for side in [-1.0, 1.0] {
            let tyre = |s: f64| {
                let f = t.frame(s);
                let w = wall(&f, side);
                w > 33.0 || (outside_at(s, side) && w - f.hw > 7.0)
            };
            // Tyre runs, cleaned of short flickers.
            let tr: Vec<[f64; 2]> = runs(tyre, 2.0, 0.0, t.length)
                .into_iter()
                .filter(|&[a, b]| b - a > 14.0)
                .collect();
            self.tyre_runs[si(side)] = tr.clone();
            let tr = Arc::new(tr);
            let in_tyre = {
                let tr = tr.clone();
                move |s: f64| tr.iter().any(|&[a, b]| s >= a && s <= b)
            };
            // `W(f, s)`.
            let ww = {
                let in_tyre = in_tyre.clone();
                move |f: &Frame, s: f64| wall(f, side) + if in_tyre(s) { TYRE_W } else { 0.0 }
            };
            // A profile point at `side * (W + off)`, `dy` above the ground at
            // `side * W`.
            let wp = |off: f64, dy: f64, u: f64| {
                let (w1, w2, ta) = (ww.clone(), ww.clone(), self.ta.clone());
                ProfilePoint::new(Lat::f(move |f, s| side * (w1(f, s) + off)))
                    .abs()
                    .dy(Lat::f(move |f, s| ta.surface_y(s, side * w2(f, s)) + dy))
                    .u(u)
            };
            // Concrete wall (behind the tyres where there are some).
            let wall_prof = vec![
                wp(0.0, -0.7, 0.0),
                wp(0.0, WALL_H, 0.5),
                wp(0.32, WALL_H, 0.6),
                wp(0.32, -0.7, 1.0),
            ];
            let fence_prof = vec![
                wp(0.12, WALL_H - 0.05, 0.0),
                wp(0.2, WALL_H + FENCE_H, FENCE_H / 1.1),
            ];
            let mut s0 = 0.0;
            while s0 < t.length {
                let r = [[s0, js::min(t.length + 1.0, s0 + 500.0)]];
                let g = extrude(
                    t,
                    &r,
                    &wall_prof,
                    &ExtrudeOpts {
                        step: 2.0,
                        v_scale: 4.0,
                        color: None,
                    },
                );
                let concrete = self.m.concrete;
                let m = self.mesh(g, concrete);
                let o = self.graph.get_mut(m);
                o.cast_shadow = true;
                o.receive_shadow = true;
                // Catch fence on top.
                let g = extrude(
                    t,
                    &r,
                    &fence_prof,
                    &ExtrudeOpts {
                        step: 4.0,
                        v_scale: 1.1,
                        color: None,
                    },
                );
                let fence = self.m.fence;
                self.mesh(g, fence);
                s0 += 500.0;
            }
            // Tyre walls.
            if !tr.is_empty() {
                let tp = |off: f64, dy: f64, u: f64| {
                    let ta = self.ta.clone();
                    ProfilePoint::new(Lat::f(move |f, _| side * (wall(f, side) + off)))
                        .abs()
                        .dy(Lat::f(move |f, s| {
                            ta.surface_y(s, side * wall(f, side)) + dy
                        }))
                        .u(u)
                };
                let tyre_prof = vec![tp(0.0, -0.3, 0.0), tp(0.0, 1.0, 0.75), tp(TYRE_W, 1.0, 1.0)];
                let g = extrude(
                    t,
                    &tr,
                    &tyre_prof,
                    &ExtrudeOpts {
                        step: 2.0,
                        v_scale: 3.0,
                        color: None,
                    },
                );
                let tyre = self.m.tyre;
                let m = self.mesh(g, tyre);
                let o = self.graph.get_mut(m);
                o.cast_shadow = true;
                o.receive_shadow = true;
            }
            // Fence posts.
            let mut posts: Vec<P3> = Vec::new();
            let mut s = 0.0;
            while s < t.length {
                let f = t.frame(s);
                let w = ww(&f, s) + 0.16;
                let p = t.point_at(s, side * w);
                posts.push([p.x, t.surface_y(s, side * w) + WALL_H - 0.1, p.z]);
                s += 4.0;
            }
            let mut post =
                cylinder_geometry(0.05, 0.05, FENCE_H + 0.15, 5.0, 1.0, false, 0.0, PI * 2.0);
            post.translate(0.0, (FENCE_H + 0.15) / 2.0, 0.0);
            let geo = self.graph.add_geometry(post);
            let im = self
                .graph
                .instanced_mesh(geo, self.m.steel, posts.len() as u32);
            {
                let inst = self
                    .graph
                    .get_mut(im)
                    .instances
                    .as_mut()
                    .expect("instanced");
                for (k, &[x, y, z]) in posts.iter().enumerate() {
                    inst.set_matrix_at(k, &Matrix4::make_translation(x, y, z));
                }
            }
            self.graph.compute_instance_bounding_sphere(im);
            self.add(im);
        }
        self.build_banners();
    }

    // Sponsor banners on the fence along the concrete walls near the
    // grandstands and the famous corners.
    fn build_banners(&mut self) {
        let t = self.t;
        let names = [
            "seaside", "midnight", "vento", "kestrel", "ion", "stiletto", "brawler", "tyres", "oil",
        ];
        let mut gb = GeoBuilder::new(false, false);
        // A plain dark patch of the atlas for the backs of the banners.
        let tr = banner_rect("tyres");
        let back = [tr[0] + 0.004, tr[1] + 0.004];
        let mut rng = Mulberry32::new(77);
        let zones = [
            [-240.0, 160.0],
            [440.0, 640.0],
            [2440.0, 2720.0],
            [3200.0, 3420.0],
        ];
        for [a, b] in zones {
            for side in [-1.0, 1.0] {
                let mut s = a + rng.next_f64() * 10.0;
                while s < b {
                    let len = 6.0;
                    let in_tyre = self.tyre_runs[si(side)]
                        .iter()
                        .any(|&[p, q]| s + len > p && s < q);
                    let tw = if in_tyre { TYRE_W } else { 0.0 };
                    let f = t.frame(s);
                    let w = wall(&f, side) + tw - 0.03;
                    let y0 = t.surface_y(s, side * w) + WALL_H + 0.15;
                    let y1 = y0 + 1.0;
                    let pick = (rng.next_f64() * names.len() as f64).floor() as usize;
                    let uv = rect_uvs(banner_rect(names[pick]));
                    let pp =
                        |ss: f64| t.point_at(ss, side * (wall(&t.frame(ss), side) + tw - 0.03));
                    let pa = pp(s);
                    let pb = pp(s + len);
                    // Face the track: on the left side the panel runs forward
                    // as u grows.
                    let aa = [pa.x, y0, pa.z];
                    let bb = [pb.x, y0, pb.z];
                    let cc = [pb.x, y1, pb.z];
                    let dd = [pa.x, y1, pa.z];
                    if side < 0.0 {
                        gb.quad(aa, bb, cc, dd, Some(uv), None, 0.0);
                    } else {
                        gb.quad(bb, aa, dd, cc, Some(uv), None, 0.0);
                    }
                    // Plain back, seen from behind the fence.
                    let bk = [back; 4];
                    if side < 0.0 {
                        gb.quad(bb, aa, dd, cc, Some(bk), None, 0.0);
                    } else {
                        gb.quad(aa, bb, cc, dd, Some(bk), None, 0.0);
                    }
                    s += len + 0.4 + if rng.next_f64() < 0.15 { 12.0 } else { 0.0 };
                }
            }
        }
        if !gb.is_empty() {
            let banner = self.m.banner;
            self.static_mesh(gb.build(), banner, false);
        }
    }

    // ── Start/finish: the gantry with its lights, the grid boxes. ──
    fn build_start(&mut self) {
        let t = self.t;
        let f = t.frame(t.start_s + 3.0);
        let mut gb = GeoBuilder::new(true, false);
        let grey = [0.55, 0.57, 0.6];
        let dark = [0.08, 0.08, 0.09];
        let l = f.wall_l + 0.9;
        let r = f.wall_r + 0.9;
        let top = f.y + 7.2;
        let yaw = kernel::atan2(f.fz, f.fx);
        let color = |c: P3| PrismOpts {
            color: Some(c),
            ..PrismOpts::default()
        };
        for lat in [-l, r] {
            let x = f.x + f.rx * lat;
            let z = f.z + f.rz * lat;
            let g = t.surface_y(t.start_s + 3.0, lat) - 0.5;
            gb.box_(x, g, z, 0.8, top - g + 1.6, 0.8, yaw, &color(grey));
        }
        // Truss beam across (a box; the light box hangs from its middle).
        let cx = f.x + f.rx * (r - l) / 2.0;
        let cz = f.z + f.rz * (r - l) / 2.0;
        gb.box_(cx, top, cz, 1.2, 1.4, l + r + 0.8, yaw, &color(grey));
        gb.box_(f.x, top - 1.3, f.z, 0.5, 1.3, 3.2, yaw, &color(dark));
        let solid = self.m.solid;
        self.static_mesh(gb.build(), solid, true);
        // START · FINISH panels on both faces of the beam.
        let uv = rect_uvs(banner_rect("startfinish"));
        let mut pb = GeoBuilder::new(false, false);
        for dir in [-1.0, 1.0] {
            let off = dir * 0.62;
            let px = cx + f.fx * off;
            let pz = cz + f.fz * off;
            let hw = 9.5;
            let a = [px - f.rx * hw, top + 0.05, pz - f.rz * hw];
            let b = [px + f.rx * hw, top + 0.05, pz + f.rz * hw];
            let c = [b[0], top + 1.35, b[2]];
            let d = [a[0], top + 1.35, a[2]];
            if dir < 0.0 {
                pb.quad(a, b, c, d, Some(uv), None, 0.0);
            } else {
                pb.quad(b, a, d, c, Some(uv), None, 0.0);
            }
        }
        let banner = self.m.banner;
        self.static_mesh(pb.build(), banner, false);

        // Start lights: five columns of two, facing the grid. They light one
        // column a second through the countdown and go out at GO.
        let lamp = self
            .graph
            .add_geometry(circle_geometry(0.17, 12.0, 0.0, PI * 2.0));
        self.lamp_mats.clear();
        for col in 0..5 {
            let mat = self.graph.add_material(
                Material::standard()
                    .set("color", 0x220806)
                    .set("emissive", 0xff2010)
                    .set("emissiveIntensity", 0.0),
            );
            self.lamp_mats.push(mat);
            for row in 0..2 {
                let m = self.graph.mesh(lamp, mat);
                let lat = (f64::from(col) - 2.0) * 0.55;
                let o = self.graph.get_mut(m);
                o.position = Vector3::new(
                    f.x + f.rx * lat - f.fx * 0.27,
                    top - 0.45 - f64::from(row) * 0.45,
                    f.z + f.rz * lat - f.fz * 0.27,
                );
                o.set_rotation(&Euler::new(0.0, -yaw - PI / 2.0, 0.0));
                self.add(m);
            }
        }

        // Grid boxes: a white bracket ahead of each starting slot.
        let mut paint = GeoBuilder::new(true, false);
        let white = [0.92, 0.92, 0.9];
        let mut strip = |s0: f64, s1: f64, l0: f64, l1: f64| {
            let p = |s: f64, l: f64| {
                let q = t.point_at(s, l);
                [q.x, t.surface_y(s, l) + 0.02, q.z]
            };
            paint.quad(
                p(s0, l0),
                p(s0, l1),
                p(s1, l1),
                p(s1, l0),
                None,
                Some(white),
                0.0,
            );
        };
        for k in 0..6 {
            let row = f64::from(k / 2);
            let col = k % 2;
            let s = t.start_s - 5.0 - row * 10.0 - f64::from(col) * 3.0 + 2.6;
            let lat = if col == 1 { 2.4 } else { -2.4 };
            strip(s, s + 0.2, lat - 1.3, lat + 1.3);
            strip(s - 1.2, s, lat - 1.3, lat - 1.1);
            strip(s - 1.2, s, lat + 1.1, lat + 1.3);
        }
        let pm = self.m.paint;
        self.static_mesh(paint.build(), pm, false);
    }

    // ── Pit lane: a paved strip along OpenStreetMap's pit lane, where it
    // runs apart from the track. ──
    fn build_pit_lane(&mut self) {
        let t = self.t;
        let pts = resample(&self.d.pit_lane, 2.0);
        let mut gb = GeoBuilder::new(true, false);
        let col = [0.3, 0.3, 0.31];
        let line = [0.85, 0.85, 0.82];
        let hw_ = 4.2;
        let near_road = |p: &Pt| {
            let da = t.distance_to_road(p.x, p.z, 96.0);
            da.i >= 0 && da.d < f64::from(t.hw[da.i as usize]) + hw_ + 0.5
        };
        for i in 0..pts.len().saturating_sub(1) {
            let a = pts[i];
            let b = pts[i + 1];
            // Skip where it merges into the track itself.
            if near_road(&a) || near_road(&b) {
                continue;
            }
            let dx = b.x - a.x;
            let dz = b.z - a.z;
            let l = js::or(kernel::hypot(dx, dz), 1.0);
            let rx = -dz / l;
            let rz = dx / l;
            let pa = pts[i.saturating_sub(1)];
            let pc = pts[(i + 2).min(pts.len() - 1)];
            let ex = pc.x - pa.x;
            let ez = pc.z - pa.z;
            let el = js::or(kernel::hypot(ex, ez), 1.0);
            let nx = -ez / el;
            let nz = ex / el;
            let s = &mut self.s;
            let mut pp = |p: Pt, nx: f64, nz: f64, off: f64, lift: f64| -> P3 {
                [
                    p.x + nx * off,
                    s.sample(p.x + nx * off, p.z + nz * off).h + lift,
                    p.z + nz * off,
                ]
            };
            let na = [rx, rz];
            let nb = [nx, nz];
            // (Wound so the face points up.)
            let q0 = pp(a, na[0], na[1], hw_, 0.1);
            let q1 = pp(b, nb[0], nb[1], hw_, 0.1);
            let q2 = pp(b, nb[0], nb[1], -hw_, 0.1);
            let q3 = pp(a, na[0], na[1], -hw_, 0.1);
            gb.quad(q0, q1, q2, q3, None, Some(col), 0.0);
            // Painted edge lines.
            for e in [-hw_ + 0.35, hw_ - 0.35] {
                let q0 = pp(a, na[0], na[1], e + 0.08, 0.13);
                let q1 = pp(b, nb[0], nb[1], e + 0.08, 0.13);
                let q2 = pp(b, nb[0], nb[1], e - 0.08, 0.13);
                let q3 = pp(a, na[0], na[1], e - 0.08, 0.13);
                gb.quad(q0, q1, q2, q3, None, Some(line), 0.0);
            }
        }
        if !gb.is_empty() {
            let pm = self.m.paint;
            let m = self.static_mesh(gb.build(), pm, false);
            self.graph.get_mut(m).receive_shadow = true;
        }
    }

    // ── Buildings from OpenStreetMap: plain boxes on their footprints,
    // walls pale, a darker band of windows or doors, flat grey roofs. ──
    fn build_buildings(&mut self) {
        let t = self.t;
        let mut gb = GeoBuilder::new(true, false);
        let walls = [
            [0.86, 0.84, 0.8],
            [0.93, 0.92, 0.88],
            [0.8, 0.76, 0.68],
            [0.74, 0.76, 0.78],
            [0.88, 0.82, 0.7],
        ];
        let roof = [0.42, 0.42, 0.43];
        let band = [0.18, 0.2, 0.23];
        let mut rng = Mulberry32::new(12);
        for b in &self.d.buildings {
            let pts = closed_open(&b.pts);
            if pts.len() < 3 {
                continue;
            }
            let area = poly_area(&pts).abs();
            if area < 12.0 {
                continue;
            }
            let (cx, cz) = centroid(&pts);
            // Nothing inside the barriers.
            let near = t.distance_to_road(cx, cz, 60.0);
            if near.i >= 0 {
                let i = near.i as usize;
                if near.d < js::max(f64::from(t.wall_l[i]), f64::from(t.wall_r[i])) - 2.0 {
                    continue;
                }
            }
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for p in &pts {
                let h = self.s.sample(p.x, p.z).h;
                lo = js::min(lo, h);
                hi = js::max(hi, h);
            }
            let hh = if area > 2500.0 {
                8.0
            } else if area > 700.0 {
                6.0
            } else if area > 150.0 {
                4.6
            } else {
                3.2
            };
            let y0 = lo - 0.6;
            let y1 = hi + hh;
            let corners: Vec<P2> = pts.iter().map(|p| [p.x, p.z]).collect();
            let wc = walls[(rng.next_f64() * walls.len() as f64).floor() as usize];
            gb.prism(
                &corners,
                y0,
                y1,
                &PrismOpts {
                    color: Some(wc),
                    roof_color: Some(roof),
                    ..PrismOpts::default()
                },
            );
            // Window / door band.
            if hh >= 4.0 {
                gb.prism(
                    &scale_poly(&corners, 1.02),
                    hi + hh * 0.28,
                    hi + hh * 0.62,
                    &PrismOpts {
                        color: Some(band),
                        roof: Some(false),
                        ..PrismOpts::default()
                    },
                );
            }
        }
        if !gb.is_empty() {
            let solid = self.m.solid;
            let m = self.static_mesh(gb.build(), solid, true);
            self.graph.get_mut(m).cast_shadow = true;
        }
    }

    // ── Grandstands: tiers of seats full of people rising away from the
    // track on each OpenStreetMap footprint, the big ones roofed. ──
    fn build_grandstands(&mut self) {
        let t = self.t;
        let mut gb = GeoBuilder::new(true, false);
        let mut crowd = GeoBuilder::new(false, false);
        let conc = [0.78, 0.77, 0.74];
        let steel_c = [0.6, 0.62, 0.66];
        let roof_c = [0.88, 0.9, 0.92];
        for poly in &self.d.grandstands {
            let pts = closed_open(poly);
            if pts.len() < 3 {
                continue;
            }
            let bx = min_rect(&pts);
            // Front: the long side nearer the track.
            let MinRect {
                c, u, v, hu, hv, ..
            } = bx; // u: unit along the long side, v: across
            let p1 = Pt {
                x: c.x + v.x * hv,
                z: c.z + v.z * hv,
            };
            let p2 = Pt {
                x: c.x - v.x * hv,
                z: c.z - v.z * hv,
            };
            let d1 = t.distance_to_road(p1.x, p1.z, 200.0).d;
            let d2 = t.distance_to_road(p2.x, p2.z, 200.0).d;
            let back = if d1 < d2 { Pt { x: -v.x, z: -v.z } } else { v }; // from front to back
            let front = Pt {
                x: c.x - back.x * hv,
                z: c.z - back.z * hv,
            };
            let mut lo = f64::INFINITY;
            for p in &pts {
                lo = js::min(lo, self.s.sample(p.x, p.z).h);
            }
            let depth = hv * 2.0;
            let rows = js::max(3.0, (depth / 0.85).floor());
            let rise = 0.42;
            let y0 = lo + 0.8;
            let y1 = y0 + rows * rise;
            let pp = |a: f64, b: f64, y: f64| -> P3 {
                [
                    front.x + u.x * a + back.x * b,
                    y,
                    front.z + u.z * a + back.z * b,
                ]
            };
            // Seating slope (crowd texture: 8 tiers per texture height).
            let vr = rows / 8.0;
            let ur = (hu * 2.0) / 12.0;
            let cuv = [[0.0, 0.0], [ur, 0.0], [ur, vr], [0.0, vr]];
            crowd.quad(
                pp(-hu, 0.0, y0),
                pp(hu, 0.0, y0),
                pp(hu, depth, y1),
                pp(-hu, depth, y1),
                Some(cuv),
                None,
                0.0,
            );
            crowd.quad(
                pp(hu, 0.0, y0),
                pp(-hu, 0.0, y0),
                pp(-hu, depth, y1),
                pp(hu, depth, y1),
                Some(cuv),
                None,
                0.0,
            );
            // Front wall, sides and back.
            let g0 = lo - 0.8;
            gb.quad(
                pp(-hu, 0.0, g0),
                pp(hu, 0.0, g0),
                pp(hu, 0.0, y0),
                pp(-hu, 0.0, y0),
                None,
                Some(conc),
                0.0,
            );
            for sgn in [-1.0, 1.0] {
                gb.quad(
                    pp(sgn * hu, 0.0, g0),
                    pp(sgn * hu, depth, g0),
                    pp(sgn * hu, depth, y1 + 0.9),
                    pp(sgn * hu, 0.0, y0 + 0.9),
                    None,
                    Some(conc),
                    0.0,
                );
            }
            gb.quad(
                pp(hu, depth, g0),
                pp(-hu, depth, g0),
                pp(-hu, depth, y1 + 0.9),
                pp(hu, depth, y1 + 0.9),
                None,
                Some(conc),
                0.0,
            );
            // Roof on the bigger stands: posts at the back, cantilevered
            // forward.
            if depth > 9.0 && hu > 12.0 {
                let ry = y1 + 4.2;
                gb.quad(
                    pp(-hu - 1.0, -2.0, ry - 0.8),
                    pp(hu + 1.0, -2.0, ry - 0.8),
                    pp(hu + 1.0, depth + 0.5, ry),
                    pp(-hu - 1.0, depth + 0.5, ry),
                    None,
                    Some(roof_c),
                    0.0,
                );
                gb.quad(
                    pp(hu + 1.0, -2.0, ry - 0.8),
                    pp(-hu - 1.0, -2.0, ry - 0.8),
                    pp(-hu - 1.0, depth + 0.5, ry),
                    pp(hu + 1.0, depth + 0.5, ry),
                    None,
                    Some(roof_c),
                    0.0,
                );
                let mut a = -hu;
                while a <= hu + 0.1 {
                    let q = pp(a, depth - 0.3, 0.0);
                    gb.box_(
                        q[0],
                        y1 - 1.0,
                        q[2],
                        0.4,
                        ry - y1 + 1.0,
                        0.4,
                        0.0,
                        &PrismOpts {
                            color: Some(steel_c),
                            ..PrismOpts::default()
                        },
                    );
                    a += 12.0;
                }
            }
        }
        if !gb.is_empty() {
            let solid = self.m.solid;
            self.static_mesh(gb.build(), solid, true);
        }
        if !crowd.is_empty() {
            let cm = self.m.crowd;
            self.static_mesh(crowd.build(), cm, false);
        }
    }

    // ── Bridges over the track (OpenStreetMap bridge ways that cross it):
    // steel footbridges and the perimeter road's concrete bridge. ──
    fn build_bridges(&mut self) {
        let t = self.t;
        let mut gb = GeoBuilder::new(true, false);
        let blue = [0.2, 0.36, 0.62];
        let white = [0.9, 0.9, 0.88];
        let conc = [0.72, 0.7, 0.66];
        let mut ban = GeoBuilder::new(false, false);
        self.bridges.clear();
        let col = |c: P3| PrismOpts {
            color: Some(c),
            ..PrismOpts::default()
        };
        for br in &self.d.bridges {
            let Some((s, dir)) = cross_track(t, &br.pts) else {
                continue;
            };
            let f = t.frame(s);
            // Span the corridor: out past both barriers.
            let reach = (js::max(f.wall_l, f.wall_r) + 5.0)
                / js::max(0.35, (dir.x * f.rx + dir.z * f.rz).abs());
            let road = br.name != "footway" && br.name != "path";
            let w = if road { 8.5 } else { 3.2 };
            let deck_y = f.y + CLEAR + 0.3;
            let nx = -dir.z;
            let nz = dir.x;
            let pp = |a: f64, b: f64, y: f64| -> P3 {
                [f.x + dir.x * a + nx * b, y, f.z + dir.z * a + nz * b]
            };
            let yaw = kernel::atan2(dir.z, dir.x);
            // Deck (a box).
            gb.box_(
                f.x,
                deck_y - 0.7,
                f.z,
                reach * 2.0,
                0.7,
                w,
                yaw,
                &PrismOpts {
                    color: Some(if road { conc } else { white }),
                    bottom: true,
                    ..PrismOpts::default()
                },
            );
            // Sides: parapets on the road bridge, blue truss girders on
            // footbridges.
            for sd in [-1.0, 1.0] {
                let cx = f.x + nx * sd * (w / 2.0 - 0.15);
                let cz = f.z + nz * sd * (w / 2.0 - 0.15);
                if road {
                    gb.box_(cx, deck_y, cz, reach * 2.0, 1.0, 0.3, yaw, &col(conc));
                } else {
                    gb.box_(cx, deck_y + 1.9, cz, reach * 2.0, 0.3, 0.3, yaw, &col(blue));
                    let mut a = -reach;
                    while a <= reach {
                        let q = pp(a, sd * (w / 2.0 - 0.15), 0.0);
                        gb.box_(q[0], deck_y, q[2], 0.18, 1.9, 0.18, yaw, &col(blue));
                        a += 2.5;
                    }
                }
            }
            if !road {
                gb.box_(
                    f.x,
                    deck_y + 2.2,
                    f.z,
                    reach * 2.0,
                    0.12,
                    w,
                    yaw,
                    &col(blue),
                ); // canopy
            }
            // Piers and abutments at each end, down to the ground.
            for a in [-reach, reach] {
                let q = pp(a, 0.0, 0.0);
                let g = self.s.sample(q[0], q[2]).h;
                gb.box_(
                    q[0],
                    g - 0.5,
                    q[2],
                    1.6,
                    deck_y - g - 0.2,
                    w * 0.9,
                    yaw,
                    &col(conc),
                );
            }
            // Banners along the girder, facing both ways.
            if !road {
                for face in [-1.0, 1.0] {
                    let uv = rect_uvs(banner_rect(if face > 0.0 { "seaside" } else { "midnight" }));
                    let b0 = pp(-6.0, face * (w / 2.0 + 0.02), deck_y + 0.3);
                    let b1 = pp(6.0, face * (w / 2.0 + 0.02), deck_y + 0.3);
                    let c1 = [b1[0], deck_y + 1.75, b1[2]];
                    let c0 = [b0[0], deck_y + 1.75, b0[2]];
                    if face > 0.0 {
                        ban.quad(b1, b0, c0, c1, Some(uv), None, 0.0);
                    } else {
                        ban.quad(b0, b1, c1, c0, Some(uv), None, 0.0);
                    }
                }
            }
            self.bridges.push(Bridge { s, road, deck_y });
        }
        if !gb.is_empty() {
            let solid = self.m.solid;
            self.static_mesh(gb.build(), solid, true);
        }
        if !ban.is_empty() {
            let banner = self.m.banner;
            self.static_mesh(ban.build(), banner, false);
        }
    }

    // ── The infield lake and the ponds nearby. ──
    fn build_lake(&mut self) {
        let mat = self.graph.add_material(
            Material::standard()
                .set("color", 0x3c6462)
                .set("roughness", 0.12)
                .set("metalness", 0.2),
        );
        let d = self.d;
        for poly in &d.water {
            let pts = closed_open(poly);
            if pts.len() < 3 || poly_area(&pts).abs() < 150.0 {
                continue;
            }
            // The lidar's water surface: the lowest ground inside the shore.
            let (cx, cz) = centroid(&pts);
            let mut lo = f64::INFINITY;
            for p in &pts {
                for k in [0.3, 0.6, 0.85] {
                    lo = js::min(lo, d.height(lerp(p.x, cx, k), lerp(p.z, cz, k)));
                }
            }
            let mut contour: Vec<Vector2> = pts.iter().map(|p| Vector2::new(p.x, p.z)).collect();
            let mut holes: Vec<Vec<Vector2>> = Vec::new();
            let tris = triangulate_shape(&mut contour, &mut holes);
            let mut pos = Vec::with_capacity(pts.len() * 3);
            for p in &pts {
                pos.extend_from_slice(&[p.x, lo + 0.25, p.z]);
            }
            let mut g = BufferGeometry::new();
            g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
            let mut idx: Vec<u32> = Vec::new();
            for [a, b, c] in tris {
                // Face up.
                let (pa, pb, pc) = (pts[a], pts[b], pts[c]);
                let cr = (pb.x - pa.x) * (pc.z - pa.z) - (pb.z - pa.z) * (pc.x - pa.x);
                if cr > 0.0 {
                    idx.extend_from_slice(&[a as u32, b as u32, c as u32]);
                } else {
                    idx.extend_from_slice(&[a as u32, c as u32, b as u32]);
                }
            }
            g.set_index(&idx);
            g.compute_vertex_normals();
            self.static_mesh(g, mat, false);
        }
    }

    // ── Braking boards before the slow corners, and marshal posts. ──
    fn build_boards(&mut self) {
        let t = self.t;
        let mut gb = GeoBuilder::new(false, false);
        let mut posts = GeoBuilder::new(true, false);
        let sp = |i: usize| f64::from(t.speed_profile[i]);
        for c in self.corners.clone() {
            // Only corners you really brake for.
            let drop = sp(t.idx(c.s0 as f64 - 200.0)) - sp(c.apex);
            if drop < 14.0 {
                continue;
            }
            let side = -c.sign; // the outside of the corner
            // Braking starts where the speed profile starts falling.
            let apex = c.apex as f64;
            let mut sb = apex;
            while sb > apex - 400.0 && sp(t.idx(sb - 1.0)) > sp(t.idx(sb)) + 0.001 {
                sb -= 1.0;
            }
            for (dd, name) in [(150.0, "b150"), (100.0, "b100"), (50.0, "b50")] {
                let s = sb - dd + 30.0;
                let f = t.frame(s);
                let in_tyre = self.tyre_runs[si(side)]
                    .iter()
                    .any(|&[a, b]| s > a && s < b);
                let w = wall(&f, side) + if in_tyre { TYRE_W } else { 0.0 };
                let lat = side * (w + 0.5);
                let p = t.point_at(s, lat);
                let g = t.surface_y(s, side * w);
                let [u0, v0, u1, v1] = banner_rect(name);
                let y0 = g + WALL_H + 0.3;
                let y1 = y0 + 0.9;
                let hx = f.rx * 1.2; // half width across the road
                let hz = f.rz * 1.2;
                let a = [p.x - hx, y0, p.z - hz];
                let b = [p.x + hx, y0, p.z + hz];
                // Facing oncoming cars (looking along +f): quad wound toward -f.
                gb.quad(
                    b,
                    a,
                    [a[0], y1, a[2]],
                    [b[0], y1, b[2]],
                    Some([[u1, v0], [u0, v0], [u0, v1], [u1, v1]]),
                    None,
                    0.0,
                );
                gb.quad(
                    a,
                    b,
                    [b[0], y1, b[2]],
                    [a[0], y1, a[2]],
                    Some([[u0, v0], [u1, v0], [u1, v1], [u0, v1]]),
                    None,
                    0.0,
                );
                posts.box_(
                    p.x,
                    g,
                    p.z,
                    0.12,
                    WALL_H + 0.35,
                    0.12,
                    0.0,
                    &PrismOpts {
                        color: Some([0.3, 0.3, 0.32]),
                        ..PrismOpts::default()
                    },
                );
            }
            // Marshal post: a white hut behind the wall on the outside, with
            // a yellow flag on a pole.
            let s = apex + 10.0;
            let f = t.frame(s);
            let w = wall(&f, side) + 4.0;
            let p = t.point_at(s, side * w);
            let g = self.s.sample(p.x, p.z).h;
            let yaw = kernel::atan2(f.fz, f.fx);
            posts.box_(
                p.x,
                g - 0.3,
                p.z,
                2.2,
                2.6,
                2.0,
                yaw,
                &PrismOpts {
                    color: Some([0.92, 0.92, 0.9]),
                    roof_color: Some([0.7, 0.2, 0.18]),
                    ..PrismOpts::default()
                },
            );
            posts.box_(
                p.x + f.fx * 1.5,
                g,
                p.z + f.fz * 1.5,
                0.06,
                4.2,
                0.06,
                0.0,
                &PrismOpts {
                    color: Some([0.7, 0.7, 0.72]),
                    ..PrismOpts::default()
                },
            );
            posts.box_(
                p.x + f.fx * 1.5 + f.rx * side * 0.55,
                g + 3.4,
                p.z + f.fz * 1.5 + f.rz * side * 0.55,
                1.0,
                0.7,
                0.02,
                yaw + PI / 2.0,
                &PrismOpts {
                    color: Some([0.95, 0.8, 0.1]),
                    ..PrismOpts::default()
                },
            );
        }
        if !gb.is_empty() {
            let banner = self.m.banner;
            self.static_mesh(gb.build(), banner, false);
        }
        if !posts.is_empty() {
            let solid = self.m.solid;
            self.static_mesh(posts.build(), solid, true);
        }
    }

    // ── Coast live oaks wherever the aerial photo shows a crown: 8 m cells
    // near the circuit, 32 m cells out to the hills. ──
    fn build_trees(&mut self) {
        let t = self.t;
        let d = self.d;
        let mut rng = Mulberry32::new(2024);
        let mut near: Vec<Tree> = Vec::new();
        let mut far: Vec<Tree> = Vec::new();
        let in_corridor = |x: f64, z: f64| {
            let r = t.distance_to_road(x, z, 60.0);
            if r.i < 0 {
                return false;
            }
            let i = r.i as usize;
            let w = f64::from(if r.lat < 0.0 {
                t.wall_l[i]
            } else {
                t.wall_r[i]
            });
            r.d < w + 3.0
        };
        // Detailed crowns only where you pass close by.
        let s = &mut self.s;
        let mut place = |x: f64, z: f64, r: f64, is_near: bool| -> Tree {
            let g = s.sample(x, z).h;
            Tree {
                x,
                z,
                y: g,
                r,
                hi: is_near && t.distance_to_road(x, z, 140.0).d < 140.0,
            }
        };
        // Near: the fine canopy grid, 2 × 2 cells (8 m) at a time.
        let tf = &d.trees_fine;
        let cover = |k: usize| f64::from(tf.values[k]);
        let mut j = 0;
        while j + 1 < tf.h {
            let mut i = 0;
            while i + 1 < tf.w {
                let c = (cover(j * tf.w + i)
                    + cover(j * tf.w + i + 1)
                    + cover((j + 1) * tf.w + i)
                    + cover((j + 1) * tf.w + i + 1))
                    / 4.0;
                'cell: {
                    if c < 0.1 || rng.next_f64() > c * 0.75 {
                        break 'cell;
                    }
                    let x = tf.x0 + (i as f64 + 0.5 + (rng.next_f64() - 0.5) * 1.6) * tf.step;
                    let z = tf.z0 + (j as f64 + 0.5 + (rng.next_f64() - 0.5) * 1.6) * tf.step;
                    if in_corridor(x, z) {
                        break 'cell;
                    }
                    let r = lerp(3.2, 5.6, rng.next_f64()) * (0.85 + c * 0.35);
                    near.push(place(x, z, r, true));
                }
                i += 2;
            }
            j += 2;
        }
        // Far: the wide grid, outside the fine one, within 2.6 km.
        let tw = &d.trees_wide;
        let fx0 = tf.x0;
        let fx1 = tf.x0 + (tf.w as f64 - 1.0) * tf.step;
        let fz0 = tf.z0;
        let fz1 = tf.z0 + (tf.h as f64 - 1.0) * tf.step;
        let cx = (t.bounds.min_x + t.bounds.max_x) / 2.0;
        let cz = (t.bounds.min_z + t.bounds.max_z) / 2.0;
        for j in 0..tw.h {
            for i in 0..tw.w {
                let c = f64::from(tw.values[j * tw.w + i]);
                if c < 0.06 {
                    continue;
                }
                let x0 = tw.x0 + i as f64 * tw.step;
                let z0 = tw.z0 + j as f64 * tw.step;
                if kernel::hypot(x0 - cx, z0 - cz) > 2600.0 {
                    continue;
                }
                if x0 > fx0 && x0 < fx1 && z0 > fz0 && z0 < fz1 {
                    continue;
                }
                // Fewer, bigger crowns in the distance: clumps read as
                // woodland.
                let n_tree = (c * 1.3 + rng.next_f64() * 0.8).floor();
                let mut k = 0.0;
                while k < n_tree {
                    let x = x0 + (rng.next_f64() - 0.5) * tw.step;
                    let z = z0 + (rng.next_f64() - 0.5) * tw.step;
                    let r = lerp(5.0, 8.5, rng.next_f64());
                    far.push(place(x, z, r, false));
                    k += 1.0;
                }
            }
        }
        self.tree_count = near.len() + far.len();
        let mat = self.graph.add_material(
            Material::standard()
                .set("vertexColors", true)
                .set("roughness", 0.95),
        );
        let trunk_mat = self.graph.add_material(
            Material::standard()
                .set("color", 0x4a3b2c)
                .set("roughness", 0.95),
        );
        let crown_hi = self.graph.add_geometry(canopy_geometry("shade", 7, 0));
        let crown_lo = self.graph.add_geometry(blob_geometry(7));
        let mut trunk = cylinder_geometry(0.22, 0.34, 1.0, 5.0, 1.0, false, 0.0, PI * 2.0);
        trunk.translate(0.0, 0.5, 0.0);
        let trunk = self.graph.add_geometry(trunk);
        // Chunked so each piece can be frustum-culled.
        const CH: f64 = 420.0;
        let mut groups: Vec<(String, Vec<Tree>)> = Vec::new();
        let mut push = |key: String, item: Tree| match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, l)) => l.push(item),
            None => groups.push((key, vec![item])),
        };
        let cell = |v: f64, size: f64| (v / size).floor() as i64;
        for tr in &near {
            let key = format!(
                "{}{},{}",
                if tr.hi { "n:" } else { "m:" },
                cell(tr.x, CH),
                cell(tr.z, CH)
            );
            push(key, *tr);
        }
        for tr in &far {
            push(
                format!("f:{},{}", cell(tr.x, CH * 2.0), cell(tr.z, CH * 2.0)),
                *tr,
            );
        }
        for (key, list) in &groups {
            let hi = key.starts_with('n');
            let n = list.len() as u32;
            let crowns = self
                .graph
                .instanced_mesh(if hi { crown_hi } else { crown_lo }, mat, n);
            let trunks = hi.then(|| self.graph.instanced_mesh(trunk, trunk_mat, n));
            for (k, tr) in list.iter().enumerate() {
                let r = tr.r;
                let th = r * 0.42 + 0.6; // trunk height to the crown's underside
                let q = Quaternion::from_euler(&Euler::new(0.0, rng.next_f64() * PI * 2.0, 0.0));
                // Seen from further off, a low dome with no trunk showing.
                let m4 = if hi {
                    Matrix4::compose(
                        Vector3::new(tr.x, tr.y + th + r * 0.45, tr.z),
                        q,
                        Vector3::new(r * 0.95, r * 0.62, r * 0.95),
                    )
                } else {
                    Matrix4::compose(
                        Vector3::new(tr.x, tr.y + r * 0.42, tr.z),
                        q,
                        Vector3::new(r, r * 0.66, r),
                    )
                };
                // Coast live oak: dark olive, some sunnier.
                let cr = lerp(0.075, 0.12, rng.next_f64());
                let cg = lerp(0.1, 0.15, rng.next_f64());
                let cb = lerp(0.035, 0.055, rng.next_f64());
                {
                    let inst = self
                        .graph
                        .get_mut(crowns)
                        .instances
                        .as_mut()
                        .expect("instanced");
                    inst.set_matrix_at(k, &m4);
                    inst.set_color_at(k, Color::new(cr, cg, cb));
                }
                if let Some(tk) = trunks {
                    let m4 = Matrix4::compose(
                        Vector3::new(tr.x, tr.y - 0.2, tr.z),
                        q,
                        Vector3::new(r * 0.28, th + r * 0.2, r * 0.28),
                    );
                    let inst = self
                        .graph
                        .get_mut(tk)
                        .instances
                        .as_mut()
                        .expect("instanced");
                    inst.set_matrix_at(k, &m4);
                }
            }
            self.graph.compute_instance_bounding_sphere(crowns);
            {
                let o = self.graph.get_mut(crowns);
                o.cast_shadow = hi;
                o.receive_shadow = true;
            }
            self.add(crowns);
            if let Some(tk) = trunks {
                self.graph.compute_instance_bounding_sphere(tk);
                self.add(tk);
            }
        }
    }
}

// ── Geometry helpers ─────────────────────────────────────────────────

/// `toFixed(3)` of a coordinate, for a map key: -0 is written "0.000" (a
/// negative number that rounds to zero keeps its sign in both).
fn key3(x: f64) -> String {
    format!("{:.3}", x + 0.0)
}

/// A distant crown: one lumpy icosahedron (20 triangles), lit like the
/// detailed ones (normals from the centre, darker underneath).
pub fn blob_geometry(seed: u32) -> BufferGeometry {
    let mut rng = Mulberry32::new(seed);
    let mut g = icosahedron_geometry(1.0, 0.0).to_non_indexed();
    let mut by_key: Vec<(String, f64)> = Vec::new();
    {
        let p = g.get_attribute_mut("position").expect("position");
        for i in 0..p.count() {
            let key = format!(
                "{},{},{}",
                key3(p.get_x(i)),
                key3(p.get_y(i)),
                key3(p.get_z(i))
            );
            let k = match by_key.iter().find(|(kk, _)| *kk == key) {
                Some(&(_, v)) => v,
                None => {
                    let v = 0.85 + rng.next_f64() * 0.3;
                    by_key.push((key, v));
                    v
                }
            };
            let (x, y, z) = (p.get_x(i), p.get_y(i), p.get_z(i));
            p.set_xyz(i, x * k, y * k, z * k);
        }
    }
    let p = g.position();
    let n = p.count();
    let mut nrm = vec![0f32; n * 3];
    let mut col = vec![0f32; n * 3];
    for i in 0..n {
        let (x, y, z) = (p.get_x(i), p.get_y(i), p.get_z(i));
        let l = js::or(kernel::hypot3(x, y + 0.15, z), 1.0);
        nrm[i * 3] = (x / l) as f32;
        nrm[i * 3 + 1] = ((y + 0.15) / l) as f32;
        nrm[i * 3 + 2] = (z / l) as f32;
        let ao = lerp(0.62, 1.05, smoothstep(-0.9, 0.7, y));
        col[i * 3] = (ao * 1.05) as f32;
        col[i * 3 + 1] = ao as f32;
        col[i * 3 + 2] = (ao * 0.85) as f32;
    }
    g.set_attribute("normal", BufferAttribute::from_f32(nrm, 3));
    g.set_attribute("color", BufferAttribute::from_f32(col, 3));
    g.delete_attribute("uv");
    g
}

/// `poly.slice()` without a repeated closing point.
fn closed_open(pts: &[Pt]) -> Vec<Pt> {
    let mut pts = pts.to_vec();
    if pts.len() > 2 {
        let (a, b) = (pts[0], pts[pts.len() - 1]);
        if a.x == b.x && a.z == b.z {
            pts.pop();
        }
    }
    pts
}

/// The mean of the points, summed in order from 0 (`reduce`).
fn centroid(pts: &[Pt]) -> (f64, f64) {
    let n = pts.len() as f64;
    let sx = pts.iter().fold(0.0, |a, p| a + p.x);
    let sz = pts.iter().fold(0.0, |a, p| a + p.z);
    (sx / n, sz / n)
}

pub fn poly_area(pts: &[Pt]) -> f64 {
    let mut a = 0.0;
    for i in 0..pts.len() {
        let p = pts[i];
        let q = pts[(i + 1) % pts.len()];
        a += p.x * q.z - q.x * p.z;
    }
    a / 2.0
}

fn scale_poly(corners: &[P2], k: f64) -> Vec<P2> {
    let n = corners.len() as f64;
    let cx = corners.iter().fold(0.0, |a, p| a + p[0]) / n;
    let cz = corners.iter().fold(0.0, |a, p| a + p[1]) / n;
    corners
        .iter()
        .map(|&[x, z]| [cx + (x - cx) * k, cz + (z - cz) * k])
        .collect()
}

/// Points every `step` metres along a polyline.
pub fn resample(pts: &[Pt], step: f64) -> Vec<Pt> {
    let mut out = vec![pts[0]];
    let mut carry = 0.0;
    for i in 0..pts.len() - 1 {
        let a = pts[i];
        let b = pts[i + 1];
        let l = kernel::hypot(b.x - a.x, b.z - a.z);
        let mut d = step - carry;
        while d <= l {
            out.push(Pt {
                x: a.x + (b.x - a.x) * d / l,
                z: a.z + (b.z - a.z) * d / l,
            });
            d += step;
        }
        carry = l - (d - step);
    }
    out.push(pts[pts.len() - 1]);
    out
}

/// `minRect`'s result.
#[derive(Clone, Copy, Debug)]
pub struct MinRect {
    pub area: f64,
    pub u: Pt,
    pub v: Pt,
    pub hu: f64,
    pub hv: f64,
    pub c: Pt,
}

/// Minimum-area bounding rectangle (edges of a small polygon).
pub fn min_rect(pts: &[Pt]) -> MinRect {
    let mut best: Option<MinRect> = None;
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        let l = kernel::hypot(b.x - a.x, b.z - a.z);
        if l < 0.5 {
            continue;
        }
        let u = Pt {
            x: (b.x - a.x) / l,
            z: (b.z - a.z) / l,
        };
        let v = Pt { x: -u.z, z: u.x };
        let (mut u0, mut u1, mut v0, mut v1) = (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        );
        for p in pts {
            let pu = p.x * u.x + p.z * u.z;
            let pv = p.x * v.x + p.z * v.z;
            u0 = js::min(u0, pu);
            u1 = js::max(u1, pu);
            v0 = js::min(v0, pv);
            v1 = js::max(v1, pv);
        }
        let area = (u1 - u0) * (v1 - v0);
        if best.is_none_or(|b| area < b.area) {
            let mu = (u0 + u1) / 2.0;
            let mv = (v0 + v1) / 2.0;
            best = Some(MinRect {
                area,
                u,
                v,
                hu: (u1 - u0) / 2.0,
                hv: (v1 - v0) / 2.0,
                c: Pt {
                    x: u.x * mu + v.x * mv,
                    z: u.z * mu + v.z * mv,
                },
            });
        }
    }
    let best = best.expect("a footprint with an edge");
    // Long side along u.
    if best.hv > best.hu {
        MinRect {
            u: best.v,
            v: Pt {
                x: -best.u.x,
                z: -best.u.z,
            },
            hu: best.hv,
            hv: best.hu,
            ..best
        }
    } else {
        best
    }
}

/// Where a polyline crosses the track centreline: `(s, dir)` (dir a unit
/// vector along the polyline), or `None`.
pub fn cross_track(t: &Track, pts: &[Pt]) -> Option<(f64, Pt)> {
    for i in 0..pts.len().saturating_sub(1) {
        let a = pts[i];
        let b = pts[i + 1];
        let mx = (a.x + b.x) / 2.0;
        let mz = (a.z + b.z) / 2.0;
        let half = kernel::hypot(b.x - a.x, b.z - a.z) / 2.0 + 4.0;
        let k0 = t.nearest(mx, mz, half + 30.0);
        if k0 < 0 {
            continue;
        }
        for o in -60i64..=60 {
            let k = t.idx((k0 + o) as f64);
            let k1 = t.idx((k0 + o + 1) as f64);
            let p = Pt {
                x: f64::from(t.px[k]),
                z: f64::from(t.pz[k]),
            };
            let q = Pt {
                x: f64::from(t.px[k1]),
                z: f64::from(t.pz[k1]),
            };
            let (d1x, d1z) = (b.x - a.x, b.z - a.z);
            let (d2x, d2z) = (q.x - p.x, q.z - p.z);
            let den = d1x * d2z - d1z * d2x;
            if den.abs() < 1e-9 {
                continue;
            }
            let ta = ((p.x - a.x) * d2z - (p.z - a.z) * d2x) / den;
            let tb = ((p.x - a.x) * d1z - (p.z - a.z) * d1x) / den;
            if (0.0..=1.0).contains(&ta) && (0.0..=1.0).contains(&tb) {
                let l = kernel::hypot(d1x, d1z);
                return Some((
                    (k0 + o) as f64 + tb,
                    Pt {
                        x: d1x / l,
                        z: d1z / l,
                    },
                ));
            }
        }
    }
    None
}
