//! The wheels of `CarModel.js` (`:961-1077`): `wheelGeometry` (tyre, rim
//! and brake groups, the outboard face +X) and `caliperGeom`.

use mp_math::{js, kernel};

use super::detail::dummy_group;
use super::kit::{Greys, O, P3, PI, R0, Tris, fix_normals, grey, lathe_x, pc, prep};
use crate::three_geom::{
    BufferGeometry, ExtrudeOptions, Shape, cylinder_geometry, extrude_geometry, merge_geometries,
};

/// A wheel's rim style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WheelType {
    Split,
    Spoke,
    Aero,
    Steel,
    Hub,
    /// A racer's wheel at low detail.
    Racer,
}

impl WheelType {
    pub fn name(self) -> &'static str {
        match self {
            WheelType::Split => "split",
            WheelType::Spoke => "spoke",
            WheelType::Aero => "aero",
            WheelType::Steel => "steel",
            WheelType::Hub => "hub",
            WheelType::Racer => "racer",
        }
    }
}

/// The rim material a kind asks for (`rimMat`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RimMat {
    Chrome,
    White,
    Aero,
    Tractor,
    Dark,
}

/// The front wheels, where they differ (`front: { r, w, track }`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrontWheels {
    pub r: f64,
    pub w: f64,
    pub track: f64,
}

/// A kind's `wheels` layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelLayout {
    pub r: f64,
    pub w: f64,
    pub track: f64,
    pub z_f: f64,
    pub z_r: f64,
    pub rim_frac: f64,
    pub spokes: f64,
    pub ty: WheelType,
    pub w0: Option<f64>,
    pub w1: Option<f64>,
    pub dish: Option<f64>,
    pub rim_mat: Option<RimMat>,
    pub caliper: Option<usize>,
    pub rear_w: Option<f64>,
    pub front: Option<FrontWheels>,
}

impl WheelLayout {
    pub fn new(
        r: f64,
        w: f64,
        track: f64,
        a: f64,
        rim_frac: f64,
        spokes: f64,
        ty: WheelType,
    ) -> WheelLayout {
        WheelLayout {
            r,
            w,
            track,
            z_f: a,
            z_r: -a,
            rim_frac,
            spokes,
            ty,
            w0: None,
            w1: None,
            dish: None,
            rim_mat: None,
            caliper: None,
            rear_w: None,
            front: None,
        }
    }
}

/// `wheelCache`'s key: `[r, w, lod, style.spokes, style.rimFrac,
/// style.type].join('|')` (numbers by their bits: the same number prints
/// the same in the JS).
pub fn wheel_key(r: f64, w: f64, hi: bool, style: &WheelLayout) -> String {
    format!(
        "{:x}|{:x}|{}|{:x}|{:x}|{}",
        r.to_bits(),
        w.to_bits(),
        if hi { "high" } else { "low" },
        style.spokes.to_bits(),
        style.rim_frac.to_bits(),
        style.ty.name()
    )
}

