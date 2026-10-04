//! Streets.js's race dressing and street furniture made in the module
//! itself: `buildLamps`, `buildSignals`, `buildBarriers`, `buildGantries`,
//! `buildCrowds`, `buildLanterns`, `buildElevated`, `buildVents` and
//! `buildReflections`, as methods of [`Bld`].

// The JS turns things by `rng() * 6.28`, not 2π.
#![allow(clippy::approx_constant)]
#![allow(clippy::too_many_arguments)]

use std::collections::BTreeSet;
use std::f64::consts::PI;

use mr_math::{js, kernel};
use mr_scene::{NodeType, three};
use serde_json::Value;

use super::buildings::col;
use super::textures::barrier_texture;
use super::{Bld, SignLight, Spill, ambient_patch, anim, ground, props};
use crate::city::textures::{BannerOpts, banner_texture};
use crate::color::Color;
use crate::geom::{GeoBuilder, P3, PrismOpts, StaticOpts, instanced, static_mesh, trs};
use crate::material::Material;
use crate::object::{Layer, MaterialId};
use crate::three_geom::{
    BufferAttribute, BufferGeometry, Euler, ExtrudeOptions, Matrix4, Shape, Vector2, Vector3,
    box_geometry, capsule_geometry, cylinder_geometry, extrude_geometry, merge_geometries,
    plane_geometry, sphere_geometry,
};

const PI2: f64 = PI * 2.0;
const ADDITIVE: f64 = three::ADDITIVE_BLENDING as f64;

/// A street lamp's place (`spots`).
struct Spot {
    x: f64,
    y: f64,
    z: f64,
    dx: f64,
    dz: f64,
    near: bool,
}

/// A traffic signal's lens (`lamps`).
#[derive(Clone, Copy, Debug)]
pub struct SignalLamp {
    pub flash: bool,
    pub phase: f64,
}

