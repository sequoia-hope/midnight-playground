//! `src/world/streets/props.js`: street furniture and effects for Downtown
//! Streets. Everything small is appended into Streets' chunked GeoBuilders
//! (one draw call per chunk and material, however many hydrants), and the
//! few animated things (steam, neon flicker) run in shaders off a single
//! time uniform.
//!
//! The material patches (`ambientPatch`, `neonFlicker`) are
//! [`super::ambient_patch`] and [`super::neon_flicker`]; `emitData` is
//! [`emit_data`]. The functions take the Streets build ([`Bld`]) as the JS
//! takes `S`.

#![allow(clippy::too_many_arguments, clippy::needless_range_loop)]

use std::collections::BTreeSet;
use std::f64::consts::PI;
use std::sync::Arc;

use mr_math::{js, kernel};
use mr_scene::{MaterialKind, NodeType, three};
use serde_json::Value;

use super::buildings::col;
use super::textures::{AWNING_C, POSTER, S_TILE, STALL, VENDING, puddle_texture};
use super::{Bld, Front, SignLight, Spill, anim, ground};
use crate::car_model::{BuildOpts as CarOpts, Lod, build_vehicle};
use crate::color::Color;
use crate::geom::{GeoBuilder, P3, StaticOpts, static_mesh};
use crate::material::{Material, num};
use crate::object::{Image, Layer, MaterialId, SceneGraph, TextureId};
use crate::textures::Texture;
use crate::three_geom::{
    BufferAttribute, BufferGeometry, Euler, EulerOrder, ExtrudeOptions, Matrix3, Matrix4,
    Quaternion, Shape, Vector2, Vector3, box_geometry, cone_geometry, cylinder_geometry,
    extrude_geometry, icosahedron_geometry, merge_geometries,
};

const PI2: f64 = PI * 2.0;
const ADDITIVE: f64 = three::ADDITIVE_BLENDING as f64;

/// The scene export's `Math.random` seed (DECISIONS D20).
pub const PAGE_RANDOM_SEED: u32 = 0x5eed;
/// Where the page's `Math.random` stood when `buildSteam` drew the puffs'
/// seeds (draws from the scene export's seed), measured from the export's
/// `aSeed` attribute: all 900 draws follow on from here (DECISIONS D611).
pub const STEAM_RANDOM_AT: u64 = 15284;

/// A canvas texture made for one material (`new THREE.CanvasTexture(c)`).
pub fn own_texture(graph: &mut SceneGraph, t: Texture) -> TextureId {
    let desc = t.desc("", 0);
    graph.add_texture(Image::Own(Arc::new(t)), desc)
}

/// GeoBuilder writes its data channel as `color`; rename it for the shaders
/// that read it as data (so three doesn't switch on vertex colours).
pub fn emit_data(mut geo: BufferGeometry, name: &str) -> BufferGeometry {
    if let Some(c) = geo.get_attribute("color").cloned() {
        geo.delete_attribute("color");
        geo.set_attribute(name, c);
    }
    geo
}

// ── Geometry helpers ─────────────────────────────────────────────

/// Append any geometry (placed by matrix m) into a GeoBuilder with one colour.
pub fn add_geo(b: &mut GeoBuilder, geo: &BufferGeometry, m: &Matrix4, col: Option<P3>) {
    let owned;
    let g = if geo.index.is_some() {
        owned = geo.to_non_indexed();
        &owned
    } else {
        geo
    };
    let p = g.get_attribute("position").expect("position");
    let n = g.get_attribute("normal").expect("normal");
    let n3 = Matrix3::get_normal_matrix(m);
    for i in 0..p.count() {
        let v = p.get_vector3(i).apply_matrix4(m);
        b.pos.extend_from_slice(&[v.x, v.y, v.z]);
        let v = n.get_vector3(i).apply_matrix3(&n3).normalize();
        b.nor.extend_from_slice(&[v.x, v.y, v.z]);
        b.uv.extend_from_slice(&[0.0, 0.0]);
        if let Some(c) = &mut b.col {
            let col = col.expect("a colour for a coloured builder");
            c.extend_from_slice(&col);
        }
        if let Some(c) = &mut b.cell {
            c.push(0.0);
        }
    }
}

/// A flat quad facing outward normal (nx, nz), centred at (cx, cz), spanning
/// w along the wall and y0..y1, with atlas UVs that read left to right.
pub fn facing(
    b: &mut GeoBuilder,
    cx: f64,
    cz: f64,
    mut tx: f64,
    mut tz: f64,
    nx: f64,
    nz: f64,
    w: f64,
    y0: f64,
    y1: f64,
    cell: f64,
    col: P3,
    uv: [f64; 4],
) {
    if nx * -tz + nz * tx < 0.0 {
        tx = -tx;
        tz = -tz;
    }
    let ax = cx - tx * w / 2.0;
    let az = cz - tz * w / 2.0;
    let bx = cx + tx * w / 2.0;
    let bz = cz + tz * w / 2.0;
    b.quad(
        [ax, y0, az],
        [bx, y0, bz],
        [bx, y1, bz],
        [ax, y1, az],
        Some([
            [uv[0], uv[1]],
            [uv[2], uv[1]],
            [uv[2], uv[3]],
            [uv[0], uv[3]],
        ]),
        Some(col),
        cell,
    );
}

/// `box(w, h, d, y = 0)`: a box standing on y.
fn bx(w: f64, h: f64, d: f64, y: f64) -> BufferGeometry {
    let mut g = box_geometry(w, h, d, 1.0, 1.0, 1.0);
    g.translate(0.0, y + h / 2.0, 0.0);
    g
}

/// `cyl(r0, r1, h, seg, y = 0)`.
fn cyl(r0: f64, r1: f64, h: f64, seg: f64, y: f64) -> BufferGeometry {
    let mut g = cylinder_geometry(r0, r1, h, seg, 1.0, false, 0.0, PI2);
    g.translate(0.0, y + h / 2.0, 0.0);
    g
}

/// `merge(list)`: non-indexed, position and normal only.
fn merge(list: Vec<BufferGeometry>) -> BufferGeometry {
    let gs: Vec<BufferGeometry> = list
        .into_iter()
        .map(|g| {
            let mut g = if g.index.is_some() {
                g.to_non_indexed()
            } else {
                g
            };
            let names: Vec<String> = g.attributes.iter().map(|(n, _)| n.clone()).collect();
            for a in names {
                if a != "position" && a != "normal" {
                    g.delete_attribute(&a);
                }
            }
            g
        })
        .collect();
    let refs: Vec<&BufferGeometry> = gs.iter().collect();
    merge_geometries(&refs, false).expect("the kit merges")
}

