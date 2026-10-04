//! Port of `src/world/desert/parts.js` (roadmap WP 7.3): the Desert Run
//! geometry kit for plants, rock pinnacles and railway rolling stock.
//! Everything is vertex-coloured and merged into one geometry per object,
//! so the scenery can instance it.
//!
//! Random choices draw from the `rng` passed in, in the JS order. Where the
//! JS leaves `paint`'s generator out it is `Math.random`; that only happens
//! with no jitter, where the draw cannot change a colour, so the port draws
//! nothing there (`None`).

// The kit keeps the JS argument lists and index loops (DECISIONS D52, D130).
#![allow(clippy::too_many_arguments, clippy::needless_range_loop)]
// The JS's own constants (a 3.14 m width, 6.283 for a turn) stay as written:
// the values must be the JS's, not π or τ.
#![allow(clippy::approx_constant)]

use std::f64::consts::PI;

use mr_math::{Mulberry32, Noise2D, js, lerp};
use serde_json::Value;

use crate::color::Color;
use crate::material::{Material, texture_value};
use crate::object::{Layer, SceneGraph};
use crate::textures::TextureCache;
use crate::three_geom::{
    BufferAttribute, BufferGeometry, Quaternion, Vector3, box_geometry, cone_geometry,
    cylinder_geometry, dodecahedron_geometry, icosahedron_geometry, merge_geometries,
    merge_vertices, octahedron_geometry, sphere_geometry,
};
use mr_scene::MaterialKind;

pub(crate) fn v3(x: f64, y: f64, z: f64) -> Vector3 {
    Vector3::new(x, y, z)
}

pub(crate) const UP: Vector3 = Vector3::new(0.0, 1.0, 0.0);

/// `new THREE.CylinderGeometry(rt, rb, h, radial, heightSeg, open)` over the
/// full turn.
pub(crate) fn cyl(rt: f64, rb: f64, h: f64, radial: f64, hs: f64, open: bool) -> BufferGeometry {
    cylinder_geometry(rt, rb, h, radial, hs, open, 0.0, 2.0 * PI)
}

/// `new THREE.ConeGeometry(r, h, radial, heightSeg, open)` over the full
/// turn.
pub(crate) fn cone(r: f64, h: f64, radial: f64, hs: f64, open: bool) -> BufferGeometry {
    cone_geometry(r, h, radial, hs, open, 0.0, 2.0 * PI)
}

/// `new THREE.BoxGeometry(w, h, d)`.
pub(crate) fn boxg(w: f64, h: f64, d: f64) -> BufferGeometry {
    box_geometry(w, h, d, 1.0, 1.0, 1.0)
}

/// `new THREE.SphereGeometry(r, ws, hs, 0, 2π, 0, thetaLength)`.
pub(crate) fn sphere(r: f64, ws: f64, hs: f64, theta_length: f64) -> BufferGeometry {
    sphere_geometry(r, ws, hs, 0.0, PI * 2.0, 0.0, theta_length)
}

/// Non-indexed, uv-free, flat-shaded (`prep(geo)`; every caller leaves
/// `smooth` false, so the normals are always computed).
pub fn prep(geo: BufferGeometry) -> BufferGeometry {
    let mut g = if geo.index.is_some() {
        geo.to_non_indexed()
    } else {
        geo
    };
    if g.has_attribute("uv") {
        g.delete_attribute("uv");
    }
    g.compute_vertex_normals();
    g
}

/// `paint(geo, hex, jitter, rng)`: a colour per triangle, jittered in
/// brightness by `rng`. `None` is the JS default `Math.random`, passed only
/// with no jitter (see the module comment).
pub fn paint(
    mut geo: BufferGeometry,
    hex: u32,
    jitter: f64,
    mut rng: Option<&mut Mulberry32>,
) -> BufferGeometry {
    let c = Color::hex(hex);
    let n = geo.position().count();
    let mut a = vec![0f32; n * 3];
    let mut i = 0;
    while i < n {
        let k = match rng.as_deref_mut() {
            Some(r) => 1.0 + (r.next_f64() - 0.5) * jitter,
            None => {
                assert!(jitter == 0.0, "paint: Math.random with jitter");
                1.0
            }
        };
        let mut j = 0;
        while j < 3 && i + j < n {
            a[(i + j) * 3] = (c.r * k) as f32;
            a[(i + j) * 3 + 1] = (c.g * k) as f32;
            a[(i + j) * 3 + 2] = (c.b * k) as f32;
            j += 1;
        }
        i += 3;
    }
    geo.set_attribute("color", BufferAttribute::from_f32(a, 3));
    geo
}

/// `merge(parts)`: one geometry, its bounding sphere computed.
pub(crate) fn merge(parts: Vec<BufferGeometry>) -> BufferGeometry {
    let refs: Vec<&BufferGeometry> = parts.iter().collect();
    let mut g = merge_geometries(&refs, false).expect("the parts share their attributes");
    g.compute_bounding_sphere();
    g
}

/// A tapered limb from a to b (world-ish local coords).
pub(crate) fn limb(a: Vector3, b: Vector3, r0: f64, r1: f64, seg: f64) -> BufferGeometry {
    let dir = b - a;
    let len = dir.length();
    let mut g = cyl(r1, r0, len, seg, 1.0, true);
    g.translate(0.0, len / 2.0, 0.0);
    g.apply_quaternion(Quaternion::from_unit_vectors(UP, dir.normalize()));
    g.translate(a.x, a.y, a.z);
    g
}

/// `new THREE.Color(hex).multiplyScalar(k).getHex()`.
pub(crate) fn darker(hex: u32, k: f64) -> u32 {
    let mut c = Color::hex(hex);
    c.multiply_scalar(k);
    c.get_hex()
}

// ── Sandstone ───────────────────────────────────────────────────────────

