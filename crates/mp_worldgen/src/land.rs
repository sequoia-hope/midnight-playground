//! The Land kit: the open world's planting by land cover
//! (docs/vision/peninsula.md, stage 4a; DECISIONS D1212). Not a port: the
//! JS game has no open world.
//!
//! An open level carries the real land cover of its box (`Level::cover`,
//! the atlas's planning grid: forest, shrub, grass, crop, urban, water...).
//! Within reach of the roads (the terrain's far field, which knows every
//! road of the network) the kit walks the land in cells and plants what
//! the cover says: conifers where it is forest (firs and spruce on the
//! verge, cheaper spruce behind, far quads beyond, as the Sierra's forest
//! is built), oaks and scrub where it is shrub, a scattering of oaks and
//! scrub on grass, nothing on crop, urban, barren, sand or water, nothing
//! on the road's clearance, under the sea or on the steepest ground.
//!
//! Every cell is seeded from the kit's seed and its own coordinates, so a
//! cell's planting is a function of the cell alone (what streaming, stage
//! 4b, builds and drops a cell at a time), and each cell's instances of a
//! template are one `InstancedMesh`.
//!
//! On crop (stage 4d) the land is fields (the terrain paints the parcels,
//! `terrain_mesh`), and the kit fences them: a post-and-rail line along
//! the road's edge wherever the verge is crop, and field boundaries
//! running out from the road every [`FIELD`] metres or so, each post and
//! rail belonging to the cell its centre is in.

use mp_math::{Mulberry32, fbm, js, kernel, lerp, smoothstep};
use mp_scene::three;
use mp_track::Cover;

use crate::flora::{canopy_geometry, conifer_geometry, foliage_material, shrub_geometry};
use crate::material::{Material, Param};
use crate::mountain::kit::{Item, SurfaceSampler, instanced};
use crate::object::{GeoId, MaterialId, NodeId, SceneGraph};
use crate::terrain::Terrain;
use crate::three_geom::{BufferGeometry, cylinder_geometry, merge_geometries};
use crate::world::{Scenery, SceneryInfo, World};

/// Metres of land a cell is on a side: one draw call per template a cell.
pub const CELL: f64 = 512.0;
/// Candidates this far apart (m) on the cell's walk.
const SPACING: f64 = 7.0;
/// Nothing is planted farther than this from the nearest road (m): past
/// the ridge a tree is never seen from the car.
pub const REACH: f64 = 450.0;
/// The forest's bands from the road (m): detailed trees on the verge,
/// cheaper ones behind, far quads beyond.
const VERGE: f64 = 45.0;
const MID: f64 = 180.0;
/// Nothing within this of the road's edge (m), and trees farther still.
const CLEAR: f64 = 2.0;
const CLEAR_TREE: f64 = 4.5;
/// The kit's seed; a cell's is mixed from it and the cell's coordinates.
const SEED: u32 = 4042;
/// Fence posts this far apart (m), and the rail between two.
const POST: f64 = 4.0;
/// A field boundary fence leaves the road about every this many metres.
const FIELD: f64 = 190.0;
/// How far from the road a field boundary runs at most (m).
const FIELD_DEPTH: f64 = 240.0;
/// The road-edge fence stands this far outside the road's margin (m).
const FENCE_OFF: f64 = 2.2;
/// The JS kits write a random yaw as `6.3`, not 2π.
const TURN: f64 = 6.3;
const DOUBLE_SIDE: f64 = three::DOUBLE_SIDE as f64;
const FRONT_SIDE: f64 = three::FRONT_SIDE as f64;

/// Instance tints over the vertex colours: the conifers' and the oaks'
/// greens, the scrub's dusty olives.
const CONIFER_COLS: [u32; 6] = [0xffffff, 0xe8f0e4, 0xd4e0d2, 0xf2ecd8, 0xc6d4c0, 0xe0e8f0];
// The canopy's vertex colours are shading only (the valley tints its
// shade trees the same way): the greens are the instance's.
const OAK_COLS: [u32; 5] = [0x40592a, 0x4a6a2e, 0x3a5224, 0x5a6e30, 0x475f2b];
const SCRUB_COLS: [u32; 7] = [
    0x5f6e4c, 0x6b7458, 0x4d5a36, 0x6e5a3c, 0x55663e, 0x7a8068, 0x4a5638,
];

