//! The menu views' stand-ins (DECISIONS D748): a light hint of each level's
//! scenery along the attract camera's stretch, in place of its scenery
//! modules. A view is the level's land, road, sky, time of day and sea
//! (its world build with no module built); what makes each level
//! recognisable from the attract camera is added here, cheaply:
//!
//! - Sierra: spruce and fir on the slopes, a few boulders.
//! - Coast Highway: telegraph poles on the inland side, scrub.
//! - Downtown Streets: blocks of lit windows both sides, street lamps.
//! - Desert Run: red hoodoos and rocks, scrub.
//! - Seaside Raceway: the red, white and blue barrier, the catch-fence
//!   posts, oaks on the hills.
//! - Night City Cruise: a skyline of lit towers, freeway lamps.
//!
//! Not the JS game's scenery: invented stand-ins, built from `mp_worldgen`'s
//! flora templates and boxes, deterministic (a fixed seed per level). They
//! go under one group, `menu view`, at the end of the view's build.

use mp_math::Mulberry32;
use mp_track::{Frame, Track};
use mp_worldgen::flora::{
    RockOpts, canopy_geometry, conifer_geometry, foliage_material, rock_geometry, shrub_geometry,
};
use mp_worldgen::material::Material;
use mp_worldgen::mountain::kit::{Item, instanced};
use mp_worldgen::object::{GeoId, Image, MaterialId, NodeId};
use mp_worldgen::section::Section;
use mp_worldgen::terrain::Terrain;
use mp_worldgen::textures::Texture;
use mp_worldgen::three_geom::{BufferGeometry, box_geometry, cylinder_geometry, merge_geometries};
use mp_worldgen::world::{Job, World};
use std::f64::consts::TAU;
use std::sync::Arc;

/// The job that adds the stand-ins, run after the view's build.
pub fn job() -> Job {
    Job::serial("Menu view", 1.0, |w| {
        add(w);
        Ok(Vec::new())
    })
}

/// Where along the stretch, and how to reach the ground there.
struct Ctx<'a> {
    track: &'a Track,
    terrain: &'a Terrain,
    s0: f64,
    s1: f64,
    rng: Mulberry32,
}

impl Ctx<'_> {
    fn frame(&self, s: f64) -> Frame {
        let t = self.track;
        t.frame(if t.is_loop { t.wrap(s) } else { s })
    }

    fn r(&mut self) -> f64 {
        self.rng.next_f64()
    }

    /// A point `off` metres beyond the road's edge on `side` (+1 right,
    /// -1 left), on the ground.
    fn beside(&self, f: &Frame, side: f64, off: f64) -> [f64; 3] {
        let d = side * (f.hw + off);
        let (x, z) = (f.x + f.rx * d, f.z + f.rz * d);
        [x, self.terrain.height_at(x, z), z]
    }

    /// Stations every `step` metres along the stretch.
    fn stations(&self, step: f64) -> Vec<f64> {
        let n = ((self.s1 - self.s0) / step).floor() as usize;
        (0..=n).map(|k| self.s0 + k as f64 * step).collect()
    }

    /// Whether a point's ground is near the road's level there (not up a
    /// canyon wall or down a cliff).
    fn level_with(&self, f: &Frame, p: &[f64; 3], tol: f64) -> bool {
        (p[1] - f.y).abs() < tol
    }

    /// The side whose ground rises (the inland side of a coast road).
    fn uphill(&self) -> f64 {
        let (mut l, mut r) = (0.0, 0.0);
        for s in self.stations(50.0) {
            let f = self.frame(s);
            l += self.beside(&f, -1.0, 25.0)[1];
            r += self.beside(&f, 1.0, 25.0)[1];
        }
        if r >= l { 1.0 } else { -1.0 }
    }
}

/// The yaw that turns a box's x axis along the road (three's `rotateY`).
fn yaw(f: &Frame) -> f64 {
    (-f.fz).atan2(f.fx)
}

