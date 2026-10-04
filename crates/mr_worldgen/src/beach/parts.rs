//! Port of `src/world/beach/parts.js`: the Seabright building kit (roadmap
//! WP 7.1; Desert uses the motel, the gas station and the diner).
//!
//! Every function draws into a [`Builder`] in its current frame: local +Z
//! is the street front, X runs along the street, Y is up and y = 0 is the
//! ground. Buildings occupy z ∈ [−depth, 0]. Random choices draw from the
//! `rng` passed in, in the JS order; an options object whose defaults draw
//! from it is a struct of `Option`s, drawn in the JS destructuring order
//! where `None`.

// The kit keeps the JS argument lists (DECISIONS D130, D190).
#![allow(clippy::too_many_arguments)]

use std::f64::consts::{FRAC_1_SQRT_2, PI};

use mr_math::{Mulberry32, kernel, rpick, rrange};

use crate::builder::Builder;
use crate::three_geom::{
    BufferGeometry, ExtrudeOptions, Shape, Vector3, cone_geometry, cylinder_geometry,
    extrude_geometry, sphere_geometry,
};

use super::atlas::{Rect, sign_geometry};

fn v(x: f64, y: f64, z: f64) -> Vector3 {
    Vector3::new(x, y, z)
}

pub const WALLS: [&str; 9] = [
    "wPink", "wMint", "wYellow", "wBlue", "wWhite", "wPeach", "wLilac", "wTeal", "wSand",
];
const AWNINGS: [&str; 4] = ["awnRed", "awnBlue", "awnYellow", "awnGreen"];
const TRIMS: [&str; 6] = ["trim", "trim", "wWhite", "wSand", "wTeal", "woodDark"];
const SHUTTERS: [&str; 5] = ["wTeal", "wBlue", "paintRed", "woodDark", "wMint"];

/// `new THREE.CylinderGeometry(rt, rb, h, radial)`.
fn cyl(rt: f64, rb: f64, h: f64, radial: f64) -> BufferGeometry {
    cylinder_geometry(rt, rb, h, radial, 1.0, false, 0.0, 2.0 * PI)
}

/// `new THREE.ConeGeometry(r, h, radial, 1, open)`.
fn cone(r: f64, h: f64, radial: f64, open: bool) -> BufferGeometry {
    cone_geometry(r, h, radial, 1.0, open, 0.0, 2.0 * PI)
}

/// `new THREE.SphereGeometry(r, ws, hs)`.
fn sphere(r: f64, ws: f64, hs: f64) -> BufferGeometry {
    sphere_geometry(r, ws, hs, 0.0, 2.0 * PI, 0.0, PI)
}

/// `B.box(key, w, h, d, x, y, z)`.
fn bx(b: &mut Builder, key: &str, w: f64, h: f64, d: f64, x: f64, y: f64, z: f64) {
    b.box_(key, w, h, d, x, y, z, 0.0, 0.0, 0.0);
}

/// `B.cbox(key, w, h, d, x, y, z)`.
fn cb(b: &mut Builder, key: &str, w: f64, h: f64, d: f64, x: f64, y: f64, z: f64) {
    b.cbox(key, w, h, d, x, y, z, [0.0; 3]);
}

/// `B.put(key, geo, x, y, z)`.
fn put(b: &mut Builder, key: &str, geo: &BufferGeometry, x: f64, y: f64, z: f64) {
    b.put(key, geo, x, y, z, [0.0; 3], [1.0; 3]);
}

/// `rng() < p`.
fn chance(rng: &mut Mulberry32, p: f64) -> bool {
    rng.next_f64() < p
}

/// Row of windows on the front face. Each pane sits in a trim surround a
/// little proud of the wall; some rows get painted shutters.
/// (`spacing = 2.4, w = 1.4, litP = 0.35, shutter = null` in the JS.)
fn windows(
    b: &mut Builder,
    rng: &mut Mulberry32,
    x0: f64,
    x1: f64,
    y: f64,
    h: f64,
    z: f64,
    spacing: f64,
    w: f64,
    lit_p: f64,
    shutter: Option<&str>,
) {
    let n = mr_math::js::max(1.0, ((x1 - x0) / spacing).floor());
    let step = (x1 - x0) / n;
    let mut i = 0.0;
    while i < n {
        let x = x0 + step * (i + 0.5);
        cb(b, "trim", w + 0.26, h + 0.22, 0.08, x, y + h / 2.0, z);
        let key = if chance(rng, lit_p) {
            "glassLit"
        } else {
            "glass"
        };
        cb(b, key, w, h, 0.1, x, y + h / 2.0, z + 0.02);
        cb(b, "trim", w + 0.36, 0.12, 0.24, x, y - 0.06, z + 0.08);
        if let Some(sh) = shutter {
            for sx in [-1.0, 1.0] {
                cb(
                    b,
                    sh,
                    w * 0.42,
                    h + 0.1,
                    0.06,
                    x + sx * (w / 2.0 + w * 0.21 + 0.14),
                    y + h / 2.0,
                    z + 0.03,
                );
            }
        }
        i += 1.0;
    }
}

/// Side windows (on ±X faces). (`spacing = 2.6, litP = 0.3`.)
fn side_windows(
    b: &mut Builder,
    rng: &mut Mulberry32,
    side: f64,
    x_face: f64,
    z0: f64,
    z1: f64,
    y: f64,
    h: f64,
    spacing: f64,
    lit_p: f64,
) {
    let n = mr_math::js::max(1.0, ((z1 - z0) / spacing).floor());
    let step = (z1 - z0) / n;
    let mut i = 0.0;
    while i < n {
        let z = z0 + step * (i + 0.5);
        let key = if chance(rng, lit_p) {
            "glassLit"
        } else {
            "glass"
        };
        cb(b, key, 0.1, h, 1.2, x_face + side * 0.02, y + h / 2.0, z);
        i += 1.0;
    }
}

/// `parapet(B, key, w, d, H, t = 0.25, ph = 0.7)`.
fn parapet(b: &mut Builder, key: &str, w: f64, d: f64, hh: f64, t: f64, ph: f64) {
    bx(b, key, w + 0.1, ph, t, 0.0, hh, -t / 2.0 + 0.05);
    bx(b, key, w + 0.1, ph, t, 0.0, hh, -d + t / 2.0 - 0.05);
    bx(b, key, t, ph, d, -w / 2.0 + t / 2.0 - 0.05, hh, -d / 2.0);
    bx(b, key, t, ph, d, w / 2.0 - t / 2.0 + 0.05, hh, -d / 2.0);
}