/// `sandstoneMaterial(key)`: triplanar strata, tinted by vertex and
/// instance colour. The patch (kind `Sandstone`) samples `tRock` on the
/// three axes and darkens steep faces with the stretched `tDetail` noise
/// (desert varnish); its GLSL is the renderer's.
pub fn sandstone_material(
    graph: &mut SceneGraph,
    textures: &mut TextureCache,
    key: &str,
) -> Material {
    let rock = graph.cached_texture(&textures.rock_texture(), Layer::Main, "");
    let detail = graph.cached_texture(&textures.detail_texture(), Layer::Main, "");
    Material::standard()
        .set("color", 0xffffff)
        .set("roughness", 0.95)
        .set("metalness", 0.0)
        .set("vertexColors", true)
        .kind(MaterialKind::Sandstone, None)
        .program_key(key)
        .uniform("tRock", texture_value(rock))
        .uniform("tDetail", texture_value(detail))
        .uniform("clippingPlanes", Value::Null)
}

// ── Plants ──────────────────────────────────────────────────────────────

/// Joshua tree: a shaggy trunk forking into crooked arms, each ending in a
/// spiky rosette. ~6–9 m tall at unit scale.
pub fn joshua_geometry(seed: u32) -> BufferGeometry {
    struct J {
        rng: Mulberry32,
        parts: Vec<BufferGeometry>,
    }
    const BARK: u32 = 0x6b5a48;
    const LEAF: u32 = 0x6f7a3c;
    const DEAD: u32 = 0x5e4e3c;
    // A burst of stiff dagger leaves: slim three-sided spikes fanning up
    // and out from the branch tip, over a small dark core.
    fn rosette(j: &mut J, at: Vector3, s: f64) {
        let mut core = octahedron_geometry(0.28 * s, 0.0);
        core.translate(at.x, at.y + 0.2 * s, at.z);
        let p = paint(prep(core), 0x4e5a2c, 0.2, Some(&mut j.rng));
        j.parts.push(p);
        for i in 0..8 {
            let az = (i as f64 / 8.0) * PI * 2.0 + j.rng.next_f64() * 0.6;
            let up = if i < 3 {
                lerp(0.75, 0.95, j.rng.next_f64())
            } else {
                lerp(0.2, 0.7, j.rng.next_f64())
            };
            let dir = v3(kernel_cos(az) * (1.0 - up), up, kernel_sin(az) * (1.0 - up)).normalize();
            let l = lerp(0.6, 0.85, j.rng.next_f64()) * s;
            let mut b = cone(0.07 * s, l, 3.0, 1.0, true);
            b.translate(0.0, l / 2.0, 0.0);
            b.apply_quaternion(Quaternion::from_unit_vectors(UP, dir));
            b.translate(at.x, at.y + 0.2 * s, at.z);
            let col = [LEAF, 0x7c8a44, 0x62703a][i % 3];
            let p = paint(prep(b), col, 0.2, Some(&mut j.rng));
            j.parts.push(p);
        }
        // Dead leaves hanging below: a skirt.
        let mut sk = cyl(0.24 * s, 0.12 * s, 0.5 * s, 5.0, 1.0, true);
        sk.translate(at.x, at.y - 0.1 * s, at.z);
        let p = paint(prep(sk), DEAD, 0.2, Some(&mut j.rng));
        j.parts.push(p);
    }
    fn grow(j: &mut J, a: Vector3, dir: Vector3, len: f64, r: f64, depth: i32) {
        let b = a.add_scaled_vector(dir, len);
        let p = paint(
            prep(limb(a, b, r, r * 0.8, 5.0)),
            BARK,
            0.2,
            Some(&mut j.rng),
        );
        j.parts.push(p);
        if depth <= 0 || (depth < 2 && j.rng.next_f64() < 0.25) {
            rosette(j, b, 0.8 + r * 1.2);
            return;
        }
        let k = 2 + if j.rng.next_f64() < 0.4 { 1 } else { 0 };
        for i in 0..k {
            let az = (i as f64 / k as f64) * PI * 2.0 + j.rng.next_f64() * 1.2;
            let up = lerp(0.35, 0.9, j.rng.next_f64());
            let d2 = v3(kernel_cos(az) * (1.0 - up), up, kernel_sin(az) * (1.0 - up)).normalize();
            let l2 = len * lerp(0.55, 0.8, j.rng.next_f64());
            grow(j, b, d2, l2, r * 0.72, depth - 1);
        }
    }
    let mut j = J {
        rng: Mulberry32::new(seed),
        parts: Vec::new(),
    };
    let dx = (j.rng.next_f64() - 0.5) * 0.2;
    let dz = (j.rng.next_f64() - 0.5) * 0.2;
    let dir = v3(dx, 1.0, dz).normalize();
    let len = lerp(2.2, 3.2, j.rng.next_f64());
    let depth = 2 + if j.rng.next_f64() < 0.5 { 1 } else { 0 };
    grow(&mut j, v3(0.0, 0.0, 0.0), dir, len, 0.26, depth);
    merge(j.parts)
}

fn kernel_cos(x: f64) -> f64 {
    mr_math::kernel::cos(x)
}

fn kernel_sin(x: f64) -> f64 {
    mr_math::kernel::sin(x)
}

/// Saguaro: ribbed column with up-turned arms.
pub fn saguaro_geometry(seed: u32) -> BufferGeometry {
    let mut rng = Mulberry32::new(seed);
    let mut parts = Vec::new();
    const GREEN: u32 = 0x55703e;
    let h = lerp(6.0, 10.0, rng.next_f64());
    let col = |parts: &mut Vec<BufferGeometry>, rng: &mut Mulberry32, a, b, r: f64| {
        parts.push(paint(
            prep(limb(a, b, r, r * 0.95, 10.0)),
            GREEN,
            0.12,
            Some(rng),
        ));
    };
    let top = |parts: &mut Vec<BufferGeometry>, rng: &mut Mulberry32, at: Vector3, r: f64| {
        let mut s = sphere(r * 0.95, 10.0, 5.0, PI / 2.0);
        s.translate(at.x, at.y, at.z);
        parts.push(paint(prep(s), GREEN, 0.12, Some(rng)));
    };
    col(
        &mut parts,
        &mut rng,
        v3(0.0, 0.0, 0.0),
        v3(0.0, h, 0.0),
        0.36,
    );
    top(&mut parts, &mut rng, v3(0.0, h, 0.0), 0.36);
    let arms = (rng.next_f64() * 4.0).floor() as usize;
    for _ in 0..arms {
        let az = rng.next_f64() * PI * 2.0;
        let y0 = lerp(2.2, h * 0.6, rng.next_f64());
        let out = lerp(0.9, 1.5, rng.next_f64());
        let dx = kernel_cos(az);
        let dz = kernel_sin(az);
        let a = v3(dx * 0.2, y0, dz * 0.2);
        let b = v3(dx * out, y0 + 0.5, dz * out);
        col(&mut parts, &mut rng, a, b, 0.24);
        let c = v3(dx * out, y0 + lerp(1.8, 3.5, rng.next_f64()), dz * out);
        col(&mut parts, &mut rng, b, c, 0.24);
        top(&mut parts, &mut rng, c, 0.24);
    }
    merge(parts)
}