fn add(w: &mut World) {
    let id = w.level.id;
    let (Some(sec), Some(track), Some(terrain)) = (&w.section, &w.track, &w.terrain) else {
        return;
    };
    let Section { s0, s1, .. } = *sec;
    let seed = match id {
        "sierra" => 101,
        "coast" => 202,
        "streets" => 303,
        "desert" => 404,
        "seaside" => 505,
        _ => 606,
    };
    let mut c = Ctx {
        track,
        terrain,
        s0,
        s1,
        rng: Mulberry32::new(seed),
    };
    let mut out = Out::default();
    match id {
        "sierra" => sierra(&mut c, &mut out),
        "coast" => coast(&mut c, &mut out),
        "streets" => streets(&mut c, &mut out),
        "desert" => desert(&mut c, &mut out),
        "seaside" => seaside(&mut c, &mut out),
        "cruise" => cruise(&mut c, &mut out),
        _ => {}
    }
    out.build(w);
}

/// What a level adds: instanced templates and merged boxes.
#[derive(Default)]
struct Out {
    /// (template, material, instances, casts a shadow).
    sets: Vec<(Template, Mat, Vec<Item>, bool)>,
    /// Building boxes: (x, y, z, yaw, width, height, depth, tone).
    blocks: Vec<[f64; 8]>,
    /// Lamp heads (glowing boxes) and their posts.
    lamps: Vec<[f64; 5]>,
}

#[derive(Clone, Copy, PartialEq)]
enum Template {
    Spruce,
    Fir,
    Rock(u32),
    Shrub,
    Canopy,
    Pole,
    Cube,
}

#[derive(Clone, Copy, PartialEq)]
enum Mat {
    Foliage,
    Plain(u32),
    /// Lit after dark (shopfronts).
    Glow(u32),
}

impl Out {
    fn put(&mut self, t: Template, m: Mat, it: Item, cast: bool) {
        match self.sets.iter_mut().find(|(a, b, _, _)| *a == t && *b == m) {
            Some((_, _, v, _)) => v.push(it),
            None => self.sets.push((t, m, vec![it], cast)),
        }
    }

    fn build(self, w: &mut World) {
        let g = &mut w.graph;
        let group = g.group("menu view");
        let mut geos: Vec<(Template, GeoId)> = Vec::new();
        let mut mats: Vec<(Mat, MaterialId)> = Vec::new();
        for (t, m, items, cast) in &self.sets {
            let geo = match geos.iter().find(|(k, _)| k == t) {
                Some(&(_, g2)) => g2,
                None => {
                    let made = match t {
                        Template::Spruce => conifer_geometry("spruce", 1, 13),
                        Template::Fir => conifer_geometry("fir", 1, 12),
                        Template::Rock(seed) => rock_geometry(*seed, 1.0, RockOpts::default()),
                        Template::Shrub => shrub_geometry(7, 1.0),
                        Template::Canopy => canopy_geometry("shade", 3, 1),
                        Template::Pole => cylinder_geometry(
                            0.5,
                            0.5,
                            1.0,
                            6.0,
                            1.0,
                            false,
                            0.0,
                            std::f64::consts::TAU,
                        ),
                        Template::Cube => box_geometry(1.0, 1.0, 1.0, 1.0, 1.0, 1.0),
                    };
                    let id = g.add_geometry(made);
                    geos.push((*t, id));
                    id
                }
            };
            let mat = match mats.iter().find(|(k, _)| k == m) {
                Some(&(_, m2)) => m2,
                None => {
                    let made = match m {
                        Mat::Foliage => foliage_material(&[]),
                        Mat::Plain(col) => Material::standard()
                            .set("color", f64::from(*col))
                            .set("roughness", 0.85)
                            .set("metalness", 0.0),
                        Mat::Glow(col) => Material::standard()
                            .set("color", f64::from(*col))
                            .set("emissive", f64::from(*col))
                            .set("emissiveIntensity", 0.0)
                            .set("roughness", 0.6),
                    };
                    let id = g.add_material(made);
                    if matches!(m, Mat::Glow(_)) {
                        g.add_night(Some(id), "emissiveIntensity", 0.1, 0.8);
                    }
                    mats.push((*m, id));
                    id
                }
            };
            let node = instanced(g, geo, mat, items, *cast, true);
            g.add(group, node);
        }
        if !self.blocks.is_empty() {
            let node = blocks(w, &self.blocks);
            w.graph.add(group, node);
        }
        if !self.lamps.is_empty() {
            let node = lamps(w, &self.lamps);
            w.graph.add(group, node);
        }
        let root = w.root;
        w.graph.add(root, group);
    }
}