/// What a cell plants, per template.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Template {
    Fir,
    Spruce,
    SpruceMid,
    SpruceFar,
    Oak,
    OakFar,
    Scrub,
    Post,
    Rail,
}

const TEMPLATES: [Template; 9] = [
    Template::Fir,
    Template::Spruce,
    Template::SpruceMid,
    Template::SpruceFar,
    Template::Oak,
    Template::OakFar,
    Template::Scrub,
    Template::Post,
    Template::Rail,
];

/// What the kit planted, for the log and the tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub cells: usize,
    pub conifers: usize,
    pub far_conifers: usize,
    pub oaks: usize,
    pub scrub: usize,
    /// Fence posts, and the rails between them.
    pub posts: usize,
    pub rails: usize,
    pub meshes: usize,
}

pub struct Land {
    #[allow(dead_code)]
    zone: usize,
    label: &'static str,
    pub counts: Counts,
}

impl Land {
    pub fn new(info: &SceneryInfo) -> Land {
        Land {
            zone: info.zone,
            label: "Planting the land",
            counts: Counts::default(),
        }
    }
}

/// An oak: a short trunk under a lobed canopy, one template (the valley's
/// shade trees are two instanced meshes; here a tree is one instance).
fn oak_geometry(seed: u32, lod: usize) -> BufferGeometry {
    let mut trunk = cylinder_geometry(
        0.09,
        0.14,
        1.1,
        6.0,
        1.0,
        false,
        0.0,
        core::f64::consts::TAU,
    );
    trunk.translate(0.0, 0.55, 0.0);
    if trunk.has_attribute("uv") {
        trunk.delete_attribute("uv");
    }
    let trunk = crate::coast::kit::colorize(trunk.to_non_indexed(), 0x4a3a2a);
    let mut crown = canopy_geometry("shade", seed, lod);
    crown.scale(1.25, 1.0, 1.25);
    crown.translate(0.0, 1.55, 0.0);
    if crown.has_attribute("uv") {
        crown.delete_attribute("uv");
    }
    let crown = if crown.index.is_some() {
        crown.to_non_indexed()
    } else {
        crown
    };
    merge_geometries(&[&trunk, &crown], false).expect("trunk and crown share their attributes")
}

/// A fence post: a weathered square post, its foot at the origin.
fn post_geometry() -> BufferGeometry {
    let mut g = crate::three_geom::box_geometry(0.13, 1.25, 0.13, 1.0, 1.0, 1.0);
    g.translate(0.0, 0.62, 0.0);
    crate::coast::kit::colorize(g.to_non_indexed(), 0x6e5a44)
}

/// Two rails between posts, along +Z, [`POST`] long.
fn rail_geometry() -> BufferGeometry {
    let mut a = crate::three_geom::box_geometry(0.05, 0.07, POST, 1.0, 1.0, 1.0);
    a.translate(0.0, 1.05, 0.0);
    let mut b = crate::three_geom::box_geometry(0.05, 0.07, POST, 1.0, 1.0, 1.0);
    b.translate(0.0, 0.6, 0.0);
    let a = crate::coast::kit::colorize(a.to_non_indexed(), 0x7a6650);
    let b = crate::coast::kit::colorize(b.to_non_indexed(), 0x7a6650);
    merge_geometries(&[&a, &b], false).expect("two boxes share their attributes")
}

