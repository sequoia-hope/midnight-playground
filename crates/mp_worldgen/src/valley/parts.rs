//! Port of `src/world/valley/parts.js`: the farm building kit (roadmap
//! WP 3.7).
//!
//! Every function draws into a [`Builder`] in its current frame: local +Z
//! is the front (facing the road), X is across, Y is up, and y = 0 is the
//! ground. Random choices draw from the `rng` passed in, in the JS order.

// The kit keeps the JS argument lists (DECISIONS D130, D190).
#![allow(clippy::too_many_arguments)]

use std::f64::consts::PI;

use mp_math::{Mulberry32, js, kernel, rpick, rrange};

use crate::builder::Builder;
use crate::three_geom::{
    BufferGeometry, ExtrudeOptions, Shape, Vector3, box_geometry, cone_geometry, cylinder_geometry,
    extrude_geometry, plane_geometry, sphere_geometry, torus_geometry,
};

fn v(x: f64, y: f64, z: f64) -> Vector3 {
    Vector3::new(x, y, z)
}

/// `new THREE.CylinderGeometry(rt, rb, h, radial)` (one height segment,
/// closed, the full turn).
fn cyl(rt: f64, rb: f64, h: f64, radial: f64) -> BufferGeometry {
    cylinder_geometry(rt, rb, h, radial, 1.0, false, 0.0, 2.0 * PI)
}

/// `new THREE.BoxGeometry(w, h, d)`.
fn boxg(w: f64, h: f64, d: f64) -> BufferGeometry {
    box_geometry(w, h, d, 1.0, 1.0, 1.0)
}

/// `B.put(key, geo, x, y, z, rx, ry, rz)` (unit scale).
fn put_r(
    b: &mut Builder,
    key: &str,
    geo: &BufferGeometry,
    x: f64,
    y: f64,
    z: f64,
    rx: f64,
    ry: f64,
    rz: f64,
) {
    b.put(key, geo, x, y, z, [rx, ry, rz], [1.0; 3]);
}

/// `B.cbox(key, w, h, d, x, y, z)`.
fn cbox0(b: &mut Builder, key: &str, w: f64, h: f64, d: f64, x: f64, y: f64, z: f64) {
    b.cbox(key, w, h, d, x, y, z, [0.0; 3]);
}

/// `B.box(key, w, h, d, x, y, z)`.
fn box0(b: &mut Builder, key: &str, w: f64, h: f64, d: f64, x: f64, y: f64, z: f64) {
    b.box_(key, w, h, d, x, y, z, 0.0, 0.0, 0.0);
}

/// `Shape` from points: moveTo the first, lineTo the rest, closePath.
fn closed(pts: &[[f64; 2]]) -> Shape {
    let mut s = Shape::new();
    for (i, p) in pts.iter().enumerate() {
        if i > 0 {
            s.line_to(p[0], p[1]);
        } else {
            s.move_to(p[0], p[1]);
        }
    }
    s.close_path();
    s
}

/// Gable-ended prism filling the triangle above the walls.
fn gable_prism(w: f64, d: f64, rh: f64) -> BufferGeometry {
    let mut s = Shape::new();
    s.move_to(-d / 2.0, 0.0);
    s.line_to(d / 2.0, 0.0);
    s.line_to(0.0, rh);
    s.close_path();
    let mut g = extrude_geometry(&[s], &ExtrudeOptions::flat(w));
    g.rotate_y(PI / 2.0);
    g.translate(-w / 2.0, 0.0, 0.0);
    g
}

/// Two sloped roof slabs over a gable (ridge along X). The JS defaults are
/// `over = 0.5`, `t = 0.16`.
fn gable_roof(b: &mut Builder, key: &str, w: f64, d: f64, h: f64, rh: f64, over: f64, t: f64) {
    let a = kernel::atan2(rh, d / 2.0);
    let l = (d / 2.0) / kernel::cos(a) + over;
    for side in [1.0, -1.0] {
        let cz = side * (l / 2.0) * kernel::cos(a) + side * kernel::sin(a) * t * 0.5;
        let cy = h + rh - (l / 2.0) * kernel::sin(a) + kernel::cos(a) * t * 0.5;
        b.cbox(key, w + over * 2.0, t, l, 0.0, cy, cz, [side * a, 0.0, 0.0]);
    }
}

/// Sash window: glass, frame, a mullion cross and (optionally) a pair of
/// louvred shutters. ry turns it about Y; the glass faces local +Z.
pub fn window_pane(
    b: &mut Builder,
    lit: bool,
    x: f64,
    y: f64,
    z: f64,
    w: f64,
    h: f64,
    ry: f64,
    shutter: Option<&str>,
) {
    b.cbox(
        if lit { "window" } else { "windowDark" },
        w,
        h,
        0.08,
        x,
        y,
        z,
        [0.0, ry, 0.0],
    );
    let fr = "trim";
    let c = kernel::cos(ry);
    let s = kernel::sin(ry);
    let off = |dx: f64, dn: f64| [x + c * dx + s * dn, z - s * dx + c * dn];
    let r = [0.0, ry, 0.0];
    let [fx, fz] = off(0.0, 0.0);
    b.cbox(fr, w + 0.16, 0.08, 0.1, fx, y + h / 2.0 + 0.04, fz, r);
    b.cbox(fr, w + 0.2, 0.1, 0.16, fx, y - h / 2.0 - 0.05, fz, r);
    let [fx, fz] = off(-w / 2.0 - 0.04, 0.0);
    b.cbox(fr, 0.08, h, 0.1, fx, y, fz, r);
    let [fx, fz] = off(w / 2.0 + 0.04, 0.0);
    b.cbox(fr, 0.08, h, 0.1, fx, y, fz, r);
    // Mullions: the meeting rail and a centre bar.
    let [fx, fz] = off(0.0, 0.04);
    b.cbox(fr, w, 0.05, 0.04, fx, y, fz, r);
    b.cbox(fr, 0.04, h, 0.04, fx, y, fz, r);
    if let Some(shutter) = shutter {
        for sx in [-1.0, 1.0] {
            let [fx, fz] = off(sx * (w / 2.0 + 0.1 + w * 0.24), 0.03);
            b.cbox(shutter, w * 0.46, h * 1.02, 0.06, fx, y, fz, r);
        }
    }
}