impl Bld<'_> {
    fn glow_tex(&mut self) -> crate::object::TextureId {
        let g = self.textures.glow_texture();
        self.graph.cached_texture(&g, Layer::Main, "")
    }

    /// `new THREE.PointsMaterial({ size, map: glowTexture(), color,
    /// transparent: true, depthWrite: false, blending: AdditiveBlending })`.
    fn glow_points(&mut self, size: f64, color: Color) -> MaterialId {
        let glow = self.glow_tex();
        self.graph.add_material(
            Material::points()
                .set("size", size)
                .set("map", glow)
                .set("color", color)
                .set("transparent", true)
                .set("depthWrite", false)
                .set("blending", ADDITIVE),
        )
    }

    /// `new THREE.Points(g, m)` added to the group, `g` holding `position`.
    fn points(&mut self, pos: &[f64], m: MaterialId) -> crate::object::NodeId {
        let mut g = BufferGeometry::new();
        g.set_attribute("position", BufferAttribute::from_f64(pos, 3));
        g.compute_bounding_sphere();
        let geo = self.graph.add_geometry(g);
        let pts = self.graph.drawable(NodeType::Points, geo, m);
        self.graph.add(self.group, pts);
        pts
    }

    // ── Street lamps ──────────────────────────────────────────────
    pub(super) fn build_lamps(&mut self) {
        let (px, pz, hw) = (self.g.px, self.g.pz, self.g.hw);
        let mut spots: Vec<Spot> = Vec::new();
        let mut seen: BTreeSet<(i64, i64)> = BTreeSet::new();
        for bi in 0..self.blocks.len() {
            let (b_i, b_j, tier) = {
                let b = &self.blocks[bi];
                (b.i as f64, b.j as f64, b.tier)
            };
            if tier > 1 {
                continue;
            }
            // Lamps along the block's four kerbs, pointing over the road.
            let x0 = b_i * px + hw + 0.6;
            let x1 = (b_i + 1.0) * px - hw - 0.6;
            let z0 = b_j * pz + hw + 0.6;
            let z1 = (b_j + 1.0) * pz - hw - 0.6;
            let every = if tier == 0 { 30.0 } else { 45.0 };
            let mut put = |s: &Self, x: f64, z: f64, dx: f64, dz: f64| {
                let key = (js::round(x) as i64, js::round(z) as i64);
                if seen.contains(&key) || !s.on_kerb(x, z) {
                    return;
                }
                seen.insert(key);
                spots.push(Spot {
                    x,
                    y: ground(x, z) + 0.15,
                    z,
                    dx,
                    dz,
                    near: tier == 0 && s.t.distance_to_road(x, z, 24.0).d < hw + 3.0,
                });
            };
            let mut x = x0 + 12.0;
            while x < x1 - 8.0 {
                put(self, x, z0, 0.0, -1.0);
                put(self, x + every / 2.0, z1, 0.0, 1.0);
                x += every;
            }
            let mut z = z0 + 12.0;
            while z < z1 - 8.0 {
                put(self, x0, z, -1.0, 0.0);
                put(self, x1, z + every / 2.0, 1.0, 0.0);
                z += every;
            }
        }
        let mut pole = cylinder_geometry(0.1, 0.14, 1.0, 6.0, 1.0, false, 0.0, PI2);
        pole.translate(0.0, 0.5, 0.0);
        let mut pole_m = Vec::new();
        let mut arm_m = Vec::new();
        let mut lens_m = Vec::new();
        let mut pool_m = Vec::new();
        for s in &spots {
            let yaw = -kernel::atan2(s.dz, s.dx);
            let h = 8.0;
            let arm = 2.2;
            pole_m.push(trs(s.x, s.y, s.z, 0.0, 1.0, h, 1.0, 0.0, 0.0));
            arm_m.push(trs(
                s.x + s.dx * arm / 2.0,
                s.y + h - 0.1,
                s.z + s.dz * arm / 2.0,
                yaw,
                arm,
                0.12,
                0.12,
                0.0,
                0.0,
            ));
            lens_m.push(trs(
                s.x + s.dx * arm,
                s.y + h - 0.25,
                s.z + s.dz * arm,
                yaw,
                0.7,
                0.12,
                0.35,
                0.0,
                0.0,
            ));
            if s.near {
                let px_ = s.x + s.dx * (arm + 1.5);
                let pz_ = s.z + s.dz * (arm + 1.5);
                pool_m.push(trs(
                    px_,
                    ground(px_, pz_) + 0.06,
                    pz_,
                    0.0,
                    12.0,
                    1.0,
                    12.0,
                    0.0,
                    0.0,
                ));
                // And a smaller pool on the pavement round the pole.
                pool_m.push(trs(
                    s.x - s.dx * 0.8,
                    s.y + 0.05,
                    s.z - s.dz * 0.8,
                    0.0,
                    7.0,
                    1.0,
                    7.0,
                    0.0,
                    0.0,
                ));
                self.sign_lights.push(SignLight {
                    x: s.x + s.dx * arm,
                    y: s.y + h,
                    z: s.z + s.dz * arm,
                    col: "#ffb870",
                    lamp: true,
                    big: false,
                });
            }
        }
        let metal = self.graph.add_material(
            Material::standard()
                .set("color", 0x4a4e54)
                .set("metalness", 0.5)
                .set("roughness", 0.6),
        );
        let pole = self.graph.add_geometry(pole);
        let a = instanced(self.graph, pole, metal, &pole_m, false, false);
        let bx = self
            .graph
            .add_geometry(box_geometry(1.0, 1.0, 1.0, 1.0, 1.0, 1.0));
        let b = instanced(self.graph, bx, metal, &arm_m, false, false);
        self.graph.add(self.group, a);
        self.graph.add(self.group, b);
        let bx = self
            .graph
            .add_geometry(box_geometry(1.0, 1.0, 1.0, 1.0, 1.0, 1.0));
        let lens = self
            .graph
            .add_material(Material::basic().set("color", Color::new(4.0, 2.9, 1.7)));
        let l = instanced(self.graph, bx, lens, &lens_m, false, false);
        self.graph.add(self.group, l);
        let mut pool_geo = plane_geometry(1.0, 1.0, 1.0, 1.0);
        pool_geo.rotate_x(-PI / 2.0);
        let glow = self.glow_tex();
        let pool_mat = self.graph.add_material(
            Material::basic()
                .set("map", glow)
                .set("color", Color::new(0.12, 0.085, 0.05))
                .set("transparent", true)
                .set("depthWrite", false)
                .set("blending", ADDITIVE)
                .set("polygonOffset", true)
                .set("polygonOffsetFactor", -4.0)
                .set("polygonOffsetUnits", -4.0),
        );
        if !pool_m.is_empty() {
            let pg = self.graph.add_geometry(pool_geo);
            let p = instanced(self.graph, pg, pool_mat, &pool_m, false, false);
            self.graph.get_mut(p).render_order = 2.0;
            self.graph.add(self.group, p);
        }
    }

    // ── Traffic signals: flashing amber where the race runs ───────
    pub(super) fn build_signals(&mut self) {
        let (px, pz, hw) = (self.g.px, self.g.pz, self.g.hw);
        let mut pole_m = Vec::new();
        let mut arm_m = Vec::new();
        let mut head_m = Vec::new();
        let mut lamps: Vec<(P3, SignalLamp)> = Vec::new();
        for bi in 0..self.blocks.len() {
            let (b_i, b_j, tier) = {
                let b = &self.blocks[bi];
                (b.i, b.j, b.tier)
            };
            if tier != 0 {
                continue;
            }
            for [di, dj] in [[0, 0], [1, 1]] {
                let i = b_i + di;
                let j = b_j + dj;
                let use_ = self.cross_use.contains(&(i, j));
                // Two masts per crossing on opposite corners, arms over the road.
                for [sx, sz, ax, az] in [
                    [1.0, 1.0, 0.0, -1.0],
                    [-1.0, -1.0, 0.0, 1.0],
                    [1.0, -1.0, -1.0, 0.0],
                    [-1.0, 1.0, 1.0, 0.0],
                ] {
                    if self.rng() < 0.5 {
                        continue;
                    }
                    let x = i as f64 * px + sx * (hw + 1.2);
                    let z = j as f64 * pz + sz * (hw + 1.2);
                    if !self.on_kerb(x, z) {
                        continue;
                    }
                    let y = ground(x, z) + 0.15;
                    pole_m.push(trs(x, y, z, 0.0, 1.0, 6.2, 1.0, 0.0, 0.0));
                    let l = hw * 0.9;
                    let yaw = -kernel::atan2(az, ax);
                    arm_m.push(trs(
                        x + ax * l / 2.0,
                        y + 6.0,
                        z + az * l / 2.0,
                        yaw,
                        l,
                        0.14,
                        0.14,
                        0.0,
                        0.0,
                    ));
                    let hx = x + ax * l;
                    let hz = z + az * l;
                    head_m.push(trs(hx, y + 5.1, hz, yaw, 0.4, 1.1, 0.4, 0.0, 0.0));
                    // Lenses face both ways along the other street.
                    for f in [-1.0, 1.0] {
                        lamps.push((
                            [
                                hx + az * f * 0.22,
                                y + (if use_ { 5.1 } else { 5.45 }),
                                hz - ax * f * 0.22,
                            ],
                            SignalLamp {
                                flash: use_,
                                phase: if ax.abs() > 0.0 { 0.0 } else { 1.0 },
                            },
                        ));
                    }
                }
            }
        }
        if pole_m.is_empty() {
            return;
        }
        let metal = self.graph.add_material(
            Material::standard()
                .set("color", 0x2c2f33)
                .set("metalness", 0.5)
                .set("roughness", 0.6),
        );
        let mut pole = cylinder_geometry(0.11, 0.13, 1.0, 6.0, 1.0, false, 0.0, PI2);
        pole.translate(0.0, 0.5, 0.0);
        let mut head = box_geometry(1.0, 1.0, 1.0, 1.0, 1.0, 1.0);
        head.translate(0.0, 0.5, 0.0);
        let pole = self.graph.add_geometry(pole);
        let a = instanced(self.graph, pole, metal, &pole_m, false, false);
        let bx = self
            .graph
            .add_geometry(box_geometry(1.0, 1.0, 1.0, 1.0, 1.0, 1.0));
        let b = instanced(self.graph, bx, metal, &arm_m, false, false);
        let head_mat = self.graph.add_material(
            Material::standard()
                .set("color", 0x14161a)
                .set("roughness", 0.7),
        );
        let head = self.graph.add_geometry(head);
        let c = instanced(self.graph, head, head_mat, &head_m, false, false);
        for n in [a, b, c] {
            self.graph.add(self.group, n);
        }
        // Lenses as glowing points: amber flashing on the route, red/green elsewhere.
        let pos: Vec<f64> = lamps.iter().flat_map(|(p, _)| *p).collect();
        let mut g = BufferGeometry::new();
        g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
        g.set_attribute("color", BufferAttribute::from_f64(&vec![0.0; pos.len()], 3));
        g.compute_bounding_sphere();
        let glow = self.glow_tex();
        let m = self.graph.add_material(
            Material::points()
                .set("size", 0.9)
                .set("map", glow)
                .set("vertexColors", true)
                .set("transparent", true)
                .set("depthWrite", false)
                .set("blending", ADDITIVE),
        );
        let geo = self.graph.add_geometry(g);
        let pts = self.graph.drawable(NodeType::Points, geo, m);
        self.graph.add(self.group, pts);
        self.anims.push(Box::new(anim::Signals {
            time: 0.0,
            last_a: -1.0,
            last_p: -1.0,
            lamps: lamps.into_iter().map(|(_, l)| l).collect(),
            geo,
        }));
    }

    // ── Race dressing ─────────────────────────────────────────────
    /// Barriers wherever the road's wall crosses open street.
    pub(super) fn build_barriers(&mut self) {
        let t = self.t;
        let hw = self.g.hw;
        struct Unit {
            s: f64,
            side: f64,
        }
        let mut units: Vec<Unit> = Vec::new();
        for side in [-1.0, 1.0] {
            let mut run: Vec<f64> = Vec::new();
            let flush = |run: &mut Vec<f64>, units: &mut Vec<Unit>| {
                if run.len() >= 2 {
                    let s0 = run[0];
                    let s1 = run[run.len() - 1];
                    let n = js::max(1.0, js::round((s1 - s0 + 1.6) / 2.05));
                    let mut k = 0.0;
                    while k < n {
                        units.push(Unit {
                            s: s0 - 0.8 + ((s1 - s0 + 1.6) * (k + 0.5)) / n,
                            side,
                        });
                        k += 1.0;
                    }
                }
                run.clear();
            };
            let mut s = 0.0;
            while s <= t.length {
                let f = t.frame(s);
                let lat = side * (f.wall_r + 0.6);
                let x = f.x + f.rx * lat;
                let z = f.z + f.rz * lat;
                if !self.on_kerb(x, z) {
                    run.push(s);
                } else {
                    flush(&mut run, &mut units);
                }
                s += 0.5;
            }
            flush(&mut run, &mut units);
        }
        // Across the road at both ends.
        struct Across {
            x: f64,
            z: f64,
            yaw: f64,
        }
        let mut across: Vec<Across> = Vec::new();
        for (s, dir) in [(0.6, 1.0), (t.length - 0.6, -1.0)] {
            let f = t.frame(s);
            let mut q = -hw - 0.3;
            while q <= hw + 0.3 {
                across.push(Across {
                    x: f.x + f.rx * q - f.fx * dir * 0.3,
                    z: f.z + f.rz * q - f.fz * dir * 0.3,
                    yaw: kernel::atan2(f.rz, f.rx),
                });
                q += 2.05;
            }
        }
        let mut m = Vec::new();
        let mut colors = Vec::new();
        let mut blink: Vec<f64> = Vec::new();
        let c1 = Color::new(1.0, 1.0, 1.0);
        let c2 = Color::new(1.0, 0.35, 0.3);
        let mut k = 0u64;
        for u in &units {
            let f = t.frame(u.s);
            let lat = u.side * (f.wall_r + 0.32);
            let x = f.x + f.rx * lat;
            let z = f.z + f.rz * lat;
            let yaw = kernel::atan2(f.fz, f.fx);
            m.push(trs(x, ground(x, z), z, -yaw, 1.0, 1.0, 1.0, 0.0, 0.0));
            colors.push(if !k.is_multiple_of(2) { c1 } else { c2 });
            k += 1;
            if k.is_multiple_of(4) {
                blink.extend_from_slice(&[x, ground(x, z) + 1.0, z]);
            }
        }
        for a in &across {
            m.push(trs(
                a.x,
                ground(a.x, a.z),
                a.z,
                -a.yaw,
                1.0,
                1.0,
                1.0,
                0.0,
                0.0,
            ));
            colors.push(if !k.is_multiple_of(2) { c1 } else { c2 });
            k += 1;
            if k.is_multiple_of(3) {
                blink.extend_from_slice(&[a.x, ground(a.x, a.z) + 1.0, a.z]);
            }
        }
        // Water-filled barrier: tapered block, 2 m long.
        let shape = Shape::from_points(&[
            Vector2::new(-0.3, 0.0),
            Vector2::new(0.3, 0.0),
            Vector2::new(0.2, 0.82),
            Vector2::new(-0.2, 0.82),
        ]);
        let mut geo = extrude_geometry(&[shape], &ExtrudeOptions::flat(2.0));
        geo.translate(0.0, 0.0, -1.0);
        geo.rotate_y(PI / 2.0); // length along local X
        // UVs: wrap the stripe texture along the length.
        {
            let pos = geo.position().clone();
            let uv = geo.get_attribute_mut("uv").expect("uv");
            for q in 0..uv.count() {
                uv.set_xy(q, (pos.get_x(q) + 1.0) / 2.0, pos.get_y(q) / 0.82);
            }
        }
        geo.compute_vertex_normals();
        let bt = barrier_texture(self.textures);
        let bt = self.graph.cached_texture(&bt, Layer::Main, "");
        let mat = self
            .graph
            .add_material(Material::standard().set("map", bt).set("roughness", 0.55));
        let geo = self.graph.add_geometry(geo);
        let im = instanced(self.graph, geo, mat, &m, true, true);
        {
            let inst = self
                .graph
                .get_mut(im)
                .instances
                .as_mut()
                .expect("instanced");
            for (q, c) in colors.iter().enumerate() {
                inst.set_color_at(q, *c);
            }
        }
        self.graph.add(self.group, im);
        self.barrier_count = m.len();
        // Amber flashers.
        if !blink.is_empty() {
            let mat = self.glow_points(0.7, Color::new(3.5, 1.8, 0.2));
            self.points(&blink, mat);
            self.anims.push(Box::new(anim::Blink { time: 0.0, mat }));
        }
    }

    /// Start and finish gantries over the street.
    pub(super) fn build_gantries(&mut self) {
        let t = self.t;
        let truss = self.graph.add_material(
            Material::standard()
                .set("color", 0x2a2c30)
                .set("metalness", 0.6)
                .set("roughness", 0.45),
        );
        let mut lights: Vec<f64> = Vec::new();
        let mut gantry = |s: &mut Self, at: f64, text: &str, checker: bool| {
            let f = t.frame(at);
            let w = f.hw + 1.2;
            let h = 6.4;
            let y = f.y;
            let yaw = kernel::atan2(f.fz, f.fx);
            let mut p = GeoBuilder::new(false, false);
            for side in [-1.0, 1.0] {
                let x = f.x + f.rx * side * w;
                let z = f.z + f.rz * side * w;
                p.box_(
                    x,
                    ground(x, z),
                    z,
                    0.6,
                    h + 1.2,
                    0.6,
                    yaw,
                    &PrismOpts::default(),
                );
            }
            p.box_(
                f.x,
                y + h,
                f.z,
                0.8,
                1.4,
                w * 2.0 + 0.6,
                yaw,
                &PrismOpts::default(),
            );
            let geo = s.graph.add_geometry(p.build());
            let mesh = static_mesh(
                s.graph,
                geo,
                truss,
                &StaticOpts {
                    cast: true,
                    ..StaticOpts::default()
                },
            );
            s.graph.add(s.group, mesh);
            // Banner on both faces.
            let tex = banner_texture(
                text,
                &BannerOpts {
                    checker,
                    bg: if checker { "#111" } else { "#0c0c12" },
                    fg: if checker { "#fff" } else { "#ff3c8a" },
                    ..BannerOpts::default()
                },
            );
            let tex = props::own_texture(s.graph, tex);
            let bm = s.graph.add_material(
                Material::basic()
                    .set("map", tex)
                    .set("color", Color::new(1.3, 1.3, 1.3)),
            );
            let bg = s
                .graph
                .add_geometry(plane_geometry(w * 2.0 - 1.0, 1.3, 1.0, 1.0));
            // One face each way: the plane's normal is +Z, turned onto ±(fx, fz).
            for dir in [-1.0, 1.0] {
                let mesh = s.graph.mesh(bg, bm);
                let o = s.graph.get_mut(mesh);
                o.position = Vector3::new(
                    f.x + f.fx * dir * 0.45,
                    y + h + 0.02,
                    f.z + f.fz * dir * 0.45,
                );
                o.set_rotation(&Euler::new(
                    0.0,
                    -yaw + PI / 2.0 + (if dir < 0.0 { PI } else { 0.0 }),
                    0.0,
                ));
                s.graph.add(s.group, mesh);
            }
            let mut q = -w + 0.6;
            while q <= w - 0.6 {
                lights.extend_from_slice(&[f.x + f.rx * q, y + h - 0.85, f.z + f.rz * q]);
                q += 1.1;
            }
        };
        gantry(self, t.start_s, "DOWNTOWN NIGHT RUN", false);
        gantry(self, t.finish_s, "FINISH", true);
        let m = self.glow_points(0.8, Color::new(3.2, 3.0, 2.6));
        self.points(&lights, m);
    }

    /// Spectators on the pavements at the start and the finish.
    pub(super) fn build_crowds(&mut self) {
        let t = self.t;
        let hw = self.g.hw;
        let mut ms = Vec::new();
        let mut cols = Vec::new();
        let mut flashes: Vec<f64> = Vec::new();
        for (c, len) in [(t.start_s + 20.0, 150.0), (t.finish_s - 40.0, 180.0)] {
            let mut s = c - len / 2.0;
            while s < c + len / 2.0 {
                let f = t.frame(s);
                for side in [-1.0, 1.0] {
                    for row in 0..3 {
                        if self.rng() < 0.3 {
                            continue;
                        }
                        let lat = side * (hw + 0.9 + f64::from(row) * 0.9 + self.rng() * 0.5);
                        let x = f.x + f.rx * lat + f.fx * (self.rng() - 0.5) * 0.6;
                        let z = f.z + f.rz * lat + f.fz * (self.rng() - 0.5) * 0.6;
                        if !self.on_kerb(x, z) || self.t.distance_to_road(x, z, 20.0).d < hw + 0.7 {
                            continue;
                        }
                        let h = 0.85 + self.rng() * 0.2;
                        let yaw = self.rng() * 6.28;
                        ms.push(trs(x, ground(x, z) + 0.15, z, yaw, 1.0, h, 1.0, 0.0, 0.0));
                        let hh = self.rng();
                        let ss = 0.25 + self.rng() * 0.4;
                        let ll = 0.12 + self.rng() * 0.3;
                        let mut cc = Color::default();
                        cc.set_hsl(hh, ss, ll);
                        cols.push(cc);
                        if self.rng() < 0.08 {
                            flashes.extend_from_slice(&[x, ground(x, z) + 1.55, z]);
                        }
                    }
                }
                s += 0.9;
            }
        }
        if ms.is_empty() {
            return;
        }
        let mut body = capsule_geometry(0.19, 0.85, 3.0, 6.0, 1.0);
        body.scale(1.15, 1.0, 0.8);
        body.translate(0.0, 0.62, 0.0);
        let bm = self
            .graph
            .add_material(Material::standard().set("roughness", 0.9));
        let body = self.graph.add_geometry(body);
        let im = instanced(self.graph, body, bm, &ms, false, true);
        {
            let inst = self
                .graph
                .get_mut(im)
                .instances
                .as_mut()
                .expect("instanced");
            for (k, c) in cols.iter().enumerate() {
                inst.set_color_at(k, *c);
            }
        }
        let mut head = sphere_geometry(0.13, 8.0, 6.0, 0.0, PI2, 0.0, PI);
        head.translate(0.0, 1.33, 0.0);
        let hm = self
            .graph
            .add_material(Material::standard().set("roughness", 0.8));
        let head = self.graph.add_geometry(head);
        let heads = instanced(self.graph, head, hm, &ms, false, false);
        let skin = [0x3a2418, 0x6a4028, 0x9a6a48, 0xc89a78, 0x2a1a12];
        {
            let inst = self
                .graph
                .get_mut(heads)
                .instances
                .as_mut()
                .expect("instanced");
            for k in 0..ms.len() {
                inst.set_color_at(k, Color::hex(skin[k % skin.len()]));
            }
        }
        self.graph.add(self.group, im);
        self.graph.add(self.group, heads);
        // Phone cameras twinkling in the crowd.
        let m = self.glow_points(0.35, Color::new(3.0, 3.0, 3.3));
        let pts = self.points(&flashes, m);
        self.graph
            .get_mut(pts)
            .user_data
            .insert("dynamic".into(), Value::Bool(true));
        self.anims
            .push(Box::new(anim::Flashes { time: 0.0, mat: m }));
    }

    /// Strings of paper lanterns across the Neon District's streets.
    pub(super) fn build_lanterns(&mut self) {
        let t = self.t;
        let (hw, walk) = (self.g.hw, self.g.walk);
        let mut pos: Vec<f64> = Vec::new();
        let mut line: Vec<f64> = Vec::new();
        let mut s = 40.0;
        while s < t.length {
            let f = t.frame(s);
            if f.zone != 0 || f.kappa.abs() > 0.004 {
                s += 16.0;
                continue;
            }
            if self.rng() < 0.35 {
                s += 16.0;
                continue;
            }
            let w = hw + walk - 0.4;
            let y0 = ground(f.x, f.z) + 7.5;
            let mut pts: Vec<P3> = Vec::new();
            for q in 0..=12 {
                let u = f64::from(q) / 12.0;
                let lat = -w + 2.0 * w * u;
                let sag = 1.6 * 4.0 * u * (1.0 - u);
                pts.push([f.x + f.rx * lat, y0 - sag, f.z + f.rz * lat]);
            }
            for q in 0..pts.len() - 1 {
                line.extend_from_slice(&pts[q]);
                line.extend_from_slice(&pts[q + 1]);
            }
            for q in 1..pts.len() - 1 {
                pos.extend_from_slice(&[pts[q][0], pts[q][1] - 0.35, pts[q][2]]);
            }
            s += 16.0;
        }
        if pos.is_empty() {
            return;
        }
        let mut lg = BufferGeometry::new();
        lg.set_attribute("position", BufferAttribute::from_f64(&line, 3));
        let lm = self
            .graph
            .add_material(Material::line_basic().set("color", 0x151515));
        let lg = self.graph.add_geometry(lg);
        let ls = self.graph.drawable(NodeType::LineSegments, lg, lm);
        self.graph.add(self.group, ls);
        let mut ball = sphere_geometry(0.3, 8.0, 6.0, 0.0, PI2, 0.0, PI);
        ball.scale(1.0, 1.25, 1.0);
        let mut ms = Vec::new();
        let mut k = 0;
        while k < pos.len() {
            ms.push(trs(
                pos[k],
                pos[k + 1],
                pos[k + 2],
                0.0,
                1.0,
                1.0,
                1.0,
                0.0,
                0.0,
            ));
            k += 3;
        }
        let bm = self
            .graph
            .add_material(Material::basic().set("color", Color::new(2.6, 0.5, 0.25)));
        let ball = self.graph.add_geometry(ball);
        let im = instanced(self.graph, ball, bm, &ms, false, false);
        let cc = [
            Color::new(2.6, 0.45, 0.22),
            Color::new(2.8, 1.5, 0.3),
            Color::new(2.4, 0.35, 0.6),
        ];
        {
            let inst = self
                .graph
                .get_mut(im)
                .instances
                .as_mut()
                .expect("instanced");
            for k in 0..ms.len() {
                inst.set_color_at(k, cc[k % 3]);
            }
        }
        self.graph.add(self.group, im);
    }

    /// The elevated railway over one of the Neon District's avenues, and a
    /// train that crosses the race route as you pass under it.
    pub(super) fn build_elevated(&mut self) {
        let t = self.t;
        let (px, pz, hw) = (self.g.px, self.g.pz, self.g.hw);
        let Some(el) = t.tag("el").first().map(|g| (g.s0, g.s1)) else {
            return;
        };
        let ci = 8.0;
        let x = ci * px;
        let j0 = -8.0;
        let j1 = 12.0;
        let z0 = j0 * pz;
        let z1 = j1 * pz;
        let deck_y = |z: f64| ground(x, z) + 8.5;
        let steel = self.graph.add_material(ambient_patch(
            Material::standard()
                .set("color", 0x4a5044)
                .set("metalness", 0.55)
                .set("roughness", 0.55),
            [0.05, 0.05, 0.045],
            "steel",
        ));
        let mut b = GeoBuilder::new(false, false);
        let mut brace_m = Vec::new();
        let step = 15.0;
        let no = PrismOpts::default();
        let mut z = z0;
        while z < z1 {
            let y = deck_y(z + step / 2.0);
            // Plate girders either side (with top and bottom flanges), the deck
            // between, and a parapet rail along each edge.
            // (GeoBuilder.box: sx runs along yaw, so π/2 puts the length on z.)
            for sx in [-1.0, 1.0] {
                b.box_(
                    x + sx * 3.4,
                    y - 1.2,
                    z + step / 2.0,
                    step,
                    1.6,
                    0.3,
                    PI / 2.0,
                    &no,
                );
                b.box_(
                    x + sx * 3.4,
                    y - 1.25,
                    z + step / 2.0,
                    step,
                    0.12,
                    0.8,
                    PI / 2.0,
                    &no,
                );
                b.box_(
                    x + sx * 3.4,
                    y + 0.28,
                    z + step / 2.0,
                    step,
                    0.12,
                    0.8,
                    PI / 2.0,
                    &no,
                );
                b.box_(
                    x + sx * 3.55,
                    y + 0.4,
                    z + step / 2.0,
                    step,
                    0.08,
                    0.08,
                    PI / 2.0,
                    &no,
                );
                b.box_(
                    x + sx * 3.55,
                    y + 1.1,
                    z + step / 2.0,
                    step,
                    0.1,
                    0.1,
                    PI / 2.0,
                    &no,
                );
                // Stiffeners down the girder face, and rail posts.
                let mut q = 0.0;
                while q < step {
                    b.box_(x + sx * 3.58, y - 1.2, z + q, 0.08, 1.5, 0.12, 0.0, &no);
                    q += 5.0;
                }
            }
            b.box_(x, y - 0.35, z + step / 2.0, step, 0.35, 6.4, PI / 2.0, &no);
            // Cross-frames under the deck, seen from the street.
            let mut q = 0.0;
            while q < step {
                b.box_(x, y - 0.8, z + q, 6.6, 0.45, 0.25, 0.0, &no);
                q += 5.0;
            }
            // X-bracing between the girders every bay.
            for d in [-1.0, 1.0] {
                brace_m.push(trs(
                    x,
                    y - 1.1,
                    z + step / 2.0,
                    d * kernel::atan2(6.4, step),
                    0.16,
                    0.16,
                    kernel::hypot(step, 6.4),
                    0.0,
                    0.0,
                ));
            }
            // Lamps under the deck light the road below.
            self.b_glow.at(x, z).box_(
                x + 2.2,
                y - 1.05,
                z + step / 2.0,
                0.5,
                0.08,
                0.9,
                0.0,
                &col([3.0, 2.6, 1.9]),
            );
            self.b_glow.at(x, z).box_(
                x - 2.2,
                y - 1.05,
                z + step / 2.0,
                0.5,
                0.08,
                0.9,
                0.0,
                &col([3.0, 2.6, 1.9]),
            );
            if self.route_dist(x, z + step / 2.0) < 30.0 {
                self.spill.push(Spill {
                    x,
                    z: z + step / 2.0,
                    r: 6.0,
                    col: [0.3, 0.26, 0.18],
                    tx: 0.0,
                    tz: 1.0,
                    long: false,
                });
            }
            // Columns on the pavements every other bay, with a crossbeam, knee
            // braces and a catenary mast above.
            if js::round((z - z0) / step) % 2.0 == 0.0 {
                let near_cross = (z - js::round(z / pz) * pz).abs() < hw + 4.0;
                if !near_cross {
                    for sx in [-1.0, 1.0] {
                        let cx = x + sx * (hw + 1.2);
                        let g0 = ground(cx, z);
                        b.box_(cx, g0, z, 0.6, y - 1.2 - g0, 0.6, 0.0, &no);
                        b.box_(cx, g0, z, 0.9, 0.5, 0.9, 0.0, &no); // footing
                        b.box_(cx, y - 2.6, z, 0.9, 0.5, 0.9, 0.0, &no); // cap
                        brace_m.push(trs(
                            cx - sx * 0.9,
                            y - 2.4,
                            z,
                            0.0,
                            0.14,
                            1.9,
                            0.14,
                            0.0,
                            sx * 0.78,
                        ));
                    }
                    b.box_(x, y - 2.0, z, hw * 2.0 + 3.2, 0.8, 0.7, 0.0, &no);
                }
                for sx in [-1.0, 1.0] {
                    b.box_(x + sx * 3.2, y + 0.3, z, 0.18, 5.2, 0.18, 0.0, &no);
                }
                b.box_(x, y + 5.3, z, 6.6, 0.16, 0.16, 0.0, &no);
            }
            // Catenary wire.
            for sx in [-1.5, 1.5] {
                props::wire(
                    self,
                    [x + sx, y + 5.0, z],
                    [x + sx, y + 5.0, z + step * 2.0],
                    0.25,
                    4,
                );
            }
            z += step;
        }
        let geo = self.graph.add_geometry(b.build());
        let mesh = static_mesh(
            self.graph,
            geo,
            steel,
            &StaticOpts {
                cast: true,
                ..StaticOpts::default()
            },
        );
        self.graph.add(self.group, mesh);
        if !brace_m.is_empty() {
            let bx = self
                .graph
                .add_geometry(box_geometry(1.0, 1.0, 1.0, 1.0, 1.0, 1.0));
            let im = instanced(self.graph, bx, steel, &brace_m, false, false);
            self.graph.add(self.group, im);
        }
        // Sleepers and rails.
        let mut r = GeoBuilder::new(false, false);
        for rx in [-2.1, -0.9, 0.9, 2.1] {
            r.box_(
                x + rx,
                deck_y(z0),
                (z0 + z1) / 2.0,
                z1 - z0,
                0.15,
                0.12,
                PI / 2.0,
                &no,
            );
        }
        let rail_mat = self.graph.add_material(
            Material::standard()
                .set("color", 0x8a8a88)
                .set("metalness", 0.8)
                .set("roughness", 0.35),
        );
        let geo = self.graph.add_geometry(r.build());
        let mesh = static_mesh(self.graph, geo, rail_mat, &StaticOpts::default());
        self.graph.add(self.group, mesh);
        // The train: four cars in one mesh, textured with lit windows,
        // passengers, doors and a cab at each end.
        let tt = super::textures::train_texture(self.textures);
        let tmap = self.graph.cached_texture(&tt, Layer::Main, "");
        let te = self.graph.cached_texture(&tt, Layer::Emissive, "");
        let body = self.graph.add_material(
            Material::standard()
                .set("map", tmap)
                .set("emissiveMap", te)
                .set("emissive", Color::new(2.2, 2.0, 1.7))
                .set("metalness", 0.55)
                .set("roughness", 0.35),
        );
        let car_l = 15.0;
        let gap = 0.8;
        let mut parts: Vec<BufferGeometry> = Vec::new();
        // BoxGeometry face order: +x, −x, +y, −y, +z, −z (4 vertices each).
        let region = |g: &mut BufferGeometry, face: usize, u0: f64, u1: f64| {
            let uv = g.get_attribute_mut("uv").expect("uv");
            for v in face * 4..face * 4 + 4 {
                let x = uv.get_x(v);
                uv.set_x(v, u0 + x * (u1 - u0));
            }
        };
        for k in 0..4 {
            let kf = f64::from(k);
            let mut c = box_geometry(2.9, 3.2, car_l, 1.0, 1.0, 1.0);
            // The box's ±x faces run their u along z: sides use the side strip.
            region(&mut c, 0, 0.0, 0.625);
            region(&mut c, 1, 0.0, 0.625);
            region(&mut c, 2, 0.875, 1.0);
            region(&mut c, 3, 0.875, 1.0);
            region(&mut c, 4, 0.625, 0.875);
            region(&mut c, 5, 0.625, 0.875);
            c.translate(0.0, 1.9, kf * (car_l + gap));
            parts.push(c);
            for bz in [-car_l * 0.32, car_l * 0.32] {
                let mut bg = box_geometry(2.4, 0.6, 2.6, 1.0, 1.0, 1.0);
                {
                    let uv = bg.get_attribute_mut("uv").expect("uv");
                    for v in 0..uv.count() {
                        uv.set_xy(v, 0.3, 0.02);
                    }
                }
                bg.translate(0.0, 0.35, kf * (car_l + gap) + bz);
                parts.push(bg);
            }
        }
        let refs: Vec<&BufferGeometry> = parts.iter().collect();
        let train_geo = merge_geometries(&refs, false).expect("the cars merge");
        let train_geo = self.graph.add_geometry(train_geo);
        let train = self.graph.mesh(train_geo, body);
        {
            let o = self.graph.get_mut(train);
            o.cast_shadow = true;
            o.user_data.insert("dynamic".into(), Value::Bool(true));
            o.position = Vector3::new(x - 1.5, 0.0, z0);
        }
        self.graph.add(self.group, train);
        let len = 4.0 * (car_l + gap);
        // Where the route passes under the viaduct.
        let mut s_cross = el.0;
        let mut bd = f64::INFINITY;
        let mut s = el.0;
        while s < el.1 {
            let d = (super::arr_at(&t.px, s) - x).abs();
            if d < bd {
                bd = d;
                s_cross = s;
            }
            s += 1.0;
        }
        let zc = super::arr_at(&t.pz, s_cross);
        self.anims.push(Box::new(anim::Train {
            node: train,
            x: x - 1.5,
            deck_x: x,
            z0,
            z1,
            len,
            s_cross,
            zc,
            zt: z0,
            free: true,
        }));
    }

    /// Steam from manholes in the road and grates by the kerb.
    pub(super) fn build_vents(&mut self) {
        let t = self.t;
        let hw = self.g.hw;
        let mut s = 60.0;
        while s < t.length - 60.0 {
            let f = t.frame(s);
            if f.zone == 1 && self.rng() < 0.7 {
                s += 90.0 + self.rng() * 140.0;
                continue;
            }
            let lat = (self.rng() - 0.5) * (hw * 1.4);
            let x = f.x + f.rx * lat;
            let z = f.z + f.rz * lat;
            let y = t.surface_y(s, lat);
            // Manhole cover: a dark disc flush with the road.
            let cyl =
                cylinder_geometry(0.4, 0.4, 0.03, 12.0, 1.0, false, 0.0, PI2).to_non_indexed();
            self.add_geo_plain(
                x,
                z,
                &cyl,
                &Matrix4::IDENTITY.set_position(x, y + 0.02, z),
                Some([0.12, 0.11, 0.1]),
            );
            self.steam.push([x, y + 0.1, z]);
            s += 90.0 + self.rng() * 140.0;
        }
    }

    /// Wet road: soft coloured streaks on the asphalt under signs and lamps.
    pub(super) fn build_reflections(&mut self) {
        let t = self.t;
        let hw = self.g.hw;
        let mut pos: Vec<f64> = Vec::new();
        let mut uv: Vec<f64> = Vec::new();
        let mut col_: Vec<f64> = Vec::new();
        for l in &self.sign_lights {
            let r = t.distance_to_road(l.x, l.z, 30.0);
            if r.i < 0 || r.d > hw + 9.0 {
                continue;
            }
            let rs = r.s.unwrap_or(f64::NAN);
            let f = t.frame(rs);
            let side = if r.lat >= 0.0 { 1.0 } else { -1.0 };
            let tmp = Color::style(l.col);
            let k = if l.lamp || l.big { 0.12 } else { 0.3 };
            let c = [tmp.r * k, tmp.g * k, tmp.b * k];
            // A streak along the road under the light, as a wet surface smears it
            // toward the viewer.
            let lat = side * (hw - (if l.lamp { 2.6 } else { 1.6 }));
            let h = if l.lamp { 1.0 } else { 0.8 };
            let len = if l.lamp { 4.5 } else { 6.0 };
            let cx = f.x + f.rx * lat;
            let cz = f.z + f.rz * lat;
            let p = [
                [cx - f.fx * len - f.rx * h, cz - f.fz * len - f.rz * h],
                [cx + f.fx * len - f.rx * h, cz + f.fz * len - f.rz * h],
                [cx + f.fx * len + f.rx * h, cz + f.fz * len + f.rz * h],
                [cx - f.fx * len + f.rx * h, cz - f.fz * len + f.rz * h],
            ];
            let u = [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]];
            let y = [rs - len, rs + len, rs + len, rs - len].map(|ss| t.surface_y(ss, 0.0) + 0.035);
            let order = if (p[1][1] - p[0][1]) * (p[2][0] - p[0][0])
                - (p[1][0] - p[0][0]) * (p[2][1] - p[0][1])
                >= 0.0
            {
                [0, 1, 2, 0, 2, 3]
            } else {
                [0, 2, 1, 0, 3, 2]
            };
            for q in order {
                pos.extend_from_slice(&[p[q][0], y[q], p[q][1]]);
                uv.extend_from_slice(&u[q]);
                col_.extend_from_slice(&c);
            }
        }
        if pos.is_empty() {
            return;
        }
        let mut g = BufferGeometry::new();
        g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
        g.set_attribute("uv", BufferAttribute::from_f64(&uv, 2));
        g.set_attribute("color", BufferAttribute::from_f64(&col_, 3));
        g.compute_bounding_sphere();
        let glow = self.glow_tex();
        let m = self.graph.add_material(
            Material::basic()
                .set("map", glow)
                .set("vertexColors", true)
                .set("transparent", true)
                .set("depthWrite", false)
                .set("blending", ADDITIVE)
                .set("polygonOffset", true)
                .set("polygonOffsetFactor", -4.0)
                .set("polygonOffsetUnits", -4.0),
        );
        let geo = self.graph.add_geometry(g);
        let mesh = static_mesh(
            self.graph,
            geo,
            m,
            &StaticOpts {
                receive: false,
                ..StaticOpts::default()
            },
        );
        self.graph.get_mut(mesh).render_order = 2.0;
        self.graph.add(self.group, mesh);
    }
}