fn translated(mut g: BufferGeometry, x: f64, y: f64, z: f64) -> BufferGeometry {
    g.translate(x, y, z);
    g
}

/// A kit item's [geometry, colour] parts.
type KitParts = Vec<(BufferGeometry, Option<P3>)>;

/// Small kit, built once: each is [geometry, colour] parts.
pub struct Kit {
    parts: Vec<(&'static str, KitParts)>,
}

impl Kit {
    fn new() -> Kit {
        let mut ico = icosahedron_geometry(0.55, 0.0);
        ico.scale(1.6, 0.6, 0.9);
        ico.translate(0.0, 0.75, 0.0);
        Kit {
            parts: vec![
                (
                    "hydrant",
                    vec![
                        (
                            merge(vec![
                                cyl(0.15, 0.17, 0.55, 6.0, 0.05),
                                translated(
                                    cone_geometry(0.16, 0.14, 6.0, 1.0, false, 0.0, PI2),
                                    0.0,
                                    0.67,
                                    0.0,
                                ),
                            ]),
                            None,
                        ),
                        (
                            merge(vec![
                                bx(0.46, 0.1, 0.1, 0.36),
                                cyl(0.2, 0.2, 0.06, 6.0, 0.0),
                            ]),
                            Some([0.55, 0.55, 0.52]),
                        ),
                    ],
                ),
                (
                    "bin",
                    vec![
                        (cyl(0.27, 0.24, 0.85, 8.0, 0.0), Some([0.12, 0.22, 0.16])),
                        (
                            merge(vec![
                                cyl(0.3, 0.3, 0.08, 8.0, 0.85),
                                translated(
                                    cone_geometry(0.29, 0.16, 8.0, 1.0, false, 0.0, PI2),
                                    0.0,
                                    1.01,
                                    0.0,
                                ),
                            ]),
                            Some([0.18, 0.3, 0.22]),
                        ),
                    ],
                ),
                (
                    "news",
                    vec![
                        (bx(0.5, 1.0, 0.45, 0.0), None),
                        (
                            translated(bx(0.44, 0.3, 0.02, 0.55), 0.0, 0.0, 0.23),
                            Some([0.8, 0.8, 0.75]),
                        ),
                    ],
                ),
                (
                    "meter",
                    vec![
                        (cyl(0.04, 0.04, 1.1, 6.0, 0.0), Some([0.3, 0.32, 0.34])),
                        (bx(0.2, 0.32, 0.16, 1.1), Some([0.5, 0.52, 0.55])),
                    ],
                ),
                (
                    "bench",
                    vec![
                        (
                            merge(vec![
                                bx(1.8, 0.06, 0.45, 0.42),
                                translated(bx(1.8, 0.4, 0.05, 0.5), 0.0, 0.0, -0.22),
                            ]),
                            Some([0.45, 0.3, 0.18]),
                        ),
                        (
                            merge(vec![
                                translated(bx(0.06, 0.42, 0.4, 0.0), -0.8, 0.0, 0.0),
                                translated(bx(0.06, 0.42, 0.4, 0.0), 0.8, 0.0, 0.0),
                            ]),
                            Some([0.18, 0.18, 0.2]),
                        ),
                    ],
                ),
                (
                    "planter",
                    vec![
                        (bx(2.2, 0.6, 1.2, 0.0), Some([0.42, 0.4, 0.38])),
                        (merge(vec![ico]), Some([0.12, 0.24, 0.1])),
                    ],
                ),
                (
                    "bollard",
                    vec![(cyl(0.1, 0.12, 0.9, 6.0, 0.0), Some([0.22, 0.22, 0.24]))],
                ),
                (
                    "cone",
                    vec![
                        (cyl(0.03, 0.18, 0.7, 8.0, 0.0), Some([1.0, 0.35, 0.05])),
                        (cyl(0.1, 0.13, 0.12, 8.0, 0.3), Some([0.95, 0.95, 0.95])),
                        (bx(0.4, 0.04, 0.4, 0.0), Some([0.1, 0.1, 0.1])),
                    ],
                ),
            ],
        }
    }

    fn get(&self, name: &str) -> &[(BufferGeometry, Option<P3>)] {
        &self
            .parts
            .iter()
            .find(|(n, _)| *n == name)
            .expect("a kit item")
            .1
    }
}

/// `putKit(B, name, m, tint)`.
pub fn put_kit(kit: &Kit, b: &mut GeoBuilder, name: &str, m: &Matrix4, tint: Option<P3>) {
    for (geo, c) in kit.get(name) {
        add_geo(b, geo, m, c.or(tint));
    }
}

/// `trs(x, y, z, yaw = 0, s = 1)` (props.js's own: an `XYZ` Euler).
fn trs(x: f64, y: f64, z: f64, yaw: f64, s: f64) -> Matrix4 {
    Matrix4::compose(
        Vector3::new(x, y, z),
        Quaternion::from_euler(&Euler::new(0.0, yaw, 0.0)),
        Vector3::new(s, s, s),
    )
}

/// three.js yaw that turns local +Z onto the horizontal direction (dx, dz).
fn yaw_z(dx: f64, dz: f64) -> f64 {
    kernel::atan2(dx, dz)
}

impl Bld<'_> {
    fn kit(&mut self) -> &Kit {
        self.kit.get_or_insert_with(Kit::new)
    }

    /// `putKit(this.bPlain.at(kx, kz), name, m, tint)`.
    pub fn put_kit_plain(&mut self, kx: f64, kz: f64, name: &str, m: &Matrix4, tint: Option<P3>) {
        self.kit();
        let kit = self.kit.as_ref().expect("built");
        put_kit(kit, self.b_plain.at(kx, kz), name, m, tint);
    }

    /// `Props.addGeo(this.bPlain.at(kx, kz), geo, m, col)`.
    pub fn add_geo_plain(
        &mut self,
        kx: f64,
        kz: f64,
        geo: &BufferGeometry,
        m: &Matrix4,
        col: Option<P3>,
    ) {
        add_geo(self.b_plain.at(kx, kz), geo, m, col);
    }
}