/// `farmhouse`'s anchors.
#[derive(Clone, Copy, Debug)]
pub struct Farmhouse {
    pub door: Vector3,
    pub lamp: Vector3,
    pub w: f64,
    pub d: f64,
}

/// Returns local anchor points of interest (porch light, front door).
/// (The JS `opts` is always empty: every choice is drawn.)
pub fn farmhouse(b: &mut Builder, rng: &mut Mulberry32) -> Farmhouse {
    let two_story = rng.next_f64() < 0.6;
    let w = rrange(rng, 9.0, 12.0);
    let d = rrange(rng, 7.0, 8.5);
    let hh = if two_story { 5.8 } else { 3.2 };
    let rh = d * 0.42;
    let wall = *rpick(
        rng,
        &[
            "wallCream",
            "wallWhite",
            "wallWhite",
            "wallYellow",
            "wallBlue",
            "wallGreen",
        ],
    );
    let roof = *rpick(
        rng,
        &[
            "roofShingle",
            "roofMetal",
            "roofRust",
            "roofSlate",
            "roofGreen",
        ],
    );
    let lit = |rng: &mut Mulberry32| rng.next_f64() < 0.6;
    let shutter = if rng.next_f64() < 0.65 {
        Some(*rpick(
            rng,
            &["shutterGreen", "shutterBlack", "shutterRed", "shutterBlue"],
        ))
    } else {
        None
    };
    // Foundation + walls
    box0(b, "stone", w + 0.3, 0.5, d + 0.3, 0.0, -0.3, 0.0);
    box0(b, wall, w, hh, d, 0.0, 0.2, 0.0);
    b.put_at(wall, &gable_prism(w, d, rh), 0.0, hh + 0.2, 0.0);
    gable_roof(b, roof, w, d, hh + 0.2, rh, 0.5, 0.16);
    // Corner trim
    for sx in [-1.0, 1.0] {
        for sz in [-1.0, 1.0] {
            box0(b, "trim", 0.18, hh, 0.18, sx * w / 2.0, 0.2, sz * d / 2.0);
        }
    }
    // Barge boards up both gable ends.
    for sx in [-1.0, 1.0] {
        for sz in [-1.0, 1.0] {
            b.beam(
                "trim",
                v(sx * (w / 2.0 + 0.52), hh + 0.1, sz * (d / 2.0 + 0.55)),
                v(sx * (w / 2.0 + 0.52), hh + 0.2 + rh + 0.12, 0.0),
                0.14,
            );
        }
    }
    // Chimney with a corbelled cap.
    let cx = rrange(rng, -w * 0.3, w * 0.3);
    box0(b, "brick", 0.8, rh + 1.6, 0.8, cx, hh + 0.2, -d * 0.12);
    box0(b, "brick", 1.0, 0.2, 1.0, cx, hh + rh + 1.6, -d * 0.12);
    // Kitchen wing out the back on some houses: a lower gable.
    if rng.next_f64() < 0.6 {
        let ww = w * rrange(rng, 0.45, 0.6);
        let wd = d * 0.55;
        let wh = 3.0;
        let wrh = wd * 0.4;
        let wx = rrange(rng, -1.0, 1.0) * (w - ww) * 0.4;
        b.push_frame(wx, 0.0, -d / 2.0 - wd / 2.0 + 0.1, 0.0);
        box0(b, "stone", ww + 0.3, 0.5, wd + 0.3, 0.0, -0.3, 0.0);
        box0(b, wall, ww, wh, wd, 0.0, 0.2, 0.0);
        b.put_at(wall, &gable_prism(ww, wd, wrh), 0.0, wh + 0.2, 0.0);
        gable_roof(b, roof, ww, wd, wh + 0.2, wrh, 0.4, 0.16);
        let l = lit(rng);
        window_pane(b, l, 0.0, 1.7, -wd / 2.0 - 0.03, 1.0, 1.2, PI, shutter);
        b.pop_frame();
    }
    // Front door + porch
    cbox0(b, "woodDark", 1.1, 2.1, 0.1, 0.0, 1.25, d / 2.0 + 0.04);
    let pw = w * 0.7;
    let pd = 2.4;
    box0(b, "woodGray", pw, 0.35, pd, 0.0, 0.0, d / 2.0 + pd / 2.0);
    for px in [-pw / 2.0 + 0.15, -pw / 6.0, pw / 6.0, pw / 2.0 - 0.15] {
        box0(b, "trim", 0.14, 2.6, 0.14, px, 0.35, d / 2.0 + pd - 0.15);
    }
    b.cbox(
        roof,
        pw + 0.4,
        0.12,
        pd + 0.4,
        0.0,
        3.05,
        d / 2.0 + pd / 2.0,
        [0.14, 0.0, 0.0],
    );
    // Porch railing with balusters (gap at the steps), steps down to the yard.
    let rz = d / 2.0 + pd - 0.15;
    for sx in [-1.0, 1.0] {
        let x0 = sx * 0.8;
        let x1 = sx * (pw / 2.0 - 0.15);
        b.beam("trim", v(x0, 1.1, rz), v(x1, 1.1, rz), 0.08);
        b.beam("trim", v(x0, 0.45, rz), v(x1, 0.45, rz), 0.06);
        let mut bx = x0;
        while bx.abs() < x1.abs() {
            box0(b, "trim", 0.05, 0.65, 0.05, bx, 0.45, rz);
            bx += sx * 0.34;
        }
    }
    for k in 0..2 {
        let k = f64::from(k);
        box0(
            b,
            "woodGray",
            1.6,
            0.35 - k * 0.17,
            0.35,
            0.0,
            -0.1,
            d / 2.0 + pd + 0.17 + k * 0.33,
        );
    }
    // Porch fascia board.
    box0(
        b,
        "trim",
        pw + 0.3,
        0.22,
        0.08,
        0.0,
        2.85,
        d / 2.0 + pd + 0.05,
    );
    // Windows (ground floor + upper floor)
    let floors: &[f64] = if two_story { &[1.5, 4.4] } else { &[1.55] };
    for &fy in floors {
        for wx in [-w * 0.33, w * 0.33] {
            let l = lit(rng);
            window_pane(b, l, wx, fy + 0.2, d / 2.0 + 0.03, 1.0, 1.4, 0.0, shutter);
        }
        if fy > 3.0 {
            let l = lit(rng);
            window_pane(b, l, 0.0, fy + 0.2, d / 2.0 + 0.03, 0.9, 1.3, 0.0, shutter);
        }
        for sx in [-1.0, 1.0] {
            let l = lit(rng);
            window_pane(
                b,
                l,
                sx * (w / 2.0 + 0.03),
                fy + 0.2,
                0.0,
                1.0,
                1.4,
                sx * PI / 2.0,
                shutter,
            );
        }
        let l = lit(rng);
        window_pane(
            b,
            l,
            -w * 0.2,
            fy + 0.2,
            -d / 2.0 - 0.03,
            1.0,
            1.4,
            PI,
            shutter,
        );
    }
    // Attic gable window
    let l = rng.next_f64() < 0.5;
    window_pane(
        b,
        l,
        w / 2.0 + 0.03,
        hh + 0.2 + rh * 0.35,
        0.0,
        0.6,
        0.6,
        PI / 2.0,
        None,
    );
    // Porch lamp
    cbox0(b, "lamp", 0.2, 0.26, 0.2, pw * 0.25, 2.55, d / 2.0 + 0.25);
    Farmhouse {
        door: v(0.0, 0.0, d / 2.0 + pd),
        lamp: v(pw * 0.25, 2.55, d / 2.0 + 0.3),
        w,
        d,
    }
}