fn roof_clutter(b: &mut Builder, rng: &mut Mulberry32, w: f64, d: f64, hh: f64) {
    let n = rrange(rng, 1.0, 4.0).floor();
    let mut i = 0.0;
    while i < n {
        let bw = rrange(rng, 1.0, 2.2);
        let bh = rrange(rng, 0.8, 1.4);
        let bd = rrange(rng, 1.0, 2.2);
        let x = rrange(rng, -w / 3.0, w / 3.0);
        let z = -rrange(rng, d * 0.3, d * 0.7);
        bx(b, "metal", bw, bh, bd, x, hh, z);
        i += 1.0;
    }
}

/// Striped canvas awning sloping out over the sidewalk
/// (`depth = 2.2, drop = 0.7`).
fn awning(b: &mut Builder, key: &str, w: f64, y: f64, depth: f64, drop: f64) {
    let a = kernel::atan2(drop, depth);
    let l = kernel::hypot(depth, drop);
    b.cbox(
        key,
        w,
        0.08,
        l,
        0.0,
        y - drop / 2.0,
        depth / 2.0,
        [a, 0.0, 0.0],
    );
    cb(b, key, w, 0.4, 0.06, 0.0, y - drop - 0.2, depth);
}

/// `sign(B, rect, w, h, x, y, z, ry = 0, key = 'signs')`.
pub fn sign(
    b: &mut Builder,
    rect: &Rect,
    w: f64,
    h: f64,
    x: f64,
    y: f64,
    z: f64,
    ry: f64,
    key: &str,
) {
    b.put(
        key,
        &sign_geometry(rect, w, h),
        x,
        y,
        z,
        [0.0, ry, 0.0],
        [1.0; 3],
    );
}

// ── Storefronts ─────────────────────────────────────────────────────────

/// `shop`'s options: `{ w = 12, d = 14, rect = null, twoStory = rng() <
/// 0.4, wall = rpick(rng, WALLS), blade = null }`.
#[derive(Clone, Copy, Debug)]
pub struct ShopOpts<'a> {
    pub w: f64,
    pub d: f64,
    pub rect: Option<Rect>,
    pub two_story: Option<bool>,
    pub wall: Option<&'a str>,
    pub blade: Option<Rect>,
}

impl Default for ShopOpts<'_> {
    fn default() -> Self {
        ShopOpts {
            w: 12.0,
            d: 14.0,
            rect: None,
            two_story: None,
            wall: None,
            blade: None,
        }
    }
}

/// One or two storey shop with display windows, awning and a sign board.
/// Returns `{ H }`.
pub fn shop(b: &mut Builder, rng: &mut Mulberry32, o: ShopOpts) -> f64 {
    let ShopOpts { w, d, rect, .. } = o;
    let two_story = match o.two_story {
        Some(t) => t,
        None => chance(rng, 0.4),
    };
    let wall = match o.wall {
        Some(wl) => wl,
        None => *rpick(rng, &WALLS),
    };
    let blade = o.blade;
    let hh = if two_story { 7.0 } else { 4.2 };
    let trim = *rpick(rng, &TRIMS);
    bx(b, wall, w, hh, d, 0.0, 0.0, -d / 2.0);
    bx(b, "concrete", w + 0.2, 0.3, d + 0.2, 0.0, -0.1, -d / 2.0);
    // Pilasters at the corners, a tiled bulkhead under the display window,
    // the window itself and a glazed door with a transom.
    for sx in [-1.0, 1.0] {
        bx(b, trim, 0.45, hh, 0.18, sx * (w / 2.0 - 0.22), 0.0, 0.06);
    }
    cb(b, "glass", w * 0.62, 2.1, 0.1, -w * 0.12, 1.75, 0.03);
    cb(b, trim, w * 0.62 + 0.3, 0.2, 0.2, -w * 0.12, 2.85, 0.07);
    let bulk = *rpick(rng, &["woodDark", "paintRed", "wTeal", "black"]);
    bx(b, bulk, w * 0.62 + 0.25, 0.7, 0.16, -w * 0.12, 0.0, 0.06);
    cb(b, "trim", w * 0.62 + 0.25, 0.1, 0.25, -w * 0.12, 0.72, 0.1);
    cb(b, trim, 1.5, 2.9, 0.1, w * 0.33, 1.45, 0.02);
    cb(b, "glass", 1.0, 1.4, 0.1, w * 0.33, 1.45, 0.05);
    cb(b, "woodDark", 1.2, 0.7, 0.12, w * 0.33, 0.35, 0.05);
    cb(b, "glass", 1.2, 0.4, 0.1, w * 0.33, 2.62, 0.05);
    let awn = *rpick(rng, &AWNINGS);
    awning(b, awn, w * 0.95, 3.35, 2.2, 0.7);
    if let Some(rect) = rect {
        bx(b, "trim", w * 0.9, 1.25, 0.2, 0.0, 3.5, 0.02);
        sign(b, &rect, w * 0.84, 1.05, 0.0, 4.12, 0.14, 0.0, "signs");
    }
    // A projecting blade sign on a bracket by the door.
    if let Some(blade) = blade {
        let bxx = w / 2.0 - 0.5;
        let up = if two_story { 1.8 } else { 0.2 };
        cb(b, "black", 0.08, 0.08, 1.1, bxx, 3.55 + up, 0.55);
        b.cbox(
            "black",
            0.9,
            1.5,
            0.1,
            bxx,
            2.75 + up,
            1.0,
            [0.0, PI / 2.0, 0.0],
        );
        sign(
            b,
            &blade,
            0.8,
            1.4,
            bxx + 0.06,
            2.75 + up,
            1.0,
            PI / 2.0,
            "signs",
        );
        sign(
            b,
            &blade,
            0.8,
            1.4,
            bxx - 0.06,
            2.75 + up,
            1.0,
            -PI / 2.0,
            "signs",
        );
    }
    if two_story {
        let shutter = if chance(rng, 0.4) {
            Some(*rpick(rng, &SHUTTERS))
        } else {
            None
        };
        windows(
            b,
            rng,
            -w / 2.0 + 0.8,
            w / 2.0 - 0.8,
            4.7,
            1.5,
            0.0,
            2.6,
            1.3,
            0.4,
            shutter,
        );
    }
    // Cornice, and on some a stepped false front above the parapet.
    bx(b, trim, w + 0.3, 0.3, 0.35, 0.0, hh - 0.1, 0.05);
    parapet(b, wall, w, d, hh, 0.25, 0.7);
    if chance(rng, 0.45) {
        bx(b, wall, w * 0.5, 0.9, 0.25, 0.0, hh + 0.7, -0.08);
        bx(b, trim, w * 0.5 + 0.2, 0.18, 0.32, 0.0, hh + 1.6, -0.06);
    }
    bx(
        b,
        "roofTar",
        w - 0.3,
        0.1,
        d - 0.3,
        0.0,
        hh - 0.05,
        -d / 2.0,
    );
    roof_clutter(b, rng, w, d, hh);
    hh
}

