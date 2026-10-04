//! Port of `src/world/desert/props.js` (roadmap WP 7.3), the second Desert
//! Run kit: sculpted sandstone (strata-shaded rock, the arch, buttes), the
//! rest of the Mojave flora, tumbleweeds, the spectators' camp and the
//! dry-lake decals. Like `parts`, everything is vertex-coloured and merged
//! into one geometry per object so the scenery can instance it.

// The kit keeps the JS argument lists and index loops (DECISIONS D52, D130).
#![allow(clippy::too_many_arguments, clippy::needless_range_loop)]
// The JS's own constants (a 3.14 m width, 6.283 for a turn) stay as written:
// the values must be the JS's, not π or τ.
#![allow(clippy::approx_constant)]

use std::f64::consts::PI;

use mr_canvas::Canvas;
use mr_math::{Mulberry32, Noise2D, clamp, js, kernel, lerp, smoothstep};

use super::parts::{boxg, cyl, limb, merge, paint, prep, sphere, v3};
use crate::color::Color;
use crate::textures::Texture;
use crate::three_geom::{
    BufferAttribute, BufferGeometry, Vector2, Vector3, icosahedron_geometry, lathe_geometry,
    merge_vertices,
};

// ── Sandstone shading ───────────────────────────────────────────────────

/// Red Rock palette: iron-red, salmon, cream and chocolate layers.
pub const STRATA: [u32; 7] = [
    0xc8744a, 0xe2b088, 0xb45e3e, 0xd8966a, 0xa4543a, 0xe8c49c, 0xbe6a44,
];
const VARNISH: u32 = 0x3e2a22;
const SHADE: u32 = 0x6a3a2a;

/// `strataPaint`'s options (`Default` is the JS defaults).
#[derive(Clone, Debug)]
pub struct StrataOpts {
    pub seed: u32,
    pub period: f64,
    pub lift: f64,
    pub varnish: f64,
    pub ao: f64,
    /// The bands as hex (`STRATA` by default).
    pub bands: Vec<u32>,
    pub bleach: f64,
}

impl Default for StrataOpts {
    fn default() -> Self {
        StrataOpts {
            seed: 1,
            period: 0.22,
            lift: 0.0,
            varnish: 0.55,
            ao: 0.5,
            bands: STRATA.to_vec(),
            bleach: 0.12,
        }
    }
}

/// `k % L` of a JS number known to be a non-negative integer.
fn modi(k: f64, l: usize) -> usize {
    assert!(k >= 0.0, "a band index below zero (the JS reads undefined)");
    (k as u64 % l as u64) as usize
}

/// Paints a (non-indexed) rock geometry: colour bands by height, the
/// underside and foot darkened as if ambient-occluded, and desert-varnish
/// streaks running down the steep faces. `period` is the band height in
/// local units; `lift` offsets the bands so instances of one shape differ.
pub fn strata_paint(geo: &mut BufferGeometry, o: &StrataOpts) {
    let n = Noise2D::new(o.seed);
    geo.compute_bounding_box();
    let bb = geo.bounding_box.expect("bounds");
    let (min, max) = (bb.min, bb.max);
    let h = js::max(1e-3, max.y - min.y);
    let bands: Vec<Color> = o.bands.iter().map(|&b| Color::hex(b)).collect();
    let varnish_c = Color::hex(VARNISH);
    let shade_c = Color::hex(SHADE);
    let pos = geo.position();
    let nor = geo.get_attribute("normal").expect("normal");
    let cnt = pos.count();
    let mut col = vec![0f32; cnt * 3];
    let l = bands.len();
    for i in 0..cnt {
        let (x, y, z) = (pos.get_x(i), pos.get_y(i), pos.get_z(i));
        let ny = nor.get_y(i);
        let v = (y + o.lift) / o.period + n.noise(x * 0.8 + 3.0, z * 0.8) * 0.9 + 50.0;
        let k = v.floor();
        let fr = smoothstep(0.72, 1.0, v - k);
        let mut c = bands[modi(k, l)];
        c.lerp(bands[modi(k + 1.0, l)], fr);
        // Sun-bleached tops, shadowed undersides.
        let up = clamp(ny, -1.0, 1.0);
        if up > 0.0 {
            c.lerp(bands[5 % l], o.bleach * up);
        } else {
            c.lerp(shade_c, -up * 0.45 * o.ao);
        }
        let foot = 1.0 - smoothstep(0.0, 0.3, (y - min.y) / h);
        c.multiply_scalar(1.0 - foot * 0.35 * o.ao);
        // Varnish: dark streaks hanging down from ledges on steep faces.
        let steep = 1.0 - ny.abs();
        let a = kernel::atan2(z, x);
        let streak = smoothstep(
            0.1,
            0.55,
            n.noise(
                kernel::cos(a) * 3.1 + x * 1.7,
                kernel::sin(a) * 3.1 + z * 1.7 + y * 0.12,
            ),
        );
        let fade = 0.4 + 0.6 * smoothstep(0.1, 0.9, (y - min.y) / h);
        c.lerp(varnish_c, o.varnish * streak * steep * steep * fade);
        col[i * 3] = c.r as f32;
        col[i * 3 + 1] = c.g as f32;
        col[i * 3 + 2] = c.b as f32;
    }
    geo.set_attribute("color", BufferAttribute::from_f32(col, 3));
}

/// A rock's character (`kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RockKind {
    /// A weathered boulder.
    Round,
    /// Fresh rockfall with broken faces.
    Block,
    /// A fallen ledge.
    Slab,
    /// A cheap talus chip.
    Scree,
}

