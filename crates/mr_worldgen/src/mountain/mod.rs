//! `src/world/Mountain.js` (roadmap WP 3.6): the Sierra Pass scenery,
//! zone 0 of Sierra. Boulders and outcrops on the rock walls, pine forest
//! on the gentler ground, the start gantry with a roadside diner, warning
//! signs for the switchbacks, a summit lookout and a waterfall in the
//! canyon.
//!
//! Everything is instanced or merged per material and chunked along the
//! road so frustum culling does useful work. Placement is deterministic:
//! each part draws from its own `mulberry32` stream in the JS order.
//!
//! The port keeps the JS structure: [`Mountain::plan`] cuts the diner
//! pull-out and the summit lookout into the hillside before the terrain's
//! fields are built; [`Mountain::build`] runs `buildRocks`, `buildForest`,
//! `buildVerge`, `buildSnow`, `buildSigns`, `buildSnowPoles`,
//! `buildDelineators`, `buildStartArea`, `buildDiner`, `buildLookout` and
//! `buildWaterfall` in that order into the group `mountain`. The flag and
//! the waterfall's updaters are [`Animator`](crate::world::Animator)s.
//!
//! The parked pickup at the diner and the sedan at the lookout come from
//! `vehicles/CarModel.js` (roadmap WP 4.1); the JS imports it optionally
//! and so does the port: [`Mountain::parked_cars`] is a hook that builds a
//! car's object tree, which `bakeStatic` then merges (DECISIONS D311).
//!
//! The material patches (`TriplanarRock`, `Reflector`) are tagged kinds;
//! their GLSL is the renderer's.

// Index loops stay index loops, and the JS signatures stay (D52, D130).
#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

pub mod canvas;
pub mod kit;

use mr_math::{Mulberry32, clamp, fbm, js, kernel, lerp, smoothstep};
use mr_scene::{NodeType, three};
use mr_track::{Frame, Track};

use crate::flora::{
    RockOpts, conifer_geometry, flower_geometry, foliage_material, grass_clump_geometry,
    rock_geometry, shrub_geometry,
};
use crate::material::{Material, Param};
use crate::object::{GeoId, Layer, MaterialId, NodeId, SceneGraph, TextureId};
use crate::terrain::Terrain;
use crate::textures::TextureCache;
use crate::three_geom::{
    BufferAttribute, BufferGeometry, Euler, ExtrudeOptions, Quaternion, Shape, Vector3,
    box_geometry, circle_geometry, cylinder_geometry, extrude_geometry, merge_geometries,
    plane_geometry,
};
use crate::world::{Animator, Change, Edit, Handle, Scenery, SceneryInfo, UpdateCtx, World};

use canvas::{Rect, SignAtlas, diamond, panel};
use kit::{Item, SurfaceSampler, bake_static, instanced, merged_mesh, placed, rock_material};

const PI: f64 = core::f64::consts::PI;

/// Metres of road per instancing chunk.
const CHUNK: f64 = 480.0;
/// Detailed pines out to this distance from road.
const NEAR_FOREST: f64 = 420.0;

/// The JS writes a whole turn as `6.28` for the random yaw of trees and
/// tufts (not 2π), and the draws must land where its do.
#[allow(clippy::approx_constant)]
const TURN: f64 = 6.28;

const UP: Vector3 = Vector3::new(0.0, 1.0, 0.0);
const DOUBLE_SIDE: f64 = three::DOUBLE_SIDE as f64;
const FRONT_SIDE: f64 = three::FRONT_SIDE as f64;

/// A place cut into the hillside by `plan()`: the diner pull-out or the
/// summit lookout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    pub s: f64,
    pub side: f64,
    pub lat: f64,
    pub x: f64,
    pub z: f64,
    pub y: f64,
}

/// `{ x, z, r }`: no scenery within r of (x, z).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Exclusion {
    pub x: f64,
    pub z: f64,
    pub r: f64,
}

/// `buildVehicle(kind, { color, seed, lod: 'low' })` then
/// `v.setHeadlights?.(0)`: builds a car's object tree in the graph and
/// returns its root (`v.root`), or `None` where the JS import fails.
pub type ParkedCars = fn(&mut SceneGraph, &mut TextureCache, &str, u32, u32) -> Option<NodeId>;

/// `this.waterfall`: where the fall is, for whoever asks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Waterfall {
    pub s: f64,
    pub side: f64,
    pub height: f64,
}

/// The Sierra Pass scenery module.
pub struct Mountain {
    pub label: &'static str,
    pub exclusions: Vec<Exclusion>,
    pub diner: Option<Spot>,
    pub lookout: Option<Spot>,
    /// The car builder of WP 4.1, when there is one.
    pub parked_cars: Option<ParkedCars>,
    /// The group `mountain`, once built.
    pub group: Option<NodeId>,
    pub rock_count: usize,
    pub tree_count: usize,
    pub far_tree_count: usize,
    pub verge_count: [usize; 3],
    pub snow_patches: usize,
    pub waterfall: Option<Waterfall>,
}

impl Mountain {
    /// `new Mountain()` (the constructor reads nothing of `{ zone, key,
    /// level }`).
    pub fn new(_info: &SceneryInfo) -> Mountain {
        Mountain {
            label: "Mountain scenery",
            exclusions: Vec::new(),
            diner: None,
            lookout: None,
            parked_cars: None,
            group: None,
            rock_count: 0,
            tree_count: 0,
            far_tree_count: 0,
            verge_count: [0; 3],
            snow_patches: 0,
            waterfall: None,
        }
    }
}

impl Scenery for Mountain {
    fn name(&self) -> &str {
        "Mountain"
    }

    fn label(&self) -> Option<&str> {
        Some(self.label)
    }

    /// Cut the diner pull-out and the summit lookout into the hillside
    /// before the terrain fields are rasterised.
    fn plan(&mut self, world: &mut World) -> Result<(), String> {
        let t = world.track.as_ref().ok_or("the route is surveyed first")?;
        let terrain = world.terrain.as_mut().ok_or("no terrain")?;
        let wall = t.wall_r[40] as f64;
        let s = 26.0;
        let side = 1.0;
        let lat = wall + 17.0;
        let dp = t.point_at(s, lat);
        let y = t.surface_y(s, wall) - 0.12;
        self.diner = Some(Spot {
            s,
            side,
            lat,
            x: dp.x,
            z: dp.z,
            y,
        });
        terrain.add_flatten(dp.x, dp.z, 17.0, 18.0, Some(y));
        self.exclusions.push(Exclusion {
            x: dp.x,
            z: dp.z,
            r: 22.0,
        });

        if let Some(sm) = t.tag("summit").first() {
            let s = js::round((sm.s0 + sm.s1) / 2.0);
            // Lookout on the side facing the valley.
            let vs = t.zone_start[1] + 800;
            let at = |a: &[f32], i: usize| a.get(i).map_or(f64::NAN, |&v| v as f64);
            let si = s as usize;
            let toward = if (at(&t.px, vs) - at(&t.px, si)) * at(&t.rx, si)
                + (at(&t.pz, vs) - at(&t.pz, si)) * at(&t.rz, si)
                > 0.0
            {
                1.0
            } else {
                -1.0
            };
            let w = if toward > 0.0 {
                at(&t.wall_r, si)
            } else {
                at(&t.wall_l, si)
            };
            let lat = toward * (w + 12.0);
            let p = t.point_at(s, lat);
            let y = t.surface_y(s, toward * w) - 0.1;
            self.lookout = Some(Spot {
                s,
                side: toward,
                lat,
                x: p.x,
                z: p.z,
                y,
            });
            terrain.add_flatten(p.x, p.z, 12.0, 14.0, Some(y));
            self.exclusions.push(Exclusion {
                x: p.x,
                z: p.z,
                r: 16.0,
            });
        }
        Ok(())
    }

    fn build(&mut self, world: &mut World) -> Result<(), String> {
        let World {
            track,
            terrain,
            graph,
            textures,
            road,
            root,
            animators,
            ..
        } = world;
        let track = track.as_ref().ok_or("the route is surveyed first")?;
        let terrain = terrain.as_ref().ok_or("no terrain")?;
        let road = road.as_ref().ok_or("the road is built first")?;
        let group = graph.group("mountain");
        let mut b = Build {
            t: track,
            terrain,
            side_l: &road.side_l,
            side_r: &road.side_r,
            surf: SurfaceSampler::new(terrain),
            exclusions: &self.exclusions,
            graph,
            textures,
            group,
            animators: Vec::new(),
            rock_mat: None,
        };

        let (rocks, rock_mat) = b.build_rocks();
        b.rock_mat = Some(rock_mat);
        self.rock_count = rocks;
        let (trees, far_trees) = b.build_forest();
        self.tree_count = trees;
        self.far_tree_count = far_trees;
        self.verge_count = b.build_verge();
        self.snow_patches = b.build_snow();
        b.build_signs();
        b.build_snow_poles();
        b.build_delineators();
        b.build_start_area();
        if let Some(d) = self.diner {
            b.build_diner(&d, self.parked_cars);
        }
        if let Some(l) = self.lookout {
            b.build_lookout(&l, self.parked_cars);
        }
        self.waterfall = b.build_waterfall();

        let new_animators = std::mem::take(&mut b.animators);
        graph.add(*root, group);
        animators.extend(new_animators);
        self.group = Some(group);
        Ok(())
    }
}

/// A frame's position and right vector at a lateral offset.
fn at(f: &Frame, lat: f64, side: f64) -> (f64, f64) {
    (f.x + f.rx * lat * side, f.z + f.rz * lat * side)
}

/// The scenery being built: what `this` holds during `build()`.
struct Build<'a> {
    t: &'a Track,
    terrain: &'a Terrain,
    /// `track.sideL` / `sideR` (the road's classification).
    side_l: &'a [u8],
    side_r: &'a [u8],
    surf: SurfaceSampler<'a>,
    exclusions: &'a [Exclusion],
    graph: &'a mut SceneGraph,
    textures: &'a mut TextureCache,
    group: NodeId,
    animators: Vec<Box<dyn Animator>>,
    rock_mat: Option<MaterialId>,
}