/// Surf shop: a shop with a giant surfboard on the roof.
pub fn surf_shop(b: &mut Builder, rng: &mut Mulberry32, rect: &Rect) {
    let w = 14.0;
    let d = 13.0;
    let wall = *rpick(rng, &["wTeal", "wYellow", "wBlue"]);
    let hh = shop(
        b,
        rng,
        ShopOpts {
            w,
            d,
            rect: Some(*rect),
            two_story: Some(false),
            wall: Some(wall),
            blade: None,
        },
    );
    // Board standing on the roof.
    let g = cyl(1.0, 1.0, 1.0, 18.0);
    b.put(
        "wTeal",
        &g,
        0.0,
        hh + 3.6,
        -3.0,
        [PI / 2.0, 0.0, 0.0],
        [1.3, 0.18, 3.8],
    );
    bx(b, "metal", 0.15, 1.2, 0.15, -0.6, hh, -3.1);
    bx(b, "metal", 0.15, 1.2, 0.15, 0.6, hh, -3.1);
    // Boards racked outside.
    for i in 0..4 {
        let key = if i % 2 == 1 { "boardB" } else { "wTeal" };
        b.put(
            key,
            &g,
            -w / 2.0 + 1.0 + i as f64 * 0.6,
            1.3,
            0.8,
            [0.2, 0.0, 0.08],
            [0.28, 2.4, 0.04],
        );
    }
}

/// Streamline diner with a rounded end, chrome band and a neon roof sign.
/// (The JS takes `rng` and draws nothing from it.)
pub fn diner(b: &mut Builder, _rng: &mut Mulberry32, neon_rect: &Rect) {
    let w = 18.0;
    let d = 10.0;
    let hh = 4.4;
    bx(b, "concrete", w + 1.0, 0.4, d + 1.0, 0.0, -0.1, -d / 2.0);
    bx(b, "wWhite", w - d / 2.0, hh, d, -d / 4.0, 0.0, -d / 2.0);
    let c = cylinder_geometry(d / 2.0, d / 2.0, hh, 20.0, 1.0, false, 0.0, PI);
    put(b, "wWhite", &c, w / 2.0 - d / 2.0, hh / 2.0, -d / 2.0);
    let band = cylinder_geometry(
        d / 2.0 + 0.06,
        d / 2.0 + 0.06,
        0.4,
        20.0,
        1.0,
        true,
        0.0,
        PI,
    );
    put(b, "metal", &band, w / 2.0 - d / 2.0, 3.2, -d / 2.0);
    cb(b, "metal", w - d / 2.0, 0.4, 0.1, -d / 4.0, 3.2, 0.03);
    cb(b, "paintRed", w - d / 2.0, 0.5, 0.1, -d / 4.0, 0.55, 0.03);
    b.put(
        "paintRed",
        &band,
        w / 2.0 - d / 2.0,
        0.55,
        -d / 2.0,
        [0.0; 3],
        [1.0, 1.25, 1.0],
    );
    // Ribbon windows.
    cb(
        b,
        "glassLit",
        w - d / 2.0 - 1.0,
        1.6,
        0.1,
        -d / 4.0,
        1.9,
        0.05,
    );
    let wcyl = cylinder_geometry(
        d / 2.0 + 0.03,
        d / 2.0 + 0.03,
        1.6,
        20.0,
        1.0,
        true,
        0.0,
        PI,
    );
    put(b, "glassLit", &wcyl, w / 2.0 - d / 2.0, 1.9, -d / 2.0);
    // Flat roof + neon sign on a frame.
    bx(b, "roofTar", w - d / 2.0, 0.2, d, -d / 4.0, hh, -d / 2.0);
    bx(b, "metal", 0.15, 1.2, 0.15, -3.0, hh, -1.0);
    bx(b, "metal", 0.15, 1.2, 0.15, 3.0, hh, -1.0);
    bx(b, "black", 7.4, 2.4, 0.3, 0.0, hh + 1.1, -1.1);
    sign(b, neon_rect, 7.0, 2.1, 0.0, hh + 2.3, -0.93, 0.0, "neon");
}

/// Taco stand: a small hut with a counter, awning and picnic tables.
pub fn taco_stand(b: &mut Builder, _rng: &mut Mulberry32, rect: Option<&Rect>) {
    let w = 7.0;
    let d = 5.0;
    let hh = 3.2;
    bx(b, "wYellow", w, hh, d, 0.0, 0.0, -d / 2.0 - 2.0);
    cb(b, "woodDark", w * 0.7, 1.1, 0.2, 0.0, 1.0, -2.0 + 0.1);
    cb(b, "glass", w * 0.7, 0.9, 0.08, 0.0, 1.9, -2.0 + 0.05);
    awning(b, "awnGreen", w + 0.6, 3.1, 1.8, 0.5);
    bx(
        b,
        "roofTile",
        w + 0.6,
        0.25,
        d + 0.6,
        0.0,
        hh,
        -d / 2.0 - 2.0,
    );
    if let Some(rect) = rect {
        sign(b, rect, 4.6, 1.3, 0.0, hh + 1.0, -2.1, 0.0, "signs");
        bx(b, "woodDark", 4.8, 1.5, 0.12, 0.0, hh + 0.25, -2.25);
    }
    for (tx, tz) in [(-3.5, 3.0), (0.5, 3.4), (4.0, 2.6)] {
        picnic_table(b, tx, tz);
    }
}