/// Rock of a given character, unit size, flat underside at y ≈ -0.25.
pub fn rock_geometry(seed: u32, kind: RockKind) -> BufferGeometry {
    let n = Noise2D::new(seed);
    let mut rng = Mulberry32::new(seed);
    let mut g = match kind {
        RockKind::Round => icosahedron_geometry(1.0, 1.0),
        RockKind::Scree => icosahedron_geometry(1.0, 0.0),
        _ => crate::three_geom::box_geometry(2.0, 2.0, 2.0, 2.0, 2.0, 2.0),
    };
    g.delete_attribute("uv");
    g.delete_attribute("normal");
    let mut g = merge_vertices(&g, 1e-4);
    let sq = match kind {
        RockKind::Slab => 0.38,
        RockKind::Block => 0.8,
        RockKind::Scree => 0.6,
        RockKind::Round => 0.72,
    };
    let round = if matches!(kind, RockKind::Block | RockKind::Slab) {
        0.35
    } else {
        1.0
    };
    // Random cutting planes knock corners off, giving broken facets.
    let mut cuts: Vec<(Vector3, f64)> = Vec::new();
    let ncuts = if kind == RockKind::Round { 3 } else { 6 };
    for _ in 0..ncuts {
        let dx = rng.next_f64() - 0.5;
        let dy = rng.next_f64() * 0.8 - 0.1;
        let dz = rng.next_f64() - 0.5;
        let d = v3(dx, dy, dz).normalize();
        cuts.push((d, lerp(0.62, 0.85, rng.next_f64())));
    }
    {
        let p = g.get_attribute_mut("position").expect("position");
        for i in 0..p.count() {
            let mut v = v3(p.get_x(i), p.get_y(i), p.get_z(i));
            let l = js::or(v.length(), 1.0);
            // Blend box toward sphere so edges round off.
            v = v.lerp(v.multiply_scalar(1.15 / l), round * 0.6);
            let d = n.noise(v.x * 1.1 + v.z * 2.3 + 5.0, v.y * 1.3 - v.z * 0.7) * 0.5
                + n.noise(v.x * 2.9 - v.z * 1.7, v.y * 2.6 + v.x) * 0.22;
            v = v.multiply_scalar(1.0 + d * if kind == RockKind::Round { 0.4 } else { 0.22 });
            for &(cd, co) in &cuts {
                let t = v.dot(cd) - co;
                if t > 0.0 {
                    // clip onto the plane: a clean facet, no folds
                    v = v.add_scaled_vector(cd, -t);
                }
            }
            p.set_xyz(i, v.x, js::max(v.y * sq, -0.25), v.z);
        }
    }
    let mut f = g.to_non_indexed();
    f.compute_vertex_normals();
    strata_paint(
        &mut f,
        &StrataOpts {
            seed: seed + 11,
            period: if kind == RockKind::Slab { 0.09 } else { 0.2 },
            varnish: if kind == RockKind::Scree { 0.25 } else { 0.5 },
            ao: 0.6,
            ..StrataOpts::default()
        },
    );
    f
}

/// Hoodoo: a banded column of soft rock pinched into necks, flaring into a
/// pedestal at the foot and carrying a hard, dark, overhanging caprock.
/// Unit height (y 0..1 plus the cap), radius ~0.12.
pub fn hoodoo_geometry(seed: u32) -> BufferGeometry {
    let mut rng = Mulberry32::new(seed);
    let n = Noise2D::new(seed + 7);
    let mut prof = Vec::new();
    let necks = 1 + (rng.next_f64() * 3.0).floor() as usize;
    const N: usize = 17;
    for i in 0..=N {
        let t = i as f64 / N as f64;
        let mut r = lerp(0.19, 0.085, kernel::pow(t, 0.8));
        r += 0.16 * kernel::exp(-t * 16.0); // pedestal flare
        for k in 0..necks {
            let c = 0.3 + (k as f64 + 0.5) / necks as f64 * 0.6 + (rng.next_f64() - 0.5) * 0.05;
            r *= 1.0 - 0.38 * kernel::exp(-kernel::pow((t - c) / 0.06, 2.0));
        }
        // Hard beds stand proud, soft beds weather back.
        r *= 1.0 + 0.07 * kernel::sin(t * 38.0 + rng.next_f64() * 0.3);
        prof.push(Vector2::new(r, t));
    }
    prof.push(Vector2::new(0.001, 1.0));
    let mut g = lathe_geometry(&prof, 12.0, 0.0, PI * 2.0);
    {
        let p = g.get_attribute_mut("position").expect("position");
        for i in 0..p.count() {
            let (x, y, z) = (p.get_x(i), p.get_y(i), p.get_z(i));
            let a = kernel::atan2(z, x);
            // Wrap-safe roughness: noise on (cos, sin) of the angle, not x/z.
            let d = 1.0
                + n.noise(
                    kernel::cos(a) * 1.3 + y * 3.0,
                    kernel::sin(a) * 1.3 - y * 2.0,
                ) * 0.22
                + n.noise(kernel::cos(a) * 4.0, kernel::sin(a) * 4.0 + y * 9.0) * 0.06;
            p.set_xyz(i, x * d, y, z * d);
        }
    }
    g.delete_attribute("uv");
    g.delete_attribute("normal");
    g.compute_vertex_normals(); // smooth (indexed)
    let mut gi = g.to_non_indexed();
    let bands = vec![0xd98a5c, 0xeec39a, 0xc86e48, 0xe4a878, 0xbc6444, 0xf0d0a8];
    strata_paint(
        &mut gi,
        &StrataOpts {
            seed,
            period: 0.09,
            bands,
            varnish: 0.5,
            ao: 0.4,
            bleach: 0.05,
            ..StrataOpts::default()
        },
    );
    // Caprock: a hard grey-brown slab, wider than the neck below it.
    let mut cap = rock_geometry(seed + 3, RockKind::Slab);
    let sx = 0.1 + rng.next_f64() * 0.03;
    let sz = 0.095 + rng.next_f64() * 0.03;
    cap.scale(sx, 0.2, sz);
    cap.translate(0.0, 1.01, 0.0);
    {
        let tint = Color::hex(0x7a5c4a);
        let cc = cap.get_attribute_mut("color").expect("color");
        for i in 0..cc.count() {
            let mut c = Color::new(cc.get_x(i), cc.get_y(i), cc.get_z(i));
            c.lerp(tint, 0.7);
            cc.set_xyz(i, c.r, c.g, c.b);
        }
    }
    merge(vec![gi, cap])
}