/// `barn`'s anchors.
#[derive(Clone, Copy, Debug)]
pub struct Barn {
    pub w: f64,
    pub d: f64,
    pub h: f64,
    pub lamp: Vector3,
}

/// Faded red barn: gambrel (or gable) roof, white trim, X-braced doors.
pub fn barn(b: &mut Builder, rng: &mut Mulberry32) -> Barn {
    let w = rrange(rng, 11.0, 14.0);
    let d = rrange(rng, 16.0, 22.0);
    let hh = rrange(rng, 4.2, 5.2);
    let ww = w / 2.0;
    let r = ww * 0.78;
    let body = *rpick(rng, &["barnRed", "barnRed", "barnRed2", "woodGray"]);
    let roof = *rpick(rng, &["roofMetal", "roofRust", "roofShingle"]);
    let gambrel = rng.next_f64() < 0.75;
    let prof: Vec<[f64; 2]> = if gambrel {
        vec![
            [-ww, 0.0],
            [ww, 0.0],
            [ww, hh],
            [0.72 * ww, hh + 0.62 * r],
            [0.0, hh + r],
            [-0.72 * ww, hh + 0.62 * r],
            [-ww, hh],
        ]
    } else {
        vec![
            [-ww, 0.0],
            [ww, 0.0],
            [ww, hh],
            [0.0, hh + r * 0.8],
            [-ww, hh],
        ]
    };
    let s = closed(&prof);
    let mut g = extrude_geometry(&[s], &ExtrudeOptions::flat(d));
    g.translate(0.0, 0.0, -d / 2.0);
    box0(b, "stone", w + 0.3, 0.45, d + 0.3, 0.0, -0.3, 0.0);
    b.put_at(body, &g, 0.0, 0.1, 0.0);
    // Roof cap: a thin band following the upper outline.
    let top: Vec<[f64; 2]> = prof[2..].to_vec(); // from right eave over the ridge to left eave
    let t = 0.28;
    let outer: Vec<[f64; 2]> = top
        .iter()
        .map(|&[x, y]| {
            let eave = x.abs() >= ww - 0.01;
            let nx = js::sign(x) * (if eave { 1.0 } else { 0.55 });
            [
                x + nx * (if eave { 0.45 } else { t }),
                y + (if eave { -0.35 } else { t }),
            ]
        })
        .collect();
    let mut cap = Shape::new();
    for (i, &[x, y]) in outer.iter().enumerate() {
        if i > 0 {
            cap.line_to(x, y);
        } else {
            cap.move_to(x, y);
        }
    }
    for &[x, y] in top.iter().rev() {
        cap.line_to(x, y);
    }
    cap.close_path();
    let mut cg = extrude_geometry(&[cap], &ExtrudeOptions::flat(d + 0.9));
    cg.translate(0.0, 0.0, -(d + 0.9) / 2.0);
    b.put_at(roof, &cg, 0.0, 0.1, 0.0);
    // Trim: corners and eave lines on the front gable.
    for sx in [-1.0, 1.0] {
        for sz in [-1.0, 1.0] {
            box0(b, "trim", 0.22, hh, 0.22, sx * ww, 0.1, sz * d / 2.0);
        }
    }
    for sz in [-1.0, 1.0] {
        for i in 2..prof.len() - 1 {
            let [x0, y0] = prof[i];
            let [x1, y1] = prof[i + 1];
            b.beam(
                "trim",
                v(x0, y0 + 0.1, sz * (d / 2.0 + 0.03)),
                v(x1, y1 + 0.1, sz * (d / 2.0 + 0.03)),
                0.2,
            );
        }
        let [xl, yl] = prof[prof.len() - 1];
        let [xp, yp] = prof[prof.len() - 2];
        b.beam(
            "trim",
            v(xl, yl + 0.1, sz * (d / 2.0 + 0.03)),
            v(xp, yp + 0.1, sz * (d / 2.0 + 0.03)),
            0.2,
        );
    }
    // Big doors with white border and X-bracing.
    let dw = js::min(5.0, w * 0.42);
    let dh = js::min(3.9, hh - 0.4);
    let fz = d / 2.0 + 0.06;
    cbox0(b, "barnDoor", dw, dh, 0.1, 0.0, 0.1 + dh / 2.0, fz);
    cbox0(b, "trim", dw + 0.3, 0.2, 0.14, 0.0, 0.1 + dh, fz);
    for sx in [-1.0, 0.0, 1.0] {
        cbox0(b, "trim", 0.18, dh, 0.14, sx * dw / 2.0, 0.1 + dh / 2.0, fz);
    }
    let diag = kernel::hypot(dw / 2.0, dh);
    for sx in [-1.0, 1.0] {
        let a = kernel::atan2(dw / 2.0, dh);
        b.cbox(
            "trim",
            0.16,
            diag,
            0.13,
            sx * dw / 4.0,
            0.1 + dh / 2.0,
            fz + 0.01,
            [0.0, 0.0, a],
        );
        b.cbox(
            "trim",
            0.16,
            diag,
            0.13,
            sx * dw / 4.0,
            0.1 + dh / 2.0,
            fz + 0.01,
            [0.0, 0.0, -a],
        );
    }
    // Hayloft door.
    let hy = hh + (if gambrel { 0.5 } else { 0.2 });
    cbox0(b, "barnDoor", 1.8, 1.6, 0.1, 0.0, hy + 0.8, fz);
    cbox0(b, "trim", 2.1, 0.16, 0.13, 0.0, hy + 1.65, fz);
    cbox0(b, "trim", 2.1, 0.16, 0.13, 0.0, hy - 0.05, fz);
    // Hay hood beam.
    cbox0(b, "woodDark", 0.2, 0.2, 1.2, 0.0, hy + 2.0, fz + 0.5);
    // Side windows (small, dark).
    for k in -1..=1 {
        let k = f64::from(k);
        for sx in [-1.0, 1.0] {
            cbox0(
                b,
                "windowDark",
                0.08,
                0.8,
                1.0,
                sx * (ww + 0.03),
                hh * 0.6,
                k * d * 0.3,
            );
        }
    }
    // Yard light on the gable.
    cbox0(
        b,
        "lamp",
        0.35,
        0.2,
        0.35,
        0.0,
        hh + (if gambrel { r * 0.35 } else { r * 0.2 }),
        fz + 0.4,
    );
    // Ventilator cupolas along the ridge, with a weathervane on the first.
    let ridge = hh + (if gambrel { r } else { r * 0.8 }) + 0.1;
    let n_cup = if rng.next_f64() < 0.5 { 1 } else { 2 };
    for k in 0..n_cup {
        let cz = if n_cup == 1 {
            0.0
        } else {
            (if k != 0 { 1.0 } else { -1.0 }) * d * 0.22
        };
        box0(b, body, 1.5, 1.5, 1.5, 0.0, ridge - 0.5, cz);
        for sx in [-1.0, 1.0] {
            cbox0(b, "trim", 0.06, 0.9, 1.1, sx * 0.77, ridge + 0.5, cz);
        }
        put_r(
            b,
            roof,
            &cone_geometry(1.35, 1.1, 4.0, 1.0, false, 0.0, 2.0 * PI),
            0.0,
            ridge + 1.55,
            cz,
            0.0,
            PI / 4.0,
            0.0,
        );
        if k == 0 {
            box0(b, "steelOld", 0.05, 1.3, 0.05, 0.0, ridge + 2.0, cz);
            cbox0(b, "steelOld", 0.04, 0.3, 0.9, 0.0, ridge + 2.95, cz);
            cbox0(b, "steelOld", 0.7, 0.03, 0.03, 0.0, ridge + 2.7, cz);
            cbox0(b, "steelOld", 0.03, 0.03, 0.7, 0.0, ridge + 2.7, cz);
        }
    }
    // Open-fronted lean-to down one side on most barns.
    if rng.next_f64() < 0.65 {
        let sx = if rng.next_f64() < 0.5 { -1.0 } else { 1.0 };
        let lw = 4.5;
        let lh = hh * 0.62;
        let ld = d * rrange(rng, 0.45, 0.7);
        let lz = rrange(rng, -0.2, 0.2) * (d - ld);
        box0(b, body, 0.25, lh, ld, sx * (ww + lw - 0.12), 0.0, lz);
        box0(
            b,
            body,
            lw,
            lh,
            0.25,
            sx * (ww + lw / 2.0),
            0.0,
            lz - ld / 2.0 + 0.12,
        );
        for k in 1..4 {
            let k = f64::from(k);
            box0(
                b,
                "woodDark",
                0.2,
                lh,
                0.2,
                sx * (ww + lw - 0.12),
                0.0,
                lz - ld / 2.0 + (k * ld) / 4.0,
            );
        }
        let a = kernel::atan2(hh - 0.4 - lh, lw);
        b.cbox(
            roof,
            lw + 0.8,
            0.14,
            ld + 0.6,
            sx * (ww + lw / 2.0 + 0.1),
            (hh - 0.35 + lh) / 2.0 + 0.2,
            lz,
            [0.0, 0.0, -sx * a],
        );
    }
    Barn {
        w,
        d,
        h: hh + r,
        lamp: v(0.0, hh + r * 0.35, fz + 0.4),
    }
}

