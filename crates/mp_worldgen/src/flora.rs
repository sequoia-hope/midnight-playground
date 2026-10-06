//! `src/world/valley/flora.js`: vegetation and rock geometry shared by the
//! Sierra Pass (`Mountain.js`, WP 3.6), Old Mill Valley (`Valley.js`,
//! WP 3.7) and the Raceway (WP 7.4) scenery.
//!
//! Everything here is a small template for `InstancedMesh`: shading comes
//! from baked vertex colours (ambient occlusion, strata, tip highlights)
//! and hand-set normals rather than extra triangles, so the instances stay
//! cheap enough for a phone. Instance colour multiplies the vertex colour,
//! which is how each copy gets its own tone.
//!
//! Each export keeps its JS name, snake-cased, with its defaults written
//! out (DECISIONS D190). The arithmetic is the JS's, in its order, with
//! every inexact `Math` function through `mp_math::kernel` and `f32`
//! stores where the JS stores into a `Float32Array` (geometry attributes,
//! the noise's value table, the rock's displacement). The golden is
//! `parity/golden/flora/flora.json` (`tools/parity/flora.mjs`,
//! `tests/flora.rs`): bit-identical.

#![allow(clippy::needless_range_loop)]

use mp_math::{Mulberry32, clamp, js, kernel, lerp, smoothstep};
use mp_scene::{BufferData, three};

use crate::material::{Material, Param};
use crate::three_geom::{
    BufferAttribute, BufferGeometry, cylinder_geometry, icosahedron_geometry, merge_geometries,
    merge_vertices,
};

const PI: f64 = core::f64::consts::PI;

/// three's default `mergeVertices` tolerance.
const MERGE_TOLERANCE: f64 = 1e-4;

// ── 3-D value noise (rock displacement) ─────────────────────────────────

/// `makeNoise3D(seed)`: the closure it returns is [`Noise3D::noise`].
#[derive(Clone, Debug)]
pub struct Noise3D {
    /// `new Uint8Array(512)`.
    p: [u8; 512],
    /// `new Float32Array(256)`.
    r: [f32; 256],
}

/// `makeNoise3D(seed = 1)`.
pub fn make_noise3d(seed: u32) -> Noise3D {
    let mut rng = Mulberry32::new(seed);
    let mut p = [0u8; 512];
    let mut r = [0f32; 256];
    for i in 0..256 {
        p[i] = i as u8;
        r[i] = (rng.next_f64() * 2.0 - 1.0) as f32;
    }
    for i in (1..256).rev() {
        let j = (rng.next_f64() * (i + 1) as f64).floor() as usize;
        p.swap(i, j);
    }
    for i in 0..256 {
        p[i + 256] = p[i];
    }
    Noise3D { p, r }
}

impl Noise3D {
    /// `h(x, y, z)`: `x & 255` on the JS number, through ToInt32.
    fn h(&self, x: f64, y: f64, z: f64) -> f64 {
        let b = |v: f64| (js::to_int32(v) & 255) as usize;
        let p = &self.p;
        self.r[p[p[p[b(x)] as usize + b(y)] as usize + b(z)] as usize] as f64
    }

    /// The noise at (x, y, z), in about [-1, 1].
    pub fn noise(&self, x: f64, y: f64, z: f64) -> f64 {
        let f = |t: f64| t * t * (3.0 - 2.0 * t);
        let (xi, yi, zi) = (x.floor(), y.floor(), z.floor());
        let (u, v, w) = (f(x - xi), f(y - yi), f(z - zi));
        let h = |a, b, c| self.h(a, b, c);
        let a = lerp(
            lerp(h(xi, yi, zi), h(xi + 1.0, yi, zi), u),
            lerp(h(xi, yi + 1.0, zi), h(xi + 1.0, yi + 1.0, zi), u),
            v,
        );
        let b = lerp(
            lerp(h(xi, yi, zi + 1.0), h(xi + 1.0, yi, zi + 1.0), u),
            lerp(
                h(xi, yi + 1.0, zi + 1.0),
                h(xi + 1.0, yi + 1.0, zi + 1.0),
                u,
            ),
            v,
        );
        lerp(a, b, w)
    }
}