/// `archGeometry(seed, { R, H, groundL, groundR, base })`.
#[derive(Clone, Copy, Debug)]
pub struct ArchOpts {
    pub r: f64,
    pub h: f64,
    pub ground_l: f64,
    pub ground_r: f64,
    pub base: f64,
}

/// The natural arch: a span swept along a flattened semicircle whose
/// cross-section is a rounded box, thick where it springs from its
/// buttresses and thin at the crown. Soft beds are cut back into grooves,
/// noise erodes the whole thing and fallen blocks lie at its feet.
/// Local: x across the road (legs at ±R), y up from the road, z along it.
pub fn arch_geometry(seed: u32, o: &ArchOpts) -> BufferGeometry {
    let (r_, h_) = (o.r, o.h);
    let n = Noise2D::new(seed);
    let mut rng = Mulberry32::new(seed);
    const NS: usize = 72;
    const NR: usize = 20;
    let mut pos: Vec<f64> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let path = |t: f64| {
        let a = PI * t;
        let s = kernel::sin(a);
        v3(
            -r_ * kernel::cos(a) + kernel::sin(a * 2.0) * 1.5,
            h_ * kernel::pow(s, 0.55),
            0.0,
        )
    };
    // Extend the legs down into the ground beyond the path's ends.
    struct P {
        c: Vector3,
        tg: Vector3,
        t: f64,
    }
    let mut ps: Vec<P> = Vec::new();
    for i in 0..=NS {
        let t = i as f64 / NS as f64;
        let c = path(t);
        let e = 0.004;
        let tg = (path(js::min(1.0, t + e)) - path(js::max(0.0, t - e))).normalize();
        ps.push(P { c, tg, t });
    }
    let first_x = ps[0].c.x;
    let last_x = ps[NS].c.x;
    ps.insert(
        0,
        P {
            c: v3(first_x, o.ground_l - 6.0, 0.0),
            tg: v3(0.0, 1.0, 0.0),
            t: 0.0,
        },
    );
    ps.push(P {
        c: v3(last_x, o.ground_r - 6.0, 0.0),
        tg: v3(0.0, -1.0, 0.0),
        t: 1.0,
    });
    let ring = ps.len();
    for i in 0..ring {
        let P { c, tg, t } = ps[i];
        // Frame: tangent, in-plane normal (outward), road axis.
        let mut nrm = v3(-tg.y, tg.x, 0.0);
        if nrm.y < 0.0 || (nrm.y.abs() < 1e-3 && c.x * nrm.x < 0.0) {
            nrm = nrm.multiply_scalar(-1.0);
        }
        let foot = kernel::pow(1.0 - kernel::sin(PI * t), 2.2);
        let crown = kernel::pow(kernel::sin(PI * t), 6.0);
        // radial half-thickness
        let a = lerp(3.4, 8.5, foot) * (1.0 + 0.25 * crown) * (1.0 + 0.12 * n.noise(t * 7.0, 1.3));
        // half-depth along road
        let b = lerp(4.6, 10.5, foot) * (1.0 + 0.15 * n.noise(t * 5.0, 4.1));
        for k in 0..NR {
            let u = (k as f64 / NR as f64) * PI * 2.0;
            let cu = kernel::cos(u);
            let su = kernel::sin(u);
            // Rounded box: superellipse exponent 0.6.
            let ex = js::sign(cu) * kernel::pow(cu.abs(), 0.6);
            let ez = js::sign(su) * kernel::pow(su.abs(), 0.6);
            let mut tmp = c.add_scaled_vector(nrm, ex * a);
            tmp.z += ez * b;
            // Erosion: big lumps, then grooves where soft beds are cut back.
            let wy = tmp.y + o.base;
            let ph = (((wy / 2.7) % 1.0) + 1.0) % 1.0;
            let soft = smoothstep(0.55, 0.8, ph) * (1.0 - smoothstep(0.85, 1.0, ph));
            let bump = n.noise(tmp.x * 0.09 + tmp.z * 0.05, wy * 0.08) * 1.6
                + n.noise(tmp.x * 0.3 + 7.0, wy * 0.25 + tmp.z * 0.3) * 0.5;
            let inward = bump - soft * 0.9;
            let ox = ex * nrm.x;
            let oy = ex * nrm.y;
            let oz = ez;
            let ol = js::or(kernel::hypot3(ox, oy, oz), 1.0);
            tmp.x += (ox / ol) * inward;
            tmp.y += (oy / ol) * inward;
            tmp.z += (oz / ol) * inward * 1.1;
            pos.extend_from_slice(&[tmp.x, tmp.y, tmp.z]);
        }
        if i < ring - 1 {
            for k in 0..NR {
                let a0 = (i * NR + k) as u32;
                let a1 = (i * NR + (k + 1) % NR) as u32;
                let b0 = a0 + NR as u32;
                let b1 = a1 + NR as u32;
                idx.extend_from_slice(&[a0, b0, a1, a1, b0, b1]);
            }
        }
    }
    let mut g = BufferGeometry::new();
    g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
    g.set_index(&idx);
    g.compute_vertex_normals();
    let mut gi = g.to_non_indexed();
    // Smooth normals from the indexed version survive toNonIndexed.
    paint_world_strata(&mut gi, o.base, seed, 0.6);
    let mut parts = vec![gi];
    // Buttress masses and fallen blocks round the feet.
    for side in [-1.0, 1.0] {
        let gy = if side < 0.0 { o.ground_l } else { o.ground_r };
        for k in 0..7u32 {
            let big = k < 2;
            let kind = if big {
                RockKind::Round
            } else if rng.next_f64() < 0.5 {
                RockKind::Block
            } else {
                RockKind::Slab
            };
            let mut r = rock_geometry(seed * 7 + k * 13 + if side > 0.0 { 5 } else { 0 }, kind);
            let s = if big {
                lerp(6.0, 9.0, rng.next_f64())
            } else {
                lerp(1.2, 3.2, rng.next_f64())
            };
            let sx = s * lerp(0.9, 1.4, rng.next_f64());
            let sy = s * if big {
                1.2
            } else {
                lerp(0.6, 1.0, rng.next_f64())
            };
            let sz = s * lerp(0.9, 1.4, rng.next_f64());
            r.scale(sx, sy, sz);
            r.rotate_y(rng.next_f64() * 6.3);
            let out = if big {
                lerp(2.0, 5.0, rng.next_f64())
            } else {
                lerp(4.0, 14.0, rng.next_f64())
            };
            let tz = (rng.next_f64() - 0.5) * if big { 10.0 } else { 26.0 };
            r.translate(
                side * (r_ + out),
                gy + if big { s * 0.2 } else { s * 0.1 },
                tz,
            );
            paint_world_strata(&mut r, o.base, seed + k, 0.4);
            parts.push(r);
        }
    }
    merge(parts)
}