/// Creosote / brittlebush: a loose low mound (unit radius), vertex-coloured
/// white so instances carry the colour.
pub fn bush_geometry(seed: u32) -> BufferGeometry {
    let n = Noise2D::new(seed);
    let mut rng = Mulberry32::new(seed);
    let mut parts = Vec::new();
    // A loose clump of three or four small mounds, ~1 m tall at unit scale.
    let k = 4;
    for j in 0..k {
        // Only the central mound needs the finer sphere; the soft normals
        // below hide the facets on the small ones.
        let mut g = icosahedron_geometry(1.0, if j == 0 { 1.0 } else { 0.0 });
        {
            let p = g.get_attribute_mut("position").expect("position");
            for i in 0..p.count() {
                let (x, y, z) = (p.get_x(i), p.get_y(i), p.get_z(i));
                let r = 1.0 + n.noise(x * 2.1 + z + j as f64, y * 2.3) * 0.35;
                p.set_xyz(i, x * r, js::max(y * r, -0.2), z * r);
            }
        }
        let a = (j as f64 / k as f64) * PI * 2.0 + rng.next_f64();
        let s = if j == 0 {
            0.62
        } else {
            lerp(0.35, 0.5, rng.next_f64())
        };
        let hs = s * lerp(0.9, 1.3, rng.next_f64());
        g.scale(s, hs, s);
        // Sit each mound on the ground (its underside was clamped at -0.2)
        // rather than floating it up on a shadow like a mushroom.
        g.translate(
            if j == 0 { 0.0 } else { kernel_cos(a) * 0.5 },
            hs * 0.12,
            if j == 0 { 0.0 } else { kernel_sin(a) * 0.5 },
        );
        // Soft, rounded shading: weld and smooth the normals.
        g.delete_attribute("normal");
        g.delete_attribute("uv");
        let mut w = merge_vertices(&g, 1e-4);
        w.compute_vertex_normals();
        let mut flat = w.to_non_indexed();
        // Foliage normals: lean them toward "out from the whole clump" so the
        // bush shades like one soft mound rather than faceted lumps.
        let cnt = flat.position().count();
        for i in 0..cnt {
            let (px, py, pz) = {
                let p2 = flat.position();
                (p2.get_x(i), p2.get_y(i), p2.get_z(i))
            };
            let nn = flat.get_attribute_mut("normal").expect("normal");
            let nv = v3(nn.get_x(i), nn.get_y(i), nn.get_z(i));
            let vv = v3(px, py + 0.3, pz).normalize().lerp(nv, 0.4).normalize();
            nn.set_xyz(i, vv.x, vv.y, vv.z);
        }
        parts.push(paint(flat, 0xffffff, 0.25, Some(&mut rng)));
    }
    merge(parts)
}

/// Juniper: a twisted trunk and a few dark foliage clumps.
pub fn juniper_geometry(seed: u32) -> BufferGeometry {
    let mut rng = Mulberry32::new(seed);
    let mut parts = Vec::new();
    const BARK: u32 = 0x5a4636;
    // A short twisted trunk splitting into crooked limbs, with ragged
    // blue-green foliage along and at the ends of them.
    let base = v3(0.0, 0.0, 0.0);
    let fx = (rng.next_f64() - 0.5) * 0.4;
    let fz = (rng.next_f64() - 0.5) * 0.4;
    let fork = v3(fx, 0.9, fz);
    parts.push(paint(
        prep(limb(base, fork, 0.2, 0.15, 5.0)),
        BARK,
        0.2,
        Some(&mut rng),
    ));
    let clump =
        |parts: &mut Vec<BufferGeometry>, rng: &mut Mulberry32, at: Vector3, sc: f64, i: usize| {
            let mut b = dodecahedron_geometry(1.0, 0.0);
            let sx = sc * lerp(0.9, 1.3, rng.next_f64());
            let sy = sc * lerp(0.55, 0.8, rng.next_f64());
            let sz = sc * lerp(0.9, 1.3, rng.next_f64());
            b.scale(sx, sy, sz);
            b.rotate_y(rng.next_f64() * 3.0);
            b.translate(at.x, at.y, at.z);
            let col = [0x4a5a3c, 0x55664a, 0x3e4c34, 0x5c6a50][i % 4];
            parts.push(paint(prep(b), col, 0.25, Some(rng)));
        };
    let limbs = 2 + (rng.next_f64() * 2.0).floor() as usize;
    let mut ci = 0;
    for i in 0..limbs {
        let az = (i as f64 / limbs as f64) * PI * 2.0 + rng.next_f64();
        let ex = kernel_cos(az) * lerp(0.8, 1.5, rng.next_f64());
        let ey = lerp(0.6, 1.6, rng.next_f64());
        let ez = kernel_sin(az) * lerp(0.8, 1.5, rng.next_f64());
        let end = fork + v3(ex, ey, ez);
        parts.push(paint(
            prep(limb(fork, end, 0.12, 0.07, 4.0)),
            BARK,
            0.2,
            Some(&mut rng),
        ));
        let sc = lerp(0.7, 1.0, rng.next_f64());
        clump(&mut parts, &mut rng, end, sc, ci);
        ci += 1;
        let sc = lerp(0.5, 0.7, rng.next_f64());
        clump(
            &mut parts,
            &mut rng,
            fork.lerp(end, 0.5) + v3(0.0, 0.25, 0.0),
            sc,
            ci,
        );
        ci += 1;
    }
    clump(&mut parts, &mut rng, fork + v3(0.0, 1.1, 0.0), 0.8, ci);
    merge(parts)
}