/// `silo(B, rng, x, z, { metal })`: the JS only ever passes `metal`.
pub fn silo(b: &mut Builder, rng: &mut Mulberry32, x: f64, z: f64, metal: bool) -> (f64, f64) {
    let r = rrange(rng, 2.2, 3.0);
    let h = rrange(rng, 11.0, 16.0);
    let key = if metal { "siloMetal" } else { "concrete" };
    b.put_at(
        key,
        &cylinder_geometry(r, r, h, 18.0, 1.0, true, 0.0, 2.0 * PI),
        x,
        h / 2.0,
        z,
    );
    b.put_at(
        "siloDome",
        &sphere_geometry(r * 1.03, 18.0, 7.0, 0.0, PI * 2.0, 0.0, PI / 2.0),
        x,
        h,
        z,
    );
    let bands = if metal { 7 } else { 4 };
    let band = cylinder_geometry(r + 0.05, r + 0.05, 0.12, 18.0, 1.0, true, 0.0, 2.0 * PI);
    for i in 1..bands {
        b.put_at(
            "siloBand",
            &band,
            x,
            (h * f64::from(i)) / f64::from(bands),
            z,
        );
    }
    // Ladder cage with hoops, a vent cap and (on stave silos) the chute.
    box0(b, "siloBand", 0.5, h, 0.08, x, 0.0, z + r + 0.15);
    let hoop = torus_geometry(0.4, 0.03, 3.0, 8.0, PI);
    let mut y = 2.5;
    while y < h {
        put_r(b, "siloBand", &hoop, x, y, z + r + 0.2, PI / 2.0, 0.0, 0.0);
        y += 1.4;
    }
    b.put_at(
        "siloBand",
        &cyl(0.25, 0.35, 0.6, 8.0),
        x,
        h + r * 1.03 - 0.1,
        z,
    );
    if !metal {
        box0(b, "woodGray", 0.9, h - 1.0, 0.7, x - r - 0.3, 0.0, z);
    }
    (r, h)
}