/// Strata by absolute height (so the arch's bands line up with the walls').
fn paint_world_strata(geo: &mut BufferGeometry, base: f64, seed: u32, varnish: f64) {
    let n = Noise2D::new(seed + 5);
    let pos = geo.position();
    let nor = geo.get_attribute("normal").expect("normal");
    let cnt = pos.count();
    let mut col = vec![0f32; cnt * 3];
    let bands: Vec<Color> = [
        0xc9774c, 0xe0ae84, 0xb45e3c, 0xd89a6c, 0xa8523a, 0xc4845a, 0xe6bc92,
    ]
    .iter()
    .map(|&h| Color::hex(h))
    .collect();
    let shade_c = Color::hex(SHADE);
    let varnish_c = Color::hex(VARNISH);
    for i in 0..cnt {
        let (x, y, z) = (pos.get_x(i), pos.get_y(i), pos.get_z(i));
        let ny = nor.get_y(i);
        let v = (y + base) / 3.4 + n.noise(x * 0.05, z * 0.05) * 0.4 + 100.0;
        let k = v.floor();
        let mut c = bands[modi(k, 7)];
        c.lerp(bands[modi(k + 1.0, 7)], smoothstep(0.7, 1.0, v - k));
        if ny < 0.0 {
            c.lerp(shade_c, -ny * 0.35);
        }
        let steep = 1.0 - ny.abs();
        let streak = smoothstep(0.05, 0.5, n.noise(x * 0.45 + z * 0.3, y * 0.02 + 3.0));
        c.lerp(
            varnish_c,
            varnish * streak * steep * (0.5 + 0.5 * smoothstep(5.0, 28.0, y)),
        );
        col[i * 3] = c.r as f32;
        col[i * 3 + 1] = c.g as f32;
        col[i * 3 + 2] = c.b as f32;
    }
    geo.set_attribute("color", BufferAttribute::from_f32(col, 3));
}

/// Butte / mesa for the far horizon: a skirt of talus, sheer banded cliffs
/// and a flat caprock. Unit radius, unit height; `spire` makes a narrow
/// pinnacle. Cheap: it's only ever seen kilometres away.
pub fn butte_geometry(seed: u32, spire: bool) -> BufferGeometry {
    let mut rng = Mulberry32::new(seed);
    let n = Noise2D::new(seed);
    let s_ = if spire { 9 } else { 14 };
    let prof: &[[f64; 2]] = if spire {
        &[
            [1.6, 0.0],
            [0.8, 0.22],
            [0.5, 0.3],
            [0.46, 0.7],
            [0.4, 0.95],
            [0.44, 0.97],
            [0.0, 1.0],
        ]
    } else {
        &[
            [1.7, 0.0],
            [1.25, 0.18],
            [1.02, 0.3],
            [1.0, 0.62],
            [0.9, 0.66],
            [0.88, 0.95],
            [0.92, 0.97],
            [0.0, 1.0],
        ]
    };
    let mut pos: Vec<f64> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let rows = prof.len();
    let sf = s_ as f64;
    let wob: Vec<f64> = (0..s_)
        .map(|k| {
            let k = k as f64;
            1.0 + n.noise(
                kernel::cos(k / sf * 6.283) * 1.4,
                kernel::sin(k / sf * 6.283) * 1.4,
            ) * 0.28
                + (rng.next_f64() - 0.5) * 0.08
        })
        .collect();
    for r in 0..rows {
        let [rad, y] = prof[r];
        for k in 0..s_ {
            let a = (k as f64 / sf) * PI * 2.0;
            pos.extend_from_slice(&[
                kernel::cos(a) * rad * wob[k],
                y,
                kernel::sin(a) * rad * wob[k],
            ]);
        }
    }
    for r in 0..rows - 1 {
        for k in 0..s_ {
            let a0 = (r * s_ + k) as u32;
            let a1 = (r * s_ + (k + 1) % s_) as u32;
            let b0 = a0 + s_ as u32;
            let b1 = a1 + s_ as u32;
            idx.extend_from_slice(&[a0, b0, a1, a1, b0, b1]);
        }
    }
    let mut g = BufferGeometry::new();
    g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
    g.set_index(&idx);
    let mut f = g.to_non_indexed();
    f.compute_vertex_normals();
    let bands = vec![0xb86a48, 0xd49a70, 0xa85a3c, 0xc88460, 0x9a5038, 0xdcae84];
    strata_paint(
        &mut f,
        &StrataOpts {
            seed,
            period: if spire { 0.08 } else { 0.07 },
            bands,
            varnish: 0.45,
            ao: 0.3,
            bleach: 0.2,
            ..StrataOpts::default()
        },
    );
    f
}

// ── Flora ───────────────────────────────────────────────────────────────