/// Yucca: a rosette of stiff blades.
pub fn yucca_geometry(seed: u32) -> BufferGeometry {
    let mut rng = Mulberry32::new(seed);
    let mut parts = Vec::new();
    for i in 0..16 {
        let az = (i as f64 / 16.0) * PI * 2.0 + rng.next_f64() * 0.3;
        let up = lerp(0.5, 0.95, rng.next_f64());
        let dir = v3(kernel_cos(az) * (1.0 - up), up, kernel_sin(az) * (1.0 - up)).normalize();
        let mut b = cone(0.06, lerp(0.8, 1.2, rng.next_f64()), 3.0, 1.0, false);
        b.translate(0.0, 0.5, 0.0);
        b.apply_quaternion(Quaternion::from_unit_vectors(UP, dir));
        parts.push(paint(prep(b), 0x7d8a52, 0.3, Some(&mut rng)));
    }
    merge(parts)
}

/// Washingtonia fan palm for the oasis: tall slim trunk, a shaggy skirt of
/// dead fronds, and a round crown.
pub fn palm_geometry(seed: u32) -> BufferGeometry {
    let mut rng = Mulberry32::new(seed);
    let mut parts = Vec::new();
    let h = lerp(10.0, 15.0, rng.next_f64());
    let lx = (rng.next_f64() - 0.5) * 0.6;
    let lz = (rng.next_f64() - 0.5) * 0.6;
    let lean = v3(lx, h, lz);
    parts.push(paint(
        prep(limb(v3(0.0, 0.0, 0.0), lean, 0.32, 0.24, 7.0)),
        0x7a6650,
        0.15,
        Some(&mut rng),
    ));
    let mut sk = cyl(0.75, 0.42, 3.2, 8.0, 1.0, true);
    sk.translate(lean.x, h - 1.7, lean.z);
    parts.push(paint(prep(sk), 0x9a8058, 0.2, Some(&mut rng)));
    // Fan leaves: a stalk out from the crown, then a pleated fan that droops.
    const N: usize = 22;
    for i in 0..N {
        let az = (i as f64 / N as f64) * PI * 2.0 + rng.next_f64() * 0.25;
        let up = lerp(
            -0.55,
            0.45,
            (i % 3) as f64 / 2.0 * 0.7 + rng.next_f64() * 0.3,
        );
        let dir = v3(kernel_cos(az), up, kernel_sin(az)).normalize();
        let side = v3(-kernel_sin(az), 0.0, kernel_cos(az));
        let a = v3(lean.x, h, lean.z);
        let b = a.add_scaled_vector(dir, 1.3);
        // Fan: a triangle fan of blades spreading from b, drooping at the tips.
        let mut pos: Vec<f64> = Vec::new();
        let blades = 6;
        let l = lerp(1.6, 2.2, rng.next_f64());
        let spread = 1.1;
        let tip = |k: usize| {
            let t = k as f64 / blades as f64 - 0.5;
            b.add_scaled_vector(dir, l * (1.0 - t.abs() * 0.4))
                .add_scaled_vector(side, t * spread * 2.0)
                + v3(0.0, -0.5 - t.abs() * 0.3, 0.0)
        };
        for k in 0..blades {
            let p0 = tip(k);
            let p1 = tip(k + 1);
            pos.extend_from_slice(&[b.x, b.y, b.z, p0.x, p0.y, p0.z, p1.x, p1.y, p1.z]);
            // both faces
            pos.extend_from_slice(&[b.x, b.y, b.z, p1.x, p1.y, p1.z, p0.x, p0.y, p0.z]);
        }
        let mut f = BufferGeometry::new();
        f.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
        f.compute_vertex_normals();
        parts.push(paint(
            f,
            [0x5a7438, 0x668040, 0x4e6630][i % 3],
            0.2,
            Some(&mut rng),
        ));
        parts.push(paint(
            prep(limb(a, b, 0.05, 0.04, 3.0)),
            0x6a6a3c,
            0.0,
            Some(&mut rng),
        ));
    }
    merge(parts)
}

// ── Course furniture ────────────────────────────────────────────────────

/// Traffic cone: orange with a white reflective band (unit ≈ 0.75 m tall).
pub fn cone_marker_geometry() -> BufferGeometry {
    let mut parts = Vec::new();
    let mut base = boxg(0.42, 0.04, 0.42);
    base.translate(0.0, 0.02, 0.0);
    parts.push(paint(prep(base), 0x1a1a1a, 0.0, None));
    let mut lo = cyl(0.12, 0.17, 0.28, 10.0, 1.0, true);
    lo.translate(0.0, 0.18, 0.0);
    let mut band = cyl(0.085, 0.12, 0.2, 10.0, 1.0, true);
    band.translate(0.0, 0.42, 0.0);
    let mut hi = cyl(0.02, 0.085, 0.26, 10.0, 1.0, true);
    hi.translate(0.0, 0.65, 0.0);
    parts.push(paint(prep(lo), 0xff5a14, 0.0, None));
    parts.push(paint(prep(band), 0xf2f2f2, 0.0, None));
    parts.push(paint(prep(hi), 0xff5a14, 0.0, None));
    merge(parts)
}

// ── Railway rolling stock ───────────────────────────────────────────────
// Each car is built along local +X (length), centred, wheels on y = 0.

/// A car: its geometry and length (`{ geo, L }`).
pub struct Car {
    pub geo: BufferGeometry,
    pub l: f64,
}