pub fn picnic_table(b: &mut Builder, x: f64, z: f64) {
    bx(b, "wood", 1.8, 0.08, 0.8, x, 0.75, z);
    bx(b, "wood", 1.8, 0.06, 0.3, x, 0.45, z - 0.65);
    bx(b, "wood", 1.8, 0.06, 0.3, x, 0.45, z + 0.65);
    for sx in [-0.7, 0.7] {
        bx(b, "woodDark", 0.1, 0.75, 1.5, x + sx, 0.0, z);
    }
}

/// `motel`'s options: `{ w = 40, signRect, neonRect, wall = rpick(rng,
/// ['wPink', 'wPeach', 'wTeal', 'wMint']) }`.
#[derive(Clone, Copy, Debug)]
pub struct MotelOpts<'a> {
    pub w: f64,
    pub sign_rect: Option<Rect>,
    pub neon_rect: Option<Rect>,
    pub wall: Option<&'a str>,
}

impl Default for MotelOpts<'_> {
    fn default() -> Self {
        MotelOpts {
            w: 40.0,
            sign_rect: None,
            neon_rect: None,
            wall: None,
        }
    }
}

/// Two-storey motel: rooms facing a parking court, exterior walkway,
/// office, and a tall VACANCY pole sign near the street. Returns `{ d: 30 }`.
pub fn motel(b: &mut Builder, rng: &mut Mulberry32, o: MotelOpts) -> f64 {
    let w = o.w;
    let wall = match o.wall {
        Some(wl) => wl,
        None => *rpick(rng, &["wPink", "wPeach", "wTeal", "wMint"]),
    };
    let d = 10.0;
    let hh = 6.2;
    let back = -26.0;
    // Room block along the back of the lot.
    bx(b, wall, w, hh, d, 0.0, 0.0, back + d / 2.0 - d / 2.0);
    let zf = back + d / 2.0; // front face of the room block
    bx(b, "concrete", w + 0.4, 0.2, 3.0, 0.0, 0.0, zf + 1.5);
    bx(b, "concrete", w + 0.4, 0.25, 2.2, 0.0, 3.0, zf + 1.1); // walkway slab
    bx(b, "white", w + 0.4, 0.06, 0.06, 0.0, 4.05, zf + 2.15); // railing top
    let mut x = -w / 2.0;
    while x <= w / 2.0 {
        bx(b, "white", 0.06, 1.0, 0.06, x, 3.05, zf + 2.15);
        x += 2.5;
    }
    let mut x = -w / 2.0 + 2.0;
    while x < w / 2.0 {
        bx(b, "white", 0.2, 3.0, 0.2, x, 0.0, zf + 2.1);
        x += 5.0;
    }
    let rooms = (w / 4.0).floor();
    let mut i = 0.0;
    while i < rooms {
        let x = -w / 2.0 + 4.0 * (i + 0.5);
        for fy in [0.2, 3.25] {
            let door = *rpick(rng, &["wTeal", "paintRed", "wYellow"]);
            cb(b, door, 0.95, 2.1, 0.1, x - 0.9, fy + 1.05, zf + 0.04);
            let key = if chance(rng, 0.3) {
                "glassLit"
            } else {
                "glass"
            };
            cb(b, key, 1.4, 1.0, 0.08, x + 0.7, fy + 1.45, zf + 0.04);
        }
        i += 1.0;
    }
    // Stairs at one end.
    for k in 0..10 {
        let k = f64::from(k);
        bx(
            b,
            "concrete",
            1.2,
            0.3,
            0.35,
            w / 2.0 + 1.0,
            k * 0.3,
            zf + 2.0 - k * 0.35,
        );
    }
    bx(
        b,
        "roofTile",
        w + 1.0,
        0.3,
        d + 3.5,
        0.0,
        hh,
        back + d / 2.0 - d / 2.0 + 1.2,
    );
    // Office with a sign, at the street end of the lot.
    bx(b, wall, 9.0, 4.0, 8.0, -w / 2.0 + 5.0, 0.0, -6.0);
    cb(b, "glassLit", 6.0, 2.0, 0.1, -w / 2.0 + 5.0, 1.6, -1.95);
    bx(b, "roofTile", 10.0, 0.3, 9.0, -w / 2.0 + 5.0, 4.0, -6.0);
    if let Some(r) = o.sign_rect {
        sign(b, &r, 7.5, 1.6, -w / 2.0 + 5.0, 5.3, -1.6, 0.0, "signs");
        bx(b, "trim", 7.8, 1.9, 0.15, -w / 2.0 + 5.0, 4.3, -1.8);
    }
    // Parking stripes + a pool.
    let mut x = -w / 2.0 + 12.0;
    while x < w / 2.0 - 2.0 {
        bx(b, "paintWhite", 0.12, 0.02, 5.0, x, 0.05, zf + 6.0);
        x += 3.0;
    }
    bx(b, "concrete", 10.0, 0.18, 6.0, w / 2.0 - 7.0, 0.0, -6.0);
    bx(b, "pool", 8.0, 0.05, 4.0, w / 2.0 - 7.0, 0.16, -6.0);
    // Pole sign by the street: MOTEL + neon VACANCY.
    let px = w / 2.0 - 2.0;
    let pz = 1.2;
    bx(b, "metal", 0.35, 9.0, 0.35, px, 0.0, pz);
    if let Some(r) = o.neon_rect {
        bx(b, "black", 4.6, 1.5, 0.35, px, 5.6, pz);
        sign(b, &r, 4.4, 1.3, px, 6.35, pz + 0.19, 0.0, "neon");
        sign(b, &r, 4.4, 1.3, px, 6.35, pz - 0.19, PI, "neon");
    }
    30.0
}

/// `gasStation`'s options: `{ signRect, priceRect }`.
#[derive(Clone, Copy, Debug, Default)]
pub struct GasOpts {
    pub sign_rect: Option<Rect>,
    pub price_rect: Option<Rect>,
}