/// Pinyon pine: short trunk, a dense rounded crown of dark clumps.
pub fn pinyon_geometry(seed: u32) -> BufferGeometry {
    let mut rng = Mulberry32::new(seed);
    let mut parts = Vec::new();
    let h = lerp(3.2, 4.5, rng.next_f64());
    let tx = (rng.next_f64() - 0.5) * 0.4;
    let tz = (rng.next_f64() - 0.5) * 0.4;
    parts.push(paint(
        prep(limb(
            v3(0.0, 0.0, 0.0),
            v3(tx, h * 0.45, tz),
            0.22,
            0.14,
            5.0,
        )),
        0x4e3a2c,
        0.2,
        Some(&mut rng),
    ));
    let cols = [0x2f4a2c, 0x3a5634, 0x2a4028, 0x44603a];
    for i in 0..9 {
        let t = i as f64 / 9.0;
        let y = lerp(h * 0.35, h * 0.95, t) + (rng.next_f64() - 0.5) * 0.3;
        let rr = lerp(1.4, 0.4, t) * lerp(0.8, 1.1, rng.next_f64());
        let a = rng.next_f64() * 6.3;
        let mut b = icosahedron_geometry(1.0, 0.0);
        let sx = lerp(0.7, 1.0, rng.next_f64()) * lerp(1.3, 0.6, t);
        let sy = lerp(0.55, 0.8, rng.next_f64());
        let sz = lerp(0.7, 1.0, rng.next_f64()) * lerp(1.3, 0.6, t);
        b.scale(sx, sy, sz);
        b.rotate_y(rng.next_f64() * 3.0);
        b.translate(kernel::cos(a) * rr * 0.6, y, kernel::sin(a) * rr * 0.6);
        parts.push(paint(prep(b), cols[i % 4], 0.3, Some(&mut rng)));
    }
    merge(parts)
}

/// Big sagebrush: a low, twiggy silver-grey mound made of many small puffs,
/// vertex-coloured white so instances carry the tint.
pub fn sage_geometry(seed: u32) -> BufferGeometry {
    let mut rng = Mulberry32::new(seed);
    let mut parts = Vec::new();
    for _ in 0..8 {
        let a = rng.next_f64() * 6.3;
        let r = rng.next_f64().sqrt() * 0.55;
        let s = lerp(0.24, 0.4, rng.next_f64());
        let mut b = icosahedron_geometry(1.0, 0.0);
        let sy = s * lerp(0.8, 1.3, rng.next_f64());
        b.scale(s, sy, s);
        b.rotate_y(rng.next_f64() * 3.0);
        let cx = kernel::cos(a) * r;
        let cy = s * 0.8 + (0.55 - r) * 0.5;
        let cz = kernel::sin(a) * r;
        b.translate(cx, cy, cz);
        let mut f = prep(b);
        // Foliage normals: bend each facet's normal toward "out from the bush"
        // so the mound shades soft and round instead of like faceted stone.
        let cnt = f.position().count();
        for i in 0..cnt {
            let (px, py, pz) = {
                let p = f.position();
                (p.get_x(i), p.get_y(i), p.get_z(i))
            };
            let vv = v3(px - cx, py - cy, pz - cz).normalize();
            let w = v3(px, py + 0.2, pz).normalize();
            let vv = vv.lerp(w, 0.5).normalize();
            f.get_attribute_mut("normal")
                .expect("normal")
                .set_xyz(i, vv.x, vv.y, vv.z);
        }
        parts.push(paint(f, 0xffffff, 0.35, Some(&mut rng)));
    }
    // A few bare stems showing at the base.
    for _ in 0..4 {
        let a = rng.next_f64() * 6.3;
        parts.push(paint(
            prep(limb(
                v3(0.0, 0.0, 0.0),
                v3(kernel::cos(a) * 0.35, 0.45, kernel::sin(a) * 0.35),
                0.04,
                0.02,
                3.0,
            )),
            0x6a5c4c,
            0.1,
            Some(&mut rng),
        ));
    }
    merge(parts)
}

/// Teddy-bear cholla: a stubby trunk of jointed segments, the spines
/// catching the light pale gold.
pub fn cholla_geometry(seed: u32) -> BufferGeometry {
    const GOLD: u32 = 0xc8c07a;
    const DARK: u32 = 0x5a5040;
    fn seg(
        parts: &mut Vec<BufferGeometry>,
        rng: &mut Mulberry32,
        a: Vector3,
        dir: Vector3,
        len: f64,
        r: f64,
        depth: i32,
    ) {
        let b = a.add_scaled_vector(dir, len);
        let mut g = cyl(r * 0.9, r, len, 5.0, 1.0, true);
        g.translate(0.0, len / 2.0, 0.0);
        g.apply_quaternion(crate::three_geom::Quaternion::from_unit_vectors(
            super::parts::UP,
            dir.normalize(),
        ));
        g.translate(a.x, a.y, a.z);
        parts.push(paint(
            prep(g),
            if depth == 3 { DARK } else { GOLD },
            0.18,
            Some(rng),
        ));
        if depth <= 0 {
            return;
        }
        let k = if depth == 3 {
            3
        } else {
            1 + if rng.next_f64() < 0.6 { 1 } else { 0 }
        };
        for _ in 0..k {
            let az = rng.next_f64() * 6.3;
            let up = lerp(0.2, 0.8, rng.next_f64());
            let d2 = v3(
                kernel::cos(az) * (1.0 - up),
                up,
                kernel::sin(az) * (1.0 - up),
            )
            .normalize();
            let l2 = lerp(0.3, 0.45, rng.next_f64());
            seg(parts, rng, b, d2, l2, r * 0.85, depth - 1);
        }
    }
    let mut rng = Mulberry32::new(seed);
    let mut parts = Vec::new();
    let l = lerp(0.7, 1.0, rng.next_f64());
    seg(
        &mut parts,
        &mut rng,
        v3(0.0, 0.0, 0.0),
        v3(0.0, 1.0, 0.0),
        l,
        0.12,
        3,
    );
    merge(parts)
}