fn bx(
    parts: &mut Vec<BufferGeometry>,
    w: f64,
    h: f64,
    d: f64,
    x: f64,
    y: f64,
    z: f64,
    hex: u32,
    rng: &mut Mulberry32,
    jit: f64,
) {
    let mut g = boxg(w, h, d);
    g.translate(x, y + h / 2.0, z);
    parts.push(paint(prep(g), hex, jit, Some(rng)));
}

/// Two three-piece trucks: side frames, bolster, wheelsets.
fn trucks(parts: &mut Vec<BufferGeometry>, l: f64, rng: &mut Mulberry32, inset: f64) {
    for x in [-l / 2.0 + inset, l / 2.0 - inset] {
        bx(parts, 2.2, 0.35, 1.9, x, 0.75, 0.0, 0x1c1c1e, rng, 0.04); // bolster
        for z in [-0.86, 0.86] {
            bx(parts, 3.0, 0.42, 0.14, x, 0.3, z, 0x242222, rng, 0.04); // side frame
            bx(parts, 0.9, 0.3, 0.16, x, 0.55, z, 0x2a2826, rng, 0.04); // spring nest
        }
        for dx in [-0.9, 0.9] {
            for dz in [-0.76, 0.76] {
                let mut w = cyl(0.46, 0.46, 0.14, 10.0, 1.0, false);
                w.rotate_x(PI / 2.0);
                w.translate(x + dx, 0.46, dz);
                parts.push(paint(prep(w), 0x3a3230, 0.0, Some(rng)));
            }
        }
    }
    bx(
        parts,
        l - 0.8,
        0.35,
        2.7,
        0.0,
        1.05,
        0.0,
        0x25221f,
        rng,
        0.04,
    ); // underframe
    // Couplers.
    for s in [-1.0, 1.0] {
        bx(
            parts,
            0.7,
            0.25,
            0.3,
            s * (l / 2.0 + 0.2),
            1.0,
            0.0,
            0x2a2826,
            rng,
            0.04,
        );
    }
}

/// Side ribs every `step` metres along both faces.
fn ribs(
    parts: &mut Vec<BufferGeometry>,
    l: f64,
    h: f64,
    y: f64,
    z: f64,
    hex: u32,
    rng: &mut Mulberry32,
    step: f64,
    t: f64,
) {
    let mut x = -l / 2.0 + step / 2.0;
    while x < l / 2.0 {
        for s in [-1.0, 1.0] {
            bx(parts, t, h, t, x, y, s * z, hex, rng, 0.02);
        }
        x += step;
    }
}

/// Corner ladders with grab irons.
fn ladders(parts: &mut Vec<BufferGeometry>, l: f64, y0: f64, z: f64, rng: &mut Mulberry32) {
    let hex = 0xd8c040;
    for sx in [-1.0, 1.0] {
        for sz in [-1.0, 1.0] {
            let x = sx * (l / 2.0 - 0.35);
            for dx in [-0.22, 0.22] {
                bx(
                    parts,
                    0.04,
                    2.6,
                    0.04,
                    x + dx,
                    y0,
                    sz * z,
                    0x2a2a2a,
                    rng,
                    0.0,
                );
            }
            for k in 0..6 {
                bx(
                    parts,
                    0.5,
                    0.04,
                    0.05,
                    x,
                    y0 + 0.3 + k as f64 * 0.42,
                    sz * z,
                    hex,
                    rng,
                    0.0,
                );
            }
        }
    }
}