/// Gas station: canopy with a lit underside, pump islands, a kiosk. (The JS
/// takes `rng` and draws nothing from it.)
pub fn gas_station(b: &mut Builder, _rng: &mut Mulberry32, o: GasOpts) {
    let cw = 20.0;
    let cd = 12.0;
    let ch = 5.4;
    let cz = 1.0 - cd / 2.0 - 3.0;
    bx(b, "asphaltLot", 34.0, 0.08, 26.0, 0.0, 0.0, -12.0);
    for (x, z) in [
        (-cw / 2.0 + 1.5, cz - cd / 2.0 + 1.5),
        (cw / 2.0 - 1.5, cz - cd / 2.0 + 1.5),
        (-cw / 2.0 + 1.5, cz + cd / 2.0 - 1.5),
        (cw / 2.0 - 1.5, cz + cd / 2.0 - 1.5),
    ] {
        bx(b, "white", 0.5, ch, 0.5, x, 0.0, z);
    }
    bx(b, "white", cw, 0.9, cd, 0.0, ch, cz);
    bx(b, "paintRed", cw + 0.1, 0.35, cd + 0.1, 0.0, ch + 0.45, cz);
    cb(
        b,
        "canopyLight",
        cw - 1.0,
        0.05,
        cd - 1.0,
        0.0,
        ch - 0.03,
        cz,
    );
    for px in [-4.5, 4.5] {
        bx(b, "concrete", 1.4, 0.2, 5.0, px, 0.0, cz);
        for pz in [-1.3, 1.3] {
            bx(b, "white", 0.7, 1.7, 0.5, px, 0.2, cz + pz);
            cb(b, "paintRed", 0.72, 0.35, 0.52, px, 1.75, cz + pz);
            cb(b, "glassLit", 0.4, 0.3, 0.02, px, 1.25, cz + pz + 0.26);
        }
    }
    // Kiosk.
    bx(b, "wWhite", 12.0, 4.0, 8.0, 0.0, 0.0, -20.0);
    cb(b, "glassLit", 9.0, 2.2, 0.1, 0.0, 1.5, -15.95);
    bx(b, "paintRed", 12.2, 0.6, 8.2, 0.0, 3.6, -20.0);
    // Price sign pole.
    if let Some(r) = o.price_rect {
        bx(b, "metal", 0.4, 7.5, 0.4, -15.0, 0.0, 0.0);
        bx(b, "white", 3.6, 3.4, 0.4, -15.0, 7.0, 0.0);
        sign(b, &r, 3.3, 3.1, -15.0, 8.7, 0.21, 0.0, "signs");
        sign(b, &r, 3.3, 3.1, -15.0, 8.7, -0.21, PI, "signs");
    }
    if let Some(r) = o.sign_rect {
        sign(
            b,
            &r,
            6.0,
            1.2,
            0.0,
            ch + 0.45,
            cz + cd / 2.0 + 0.07,
            0.0,
            "signs",
        );
    }
}

/// `beachHouse`'s options: `{ w = rrange(rng, 9, 13), d = rrange(rng, 10,
/// 14), floors = rng() < 0.55 ? 2 : 3, wall = rpick(rng, WALLS), tile =
/// rng() < 0.4, porch = rng() < 0.5 }`.
#[derive(Clone, Copy, Debug, Default)]
pub struct HouseOpts<'a> {
    pub w: Option<f64>,
    pub d: Option<f64>,
    pub floors: Option<f64>,
    pub wall: Option<&'a str>,
    pub tile: Option<bool>,
    pub porch: Option<bool>,
}

/// What `beachHouse` returns: `{ w, d, H }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct House {
    pub w: f64,
    pub d: f64,
    pub h: f64,
}