/// Ocotillo: a fan of long whip canes from one root, tipped red.
pub fn ocotillo_geometry(seed: u32) -> BufferGeometry {
    let mut rng = Mulberry32::new(seed);
    let mut parts = Vec::new();
    const N: usize = 11;
    for i in 0..N {
        let az = (i as f64 / N as f64) * PI * 2.0 + rng.next_f64() * 0.4;
        let lean = lerp(0.12, 0.45, rng.next_f64());
        let l = lerp(3.2, 5.2, rng.next_f64());
        let a = v3(0.0, 0.0, 0.0);
        let m = v3(
            kernel::cos(az) * lean * l * 0.4,
            l * 0.5,
            kernel::sin(az) * lean * l * 0.4,
        );
        let b = v3(kernel::cos(az) * lean * l, l, kernel::sin(az) * lean * l);
        parts.push(paint(
            prep(limb(a, m, 0.05, 0.04, 3.0)),
            0x5a5a3c,
            0.15,
            Some(&mut rng),
        ));
        parts.push(paint(
            prep(limb(m, b, 0.04, 0.02, 3.0)),
            0x626a3c,
            0.15,
            Some(&mut rng),
        ));
        let tip = limb(b, b + v3(0.0, 0.35, 0.0), 0.03, 0.005, 3.0);
        parts.push(paint(prep(tip), 0xc83a24, 0.1, Some(&mut rng)));
    }
    merge(parts)
}

/// Tumbleweed: a ball of thin, criss-crossing twigs. Each twig is a sliver
/// triangle, both faces, so the ball reads as a tangle, not a solid.
pub fn tumbleweed_geometry(seed: u32) -> BufferGeometry {
    let mut rng = Mulberry32::new(seed);
    let mut pos: Vec<f64> = Vec::new();
    let mut col: Vec<f64> = Vec::new();
    for _ in 0..70 {
        let u = rng.next_f64() * 2.0 - 1.0;
        let a = rng.next_f64() * 6.283;
        let r = (1.0 - u * u).sqrt();
        let p = v3(kernel::cos(a) * r, u, kernel::sin(a) * r).multiply_scalar(lerp(
            0.55,
            1.0,
            rng.next_f64(),
        ));
        let (dx, dy, dz) = (
            rng.next_f64() - 0.5,
            rng.next_f64() - 0.5,
            rng.next_f64() - 0.5,
        );
        let d = v3(dx, dy, dz)
            .normalize()
            .multiply_scalar(lerp(0.5, 0.9, rng.next_f64()));
        let (wx, wy, wz) = (
            rng.next_f64() - 0.5,
            rng.next_f64() - 0.5,
            rng.next_f64() - 0.5,
        );
        let w = v3(wx, wy, wz).normalize().multiply_scalar(0.11);
        let aa = p - d;
        let bb = p + d;
        let cc = p + w;
        pos.extend_from_slice(&[
            aa.x, aa.y, aa.z, bb.x, bb.y, bb.z, cc.x, cc.y, cc.z, aa.x, aa.y, aa.z, cc.x, cc.y,
            cc.z, bb.x, bb.y, bb.z,
        ]);
        let mut c = Color::hex(0xa88a5c);
        c.multiply_scalar(lerp(0.7, 1.15, rng.next_f64()));
        for _ in 0..6 {
            col.extend_from_slice(&[c.r, c.g, c.b]);
        }
    }
    let mut g = BufferGeometry::new();
    g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
    g.set_attribute("color", BufferAttribute::from_f64(&col, 3));
    g.compute_vertex_normals();
    // A denser, darker core so the ball still reads when the twigs go
    // sub-pixel at distance.
    let core = icosahedron_geometry(0.62, 0.0);
    let core = paint(prep(core), 0x6e5e44, 0.3, Some(&mut rng));
    merge(vec![g, core])
}

// ── Roadside furniture ──────────────────────────────────────────────────

/// White delineator post with an amber reflector (unit: 1.2 m tall).
pub fn delineator_geometry() -> BufferGeometry {
    let mut p = boxg(0.1, 1.2, 0.06);
    p.translate(0.0, 0.6, 0.0);
    let mut r = boxg(0.08, 0.16, 0.02);
    r.translate(0.0, 1.0, 0.04);
    merge(vec![
        paint(prep(p), 0xeeeeea, 0.0, None),
        paint(prep(r), 0xffa020, 0.0, None),
    ])
}

/// Light-tower beam: an open cone, apex at the origin pointing down -Y,
/// brightness fading along it in the colour attribute (drawn additive).
pub fn beam_geometry(len: f64, r: f64) -> BufferGeometry {
    let mut g = cyl(r * 0.05, r, len, 18.0, 4.0, true);
    g.translate(0.0, -len / 2.0, 0.0);
    let p = g.position();
    let cnt = p.count();
    let mut a = vec![0f32; cnt * 3];
    for i in 0..cnt {
        let f = kernel::pow(1.0 - (-p.get_y(i) / len), 1.6);
        a[i * 3] = f as f32;
        a[i * 3 + 1] = (f * 0.97) as f32;
        a[i * 3 + 2] = (f * 0.9) as f32;
    }
    g.set_attribute("color", BufferAttribute::from_f32(a, 3));
    g.delete_attribute("uv");
    g
}

// ── The spectators' camp ────────────────────────────────────────────────

/// A motorhome: the shell, the windows and the length (`{ body, glass, L }`).
pub struct Motorhome {
    pub body: BufferGeometry,
    pub glass: BufferGeometry,
    pub l: f64,
}