fn item(p: [f64; 3], s: f64, ry: f64, col: Option<u32>) -> Item {
    Item {
        ry,
        col,
        ..Item::at(p[0], p[1], p[2], s, s, s)
    }
}

// ── The levels ──────────────────────────────────────────────────────────

fn sierra(c: &mut Ctx, o: &mut Out) {
    for s in c.stations(7.0) {
        let f = c.frame(s);
        for side in [-1.0, 1.0] {
            if c.r() < 0.7 {
                continue;
            }
            let off = 14.0 + c.r() * 50.0;
            let p = c.beside(&f, side, off);
            let t = if c.r() < 0.6 {
                Template::Spruce
            } else {
                Template::Fir
            };
            let sc = 0.8 + c.r() * 0.7;
            let ry = c.r() * TAU;
            let tone = [0xffffff, 0xe6eedd, 0xd4dccb][(c.r() * 3.0) as usize % 3];
            o.put(
                t,
                Mat::Foliage,
                item([p[0], p[1] - 0.3, p[2]], sc, ry, Some(tone)),
                true,
            );
        }
        if c.r() < 0.3 {
            let side = if c.r() < 0.5 { -1.0 } else { 1.0 };
            let off = 2.0 + c.r() * 6.0;
            let p = c.beside(&f, side, off);
            let sc = 0.6 + c.r() * 1.4;
            let ry = c.r() * TAU;
            o.put(
                Template::Rock(5),
                Mat::Foliage,
                item(p, sc, ry, Some(0x8a8478)),
                true,
            );
        }
    }
}

fn coast(c: &mut Ctx, o: &mut Out) {
    let inland = c.uphill();
    for s in c.stations(42.0) {
        let f = c.frame(s);
        let p = c.beside(&f, inland, 3.5);
        o.put(
            Template::Pole,
            Mat::Plain(0x4a3a2a),
            Item::at(p[0], p[1] + 4.5, p[2], 0.32, 9.0, 0.32),
            true,
        );
        let mut bar = Item::at(p[0], p[1] + 8.4, p[2], 2.4, 0.14, 0.14);
        bar.ry = yaw(&f) + std::f64::consts::FRAC_PI_2;
        o.put(Template::Cube, Mat::Plain(0x4a3a2a), bar, true);
    }
    for s in c.stations(6.0) {
        let f = c.frame(s);
        if c.r() < 0.5 {
            continue;
        }
        let off = 2.0 + c.r() * 30.0;
        let p = c.beside(&f, inland, off);
        let sc = 0.7 + c.r() * 0.8;
        let ry = c.r() * TAU;
        o.put(
            Template::Shrub,
            Mat::Foliage,
            item(p, sc, ry, Some(0x8a9a70)),
            false,
        );
    }
}

fn streets(c: &mut Ctx, o: &mut Out) {
    for side in [-1.0, 1.0] {
        let mut s = c.s0;
        while s < c.s1 {
            let width = 10.0 + c.r() * 14.0;
            let f = c.frame(s + width / 2.0);
            let depth = 14.0 + c.r() * 8.0;
            let height = 9.0 + c.r() * c.r() * 30.0;
            let p = c.beside(&f, side, 6.0 + depth / 2.0);
            let tone = (c.r() * 3.0).floor();
            o.blocks
                .push([p[0], p[1] - 0.5, p[2], yaw(&f), width, height, depth, tone]);
            // The lit shopfront along the pavement.
            if c.r() < 0.75 {
                let q = c.beside(&f, side, 5.85);
                let col = [0xffb070, 0xff6aa0, 0x70d8ff, 0xffe080][(c.r() * 4.0) as usize % 4];
                let mut it = Item::at(q[0], q[1] + 1.6, q[2], width * 0.8, 2.6, 0.2);
                it.ry = yaw(&f);
                o.put(Template::Cube, Mat::Glow(col), it, false);
            }
            s += width + c.r() * 2.0;
        }
        for s in c.stations(28.0) {
            let f = c.frame(s);
            let p = c.beside(&f, side, 2.0);
            o.lamps.push([p[0], p[1], p[2], 7.0, 0xffd28a as f64]);
        }
    }
}