/// GE-style wide-cab road diesel: pilot and nose, four-window cab, long hood
/// with radiator wings at the back, walkways and handrails, fuel tank.
pub fn locomotive_geometry(scheme: u32) -> Car {
    let mut rng = Mulberry32::new(66 + scheme);
    let rng = &mut rng;
    let mut parts = Vec::new();
    let p = &mut parts;
    let l = 22.5;
    let [body, top, stripe] = [
        [0xd8741c, 0x1e1e20, 0xf2d24a],
        [0x1f4a8a, 0xe8e4da, 0xc8322a],
    ][(scheme % 2) as usize];
    trucks(p, l, rng, 3.3);
    // Fuel tank slung between the trucks.
    let mut ft = cyl(0.75, 0.75, 7.6, 10.0, 1.0, false);
    ft.rotate_z(PI / 2.0);
    ft.scale(1.0, 0.8, 1.6);
    ft.translate(-0.4, 0.95, 0.0);
    p.push(paint(prep(ft), 0x2a2a2c, 0.03, Some(rng)));
    bx(p, l, 0.28, 3.1, 0.0, 1.35, 0.0, 0x2a2a2c, rng, 0.04); // deck
    bx(p, l - 0.2, 0.18, 3.14, 0.0, 1.3, 0.0, stripe, rng, 0.04); // sill stripe
    // Long hood, narrower than the deck so the walkways show.
    let hx0 = -l / 2.0 + 0.8;
    let hx1 = l / 2.0 - 6.2;
    bx(
        p,
        hx1 - hx0,
        2.75,
        2.3,
        (hx0 + hx1) / 2.0,
        1.63,
        0.0,
        body,
        rng,
        0.04,
    );
    bx(
        p,
        hx1 - hx0,
        0.3,
        2.34,
        (hx0 + hx1) / 2.0,
        4.38,
        0.0,
        top,
        rng,
        0.04,
    );
    // Hood doors: shallow panels and louvres along the sides.
    let mut x = hx0 + 0.8;
    while x < hx1 - 0.6 {
        for s in [-1.0, 1.0] {
            bx(
                p,
                1.1,
                1.9,
                0.04,
                x,
                2.0,
                s * 1.16,
                darker(body, 0.88),
                rng,
                0.02,
            );
            bx(p, 0.7, 0.3, 0.05, x, 3.5, s * 1.17, 0x2a2a2a, rng, 0.0);
        }
        x += 1.3;
    }
    // Radiator wings and fans at the back.
    bx(p, 3.8, 1.3, 3.02, hx0 + 2.1, 3.4, 0.0, body, rng, 0.04);
    bx(p, 3.8, 0.12, 3.04, hx0 + 2.1, 4.7, 0.0, top, rng, 0.04);
    for s in [-1.0, 1.0] {
        bx(
            p,
            3.4,
            1.0,
            0.05,
            hx0 + 2.1,
            3.55,
            s * 1.52,
            0x2e2e30,
            rng,
            0.0,
        );
    }
    for x in [hx0 + 1.2, hx0 + 3.0] {
        let mut fan = cyl(0.7, 0.7, 0.14, 12.0, 1.0, false);
        fan.translate(x, 4.85, 0.0);
        p.push(paint(prep(fan), 0x2a2a2a, 0.0, Some(rng)));
    }
    // Exhaust stack and dynamic brake blister.
    bx(p, 0.9, 0.5, 0.5, hx0 + 6.5, 4.6, 0.0, 0x202022, rng, 0.04);
    bx(
        p,
        3.2,
        0.45,
        2.0,
        hx0 + 9.5,
        4.6,
        0.0,
        darker(body, 0.9),
        rng,
        0.04,
    );
    // Cab.
    let cx0 = l / 2.0 - 6.2;
    let cx1 = l / 2.0 - 2.4;
    bx(
        p,
        cx1 - cx0,
        3.0,
        3.06,
        (cx0 + cx1) / 2.0,
        1.63,
        0.0,
        body,
        rng,
        0.04,
    );
    bx(
        p,
        cx1 - cx0 + 0.1,
        0.35,
        3.1,
        (cx0 + cx1) / 2.0,
        4.63,
        0.0,
        top,
        rng,
        0.04,
    ); // roof
    bx(
        p,
        cx1 - cx0,
        1.05,
        3.08,
        (cx0 + cx1) / 2.0,
        3.55,
        0.0,
        top,
        rng,
        0.04,
    ); // window band
    for s in [-1.0, 1.0] {
        for x in [cx0 + 0.8, cx0 + 2.3] {
            bx(p, 1.1, 0.8, 0.04, x, 3.68, s * 1.55, 0x101418, rng, 0.0);
        }
    }
    // Windshield: two dark panes raked back.
    for z in [-0.72, 0.72] {
        let mut w = boxg(0.06, 0.9, 1.25);
        w.rotate_z(0.12);
        w.translate(cx1 + 0.03, 4.1, z);
        p.push(paint(prep(w), 0x0e1216, 0.0, Some(rng)));
    }
    // Short nose with a sloped top.
    bx(p, 2.0, 1.55, 2.7, cx1 + 1.0, 1.63, 0.0, body, rng, 0.04);
    let mut slope = boxg(2.1, 0.12, 2.72);
    slope.rotate_z(-0.35);
    slope.translate(cx1 + 0.95, 3.35, 0.0);
    p.push(paint(prep(slope), body, 0.03, Some(rng)));
    bx(p, 0.12, 0.35, 2.4, cx1 + 1.95, 2.55, 0.0, stripe, rng, 0.04); // nose chevron band
    bx(p, 0.1, 0.4, 1.0, cx1 + 2.02, 2.95, 0.0, 0xf0f0e0, rng, 0.0); // headlight bar
    // Pilot / snowplough, black-and-yellow.
    let mut plow = boxg(0.5, 0.9, 3.0);
    plow.rotate_z(0.35);
    plow.translate(l / 2.0 + 0.15, 0.75, 0.0);
    p.push(paint(prep(plow), 0x1c1c1c, 0.0, Some(rng)));
    bx(
        p,
        0.2,
        0.22,
        3.12,
        l / 2.0 - 0.05,
        1.1,
        0.0,
        stripe,
        rng,
        0.04,
    );
    // Walkway handrails and stanchions, both sides and across the ends.
    for s in [-1.0, 1.0] {
        bx(
            p,
            hx1 - hx0 + 0.6,
            0.05,
            0.05,
            (hx0 + hx1) / 2.0,
            2.55,
            s * 1.5,
            stripe,
            rng,
            0.0,
        );
        let mut x = hx0;
        while x <= hx1 {
            bx(p, 0.05, 1.05, 0.05, x, 1.5, s * 1.5, stripe, rng, 0.0);
            x += 2.2;
        }
        // Steps at the corners.
        for x in [-l / 2.0 + 0.6, l / 2.0 - 1.2] {
            bx(p, 0.7, 0.9, 0.06, x, 0.45, s * 1.5, 0x2a2a2a, rng, 0.0);
        }
    }
    bx(
        p,
        0.05,
        0.05,
        3.0,
        -l / 2.0 + 0.3,
        2.55,
        0.0,
        stripe,
        rng,
        0.0,
    );
    // Horn and antenna on the cab roof.
    bx(p, 0.6, 0.2, 0.2, cx0 + 1.2, 4.98, 0.4, 0x6a6a6a, rng, 0.0);
    Car {
        geo: merge(parts),
        l,
    }
}

pub fn boxcar_geometry(hex: u32, seed: u32) -> Car {
    let mut rng = Mulberry32::new(seed);
    let rng = &mut rng;
    let mut parts = Vec::new();
    let p = &mut parts;
    let l = 17.0;
    trucks(p, l, rng, 2.4);
    bx(p, l, 3.3, 2.9, 0.0, 1.25, 0.0, hex, rng, 0.04);
    bx(
        p,
        l + 0.05,
        0.15,
        3.0,
        0.0,
        4.55,
        0.0,
        darker(hex, 0.7),
        rng,
        0.04,
    ); // roof
    ribs(p, l, 3.2, 1.3, 1.47, darker(hex, 0.85), rng, 1.3, 0.08);
    bx(
        p,
        3.4,
        3.0,
        3.04,
        0.0,
        1.3,
        0.0,
        darker(hex, 0.78),
        rng,
        0.04,
    ); // plug door
    bx(p, 7.5, 0.1, 3.08, 0.0, 4.25, 0.0, 0x2a2826, rng, 0.0); // door track
    bx(p, 0.5, 0.35, 0.2, 5.5, 3.4, 1.5, 0xe8e2d4, rng, 0.0); // reporting marks
    bx(p, 0.5, 0.35, 0.2, -5.5, 3.4, -1.5, 0xe8e2d4, rng, 0.0);
    bx(
        p,
        0.6,
        0.06,
        1.0,
        l / 2.0 - 0.4,
        4.7,
        0.0,
        0x2a2826,
        rng,
        0.0,
    ); // roof walk ends
    ladders(p, l, 1.4, 1.5, rng);
    Car {
        geo: merge(parts),
        l,
    }
}