/// Class-C motorhome: cab-over body, stripes, a door and lit windows. The
/// shell goes to 'solid' (vertex-coloured); windows are a separate glass
/// geometry so they can glow at night. Front faces +Z, wheels on y = 0.
pub fn motorhome(rng: &mut Mulberry32) -> Motorhome {
    let mut body = Vec::new();
    let mut glass = Vec::new();
    let l = lerp(7.5, 9.5, rng.next_f64());
    let w = 2.45;
    let hh = 3.2;
    let stripe = [0xb8322a, 0x2e5a8a, 0x8a5a2a, 0x3a7a5a][(rng.next_f64() * 4.0).floor() as usize];
    let bx = |into: &mut Vec<BufferGeometry>,
              w: f64,
              h: f64,
              d: f64,
              x: f64,
              y: f64,
              z: f64,
              hex: u32| {
        let mut g = boxg(w, h, d);
        g.translate(x, y + h / 2.0, z);
        into.push(paint(prep(g), hex, 0.0, None));
    };
    bx(&mut body, w, hh - 0.6, l - 2.2, 0.0, 0.6, -1.1, 0xeeebe2); // coach box
    bx(
        &mut body,
        w,
        0.9,
        2.3,
        0.0,
        hh - 0.9,
        l / 2.0 - 1.9 - 0.3,
        0xeeebe2,
    ); // cab-over bunk
    bx(
        &mut body,
        w - 0.1,
        1.6,
        2.0,
        0.0,
        0.55,
        l / 2.0 - 1.0,
        0xe4e0d6,
    ); // cab
    bx(&mut body, w + 0.02, 0.22, l - 2.2, 0.0, 1.25, -1.1, stripe);
    bx(
        &mut body,
        w + 0.02,
        0.12,
        l - 2.2,
        0.0,
        1.55,
        -1.1,
        0x2a2a2a,
    );
    bx(
        &mut body,
        w - 0.2,
        0.3,
        0.2,
        0.0,
        0.4,
        l / 2.0 + 0.05,
        0x9a9a9a,
    ); // bumper
    bx(&mut body, w, 0.35, 0.3, 0.0, 0.3, -l / 2.0 - 0.1, 0x3a3a3a);
    bx(&mut body, 1.4, 0.3, 1.2, 0.0, hh, -2.5, 0xd8d4ca); // roof AC
    for z in [l / 2.0 - 1.4, -l / 2.0 + 1.6] {
        for x in [-1.05, 1.05] {
            let mut wh = cyl(0.4, 0.4, 0.3, 10.0, 1.0, false);
            wh.rotate_z(PI / 2.0);
            wh.translate(x, 0.4, z);
            body.push(paint(prep(wh), 0x1a1a1a, 0.0, None));
        }
    }
    bx(&mut glass, w + 0.04, 0.55, 0.9, 0.0, 1.75, -2.8, 0x000000); // side windows
    bx(&mut glass, w + 0.04, 0.55, 1.1, 0.0, 1.75, 0.6, 0x000000);
    bx(
        &mut glass,
        w - 0.3,
        0.6,
        0.06,
        0.0,
        1.2,
        l / 2.0 + 0.01,
        0x000000,
    ); // windshield
    bx(
        &mut body,
        0.06,
        1.8,
        0.7,
        w / 2.0 + 0.01,
        0.55,
        -0.6,
        0x5a5a5a,
    ); // door
    // Awning rolled out on the door side.
    bx(
        &mut body,
        0.08,
        0.08,
        4.2,
        w / 2.0 + 2.1,
        2.45,
        -1.6,
        0x9a9a9a,
    );
    let mut aw = boxg(2.2, 0.05, 4.2);
    aw.rotate_z(-0.12);
    aw.translate(w / 2.0 + 1.05, 2.62, -1.6);
    body.push(paint(prep(aw), stripe, 0.0, None));
    for z in [-3.6, 0.4] {
        bx(&mut body, 0.05, 2.4, 0.05, w / 2.0 + 2.1, 0.0, z, 0x9a9a9a);
    }
    Motorhome {
        body: merge(body),
        glass: merge(glass),
        l,
    }
}

/// Dome tent (unit ≈ 2.4 m across) with a darker fly.
pub fn dome_tent_geometry(seed: u32) -> BufferGeometry {
    let mut rng = Mulberry32::new(seed);
    let mut g = sphere(1.2, 8.0, 3.0, PI / 2.0);
    g.scale(1.0, 0.85, 1.0);
    let mut d = boxg(0.7, 0.9, 0.05);
    d.translate(0.0, 0.45, 1.15);
    let pole = limb(v3(-1.2, 0.0, 0.0), v3(0.0, 1.05, 0.0), 0.02, 0.02, 3.0);
    let pole2 = limb(v3(0.0, 1.05, 0.0), v3(1.2, 0.0, 0.0), 0.02, 0.02, 3.0);
    merge(vec![
        paint(prep(g), 0xffffff, 0.12, Some(&mut rng)),
        paint(prep(d), 0x303030, 0.0, None),
        paint(prep(pole), 0x333333, 0.0, None),
        paint(prep(pole2), 0x333333, 0.0, None),
    ])
}

/// Folding camp chair + cooler, a little scene at each fire.
pub fn camp_set_geometry(seed: u32) -> BufferGeometry {
    let mut rng = Mulberry32::new(seed);
    let mut parts = Vec::new();
    let cols = [0x2e5a8a, 0xb8322a, 0x3a6a3a, 0x303030];
    for i in 0..3 {
        let a = (i as f64 / 3.0) * PI * 2.0 + rng.next_f64() * 0.5;
        let x = kernel::cos(a) * 2.2;
        let z = kernel::sin(a) * 2.2;
        let c = cols[(rng.next_f64() * 4.0).floor() as usize];
        let mut seat = boxg(0.55, 0.06, 0.5);
        seat.translate(0.0, 0.45, 0.0);
        let mut back = boxg(0.55, 0.55, 0.05);
        back.rotate_x(-0.2);
        back.translate(0.0, 0.72, 0.27);
        let mut cp = vec![
            paint(prep(seat), c, 0.0, None),
            paint(prep(back), c, 0.0, None),
        ];
        for lx in [-0.25, 0.25] {
            for lz in [-0.22, 0.22] {
                let mut l = boxg(0.03, 0.47, 0.03);
                l.rotate_x(if lz > 0.0 { 0.35 } else { -0.35 });
                l.translate(lx, 0.22, 0.0);
                cp.push(paint(prep(l), 0x222222, 0.0, None));
            }
        }
        let mut chair = merge(cp);
        chair.rotate_y(kernel::atan2(-x, -z) + PI);
        chair.translate(x, 0.0, z);
        parts.push(chair);
    }
    let mut cooler = boxg(0.7, 0.45, 0.42);
    cooler.translate(1.3, 0.22, -1.9);
    let cc = if rng.next_f64() < 0.5 {
        0x2a6ab8
    } else {
        0xd83a2a
    };
    parts.push(paint(prep(cooler), cc, 0.0, None));
    let mut lid = boxg(0.72, 0.08, 0.44);
    lid.translate(1.3, 0.47, -1.9);
    parts.push(paint(prep(lid), 0xf0f0ea, 0.0, None));
    // Fire ring: a circle of stones.
    for k in 0..9 {
        let a = (k as f64 / 9.0) * PI * 2.0;
        let mut s = icosahedron_geometry(0.16, 0.0);
        s.scale(1.0, 0.7, 1.0);
        s.translate(kernel::cos(a) * 0.6, 0.06, kernel::sin(a) * 0.6);
        parts.push(paint(prep(s), 0x6a6258, 0.3, Some(&mut rng)));
    }
    for k in 0..4 {
        let mut l = boxg(0.8, 0.1, 0.1);
        l.rotate_z(0.35);
        l.rotate_y((k as f64 / 4.0) * PI);
        l.translate(0.0, 0.18, 0.0);
        parts.push(paint(prep(l), 0x3a2a1c, 0.0, None));
    }
    merge(parts)
}