// ── Kerbside furniture ───────────────────────────────────────────
/// Hydrants, bins, newspaper boxes and parking meters along the kerbs of
/// the blocks near the route; district decides the mix.
pub fn kerb_furniture(s: &mut Bld) {
    let (px, pz, hw) = (s.g.px, s.g.pz, s.g.hw);
    let mut seen: BTreeSet<(i64, i64)> = BTreeSet::new();
    for bi in 0..s.blocks.len() {
        let (b_i, b_j, tier, district) = {
            let b = &s.blocks[bi];
            (b.i as f64, b.j as f64, b.tier, b.district)
        };
        if tier != 0 {
            continue;
        }
        let x0 = b_i * px + hw + 0.7;
        let x1 = (b_i + 1.0) * px - hw - 0.7;
        let z0 = b_j * pz + hw + 0.7;
        let z1 = (b_j + 1.0) * pz - hw - 0.7;
        let mut put = |s: &mut Bld, x: f64, z: f64, nx: f64, nz: f64| {
            let key = (js::round(x / 2.0) as i64, js::round(z / 2.0) as i64);
            if seen.contains(&key) || !s.on_kerb(x, z) {
                return;
            }
            seen.insert(key);
            let y = ground(x, z) + 0.15;
            s.kit();
            let r = s.rng();
            let yaw = yaw_z(nx, nz);
            if r < 0.2 {
                let tint = if district == 1 {
                    [0.85, 0.82, 0.75]
                } else if district == 2 {
                    [0.75, 0.62, 0.1]
                } else {
                    [0.7, 0.1, 0.08]
                };
                s.put_kit_plain(x, z, "hydrant", &trs(x, y, z, yaw, 1.0), Some(tint));
            } else if r < 0.55 {
                let m = trs(x, y, z, s.rng() * 6.0, 1.0);
                s.put_kit_plain(x, z, "bin", &m, None);
            } else if r < 0.7 && district != 1 {
                let cols = [[0.1, 0.25, 0.55], [0.7, 0.1, 0.1], [0.85, 0.7, 0.1]];
                let mut k = 0.0;
                loop {
                    // The loop's bound draws anew every time it is tested.
                    let bound = 1.0 + (s.rng() * 3.0).floor();
                    if k >= bound {
                        break;
                    }
                    let px_ = x - nz * k * 0.55;
                    let pz_ = z + nx * k * 0.55;
                    let c = cols[(s.rng() * 3.0).floor() as usize];
                    s.put_kit_plain(x, z, "news", &trs(px_, y, pz_, yaw, 1.0), Some(c));
                    k += 1.0;
                }
            } else if district == 1 {
                for k in 0..3 {
                    let k = f64::from(k);
                    let mx = x - nz * k * 6.0;
                    let mz = z + nx * k * 6.0;
                    s.put_kit_plain(
                        x,
                        z,
                        "meter",
                        &trs(mx, ground(mx, mz) + 0.15, mz, yaw, 1.0),
                        None,
                    );
                }
            } else {
                s.put_kit_plain(x, z, "bollard", &trs(x, y, z, 0.0, 1.0), None);
            }
        };
        // Along each kerb, facing the road (n is toward the road).
        // Only the kerbs you can see from the route.
        let near = |s: &Bld, x: f64, z: f64| s.route_dist(x, z) < 40.0;
        let mut x = x0 + 9.0;
        while x < x1 - 9.0 {
            if near(s, x, z0) {
                put(s, x, z0, 0.0, -1.0);
            }
            if near(s, x + 5.0, z1) {
                put(s, x + 5.0, z1, 0.0, 1.0);
            }
            x += 18.0 + s.rng() * 18.0;
        }
        let mut z = z0 + 9.0;
        while z < z1 - 9.0 {
            if near(s, x0, z) {
                put(s, x0, z, -1.0, 0.0);
            }
            if near(s, x1, z + 5.0) {
                put(s, x1, z + 5.0, 1.0, 0.0);
            }
            z += 18.0 + s.rng() * 18.0;
        }
    }
}

// ── Things against shop fronts ───────────────────────────────────
/// fronts: {fa, tx, tz, nx, nz, L, y (street floor), district, shop}
pub fn front_props(s: &mut Bld) {
    let fronts = s.fronts.clone();
    for f in &fronts {
        if f.tier != 0 {
            continue;
        }
        // Vending machines in pairs beside Neon District doors.
        if f.district == 0 && s.rng() < 0.3 && f.l > 5.0 {
            let u = if s.rng() < 0.5 { 0.9 } else { f.l - 0.9 - 1.1 };
            let n = 1 + (if s.rng() < 0.5 { 1 } else { 0 });
            for k in 0..n {
                let uu = u + f64::from(k) * 1.1;
                let cx = f.fa[0] + f.tx * (uu + 0.5) + f.nx * 0.42;
                let cz = f.fa[1] + f.tz * (uu + 0.5) + f.nz * 0.42;
                if !s.on_kerb(cx, cz) {
                    continue;
                }
                let y = ground(cx, cz) + 0.15;
                s.b_plain.at(cx, cz).box_(
                    cx,
                    y,
                    cz,
                    1.0,
                    1.9,
                    0.8,
                    kernel::atan2(f.tz, f.tx),
                    &col([0.75, 0.76, 0.78]),
                );
                let cell = VENDING as f64 + 16.0 * s.seed();
                facing(
                    s.b_street.at(cx, cz),
                    cx + f.nx * 0.41,
                    cz + f.nz * 0.41,
                    f.tx,
                    f.tz,
                    f.nx,
                    f.nz,
                    0.96,
                    y + 0.02,
                    y + 1.88,
                    cell,
                    [1.0, 1.0, 1.0],
                    [0.0, 0.0, 1.0, 1.0],
                );
                s.spill.push(Spill {
                    x: cx + f.nx * 1.3,
                    z: cz + f.nz * 1.3,
                    r: 1.8,
                    col: [0.25, 0.3, 0.36],
                    tx: f.tx,
                    tz: f.tz,
                    long: false,
                });
            }
        }
        // Food stalls with awnings and steam, a few per Neon District street.
        if f.district == 0 && s.rng() < 0.1 && f.l > 7.0 {
            stall(s, f);
        }
        // Warm light from lit shop windows spilling across the pavement.
        if f.shop.is_some()
            && let Some(lit) = f.lit
        {
            let cx = f.fa[0] + f.tx * f.l / 2.0 + f.nx * 2.2;
            let cz = f.fa[1] + f.tz * f.l / 2.0 + f.nz * 2.2;
            s.spill.push(Spill {
                x: cx,
                z: cz,
                r: js::min(f.l * 0.45, 5.0),
                col: lit,
                tx: f.tx,
                tz: f.tz,
                long: true,
            });
        }
    }
}