fn desert(c: &mut Ctx, o: &mut Out) {
    const RED: [u32; 4] = [0xc0603a, 0xb0522e, 0xcf7448, 0xa4482a];
    for s in c.stations(20.0) {
        let f = c.frame(s);
        if c.r() < 0.4 {
            continue;
        }
        let side = if c.r() < 0.5 { -1.0 } else { 1.0 };
        let off = 8.0 + c.r() * 40.0;
        let p = c.beside(&f, side, off);
        let col = RED[(c.r() * 4.0) as usize % 4];
        if !c.level_with(&f, &p, 8.0) {
            continue;
        }
        // A hoodoo: two rough rocks stretched into a waisted column, a
        // darker cap rock.
        let base = 1.0 + c.r() * 1.0;
        let h = 5.0 + c.r() * 8.0;
        for (k, (y0, sy, w)) in [(0.25, 0.75, 1.3), (0.65, 0.55, 0.9)]
            .into_iter()
            .enumerate()
        {
            let mut it = Item::at(p[0], p[1] + h * y0, p[2], base * w, h * sy, base * w);
            it.ry = c.r() * TAU + k as f64;
            it.col = Some(col);
            o.put(Template::Rock(9), Mat::Foliage, it, true);
        }
        let mut cap = Item::at(
            p[0],
            p[1] + h * 0.9,
            p[2],
            base * 1.5,
            base * 0.8,
            base * 1.5,
        );
        cap.ry = c.r() * TAU;
        cap.col = Some(0x7a4028);
        o.put(Template::Rock(9), Mat::Foliage, cap, true);
    }
    for s in c.stations(8.0) {
        let f = c.frame(s);
        if c.r() < 0.55 {
            continue;
        }
        let side = if c.r() < 0.5 { -1.0 } else { 1.0 };
        let off = 3.0 + c.r() * 40.0;
        let p = c.beside(&f, side, off);
        if !c.level_with(&f, &p, 6.0) {
            continue;
        }
        let sc = 0.5 + c.r() * 0.6;
        let ry = c.r() * TAU;
        if c.r() < 0.7 {
            o.put(
                Template::Shrub,
                Mat::Foliage,
                item(p, sc, ry, Some(0x9a9a60)),
                false,
            );
        } else {
            o.put(
                Template::Rock(4),
                Mat::Foliage,
                item(p, sc, ry, Some(0xc07050)),
                true,
            );
        }
    }
}

fn seaside(c: &mut Ctx, o: &mut Out) {
    const BARRIER: [u32; 3] = [0xd0302a, 0xf0f0f0, 0x2a4ab0];
    for (k, s) in c.stations(2.5).into_iter().enumerate() {
        let f = c.frame(s);
        let p = c.beside(&f, 1.0, 3.0);
        let mut it = Item::at(p[0], p[1] + 0.45, p[2], 2.5, 0.9, 0.4);
        it.ry = yaw(&f);
        it.col = Some(BARRIER[k % 3]);
        o.put(Template::Cube, Mat::Plain(0xffffff), it, true);
    }
    for s in c.stations(4.0) {
        let f = c.frame(s);
        let p = c.beside(&f, -1.0, 4.0);
        o.put(
            Template::Pole,
            Mat::Plain(0x9aa0a6),
            Item::at(p[0], p[1] + 2.0, p[2], 0.1, 4.0, 0.1),
            false,
        );
    }
    for s in c.stations(12.0) {
        let f = c.frame(s);
        if c.r() < 0.4 {
            continue;
        }
        let side = if c.r() < 0.5 { -1.0 } else { 1.0 };
        let off = 40.0 + c.r() * 160.0;
        let p = c.beside(&f, side, off);
        let sc = 3.0 + c.r() * 2.0;
        let mut it = Item::at(p[0], p[1] + sc * 1.1, p[2], sc * 1.3, sc, sc * 1.3);
        it.ry = c.r() * TAU;
        it.col = Some(0x6f8a4a);
        o.put(Template::Canopy, Mat::Foliage, it, true);
    }
}