/// `wheelGeometry(r, w, lod, style)`: groups 0 tyre, 1 rim, 2 brake/inner
/// metal. The wheel's outboard face is +X.
pub fn wheel_geometry(r: f64, w: f64, hi: bool, style: &WheelLayout) -> BufferGeometry {
    let rim_r = r * style.rim_frac;
    let hw = w / 2.0;
    let mut tyre: Vec<BufferGeometry> = Vec::new();
    let mut rim: Vec<BufferGeometry> = Vec::new();
    let mut inner: Vec<BufferGeometry> = Vec::new();
    if hi {
        let sw = r - rim_r;
        // Sidewall bulges past the rim, a rounded shoulder, then the tread.
        tyre.push(lathe_x(
            &[
                [rim_r + 0.012, -hw + 0.02],
                [rim_r + sw * 0.45, -hw - 0.006],
                [r - 0.03, -hw + 0.004],
                [r - 0.008, -hw + 0.022],
                [r, -hw + 0.045],
                [r, hw - 0.045],
                [r - 0.008, hw - 0.022],
                [r - 0.03, hw - 0.004],
                [rim_r + sw * 0.45, hw + 0.006],
                [rim_r + 0.012, hw - 0.02],
            ],
            30.0,
            Greys::Per(&[1.2, 1.25, 1.1, 0.95, 0.85, 0.85, 0.95, 1.1, 1.25, 1.2]),
        ));
        // Rim: outer lip, inner barrel (faces inward), backing plate.
        let lip_x = hw * 0.8;
        rim.push(lathe_x(
            &[
                [rim_r + 0.016, lip_x - 0.012],
                [rim_r + 0.012, lip_x + 0.006],
                [rim_r - 0.012, lip_x + 0.008],
                [rim_r - 0.022, lip_x - 0.01],
            ],
            30.0,
            Greys::Per(&[0.9, 1.15, 1.1, 0.8]),
        ));
        inner.push(lathe_x(
            &[[rim_r - 0.004, lip_x - 0.01], [rim_r - 0.004, -hw * 0.75]],
            24.0,
            Greys::One(0.45),
        ));
        inner.push(lathe_x(
            &[[rim_r, -hw * 0.72], [0.0, -hw * 0.72]],
            16.0,
            Greys::One(0.06),
        ));
        // Brake disc (with a darker hat) sitting inboard of the spokes.
        let dx = -hw * 0.18;
        let dt = 0.026;
        inner.push(lathe_x(
            &[[rim_r * 0.86, dx + dt / 2.0], [rim_r * 0.4, dx + dt / 2.0]],
            24.0,
            Greys::Per(&[0.95, 0.75]),
        ));
        inner.push(lathe_x(
            &[[rim_r * 0.86, dx - dt / 2.0], [rim_r * 0.86, dx + dt / 2.0]],
            24.0,
            Greys::One(0.6),
        ));
        inner.push(lathe_x(
            &[
                [rim_r * 0.4, dx + dt / 2.0],
                [rim_r * 0.4, dx + 0.05],
                [rim_r * 0.2, dx + 0.05],
            ],
            16.0,
            Greys::One(0.35),
        ));
        let face_x = lip_x - 0.006;
        let hub_r = rim_r * 0.3;
        let dish = style.dish.unwrap_or(0.05);
        let mut t = Tris::default();
        let mut spoke = |th: f64, w0: f64, w1: f64, depth: f64, col: f64| {
            // Radial stations from hub to lip; the face curves back
            // (concave) toward the hub.
            struct St {
                fl: P3,
                fr: P3,
                bl: P3,
                br: P3,
                tan: P3,
            }
            let ring: Vec<St> = [0.0, 0.35, 0.7, 1.0]
                .iter()
                .map(|&s| {
                    let rr = hub_r * 0.9 + (rim_r * 0.97 - hub_r * 0.9) * s;
                    let x = face_x - dish * kernel::pow(1.0 - s, 1.6);
                    let ww = (w0 + (w1 - w0) * s) / 2.0;
                    let c = kernel::cos(th);
                    let sn = kernel::sin(th);
                    let p2 = |t: f64, xx: f64| -> P3 { [xx, rr * c - t * sn, rr * sn + t * c] };
                    St {
                        fl: p2(ww, x),
                        fr: p2(-ww, x),
                        bl: p2(ww * 0.8, x - depth),
                        br: p2(-ww * 0.8, x - depth),
                        tan: [0.0, -sn, c],
                    }
                })
                .collect();
            for i in 0..ring.len() - 1 {
                let (a, b) = (&ring[i], &ring[i + 1]);
                t.flat_quad(a.fl, a.fr, b.fr, b.fl, [1.0, 0.0, 0.0], grey(col));
                t.flat_quad(a.fl, a.bl, b.bl, b.fl, a.tan, grey(col * 0.85));
                t.flat_quad(
                    a.fr,
                    a.br,
                    b.br,
                    b.fr,
                    [-a.tan[0], -a.tan[1], -a.tan[2]],
                    grey(col * 0.85),
                );
            }
        };
        let n = style.spokes;
        match style.ty {
            WheelType::Aero => {
                rim.push(lathe_x(
                    &[
                        [rim_r - 0.02, face_x],
                        [rim_r * 0.72, face_x - 0.018],
                        [rim_r * 0.35, face_x - 0.02],
                        [hub_r * 0.9, face_x - 0.012],
                    ],
                    30.0,
                    Greys::Per(&[1.0, 0.95, 0.9, 0.9]),
                ));
                for k in 0..5 {
                    spoke((k as f64 / 5.0) * 2.0 * PI + 0.3, 0.02, 0.07, 0.01, 0.12);
                }
            }
            WheelType::Steel | WheelType::Hub => {
                rim.push(lathe_x(
                    &[
                        [rim_r - 0.02, face_x],
                        [rim_r * 0.62, face_x - 0.03],
                        [hub_r, face_x - 0.035],
                    ],
                    24.0,
                    Greys::Per(&[1.0, 0.9, 0.9]),
                ));
            }
            _ => {
                let split = style.ty == WheelType::Split;
                let mut k = 0.0;
                while k < n {
                    let th = (k / n) * 2.0 * PI;
                    if split {
                        spoke(th - 0.075, 0.03, 0.032, 0.03, 1.0);
                        spoke(th + 0.075, 0.03, 0.032, 0.03, 1.0);
                    } else {
                        spoke(
                            th,
                            style.w0.unwrap_or(0.05),
                            style.w1.unwrap_or(if n > 6.0 { 0.035 } else { 0.06 }),
                            0.035,
                            1.0,
                        );
                    }
                    k += 1.0;
                }
            }
        }
        rim.push(t.geo());
        // Hub face, centre cap and lug nuts.
        let hx = face_x - dish;
        rim.push(lathe_x(
            &[
                [hub_r * 1.05, hx - 0.004],
                [hub_r * 0.75, hx + 0.004],
                [hub_r * 0.45, hx + 0.006],
            ],
            18.0,
            Greys::One(0.9),
        ));
        rim.push(lathe_x(
            &[
                [hub_r * 0.45, hx + 0.006],
                [hub_r * 0.3, hx + 0.018],
                [0.0, hx + 0.022],
            ],
            14.0,
            Greys::Per(&[0.35, 0.3, 0.3]),
        ));
        for k in 0..5 {
            let a = (k as f64 / 5.0) * 2.0 * PI;
            let mut nut = cylinder_geometry(0.011, 0.012, 0.022, 6.0, 1.0, false, 0.0, 2.0 * PI);
            nut.rotate_z(PI / 2.0);
            nut.translate(
                hx + 0.01,
                kernel::cos(a) * hub_r * 0.68,
                kernel::sin(a) * hub_r * 0.68,
            );
            rim.push(prep(nut, O, R0, pc(1.3)));
        }
    } else {
        let e = js::min(0.04, w * 0.18);
        // Outer sidewall + tread only: the inner face is never seen.
        tyre.push(lathe_x(
            &[[r, -hw], [r, hw - e], [r - e, hw], [rim_r, hw]],
            12.0,
            Greys::Per(&[0.9, 0.9, 1.1, 1.2]),
        ));
        if matches!(style.ty, WheelType::Steel | WheelType::Hub) {
            rim.push(lathe_x(
                &[
                    [rim_r * 1.02, hw * 0.8],
                    [rim_r * 0.82, hw * 0.9],
                    [rim_r * 0.35, hw * 0.94],
                    [0.0, hw * 0.95],
                ],
                12.0,
                Greys::One(1.0),
            ));
        } else {
            // Racer at low LOD: flat face with a dark recess ring standing
            // in for spokes.
            rim.push(lathe_x(
                &[[rim_r * 1.02, hw * 0.82], [rim_r * 0.9, hw * 0.84]],
                12.0,
                Greys::One(1.0),
            ));
            inner.push(lathe_x(
                &[[rim_r * 0.9, hw * 0.7], [rim_r * 0.3, hw * 0.72]],
                12.0,
                Greys::One(0.1),
            ));
            rim.push(lathe_x(
                &[[rim_r * 0.3, hw * 0.84], [0.0, hw * 0.86]],
                12.0,
                Greys::One(1.0),
            ));
        }
    }
    let merge = |list: Vec<BufferGeometry>| -> Option<BufferGeometry> {
        if list.is_empty() {
            return None;
        }
        let prepped: Vec<BufferGeometry> = list.into_iter().map(|g| prep(g, O, R0, None)).collect();
        let refs: Vec<&BufferGeometry> = prepped.iter().collect();
        merge_geometries(&refs, false)
    };
    let mut groups = vec![merge(tyre), merge(rim), merge(inner)];
    // Keep three groups (tyre, rim, brake) even when the brake group is
    // empty. (A trailing empty brake group is dropped: traffic wheels draw
    // twice, not three times.)
    if groups[2].is_none() {
        groups.pop();
    }
    let mut parts: Vec<BufferGeometry> = groups
        .into_iter()
        .map(|g| g.unwrap_or_else(|| prep(dummy_group(), O, R0, None)))
        .collect();
    if !hi {
        for g in &mut parts {
            g.delete_attribute("color");
        }
    }
    let refs: Vec<&BufferGeometry> = parts.iter().collect();
    let mut geom = merge_geometries(&refs, true).expect("the wheel's groups merge");
    fix_normals(&mut geom, [1.0, 0.0, 0.0]);
    geom.compute_bounding_sphere();
    geom
}

/// `caliperCache`'s key: `rimR.toFixed(3)`.
pub fn caliper_key(rim_r: f64) -> String {
    format!("{rim_r:.3}")
}

/// `caliperGeom(rimR)`: an annular sector (in the wheel's Z/Y plane)
/// extruded across X.
pub fn caliper_geometry(rim_r: f64) -> BufferGeometry {
    let ro = rim_r * 0.9;
    let ri = rim_r * 0.6;
    let a0 = PI * 0.04;
    let a1 = PI * 0.4;
    let mut s = Shape::new();
    s.absarc(0.0, 0.0, ro, a0, a1, false);
    s.absarc(0.0, 0.0, ri, a1, a0, true);
    let mut g = extrude_geometry(
        &[s],
        &ExtrudeOptions {
            depth: 0.07,
            bevel_enabled: false,
            curve_segments: 4.0,
            ..ExtrudeOptions::THREE_DEFAULTS
        },
    );
    g.translate(0.0, 0.0, -0.035);
    g.rotate_y(PI / 2.0); // shape x → -Z (behind the axle), extrusion → X
    g.clear_groups(); // one draw call, not caps + sides
    g.compute_vertex_normals();
    g
}