fn stall(s: &mut Bld, f: &Front) {
    let u = f.l * 0.5;
    let nx = f.nx;
    let nz = f.nz;
    let cx = f.fa[0] + f.tx * u + nx * 2.3;
    let cz = f.fa[1] + f.tz * u + nz * 2.3;
    if !s.on_kerb(cx + nx * 1.2, cz + nz * 1.2) {
        return;
    }
    let y = ground(cx, cz) + 0.15;
    let yaw = kernel::atan2(f.tz, f.tx);
    // Counter and back posts; the lit front faces the road.
    s.b_plain
        .at(cx, cz)
        .box_(cx, y, cz, 3.0, 1.05, 1.3, yaw, &col([0.36, 0.24, 0.14]));
    let cell = STALL as f64 + 16.0 * s.seed();
    facing(
        s.b_street.at(cx, cz),
        cx + nx * 0.66,
        cz + nz * 0.66,
        f.tx,
        f.tz,
        nx,
        nz,
        3.0,
        y,
        y + 2.4,
        cell,
        [1.0, 1.0, 1.0],
        [0.0, 0.0, 1.0, 1.0],
    );
    for sd in [-1.45, 1.45] {
        s.b_plain.at(cx, cz).box_(
            cx + f.tx * sd - nx * 0.6,
            y,
            cz + f.tz * sd - nz * 0.6,
            0.08,
            2.5,
            0.08,
            yaw,
            &col([0.2, 0.15, 0.1]),
        );
    }
    // Sloped striped roof.
    let c =
        [[0.9, 0.2, 0.15], [0.95, 0.75, 0.2], [0.2, 0.45, 0.85]][(s.rng() * 3.0).floor() as usize];
    let a = [
        cx - f.tx * 1.7 - nx * 0.7,
        y + 2.6,
        cz - f.tz * 1.7 - nz * 0.7,
    ];
    let bq = [
        cx + f.tx * 1.7 - nx * 0.7,
        y + 2.6,
        cz + f.tz * 1.7 - nz * 0.7,
    ];
    let cc = [bq[0] + nx * 2.0, y + 2.25, bq[2] + nz * 2.0];
    let d = [a[0] + nx * 2.0, y + 2.25, a[2] + nz * 2.0];
    let w = 3.4 / S_TILE[AWNING_C][0];
    let uvs = [[0.0, 0.0], [w, 0.0], [w, 1.0], [0.0, 1.0]];
    let cell = AWNING_C as f64 + 16.0 * s.seed();
    let st = s.b_street.at(cx, cz);
    st.quad(
        a,
        d,
        cc,
        bq,
        Some([uvs[0], uvs[3], uvs[2], uvs[1]]),
        Some(c),
        cell,
    );
    st.quad(
        a,
        bq,
        cc,
        d,
        Some(uvs),
        Some([c[0] * 0.6, c[1] * 0.6, c[2] * 0.6]),
        cell,
    );
    // Paper lanterns along the roof edge and a steam plume from the pots.
    for sd in [-1.1, 0.0, 1.1] {
        let lx = cx + f.tx * sd + nx * 1.25;
        let lz = cz + f.tz * sd + nz * 1.25;
        s.b_glow.at(cx, cz).box_(
            lx,
            y + 1.75,
            lz,
            0.32,
            0.42,
            0.32,
            yaw,
            &col([3.2, 0.7, 0.3]),
        );
    }
    s.steam.push([cx + nx * 0.2, y + 1.1, cz + nz * 0.2]);
    s.spill.push(Spill {
        x: cx + nx * 2.2,
        z: cz + nz * 2.2,
        r: 3.0,
        col: [0.5, 0.28, 0.1],
        tx: f.tx,
        tz: f.tz,
        long: false,
    });
    s.sign_lights.push(SignLight {
        x: cx + nx * 1.2,
        y: y + 1.8,
        z: cz + nz * 1.2,
        col: "#ff8a3a",
        lamp: false,
        big: false,
    });
}