/// Beach house / small apartment: stucco box(es), balconies with glass
/// rails, roof deck, garage.
pub fn beach_house(b: &mut Builder, rng: &mut Mulberry32, o: HouseOpts) -> House {
    let w = o.w.unwrap_or_else(|| rrange(rng, 9.0, 13.0));
    let d = o.d.unwrap_or_else(|| rrange(rng, 10.0, 14.0));
    let floors = o
        .floors
        .unwrap_or_else(|| if chance(rng, 0.55) { 2.0 } else { 3.0 });
    let wall = match o.wall {
        Some(wl) => wl,
        None => *rpick(rng, &WALLS),
    };
    let tile = o.tile.unwrap_or_else(|| chance(rng, 0.4));
    let porch = o.porch.unwrap_or_else(|| chance(rng, 0.5));
    let fh = 3.1;
    let hh = floors * fh;
    let shutter = if chance(rng, 0.45) {
        Some(*rpick(rng, &SHUTTERS))
    } else {
        None
    };
    bx(b, "concrete", w + 0.4, 0.35, d + 0.4, 0.0, -0.2, -d / 2.0);
    bx(b, wall, w, hh, d, 0.0, 0.0, -d / 2.0);
    // Corner boards and a band at each floor line.
    for sx in [-1.0, 1.0] {
        bx(b, "trim", 0.3, hh, 0.3, sx * (w / 2.0 - 0.1), 0.0, -0.1);
    }
    let mut f = 1.0;
    while f < floors {
        bx(b, "trim", w + 0.12, 0.22, 0.14, 0.0, f * fh - 0.15, 0.02);
        f += 1.0;
    }
    let mut f = 0.0;
    while f < floors {
        let y = f * fh;
        if f == 0.0 {
            cb(b, "trim", w * 0.42, 2.3, 0.1, -w * 0.22, 1.15, 0.03);
            cb(b, "woodDark", 1.0, 2.2, 0.1, w * 0.26, 1.1, 0.03);
            windows(
                b,
                rng,
                w * 0.05,
                w / 2.0 - 0.6,
                0.9,
                1.3,
                0.0,
                2.2,
                1.1,
                0.3,
                shutter,
            );
            if porch {
                // Front porch: deck, posts, a rail, steps and a shed roof
                // over it.
                let pw = w * 0.9;
                let pd = 2.4;
                bx(b, "wood", pw, 0.45, pd, 0.0, 0.0, pd / 2.0 + 0.05);
                for k in 0..=3 {
                    let k = f64::from(k);
                    bx(
                        b,
                        "trim",
                        0.18,
                        2.6,
                        0.18,
                        -pw / 2.0 + 0.1 + k * (pw - 0.2) / 3.0,
                        0.45,
                        pd - 0.05,
                    );
                }
                bx(b, "trim", pw, 0.08, 0.1, 0.0, 1.4, pd - 0.05);
                let mut x = -pw / 2.0 + 0.3;
                while x < pw / 2.0 - 0.2 {
                    if (x - w * 0.26).abs() > 0.7 {
                        bx(b, "trim", 0.05, 0.9, 0.05, x, 0.5, pd - 0.05);
                    }
                    x += 0.35;
                }
                for k in 0..2 {
                    let k = f64::from(k);
                    bx(
                        b,
                        "concrete",
                        1.4,
                        0.15 + k * 0.15,
                        0.35,
                        w * 0.26,
                        0.0,
                        pd + 0.35 - k * 0.3,
                    );
                }
                b.cbox(
                    if tile { "roofTile" } else { "roofTar" },
                    pw + 0.4,
                    0.12,
                    pd + 0.4,
                    0.0,
                    3.15,
                    pd / 2.0 + 0.05,
                    [0.12, 0.0, 0.0],
                );
            }
        } else {
            // `shutter && rng() < 0.6 ? shutter : null`
            let sh = match shutter {
                Some(s) if chance(rng, 0.6) => Some(s),
                _ => None,
            };
            windows(
                b,
                rng,
                -w / 2.0 + 0.6,
                w / 2.0 - 0.6,
                y + 0.6,
                1.9,
                0.0,
                2.3,
                1.6,
                0.45,
                sh,
            );
            if chance(rng, 0.75) && !(porch && f == 1.0) {
                // Balcony with glass balustrade.
                bx(b, "concrete", w * 0.8, 0.2, 1.6, 0.0, y - 0.1, 0.8);
                cb(b, "balGlass", w * 0.8, 1.0, 0.05, 0.0, y + 0.6, 1.58);
                bx(b, "white", w * 0.8, 0.06, 0.1, 0.0, y + 1.1, 1.58);
            }
        }
        side_windows(b, rng, 1.0, w / 2.0, -d + 1.0, -1.0, y + 0.8, 1.4, 2.6, 0.3);
        side_windows(
            b,
            rng,
            -1.0,
            -w / 2.0,
            -d + 1.0,
            -1.0,
            y + 0.8,
            1.4,
            2.6,
            0.3,
        );
        f += 1.0;
    }
    if tile {
        // Low hipped tile roof (a squashed pyramid).
        let mut c = cone(FRAC_1_SQRT_2, 1.0, 4.0, false);
        c.rotate_y(PI / 4.0);
        b.put(
            "roofTile",
            &c,
            0.0,
            hh + 1.0,
            -d / 2.0,
            [0.0; 3],
            [w + 0.9, 2.0, d + 0.9],
        );
    } else {
        parapet(b, "trim", w, d, hh, 0.2, 0.9);
        bx(
            b,
            "roofTar",
            w - 0.3,
            0.1,
            d - 0.3,
            0.0,
            hh - 0.05,
            -d / 2.0,
        );
        if chance(rng, 0.5) {
            // Roof-deck pergola.
            for (sx, sz) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                bx(
                    b,
                    "wood",
                    0.15,
                    2.4,
                    0.15,
                    sx * w * 0.25,
                    hh,
                    -d / 2.0 + sz * d * 0.2,
                );
            }
            for k in -3..=3 {
                let k = f64::from(k);
                bx(
                    b,
                    "wood",
                    w * 0.55,
                    0.12,
                    0.12,
                    0.0,
                    hh + 2.4,
                    -d / 2.0 + k * d * 0.07,
                );
            }
        }
    }
    House { w, d, h: hh }
}

/// `hotel`'s options: `{ w = 34, d = 16, rect }`.
#[derive(Clone, Copy, Debug)]
pub struct HotelOpts {
    pub w: f64,
    pub d: f64,
    pub rect: Option<Rect>,
}

impl Default for HotelOpts {
    fn default() -> Self {
        HotelOpts {
            w: 34.0,
            d: 16.0,
            rect: None,
        }
    }
}

/// Pastel hotel block, 5–6 floors, balconies on every floor, a roof sign.
pub fn hotel(b: &mut Builder, rng: &mut Mulberry32, o: HotelOpts) {
    let HotelOpts { w, d, rect } = o;
    let floors = 6.0;
    let fh = 3.2;
    let hh = floors * fh;
    bx(b, "wPeach", w, hh, d, 0.0, 0.0, -d / 2.0);
    bx(b, "trim", w + 0.3, 0.6, d + 0.3, 0.0, hh, -d / 2.0);
    cb(b, "glassLit", w * 0.5, 2.6, 0.1, 0.0, 1.6, 0.04);
    bx(b, "white", w * 0.6, 0.3, 4.0, 0.0, 3.2, 2.0);
    let mut f = 1.0;
    while f < floors {
        let y = f * fh;
        let mut x = -w / 2.0 + 2.2;
        while x < w / 2.0 - 1.0 {
            let key = if chance(rng, 0.4) {
                "glassLit"
            } else {
                "glass"
            };
            cb(b, key, 2.4, 2.1, 0.1, x, y + 1.3, 0.03);
            bx(b, "white", 3.4, 0.16, 1.3, x, y, 0.65);
            cb(b, "balGlass", 3.4, 0.95, 0.04, x, y + 0.6, 1.3);
            x += 4.4;
        }
        f += 1.0;
    }
    let mut f = 1.0;
    while f < floors {
        side_windows(
            b,
            rng,
            1.0,
            w / 2.0,
            -d + 1.5,
            -1.5,
            f * fh + 0.6,
            2.0,
            3.4,
            0.4,
        );
        side_windows(
            b,
            rng,
            -1.0,
            -w / 2.0,
            -d + 1.5,
            -1.5,
            f * fh + 0.6,
            2.0,
            3.4,
            0.4,
        );
        f += 1.0;
    }
    if let Some(r) = rect {
        bx(b, "metal", 0.2, 2.2, 0.2, -6.0, hh + 0.6, -2.0);
        bx(b, "metal", 0.2, 2.2, 0.2, 6.0, hh + 0.6, -2.0);
        sign(b, &r, 15.0, 2.6, 0.0, hh + 2.4, -1.9, 0.0, "neon");
        bx(b, "black", 15.4, 2.9, 0.2, 0.0, hh + 1.0, -2.05);
    }
    roof_clutter(b, rng, w, d, hh + 0.6);
}