fn cruise(c: &mut Ctx, o: &mut Out) {
    for side in [-1.0, 1.0] {
        let mut s = c.s0 - 100.0;
        while s < c.s1 + 200.0 {
            let f = c.frame(s);
            let width = 20.0 + c.r() * 25.0;
            let height = 40.0 + c.r() * c.r() * 140.0;
            let off = 90.0 + c.r() * 350.0;
            let p = c.beside(&f, side, off);
            o.blocks.push([
                p[0],
                p[1] - 1.0,
                p[2],
                yaw(&f),
                width,
                height,
                width * (0.7 + c.r() * 0.6),
                2.0 + (c.r() * 2.0).floor(),
            ]);
            s += 25.0 + c.r() * 40.0;
        }
        for st in c.stations(45.0) {
            let f = c.frame(st);
            let p = c.beside(&f, side, 1.5);
            o.lamps.push([p[0], p[1], p[2], 11.0, 0xffe6b0 as f64]);
        }
    }
}

// ── Buildings and lamps ─────────────────────────────────────────────────

/// One storey and one window bay of the facade texture, metres.
const BAY: f64 = 3.0;
/// Bays a side of the texture holds.
const BAYS: u32 = 8;

/// The facade picture: wall, frames and windows, some lit (map); the lit
/// ones alone (emissive map). Its top-left corner is plain wall (roofs).
fn facade() -> (Texture, Texture) {
    use mp_canvas::Canvas;
    const PX: u32 = 16;
    let n = BAYS * PX;
    let mut map = Canvas::new(n, n);
    let mut glow = Canvas::new(n, n);
    map.set_fill_style("#d8d4cc");
    map.fill_rect(0.0, 0.0, f64::from(n), f64::from(n));
    // The walls glow faintly too: lit by the street after dark.
    glow.set_fill_style("#3a3029");
    glow.fill_rect(0.0, 0.0, f64::from(n), f64::from(n));
    let mut rng = Mulberry32::new(77);
    for j in 0..BAYS {
        for i in 0..BAYS {
            if i == 0 && j == 0 {
                continue;
            }
            let (x, y) = (f64::from(i * PX) + 4.0, f64::from(j * PX) + 4.0);
            let lit = rng.next_f64() < 0.35;
            let col = if lit {
                ["#ffd98a", "#ffe9b8", "#ffc56a", "#cfe4ff"][(rng.next_f64() * 4.0) as usize % 4]
            } else {
                "#20283a"
            };
            map.set_fill_style(col);
            map.fill_rect(x, y, f64::from(PX) - 8.0, f64::from(PX) - 8.0);
            if lit {
                glow.set_fill_style(col);
                glow.fill_rect(x, y, f64::from(PX) - 8.0, f64::from(PX) - 8.0);
            }
        }
    }
    (
        Texture::from_canvas(&map, true, true, 4.0),
        Texture::from_canvas(&glow, true, true, 4.0),
    )
}

/// The buildings' wall tints: brick, sandstone, render; tower glass, steel.
const TONES: [u32; 5] = [0xa0644e, 0xc4ae8c, 0x9a9ca4, 0x6a7c92, 0x8a8e96];