impl Build<'_> {
    fn m_end(&self) -> f64 {
        self.t.zone_start[1] as f64
    }

    fn add(&mut self, n: NodeId) {
        self.graph.add(self.group, n);
    }

    fn mat(&mut self, m: Material) -> MaterialId {
        self.graph.add_material(m)
    }

    fn geo(&mut self, g: BufferGeometry) -> GeoId {
        self.graph.add_geometry(g)
    }

    fn excluded(&self, x: f64, z: f64) -> bool {
        self.exclusions
            .iter()
            .any(|e| kernel::pow(x - e.x, 2.0) + kernel::pow(z - e.z, 2.0) < e.r * e.r)
    }

    /// Clear of every stretch of road (switchback legs included)?
    fn clear_of_road(&self, x: f64, z: f64, radius: f64) -> bool {
        let info = self.terrain.road_info(x, z);
        if !info.near {
            return info.d > radius + 12.0;
        }
        let i = self.t.idx(info.s);
        let wall = js::max(self.t.wall_l[i] as f64, self.t.wall_r[i] as f64);
        info.d - radius >= wall + 0.6
    }

    fn side_kind(&self, side: f64, i: usize) -> u8 {
        if side < 0.0 {
            self.side_l[i]
        } else {
            self.side_r[i]
        }
    }

    fn wall(&self, side: f64, i: usize) -> f64 {
        if side < 0.0 {
            self.t.wall_l[i] as f64
        } else {
            self.t.wall_r[i] as f64
        }
    }

    /// `instanced(...)` added to the group.
    fn add_instanced(
        &mut self,
        geo: GeoId,
        mat: MaterialId,
        items: &[Item],
        cast: bool,
        receive: bool,
    ) {
        let n = instanced(self.graph, geo, mat, items, cast, receive);
        self.add(n);
    }

    /// `mergedMesh(...)` added to the group (if any).
    fn add_merged(
        &mut self,
        geos: Vec<BufferGeometry>,
        mat: MaterialId,
        cast: bool,
        receive: bool,
    ) {
        if let Some(n) = merged_mesh(self.graph, geos, mat, cast, receive) {
            self.add(n);
        }
    }

    // ── Rocks ───────────────────────────────────────────────────────────

    fn build_rocks(&mut self) -> (usize, MaterialId) {
        let mut rng = Mulberry32::new(4242);
        let m_end = self.m_end();
        let n_chunks = (m_end / CHUNK).ceil() as usize + 1;
        // Rocks the chase camera passes close to get the detailed mesh; the
        // outcrops scattered up the slopes use a coarser one.
        let mut big: Vec<Vec<Item>> = vec![Vec::new(); n_chunks];
        let mut outcrops: Vec<Vec<Item>> = vec![Vec::new(); n_chunks];
        let mut scree: Vec<Item> = Vec::new();
        // Instance tints multiply the rock's baked vertex colours (granite
        // greys with a few warmer, iron-stained stones).
        const PALETTE: [u32; 7] = [
            0xc4bdb3, 0xb3aca2, 0xcac0b0, 0xa8a29a, 0xd2c8b6, 0xb8ad9d, 0xc9b39a,
        ];

        #[derive(Default, Clone, Copy)]
        struct Opts {
            stretch: Option<f64>,
            flat: Option<f64>,
            yaw: Option<f64>,
            on_slope: bool,
            dark: bool,
        }
        enum List {
            Big(usize),
            Outcrop(usize),
            Scree,
        }
        // `addRock(list, x, z, size, embed, opts)`.
        let add_rock = |me: &mut Self,
                        rng: &mut Mulberry32,
                        big: &mut Vec<Vec<Item>>,
                        outcrops: &mut Vec<Vec<Item>>,
                        scree: &mut Vec<Item>,
                        list: List,
                        x: f64,
                        z: f64,
                        size: f64,
                        embed: f64,
                        o: Opts|
         -> bool {
            let sx = size * o.stretch.unwrap_or_else(|| lerp(0.8, 1.35, rng.next_f64()));
            let sz = size * lerp(0.75, 1.25, rng.next_f64());
            let sy = size * o.flat.unwrap_or_else(|| lerp(0.7, 1.2, rng.next_f64()));
            // Slabs by the road lie along it, so their long axis can't poke in.
            let aligned = o.yaw.is_some();
            let ry = match o.yaw {
                Some(yaw) => yaw + (rng.next_f64() - 0.5) * 0.5,
                None => rng.next_f64() * PI * 2.0,
            };
            let reach = if aligned {
                sz * 0.95 + sx * 0.28
            } else {
                js::max(sx, sz) * 0.95
            };
            if me.excluded(x, z) {
                return false;
            }
            if !me.clear_of_road(x, z, reach) {
                return false;
            }
            let s = me.surf.sample(x, z);
            let list = match list {
                List::Big(k) => &mut big[k],
                List::Outcrop(k) => &mut outcrops[k],
                List::Scree => scree,
            };
            if o.on_slope {
                // Lie the slab along the face and sink it along the normal, so it
                // reads as an outcrop rather than a shelf sticking out.
                let n = Vector3::new(-s.gx, 1.0, -s.gz).normalize();
                let q = Quaternion::from_unit_vectors(UP, n)
                    .multiply(Quaternion::from_axis_angle(UP, ry));
                let e = sy * embed;
                let col = PALETTE[(rng.next_f64() * PALETTE.len() as f64).floor() as usize];
                let b = lerp(0.8, 1.12, rng.next_f64()) * if o.dark { 0.78 } else { 1.0 };
                list.push(Item {
                    x: x - n.x * e,
                    y: s.h - n.y * e,
                    z: z - n.z * e,
                    sx,
                    sy,
                    sz,
                    q: Some(q),
                    col: Some(col),
                    b: Some(b),
                    ..Item::default()
                });
                return true;
            }
            let y = s.h - sy * js::min(0.75, embed + s.slope * 0.2);
            let rx = (rng.next_f64() - 0.5) * 0.5;
            let rz = (rng.next_f64() - 0.5) * 0.5;
            let col = PALETTE[(rng.next_f64() * PALETTE.len() as f64).floor() as usize];
            let b = lerp(0.8, 1.12, rng.next_f64());
            list.push(Item {
                x,
                y,
                z,
                sx,
                sy,
                sz,
                rx,
                ry,
                rz,
                col: Some(col),
                b: Some(b),
                ..Item::default()
            });
            true
        };

        let t = self.t;
        let mut s = 0.0;
        while s < m_end {
            let i = t.idx(s);
            let ch = (s / CHUNK).floor() as usize;
            let f = t.frame(s);
            let road_yaw = -kernel::atan2(f.fz, f.fx);
            for side in [-1.0, 1.0] {
                let kind = self.side_kind(side, i);
                let wall = self.wall(side, i);
                if kind == 1 {
                    // Boulders heaped at the foot of the rock wall.
                    if rng.next_f64() < 0.5 {
                        let size = lerp(0.5, 2.4, kernel::pow(rng.next_f64(), 1.6));
                        let (x, z) = at(
                            &f,
                            wall + 0.7 + size + rng.next_f64() * rng.next_f64() * 6.0,
                            side,
                        );
                        let o = Opts {
                            yaw: Some(road_yaw),
                            ..Opts::default()
                        };
                        add_rock(
                            self,
                            &mut rng,
                            &mut big,
                            &mut outcrops,
                            &mut scree,
                            List::Big(ch),
                            x,
                            z,
                            size,
                            0.3,
                            o,
                        );
                    }
                    // Larger slabs set into the face above.
                    if rng.next_f64() < 0.16 {
                        let size = lerp(2.5, 7.0, rng.next_f64());
                        let (x, z) = at(&f, wall + 4.0 + rng.next_f64() * 12.0, side);
                        let stretch = lerp(1.2, 2.2, rng.next_f64());
                        let flat = lerp(0.5, 0.9, rng.next_f64());
                        let o = Opts {
                            stretch: Some(stretch),
                            flat: Some(flat),
                            yaw: Some(road_yaw),
                            on_slope: true,
                            dark: false,
                        };
                        add_rock(
                            self,
                            &mut rng,
                            &mut big,
                            &mut outcrops,
                            &mut scree,
                            List::Big(ch),
                            x,
                            z,
                            size,
                            0.5,
                            o,
                        );
                    }
                } else if rng.next_f64() < 0.12 {
                    let size = lerp(0.6, 2.2, kernel::pow(rng.next_f64(), 2.0));
                    let (x, z) = at(&f, wall + 2.0 + rng.next_f64() * 10.0, side);
                    add_rock(
                        self,
                        &mut rng,
                        &mut big,
                        &mut outcrops,
                        &mut scree,
                        List::Big(ch),
                        x,
                        z,
                        size,
                        0.35,
                        Opts::default(),
                    );
                }
                // Scree along the shoulder, thickest below the rock walls.
                let n_scree = if kind == 1 {
                    1 + usize::from(rng.next_f64() < 0.45)
                } else {
                    usize::from(rng.next_f64() < 0.25)
                };
                for _ in 0..n_scree {
                    let size = lerp(0.1, 0.5, kernel::pow(rng.next_f64(), 1.5));
                    let (x, z) = at(
                        &f,
                        wall + 0.7 + size + rng.next_f64() * rng.next_f64() * 4.0,
                        side,
                    );
                    add_rock(
                        self,
                        &mut rng,
                        &mut big,
                        &mut outcrops,
                        &mut scree,
                        List::Scree,
                        x,
                        z,
                        size,
                        0.25,
                        Opts::default(),
                    );
                }
            }
            s += 4.0;
        }

        // Outcrops on steep slopes away from the road give the cliffs texture.
        let b = t.bounds;
        let x1 = self.terrain.x1() + 150.0;
        let mut x = b.min_x - 450.0;
        while x < x1 {
            let mut z = b.min_z - 450.0;
            while z < b.max_z + 450.0 {
                let px = x + (rng.next_f64() - 0.5) * 9.0;
                let pz = z + (rng.next_f64() - 0.5) * 9.0;
                let r0 = rng.next_f64();
                z += 11.0;
                if r0 > 0.5 {
                    continue;
                }
                let wm = self.terrain.zone_weights(px).w[0];
                if wm < 0.5 {
                    continue;
                }
                let far = self.terrain.far(px, pz);
                if far.d < 14.0 || far.d > 320.0 || far.s > m_end + 100.0 {
                    continue;
                }
                let sm = self.surf.sample(px, pz);
                if sm.slope < 0.85 {
                    continue;
                }
                if r0 > 0.22 * smoothstep(0.85, 1.6, sm.slope) {
                    continue;
                }
                let size = lerp(3.0, 9.0, kernel::pow(rng.next_f64(), 1.5))
                    * if far.d < 60.0 { 0.6 } else { 1.0 };
                let ci = clamp((far.s / CHUNK).floor(), 0.0, (n_chunks - 1) as f64) as usize;
                let stretch = lerp(1.0, 1.8, rng.next_f64());
                let flat = lerp(0.7, 1.1, rng.next_f64());
                let o = Opts {
                    stretch: Some(stretch),
                    flat: Some(flat),
                    yaw: None,
                    on_slope: true,
                    dark: true,
                };
                let list = if far.d < 45.0 {
                    List::Big(ci)
                } else {
                    List::Outcrop(ci)
                };
                add_rock(
                    self,
                    &mut rng,
                    &mut big,
                    &mut outcrops,
                    &mut scree,
                    list,
                    px,
                    pz,
                    size,
                    0.42,
                    o,
                );
            }
            x += 11.0;
        }

        let mat = rock_material(self.graph, self.textures, true);
        let mat = self.mat(mat);
        let crag = RockOpts {
            crag: true,
            ..RockOpts::default()
        };
        let geo_big = [
            self.geo(rock_geometry(3, 2.0, crag)),
            self.geo(rock_geometry(
                17,
                2.0,
                RockOpts {
                    squash: 0.8,
                    ..crag
                },
            )),
        ];
        let geo_far = self.geo(rock_geometry(5, 1.0, crag));
        let geo_small = self.geo(rock_geometry(
            9,
            0.0,
            RockOpts {
                squash: 0.8,
                lichen: 0.4,
                ..RockOpts::default()
            },
        ));
        for (k, items) in big.iter().enumerate() {
            if items.is_empty() {
                continue;
            }
            self.add_instanced(geo_big[k & 1], mat, items, true, true);
        }
        let mut k = 0;
        while k < n_chunks {
            let mut items = outcrops[k].clone();
            if let Some(next) = outcrops.get(k + 1) {
                items.extend_from_slice(next);
            }
            if !items.is_empty() {
                self.add_instanced(geo_far, mat, &items, true, true);
            }
            k += 2;
        }
        if !scree.is_empty() {
            self.add_instanced(geo_small, mat, &scree, false, true);
        }
        let count = big.iter().map(Vec::len).sum::<usize>()
            + outcrops.iter().map(Vec::len).sum::<usize>()
            + scree.len();
        (count, mat)
    }

    // ── Forest ──────────────────────────────────────────────────────────

    fn build_forest(&mut self) -> (usize, usize) {
        let t = self.t;
        let tr = self.terrain;
        let mut rng = Mulberry32::new(777);
        let n2 = &tr.noise2;
        let m_end = self.m_end();
        let n_chunks = (m_end / CHUNK).ceil() as usize + 1;
        // Three levels of detail, chosen by distance from the road (the camera
        // never leaves it): full trees on the verge, simpler ones in the forest
        // behind, and a few-triangle version for the far hillsides.
        const VERGE: f64 = 75.0;
        let mut verge_spruce: Vec<Vec<Item>> = vec![Vec::new(); n_chunks];
        let mut verge_fir: Vec<Vec<Item>> = vec![Vec::new(); n_chunks];
        let mut mid: Vec<Vec<Item>> = vec![Vec::new(); n_chunks];
        let mut far_quads: Vec<((i64, i64), Vec<Item>)> = Vec::new();
        // Tints over the baked greens: most trees neutral, some bluer (spruce),
        // some olive or sun-scorched, a few darker.
        const GREENS: [u32; 8] = [
            0xffffff, 0xf0f4e8, 0xdde8ea, 0xfff2d0, 0xc9d6c4, 0xe6efff, 0xb8c4b0, 0xf5e8c8,
        ];
        let forest_mask =
            |x: f64, z: f64| smoothstep(0.1, 0.35, fbm(n2, x / 420.0 + 7.0, z / 420.0, 3));
        // Stands of one species rather than salt-and-pepper mixing.
        let fir_mask = |x: f64, z: f64| fbm(n2, x / 160.0 - 3.0, z / 160.0 + 11.0, 2) > 0.08;
        let pick =
            |rng: &mut Mulberry32| GREENS[(rng.next_f64() * GREENS.len() as f64).floor() as usize];
        let b = t.bounds;
        let x1 = tr.x1() + 250.0;

        // Detailed pines near the road.
        let mut x = b.min_x - NEAR_FOREST;
        while x < x1 {
            let mut z = b.min_z - NEAR_FOREST;
            while z < b.max_z + NEAR_FOREST {
                let px = x + (rng.next_f64() - 0.5) * 4.5;
                let pz = z + (rng.next_f64() - 0.5) * 4.5;
                let r = rng.next_f64();
                z += 5.0;
                let wm = tr.zone_weights(px).w[0];
                if wm <= 0.02 {
                    continue;
                }
                let f = forest_mask(px, pz);
                let mut p = (0.03 + f * 0.55) * wm;
                let far = tr.far(px, pz);
                if far.d > NEAR_FOREST || far.s > m_end + 150.0 {
                    continue;
                }
                // Forest crowds the road where the ground allows it.
                p *= 1.0
                    + 0.9 * (1.0 - smoothstep(12.0, 70.0, far.d)) * smoothstep(0.02, 0.2, f + 0.1);
                if r > p {
                    continue;
                }
                if self.excluded(px, pz) || !self.clear_of_road(px, pz, 2.2) {
                    continue;
                }
                let sm = self.surf.sample(px, pz);
                if sm.slope > 0.8 {
                    continue;
                }
                let line = 330.0 + fbm(n2, px / 300.0, pz / 300.0, 2) * 60.0;
                let alt = smoothstep(line - 50.0, line + 15.0, sm.h);
                if rng.next_f64() > 1.0 - alt {
                    continue;
                }
                // Stunted near the tree line.
                let sc = lerp(0.7, 1.9, kernel::pow(rng.next_f64(), 1.3))
                    * (0.8 + f * 0.35)
                    * (1.0 - alt * 0.35);
                let ci = clamp((far.s / CHUNK).floor(), 0.0, (n_chunks - 1) as f64) as usize;
                let fir = if fir_mask(px, pz) {
                    rng.next_f64() < 0.8
                } else {
                    rng.next_f64() < 0.15
                };
                let sx = sc * lerp(0.8, 1.15, rng.next_f64());
                let sy = sc
                    * lerp(0.9, 1.3, rng.next_f64())
                    * if fir && far.d >= VERGE { 1.1 } else { 1.0 };
                let sz = sc * lerp(0.8, 1.15, rng.next_f64());
                let ry = rng.next_f64() * TURN;
                let rx = (rng.next_f64() - 0.5) * 0.06;
                let rz = (rng.next_f64() - 0.5) * 0.06;
                let col = pick(&mut rng);
                let bb = lerp(0.8, 1.15, rng.next_f64());
                let item = Item {
                    x: px,
                    z: pz,
                    y: sm.h - 0.3,
                    sx,
                    sy,
                    sz,
                    ry,
                    rx,
                    rz,
                    col: Some(col),
                    b: Some(bb),
                    q: None,
                };
                let list = if far.d < VERGE {
                    if fir {
                        &mut verge_fir[ci]
                    } else {
                        &mut verge_spruce[ci]
                    }
                } else {
                    &mut mid[ci]
                };
                list.push(item);
            }
            x += 5.0;
        }

        // Distant forest cover: a two-tier silhouette per tree on a finer grid,
        // so the far slopes read as continuous forest with ragged edges.
        let (min_x, max_x, min_z, max_z) =
            (tr.min_x + 200.0, x1, tr.min_z + 200.0, tr.max_z - 200.0);
        let mut x = min_x;
        while x < max_x {
            let mut z = min_z;
            while z < max_z {
                let px = x + (rng.next_f64() - 0.5) * 11.0;
                let pz = z + (rng.next_f64() - 0.5) * 11.0;
                let r = rng.next_f64();
                z += 12.5;
                let wm = tr.zone_weights(px).w[0];
                if wm <= 0.02 {
                    continue;
                }
                let f = forest_mask(px, pz);
                let p = (0.012 + f * 0.62) * wm;
                if r > p {
                    continue;
                }
                let far = tr.far(px, pz);
                if far.d <= NEAR_FOREST || far.d > 2300.0 {
                    continue;
                }
                let sm = self.surf.sample(px, pz);
                if sm.slope > 0.75 {
                    continue;
                }
                let line = 340.0 + fbm(n2, px / 300.0, pz / 300.0, 2) * 70.0;
                if sm.h > line + rng.next_f64() * 30.0 {
                    continue;
                }
                let sc =
                    lerp(1.1, 2.1, rng.next_f64()) * if sm.h > line - 40.0 { 0.75 } else { 1.0 };
                let key = (
                    ((px - tr.min_x) / 3000.0).floor() as i64,
                    ((pz - tr.min_z) / 3000.0).floor() as i64,
                );
                let sy = sc * lerp(0.85, 1.25, rng.next_f64());
                let ry = rng.next_f64() * TURN;
                let col = pick(&mut rng);
                let bb = lerp(0.75, 1.1, rng.next_f64());
                let item = Item {
                    x: px,
                    z: pz,
                    y: sm.h - 0.5,
                    sx: sc,
                    sy,
                    sz: sc,
                    ry,
                    col: Some(col),
                    b: Some(bb),
                    ..Item::default()
                };
                match far_quads.iter_mut().find(|(k, _)| *k == key) {
                    Some((_, l)) => l.push(item),
                    None => far_quads.push((key, vec![item])),
                }
            }
            x += 12.5;
        }

        let mat = self.mat(foliage_material(&[]));
        let geo_s = self.geo(conifer_geometry("spruce", 0, 11));
        let geo_f = self.geo(conifer_geometry("fir", 0, 12));
        let geo_mid = self.geo(conifer_geometry("spruce", 1, 13));
        for k in 0..n_chunks {
            if !verge_spruce[k].is_empty() {
                self.add_instanced(geo_s, mat, &verge_spruce[k], true, true);
            }
            if !verge_fir[k].is_empty() {
                self.add_instanced(geo_f, mat, &verge_fir[k], true, true);
            }
        }
        // The mid-distance forest is cheap per tree; pairs of chunks share a mesh.
        let mut k = 0;
        while k < n_chunks {
            let mut items = mid[k].clone();
            if let Some(next) = mid.get(k + 1) {
                items.extend_from_slice(next);
            }
            if !items.is_empty() {
                self.add_instanced(geo_mid, mat, &items, true, true);
            }
            k += 2;
        }
        let far_geo = self.geo(conifer_geometry("spruce", 2, 14));
        let far_mat = self.mat(
            Material::lambert()
                .set("vertexColors", true)
                .set("side", DOUBLE_SIDE),
        );
        for (_, items) in &far_quads {
            self.add_instanced(far_geo, far_mat, items, false, false);
        }
        let trees = mid.iter().map(Vec::len).sum::<usize>()
            + verge_spruce.iter().map(Vec::len).sum::<usize>()
            + verge_fir.iter().map(Vec::len).sum::<usize>();
        let far_trees = far_quads.iter().map(|(_, l)| l.len()).sum();
        (trees, far_trees)
    }

    // ── Verge: grass tussocks, wildflowers, shrubs ──────────────────────
    // Ground cover on the shoulders and the open slopes just off the road,
    // where the chase camera sees it streak past. Clumps are tiny instanced
    // templates, chunked coarsely since each chunk costs a draw call.

    fn build_verge(&mut self) -> [usize; 3] {
        let t = self.t;
        let tr = self.terrain;
        let mut rng = Mulberry32::new(9191);
        let n2 = &tr.noise2;
        let ch = CHUNK * 2.0;
        let m_end = self.m_end();
        let n_chunks = (m_end / ch).ceil() as usize + 1;
        let mut grass: Vec<Vec<Item>> = vec![Vec::new(); n_chunks];
        let mut flowers: Vec<Vec<Item>> = vec![Vec::new(); n_chunks];
        let mut shrubs: Vec<Vec<Item>> = vec![Vec::new(); n_chunks];
        const STRAW: [u32; 4] = [0xffffff, 0xf2e6c4, 0xe8dcb0, 0xfff4dc];
        const GREEN: [u32; 4] = [0x9fbf6a, 0xb4c878, 0x8cae62, 0xc8d090];
        const BLOOM: [u32; 7] = [
            0xffd23a, 0xfff4e0, 0x9a6ee0, 0xff8a2a, 0x6e8ef0, 0xffe070, 0xe86aa0,
        ];
        const SHRUB_COL: [u32; 5] = [0x5d6e3a, 0x4f6034, 0x6f7442, 0x57663c, 0x7a6e44];
        let mut s = 6.0;
        while s < m_end + 60.0 {
            let i = t.idx(s);
            let f = t.frame(s);
            let ci = clamp((s / ch).floor(), 0.0, (n_chunks - 1) as f64) as usize;
            for side in [-1.0, 1.0] {
                let kind = self.side_kind(side, i);
                let wall = self.wall(side, i);
                let tries = if kind == 1 {
                    usize::from(rng.next_f64() < 0.4)
                } else {
                    5
                };
                for _ in 0..tries {
                    let lat = side
                        * (wall
                            + 0.9
                            + rng.next_f64() * rng.next_f64() * if kind == 1 { 2.5 } else { 26.0 });
                    let x = f.x + f.rx * lat + f.fx * (rng.next_f64() - 0.5) * 2.5;
                    let z = f.z + f.rz * lat + f.fz * (rng.next_f64() - 0.5) * 2.5;
                    if self.excluded(x, z) || !self.clear_of_road(x, z, 0.3) {
                        continue;
                    }
                    let h = self.surf.sample(x, z);
                    if h.slope > 1.1 || h.h > 300.0 {
                        continue;
                    }
                    // Patches: lush where the noise says damp, straw-dry elsewhere.
                    let wet = fbm(n2, x / 60.0 + 5.0, z / 60.0, 2);
                    let sc = lerp(0.85, 1.7, rng.next_f64()) * if h.h > 270.0 { 0.7 } else { 1.0 };
                    let sy = sc * lerp(0.8, 1.3, rng.next_f64());
                    let ry = rng.next_f64() * TURN;
                    let pal = if wet > 0.1 { &GREEN } else { &STRAW };
                    let col = pal[(rng.next_f64() * 4.0).floor() as usize];
                    let bb = lerp(0.8, 1.1, rng.next_f64());
                    grass[ci].push(Item {
                        x,
                        z,
                        y: h.h - 0.05,
                        sx: sc,
                        sy,
                        sz: sc,
                        ry,
                        rx: -h.gz * 0.5,
                        rz: h.gx * 0.5,
                        col: Some(col),
                        b: Some(bb),
                        q: None,
                    });
                    // Drifts of wildflowers in the damper patches.
                    if wet > 0.05 && rng.next_f64() < 0.35 && h.h < 285.0 {
                        let fi = ((fbm(n2, x / 25.0, z / 25.0 + 9.0, 2) * 0.5 + 0.5)
                            * BLOOM.len() as f64
                            * 1.6)
                            .floor()
                            % BLOOM.len() as f64;
                        let fx = x + (rng.next_f64() - 0.5);
                        let fz = z + (rng.next_f64() - 0.5);
                        let sy = lerp(0.8, 1.2, rng.next_f64());
                        let ry = rng.next_f64() * TURN;
                        flowers[ci].push(Item {
                            x: fx,
                            z: fz,
                            y: h.h - 0.02,
                            sx: 1.0,
                            sy,
                            sz: 1.0,
                            ry,
                            col: Some(BLOOM[fi as usize]),
                            b: Some(1.0),
                            ..Item::default()
                        });
                    }
                }
                // Shrubs a little further out, some tucked against the rock walls.
                if rng.next_f64() < if kind == 1 { 0.06 } else { 0.22 } {
                    let lat = side
                        * (wall
                            + if kind == 1 {
                                1.4
                            } else {
                                2.5 + rng.next_f64() * 22.0
                            });
                    let x = f.x + f.rx * lat;
                    let z = f.z + f.rz * lat;
                    let sz = lerp(0.6, 1.6, kernel::pow(rng.next_f64(), 1.4));
                    if self.excluded(x, z) || !self.clear_of_road(x, z, sz) {
                        continue;
                    }
                    let h = self.surf.sample(x, z);
                    if h.slope > 1.0 || h.h > 295.0 {
                        continue;
                    }
                    let sxx = sz * lerp(1.0, 1.6, rng.next_f64());
                    let szz = sz * lerp(1.0, 1.4, rng.next_f64());
                    let ry = rng.next_f64() * TURN;
                    let col = SHRUB_COL[(rng.next_f64() * SHRUB_COL.len() as f64).floor() as usize];
                    let bb = lerp(0.8, 1.2, rng.next_f64());
                    shrubs[ci].push(Item {
                        x,
                        z,
                        y: h.h - 0.15 * sz,
                        sx: sxx,
                        sy: sz,
                        sz: szz,
                        ry,
                        col: Some(col),
                        b: Some(bb),
                        ..Item::default()
                    });
                }
            }
            s += 2.5;
        }
        let mat = self.mat(foliage_material(&[]));
        let g_geo = self.geo(grass_clump_geometry(11, 5));
        let f_geo = self.geo(flower_geometry(5, 9));
        let s_geo = self.geo(shrub_geometry(21, 1.0));
        let shrub_mat = self.mat(foliage_material(&[("side", Param::Num(FRONT_SIDE))]));
        for k in 0..n_chunks {
            if !grass[k].is_empty() {
                self.add_instanced(g_geo, mat, &grass[k], false, true);
            }
        }
        let mut k = 0;
        while k < n_chunks {
            let mut fl = flowers[k].clone();
            if let Some(next) = flowers.get(k + 1) {
                fl.extend_from_slice(next);
            }
            let mut sh = shrubs[k].clone();
            if let Some(next) = shrubs.get(k + 1) {
                sh.extend_from_slice(next);
            }
            if !fl.is_empty() {
                self.add_instanced(f_geo, mat, &fl, false, true);
            }
            if !sh.is_empty() {
                self.add_instanced(s_geo, shrub_mat, &sh, true, true);
            }
            k += 2;
        }
        let count = |l: &Vec<Vec<Item>>| l.iter().map(Vec::len).sum::<usize>();
        [count(&grass), count(&flowers), count(&shrubs)]
    }

    // ── Snow patches at the summit ──────────────────────────────────────
    // Irregular discs draped on the rendered terrain wherever it rises above
    // a ragged snow line, on the gentler ground. One merged mesh.

    fn build_snow(&mut self) -> usize {
        let t = self.t;
        let tr = self.terrain;
        let mut rng = Mulberry32::new(606);
        let n2 = &tr.noise2;
        let mut pos: Vec<f64> = Vec::new();
        let mut col: Vec<f64> = Vec::new();
        let mut idx: Vec<u32> = Vec::new();
        // mound > 0 heaps the middle up instead of lying flat.
        let patch = |me: &mut Self,
                     rng: &mut Mulberry32,
                     pos: &mut Vec<f64>,
                     col: &mut Vec<f64>,
                     idx: &mut Vec<u32>,
                     cx: f64,
                     cz: f64,
                     r_: f64| {
            let lift = 0.12;
            let mound = 0.0;
            const RINGS: usize = 3;
            const SEG: usize = 11;
            let base = (pos.len() / 3) as u32;
            let ph = rng.next_f64() * 10.0;
            let h = me.surf.sample(cx, cz);
            pos.extend([cx, h.h + lift + mound, cz]);
            col.extend([1.0, 1.0, 1.0]);
            for r in 1..=RINGS {
                for j in 0..SEG {
                    let a = (j as f64 / SEG as f64) * PI * 2.0;
                    let edge = 0.65
                        + 0.35 * kernel::sin(a * 3.0 + ph) * kernel::cos(a * 2.0 - ph * 0.7)
                        + (rng.next_f64() - 0.5) * 0.25;
                    let rr = r_
                        * (r as f64 / RINGS as f64)
                        * if r == RINGS {
                            edge
                        } else {
                            lerp(1.0, edge, 0.5)
                        };
                    let x = cx + kernel::cos(a) * rr;
                    let z = cz + kernel::sin(a) * rr;
                    let h = me.surf.sample(x, z);
                    pos.extend([
                        x,
                        h.h + lift * if r == RINGS { 0.4 } else { 1.0 }
                            + mound * (1.0 - kernel::pow(r as f64 / RINGS as f64, 2.0)),
                        z,
                    ]);
                    // Thin, dirty edge; clean, bright middle.
                    let v = if r == RINGS {
                        0.62
                    } else if r == RINGS - 1 {
                        0.88
                    } else {
                        1.0
                    };
                    col.extend([v, v, v * 1.02]);
                }
            }
            let (seg, b) = (SEG as u32, base);
            for j in 0..seg {
                idx.extend([b, b + 1 + ((j + 1) % seg), b + 1 + j]);
            }
            for r in 1..RINGS as u32 {
                let a0 = b + 1 + (r - 1) * seg;
                let b0 = b + 1 + r * seg;
                for j in 0..seg {
                    let j1 = (j + 1) % seg;
                    idx.extend([a0 + j, a0 + j1, b0 + j, a0 + j1, b0 + j1, b0 + j]);
                }
            }
        };
        // Hillside patches around the summit.
        let s_mid = match t.tag("summit").first() {
            Some(sm) => (sm.s0 + sm.s1) / 2.0,
            None => self.m_end() * 0.6,
        };
        let c = t.frame(s_mid);
        let mut n = 0;
        let mut k = 0;
        while k < 8000 && n < 320 {
            k += 1;
            let x = c.x + (rng.next_f64() - 0.5) * 2400.0;
            let z = c.z + (rng.next_f64() - 0.5) * 2400.0;
            if tr.zone_weights(x).w[0] < 0.6 {
                continue;
            }
            let h = self.surf.sample(x, z);
            let line = 292.0 + fbm(n2, x / 180.0, z / 180.0 + 4.0, 3) * 40.0;
            if h.h < line || h.slope > 0.55 {
                continue;
            }
            if !self.clear_of_road(x, z, 25.0) || self.excluded(x, z) {
                continue;
            }
            // Only on ground that stays gentle across the whole patch; on a steep
            // face a draped disc reads as a white shard.
            let r = lerp(4.0, 14.0, kernel::pow(rng.next_f64(), 1.5));
            let h0 = h.h;
            let mut steep = false;
            let mut j = 0;
            while j < 6 && !steep {
                let a = (j as f64 / 6.0) * PI * 2.0;
                let s = self
                    .surf
                    .sample(x + kernel::cos(a) * r, z + kernel::sin(a) * r);
                if (s.h - h0).abs() > r * 0.45 {
                    steep = true;
                }
                j += 1;
            }
            if steep {
                continue;
            }
            patch(self, &mut rng, &mut pos, &mut col, &mut idx, x, z, r);
            n += 1;
        }
        if idx.is_empty() {
            return n;
        }
        let mut g = BufferGeometry::new();
        g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
        g.set_attribute("color", BufferAttribute::from_f64(&col, 3));
        g.set_index(&idx);
        g.compute_vertex_normals();
        let nrm = g.get_attribute_mut("normal").expect("normals");
        for i in 0..nrm.count() {
            if nrm.get_y(i) < 0.0 {
                let (x, y, z) = (nrm.get_x(i), nrm.get_y(i), nrm.get_z(i));
                nrm.set_xyz(i, -x, -y, -z);
            }
        }
        let mat = self.mat(
            Material::standard()
                .set("color", 0xb8c0cc)
                .set("roughness", 0.75)
                .set("vertexColors", true)
                .set("polygonOffset", true)
                .set("polygonOffsetFactor", -2.0)
                .set("polygonOffsetUnits", -2.0),
        );
        let geo = self.geo(g);
        let m = self.graph.mesh(geo, mat);
        let o = self.graph.get_mut(m);
        o.receive_shadow = true;
        o.matrix_auto_update = false;
        self.add(m);
        n
    }

    // ── Signs ───────────────────────────────────────────────────────────

    fn build_signs(&mut self) {
        let t = self.t;
        let mut atlas = SignAtlas::new();
        struct Sign {
            s: f64,
            rect: Rect,
            w: f64,
            h: f64,
            height: f64,
            side: f64,
            extra: Option<(Rect, f64, f64)>,
        }
        let mut signs: Vec<Sign> = Vec::new();
        let mut add = |s: f64,
                       rect: Rect,
                       w: f64,
                       h: f64,
                       height: f64,
                       side: f64,
                       extra: Option<(Rect, f64, f64)>| {
            signs.push(Sign {
                s,
                rect,
                w,
                h,
                height,
                side,
                extra,
            })
        };
        use canvas::FONT;
        let mph = |atlas: &mut SignAtlas, n: u32| {
            atlas.add(200.0, 240.0, |g, w, h| {
                panel(
                    g,
                    w,
                    h,
                    "#f5c518",
                    "#111",
                    "#111",
                    &[&n.to_string(), "MPH"],
                    &[110.0, 52.0],
                )
            })
        };
        let tag_d = |mut r: Rect| {
            r.diamond = true;
            r
        };
        let hairpin = |atlas: &mut SignAtlas, dir: f64| {
            tag_d(atlas.add(256.0, 256.0, |g, w, h| {
                diamond(g, w, h, |g2, cx, cy| {
                    g2.save();
                    g2.translate(cx, cy);
                    g2.scale(dir, 1.0);
                    g2.set_line_width(16.0);
                    g2.set_line_cap("butt");
                    g2.begin_path();
                    g2.move_to(22.0, 62.0);
                    g2.line_to(22.0, -8.0);
                    g2.arc(-4.0, -8.0, 26.0, 0.0, PI, true);
                    g2.line_to(-30.0, 28.0);
                    g2.stroke();
                    g2.begin_path();
                    g2.move_to(-52.0, 22.0);
                    g2.line_to(-8.0, 22.0);
                    g2.line_to(-30.0, 58.0);
                    g2.close_path();
                    g2.fill();
                    g2.restore();
                })
            }))
        };
        let text_diamond = |atlas: &mut SignAtlas, lines: &[&str], size: f64| {
            tag_d(atlas.add(256.0, 256.0, |g, w, h| {
                diamond(g, w, h, |g2, cx, cy| {
                    g2.set_text_align("center");
                    g2.set_text_baseline("middle");
                    g2.set_font(&format!("bold {size}px {FONT}"));
                    let n = lines.len() as f64;
                    for (i, l) in lines.iter().enumerate() {
                        g2.fill_text(l, cx, cy + (i as f64 - (n - 1.0) / 2.0) * size * 1.05);
                    }
                })
            }))
        };
        let hairpin_l = hairpin(&mut atlas, -1.0);
        let hairpin_r = hairpin(&mut atlas, 1.0);
        let pin15 = mph(&mut atlas, 15);
        for hp in t.tag("hairpin") {
            let r = if hp.turn.is_some_and(|x| x < 0.0) {
                hairpin_l
            } else {
                hairpin_r
            };
            add(
                hp.s0 - 75.0,
                r,
                1.2,
                1.2,
                2.5,
                1.0,
                Some((pin15, 0.75, 0.9)),
            );
        }
        if let Some(sb) = t.tag("switchbacks").first() {
            let r = atlas.add(512.0, 256.0, |g, w, h| {
                panel(
                    g,
                    w,
                    h,
                    "#f5c518",
                    "#111",
                    "#111",
                    &["SWITCHBACKS", "NEXT 2 MILES"],
                    &[70.0, 58.0],
                )
            });
            add(sb.s0 - 170.0, r, 2.6, 1.3, 2.0, 1.0, None);
        }
        if let Some(cy) = t.tag("canyon").first() {
            let r = text_diamond(&mut atlas, &["FALLING", "ROCKS"], 44.0);
            add(cy.s0 - 50.0, r, 1.2, 1.2, 2.4, 1.0, None);
        }
        if let Some(sm) = t.tag("summit").first() {
            let r = atlas.add(512.0, 288.0, |g, w, h| {
                panel(
                    g,
                    w,
                    h,
                    "#5a3a22",
                    "#f3ead8",
                    "#f3ead8",
                    &["SIERRA PASS", "SUMMIT", "ELEV 7,240 FT"],
                    &[58.0, 44.0, 48.0],
                )
            });
            add(sm.s0 + 30.0, r, 2.8, 1.575, 2.0, 1.0, None);
        }
        if let Some(ds) = t.tag("descent").first() {
            let r = atlas.add(512.0, 256.0, |g, w, h| {
                panel(
                    g,
                    w,
                    h,
                    "#f5c518",
                    "#111",
                    "#111",
                    &["TRUCKS", "USE LOW GEAR"],
                    &[70.0, 58.0],
                )
            });
            add(ds.s0 - 60.0, r, 2.4, 1.2, 2.0, 1.0, None);
            let r = text_diamond(&mut atlas, &["7%", "GRADE"], 58.0);
            add(ds.s0 + 40.0, r, 1.2, 1.2, 2.4, 1.0, None);
            let g2 = atlas.add(512.0, 256.0, |g, w, h| {
                panel(g, w, h, "#0b6b3a", "#fff", "#fff", &[], &[]);
                g.set_fill_style("#fff");
                g.set_font(&format!("bold 58px {FONT}"));
                g.set_text_baseline("middle");
                g.set_text_align("left");
                g.fill_text("Mill Valley", 36.0, 88.0);
                g.fill_text("Meridian", 36.0, 172.0);
                g.set_text_align("right");
                g.fill_text("3", w - 36.0, 88.0);
                g.fill_text("9", w - 36.0, 172.0);
            });
            add(ds.s0 + 420.0, g2, 3.2, 1.6, 2.2, 1.0, None);
        }
        let lim = atlas.add(200.0, 256.0, |g, w, h| {
            panel(
                g,
                w,
                h,
                "#f8f8f4",
                "#111",
                "#111",
                &["SPEED", "LIMIT", "45"],
                &[44.0, 44.0, 96.0],
            );
        });
        add(240.0, lim, 0.9, 1.15, 2.1, 1.0, None);
        add(1650.0, lim, 0.9, 1.15, 2.1, 1.0, None);

        let tex = canvas::canvas_tex(self.graph, &atlas.canvas);
        let face_mat = self.mat(
            Material::standard()
                .set("map", tex)
                .set("roughness", 0.45)
                .set("metalness", 0.1)
                .set("emissive", 0xffffff)
                .set("emissiveMap", tex)
                .set("emissiveIntensity", 0.04)
                .set("alphaTest", 0.5),
        );
        self.graph
            .add_night(Some(face_mat), "emissiveIntensity", 0.04, 0.55);
        let metal_mat = self.mat(
            Material::standard()
                .set("color", 0x8d9197)
                .set("metalness", 0.6)
                .set("roughness", 0.45),
        );
        let mut faces: Vec<BufferGeometry> = Vec::new();
        let mut metal: Vec<BufferGeometry> = Vec::new();
        let plate = |faces: &mut Vec<BufferGeometry>,
                     metal: &mut Vec<BufferGeometry>,
                     rect: &Rect,
                     w: f64,
                     h: f64,
                     x: f64,
                     y: f64,
                     z: f64,
                     yaw: f64| {
            let mut g = plane_geometry(w, h, 1.0, 1.0);
            let uv = g.get_attribute_mut("uv").expect("uv");
            for k in 0..uv.count() {
                let (u, v) = (uv.get_x(k), uv.get_y(k));
                uv.set_xy(k, lerp(rect.u0, rect.u1, u), lerp(rect.v0, rect.v1, v));
            }
            faces.push(placed(&g, x, y, z, yaw));
            let mut back = if rect.diamond {
                let mut b = plane_geometry(w * 0.69, h * 0.69, 1.0, 1.0);
                b.rotate_z(PI / 4.0);
                b
            } else {
                plane_geometry(w, h, 1.0, 1.0)
            };
            back.rotate_y(PI);
            back.translate(0.0, 0.0, -0.02);
            metal.push(placed(&back, x, y, z, yaw));
        };
        for sg in &signs {
            let f = t.frame(sg.s);
            let i = t.idx(sg.s);
            let wall = self.wall(sg.side, i);
            let kind = self.side_kind(sg.side, i);
            // Keep the whole plate outside the corridor, not just the post.
            let lat = sg.side * (wall + if kind == 1 { 0.75 } else { 1.1 } + sg.w * 0.5);
            let x = f.x + f.rx * lat;
            let z = f.z + f.rz * lat;
            let road_y = f.y - lat * f.bank;
            let hh = self.surf.sample(x, z);
            let base = clamp(hh.h, road_y - 4.0, road_y + 0.6);
            let yaw = kernel::atan2(-f.fx, -f.fz) + sg.side * 0.12;
            let top = road_y + sg.height + sg.h;
            let mut bottom = road_y + sg.height;
            if let Some((r, ew, eh)) = &sg.extra {
                plate(
                    &mut faces,
                    &mut metal,
                    r,
                    *ew,
                    *eh,
                    x,
                    bottom - 0.08 - eh / 2.0,
                    z,
                    yaw,
                );
                bottom -= eh + 0.1;
            }
            let _ = bottom;
            plate(
                &mut faces,
                &mut metal,
                &sg.rect,
                sg.w,
                sg.h,
                x,
                road_y + sg.height + sg.h / 2.0,
                z,
                yaw,
            );
            let post_h = top - 0.1 - base;
            let mut post = cylinder_geometry(0.045, 0.045, post_h, 6.0, 1.0, false, 0.0, PI * 2.0);
            post.translate(0.0, base + post_h / 2.0, 0.0);
            let dx = -kernel::sin(yaw) * 0.05;
            let dz = -kernel::cos(yaw) * 0.05;
            metal.push(placed(&post, x + dx, 0.0, z + dz, yaw));
            if sg.w > 2.0 {
                let off = sg.w * 0.32;
                let px2 = kernel::cos(yaw) * off;
                let pz2 = -kernel::sin(yaw) * off;
                metal.pop();
                metal.push(placed(&post, x + dx + px2, 0.0, z + dz + pz2, yaw));
                metal.push(placed(&post, x + dx - px2, 0.0, z + dz - pz2, yaw));
            }
        }
        self.add_merged(faces, face_mat, false, true);
        self.add_merged(metal, metal_mat, true, true);
    }

    fn build_snow_poles(&mut self) {
        let t = self.t;
        let mut items = Vec::new();
        let mut s = 200.0;
        while s < self.m_end() - 100.0 {
            let f = t.frame(s);
            let cur = s;
            s += 16.0;
            if f.y < 215.0 {
                continue;
            }
            let side = if js::to_int32((cur / 16.0).floor()) & 1 != 0 {
                1.0
            } else {
                -1.0
            };
            let i = t.idx(cur);
            let lat = side * (self.wall(side, i) + 0.75);
            let x = f.x + f.rx * lat;
            let z = f.z + f.rz * lat;
            if !self.clear_of_road(x, z, 0.1) {
                continue;
            }
            let ground = self.surf.sample(x, z).h;
            items.push(Item::at(
                x,
                js::min(f.y - lat * f.bank - 0.2, ground - 0.05),
                z,
                1.0,
                1.0,
                1.0,
            ));
        }
        if items.is_empty() {
            return;
        }
        let c = canvas::snow_pole_canvas();
        let tex = canvas::canvas_tex(self.graph, &c);
        let mut geo = cylinder_geometry(0.035, 0.035, 2.4, 5.0, 1.0, true, 0.0, PI * 2.0);
        geo.translate(0.0, 1.2, 0.0);
        let mat = self.mat(
            Material::standard()
                .set("map", tex)
                .set("roughness", 0.6)
                .set("emissive", 0xffffff)
                .set("emissiveMap", tex)
                .set("emissiveIntensity", 0.05),
        );
        self.graph
            .add_night(Some(mat), "emissiveIntensity", 0.05, 0.5);
        let geo = self.geo(geo);
        self.add_instanced(geo, mat, &items, false, true);
    }

    /// Roadside delineators below the snow-pole line: white posts with a
    /// black band and a reflector (amber on the right, white on the left,
    /// as seen by the driver) that lights up in headlights after dark.
    fn build_delineators(&mut self) {
        let t = self.t;
        let mut posts = Vec::new();
        let mut refl = Vec::new();
        let mut s = 40.0;
        while s < self.m_end() - 60.0 {
            let f = t.frame(s);
            let i = t.idx(s);
            s += 33.0;
            if f.y >= 215.0 {
                continue;
            }
            for side in [-1.0, 1.0] {
                let kind = self.side_kind(side, i);
                if kind == 1 {
                    continue;
                }
                let lat = side * (self.wall(side, i) + 0.8);
                let x = f.x + f.rx * lat;
                let z = f.z + f.rz * lat;
                if !self.clear_of_road(x, z, 0.05) {
                    continue;
                }
                let ground = self.surf.sample(x, z).h;
                let y = js::min(f.y - lat * f.bank - 0.1, ground - 0.02);
                let ry = kernel::atan2(f.fx, f.fz) + PI;
                posts.push(Item {
                    ry,
                    ..Item::at(x, y, z, 1.0, 1.0, 1.0)
                });
                refl.push(Item {
                    ry,
                    col: Some(if side > 0.0 { 0xffa726 } else { 0xf4f6ff }),
                    ..Item::at(x, y, z, 1.0, 1.0, 1.0)
                });
            }
        }
        if posts.is_empty() {
            return;
        }
        let mut post = box_geometry(0.11, 1.15, 0.07, 1.0, 1.0, 1.0);
        post.translate(0.0, 0.575, 0.0);
        let mut band = box_geometry(0.115, 0.14, 0.075, 1.0, 1.0, 1.0);
        band.translate(0.0, 1.0, 0.0);
        let color_of = |g: BufferGeometry, v: f64| {
            let mut g = g;
            let n = g.position().count();
            g.set_attribute(
                "color",
                BufferAttribute::new(mr_scene::BufferData::F32(vec![v as f32; n * 3]), 3, false),
            );
            g
        };
        let a = color_of(post.to_non_indexed(), 0.85);
        let b = color_of(band.to_non_indexed(), 0.02);
        let post_geo = merge_geometries(&[&a, &b], false).expect("same attributes");
        let post_mat = self.mat(
            Material::standard()
                .set("vertexColors", true)
                .set("roughness", 0.55),
        );
        let post_geo = self.geo(post_geo);
        self.add_instanced(post_geo, post_mat, &posts, false, true);
        // Reflector faces the approaching traffic (local +Z after the yaw).
        let mut r = plane_geometry(0.07, 0.16, 1.0, 1.0);
        r.translate(0.0, 0.8, 0.04);
        // Emissive takes the instance colour so one material does amber and
        // white (the `Reflector` patch).
        let refl_mat = self.mat(
            Material::standard()
                .set("color", 0xffffff)
                .set("emissive", 0xffffff)
                .set("emissiveIntensity", 0.1)
                .set("roughness", 0.3)
                .kind(mr_scene::MaterialKind::Reflector, None)
                .uniform("clippingPlanes", serde_json::Value::Null),
        );
        self.graph
            .add_night(Some(refl_mat), "emissiveIntensity", 0.1, 1.6);
        let r = self.geo(r);
        self.add_instanced(r, refl_mat, &refl, false, false);
    }

    // ── Start gantry ────────────────────────────────────────────────────

    fn build_start_area(&mut self) {
        let t = self.t;
        let s = t.start_s;
        let f = t.frame(s);
        let wl = t.wall_l[t.idx(s)] as f64;
        let wr = t.wall_r[t.idx(s)] as f64;
        let yaw = kernel::atan2(f.fx, f.fz); // local +z → forward
        let steel = self.mat(
            Material::standard()
                .set("color", 0x2b2f36)
                .set("metalness", 0.7)
                .set("roughness", 0.35),
        );
        let mut geos = Vec::new();
        const H: f64 = 7.2;
        for lat in [-(wl + 1.25), wr + 1.25] {
            let x = f.x + f.rx * lat;
            let z = f.z + f.rz * lat;
            let y = f.y - lat * f.bank;
            // Truss-style leg: two uprights with rungs.
            for off in [-0.35, 0.35] {
                let mut up = box_geometry(0.16, H + 1.5, 0.16, 1.0, 1.0, 1.0);
                up.translate(0.0, (H + 1.5) / 2.0 - 1.2, off);
                geos.push(placed(&up, x, y, z, yaw));
            }
            for k in 0..7 {
                let mut rung = box_geometry(0.08, 0.08, 0.7, 1.0, 1.0, 1.0);
                rung.translate(0.0, 0.5 + k as f64 * 1.0, 0.0);
                geos.push(placed(&rung, x, y, z, yaw));
            }
            let mut foot = box_geometry(1.1, 0.5, 1.4, 1.0, 1.0, 1.0);
            foot.translate(0.0, -0.05, 0.0);
            geos.push(placed(&foot, x, y, z, yaw));
        }
        let span = wl + wr + 2.5;
        let cx = f.x + f.rx * ((wr - wl) / 2.0);
        let cz = f.z + f.rz * ((wr - wl) / 2.0);
        for dy in [H, H - 0.9] {
            let mut beam = box_geometry(span, 0.18, 0.18, 1.0, 1.0, 1.0);
            beam.translate(0.0, f.y + dy, 0.0);
            geos.push(placed(&beam, cx, 0.0, cz, yaw));
        }
        self.add_merged(geos, steel, true, true);

        // Banner: front says SIERRA PASS, back (seen when looking back) START.
        let bw = span - 1.6;
        let bh = bw * 160.0 / 1024.0;
        for front in [true, false] {
            let c = canvas::banner_canvas(front);
            let tex = canvas::canvas_tex(self.graph, &c);
            let mat = self.mat(
                Material::standard()
                    .set("map", tex)
                    .set("roughness", 0.7)
                    .set("emissive", 0xffffff)
                    .set("emissiveMap", tex)
                    .set("emissiveIntensity", 0.15),
            );
            let mut g = plane_geometry(bw, bh, 1.0, 1.0);
            // Front faces approaching cars (−forward); back faces forward.
            g.rotate_y(if front { PI } else { 0.0 });
            g.translate(
                0.0,
                f.y + H + 0.1 - bh / 2.0,
                if front { -0.05 } else { 0.05 },
            );
            let geo = self.geo(placed(&g, cx, 0.0, cz, yaw));
            let mesh = self.graph.mesh(geo, mat);
            self.graph.get_mut(mesh).matrix_auto_update = false;
            self.add(mesh);
        }
    }

    /// `rotateY(extraYaw)`, `translate(lx, ly, lz)`, then `placed` at the
    /// building's frame: the diner's `P`.
    fn gravel(&mut self, color: u32, repeat: f64) -> MaterialId {
        let cached = self
            .graph
            .cached_texture(&self.textures.gravel_texture(), Layer::Main, "");
        let map = self.graph.clone_texture(cached);
        canvas::set_repeat(self.graph, map, repeat, repeat);
        self.mat(
            Material::standard()
                .set("map", map)
                .set("roughness", 1.0)
                .set("color", color)
                .set("polygonOffset", true)
                .set("polygonOffsetFactor", -1.0),
        )
    }

    /// A parked car (`buildVehicle` through the hook, then `bakeStatic`),
    /// at (x, y, z) turned to `yaw`.
    fn parked_car(
        &mut self,
        hook: Option<ParkedCars>,
        kind: &str,
        color: u32,
        seed: u32,
        p: Vector3,
        yaw: f64,
    ) {
        let Some(make) = hook else { return };
        let Some(root) = make(self.graph, self.textures, kind, color, seed) else {
            return;
        };
        let o = self.graph.get_mut(root);
        o.position = p;
        o.set_rotation(&Euler::new(0.0, yaw, 0.0));
        let baked = bake_static(self.graph, root);
        self.add(baked);
    }

    // ── Roadside diner at the foot of the pass ──────────────────────────

    fn build_diner(&mut self, d: &Spot, cars: Option<ParkedCars>) {
        let t = self.t;
        let f = t.frame(d.s);
        let y = d.y;
        // Building faces the road: its local +z points back toward the road.
        let yaw = kernel::atan2(-f.rx * d.side, -f.rz * d.side);
        let mut g0 = circle_geometry(16.0, 28.0, 0.0, PI * 2.0);
        g0.rotate_x(-PI / 2.0);
        g0.translate(0.0, y + 0.06, 0.0);
        // The material is made with the cached gravel texture, then given a
        // clone of it repeated six times.
        let gravel = self.gravel(0xb0a898, 6.0);
        let pad_geo = self.geo(placed(&g0, d.x, 0.0, d.z, 0.0));
        let pad = self.graph.mesh(pad_geo, gravel);
        self.graph.get_mut(pad).receive_shadow = true;
        self.add(pad);

        let siding = self.mat(
            Material::standard()
                .set("color", 0xd8ccb0)
                .set("roughness", 0.85),
        );
        let roof = self.mat(
            Material::standard()
                .set("color", 0x5b3b2c)
                .set("roughness", 0.9),
        );
        let trim = self.mat(
            Material::standard()
                .set("color", 0xa3272c)
                .set("roughness", 0.6),
        );
        let glass = self.mat(
            Material::standard()
                .set("color", 0x3a3226)
                .set("roughness", 0.2)
                .set("metalness", 0.2)
                .set("emissive", 0xffc27a)
                .set("emissiveIntensity", 0.25),
        );
        self.graph
            .add_night(Some(glass), "emissiveIntensity", 0.25, 1.6);
        let mut wall_g = Vec::new();
        let mut roof_g = Vec::new();
        let mut trim_g = Vec::new();
        let mut glass_g = Vec::new();
        // Local frame: origin at pad centre, +z toward road.
        let bx = d.x + f.rx * d.side * 4.0;
        let bz = d.z + f.rz * d.side * 4.0;
        let p = |mut geo: BufferGeometry, lx: f64, ly: f64, lz: f64, extra_yaw: f64| {
            geo.rotate_y(extra_yaw);
            geo.translate(lx, ly, lz);
            placed(&geo, bx, y, bz, yaw)
        };
        let bx_ = |w: f64, h: f64, dd: f64| box_geometry(w, h, dd, 1.0, 1.0, 1.0);
        wall_g.push(p(bx_(13.0, 4.0, 7.0), 0.0, 2.0, 0.0, 0.0));
        // Gable roof as a triangular prism.
        let mut roof_shape = Shape::new();
        roof_shape.move_to(-4.1, 0.0);
        roof_shape.line_to(0.0, 2.2);
        roof_shape.line_to(4.1, 0.0);
        roof_shape.close_path();
        let mut rg = extrude_geometry(&[roof_shape], &ExtrudeOptions::flat(14.0));
        rg.translate(0.0, 0.0, -7.0);
        rg.rotate_y(PI / 2.0);
        roof_g.push(p(rg, 0.0, 4.0, 0.0, 0.0));
        trim_g.push(p(bx_(13.2, 0.35, 7.2), 0.0, 3.9, 0.0, 0.0));
        trim_g.push(p(bx_(13.1, 0.5, 7.1), 0.0, 0.25, 0.0, 0.0));
        // Porch roof + posts.
        roof_g.push(p(bx_(13.0, 0.2, 2.6), 0.0, 3.3, 4.8, 0.0));
        for px in [-6.0, -2.0, 2.0, 6.0] {
            trim_g.push(p(bx_(0.18, 3.2, 0.18), px, 1.6, 5.9, 0.0));
        }
        // Windows & door.
        for px in [-5.0, -2.6, 2.6, 5.0] {
            glass_g.push(p(plane_geometry(1.9, 1.5, 1.0, 1.0), px, 2.1, 3.52, 0.0));
        }
        glass_g.push(p(plane_geometry(1.2, 2.3, 1.0, 1.0), 0.0, 1.15, 3.52, 0.0));
        // Gas canopy with two pumps, between diner and road.
        let (cx0, cz0) = (9.0, 7.0);
        roof_g.push(p(bx_(7.0, 0.5, 5.0), cx0, 4.6, cz0, 0.0));
        trim_g.push(p(bx_(7.05, 0.3, 5.05), cx0, 4.3, cz0, 0.0));
        for (a, b) in [(-3.0, -2.0), (3.0, -2.0), (-3.0, 2.0), (3.0, 2.0)] {
            wall_g.push(p(bx_(0.25, 4.4, 0.25), cx0 + a, 2.2, cz0 + b, 0.0));
        }
        for a in [-1.5, 1.5] {
            wall_g.push(p(bx_(0.7, 1.6, 0.5), cx0 + a, 0.8, cz0, 0.0));
            trim_g.push(p(bx_(0.72, 0.35, 0.52), cx0 + a, 1.55, cz0, 0.0));
        }
        let lit = self.mat(
            Material::standard()
                .set("color", 0xffffff)
                .set("emissive", 0xfff2d8)
                .set("emissiveIntensity", 0.4),
        );
        self.graph
            .add_night(Some(lit), "emissiveIntensity", 0.4, 2.2);
        let mut lit_plane = plane_geometry(6.4, 4.4, 1.0, 1.0);
        lit_plane.rotate_x(PI / 2.0);
        let lit_g = vec![p(lit_plane, cx0, 4.34, cz0, 0.0)];
        for (geos, mat) in [
            (wall_g, siding),
            (roof_g, roof),
            (trim_g, trim),
            (glass_g, glass),
            (lit_g, lit),
        ] {
            self.add_merged(geos, mat, mat != glass && mat != lit, true);
        }

        // Neon roof sign + tall pole sign by the road.
        let neon_tex = canvas::canvas_tex(self.graph, &canvas::neon_canvas());
        let neon = self.mat(
            Material::standard()
                .set("map", neon_tex)
                .set("emissive", 0xffffff)
                .set("emissiveMap", neon_tex)
                .set("emissiveIntensity", 0.7)
                .set("roughness", 0.5)
                .set("side", DOUBLE_SIDE),
        );
        self.graph
            .add_night(Some(neon), "emissiveIntensity", 0.7, 2.6);
        let ns = p(plane_geometry(5.0, 1.25, 1.0, 1.0), 0.0, 5.9, 1.2, 0.0);
        let ns = self.geo(ns);
        let m = self.graph.mesh(ns, neon);
        self.add(m);
        let pole_tex = canvas::canvas_tex(self.graph, &canvas::pole_sign_canvas());
        let pole_mat = self.mat(
            Material::standard()
                .set("map", pole_tex)
                .set("emissive", 0xffffff)
                .set("emissiveMap", pole_tex)
                .set("emissiveIntensity", 0.12)
                .set("roughness", 0.6),
        );
        self.graph
            .add_night(Some(pole_mat), "emissiveIntensity", 0.12, 1.2);
        let wr = t.wall_r[t.idx(d.s + 22.0)] as f64;
        let sp = t.point_at(d.s + 22.0, d.side * (wr + 2.2));
        let face_yaw = kernel::atan2(-f.fx, -f.fz) + d.side * 0.5;
        let mut board = plane_geometry(2.6, 3.25, 1.0, 1.0);
        board.translate(0.0, 6.2, 0.0);
        let mut board_back = plane_geometry(2.6, 3.25, 1.0, 1.0);
        board_back.rotate_y(PI);
        board_back.translate(0.0, 6.2, -0.05);
        let bg = self.geo(placed(&board, sp.x, sp.y, sp.z, face_yaw));
        let bm = self.graph.mesh(bg, pole_mat);
        self.add(bm);
        let mut pg = cylinder_geometry(0.14, 0.18, 7.8, 8.0, 1.0, false, 0.0, PI * 2.0);
        pg.translate(0.0, 3.9 - 0.8, -0.12);
        let pole_m = self.mat(
            Material::standard()
                .set("color", 0x3a2a1e)
                .set("roughness", 0.8),
        );
        self.add_merged(
            vec![
                placed(&pg, sp.x, sp.y, sp.z, face_yaw),
                placed(&board_back, sp.x, sp.y, sp.z, face_yaw),
            ],
            pole_m,
            true,
            true,
        );

        // A parked pickup, if the vehicle module is present.
        let lp = Vector3::new(-8.5, 0.0, 6.5).apply_axis_angle(UP, yaw);
        self.parked_car(
            cars,
            "pickup",
            0x6b7f5a,
            12,
            Vector3::new(bx + lp.x, y + 0.05, bz + lp.z),
            yaw + 0.3,
        );
    }

    // ── Summit lookout ──────────────────────────────────────────────────

    fn build_lookout(&mut self, l: &Spot, cars: Option<ParkedCars>) {
        let t = self.t;
        let f = t.frame(l.s);
        let out = (f.rx * l.side, f.rz * l.side); // away from road
        let yaw = kernel::atan2(out.0, out.1);
        let y = l.y;
        let gravel = self.gravel(0xa39c90, 5.0);
        let mut g0 = circle_geometry(11.3, 28.0, 0.0, PI * 2.0);
        g0.rotate_x(-PI / 2.0);
        g0.translate(l.x, y + 0.06, l.z);
        let g0 = self.geo(g0);
        let pad = self.graph.mesh(g0, gravel);
        self.graph.get_mut(pad).receive_shadow = true;
        self.add(pad);

        // Low stone wall around the outer edge.
        let mut stones = Vec::new();
        let mut a = -1.25;
        while a <= 1.25 {
            let r = 10.7;
            let lx = kernel::sin(a) * r;
            let lz = kernel::cos(a) * r;
            let mut bxg = box_geometry(
                1.2,
                0.8 + (kernel::sin(a * 13.0).abs() * 0.15),
                0.7,
                1.0,
                1.0,
                1.0,
            );
            bxg.rotate_y(a);
            bxg.translate(lx, 0.4, lz);
            stones.push(placed(&bxg, l.x, y, l.z, yaw));
            a += 0.085;
        }
        let stone_mat = rock_material(self.graph, self.textures, false).set("color", 0xb8ad9c);
        let stone_mat = self.mat(stone_mat);
        self.add_merged(stones, stone_mat, true, true);

        // Coin-op viewer and a bench.
        let metal = self.mat(
            Material::standard()
                .set("color", 0x3d6b4f)
                .set("metalness", 0.6)
                .set("roughness", 0.4),
        );
        let wood = self.mat(
            Material::standard()
                .set("color", 0x6d5038)
                .set("roughness", 0.9),
        );
        let mut mg = Vec::new();
        let mut wg = Vec::new();
        let p = |mut geo: BufferGeometry, lx: f64, ly: f64, lz: f64| {
            geo.translate(lx, ly, lz);
            placed(&geo, l.x, y, l.z, yaw)
        };
        let cyl = |rt: f64, rb: f64, h: f64, n: f64| {
            cylinder_geometry(rt, rb, h, n, 1.0, false, 0.0, PI * 2.0)
        };
        let bx_ = |w: f64, h: f64, dd: f64| box_geometry(w, h, dd, 1.0, 1.0, 1.0);
        mg.push(p(cyl(0.08, 0.12, 1.1, 8.0), 2.0, 0.55, 9.6));
        mg.push(p(bx_(0.5, 0.35, 0.7), 2.0, 1.25, 9.6));
        let mut eye = cyl(0.09, 0.09, 0.4, 8.0);
        eye.rotate_x(PI / 2.0);
        mg.push(p(eye.clone(), 1.88, 1.3, 10.05));
        mg.push(p(eye, 2.12, 1.3, 10.05));
        wg.push(p(bx_(2.2, 0.08, 0.5), -3.0, 0.5, 8.4));
        wg.push(p(bx_(2.2, 0.5, 0.08), -3.0, 0.8, 8.65));
        for lx in [-3.9, -2.1] {
            wg.push(p(bx_(0.1, 0.5, 0.5), lx, 0.25, 8.4));
        }
        // Flagpole.
        mg.push(p(cyl(0.05, 0.07, 8.0, 6.0), 6.0, 4.0, 6.0));
        self.add_merged(mg, metal, true, true);
        self.add_merged(wg, wood, true, true);
        let flag_mat = self.mat(
            Material::standard()
                .set("color", 0xd6423a)
                .set("side", DOUBLE_SIDE)
                .set("roughness", 0.8),
        );
        let mut flag_geo = plane_geometry(1.8, 1.1, 8.0, 1.0);
        let fp = Vector3::new(6.0, 7.3, 6.0).apply_axis_angle(UP, yaw);
        let flag_pos = Vector3::new(l.x + fp.x, y + fp.y, l.z + fp.z);
        let pos = flag_geo.get_attribute_mut("position").expect("position");
        let base: Vec<f32> = pos.as_f32().expect("f32").to_vec();
        for k in 0..pos.count() {
            pos.set_x(k, base[k * 3] as f64 + 0.9);
        }
        let flag_geo = self.geo(flag_geo);
        let flag = self.graph.mesh(flag_geo, flag_mat);
        self.graph.get_mut(flag).position = flag_pos;
        self.animators
            .push(Box::new(flag_animator(flag, flag_geo, flag_pos, base, yaw)));
        self.add(flag);

        let lp = Vector3::new(-5.5, 0.0, 3.0).apply_axis_angle(UP, yaw);
        self.parked_car(
            cars,
            "sedan",
            0x2f4f7f,
            5,
            Vector3::new(l.x + lp.x, y + 0.05, l.z + lp.z),
            yaw + 0.2,
        );
    }

    // ── Canyon waterfall ────────────────────────────────────────────────

    fn build_waterfall(&mut self) -> Option<Waterfall> {
        let t = self.t;
        // Find the tallest rock wall between the last hairpin and the summit.
        let hp = t.tag("hairpin");
        let s0 = hp.last().map_or(1600.0, |h| h.s1 + 60.0);
        let s_end = t.tag("summit").first().map_or(2200.0, |s| s.s0) - 40.0;
        struct Best {
            s: f64,
            side: f64,
            rise: f64,
            wall: f64,
        }
        let mut best: Option<Best> = None;
        let mut s = s0;
        while s < s_end {
            let i = t.idx(s);
            for side in [-1.0, 1.0] {
                if self.side_kind(side, i) != 1 {
                    continue;
                }
                // Needs a straight-ish stretch so the sheet is visible ahead.
                if (t.k_smooth[i] as f64).abs() > 0.012 {
                    continue;
                }
                let f = t.frame(s);
                let wall = self.wall(side, i);
                let lat = side * (wall + 22.0);
                let h = self.surf.sample(f.x + f.rx * lat, f.z + f.rz * lat);
                let rise = h.h - f.y;
                if best.as_ref().is_none_or(|b| rise > b.rise) {
                    best = Some(Best {
                        s,
                        side,
                        rise,
                        wall,
                    });
                }
            }
            s += 6.0;
        }
        let best = best?;
        if best.rise < 12.0 {
            return None;
        }
        let f = t.frame(best.s);
        let (side, wall) = (best.side, best.wall);
        // Walk up the face collecting a spine of points hugging the terrain.
        struct P {
            x: f64,
            z: f64,
            y: f64,
        }
        let mut spine: Vec<P> = Vec::new();
        let mut l = wall + 1.2;
        while l < wall + 60.0 {
            let lat = side * l;
            let x = f.x + f.rx * lat;
            let z = f.z + f.rz * lat;
            let h = self.surf.sample(x, z);
            spine.push(P { x, z, y: h.h });
            if h.h - f.y > 40.0 || (spine.len() > 8 && h.slope < 0.35) {
                break;
            }
            l += 0.8;
        }
        if spine.len() < 6 {
            return None;
        }
        // A ribbon hugging the face, widening as it falls. Built three times:
        // a dark wet streak on the rock, the main sheet, and a faster, fainter
        // veil in front of it, so the fall has depth and visible motion.
        let ribbon = |off: f64, w0: f64, w1: f64, cols: usize| {
            let mut pos: Vec<f64> = Vec::new();
            let mut uv: Vec<f64> = Vec::new();
            let mut idx: Vec<u32> = Vec::new();
            let mut v = 0.0;
            let n = spine.len();
            for r in 0..n {
                let p = &spine[n - 1 - r]; // top first
                if r > 0 {
                    let q = &spine[n - r];
                    v += kernel::hypot3(p.x - q.x, p.y - q.y, p.z - q.z);
                }
                let frac = r as f64 / (n - 1) as f64;
                let half_w = lerp(w0, w1, kernel::pow(frac, 0.8))
                    * (1.0 + 0.12 * kernel::sin(r as f64 * 0.9));
                for c in 0..cols {
                    let u = c as f64 / (cols - 1) as f64;
                    let a = (u - 0.5) * 2.0 * half_w;
                    // Bulge the middle outward so the sheet reads as falling water.
                    let bulge = off + (1.0 - kernel::pow(2.0 * u - 1.0, 2.0)) * 0.25;
                    pos.extend([
                        p.x + f.fx * a - f.rx * side * bulge,
                        p.y + 0.35,
                        p.z + f.fz * a - f.rz * side * bulge,
                    ]);
                    uv.extend([u, v / 6.0]);
                }
            }
            for r in 0..n - 1 {
                for c in 0..cols - 1 {
                    let a = (r * cols + c) as u32;
                    let b = a + 1;
                    let cc = a + cols as u32;
                    let d = cc + 1;
                    idx.extend([a, cc, b, b, cc, d]);
                }
            }
            let mut geo = BufferGeometry::new();
            geo.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
            geo.set_attribute("uv", BufferAttribute::from_f64(&uv, 2));
            geo.set_index(&idx);
            geo.compute_vertex_normals();
            geo
        };
        let c = canvas::waterfall_canvas();
        let tex = canvas::plain_canvas_tex(self.graph, &c);
        canvas::set_wrap(self.graph, tex, canvas::CLAMP, canvas::REPEAT);
        let tex2 = self.graph.clone_texture(tex);
        canvas::set_repeat(self.graph, tex2, 1.0, 0.6);
        let wet_mat = self.mat(
            Material::basic()
                .set("color", 0x0d1418)
                .set("transparent", true)
                .set("opacity", 0.45)
                .set("depthWrite", false)
                .set("polygonOffset", true)
                .set("polygonOffsetFactor", -2.0)
                .set("polygonOffsetUnits", -2.0),
        );
        let wet_geo = self.geo(ribbon(0.12, 2.2, 5.2, 5));
        let wet = self.graph.mesh(wet_geo, wet_mat);
        self.graph.get_mut(wet).render_order = 1.0;
        self.add(wet);
        let sheet_mat = Material::lambert()
            .set("color", 0xe6f2ff)
            .set("map", tex)
            .set("alphaMap", tex)
            .set("transparent", true)
            .set("depthWrite", false)
            .set("side", DOUBLE_SIDE)
            .set("emissive", 0x6d8fb0)
            .set("emissiveIntensity", 0.35);
        // `mat.clone()` with the faster texture and a fainter opacity.
        let veil_mat = sheet_mat
            .clone()
            .set("map", tex2)
            .set("alphaMap", tex2)
            .set("opacity", 0.55);
        let sheet_mat = self.mat(sheet_mat);
        let sheet_geo = self.geo(ribbon(0.45, 1.3, 3.6, 7));
        let sheet = self.graph.mesh(sheet_geo, sheet_mat);
        self.graph.get_mut(sheet).render_order = 2.0;
        self.add(sheet);
        let veil_mat = self.mat(veil_mat);
        let veil_geo = self.geo(ribbon(0.85, 1.0, 4.2, 5));
        let veil = self.graph.mesh(veil_geo, veil_mat);
        self.graph.get_mut(veil).render_order = 3.0;
        self.add(veil);

        // Plunge pool with churning foam, spray, and boulders around the rim.
        let base = &spine[0];
        let mut pool_geo = circle_geometry(3.4, 24.0, 0.0, PI * 2.0);
        pool_geo.rotate_x(-PI / 2.0);
        let pool_mat = self.mat(
            Material::standard()
                .set("color", 0x2a4b5c)
                .set("roughness", 0.08)
                .set("metalness", 0.3),
        );
        let pool_geo = self.geo(pool_geo);
        let pool = self.graph.mesh(pool_geo, pool_mat);
        let pool_pos = Vector3::new(
            base.x + f.rx * side * 1.2,
            base.y + 0.12,
            base.z + f.rz * side * 1.2,
        );
        self.graph.get_mut(pool).position = pool_pos;
        self.add(pool);
        let foam_tex = canvas::plain_canvas_tex(self.graph, &canvas::foam_canvas());
        let mut foam_geo = circle_geometry(3.3, 24.0, 0.0, PI * 2.0);
        foam_geo.rotate_x(-PI / 2.0);
        let foam_mat = self.mat(
            Material::lambert()
                .set("color", 0xf2f8ff)
                .set("map", foam_tex)
                .set("transparent", true)
                .set("depthWrite", false)
                .set("emissive", 0x6d8fb0)
                .set("emissiveIntensity", 0.3),
        );
        let foam_geo = self.geo(foam_geo);
        let foam = self.graph.mesh(foam_geo, foam_mat);
        let foam_pos = Vector3::new(pool_pos.x, pool_pos.y + 0.04, pool_pos.z);
        {
            let o = self.graph.get_mut(foam);
            o.position = foam_pos;
            o.render_order = 2.0;
        }
        self.add(foam);
        let mut ring = Vec::new();
        for k in 0..14 {
            let a = (k as f64 / 14.0) * PI * 2.0;
            let r = 3.6 + (k % 3) as f64 * 0.4;
            let x = pool_pos.x + kernel::cos(a) * r;
            let z = pool_pos.z + kernel::sin(a) * r;
            if !self.clear_of_road(x, z, 0.6) {
                continue;
            }
            let h2 = self.surf.sample(x, z);
            let sz = 0.5 + ((k * 7) % 5) as f64 * 0.2;
            ring.push(Item {
                ry: a * 3.0,
                col: Some(0x8c8880),
                b: Some(0.8),
                ..Item::at(
                    x,
                    js::min(h2.h, pool_pos.y + 0.3) - sz * 0.25,
                    z,
                    sz * 1.3,
                    sz,
                    sz,
                )
            });
        }
        if !ring.is_empty() {
            let g = self.geo(rock_geometry(
                31,
                1.0,
                RockOpts {
                    lichen: 1.6,
                    ..RockOpts::default()
                },
            ));
            let m = self.rock_mat.expect("the rocks are built first");
            self.add_instanced(g, m, &ring, true, true);
        }
        let smoke = self
            .graph
            .cached_texture(&self.textures.smoke_texture(), Layer::Main, "");
        let smoke_mat = self.mat(
            Material::sprite()
                .set("map", smoke)
                .set("color", 0xe8f2ff)
                .set("transparent", true)
                .set("opacity", 0.32)
                .set("depthWrite", false),
        );
        let sprite_geo = self.geo(sprite_geometry());
        let mut sprays = Vec::new();
        for k in 0..5 {
            let sp = self.graph.drawable(NodeType::Sprite, sprite_geo, smoke_mat);
            let kf = k as f64;
            let p = Vector3::new(
                pool_pos.x + (kf - 2.0) * f.fx * 1.1,
                pool_pos.y + 0.8 + (k % 3) as f64 * 0.7,
                pool_pos.z + (kf - 2.0) * f.fz * 1.1,
            );
            let o = self.graph.get_mut(sp);
            o.position = p;
            o.scale = Vector3::splat(4.0 + kf);
            sprays.push((sp, p));
            self.add(sp);
        }
        self.animators.push(Box::new(waterfall_animator(
            tex, tex2, foam, foam_pos, pool_pos, sprays, smoke_mat,
        )));
        Some(Waterfall {
            s: best.s,
            side,
            height: best.rise,
        })
    }
}