/// `shed(B, rng)` (the JS `opts` is always empty).
pub fn shed(b: &mut Builder, rng: &mut Mulberry32) -> (f64, f64) {
    let w = rrange(rng, 5.0, 8.0);
    let d = rrange(rng, 4.0, 6.0);
    let hh = rrange(rng, 2.6, 3.4);
    let wall = *rpick(rng, &["woodGray", "woodGray", "barnRed2"]);
    box0(b, wall, w, hh, d, 0.0, 0.0, 0.0);
    // Lean-to roof sloping back.
    let a = 0.18;
    b.cbox(
        "roofRust",
        w + 0.6,
        0.12,
        d + 0.8,
        0.0,
        hh + 0.25,
        0.0,
        [-a, 0.0, 0.0],
    );
    cbox0(
        b,
        "woodDark",
        w * 0.45,
        hh * 0.75,
        0.08,
        -w * 0.15,
        hh * 0.375,
        d / 2.0 + 0.04,
    );
    (w, d)
}

/// Lattice tower of an old windpump; returns the hub position.
pub fn windpump_tower(b: &mut Builder, h: f64) -> Vector3 {
    let base = 1.5;
    let top = 0.3;
    let corners = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
    for [cx, cz] in corners {
        b.beam(
            "steelOld",
            v(cx * base, 0.0, cz * base),
            v(cx * top, h, cz * top),
            0.1,
        );
    }
    for lv in 0..4 {
        let lv = f64::from(lv);
        let y0 = (h * lv) / 4.0;
        let y1 = (h * (lv + 1.0)) / 4.0;
        let w0 = base + (top - base) * (lv / 4.0);
        let w1 = base + (top - base) * ((lv + 1.0) / 4.0);
        for e in 0..4 {
            let [ax, az] = corners[e];
            let [bx, bz] = corners[(e + 1) % 4];
            b.beam(
                "steelOld",
                v(ax * w0, y0, az * w0),
                v(bx * w1, y1, bz * w1),
                0.05,
            );
            b.beam(
                "steelOld",
                v(ax * w1, y1, az * w1),
                v(bx * w1, y1, bz * w1),
                0.06,
            );
        }
    }
    // Head, tail boom and vane (vane faces along -Z: wheel faces +Z).
    cbox0(b, "steelOld", 0.5, 0.5, 0.9, 0.0, h + 0.3, 0.0);
    cbox0(b, "steelOld", 0.08, 0.08, 2.6, 0.0, h + 0.35, -1.6);
    cbox0(b, "vane", 0.05, 1.1, 1.6, 0.0, h + 0.45, -3.0);
    // Pump rod + water tank at the foot.
    b.beam("steelOld", v(0.0, 0.5, 0.0), v(0.0, h, 0.0), 0.05);
    b.put_at("woodDark", &cyl(1.6, 1.6, 1.2, 14.0), 2.8, 0.6, 1.2);
    v(0.0, h + 0.3, 0.75)
}

/// Blade wheel geometry (axis = local Z), for instancing.
pub fn windpump_wheel_geometry(b: &mut Builder) -> Option<BufferGeometry> {
    let n = 16;
    let blade = boxg(0.42, 1.5, 0.03);
    for i in 0..n {
        let a = (f64::from(i) / f64::from(n)) * PI * 2.0;
        let r = 1.35;
        put_r(
            b,
            "x",
            &blade,
            kernel::sin(a) * r,
            kernel::cos(a) * r,
            0.0,
            0.35,
            0.0,
            -a,
        );
    }
    b.put_at(
        "x",
        &torus_geometry(2.05, 0.035, 4.0, 32.0, PI * 2.0),
        0.0,
        0.0,
        0.0,
    );
    b.put_at(
        "x",
        &torus_geometry(0.65, 0.035, 4.0, 20.0, PI * 2.0),
        0.0,
        0.0,
        0.0,
    );
    let spoke = boxg(0.05, 4.1, 0.05);
    for i in 0..4 {
        put_r(
            b,
            "x",
            &spoke,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            (f64::from(i) * PI) / 4.0,
        );
    }
    put_r(
        b,
        "x",
        &cyl(0.15, 0.15, 0.4, 8.0),
        0.0,
        0.0,
        0.0,
        PI / 2.0,
        0.0,
        0.0,
    );
    b.merge_all()
}

/// Mill waterwheel (axis = local X). The JS defaults are `R = 3.2`,
/// `W = 1.4`.
pub fn waterwheel_geometry(b: &mut Builder, r: f64, w: f64) -> Option<BufferGeometry> {
    let rim = torus_geometry(r, 0.1, 5.0, 32.0, PI * 2.0);
    let hub = torus_geometry(r * 0.45, 0.08, 5.0, 20.0, PI * 2.0);
    let spoke = boxg(0.12, r * 2.0, 0.14);
    for sx in [-w / 2.0, w / 2.0] {
        put_r(b, "x", &rim, sx, 0.0, 0.0, 0.0, PI / 2.0, 0.0);
        put_r(b, "x", &hub, sx, 0.0, 0.0, 0.0, PI / 2.0, 0.0);
        for i in 0..8 {
            put_r(
                b,
                "x",
                &spoke,
                sx,
                0.0,
                0.0,
                (f64::from(i) * PI) / 8.0,
                0.0,
                0.0,
            );
        }
    }
    let n = 20;
    let paddle = boxg(w + 0.1, 0.06, 0.7);
    for i in 0..n {
        let a = (f64::from(i) / f64::from(n)) * PI * 2.0;
        put_r(
            b,
            "x",
            &paddle,
            0.0,
            kernel::cos(a) * (r - 0.3),
            kernel::sin(a) * (r - 0.3),
            -a,
            0.0,
            0.0,
        );
    }
    put_r(
        b,
        "x",
        &cyl(0.22, 0.22, w + 1.2, 10.0),
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        PI / 2.0,
    );
    b.merge_all()
}

/// `millHouse`'s anchors.
#[derive(Clone, Copy, Debug)]
pub struct MillHouse {
    pub w: f64,
    pub d: f64,
    pub wheel: Vector3,
}