/// `setColors(geo, fn)`: `fn(x, y, z, ny, c, i)` per vertex (`ny` 0 without
/// normals) into a new `color` attribute.
fn set_colors(
    geo: &mut BufferGeometry,
    mut f: impl FnMut(f64, f64, f64, f64, &mut [f64; 3], usize),
) {
    let pos = geo.position();
    let nrm = geo.get_attribute("normal");
    let n = pos.count();
    let mut a = vec![0f32; n * 3];
    let mut c = [0.0; 3];
    for i in 0..n {
        let ny = nrm.map_or(0.0, |m| m.get_y(i));
        f(pos.get_x(i), pos.get_y(i), pos.get_z(i), ny, &mut c, i);
        a[i * 3] = c[0] as f32;
        a[i * 3 + 1] = c[1] as f32;
        a[i * 3 + 2] = c[2] as f32;
    }
    geo.set_attribute("color", BufferAttribute::from_f32(a, 3));
}

// ── Rocks ───────────────────────────────────────────────────────────────

/// `rockGeometry`'s options object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RockOpts {
    pub squash: f64,
    pub crag: bool,
    pub lichen: f64,
    pub tint: [f64; 3],
}

impl Default for RockOpts {
    /// `{ squash = 0.74, crag = false, lichen = 1, tint = [0.62, 0.6, 0.57] }`.
    fn default() -> Self {
        RockOpts {
            squash: 0.74,
            crag: false,
            lichen: 1.0,
            tint: [0.62, 0.6, 0.57],
        }
    }
}

/// `rockGeometry(seed, detail = 2, opts)`.
///
/// A fractured boulder: a noise-displaced sphere with a few random cleavage
/// planes shaved off, so it has broad flat faces and softened edges instead
/// of the lumpy-potato look. Welded, so shading is smooth; vertex colours
/// carry sedimentary banding, dark crevices and lichen on upward faces.
/// `crag` makes it blockier (more, deeper cuts) for outcrops set in a slope.
pub fn rock_geometry(seed: u32, detail: f64, opts: RockOpts) -> BufferGeometry {
    let RockOpts {
        squash,
        crag,
        lichen,
        tint,
    } = opts;
    let mut g = icosahedron_geometry(1.0, detail);
    g.delete_attribute("normal");
    g.delete_attribute("uv");
    let mut g = merge_vertices(&g, MERGE_TOLERANCE);
    let n3 = make_noise3d(seed);
    let mut rng = Mulberry32::new(js::to_uint32(seed as f64 * 7.0 + 3.0));
    let mut planes: Vec<[f64; 4]> = Vec::new();
    let n_pl = if crag { 7 } else { 5 };
    for _ in 0..n_pl {
        let th = rng.next_f64() * PI * 2.0;
        let ph = kernel::acos(rng.next_f64() * 1.6 - 0.6); // bias away from the base
        let nx = kernel::sin(ph) * kernel::cos(th);
        let ny = kernel::cos(ph);
        let nz = kernel::sin(ph) * kernel::sin(th);
        planes.push([
            nx,
            ny,
            nz,
            lerp(
                if crag { 0.5 } else { 0.62 },
                if crag { 0.78 } else { 0.9 },
                rng.next_f64(),
            ),
        ]);
    }
    // The top is usually a flatter bedding plane.
    planes.push([0.0, 1.0, 0.0, if crag { 0.55 } else { 0.7 }]);
    let count = g.position().count();
    let mut disp = vec![0f32; count];
    {
        let pos = g.get_attribute_mut("position").expect("position");
        for i in 0..count {
            let (mut x, mut y, mut z) = (pos.get_x(i), pos.get_y(i), pos.get_z(i));
            let d = n3.noise(x * 1.6 + 11.0, y * 1.6, z * 1.6) * 0.22
                + n3.noise(x * 4.1, y * 4.1 + 7.0, z * 4.1) * 0.07;
            let r = 1.0 + d;
            x *= r;
            y *= r;
            z *= r;
            for &[nx, ny, nz, off] in &planes {
                let s = x * nx + y * ny + z * nz;
                if s > off {
                    let k = (s - off) * 0.9;
                    x -= nx * k;
                    y -= ny * k;
                    z -= nz * k;
                }
            }
            y *= squash;
            if y < -0.32 {
                y = -0.32 - (y + 0.32) * 0.15;
            }
            disp[i] = kernel::hypot3(x, y / squash, z) as f32;
            pos.set_xyz(i, x, y, z);
        }
    }
    g.compute_vertex_normals();
    let band = seed as f64 * 0.37;
    set_colors(&mut g, |x, y, z, ny, c, i| {
        let strata = 0.88
            + 0.12 * kernel::sin(y * 11.0 + n3.noise(x * 2.0, y * 2.0 + 3.0, z * 2.0) * 3.0 + band);
        let ao = clamp(0.55 + (disp[i] as f64 - 0.72) * 1.4, 0.55, 1.05)
            * if y < -0.2 { 0.78 } else { 1.0 };
        let mut r = tint[0] * strata * ao;
        let mut gg = tint[1] * strata * ao;
        let mut b = tint[2] * strata * ao;
        // Lichen and moss on faces that catch rain, in noisy patches.
        let l = smoothstep(0.35, 0.75, ny)
            * smoothstep(0.0, 0.35, n3.noise(x * 3.0 + 40.0, y * 3.0, z * 3.0))
            * lichen;
        r = lerp(r, 0.5, l * 0.55);
        gg = lerp(gg, 0.56, l * 0.55);
        b = lerp(b, 0.34, l * 0.55);
        // A few rusty-orange lichen spots.
        let o = smoothstep(0.42, 0.55, n3.noise(x * 6.0 - 9.0, y * 6.0, z * 6.0)) * 0.5 * lichen;
        r = lerp(r, 0.72, o);
        gg = lerp(gg, 0.5, o);
        b = lerp(b, 0.3, o);
        c[0] = r;
        c[1] = gg;
        c[2] = b;
    });
    g
}