/// three's shared `Sprite` geometry: a unit quad, interleaved position and
/// uv (the export writes them apart), indexed.
pub fn sprite_geometry() -> BufferGeometry {
    let mut g = BufferGeometry::new();
    g.set_index(&[0, 1, 2, 0, 2, 3]);
    g.set_attribute(
        "position",
        BufferAttribute::from_f64(
            &[
                -0.5, -0.5, 0.0, 0.5, -0.5, 0.0, 0.5, 0.5, 0.0, -0.5, 0.5, 0.0,
            ],
            3,
        ),
    );
    g.set_attribute(
        "uv",
        BufferAttribute::from_f64(&[0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0], 2),
    );
    g
}

/// The flag's updater: it waves and swings in the wind within 600 m of
/// the camera.
fn flag_animator(
    flag: NodeId,
    geo: GeoId,
    position: Vector3,
    base: Vec<f32>,
    yaw: f64,
) -> impl Animator {
    let mut ft = 0.0;
    move |u: &UpdateCtx, out: &mut Vec<Edit>| {
        if let Some(c) = &u.camera {
            let cp = Vector3::new(c.position[0], c.position[1], c.position[2]);
            if cp.distance_to_squared(position) > 600.0 * 600.0 {
                return;
            }
        }
        ft += u.dt;
        let count = base.len() / 3;
        let mut values = Vec::with_capacity(base.len());
        for k in 0..count {
            let x = base[k * 3] as f64 + 0.9;
            // The x the geometry holds (set at build) and y as built.
            values.push((base[k * 3] as f64 + 0.9) as f32);
            values.push(base[k * 3 + 1]);
            values.push((kernel::sin(ft * 6.0 - x * 3.0) * 0.12 * x) as f32);
        }
        out.push(Edit {
            target: Handle::Geometry(geo),
            change: Change::Attribute {
                name: "position",
                offset: 0,
                values,
            },
        });
        let q = Quaternion::from_euler(&Euler::new(
            0.0,
            yaw + 1.2 + kernel::sin(ft * 0.7) * 0.2,
            0.0,
        ));
        out.push(Edit {
            target: Handle::Node(flag),
            change: Change::Transform {
                position: [position.x, position.y, position.z],
                quaternion: [q.x, q.y, q.z, q.w],
                scale: [1.0, 1.0, 1.0],
            },
        });
    }
}