/// A standing spectator: legs, torso, head (≈1.75 m), white so instances
/// carry a shirt colour; skin/jeans baked in.
pub fn person_geometry(seed: u32) -> BufferGeometry {
    let mut rng = Mulberry32::new(seed);
    let mut legs = boxg(0.34, 0.85, 0.2);
    legs.translate(0.0, 0.43, 0.0);
    let mut torso = boxg(0.44, 0.62, 0.24);
    torso.translate(0.0, 1.16, 0.0);
    let mut arms = boxg(0.62, 0.5, 0.14);
    arms.translate(0.0, 1.2, 0.02);
    let mut head = icosahedron_geometry(0.12, 0.0);
    head.scale(1.0, 1.2, 1.0);
    head.translate(0.0, 1.62, 0.0);
    merge(vec![
        paint(prep(legs), 0x2e3a52, 0.1, Some(&mut rng)),
        paint(prep(torso), 0xffffff, 0.0, None),
        paint(prep(arms), 0xe8e8e8, 0.0, None),
        paint(prep(head), 0xc89a78, 0.0, None),
    ])
}

// ── Silver Lake decals ──────────────────────────────────────────────────

/// Cracked playa mud: polygon plates with dark, slightly curled edges on a
/// transparent ground, fading out at the tile's rim so tiles overlap
/// seamlessly. Channels: rgb colour, alpha coverage.
pub fn crack_decal_texture(seed: u32) -> Texture {
    const S: usize = 512;
    let sf = S as f64;
    let mut g = Canvas::new(S as u32, S as u32);
    let mut img = g.create_image_data(S as u32, S as u32);
    let mut rng = Mulberry32::new(seed);
    let mut cells: Vec<[f64; 3]> = Vec::new();
    const N: usize = 15;
    let nf = N as f64;
    for j in 0..N {
        for i in 0..N {
            let cx = (i as f64 + 0.15 + rng.next_f64() * 0.7) * sf / nf;
            let cy = (j as f64 + 0.15 + rng.next_f64() * 0.7) * sf / nf;
            cells.push([cx, cy, rng.next_f64()]);
        }
    }
    // Grid lookup: only the 3×3 neighbourhood can hold the nearest seeds.
    let cell_at = |i: f64, j: f64| {
        let j = clamp(j, 0.0, nf - 1.0) as usize;
        let i = clamp(i, 0.0, nf - 1.0) as usize;
        cells[j * N + i]
    };
    for y in 0..S {
        for x in 0..S {
            let (xf, yf) = (x as f64, y as f64);
            let ci = (xf / (sf / nf)).floor();
            let cj = (yf / (sf / nf)).floor();
            let mut d1 = 1e9;
            let mut d2 = 1e9;
            let mut own = [0.0; 3];
            for dj in -1..=1 {
                for di in -1..=1 {
                    let c = cell_at(ci + di as f64, cj + dj as f64);
                    let d = kernel::pow(xf - c[0], 2.0) + kernel::pow(yf - c[1], 2.0);
                    if d < d1 {
                        d2 = d1;
                        d1 = d;
                        own = c;
                    } else if d < d2 {
                        d2 = d;
                    }
                }
            }
            let e = d2.sqrt() - d1.sqrt();
            let k = (y * S + x) * 4;
            let r = kernel::hypot(xf - sf / 2.0, yf - sf / 2.0) / (sf / 2.0);
            let rim = 1.0 - smoothstep(0.3, 0.98, r);
            // Crack: dark line; lip: a pale curled edge just inside the plate.
            let crack = 1.0 - smoothstep(1.2, 3.2, e);
            let lip = smoothstep(2.5, 4.0, e) * (1.0 - smoothstep(4.0, 9.0, e));
            let plate = own[2];
            let mut rr = 226.0 + plate * 18.0;
            let mut gg = rr * 0.955;
            let mut bb = rr * 0.9;
            let mut aa = 0.04 + plate * 0.1;
            rr += lip * 22.0;
            gg += lip * 22.0;
            bb += lip * 22.0;
            aa = js::max(aa, lip * 0.4);
            rr = lerp(rr, 70.0, crack);
            gg = lerp(gg, 62.0, crack);
            bb = lerp(bb, 56.0, crack);
            aa = lerp(aa, 0.85, crack);
            img.set(k, rr);
            img.set(k + 1, gg);
            img.set(k + 2, bb);
            img.set(k + 3, 255.0 * aa * rim);
        }
    }
    g.put_image_data(&img, 0, 0);
    Texture::from_canvas(&g, false, true, 8.0)
}