// ── Conifers ────────────────────────────────────────────────────────────

/// `coniferParts`' spec.
struct ConiferSpec {
    tiers: usize,
    h: f64,
    r: f64,
    trunk_h: f64,
    m: usize,
    underside: bool,
    taper: f64,
    droop: f64,
    lean: f64,
    trunk_sides: f64,
    no_trunk: bool,
}

/// Each tier is a drooping skirt of branches: a jagged cone from the tier's
/// apex out to branch tips that hang down, closed underneath by a shallow
/// inverted cone so the tree has a dark, solid underside. Normals are bent
/// outward from the trunk (a rounded "volume" normal) so the whole tree
/// shades like a soft mass rather than a stack of flat facets.
fn conifer_parts(spec: &ConiferSpec, rng: &mut Mulberry32) -> BufferGeometry {
    let ConiferSpec {
        tiers,
        h: big_h,
        r: big_r,
        trunk_h,
        m,
        underside,
        taper,
        droop,
        lean,
        ..
    } = *spec;
    let mut pos: Vec<f64> = Vec::new();
    let mut nrm: Vec<f64> = Vec::new();
    let mut col: Vec<f64> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    #[allow(clippy::too_many_arguments)]
    fn push(
        pos: &mut Vec<f64>,
        nrm: &mut Vec<f64>,
        col: &mut Vec<f64>,
        x: f64,
        y: f64,
        z: f64,
        nx: f64,
        ny: f64,
        nz: f64,
        r: f64,
        g: f64,
        b: f64,
    ) -> u32 {
        pos.extend([x, y, z]);
        let l = js::or(kernel::hypot3(nx, ny, nz), 1.0);
        nrm.extend([nx / l, ny / l, nz / l]);
        col.extend([r, g, b]);
        (pos.len() / 3 - 1) as u32
    }
    let tiers_f = tiers as f64;
    let crown_y0 = trunk_h;
    for k in 0..tiers {
        let kf = k as f64;
        let f = kf / js::max(1.0, tiers_f - 1.0); // 0 bottom → 1 top
        let y0 = crown_y0 + (big_h - crown_y0) * kernel::pow(kf / tiers_f, 0.92);
        let apex =
            crown_y0 + (big_h - crown_y0) * js::min(1.0, kernel::pow((kf + 1.6) / tiers_f, 0.92));
        let top = if k == tiers - 1 { big_h } else { apex };
        let rad = big_r * kernel::pow(1.0 - f * 0.86, taper) * lerp(0.92, 1.08, rng.next_f64());
        let light_top = lerp(0.95, 1.15, f);
        let dark = lerp(0.38, 0.55, f);
        let base = [0.045, 0.088, 0.048];
        let a = push(
            &mut pos,
            &mut nrm,
            &mut col,
            lean * f,
            top,
            0.0,
            0.0,
            1.0,
            0.0,
            base[0] * light_top * 1.15,
            base[1] * light_top * 1.1,
            base[2] * light_top,
        );
        let rot = rng.next_f64() * PI * 2.0;
        let mut rim = Vec::with_capacity(m);
        for j in 0..m {
            let odd = j % 2 == 1;
            let ang = rot + (j as f64 / m as f64) * PI * 2.0 + (rng.next_f64() - 0.5) * 0.35;
            let long = if odd {
                lerp(0.62, 0.8, rng.next_f64())
            } else {
                lerp(0.95, 1.12, rng.next_f64())
            };
            let rr = rad * long;
            let x = kernel::cos(ang) * rr + lean * f;
            let z = kernel::sin(ang) * rr;
            let y = y0 - droop * rad * long + if odd { rad * 0.12 } else { 0.0 };
            // Tips catch light, notches between branches are shaded.
            let t = if odd { 0.72 } else { 1.08 };
            rim.push(push(
                &mut pos,
                &mut nrm,
                &mut col,
                x,
                y,
                z,
                kernel::cos(ang),
                0.55,
                kernel::sin(ang),
                base[0] * t * light_top,
                base[1] * t * light_top,
                base[2] * t * light_top,
            ));
        }
        for j in 0..m {
            idx.extend([a, rim[(j + 1) % m], rim[j]]);
        }
        if underside {
            let u = push(
                &mut pos,
                &mut nrm,
                &mut col,
                lean * f,
                y0 + rad * 0.12,
                0.0,
                0.0,
                -1.0,
                0.0,
                base[0] * dark * 0.6,
                base[1] * dark * 0.6,
                base[2] * dark * 0.6,
            );
            for j in 0..m {
                idx.extend([u, rim[j], rim[(j + 1) % m]]);
            }
        }
    }
    let mut crown = BufferGeometry::new();
    crown.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
    crown.set_attribute("normal", BufferAttribute::from_f64(&nrm, 3));
    crown.set_attribute("color", BufferAttribute::from_f64(&col, 3));
    crown.set_index(&idx);
    let mut trunk = cylinder_geometry(
        0.1 * big_r / 1.8,
        0.22 * big_r / 1.8,
        trunk_h + 1.2,
        spec.trunk_sides,
        1.0,
        true,
        0.0,
        PI * 2.0,
    );
    trunk.delete_attribute("uv");
    trunk.translate(0.0, (trunk_h + 1.2) / 2.0 - 0.4, 0.0);
    set_colors(&mut trunk, |_x, y, _z, _ny, c, _i| {
        let v = 0.8 + y * 0.05;
        c[0] = 0.08 * v;
        c[1] = 0.055 * v;
        c[2] = 0.035 * v;
    });
    // The far version has no trunk: at that range it is never seen.
    if spec.no_trunk {
        return crown.to_non_indexed();
    }
    merge_geometries(&[&trunk.to_non_indexed(), &crown.to_non_indexed()], false)
        .expect("trunk and crown share their attributes")
}