pub fn tank_geometry(hex: u32, seed: u32) -> Car {
    let mut rng = Mulberry32::new(seed);
    let rng = &mut rng;
    let mut parts = Vec::new();
    let p = &mut parts;
    let l = 16.0;
    trucks(p, l, rng, 2.4);
    let mut t = cyl(1.45, 1.45, l - 1.2, 14.0, 1.0, false);
    t.rotate_z(PI / 2.0);
    t.translate(0.0, 2.8, 0.0);
    p.push(paint(prep(t), hex, 0.03, Some(rng)));
    for x in [-(l - 1.2) / 2.0, (l - 1.2) / 2.0] {
        let mut cap = sphere(1.45, 12.0, 6.0, PI / 2.0);
        cap.scale(0.35, 1.0, 1.0);
        cap.rotate_z(if x > 0.0 { -PI / 2.0 } else { PI / 2.0 });
        cap.translate(x, 2.8, 0.0);
        p.push(paint(prep(cap), hex, 0.03, Some(rng)));
    }
    // Tank bands, walkway and the dome with its platform.
    for x in [-4.5, 0.0, 4.5] {
        let mut b = cyl(1.48, 1.48, 0.12, 14.0, 1.0, true);
        b.rotate_z(PI / 2.0);
        b.translate(x, 2.8, 0.0);
        p.push(paint(prep(b), darker(hex, 0.7), 0.0, Some(rng)));
    }
    bx(p, 1.2, 0.5, 1.2, 0.0, 4.2, 0.0, 0x3a3634, rng, 0.04);
    bx(p, 2.4, 0.06, 1.8, 0.0, 4.25, 0.0, 0x2a2826, rng, 0.0);
    bx(p, 0.05, 0.9, 1.8, -1.2, 4.3, 0.0, 0xd8c040, rng, 0.0);
    bx(p, 0.05, 0.9, 1.8, 1.2, 4.3, 0.0, 0xd8c040, rng, 0.0);
    bx(p, 2.2, 0.35, 0.04, -4.5, 2.2, 1.49, 0xd8a020, rng, 0.0); // placard band
    Car {
        geo: merge(parts),
        l,
    }
}

/// Covered hopper: ribbed body with sloped ends, a rounded roof with
/// hatches, three discharge bays underneath.
pub fn hopper_geometry(hex: u32, seed: u32) -> Car {
    let mut rng = Mulberry32::new(seed);
    let rng = &mut rng;
    let mut parts = Vec::new();
    let p = &mut parts;
    let l = 18.0;
    trucks(p, l, rng, 2.4);
    bx(p, l - 2.4, 2.9, 3.1, 0.0, 1.7, 0.0, hex, rng, 0.04);
    for s in [-1.0, 1.0] {
        let mut e = boxg(1.6, 2.4, 3.1);
        e.rotate_z(s * 0.45);
        e.translate(s * (l / 2.0 - 1.1), 2.6, 0.0);
        p.push(paint(prep(e), hex, 0.04, Some(rng)));
    }
    let mut roof = cylinder_geometry(1.55, 1.55, l - 2.4, 12.0, 1.0, false, 0.0, PI);
    roof.rotate_z(PI / 2.0);
    roof.rotate_x(PI / 2.0);
    roof.scale(1.0, 0.32, 1.0);
    roof.translate(0.0, 4.6, 0.0);
    p.push(paint(prep(roof), darker(hex, 0.92), 0.03, Some(rng)));
    let mut x = -6.0;
    while x <= 6.0 {
        bx(p, 0.6, 0.12, 0.6, x, 5.0, 0.0, darker(hex, 0.7), rng, 0.0);
        x += 2.0;
    }
    ribs(p, l - 2.4, 2.8, 1.75, 1.57, darker(hex, 0.8), rng, 1.5, 0.1);
    for x in [-4.5, 0.0, 4.5] {
        let mut c = cone(1.5, 1.2, 4.0, 1.0, false);
        c.rotate_y(PI / 4.0);
        c.rotate_x(PI);
        c.translate(x, 1.3, 0.0);
        p.push(paint(prep(c), hex, 0.05, Some(rng)));
    }
    ladders(p, l, 1.4, 1.58, rng);
    Car {
        geo: merge(parts),
        l,
    }
}

/// What an open gondola is heaped with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Load {
    Scrap,
    Ballast,
}

/// Open gondola, heaped with scrap or ballast.
pub fn gondola_geometry(hex: u32, seed: u32, load: Load) -> Car {
    let mut rng = Mulberry32::new(seed);
    let rng = &mut rng;
    let mut parts = Vec::new();
    let p = &mut parts;
    let l = 16.0;
    trucks(p, l, rng, 2.4);
    bx(p, l, 0.25, 3.0, 0.0, 1.25, 0.0, darker(hex, 0.6), rng, 0.04);
    for s in [-1.0, 1.0] {
        bx(p, l, 1.6, 0.1, 0.0, 1.4, s * 1.45, hex, rng, 0.04);
    }
    for s in [-1.0, 1.0] {
        bx(
            p,
            0.1,
            1.6,
            3.0,
            s * (l / 2.0 - 0.05),
            1.4,
            0.0,
            hex,
            rng,
            0.04,
        );
    }
    ribs(p, l, 1.6, 1.4, 1.52, darker(hex, 0.8), rng, 1.0, 0.1);
    if load == Load::Scrap {
        for i in 0..18 {
            let w = lerp(0.6, 2.2, rng.next_f64());
            let h = lerp(0.2, 0.7, rng.next_f64());
            let d = lerp(0.4, 1.8, rng.next_f64());
            let mut g = boxg(w, h, d);
            g.rotate_y(rng.next_f64() * 3.0);
            g.rotate_x((rng.next_f64() - 0.5) * 0.6);
            g.rotate_z((rng.next_f64() - 0.5) * 0.6);
            let tx = lerp(-l / 2.0 + 1.5, l / 2.0 - 1.5, rng.next_f64());
            let ty = 2.8 + rng.next_f64() * 0.4;
            let tz = (rng.next_f64() - 0.5) * 1.8;
            g.translate(tx, ty, tz);
            let col = [0x6a4a34, 0x5a5a5c, 0x7a5a3a, 0x3a3a3c][i % 4];
            p.push(paint(prep(g), col, 0.2, Some(rng)));
        }
    } else {
        let mut heap = cyl(0.2, 1.4, 0.9, 8.0, 1.0, false);
        heap.scale(l / 3.2, 1.0, 1.0);
        heap.translate(0.0, 3.3, 0.0);
        p.push(paint(prep(heap), 0x8a7a68, 0.15, Some(rng)));
    }
    Car {
        geo: merge(parts),
        l,
    }
}