// ── Bus shelters ─────────────────────────────────────────────────
/// Glass box, lit poster at one end, a bench; on the kerb facing the road.
pub fn bus_shelters(s: &mut Bld) {
    let t = s.t;
    let hw = s.g.hw;
    let mut ss = t.start_s + 260.0;
    while ss < t.length - 150.0 {
        let f = t.frame(ss);
        if f.zone == 0 || f.kappa.abs() > 0.002 {
            ss += 170.0 + s.rng() * 120.0;
            continue;
        }
        let side = if s.rng() < 0.5 { 1.0 } else { -1.0 };
        let lat = side * (hw + 2.0);
        let x = f.x + f.rx * lat;
        let z = f.z + f.rz * lat;
        if !s.on_kerb(x, z)
            || !s.on_kerb(x + f.fx * 3.0, z + f.fz * 3.0)
            || !s.on_kerb(x - f.fx * 3.0, z - f.fz * 3.0)
            || t.distance_to_road(x, z, 20.0).d < hw + 1.2
        {
            ss += 170.0 + s.rng() * 120.0;
            continue;
        }
        let y = ground(x, z) + 0.15;
        let nx = -f.rx * side;
        let nz = -f.rz * side; // toward the road
        let yaw = kernel::atan2(f.fz, f.fx);
        let l = 4.2;
        {
            let p = s.b_plain.at(x, z);
            // Frame posts, roof, back glass (dark tinted).
            for u in [-l / 2.0, l / 2.0] {
                for d in [-0.7, 0.7] {
                    p.box_(
                        x + f.fx * u + nx * d,
                        y,
                        z + f.fz * u + nz * d,
                        0.08,
                        2.4,
                        0.08,
                        yaw,
                        &col([0.3, 0.32, 0.35]),
                    );
                }
            }
            p.box_(
                x,
                y + 2.4,
                z,
                l + 0.3,
                0.12,
                1.7,
                yaw,
                &col([0.25, 0.27, 0.3]),
            );
            facing(
                p,
                x - nx * 0.7,
                z - nz * 0.7,
                f.fx,
                f.fz,
                nx,
                nz,
                l,
                y + 0.3,
                y + 2.2,
                0.0,
                [0.12, 0.16, 0.2],
                [0.0, 0.0, 1.0, 1.0],
            );
            facing(
                p,
                x - nx * 0.72,
                z - nz * 0.72,
                f.fx,
                f.fz,
                -nx,
                -nz,
                l,
                y + 0.3,
                y + 2.2,
                0.0,
                [0.12, 0.16, 0.2],
                [0.0, 0.0, 1.0, 1.0],
            );
        }
        s.put_kit_plain(
            x,
            z,
            "bench",
            &trs(x - nx * 0.35, y, z - nz * 0.35, yaw_z(nx, nz), 1.0),
            None,
        );
        // Poster panel at one end, lit both sides, and a light strip under the roof.
        let ex = x + f.fx * l / 2.0;
        let ez = z + f.fz * l / 2.0;
        let cell = POSTER as f64 + 16.0 * s.seed();
        let st = s.b_street.at(x, z);
        facing(
            st,
            ex + f.fx * 0.06,
            ez + f.fz * 0.06,
            nx,
            nz,
            f.fx,
            f.fz,
            1.4,
            y + 0.2,
            y + 2.2,
            cell,
            [1.0, 1.0, 1.0],
            [0.0, 0.0, 1.0, 1.0],
        );
        facing(
            st,
            ex - f.fx * 0.06,
            ez - f.fz * 0.06,
            nx,
            nz,
            -f.fx,
            -f.fz,
            1.4,
            y + 0.2,
            y + 2.2,
            cell,
            [1.0, 1.0, 1.0],
            [0.0, 0.0, 1.0, 1.0],
        );
        s.b_glow.at(x, z).box_(
            x,
            y + 2.33,
            z,
            l - 0.4,
            0.06,
            0.12,
            yaw,
            &col([2.4, 2.6, 3.0]),
        );
        s.spill.push(Spill {
            x,
            z,
            r: 3.2,
            col: [0.2, 0.24, 0.3],
            tx: f.fx,
            tz: f.fz,
            long: false,
        });
        ss += 170.0 + s.rng() * 120.0;
    }
}

// ── Parked cars ──────────────────────────────────────────────────

struct Slot {
    x: f64,
    y: f64,
    z: f64,
    yaw: f64,
    rd: f64,
    pitch: f64,
}

/// A few traffic models baked to vertex-coloured geometry and copied along
/// the kerbs of the side streets the race doesn't use.
pub fn parked_cars(s: &mut Bld) {
    let (px, pz, hw) = (s.g.px, s.g.pz, s.g.hw);
    let t = s.t;
    let kinds = ["sedan", "hatch", "van", "pickup"];
    let mut slots: [Vec<Slot>; 4] = Default::default();
    for bi in 0..s.blocks.len() {
        let (b_i, b_j, tier, district) = {
            let b = &s.blocks[bi];
            (b.i as f64, b.j as f64, b.tier, b.district)
        };
        if tier != 0 {
            continue;
        }
        let x0 = b_i * px + hw;
        let x1 = (b_i + 1.0) * px - hw;
        let z0 = b_j * pz + hw;
        let z1 = (b_j + 1.0) * pz - hw;
        let dens = if district == 1 { 0.45 } else { 0.12 };
        let mut edge = |s: &mut Bld, ax: f64, az: f64, bx: f64, bz: f64, nx: f64, nz: f64| {
            let l = kernel::hypot(bx - ax, bz - az);
            let tx = (bx - ax) / l;
            let tz = (bz - az) / l;
            let mut u = 12.0;
            while u < l - 12.0 {
                if s.rng() > dens {
                    u += 6.2;
                    continue;
                }
                let x = ax + tx * u + nx * 1.25;
                let z = az + tz * u + nz * 1.25;
                // Only down the side streets you can see from the route.
                let rd = s.route_dist(x, z);
                if rd > 28.0 || t.distance_to_road(x, z, 30.0).d < hw + 5.0 {
                    u += 6.2;
                    continue;
                }
                if s.on_kerb(x, z) {
                    u += 6.2;
                    continue;
                }
                let k = (s.rng()
                    * (if district == 1 {
                        2.6
                    } else {
                        kinds.len() as f64
                    }))
                .floor() as usize;
                let yaw = kernel::atan2(tx, tz) + (if s.rng() < 0.5 { 0.0 } else { PI });
                // Pitch to the hill along the street.
                let y0 = ground(x - tx * 1.4, z - tz * 1.4);
                let y1 = ground(x + tx * 1.4, z + tz * 1.4);
                slots[k].push(Slot {
                    x,
                    y: (y0 + y1) / 2.0,
                    z,
                    yaw,
                    rd,
                    pitch: kernel::atan2(y1 - y0, 2.8)
                        * (if kernel::cos(yaw - kernel::atan2(tx, tz)) > 0.0 {
                            -1.0
                        } else {
                            1.0
                        }),
                });
                u += 6.2;
            }
        };
        // Each kerb, the car on the road side of it (n points into the road).
        edge(s, x0, z0, x1, z0, 0.0, -1.0);
        edge(s, x0, z1, x1, z1, 0.0, 1.0);
        edge(s, x0, z0, x0, z1, -1.0, 0.0);
        edge(s, x1, z0, x1, z1, 1.0, 0.0);
    }
    // Baked into the chunked static geometry (culled per chunk, one draw call
    // each) with the paint colour in the vertex colours.
    let cols = [
        0xb8bcc2, 0x2b2f36, 0xe8e6e0, 0x7a1f1f, 0x1f3a5f, 0x4a5a3a, 0x8c7a5a, 0x5f6670, 0xc8a040,
        0x243048,
    ];
    for (k, kind) in kinds.iter().enumerate() {
        if slots[k].is_empty() {
            continue;
        }
        // Full traffic models where you pass close; a low-poly stand-in (a
        // tenth of the triangles) further down the side streets.
        let full = bake_car(s, kind);
        let low = low_car(kind);
        for sl in &slots[k] {
            let geo = match &full {
                Some(f) if sl.rd < 16.0 => f,
                _ => &low,
            };
            let p = geo.get_attribute("position").expect("position");
            let n = geo.get_attribute("normal").expect("normal");
            let c = geo.get_attribute("color").expect("color");
            let e = Euler::with_order(sl.pitch, sl.yaw, 0.0, EulerOrder::YXZ);
            let m = Matrix4::compose(
                Vector3::new(sl.x, sl.y, sl.z),
                Quaternion::from_euler(&e),
                Vector3::new(1.0, 1.0, 1.0),
            );
            let n3 = Matrix3::get_normal_matrix(&m);
            let mut paint = Color::hex(cols[(s.rng() * cols.len() as f64).floor() as usize]);
            paint.convert_srgb_to_linear();
            let b = s.b_car.at(sl.x, sl.z);
            let mut cs: Vec<f64> = Vec::with_capacity(p.count() * 3);
            for i in 0..p.count() {
                let v = p.get_vector3(i).apply_matrix4(&m);
                b.pos.extend_from_slice(&[v.x, v.y, v.z]);
                let v = n.get_vector3(i).apply_matrix3(&n3).normalize();
                b.nor.extend_from_slice(&[v.x, v.y, v.z]);
                b.uv.extend_from_slice(&[0.0, 0.0]);
                // Magenta marks paint (see bakeCar).
                let r = c.get_x(i);
                let g = c.get_y(i);
                let bb = c.get_z(i);
                if r > 0.99 && g < 0.01 && bb > 0.99 {
                    cs.extend_from_slice(&[paint.r, paint.g, paint.b]);
                } else {
                    cs.extend_from_slice(&[r, g, bb]);
                }
            }
            b.col.as_mut().expect("coloured").extend_from_slice(&cs);
        }
    }
}