/// The buildings as merged meshes (one per tone), their facades tiled by
/// size, under one group.
fn blocks(w: &mut World, list: &[[f64; 8]]) -> NodeId {
    let span = BAY * f64::from(BAYS);
    let (map, glow) = facade();
    let sg = &mut w.graph;
    let group = sg.group("menu view blocks");
    let tmap = Arc::new(map);
    let tglow = Arc::new(glow);
    let dm = tmap.desc("menu view facade", 0);
    let dg = tglow.desc("menu view windows", 0);
    let map_id = sg.add_texture(Image::Own(tmap), dm);
    let glow_id = sg.add_texture(Image::Own(tglow), dg);
    for (tone, &col) in TONES.iter().enumerate() {
        let mut geos: Vec<BufferGeometry> = Vec::new();
        for &[x, y, z, ry, wd, h, d, t] in list {
            if t as usize != tone {
                continue;
            }
            let mut b = box_geometry(wd, h, d, 1.0, 1.0, 1.0);
            if let Some(uv) = b.get_attribute_mut("uv") {
                // Faces +x, -x, +y, -y, +z, -z, four vertices each.
                for face in 0..6 {
                    let (fu, fv) = match face {
                        0 | 1 => (d / span, h / span),
                        4 | 5 => (wd / span, h / span),
                        _ => (0.0, 0.0),
                    };
                    for k in 0..4 {
                        let i = face * 4 + k;
                        let (u, v) = (uv.get_x(i), uv.get_y(i));
                        // Roofs read the plain corner; walls tile the bays.
                        if fu == 0.0 {
                            uv.set_xy(i, 0.02, 0.98);
                        } else {
                            uv.set_xy(i, u * fu, v * fv);
                        }
                    }
                }
            }
            b.translate(0.0, h / 2.0, 0.0);
            b.rotate_y(ry);
            b.translate(x, y, z);
            geos.push(b);
        }
        if geos.is_empty() {
            continue;
        }
        let refs: Vec<&BufferGeometry> = geos.iter().collect();
        let Some(mut merged) = merge_geometries(&refs, false) else {
            continue;
        };
        merged.compute_bounding_sphere();
        let mat = sg.add_material(
            Material::standard()
                .set("color", f64::from(col))
                .set("map", map_id)
                .set("emissiveMap", glow_id)
                .set("emissive", f64::from(0xffffffu32))
                .set("emissiveIntensity", 0.0)
                .set("roughness", 0.8)
                .set("metalness", 0.1),
        );
        // The windows light up after dark.
        sg.add_night(Some(mat), "emissiveIntensity", 0.0, 0.9);
        let geo = sg.add_geometry(merged);
        let node = sg.mesh(geo, mat);
        let o = sg.get_mut(node);
        o.cast_shadow = true;
        o.receive_shadow = true;
        sg.add(group, node);
    }
    group
}

/// Street lamps: posts and glowing heads, `[x, y, z, height, colour]`.
fn lamps(w: &mut World, list: &[[f64; 5]]) -> NodeId {
    let g = &mut w.graph;
    let group = g.group("menu view lamps");
    let post = g.add_geometry(cylinder_geometry(
        0.09,
        0.12,
        1.0,
        6.0,
        1.0,
        false,
        0.0,
        std::f64::consts::TAU,
    ));
    let head = g.add_geometry(box_geometry(0.7, 0.25, 0.45, 1.0, 1.0, 1.0));
    let post_mat = g.add_material(
        Material::standard()
            .set("color", f64::from(0x30343cu32))
            .set("roughness", 0.6)
            .set("metalness", 0.4),
    );
    let col = list.first().map_or(0xffd28a, |l| l[4] as u32);
    let head_mat = g.add_material(
        Material::standard()
            .set("color", f64::from(col))
            .set("emissive", f64::from(col))
            .set("emissiveIntensity", 0.0),
    );
    g.add_night(Some(head_mat), "emissiveIntensity", 0.1, 3.0);
    let posts: Vec<Item> = list
        .iter()
        .map(|l| Item::at(l[0], l[1] + l[3] / 2.0, l[2], 1.0, l[3], 1.0))
        .collect();
    let heads: Vec<Item> = list
        .iter()
        .map(|l| Item::at(l[0], l[1] + l[3], l[2], 1.0, 1.0, 1.0))
        .collect();
    let a = instanced(g, post, post_mat, &posts, true, true);
    let b = instanced(g, head, head_mat, &heads, false, false);
    g.add(group, a);
    g.add(group, b);
    group
}