/// Stone-and-timber watermill building (front toward +Z). Wheel mounts on
/// +X side.
pub fn mill_house(b: &mut Builder, rng: &mut Mulberry32) -> MillHouse {
    let (w, d, hh, rh) = (9.0, 8.0, 6.5, 3.2);
    box0(b, "stone", w + 0.6, 3.6, d + 0.6, 0.0, -3.4, 0.0); // foundation, goes down the bank
    box0(b, "stoneLight", w, 3.2, d, 0.0, 0.0, 0.0);
    box0(b, "woodGray", w, hh - 3.2, d, 0.0, 3.2, 0.0);
    put_r(
        b,
        "woodGray",
        &gable_prism(d, w, rh),
        0.0,
        hh,
        0.0,
        0.0,
        PI / 2.0,
        0.0,
    );
    // Ridge runs along Z here (gable faces front).
    let a = kernel::atan2(rh, w / 2.0);
    let l = (w / 2.0) / kernel::cos(a) + 0.5;
    for side in [1.0, -1.0] {
        b.cbox(
            "roofShingle",
            l,
            0.16,
            d + 1.0,
            side * (l / 2.0) * kernel::cos(a),
            hh + rh - (l / 2.0) * kernel::sin(a) + 0.08,
            0.0,
            [0.0, 0.0, -side * a],
        );
    }
    for sx in [-1.0, 1.0] {
        for sz in [-1.0, 1.0] {
            box0(
                b,
                "trim",
                0.2,
                hh - 3.2,
                0.2,
                sx * w / 2.0,
                3.2,
                sz * d / 2.0,
            );
        }
    }
    window_pane(b, true, -2.0, 4.6, d / 2.0 + 0.03, 1.0, 1.3, 0.0, None);
    let l2 = rng.next_f64() < 0.5;
    window_pane(b, l2, 2.0, 4.6, d / 2.0 + 0.03, 1.0, 1.3, 0.0, None);
    window_pane(b, true, 0.0, 1.8, d / 2.0 + 0.03, 1.2, 1.1, 0.0, None);
    cbox0(b, "woodDark", 1.4, 2.3, 0.1, -3.0, 1.15, d / 2.0 + 0.04);
    box0(b, "brick", 0.9, 3.5, 0.9, -2.5, hh + 0.5, -1.5);
    // Axle bearing on the +X wall.
    cbox0(b, "steelOld", 0.6, 0.6, 0.6, w / 2.0 + 0.3, 1.8, 0.0);
    cbox0(b, "lamp", 0.25, 0.3, 0.25, -3.0, 2.6, d / 2.0 + 0.25);
    MillHouse {
        w,
        d,
        wheel: v(w / 2.0 + 1.4, 1.8, 0.0),
    }
}

/// `generalStore`'s anchors.
#[derive(Clone, Copy, Debug)]
pub struct Store {
    pub w: f64,
    pub d: f64,
    pub sign: Vector3,
    pub neon: Vector3,
}

/// Western false-front general store with a porch; returns sign anchor.
/// (The JS passes an `rng` it never draws from.)
pub fn general_store(b: &mut Builder) -> Store {
    let (w, d, hh) = (13.0, 10.0, 4.2);
    box0(b, "stone", w + 0.3, 0.45, d + 0.3, 0.0, -0.3, 0.0);
    box0(b, "woodGray", w, hh, d, 0.0, 0.15, 0.0);
    put_r(
        b,
        "woodGray",
        &gable_prism(d, w, 2.2),
        0.0,
        hh + 0.15,
        0.0,
        0.0,
        PI / 2.0,
        0.0,
    );
    gable_roof_z(b, "roofRust", w, d, hh + 0.15, 2.2, 0.4, 0.16);
    // False front
    box0(
        b,
        "wallCream",
        w + 0.6,
        7.6,
        0.35,
        0.0,
        0.15,
        d / 2.0 + 0.18,
    );
    box0(b, "trim", w + 1.0, 0.35, 0.6, 0.0, 7.75, d / 2.0 + 0.2);
    // Porch
    box0(b, "woodDark", w + 1.0, 0.3, 3.0, 0.0, 0.0, d / 2.0 + 1.8);
    for k in 0..5 {
        box0(
            b,
            "trim",
            0.16,
            3.1,
            0.16,
            -w / 2.0 + (f64::from(k) * w) / 4.0,
            0.3,
            d / 2.0 + 3.1,
        );
    }
    b.cbox(
        "roofRust",
        w + 1.2,
        0.12,
        3.4,
        0.0,
        3.55,
        d / 2.0 + 1.9,
        [0.12, 0.0, 0.0],
    );
    // Shop windows (lit) and door
    window_pane(b, true, -3.8, 1.9, d / 2.0 + 0.38, 2.6, 1.8, 0.0, None);
    window_pane(b, true, 3.8, 1.9, d / 2.0 + 0.38, 2.6, 1.8, 0.0, None);
    cbox0(b, "woodDark", 1.4, 2.4, 0.1, 0.0, 1.45, d / 2.0 + 0.4);
    window_pane(b, true, 0.0, 5.6, d / 2.0 + 0.38, 1.4, 1.1, 0.0, None);
    cbox0(b, "lamp", 0.25, 0.3, 0.25, -1.3, 3.1, d / 2.0 + 0.5);
    cbox0(b, "lamp", 0.25, 0.3, 0.25, 1.3, 3.1, d / 2.0 + 0.5);
    Store {
        w,
        d,
        sign: v(0.0, 6.6, d / 2.0 + 0.38),
        neon: v(3.8, 2.95, d / 2.0 + 0.42),
    }
}

/// Gable roof with the ridge along Z (gable facing the front). The JS
/// defaults are `over = 0.4`, `t = 0.16`.
fn gable_roof_z(b: &mut Builder, key: &str, w: f64, d: f64, h: f64, rh: f64, over: f64, t: f64) {
    let a = kernel::atan2(rh, w / 2.0);
    let l = (w / 2.0) / kernel::cos(a) + over;
    for side in [1.0, -1.0] {
        b.cbox(
            key,
            l,
            t,
            d + over * 2.0,
            side * (l / 2.0) * kernel::cos(a),
            h + rh - (l / 2.0) * kernel::sin(a) + t / 2.0,
            0.0,
            [0.0, 0.0, -side * a],
        );
    }
}