/// Enclosed autorack: tall, with perforated side panels (light and dark
/// bands suggest the holes).
pub fn autorack_geometry(hex: u32, seed: u32) -> Car {
    let mut rng = Mulberry32::new(seed);
    let rng = &mut rng;
    let mut parts = Vec::new();
    let p = &mut parts;
    let l = 26.0;
    trucks(p, l, rng, 2.4);
    bx(p, l, 5.0, 3.1, 0.0, 1.25, 0.0, hex, rng, 0.04);
    bx(p, l + 0.05, 0.2, 3.14, 0.0, 6.25, 0.0, 0xd8d6d0, rng, 0.04);
    let mut x = -l / 2.0 + 0.7;
    while x < l / 2.0 {
        for s in [-1.0, 1.0] {
            bx(
                p,
                0.9,
                3.4,
                0.04,
                x,
                2.2,
                s * 1.57,
                darker(hex, 0.55),
                rng,
                0.05,
            );
        }
        x += 1.2;
    }
    bx(p, l, 0.25, 3.14, 0.0, 1.3, 0.0, darker(hex, 0.8), rng, 0.04);
    bx(
        p,
        0.1,
        4.6,
        3.1,
        l / 2.0,
        1.4,
        0.0,
        darker(hex, 0.7),
        rng,
        0.04,
    );
    bx(
        p,
        0.1,
        4.6,
        3.1,
        -l / 2.0,
        1.4,
        0.0,
        darker(hex, 0.7),
        rng,
        0.04,
    );
    Car {
        geo: merge(parts),
        l,
    }
}

/// Centre-beam flatcar stacked with wrapped lumber bundles.
pub fn lumber_geometry(seed: u32) -> Car {
    let mut rng = Mulberry32::new(seed);
    let rng = &mut rng;
    let mut parts = Vec::new();
    let p = &mut parts;
    let l = 22.0;
    trucks(p, l, rng, 2.4);
    bx(p, l, 0.35, 3.0, 0.0, 1.25, 0.0, 0x4a4a4c, rng, 0.04);
    bx(
        p,
        0.4,
        3.3,
        3.0,
        -l / 2.0 + 0.3,
        1.6,
        0.0,
        0x4a4a4c,
        rng,
        0.04,
    );
    bx(
        p,
        0.4,
        3.3,
        3.0,
        l / 2.0 - 0.3,
        1.6,
        0.0,
        0x4a4a4c,
        rng,
        0.04,
    );
    bx(p, l - 0.4, 0.3, 0.3, 0.0, 4.6, 0.0, 0x4a4a4c, rng, 0.04);
    let mut x = -l / 2.0 + 1.2;
    while x < l / 2.0 - 1.0 {
        bx(p, 0.15, 3.0, 0.2, x, 1.6, 0.0, 0x4a4a4c, rng, 0.0);
        x += 1.8;
    }
    let cols = [0xe4d8b4, 0xd8c89a, 0xf0ece0, 0xc4ac7a];
    for k in 0..4 {
        for j in 0..3 {
            for s in [-1.0, 1.0] {
                bx(
                    p,
                    4.9,
                    0.95,
                    1.3,
                    -l / 2.0 + 3.1 + k as f64 * 5.2,
                    1.6 + j as f64 * 1.0,
                    s * 0.8,
                    cols[(k + j + if s > 0.0 { 1 } else { 0 }) % 4],
                    rng,
                    0.08,
                );
            }
        }
    }
    Car {
        geo: merge(parts),
        l,
    }
}

pub fn stack_geometry(cols: [u32; 3], seed: u32) -> Car {
    let mut rng = Mulberry32::new(seed);
    let rng = &mut rng;
    let mut parts = Vec::new();
    let p = &mut parts;
    let l = 16.0;
    trucks(p, l, rng, 2.4);
    bx(p, l - 1.0, 0.4, 2.8, 0.0, 0.9, 0.0, 0x2a2826, rng, 0.04);
    // Containers with corrugation ribs and door-end detail.
    let cont =
        |p: &mut Vec<BufferGeometry>, rng: &mut Mulberry32, len: f64, x: f64, y: f64, hex: u32| {
            bx(p, len, 2.6, 2.45, x, y, 0.0, hex, rng, 0.04);
            ribs(
                p,
                len - 0.2,
                2.5,
                y + 0.05,
                1.23,
                darker(hex, 0.82),
                rng,
                0.55,
                0.06,
            );
            bx(
                p,
                len + 0.04,
                0.12,
                2.49,
                x,
                y + 2.5,
                0.0,
                darker(hex, 0.75),
                rng,
                0.0,
            );
        };
    cont(p, rng, 6.05, -3.15, 1.3, cols[0]);
    cont(p, rng, 6.05, 3.15, 1.3, cols[1]);
    cont(p, rng, 12.2, 0.0, 3.92, cols[2]);
    Car {
        geo: merge(parts),
        l,
    }
}