/// Low-poly parked car: extruded side profiles for body and cabin, simple
/// wheels and lamp blocks, vertex coloured (magenta = paint).
fn low_car(kind: &str) -> BufferGeometry {
    let tall = kind == "van";
    let bed = kind == "pickup";
    let prof = |pts: &[[f64; 2]], depth: f64, c: P3| -> (BufferGeometry, P3) {
        let shape = Shape::from_points(
            &pts.iter()
                .map(|&[a, b]| Vector2::new(a, b))
                .collect::<Vec<_>>(),
        );
        let mut g = extrude_geometry(&[shape], &ExtrudeOptions::flat(depth));
        g.translate(0.0, 0.0, -depth / 2.0);
        g.rotate_y(-PI / 2.0); // profile x (length) onto +z
        (g, c)
    };
    const PAINT: P3 = [1.0, 0.0, 1.0];
    const GLASS: P3 = [0.06, 0.08, 0.11];
    const DARK: P3 = [0.05, 0.05, 0.05];
    let mut parts = vec![
        prof(
            &[
                [-2.25, 0.3],
                [2.25, 0.3],
                [2.32, 0.62],
                [2.12, 0.84],
                [-2.2, 0.88],
                [-2.3, 0.6],
            ],
            1.76,
            PAINT,
        ),
        if tall {
            prof(
                &[
                    [-2.2, 0.86],
                    [1.3, 0.86],
                    [1.9, 1.3],
                    [1.8, 1.95],
                    [-2.2, 1.98],
                ],
                1.7,
                GLASS,
            )
        } else if bed {
            prof(
                &[[-0.2, 0.86], [1.1, 0.86], [0.55, 1.5], [-0.2, 1.52]],
                1.6,
                GLASS,
            )
        } else {
            prof(
                &[
                    [-1.4, 0.86],
                    [0.9, 0.86],
                    [0.25, 1.36],
                    [-1.05, 1.38],
                    [-1.62, 0.9],
                ],
                1.52,
                GLASS,
            )
        },
    ];
    let b = |w: f64, h: f64, d: f64, x: f64, y: f64, z: f64, c: P3| {
        let mut g = box_geometry(w, h, d, 1.0, 1.0, 1.0);
        g.translate(x, y, z);
        (g, c)
    };
    if tall {
        parts.push(b(1.74, 0.06, 3.3, 0.0, 1.98, -0.35, PAINT));
    } else if !bed {
        parts.push(b(1.46, 0.05, 1.0, 0.0, 1.39, 0.35, PAINT));
    } else {
        parts.push(b(1.7, 0.12, 2.0, 0.0, 0.9, 1.2, DARK));
    }
    for sx in [-0.82, 0.82] {
        parts.push(b(0.36, 0.1, 0.05, sx * 0.9, 0.7, 2.29, [0.7, 0.7, 0.66]));
        parts.push(b(
            0.34,
            0.12,
            0.05,
            sx * 0.9,
            0.72,
            -2.26,
            [0.5, 0.03, 0.02],
        ));
    }
    for [x, z] in [[-0.8, 1.45], [0.8, 1.45], [-0.8, -1.45], [0.8, -1.45]] {
        let mut w = cylinder_geometry(0.33, 0.33, 0.24, 8.0, 1.0, false, 0.0, PI2);
        w.rotate_z(PI / 2.0);
        w.translate(x, 0.33, z);
        parts.push((w, DARK));
    }
    let geos: Vec<BufferGeometry> = parts
        .into_iter()
        .map(|(g, c)| {
            let mut h = if g.index.is_some() {
                g.to_non_indexed()
            } else {
                g
            };
            let names: Vec<String> = h.attributes.iter().map(|(n, _)| n.clone()).collect();
            for a in names {
                if a != "position" {
                    h.delete_attribute(&a);
                }
            }
            h.compute_vertex_normals();
            let n = h.position().count();
            let mut colr = vec![0.0f64; n * 3];
            for i in 0..n {
                colr[i * 3..i * 3 + 3].copy_from_slice(&c);
            }
            h.set_attribute("color", BufferAttribute::from_f64(&colr, 3));
            h
        })
        .collect();
    let refs: Vec<&BufferGeometry> = geos.iter().collect();
    merge_geometries(&refs, false).expect("the low car merges")
}