/// `coniferGeometry(kind = 'spruce', lod = 0, seed = 1)`.
///
/// kind: `"spruce"` (narrow, dense), `"fir"` (broad, open, taller trunk).
/// lod: 0 detailed (verge), 1 mid (the forest behind), 2 far (hillsides).
pub fn conifer_geometry(kind: &str, lod: usize, seed: u32) -> BufferGeometry {
    let mut rng = Mulberry32::new(seed);
    let spruce = kind == "spruce";
    let mut spec = ConiferSpec {
        h: if spruce { 8.2 } else { 9.0 },
        r: if spruce { 1.75 } else { 2.3 },
        trunk_h: if spruce { 0.9 } else { 2.2 },
        tiers: [if spruce { 7 } else { 5 }, 3, 2][lod],
        m: [8, 6, 5][lod],
        underside: lod == 0,
        taper: if spruce { 1.0 } else { 0.8 },
        droop: if spruce { 0.3 } else { 0.18 },
        lean: 0.0,
        trunk_sides: if lod == 0 { 5.0 } else { 3.0 },
        no_trunk: false,
    };
    if lod == 2 {
        spec.trunk_h = 0.8;
        spec.underside = false;
        spec.no_trunk = true;
    }
    conifer_parts(&spec, &mut rng)
}