// ── Street furniture ────────────────────────────────────────────────────

/// `bench(B, x, z, ry = 0)`.
pub fn bench(b: &mut Builder, x: f64, z: f64, ry: f64) {
    b.push_frame(x, 0.0, z, ry);
    bx(b, "wood", 1.9, 0.08, 0.45, 0.0, 0.45, 0.0);
    bx(b, "wood", 1.9, 0.4, 0.06, 0.0, 0.55, -0.22);
    for sx in [-0.8, 0.8] {
        bx(b, "black", 0.08, 0.45, 0.45, sx, 0.0, 0.0);
    }
    b.pop_frame();
}

/// Promenade lamp: slim post with a pair of globes.
pub fn prom_lamp(b: &mut Builder, x: f64, z: f64) {
    bx(b, "black", 0.14, 4.2, 0.14, x, 0.0, z);
    bx(b, "black", 1.2, 0.08, 0.08, x, 4.1, z);
    let s = sphere(0.22, 10.0, 8.0);
    put(b, "lampGlow", &s, x - 0.55, 4.4, z);
    put(b, "lampGlow", &s, x + 0.55, 4.4, z);
}

/// Cobra-head street light; arm reaches toward −Z by `reach`
/// (`reach = 3.2, h = 8.6`).
pub fn street_light(b: &mut Builder, x: f64, z: f64, ry: f64, reach: f64, h: f64) {
    b.push_frame(x, 0.0, z, ry);
    bx(b, "metal", 0.26, h, 0.26, 0.0, 0.0, 0.0);
    b.beam(
        "metal",
        v(0.0, h - 0.2, 0.0),
        v(0.0, h + 0.4, reach * 0.5),
        0.14,
    );
    b.beam(
        "metal",
        v(0.0, h + 0.4, reach * 0.5),
        v(0.0, h + 0.5, reach),
        0.14,
    );
    cb(b, "metal", 0.5, 0.22, 1.1, 0.0, h + 0.4, reach + 0.3);
    cb(b, "lampGlow", 0.38, 0.06, 0.8, 0.0, h + 0.27, reach + 0.3);
    b.pop_frame();
}

pub fn trash_can(b: &mut Builder, x: f64, z: f64) {
    put(b, "wTeal", &cyl(0.32, 0.3, 0.95, 10.0), x, 0.48, z);
}

/// Lifeguard tower: hut on stilts with a ramp, number on the side.
pub fn lifeguard_tower(b: &mut Builder, rng: &mut Mulberry32, rect: Option<&Rect>) {
    let col = *rpick(rng, &["wBlue", "wYellow", "wPink", "wTeal"]);
    for (sx, sz) in [(-1.3, -1.1), (1.3, -1.1), (-1.3, 1.1), (1.3, 1.1)] {
        bx(b, "white", 0.2, 2.2, 0.2, sx, 0.0, sz);
    }
    bx(b, "white", 3.4, 0.2, 3.0, 0.0, 2.2, 0.0);
    bx(b, col, 3.0, 2.2, 2.6, 0.0, 2.4, -0.1);
    cb(b, "glass", 2.4, 0.9, 0.08, 0.0, 3.7, 1.21);
    cb(b, "glass", 0.08, 0.9, 1.6, 1.51, 3.7, -0.1);
    cb(b, "glass", 0.08, 0.9, 1.6, -1.51, 3.7, -0.1);
    let mut c = cone(FRAC_1_SQRT_2, 1.0, 4.0, false);
    c.rotate_y(PI / 4.0);
    b.put("white", &c, 0.0, 5.05, -0.1, [0.0; 3], [3.8, 0.9, 3.4]);
    // Deck + ramp down to the sand.
    bx(b, "wood", 3.4, 0.12, 1.2, 0.0, 2.2, 1.9);
    b.cbox("wood", 1.3, 0.1, 4.6, 0.0, 1.05, 4.3, [0.49, 0.0, 0.0]);
    bx(b, "white", 3.4, 0.06, 0.06, 0.0, 3.2, 2.5);
    if let Some(r) = rect {
        sign(b, r, 1.4, 1.1, 1.52, 3.0, -0.1, PI / 2.0, "signs");
    }
    // Rescue board and buoy.
    b.put(
        "paintRed",
        &cyl(1.0, 1.0, 1.0, 12.0),
        -1.9,
        1.4,
        0.3,
        [0.0, 0.0, 0.12],
        [0.26, 2.6, 0.05],
    );
}

/// Beach umbrella (mostly still folded at dawn) with a couple of loungers
/// (`open = rng() < 0.55` when `None`).
pub fn umbrella_set(b: &mut Builder, rng: &mut Mulberry32, fabric: &str, open: Option<bool>) {
    let open = open.unwrap_or_else(|| chance(rng, 0.55));
    bx(
        b,
        "white",
        0.07,
        if open { 2.6 } else { 2.5 },
        0.07,
        0.0,
        -0.2,
        0.0,
    );
    if open {
        put(b, fabric, &cone(1.5, 0.55, 10.0, true), 0.0, 2.45, 0.0);
        put(b, "white", &sphere(0.06, 5.0, 4.0), 0.0, 2.75, 0.0);
    } else {
        put(b, fabric, &cone(0.36, 1.7, 8.0, true), 0.0, 1.55, 0.0);
        b.put(
            fabric,
            &cone(0.36, 0.5, 8.0, true),
            0.0,
            2.65,
            0.0,
            [PI, 0.0, 0.0],
            [1.0; 3],
        );
    }
    if chance(rng, 0.5) {
        for sx in [-1.0, 1.0] {
            let yaw = rrange(rng, -0.2, 0.2);
            b.push_frame(sx * 1.2, 0.0, 0.7, yaw);
            bx(b, fabric, 0.65, 0.08, 1.9, 0.0, 0.32, 0.0);
            b.cbox(fabric, 0.65, 0.08, 0.6, 0.0, 0.55, -0.85, [-0.9, 0.0, 0.0]);
            for lz in [-0.8, 0.8] {
                bx(b, "white", 0.6, 0.3, 0.05, 0.0, 0.0, lz);
            }
            b.pop_frame();
        }
    } else {
        // Towels spread on the sand, a cooler and a bag.
        let n = 1.0 + (rng.next_f64() * 2.0).floor();
        let mut k = 0.0;
        while k < n {
            let x = rrange(rng, -1.6, 1.6);
            let z = rrange(rng, 0.6, 1.6);
            let yaw = rrange(rng, -0.5, 0.5);
            b.push_frame(x, 0.0, z, yaw);
            let key = *rpick(rng, &AWNINGS);
            bx(b, key, 0.9, 0.03, 1.8, 0.0, 0.01, 0.0);
            b.pop_frame();
            k += 1.0;
        }
        if chance(rng, 0.6) {
            let key = *rpick(rng, &["wTeal", "paintRed", "wBlue", "white"]);
            let x = rrange(rng, -1.0, 1.0);
            let ry = rng.next_f64();
            b.box_(key, 0.55, 0.4, 0.38, x, 0.0, -0.6, ry, 0.0, 0.0);
        }
        if chance(rng, 0.4) {
            let key = *rpick(rng, &["wYellow", "wPink", "wMint"]);
            let x = rrange(rng, -1.0, 1.0);
            let ry = rng.next_f64();
            b.box_(key, 0.45, 0.3, 0.25, x, 0.0, 0.3, ry, 0.0, 0.0);
        }
    }
}