/// `bakeCar(buildVehicle, kind)`: a traffic model's meshes merged into one
/// geometry, each vertex coloured by its material (magenta for paint, dim
/// red for tail lamps).
fn bake_car(s: &mut Bld, kind: &str) -> Option<BufferGeometry> {
    let v = build_vehicle(
        s.graph,
        s.textures,
        kind,
        &CarOpts {
            color: Some(0xffffff),
            seed: 3,
            lod: Some(Lod::Low),
            ..CarOpts::default()
        },
    )?;
    // `v.setHeadlights?.(0)` changes only emissive intensities, which the
    // bake does not read.
    let mut parts: Vec<BufferGeometry> = Vec::new();
    for (id, world) in crate::mountain::kit::world_matrices(s.graph, v.root) {
        let o = s.graph.get(id);
        if !o.ty.is_mesh() {
            continue;
        }
        let Some(geo) = o.geometry else { continue };
        let mats = o.materials.clone();
        let multi = o.multi_material;
        let mut g = s.graph.geometry(geo).clone();
        g.apply_matrix4(&world);
        let groups: Vec<(usize, usize, usize)> = if multi && !g.groups.is_empty() {
            g.groups
                .iter()
                .map(|gr| (gr.start, gr.count, gr.material_index))
                .collect()
        } else {
            let count = match &g.index {
                Some(ix) => ix.count(),
                None => g.position().count(),
            };
            vec![(0, count, 0)]
        };
        let mut g = if g.index.is_some() {
            g.to_non_indexed()
        } else {
            g
        };
        let n = g.position().count();
        let mut colr = vec![0.0f32; n * 3];
        for (start, count, mi) in groups {
            let m = *mats.get(mi).unwrap_or(&mats[0]);
            let mat = s.graph.material(m);
            let mut c = if m == v.paint {
                Color::new(1.0, 0.0, 1.0)
            } else {
                mat.color("color").unwrap_or(Color::new(0.1, 0.1, 0.1))
            };
            // Tail lamps read as dim red lenses; paint is marked for recolouring.
            if let Some(e) = mat.color("emissive")
                && e.r > 0.9
                && e.g < 0.2
            {
                c.set_rgb(0.5, 0.03, 0.02);
            }
            let s0 = start;
            let s1 = n.min(s0 + count);
            for i in s0..s1 {
                colr[i * 3] = c.r as f32;
                colr[i * 3 + 1] = c.g as f32;
                colr[i * 3 + 2] = c.b as f32;
            }
        }
        let names: Vec<String> = g.attributes.iter().map(|(n, _)| n.clone()).collect();
        for a in names {
            if a != "position" && a != "normal" {
                g.delete_attribute(&a);
            }
        }
        g.set_attribute("color", BufferAttribute::from_f32(colr, 3));
        parts.push(g);
    }
    if parts.is_empty() {
        return None;
    }
    let refs: Vec<&BufferGeometry> = parts.iter().collect();
    merge_geometries(&refs, false)
}

// ── Steam ────────────────────────────────────────────────────────

pub const STEAM_VERTEX: &str = "
      attribute vec2 aSeed;
      uniform float uTime, uScale;
      varying float vA;
      varying float vS;
      void main() {
        float life = fract(uTime * 0.28 + aSeed.x + aSeed.y * 0.1);
        vec3 p = position + vec3(sin(aSeed.y * 40.0 + life * 3.0) * 0.5 * life, life * 4.5, cos(aSeed.y * 23.0) * 0.4 * life);
        vec4 mv = modelViewMatrix * vec4(p, 1.0);
        gl_Position = projectionMatrix * mv;
        gl_PointSize = (0.8 + life * 3.2) * uScale / -mv.z;
        // Fade in, thin out as it rises, and never fill the camera.
        vA = smoothstep(0.0, 0.15, life) * (1.0 - life) * smoothstep(2.0, 9.0, -mv.z);
        vS = aSeed.y;
      }";

pub const STEAM_FRAGMENT: &str = "
      varying float vA;
      varying float vS;
      void main() {
        vec2 p = gl_PointCoord - 0.5;
        float r2 = dot(p, p) * 4.0;
        // Lumpy soft puff.
        float lump = 0.75 + 0.25 * sin(atan(p.y, p.x) * 3.0 + vS * 20.0);
        float a = max(0.0, 1.0 - r2 / lump);
        a = a * a * vA * 0.16;
        gl_FragColor = vec4(vec3(0.6, 0.58, 0.64) * a, a);
      }";

/// Soft puffs rising from street vents and stall pots, lit by the street.
/// All animation is in the vertex shader off one time uniform.
pub fn build_steam(s: &mut Bld) {
    if s.steam.is_empty() {
        return;
    }
    const N: usize = 10;
    // The page's `Math.random` at the point the JS draws (D611).
    let mut random = crate::valley::page_random(STEAM_RANDOM_AT);
    let mut pos = Vec::with_capacity(s.steam.len() * N * 3);
    let mut seed = Vec::with_capacity(s.steam.len() * N * 2);
    for e in &s.steam {
        for k in 0..N {
            pos.extend_from_slice(e);
            seed.extend_from_slice(&[k as f64 / N as f64, random.next_f64()]);
        }
    }
    let mut g = BufferGeometry::new();
    g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
    g.set_attribute("aSeed", BufferAttribute::from_f64(&seed, 2));
    g.compute_bounding_sphere();
    if let Some(bs) = &mut g.bounding_sphere {
        bs.radius += 6.0;
    }
    let m = s.graph.add_material(
        Material::shader()
            .uniform("uTime", num(0.0))
            .uniform("uScale", num(600.0))
            .shader_source(STEAM_VERTEX, STEAM_FRAGMENT)
            .set("transparent", true)
            .set("depthWrite", false)
            .set("blending", three::CUSTOM_BLENDING as f64)
            .set("blendSrc", 201.0)
            .set("blendDst", 205.0)
            .kind(MaterialKind::Steam, None),
    );
    let geo = s.graph.add_geometry(g);
    let pts = s.graph.drawable(NodeType::Points, geo, m);
    s.graph
        .get_mut(pts)
        .user_data
        .insert("dynamic".into(), Value::Bool(true));
    s.graph.add(s.group, pts);
    s.anims.push(Box::new(anim::Steam { time: 0.0, mat: m }));
}