// ── Broadleaf canopies ──────────────────────────────────────────────────

/// `canopyGeometry(kind = 'shade', seed = 3, lod = 0)`.
///
/// A cluster of welded, noise-displaced lobes. Normals come from the whole
/// cluster's centre (not each lobe's), so light wraps over it like a real
/// crown; vertex colours darken the underside and the inner creases.
/// lod 1 keeps only the main lobe and two side lobes (distant trees).
pub fn canopy_geometry(kind: &str, seed: u32, lod: usize) -> BufferGeometry {
    let mut rng = Mulberry32::new(seed);
    let n3 = make_noise3d(js::to_uint32(seed as f64 + 11.0));
    let mut lobes: Vec<[f64; 4]> = Vec::new();
    match kind {
        "orchard" => {
            lobes.push([0.0, 0.0, 0.0, 1.0]);
            for k in 0..5 {
                let a = (k as f64 / 5.0) * PI * 2.0 + rng.next_f64();
                let y = 0.05 + rng.next_f64() * 0.2;
                lobes.push([kernel::cos(a) * 0.55, y, kernel::sin(a) * 0.55, 0.55]);
            }
        }
        "poplar" => {
            for k in 0..5 {
                let kf = k as f64;
                let x = (rng.next_f64() - 0.5) * 0.25;
                let z = (rng.next_f64() - 0.5) * 0.25;
                lobes.push([x, -0.75 + kf * 0.38, z, 0.62 - (kf - 1.6).abs() * 0.09]);
            }
        }
        "willow" => {
            lobes.push([0.0, 0.1, 0.0, 0.95]);
            for k in 0..6 {
                let a = (k as f64 / 6.0) * PI * 2.0 + rng.next_f64();
                lobes.push([kernel::cos(a) * 0.7, -0.25, kernel::sin(a) * 0.7, 0.6]);
            }
        }
        _ => {
            lobes.push([0.0, 0.0, 0.0, 0.9]);
            for k in 0..6 {
                let a = (k as f64 / 6.0) * PI * 2.0 + rng.next_f64() * 0.6;
                let y = (rng.next_f64() - 0.3) * 0.5;
                let r = lerp(0.45, 0.62, rng.next_f64());
                lobes.push([kernel::cos(a) * 0.62, y, kernel::sin(a) * 0.62, r]);
            }
            lobes.push([0.1, 0.55, -0.05, 0.55]);
        }
    }
    if lod > 0 && kind != "poplar" {
        lobes.truncate(3);
    }
    let parts: Vec<BufferGeometry> = lobes
        .iter()
        .enumerate()
        .map(|(k, &[ox, oy, oz, r])| {
            let detail = if k == 0 && kind != "poplar" && lod == 0 {
                1.0
            } else {
                0.0
            };
            let mut g = icosahedron_geometry(r, detail);
            g.delete_attribute("normal");
            g.delete_attribute("uv");
            let mut g = merge_vertices(&g, MERGE_TOLERANCE);
            let p = g.get_attribute_mut("position").expect("position");
            for i in 0..p.count() {
                let (x, y, z) = (p.get_x(i), p.get_y(i), p.get_z(i));
                let d = 1.0 + 0.2 * n3.noise(x * 3.0 + ox * 5.0, y * 3.0 + k as f64, z * 3.0);
                p.set_xyz(i, x * d + ox, y * d + oy, z * d + oz);
            }
            g
        })
        .collect();
    let refs: Vec<&BufferGeometry> = parts.iter().collect();
    let mut g = merge_geometries(&refs, false).expect("lobes share their attributes");
    let p = g.position();
    let count = p.count();
    let mut nrm = vec![0f32; count * 3];
    let mut col = vec![0f32; count * 3];
    let sy = if kind == "poplar" { 0.35 } else { 1.0 };
    for i in 0..count {
        let (x, y, z) = (p.get_x(i), p.get_y(i), p.get_z(i));
        let l = js::or(kernel::hypot3(x, (y + 0.1) * sy, z), 1.0);
        let nx = x / l;
        let ny = (y + 0.1) * sy / l + 0.15;
        let nz = z / l;
        let ll = kernel::hypot3(nx, ny, nz);
        nrm[i * 3] = (nx / ll) as f32;
        nrm[i * 3 + 1] = (ny / ll) as f32;
        nrm[i * 3 + 2] = (nz / ll) as f32;
        // Occlusion: inner points (close to the centre) and the underside are
        // darker; the sunny top a little yellower.
        let rad = kernel::hypot3(x, y * sy, z);
        let ao =
            clamp(0.45 + rad * 0.55, 0.45, 1.05) * lerp(0.62, 1.08, smoothstep(-0.8, 0.6, y * sy));
        let v = 0.9 + 0.2 * n3.noise(x * 5.0, y * 5.0, z * 5.0 + 3.0);
        col[i * 3] = (ao * v * (1.0 + smoothstep(0.2, 0.9, y * sy) * 0.12)) as f32;
        col[i * 3 + 1] = (ao * v) as f32;
        col[i * 3 + 2] = (ao * v * 0.85) as f32;
    }
    g.set_attribute(
        "normal",
        BufferAttribute::new(BufferData::F32(nrm), 3, false),
    );
    g.set_attribute(
        "color",
        BufferAttribute::new(BufferData::F32(col), 3, false),
    );
    g
}