/// A fence line's posts and rails that fall in the cell `[x0, x0 + CELL)
/// × [z0, z0 + CELL)`: the line is walked from `from` along `dir` (unit)
/// in steps of [`POST`] while `keep` allows, a post at each step and a
/// rail between two; each goes in by its own centre, so the line is the
/// same whichever cell walks it.
#[allow(clippy::too_many_arguments)]
fn fence_line(
    from: [f64; 2],
    dir: [f64; 2],
    max_len: f64,
    x0: f64,
    z0: f64,
    surf: &mut SurfaceSampler,
    keep: &mut dyn FnMut(f64, f64) -> bool,
    posts: &mut Vec<Item>,
    rails: &mut Vec<Item>,
) {
    let inside = |x: f64, z: f64| x >= x0 && x < x0 + CELL && z >= z0 && z < z0 + CELL;
    let near = |x: f64, z: f64| {
        x >= x0 - POST && x < x0 + CELL + POST && z >= z0 - POST && z < z0 + CELL + POST
    };
    let ry = kernel::atan2(dir[0], dir[1]);
    let mut prev: Option<[f64; 3]> = None;
    let mut d = 0.0;
    while d <= max_len {
        let (x, z) = (from[0] + dir[0] * d, from[1] + dir[1] * d);
        d += POST;
        if !keep(x, z) {
            prev = None;
            continue;
        }
        if !near(x, z) {
            prev = None;
            continue;
        }
        let sv = surf.sample(x, z);
        if sv.slope > 1.2 {
            prev = None;
            continue;
        }
        let here = [x, sv.h, z];
        if inside(x, z) {
            posts.push(Item {
                ry,
                ..Item::at(x, sv.h - 0.05, z, 1.0, 1.0, 1.0)
            });
        }
        if let Some(p) = prev {
            let (mx, mz) = ((p[0] + x) / 2.0, (p[2] + z) / 2.0);
            if inside(mx, mz) {
                let my = (p[1] + sv.h) / 2.0;
                let rx = kernel::atan2(p[1] - sv.h, POST);
                rails.push(Item {
                    ry,
                    rx,
                    ..Item::at(mx, my - 0.05, mz, 1.0, 1.0, 1.0)
                });
            }
        }
        prev = Some(here);
    }
}

/// A cell's seed: the kit's mixed with the cell's coordinates (a plain
/// integer hash, so a cell plants the same wherever the walk starts).
fn cell_seed(ci: i64, cj: i64) -> u32 {
    let mut h =
        SEED ^ (ci as u32).wrapping_mul(0x9E37_79B9) ^ (cj as u32).wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h
}

/// How likely a candidate is planted, by cover and distance from the
/// road, and with what.
fn choose(cover: Cover, d: f64, clump: f64, r: f64) -> Option<Template> {
    match cover {
        Cover::Forest => {
            // Per km of road: about 250 verge trees, 600 behind, 900 far
            // quads, at 7 m candidates; the clumps thin the far edge.
            let p = if d < VERGE {
                0.14
            } else if d < MID {
                0.11
            } else {
                0.085 * (0.6 + 0.8 * clump)
            };
            if r >= p {
                return None;
            }
            Some(if d < VERGE {
                if (r / p) < 0.5 {
                    Template::Fir
                } else {
                    Template::Spruce
                }
            } else if d < MID {
                Template::SpruceMid
            } else {
                Template::SpruceFar
            })
        }
        Cover::Shrub => {
            if d > MID + 60.0 {
                return None;
            }
            let oak = 0.014 * (0.5 + clump);
            let scrub = 0.10;
            if r < oak {
                Some(if d < MID {
                    Template::Oak
                } else {
                    Template::OakFar
                })
            } else if r < oak + scrub {
                Some(Template::Scrub)
            } else {
                None
            }
        }
        Cover::Grass | Cover::Wetland => {
            if d > MID {
                return None;
            }
            let oak = if cover == Cover::Grass { 0.006 } else { 0.0 };
            let scrub = if cover == Cover::Grass { 0.03 } else { 0.05 };
            if r < oak {
                Some(Template::Oak)
            } else if r < oak + scrub {
                Some(Template::Scrub)
            } else {
                None
            }
        }
        Cover::Crop | Cover::Urban | Cover::Barren | Cover::Sand | Cover::Sea | Cover::Water => {
            None
        }
    }
}

impl Scenery for Land {
    fn name(&self) -> &str {
        "Land"
    }

    fn label(&self) -> Option<&str> {
        Some(self.label)
    }