/// Vintage gas pump (front toward +Z).
pub fn gas_pump(b: &mut Builder, x: f64, z: f64) {
    box0(b, "pumpRed", 0.7, 1.7, 0.55, x, 0.25, z);
    cbox0(b, "trim", 0.5, 0.6, 0.05, x, 1.4, z + 0.29);
    b.put_at(
        "pumpGlobe",
        &sphere_geometry(0.28, 12.0, 8.0, 0.0, PI * 2.0, 0.0, PI),
        x,
        2.25,
        z,
    );
    box0(b, "stoneLight", 1.6, 0.25, 1.2, x, 0.0, z);
    b.cbox(
        "black",
        0.06,
        0.06,
        0.6,
        x + 0.38,
        1.1,
        z + 0.1,
        [0.4, 0.0, 0.0],
    );
}

pub fn mailbox(b: &mut Builder, x: f64, z: f64, ry: f64) {
    b.box_yaw("woodDark", 0.12, 1.05, 0.12, x, 0.0, z, ry);
    put_r(
        b,
        "mailbox",
        &cylinder_geometry(0.2, 0.2, 0.55, 10.0, 1.0, false, 0.0, PI),
        x,
        1.2,
        z,
        0.0,
        ry,
        PI / 2.0,
    );
    put_r(b, "mailbox", &boxg(0.4, 0.2, 0.55), x, 1.1, z, 0.0, ry, 0.0);
}

/// Paddock fence around a rectangle (local), leaving a gate gap on +Z.
pub fn paddock(b: &mut Builder, w: f64, d: f64, gate: f64) {
    let mut posts: Vec<[f64; 2]> = Vec::new();
    let mut edge = |b: &mut Builder, ax: f64, az: f64, bx: f64, bz: f64, gap: bool| {
        let len = kernel::hypot(bx - ax, bz - az);
        let n = js::max(1.0, js::round(len / 3.0));
        let mut i = 0.0;
        while i <= n {
            let t = i / n;
            let x = ax + (bx - ax) * t;
            let z = az + (bz - az) * t;
            i += 1.0;
            if gap && x.abs() < gate / 2.0 {
                continue;
            }
            posts.push([x, z]);
        }
        for h in [0.55, 1.05] {
            if gap {
                b.beam("woodGray", v(ax, h, az), v(-gate / 2.0, h, bz), 0.09);
                b.beam("woodGray", v(gate / 2.0, h, az), v(bx, h, bz), 0.09);
            } else {
                b.beam("woodGray", v(ax, h, az), v(bx, h, bz), 0.09);
            }
        }
    };
    edge(b, -w / 2.0, d / 2.0, w / 2.0, d / 2.0, true);
    edge(b, w / 2.0, d / 2.0, w / 2.0, -d / 2.0, false);
    edge(b, w / 2.0, -d / 2.0, -w / 2.0, -d / 2.0, false);
    edge(b, -w / 2.0, -d / 2.0, -w / 2.0, d / 2.0, false);
    for [x, z] in posts {
        box0(b, "woodDark", 0.14, 1.25, 0.14, x, 0.0, z);
    }
}

/// Wooden board sign on two posts, textured face.
pub fn board_sign(b: &mut Builder, key: &str, w: f64, h: f64, x: f64, z: f64, ry: f64, y0: f64) {
    let c = kernel::cos(ry);
    let s = kernel::sin(ry);
    for sx in [-1.0, 1.0] {
        box0(
            b,
            "woodDark",
            0.16,
            y0 + h,
            0.16,
            x + c * sx * (w / 2.0 - 0.2),
            0.0,
            z - s * sx * (w / 2.0 - 0.2),
        );
    }
    put_r(
        b,
        "woodDark",
        &boxg(w + 0.2, h + 0.2, 0.08),
        x - s * 0.02,
        y0 + h / 2.0,
        z - c * 0.02,
        0.0,
        ry,
        0.0,
    );
    put_r(
        b,
        key,
        &plane_geometry(w, h, 1.0, 1.0),
        x + s * 0.05,
        y0 + h / 2.0,
        z + c * 0.05,
        0.0,
        ry,
        0.0,
    );
}

/// Merged cow geometry (length along Z, head at +Z) for instancing: a
/// rounded barrel of a body, bony hips, a drooping neck and tapered head.
pub fn cow_geometry(b: &mut Builder) -> Option<BufferGeometry> {
    b.put(
        "x",
        &crate::three_geom::capsule_geometry(0.4, 0.95, 3.0, 8.0, 1.0),
        0.0,
        1.02,
        -0.02,
        [PI / 2.0, 0.0, 0.0],
        [1.0, 1.0, 1.05],
    );
    b.put_at("x", &boxg(0.62, 0.3, 0.4), 0.0, 1.3, -0.62); // hips
    put_r(
        b,
        "x",
        &cyl(0.2, 0.3, 0.55, 7.0),
        0.0,
        1.18,
        0.82,
        1.05,
        0.0,
        0.0,
    ); // neck
    put_r(
        b,
        "x",
        &cyl(0.11, 0.19, 0.55, 7.0),
        0.0,
        1.02,
        1.18,
        2.2,
        0.0,
        0.0,
    ); // head, muzzle down
    for sx in [-1.0, 1.0] {
        put_r(
            b,
            "x",
            &boxg(0.2, 0.05, 0.1),
            sx * 0.2,
            1.22,
            1.05,
            0.0,
            0.0,
            sx * 0.3,
        ); // ears
        put_r(
            b,
            "x",
            &cone_geometry(0.03, 0.14, 4.0, 1.0, false, 0.0, 2.0 * PI),
            sx * 0.11,
            1.33,
            1.03,
            0.0,
            0.0,
            -sx * 0.6,
        ); // horns
    }
    let leg = cyl(0.07, 0.1, 0.78, 5.0);
    for [lx, lz] in [[-0.24, 0.52], [0.24, 0.52], [-0.24, -0.58], [0.24, -0.58]] {
        b.put_at("x", &leg, lx, 0.39, lz);
    }
    put_r(
        b,
        "x",
        &cyl(0.025, 0.03, 0.7, 4.0),
        0.0,
        1.0,
        -0.98,
        0.18,
        0.0,
        0.0,
    );
    b.put(
        "x",
        &sphere_geometry(0.14, 6.0, 4.0, 0.0, PI * 2.0, 0.0, PI),
        0.0,
        0.62,
        -0.25,
        [0.0; 3],
        [1.0, 0.7, 1.2],
    ); // udder
    b.merge_all()
}