// ── Ground cover ────────────────────────────────────────────────────────

/// `grassClumpGeometry(blades = 9, seed = 5)`.
///
/// Grass tussock: a fan of bent blades (one triangle each, double-sided),
/// dark at the root and sun-bleached at the tips.
pub fn grass_clump_geometry(blades: usize, seed: u32) -> BufferGeometry {
    let mut rng = Mulberry32::new(seed);
    let mut pos: Vec<f64> = Vec::new();
    let mut col: Vec<f64> = Vec::new();
    let mut nrm: Vec<f64> = Vec::new();
    for k in 0..blades {
        let a = (k as f64 / blades as f64) * PI * 2.0 + rng.next_f64() * 0.5;
        let r0 = rng.next_f64() * 0.2;
        let h = lerp(0.3, 0.8, rng.next_f64());
        let lean = lerp(0.08, 0.3, rng.next_f64());
        let w = lerp(0.035, 0.065, rng.next_f64());
        let (cx, cz) = (kernel::cos(a), kernel::sin(a));
        let (bx, bz) = (cx * r0, cz * r0);
        let (px, pz) = (-cz * w, cx * w);
        pos.extend([
            bx - px,
            0.0,
            bz - pz,
            bx + px,
            0.0,
            bz + pz,
            bx + cx * lean,
            h,
            bz + cz * lean,
        ]);
        for _ in 0..3 {
            nrm.extend([cx * 0.4, 0.9, cz * 0.4]);
        }
        let tip = lerp(0.9, 1.25, rng.next_f64());
        col.extend([
            0.05,
            0.06,
            0.025,
            0.05,
            0.06,
            0.025,
            0.3 * tip,
            0.28 * tip,
            0.13 * tip,
        ]);
    }
    let mut g = BufferGeometry::new();
    g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
    g.set_attribute("normal", BufferAttribute::from_f64(&nrm, 3));
    g.set_attribute("color", BufferAttribute::from_f64(&col, 3));
    g
}