// ── Wet road: puddles and light spill ────────────────────────────
/// Additive coloured blobs: puddles on the road that catch the nearest sign
/// or lamp, and warm pools on the pavement in front of lit windows.
pub fn build_puddles_and_spill(s: &mut Bld) {
    let t = s.t;
    let hw = s.g.hw;
    let mut pos: Vec<f64> = Vec::new();
    let mut uv: Vec<f64> = Vec::new();
    let mut colv: Vec<f64> = Vec::new();
    let quad = |pos: &mut Vec<f64>,
                uv: &mut Vec<f64>,
                colv: &mut Vec<f64>,
                cx: f64,
                cy: &dyn Fn(f64, f64) -> f64,
                cz: f64,
                ax: f64,
                az: f64,
                a: f64,
                bx: f64,
                bz: f64,
                b: f64,
                c: P3| {
        let p = [
            [cx - ax * a - bx * b, cz - az * a - bz * b],
            [cx + ax * a - bx * b, cz + az * a - bz * b],
            [cx + ax * a + bx * b, cz + az * a + bz * b],
            [cx - ax * a + bx * b, cz - az * a + bz * b],
        ];
        let u = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let cr =
            (p[1][1] - p[0][1]) * (p[2][0] - p[0][0]) - (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]);
        let order = if cr >= 0.0 {
            [0, 1, 2, 0, 2, 3]
        } else {
            [0, 2, 1, 0, 3, 2]
        };
        for q in order {
            pos.extend_from_slice(&[p[q][0], cy(p[q][0], p[q][1]), p[q][1]]);
            uv.extend_from_slice(&u[q]);
            colv.extend_from_slice(&c);
        }
    };
    // Puddles: along the route near the kerbs, tinted by the brightest light nearby.
    let mut ss = 20.0;
    while ss < t.length - 20.0 {
        let f = t.frame(ss);
        let sign = if s.rng() < 0.5 { -1.0 } else { 1.0 };
        let lat = sign * (hw - 0.8 - s.rng() * 2.2);
        let x = f.x + f.rx * lat;
        let z = f.z + f.rz * lat;
        let mut best: Option<&SignLight> = None;
        let mut bd = 18.0;
        for l in &s.sign_lights {
            let d = kernel::hypot(l.x - x, l.z - z);
            if d < bd {
                bd = d;
                best = Some(l);
            }
        }
        let c = match best {
            Some(l) => Color::style(l.col),
            None => Color::new(0.5, 0.45, 0.55),
        };
        let k = (if best.is_some() {
            0.3 * (1.0 - bd / 22.0)
        } else {
            0.05
        }) + 0.04;
        let sz = 0.8 + s.rng() * 1.6;
        let a = sz * (1.2 + s.rng());
        let surf = |px: f64, pz: f64| s.surface_at(px, pz) + 0.04;
        quad(
            &mut pos,
            &mut uv,
            &mut colv,
            x,
            &surf,
            z,
            f.fx,
            f.fz,
            a,
            f.rx,
            f.rz,
            sz,
            [c.r * k, c.g * k, c.b * k],
        );
        ss += 9.0 + s.rng() * 16.0;
    }
    let pt = puddle_texture(s.textures);
    let pt = s.graph.cached_texture(&pt, Layer::Main, "");
    let pmat = blob_material(s.graph, pt);
    let mk =
        |s: &mut Bld, m: MaterialId, pos: &mut Vec<f64>, uv: &mut Vec<f64>, colv: &mut Vec<f64>| {
            if pos.is_empty() {
                return;
            }
            let mut g = BufferGeometry::new();
            g.set_attribute("position", BufferAttribute::from_f64(pos, 3));
            g.set_attribute("uv", BufferAttribute::from_f64(uv, 2));
            g.set_attribute("color", BufferAttribute::from_f64(colv, 3));
            g.compute_bounding_sphere();
            let geo = s.graph.add_geometry(g);
            let mesh = static_mesh(
                s.graph,
                geo,
                m,
                &StaticOpts {
                    receive: false,
                    ..StaticOpts::default()
                },
            );
            s.graph.get_mut(mesh).render_order = 2.0;
            s.graph.add(s.group, mesh);
            pos.clear();
            uv.clear();
            colv.clear();
        };
    mk(s, pmat, &mut pos, &mut uv, &mut colv);
    // Spill: soft ellipses on the pavement.
    for sp in &s.spill {
        let k = if sp.long { 0.55 } else { 0.6 };
        let a = if sp.long { sp.r * 1.3 } else { sp.r };
        let b = if sp.long { 2.2 } else { sp.r };
        quad(
            &mut pos,
            &mut uv,
            &mut colv,
            sp.x,
            &|px, pz| ground(px, pz) + 0.19,
            sp.z,
            sp.tx,
            sp.tz,
            a,
            -sp.tz,
            sp.tx,
            b,
            [sp.col[0] * k, sp.col[1] * k, sp.col[2] * k],
        );
    }
    let glow = s.textures.glow_texture();
    let glow = s.graph.cached_texture(&glow, Layer::Main, "");
    let gmat = blob_material(s.graph, glow);
    mk(s, gmat, &mut pos, &mut uv, &mut colv);
}

/// The puddles' and the spill's additive material.
fn blob_material(graph: &mut SceneGraph, map: TextureId) -> MaterialId {
    graph.add_material(
        Material::basic()
            .set("map", map)
            .set("vertexColors", true)
            .set("transparent", true)
            .set("depthWrite", false)
            .set("blending", ADDITIVE)
            .set("polygonOffset", true)
            .set("polygonOffsetFactor", -4.0)
            .set("polygonOffsetUnits", -4.0),
    )
}

// ── Overhead wires ───────────────────────────────────────────────
pub fn build_wires(s: &mut Bld) {
    if s.cables.is_empty() {
        return;
    }
    let mut g = BufferGeometry::new();
    g.set_attribute("position", BufferAttribute::from_f64(&s.cables, 3));
    g.compute_bounding_sphere();
    let m = s
        .graph
        .add_material(Material::line_basic().set("color", 0x0c0c0e));
    let geo = s.graph.add_geometry(g);
    let l = s.graph.drawable(NodeType::LineSegments, geo, m);
    s.graph.add(s.group, l);
}

/// A sagging wire from a to b, appended as line segments.
pub fn wire(s: &mut Bld, a: P3, b: P3, sag: f64, n: u32) {
    let mut prev = a;
    for q in 1..=n {
        let u = f64::from(q) / f64::from(n);
        let p = [
            a[0] + (b[0] - a[0]) * u,
            a[1] + (b[1] - a[1]) * u - sag * 4.0 * u * (1.0 - u),
            a[2] + (b[2] - a[2]) * u,
        ];
        s.cables.extend_from_slice(&prev);
        s.cables.extend_from_slice(&p);
        prev = p;
    }
}