pub fn volleyball_net(b: &mut Builder) {
    for sx in [-4.8, 4.8] {
        bx(b, "white", 0.12, 2.6, 0.12, sx, 0.0, 0.0);
    }
    cb(b, "black", 9.5, 0.9, 0.03, 0.0, 2.05, 0.0);
    cb(b, "white", 9.5, 0.06, 0.05, 0.0, 2.5, 0.0);
}

/// `surfboardsInSand(B, rng, n = 3)`.
pub fn surfboards_in_sand(b: &mut Builder, rng: &mut Mulberry32, n: f64) {
    let g = cyl(1.0, 1.0, 1.0, 14.0);
    let mut i = 0.0;
    while i < n {
        let key = if chance(rng, 0.5) { "wTeal" } else { "boardB" };
        let z = rrange(rng, -0.2, 0.2);
        let rx = rrange(rng, -0.12, 0.12);
        let rz = rrange(rng, -0.15, 0.15);
        b.put(key, &g, i * 0.7, 1.05, z, [rx, 0.0, rz], [0.27, 2.2, 0.035]);
        i += 1.0;
    }
}

// ── Boats ───────────────────────────────────────────────────────────────

/// Hull as a lathe-ish extrusion: pointed bow at +Z.
fn hull_geometry(l: f64, w: f64, h: f64) -> BufferGeometry {
    let mut s = Shape::new();
    s.move_to(-w / 2.0, -l / 2.0);
    s.line_to(w / 2.0, -l / 2.0);
    s.quadratic_curve_to(w / 2.0, l * 0.2, 0.0, l / 2.0);
    s.quadratic_curve_to(-w / 2.0, l * 0.2, -w / 2.0, -l / 2.0);
    let mut g = extrude_geometry(
        &[s],
        &ExtrudeOptions {
            depth: h,
            bevel_enabled: true,
            bevel_thickness: 0.15,
            bevel_size: 0.12,
            bevel_segments: 2.0,
            curve_segments: 8.0,
            ..ExtrudeOptions::THREE_DEFAULTS
        },
    );
    g.rotate_x(-PI / 2.0); // extrude upward, shape in XZ (y of shape → −z)
    g.scale(1.0, 1.0, -1.0);
    g
}

/// A moored sailboat; returns its height (mast and freeboard).
pub fn sailboat(b: &mut Builder, rng: &mut Mulberry32) -> f64 {
    let l = rrange(rng, 8.0, 12.0);
    let w = l * 0.32;
    let h = 1.1;
    put(b, "hullWhite", &hull_geometry(l, w, h), 0.0, -0.4, 0.0);
    bx(b, "wood", w * 0.8, 0.08, l * 0.7, 0.0, h - 0.35, -l * 0.1);
    bx(
        b,
        "hullWhite",
        w * 0.55,
        0.7,
        l * 0.3,
        0.0,
        h - 0.3,
        -l * 0.08,
    );
    cb(b, "glass", w * 0.56, 0.25, l * 0.2, 0.0, h + 0.2, -l * 0.02);
    let mh = l * 1.35;
    bx(b, "metal", 0.14, mh, 0.14, 0.0, h - 0.3, l * 0.08);
    b.beam(
        "metal",
        v(0.0, h + 0.6, l * 0.08),
        v(0.0, h + 0.6, -l * 0.42),
        0.1,
    ); // boom
    let cover = *rpick(rng, &["sailBlue", "white", "sailBlue"]);
    cb(b, cover, 0.34, 0.4, l * 0.44, 0.0, h + 0.82, -l * 0.17); // furled sail cover
    b.beam(
        "black",
        v(0.0, h + mh - 0.6, l * 0.08),
        v(0.0, h - 0.2, l * 0.48),
        0.03,
    ); // forestay
    b.beam(
        "black",
        v(0.0, h + mh - 0.6, l * 0.08),
        v(0.0, h - 0.2, -l * 0.48),
        0.03,
    ); // backstay
    mh + h
}

pub fn motorboat(b: &mut Builder, rng: &mut Mulberry32) {
    let l = rrange(rng, 6.0, 9.0);
    let w = l * 0.36;
    let h = 1.0;
    let key = if chance(rng, 0.5) {
        "hullWhite"
    } else {
        "wBlue"
    };
    put(b, key, &hull_geometry(l, w, h), 0.0, -0.4, 0.0);
    bx(
        b,
        "hullWhite",
        w * 0.6,
        0.9,
        l * 0.3,
        0.0,
        h - 0.3,
        l * 0.02,
    );
    b.cbox(
        "glass",
        w * 0.62,
        0.45,
        0.06,
        0.0,
        h + 0.3,
        l * 0.18,
        [0.5, 0.0, 0.0],
    );
    bx(b, "wood", w * 0.8, 0.06, l * 0.3, 0.0, h - 0.35, -l * 0.3);
}