/// Telephone pole with crossarm (arm along local X).
pub fn pole_geometry(b: &mut Builder) -> Option<BufferGeometry> {
    b.put_at("x", &cyl(0.13, 0.17, 9.2, 7.0), 0.0, 4.6, 0.0);
    b.put_at("x", &boxg(2.2, 0.14, 0.14), 0.0, 8.6, 0.0);
    let ins = cyl(0.05, 0.06, 0.2, 6.0);
    for x in [-0.95, 0.0, 0.95] {
        b.put_at("x", &ins, x, 8.77, 0.0);
    }
    put_r(b, "x", &boxg(0.06, 0.8, 0.06), 0.5, 8.2, 0.0, 0.0, 0.0, 0.9);
    put_r(
        b,
        "x",
        &boxg(0.06, 0.8, 0.06),
        -0.5,
        8.2,
        0.0,
        0.0,
        0.0,
        -0.9,
    );
    b.merge_all()
}

/// Tower mill (the valley's windmill): a stone plinth, a tapering white
/// boarded tower with a reefing stage, and a boat-shaped cap. The sails
/// turn separately; returns the windshaft hub (sails face local +Z).
pub fn tower_mill(b: &mut Builder, rng: &mut Mulberry32) -> Vector3 {
    put_r(
        b,
        "stone",
        &cyl(3.3, 3.7, 3.0, 8.0),
        0.0,
        1.3,
        0.0,
        0.0,
        PI / 8.0,
        0.0,
    );
    put_r(
        b,
        "barnWhite",
        &cyl(2.1, 3.1, 9.4, 8.0),
        0.0,
        2.8 + 4.7,
        0.0,
        0.0,
        PI / 8.0,
        0.0,
    );
    // Stage (gallery) with posts and a rail.
    b.put_at("woodDark", &cyl(4.3, 4.3, 0.18, 16.0), 0.0, 5.4, 0.0);
    for k in 0..16 {
        let a = (f64::from(k) / 16.0) * PI * 2.0;
        box0(
            b,
            "woodDark",
            0.1,
            1.0,
            0.1,
            kernel::cos(a) * 4.2,
            5.45,
            kernel::sin(a) * 4.2,
        );
        let a2 = (f64::from(k + 1) / 16.0) * PI * 2.0;
        b.beam(
            "woodDark",
            v(kernel::cos(a) * 4.2, 6.4, kernel::sin(a) * 4.2),
            v(kernel::cos(a2) * 4.2, 6.4, kernel::sin(a2) * 4.2),
            0.08,
        );
        if k % 4 == 0 {
            b.beam(
                "woodDark",
                v(kernel::cos(a) * 4.2, 5.4, kernel::sin(a) * 4.2),
                v(kernel::cos(a) * 2.9, 3.4, kernel::sin(a) * 2.9),
                0.12,
            );
        }
    }
    // Cap: a ring with an onion of shingles, stretched fore-aft.
    b.put_at("woodDark", &cyl(2.35, 2.35, 0.5, 16.0), 0.0, 12.45, 0.0);
    b.put(
        "roofShingle",
        &sphere_geometry(2.4, 14.0, 6.0, 0.0, PI * 2.0, 0.0, PI / 2.0),
        0.0,
        12.65,
        0.0,
        [0.0; 3],
        [1.0, 0.95, 1.3],
    );
    b.put_at(
        "trim",
        &sphere_geometry(0.25, 8.0, 6.0, 0.0, PI * 2.0, 0.0, PI),
        0.0,
        14.95,
        0.0,
    );
    // Fantail steering gear at the back.
    b.beam("woodDark", v(0.0, 12.5, -2.2), v(0.0, 11.0, -4.8), 0.12);
    put_r(
        b,
        "vane",
        &cyl(0.9, 0.9, 0.08, 10.0),
        0.0,
        11.4,
        -5.0,
        0.0,
        0.0,
        PI / 2.0,
    );
    // Door and windows up the tower.
    b.cbox(
        "woodDark",
        1.2,
        2.2,
        0.12,
        0.0,
        1.1 + 0.2,
        3.62,
        [-0.0, 0.0, 0.0],
    );
    let l = rng.next_f64() < 0.7;
    window_pane(b, l, 0.0, 4.1, 3.1, 0.7, 0.9, 0.0, None);
    let l = rng.next_f64() < 0.7;
    window_pane(b, l, 0.0, 8.3, 2.55, 0.6, 0.8, 0.0, None);
    window_pane(b, false, 0.0, 10.4, -2.35, 0.55, 0.7, PI, None);
    // Windshaft out through the front of the cap, clear of the stage.
    b.beam("steelOld", v(0.0, 13.25, 1.6), v(0.0, 13.1, 4.6), 0.4);
    v(0.0, 13.1, 4.75)
}

/// Four sails on a windshaft (axis = local Z): stocks, the lattice of each
/// sail frame and its canvas. Built with a paint builder so the wood and
/// the sailcloth stay separate colours in one geometry.
pub fn mill_sails_geometry(b: &mut Builder) -> Option<BufferGeometry> {
    for k in 0..4 {
        let a = (f64::from(k) / 4.0) * PI * 2.0;
        let c = kernel::cos(a);
        let s = kernel::sin(a);
        // Rotate a local (x across, y out) point into the sail's plane.
        let p = |x: f64, y: f64, z: f64| v(c * x - s * y, s * x + c * y, z);
        b.beam("wood", p(0.0, -0.2, 0.0), p(0.0, 10.8, 0.0), 0.26);
        let (x0, x1, r0, r1) = (0.25, 2.1, 1.8, 10.4);
        b.beam("wood", p(x1, r0, 0.05), p(x1, r1, 0.05), 0.09);
        let mut r = r0;
        while r <= r1 + 0.01 {
            b.beam("wood", p(-0.35, r, 0.06), p(x1, r, 0.06), 0.07);
            r += (r1 - r0) / 9.0;
        }
        // Canvas spread over most of the frame.
        let mut g = plane_geometry(x1 - x0, r1 - r0 - 0.3, 1.0, 1.0);
        g.translate((x0 + x1) / 2.0, (r0 + r1) / 2.0, 0.1);
        g.rotate_z(a);
        b.add("cloth", &g, None);
    }
    put_r(
        b,
        "wood",
        &cyl(0.4, 0.45, 0.8, 8.0),
        0.0,
        0.0,
        0.0,
        PI / 2.0,
        0.0,
        0.0,
    );
    b.merge_all()
}