/// The waterfall's updater: the streaks scroll down the sheet and the
/// veil; within 800 m the foam turns and the spray billows.
#[allow(clippy::too_many_arguments)]
fn waterfall_animator(
    tex: TextureId,
    tex2: TextureId,
    foam: NodeId,
    foam_pos: Vector3,
    pool_pos: Vector3,
    sprays: Vec<(NodeId, Vector3)>,
    smoke_mat: MaterialId,
) -> impl Animator {
    let mut wt = 0.0;
    move |u: &UpdateCtx, out: &mut Vec<Edit>| {
        wt += u.dt;
        out.push(Edit {
            target: Handle::Texture(tex),
            change: Change::TextureOffset([0.0, -wt * 0.9]),
        });
        out.push(Edit {
            target: Handle::Texture(tex2),
            change: Change::TextureOffset([0.0, -wt * 1.5]),
        });
        if let Some(c) = &u.camera {
            let cp = Vector3::new(c.position[0], c.position[1], c.position[2]);
            if cp.distance_to_squared(pool_pos) > 800.0 * 800.0 {
                return;
            }
        }
        let q = Quaternion::from_euler(&Euler::new(0.0, wt * 0.35, 0.0));
        out.push(Edit {
            target: Handle::Node(foam),
            change: Change::Transform {
                position: [foam_pos.x, foam_pos.y, foam_pos.z],
                quaternion: [q.x, q.y, q.z, q.w],
                scale: [1.0, 1.0, 1.0],
            },
        });
        for (k, &(sp, p)) in sprays.iter().enumerate() {
            let a = wt * 0.9 + k as f64 * 2.1;
            let s = 3.5 + k as f64 * 0.8 + kernel::sin(a) * 0.9;
            out.push(Edit {
                target: Handle::Node(sp),
                change: Change::Transform {
                    position: [p.x, p.y, p.z],
                    quaternion: [0.0, 0.0, 0.0, 1.0],
                    scale: [s, s, s],
                },
            });
            out.push(Edit {
                target: Handle::Material(smoke_mat),
                change: Change::Number {
                    prop: "rotation",
                    value: a * 0.1,
                },
            });
        }
    }
}