/// `flowerGeometry(n = 5, seed = 9)`.
///
/// Wildflower heads: small upturned stars on short stems at varied heights.
/// White vertex colour, so the instance colour is the flower's colour.
pub fn flower_geometry(n: usize, seed: u32) -> BufferGeometry {
    let mut rng = Mulberry32::new(seed);
    let mut pos: Vec<f64> = Vec::new();
    let mut col: Vec<f64> = Vec::new();
    let mut nrm: Vec<f64> = Vec::new();
    for _ in 0..n {
        let x = (rng.next_f64() - 0.5) * 0.7;
        let z = (rng.next_f64() - 0.5) * 0.7;
        let y = lerp(0.3, 0.6, rng.next_f64());
        let r = lerp(0.05, 0.08, rng.next_f64());
        let a0 = rng.next_f64() * PI;
        for j in 0..3 {
            let a = a0 + (j as f64 / 3.0) * PI * 2.0;
            pos.extend([
                x,
                y + 0.02,
                z,
                x + kernel::cos(a) * r,
                y,
                z + kernel::sin(a) * r,
                x + kernel::cos(a + 1.2) * r,
                y,
                z + kernel::sin(a + 1.2) * r,
            ]);
            for _ in 0..3 {
                nrm.extend([0.0, 1.0, 0.0]);
                col.extend([1.0, 1.0, 1.0]);
            }
        }
        // Stem.
        pos.extend([x - 0.012, 0.0, z, x + 0.012, 0.0, z, x, y, z]);
        for _ in 0..3 {
            nrm.extend([0.0, 0.5, 0.8]);
            col.extend([0.05, 0.09, 0.03]);
        }
    }
    let mut g = BufferGeometry::new();
    g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
    g.set_attribute("normal", BufferAttribute::from_f64(&nrm, 3));
    g.set_attribute("color", BufferAttribute::from_f64(&col, 3));
    g
}

/// `shrubGeometry(seed = 21, detail = 1)`: a low shrub (manzanita and sage
/// on the pass, hedge filler in the valley).
pub fn shrub_geometry(seed: u32, detail: f64) -> BufferGeometry {
    let n3 = make_noise3d(seed);
    let mut g = icosahedron_geometry(1.0, detail);
    g.delete_attribute("normal");
    g.delete_attribute("uv");
    let mut g = merge_vertices(&g, MERGE_TOLERANCE);
    let p = g.get_attribute_mut("position").expect("position");
    let count = p.count();
    let mut nrm = vec![0f32; count * 3];
    let mut col = vec![0f32; count * 3];
    for i in 0..count {
        let (x, y, z) = (p.get_x(i), p.get_y(i), p.get_z(i));
        let d = 1.0 + 0.28 * n3.noise(x * 2.2, y * 2.2, z * 2.2);
        let yy = js::max(y * 0.62 * d, -0.1) + 0.1;
        p.set_xyz(i, x * d, yy, z * d);
        let l = kernel::hypot3(x, y + 0.3, z);
        nrm[i * 3] = (x / l) as f32;
        nrm[i * 3 + 1] = ((y + 0.3) / l) as f32;
        nrm[i * 3 + 2] = (z / l) as f32;
        let ao = lerp(0.5, 1.1, smoothstep(-0.2, 0.7, y))
            * (0.9 + 0.2 * n3.noise(x * 6.0, y * 6.0, z * 6.0));
        col[i * 3] = ao as f32;
        col[i * 3 + 1] = ao as f32;
        col[i * 3 + 2] = (ao * 0.9) as f32;
    }
    g.set_attribute(
        "normal",
        BufferAttribute::new(BufferData::F32(nrm), 3, false),
    );
    g.set_attribute(
        "color",
        BufferAttribute::new(BufferData::F32(col), 3, false),
    );
    g
}

/// `foliageMaterial(o = {})`: vertex colours carry the shading,
/// double-sided for blades and skirt undersides. `o` is the object spread
/// over the defaults (`{ side: THREE.FrontSide, roughness: 0.95 }`), in
/// its order.
pub fn foliage_material(o: &[(&str, Param)]) -> Material {
    let mut m = Material::standard()
        .set("vertexColors", true)
        .set("roughness", 0.92)
        .set("metalness", 0.0)
        .set("side", three::DOUBLE_SIDE as f64);
    for (k, v) in o {
        m.set_value(k, v.clone());
    }
    m
}