    fn build(&mut self, world: &mut World) -> Result<(), String> {
        // A section (the menu's views, 4b's streaming) plants only the
        // cells within its reach.
        let ci0 = (world.terrain.as_ref().map_or(0.0, |t| t.min_x) / CELL).floor() as i64;
        let ci1 = (world.terrain.as_ref().map_or(0.0, |t| t.max_x) / CELL).ceil() as i64;
        let cj0 = (world.terrain.as_ref().map_or(0.0, |t| t.min_z) / CELL).floor() as i64;
        let cj1 = (world.terrain.as_ref().map_or(0.0, |t| t.max_z) / CELL).ceil() as i64;
        let mut wanted: Vec<(i64, i64)> = Vec::new();
        for cj in cj0..cj1 {
            for ci in ci0..ci1 {
                let (x0, z0) = (ci as f64 * CELL, cj as f64 * CELL);
                if crate::section::wants_box(world, x0, z0, x0 + CELL, z0 + CELL) {
                    wanted.push((ci, cj));
                }
            }
        }
        let World {
            level,
            track,
            terrain,
            graph,
            root,
            ..
        } = world;
        let cover = level.cover.as_ref().ok_or("the level has no land cover")?;
        let t = track.as_ref().ok_or("the route is surveyed first")?;
        let terrain: &Terrain = terrain.as_ref().ok_or("no terrain")?;
        let sea_y = terrain.sea_y.unwrap_or(f64::NEG_INFINITY);
        let n2 = &terrain.noise2;
        let group = graph.group("land");
        let mut surf = SurfaceSampler::new(terrain);

        // The templates and their materials, made once.
        let foliage = graph.add_material(foliage_material(&[]));
        let far_mat = graph.add_material(
            Material::lambert()
                .set("vertexColors", true)
                .set("side", DOUBLE_SIDE),
        );
        let scrub_mat = graph.add_material(foliage_material(&[("side", Param::Num(FRONT_SIDE))]));
        let wood_mat = graph.add_material(
            Material::lambert()
                .set("vertexColors", true)
                .set("side", FRONT_SIDE),
        );
        let geos: Vec<(Template, GeoId, MaterialId, bool)> = vec![
            (
                Template::Fir,
                graph.add_geometry(conifer_geometry("fir", 0, 31)),
                foliage,
                true,
            ),
            (
                Template::Spruce,
                graph.add_geometry(conifer_geometry("spruce", 0, 32)),
                foliage,
                true,
            ),
            (
                Template::SpruceMid,
                graph.add_geometry(conifer_geometry("spruce", 1, 33)),
                foliage,
                true,
            ),
            (
                Template::SpruceFar,
                graph.add_geometry(conifer_geometry("spruce", 2, 34)),
                far_mat,
                false,
            ),
            (
                Template::Oak,
                graph.add_geometry(oak_geometry(35, 0)),
                foliage,
                true,
            ),
            (
                Template::OakFar,
                graph.add_geometry(oak_geometry(35, 1)),
                foliage,
                false,
            ),
            (
                Template::Scrub,
                graph.add_geometry(shrub_geometry(36, 1.0)),
                scrub_mat,
                false,
            ),
            (
                Template::Post,
                graph.add_geometry(post_geometry()),
                wood_mat,
                true,
            ),
            (
                Template::Rail,
                graph.add_geometry(rail_geometry()),
                wood_mat,
                false,
            ),
        ];

        // The cells over the terrain's box, on a grid fixed in the world
        // (not the box), so a cell is the same cell on every level.
        let diag = CELL * core::f64::consts::SQRT_2 / 2.0;
        let pick =
            |rng: &mut Mulberry32, a: &[u32]| a[(rng.next_f64() * a.len() as f64).floor() as usize];
        let mut counts = Counts::default();
        {
            for &(ci, cj) in &wanted {
                let x0 = ci as f64 * CELL;
                let z0 = cj as f64 * CELL;
                // A cell no road comes near plants nothing: the far field
                // at its centre bounds the distance anywhere in it.
                if terrain.far(x0 + CELL / 2.0, z0 + CELL / 2.0).d > REACH + diag {
                    continue;
                }
                let mut rng = Mulberry32::new(cell_seed(ci, cj));
                let mut items: Vec<Vec<Item>> = vec![Vec::new(); TEMPLATES.len()];
                let mut x = x0 + SPACING / 2.0;
                while x < x0 + CELL {
                    let mut z = z0 + SPACING / 2.0;
                    while z < z0 + CELL {
                        let px = x + (rng.next_f64() - 0.5) * SPACING * 0.9;
                        let pz = z + (rng.next_f64() - 0.5) * SPACING * 0.9;
                        let r = rng.next_f64();
                        z += SPACING;
                        let f = terrain.far(px, pz);
                        if f.d > REACH {
                            continue;
                        }
                        let c = Cover::of(cover(px, pz));
                        let clump = smoothstep(-0.2, 0.4, fbm(n2, px / 260.0 + 1.7, pz / 260.0, 3));
                        let Some(tpl) = choose(c, f.d, clump, r) else {
                            continue;
                        };
                        let tree = !matches!(tpl, Template::Scrub);
                        // Clear of every road: the far field's distance is
                        // to the nearest road's centreline.
                        let road_w = js::max(at(&t.hw, t.idx(f.s)), 5.0) + 1.5;
                        if f.d < road_w + if tree { CLEAR_TREE } else { CLEAR } {
                            continue;
                        }
                        let sv = surf.sample(px, pz);
                        if sv.h < sea_y + 1.5 || sv.slope > if tree { 1.1 } else { 1.4 } {
                            continue;
                        }
                        let steep = smoothstep(0.35, 0.8, sv.slope);
                        let item = match tpl {
                            Template::Fir
                            | Template::Spruce
                            | Template::SpruceMid
                            | Template::SpruceFar => {
                                // Redwood country: tall. The far quads a
                                // little bigger still, to read at distance.
                                let sc = lerp(1.0, 2.2, kernel::pow(rng.next_f64(), 1.2))
                                    * if tpl == Template::SpruceFar {
                                        1.15
                                    } else {
                                        1.0
                                    };
                                let sx = sc * lerp(0.85, 1.15, rng.next_f64());
                                let sy = sc * lerp(0.95, 1.35, rng.next_f64());
                                Item {
                                    ry: rng.next_f64() * TURN,
                                    rx: (rng.next_f64() - 0.5) * 0.05,
                                    rz: (rng.next_f64() - 0.5) * 0.05,
                                    col: Some(pick(&mut rng, &CONIFER_COLS)),
                                    b: Some(lerp(0.7, 1.05, rng.next_f64())),
                                    ..Item::at(px, sv.h - 0.4, pz, sx, sy, sx)
                                }
                            }
                            Template::Oak | Template::OakFar => {
                                let sc = lerp(2.2, 4.2, kernel::pow(rng.next_f64(), 1.3));
                                let sx = sc * lerp(0.85, 1.2, rng.next_f64());
                                let sy = sc * lerp(0.8, 1.1, rng.next_f64());
                                Item {
                                    ry: rng.next_f64() * TURN,
                                    col: Some(pick(&mut rng, &OAK_COLS)),
                                    b: Some(lerp(0.75, 1.05, rng.next_f64())),
                                    ..Item::at(px, sv.h - 0.25, pz, sx, sy, sx)
                                }
                            }
                            // The fences are not planted by the walk.
                            Template::Post | Template::Rail => continue,
                            Template::Scrub => {
                                let sc = lerp(0.6, 1.9, kernel::pow(rng.next_f64(), 1.3));
                                let sx = sc * lerp(0.8, 1.3, rng.next_f64());
                                let sy = sc * lerp(lerp(0.6, 1.0, rng.next_f64()), 1.25, steep);
                                Item {
                                    ry: rng.next_f64() * TURN,
                                    col: Some(pick(&mut rng, &SCRUB_COLS)),
                                    b: Some(lerp(0.8, 1.05, rng.next_f64())),
                                    ..Item::at(px, sv.h - 0.12 - steep * sc * 0.45, pz, sx, sy, sc)
                                }
                            }
                        };
                        let k = TEMPLATES
                            .iter()
                            .position(|&p| p == tpl)
                            .expect("a template");
                        items[k].push(item);
                    }
                    x += SPACING;
                }
                // The fences of the crop land: along every road's edge
                // where the verge is crop, and field boundaries out from
                // the road.
                {
                    let kp = TEMPLATES
                        .iter()
                        .position(|&p| p == Template::Post)
                        .expect("a template");
                    let kr = kp + 1;
                    let (mut posts, mut rails) = (Vec::new(), Vec::new());
                    let crop_at = |x: f64, z: f64| Cover::of(cover(x, z)) == Cover::Crop;
                    for (ri, road) in t.roads().enumerate() {
                        let mut s = 0.0;
                        while s < road.length {
                            let f = road.frame(s);
                            // Only the road near the cell matters.
                            let pad = FIELD_DEPTH + 40.0;
                            if f.x < x0 - pad
                                || f.x > x0 + CELL + pad
                                || f.z < z0 - pad
                                || f.z > z0 + CELL + pad
                            {
                                s += POST * 8.0;
                                continue;
                            }
                            let off = f.hw + f.margin + FENCE_OFF;
                            for side in [-1.0, 1.0] {
                                let (x, z) = (f.x + f.rx * off * side, f.z + f.rz * off * side);
                                let inside = x >= x0 && x < x0 + CELL && z >= z0 && z < z0 + CELL;
                                let crop = crop_at(x, z) && terrain.far(x, z).d >= off - 0.5;
                                if !crop {
                                    continue;
                                }
                                let sv = surf.sample(x, z);
                                if sv.h < sea_y + 1.0 || sv.slope > 1.2 {
                                    continue;
                                }
                                let ry = kernel::atan2(f.fx, f.fz);
                                if inside {
                                    posts.push(Item {
                                        ry,
                                        ..Item::at(x, sv.h - 0.05, z, 1.0, 1.0, 1.0)
                                    });
                                }
                                // The rail back to the post before.
                                let fp = road.frame((s - POST).max(0.0));
                                let (px, pz) =
                                    (fp.x + fp.rx * off * side, fp.z + fp.rz * off * side);
                                let (mx, mz) = ((px + x) / 2.0, (pz + z) / 2.0);
                                if s >= POST
                                    && mx >= x0
                                    && mx < x0 + CELL
                                    && mz >= z0
                                    && mz < z0 + CELL
                                    && crop_at(px, pz)
                                {
                                    let ph = surf.sample(px, pz);
                                    if ph.slope <= 1.2 {
                                        rails.push(Item {
                                            ry,
                                            rx: kernel::atan2(ph.h - sv.h, POST),
                                            ..Item::at(
                                                mx,
                                                (ph.h + sv.h) / 2.0 - 0.05,
                                                mz,
                                                1.0,
                                                1.0,
                                                1.0,
                                            )
                                        });
                                    }
                                }
                                // A field boundary out from the road, every
                                // FIELD or so (where in each window is the
                                // window's own draw), while the land is crop.
                                let win = (s / FIELD).floor();
                                let at = win * FIELD
                                    + mp_math::hash2(win, ri as f64 + side, 11.0)
                                        * (FIELD - POST * 2.0)
                                    + POST;
                                if s < at || s - POST >= at {
                                    continue;
                                }
                                let dir = [f.rx * side, f.rz * side];
                                let from = [x + dir[0] * POST, z + dir[1] * POST];
                                let mut keep = |qx: f64, qz: f64| {
                                    crop_at(qx, qz)
                                        && terrain.far(qx, qz).d >= off - 0.5
                                        && terrain.height_at(qx, qz) >= sea_y + 1.0
                                };
                                fence_line(
                                    from,
                                    dir,
                                    FIELD_DEPTH,
                                    x0,
                                    z0,
                                    &mut surf,
                                    &mut keep,
                                    &mut posts,
                                    &mut rails,
                                );
                            }
                            s += POST;
                        }
                    }
                    counts.posts += posts.len();
                    counts.rails += rails.len();
                    items[kp].extend(posts);
                    items[kr].extend(rails);
                }
                let mut planted = false;
                for (k, list) in items.iter().enumerate() {
                    if list.is_empty() {
                        continue;
                    }
                    let (tpl, geo, mat, cast) = geos[k];
                    let n = instanced(graph, geo, mat, list, cast, true);
                    graph.add(group, n);
                    planted = true;
                    counts.meshes += 1;
                    match tpl {
                        Template::SpruceFar => counts.far_conifers += list.len(),
                        Template::Fir | Template::Spruce | Template::SpruceMid => {
                            counts.conifers += list.len()
                        }
                        Template::Oak | Template::OakFar => counts.oaks += list.len(),
                        Template::Scrub => counts.scrub += list.len(),
                        Template::Post | Template::Rail => {}
                    }
                }
                if planted {
                    counts.cells += 1;
                }
            }
        }
        graph.add(*root, group);
        graph.log.push(format!(
            "Land: {} cells, {} conifers ({} far), {} oaks, {} scrub, {} fence posts, {} meshes",
            counts.cells,
            counts.conifers,
            counts.far_conifers,
            counts.oaks,
            counts.scrub,
            counts.posts,
            counts.meshes
        ));
        self.counts = counts;
        Ok(())
    }
}

fn at(a: &[f32], i: usize) -> f64 {
    a.get(i).map_or(f64::NAN, |&v| f64::from(v))
}

/// The group's node, for a test that wants to find it.
pub fn group_of(graph: &SceneGraph, root: NodeId) -> Option<NodeId> {
    graph
        .get(root)
        .children
        .iter()
        .copied()
        .find(|&c| graph.get(c).name == "land")
}
