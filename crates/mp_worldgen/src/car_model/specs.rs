//! `SPECS` of `CarModel.js` (`:1168-2016`): one function per kind, filling
//! the part buckets in the JS order and returning the layout.

use std::rc::Rc;

use super::detail::detail;
use super::detail::{
    BodyOpts, CabinOpts, Crease, DloOpts, DotOpts, GapOpts, GlowOpts, InteriorOpts, LampOpts,
    MirrorOpts, Seat, Shaper, Stripe, WingOpts, body_loft, cabin_loft, dlo, dot, gap, gap0, gap1,
    geo, glow, interior, lamp, lightbar, mirrors, number, push_bar, strobes, tip, wheel_wells,
    wing,
};
use super::kit::{
    Arch, Axis, Col, DCol, DecalOpts, Line, LoftOpts, P2, PI, Parts, R0, RibbonOpts, View, amber,
    bump, bx, decal, disc, ellipse, extrude, grey, in_range, interp, linear, loft, pc, ribbon,
    smooth, spline, stations, sweep,
};
use super::wheels::{FrontWheels, RimMat, WheelLayout, WheelType};
use super::{Dims, Layout, Variant};
use crate::three_geom::torus_geometry;

/// A kind's spec: fills the buckets, returns the layout.
pub type Spec = fn(&mut Parts, bool, Variant) -> Layout;

/// `SPECS[kind]`.
pub fn spec(kind: &str) -> Option<Spec> {
    Some(match kind {
        "sports" => sports,
        "muscle" => muscle,
        "super" => super_,
        "electric" => electric,
        "rally" => rally,
        "sedan" => sedan,
        "hatch" => hatch,
        "van" => van,
        "pickup" => pickup,
        "boxtruck" => boxtruck,
        "tractor" => tractor,
        "police" => police,
        "policeSuv" => police_suv,
        _ => return None,
    })
}

use View::{Front, Rear, Side, Top};

/// A decal's options.
fn dd(off: f64, nu: usize, nv: usize, mirror: bool, col: f64) -> DecalOpts {
    DecalOpts {
        off,
        nu,
        nv,
        mirror,
        col: DCol::C(grey(col)),
    }
}

/// A ribbon's options.
fn ro(off: f64, col: f64, mirror: bool) -> RibbonOpts {
    RibbonOpts {
        off,
        col: grey(col),
        mirror,
        ..RibbonOpts::default()
    }
}

fn dims(
    length: f64,
    width: f64,
    height: f64,
    wheel_radius: f64,
    wheel_base: f64,
    track: f64,
) -> Dims {
    Dims {
        length,
        width,
        height,
        wheel_radius,
        wheel_base,
        track,
    }
}

fn hw2(f: impl Fn(f64, f64) -> f64 + 'static) -> Rc<dyn Fn(f64, f64) -> f64> {
    Rc::new(f)
}

fn arches2(a: f64, r: f64, rr: f64) -> Vec<Arch> {
    vec![[a, r, rr], [-a, r, rr]]
}

fn lamp_o(bucket: &'static str, lens: f64, nu: usize, nv: usize) -> LampOpts {
    LampOpts {
        bucket,
        lens,
        nu,
        nv,
        ..LampOpts::default()
    }
}

fn dot_c(col: f64) -> DotOpts {
    DotOpts {
        col: grey(col),
        ..DotOpts::default()
    }
}

fn glow_c(col: Col) -> GlowOpts {
    GlowOpts {
        col,
        ..GlowOpts::default()
    }
}

fn gap_step(step: f64) -> GapOpts {
    GapOpts {
        step,
        ..GapOpts::default()
    }
}

fn crown(stripe: Stripe, xs: &[f64]) -> Vec<f64> {
    if stripe.is_some() {
        xs.to_vec()
    } else {
        Vec::new()
    }
}

fn is_stripe(stripe: Stripe, ax: f64) -> bool {
    stripe.is_some_and(|f| f(ax))
}

// Front-engined GT: long bonnet, fastback, swept headlights, a slim LED
// tail bar, quad-ish diffuser exits and a carbon wing.
fn sports(p: &mut Parts, hi: bool, v: Variant) -> Layout {
    let r = 0.34;
    let wb = 2.6;
    let w = 1.9;
    let rr = r + 0.07;
    let a = wb / 2.0;
    let (z0, z1) = (-2.22, 2.25);
    let arches = arches2(a, r, rr);
    let top: [P2; 11] = [
        [-2.22, 0.6],
        [-2.17, 0.8],
        [-2.08, 0.89],
        [-1.9, 0.91],
        [-1.35, 0.9],
        [0.75, 0.865],
        [1.35, 0.81],
        [1.8, 0.725],
        [2.06, 0.63],
        [2.19, 0.53],
        [2.25, 0.44],
    ];
    let bot: [P2; 6] = [
        [-2.22, 0.42],
        [-2.14, 0.24],
        [-1.8, 0.19],
        [1.8, 0.19],
        [2.14, 0.21],
        [2.25, 0.3],
    ];
    let top_f = spline(&top);
    let stripe: Stripe = if v.stripes {
        Some(|ax| ax < 0.17)
    } else {
        None
    };
    let cab = (-1.95, 0.82);
    // Police livery: white doors on a black car (between the shut lines).
    let police = v.police;
    let white = if hi { "stripe" } else { "plate" };
    let doors = [-0.62, 0.75];
    let mut body = body_loft(
        hi,
        BodyOpts {
            r_top: 0.13,
            end_r: 0.26,
            crown_x: crown(stripe, &[0.17]),
            side: vec![0.56, 0.6, 0.64],
            extra_z: if police { doors.to_vec() } else { Vec::new() },
            mat: Some(Box::new(move |tag, y, z, ax| {
                if tag == 0 {
                    return "trim";
                }
                if hi && tag == 3 && in_range(z, cab.0 + 0.12, cab.1 - 0.12) && ax < 0.6 {
                    return "trim";
                }
                if police && tag != 3 && in_range(z, doors[0], doors[1]) && y > 0.3 {
                    return white;
                }
                if tag == 3 && is_stripe(stripe, ax) {
                    "stripe"
                } else {
                    "paint"
                }
            })),
            ..BodyOpts::new(
                z0,
                z1,
                &top,
                &bot,
                &arches,
                Shaper {
                    nose: 0.1,
                    tail: 0.05,
                    nose_len: 0.95,
                    tail_len: 0.7,
                    end_r: 0.26,
                    tumble: 0.075,
                    y0: 0.64,
                    y1: 0.9,
                    tuck: 0.05,
                    hips: 0.045,
                    hip_z: -a,
                    hip_w: 0.6,
                    hip_y: [0.45, 0.78],
                    crease: Some(Crease {
                        y: 0.6,
                        d: 0.02,
                        h: 0.05,
                    }),
                    arches: arches.clone(),
                    flare: 0.02,
                    lip: 0.008,
                    ..Shaper::new(w, z0, z1)
                }
                .build(),
            )
        },
    );
    p.add_all(geo(&mut body));
    let s = &body.s;
    let mut cabin = cabin_loft(
        hi,
        CabinOpts {
            c_pillar: Some(-1.02),
            crown_x: crown(stripe, &[0.17]),
            stripe,
            ..CabinOpts::new(
                cab.0,
                cab.1,
                &[
                    [-1.95, 0.9],
                    [-1.45, 1.02],
                    [-0.92, 1.17],
                    [-0.5, 1.245],
                    [-0.12, 1.245],
                    [0.12, 1.2],
                    [0.82, 0.86],
                ],
                top_f.clone(),
                hw2(|y, z| {
                    0.745
                        * (1.0 - 0.2 * smooth(0.88, 1.25, y))
                        * (1.0 - 0.12 * smooth(-0.6, -1.95, z))
                }),
                [-0.66, 0.06],
            )
        },
    );
    p.add_all(geo(&mut cabin));
    let c = &cabin.s;
    wheel_wells(p, &[a, -a], r, rr, w, hi);
    // Headlights: swept lenses on the bonnet corners, DRL brow, two
    // projectors.
    let head = [
        [0.47, 2.14],
        [0.64, 2.1],
        [0.8, 2.0],
        [0.87, 1.88],
        [0.84, 1.83],
        [0.7, 1.9],
        [0.5, 2.05],
    ];
    lamp(
        p,
        s,
        Top,
        &head,
        LampOpts {
            nu: if hi { 8 } else { 2 },
            nv: if hi { 5 } else { 1 },
            ..LampOpts::default()
        },
    );
    if hi {
        glow(
            p,
            s,
            Top,
            &[[0.5, 2.07], [0.7, 1.925], [0.84, 1.85]],
            0.016,
            "head",
            GlowOpts::default(),
        );
        dot(p, s, Top, 0.6, 2.06, 0.03, "head", dot_c(0.9));
        dot(p, s, Top, 0.71, 1.99, 0.028, "head", dot_c(0.9));
        glow(
            p,
            s,
            Top,
            &[[0.8, 1.97], [0.85, 1.895]],
            0.014,
            "head",
            glow_c(amber(0.5)),
        );
    }
    // Front: big lower intake with slats, a splitter, bonnet shut lines.
    let intake = [
        [-0.6, 0.22],
        [0.6, 0.22],
        [0.66, 0.34],
        [0.52, 0.39],
        [-0.52, 0.39],
        [-0.66, 0.34],
    ];
    p.add0(
        "trim",
        decal(
            s,
            Front,
            &intake,
            dd(
                0.003,
                if hi { 8 } else { 2 },
                if hi { 3 } else { 1 },
                false,
                0.6,
            ),
        ),
    );
    if hi {
        for y in [0.27, 0.32] {
            p.add0(
                "trim",
                ribbon(
                    s,
                    Front,
                    &[[-0.58, y], [0.58, y]],
                    0.012,
                    ro(0.008, 3.0, false),
                ),
            );
        }
        gap0(p, s, Top, &[[0.64, 0.85], [0.63, 1.5], [0.5, 2.0]]);
        gap1(p, s, Top, &[[-0.44, 2.12], [0.44, 2.12]]);
        // Doors and handle.
        gap0(p, s, Side, &[[0.78, 0.85], [0.73, 0.62], [0.76, 0.33]]);
        gap0(p, s, Side, &[[-0.62, 0.86], [-0.64, 0.6], [-0.6, 0.33]]);
        p.add0(
            "chrome",
            ribbon(
                s,
                Side,
                &[[-0.3, 0.74], [-0.46, 0.74]],
                0.026,
                ro(0.004, 0.8, true),
            ),
        );
        // Fuel flap.
        gap(
            p,
            s,
            Side,
            &ellipse(-1.62, 0.76, 0.07, 0.06, 10),
            GapOpts {
                closed: true,
                mirror: false,
                ..GapOpts::default()
            },
        );
        dlo(p, c, &top_f, 0.66, -1.02, DloOpts::default());
        interior(p, InteriorOpts::new(-0.45, 1.14, 0.4, 0.93));
    }
    p.add0(
        "carbon",
        sweep(
            &[[2.27, 0.19], [2.08, 0.19], [2.08, 0.21], [2.24, 0.215]],
            Axis::X,
            -0.76,
            0.76,
            1,
        ),
    );
    p.pair(
        "carbon",
        || {
            sweep(
                &[[0.0, 0.19], [0.07, 0.19], [0.075, 0.23], [0.0, 0.26]],
                Axis::Z,
                -0.86,
                0.86,
                1,
            )
        },
        0.86,
        0.0,
        0.0,
        R0,
        None,
    );
    mirrors(
        p,
        hi,
        MirrorOpts {
            base: 0.76,
            ..MirrorOpts::at(0.94, 0.97, 0.5)
        },
    );
    // Rear: tail lamps with a C-shaped LED, a full-width light bar between.
    let tl = [
        [0.4, 0.74],
        [0.84, 0.72],
        [0.88, 0.78],
        [0.84, 0.835],
        [0.44, 0.825],
    ];
    lamp(
        p,
        s,
        Rear,
        &tl,
        lamp_o("tail", 0.3, if hi { 8 } else { 2 }, if hi { 3 } else { 1 }),
    );
    if hi {
        glow(
            p,
            s,
            Rear,
            &[[0.46, 0.805], [0.83, 0.805], [0.86, 0.775], [0.82, 0.745]],
            0.018,
            "tail",
            GlowOpts::default(),
        );
        glow(
            p,
            s,
            Rear,
            &[[-0.4, 0.785], [0.4, 0.785]],
            0.012,
            "tail",
            GlowOpts {
                mirror: false,
                col: grey(0.7),
                ..GlowOpts::default()
            },
        );
        gap1(p, s, Rear, &[[-0.36, 0.72], [0.36, 0.72]]);
    }
    p.add0(
        "rev",
        decal(
            s,
            Rear,
            &[[0.3, 0.52], [0.44, 0.52], [0.44, 0.555], [0.3, 0.555]],
            dd(0.004, 2, 1, true, 1.0),
        ),
    );
    p.add0(
        "trim",
        decal(
            s,
            Rear,
            &[[-0.27, 0.45], [0.27, 0.45], [0.27, 0.585], [-0.27, 0.585]],
            dd(0.002, 2, 1, false, 1.0),
        ),
    );
    p.box_("plate", 0.46, 0.11, 0.02, [0.0, 0.517, -2.225], R0, None);
    // Diffuser with strakes and the tips inside it.
    p.add0(
        "carbon",
        sweep(
            &[[-2.24, 0.2], [-1.9, 0.2], [-2.18, 0.4], [-2.26, 0.4]],
            Axis::X,
            -0.72,
            0.72,
            1,
        ),
    );
    for x in [-0.18, 0.18] {
        p.box_("carbon", 0.02, 0.17, 0.3, [x, 0.29, -2.12], R0, None);
    }
    tip(p, hi, 0.46, 0.3, -2.14, 0.048, 0.13, true, 1.0);
    tip(p, hi, 0.34, 0.3, -2.14, 0.048, 0.13, true, 1.0);
    if v.spoiler {
        wing(
            p,
            hi,
            WingOpts {
                span: 1.62,
                chord: 0.3,
                thick: 0.12,
                y: 1.1,
                z: -1.96,
                pitch: -0.1,
                bucket: "carbon",
                plate_h: 0.14,
                uprights: vec![0.46],
                base_y: 0.88,
            },
        );
    } else {
        p.add0(
            "paint",
            sweep(
                &[[-2.2, 0.88], [-2.02, 0.905], [-2.04, 0.93], [-2.23, 0.935]],
                Axis::X,
                -0.72,
                0.72,
                1,
            ),
        );
    }
    let siren = police.then(|| {
        strobes(
            p,
            s,
            hi,
            [0.2, 0.44, 0.28, 0.33],
            [0.34, top_f(-2.07), -2.07],
        )
    });
    Layout {
        dims: dims(4.47, w, 1.25, r, wb, 1.62),
        wheels: WheelLayout::new(r, 0.27, 1.62, a, 0.7, 5.0, WheelType::Split),
        head: [0.0, 0.64, 2.22],
        exhausts: vec![[0.4, 0.3, -2.3], [-0.4, 0.3, -2.3]],
        siren,
    }
}

// '69 muscle: long flat bonnet with a scoop, coke-bottle hips, chrome
// bumpers, quad round lamps in a full-width grille, a full-width tail panel
// with chrome surround.
fn muscle(p: &mut Parts, hi: bool, v: Variant) -> Layout {
    let r = 0.36;
    let wb = 2.8;
    let w = 1.95;
    let rr = r + 0.07;
    let a = wb / 2.0;
    let (z0, z1) = (-2.37, 2.49);
    let arches = arches2(a, r, rr);
    let top: [P2; 8] = [
        [-2.37, 0.86],
        [-2.3, 0.945],
        [-2.1, 0.965],
        [-1.45, 0.975],
        [0.3, 0.98],
        [2.2, 0.945],
        [2.42, 0.91],
        [2.49, 0.86],
    ];
    let bot: [P2; 6] = [
        [-2.37, 0.44],
        [-2.3, 0.27],
        [-1.9, 0.24],
        [1.9, 0.24],
        [2.4, 0.26],
        [2.49, 0.34],
    ];
    let top_f = spline(&top);
    let stripe: Stripe = if v.stripes {
        Some(|ax| ax > 0.08 && ax < 0.3)
    } else {
        None
    };
    let crown_x = crown(stripe, &[0.3, 0.08]);
    let cab = (-1.5, 0.36);
    let police = v.police;
    let white = if hi { "stripe" } else { "plate" };
    let doors = [-0.92, 0.39];
    let mut body = body_loft(
        hi,
        BodyOpts {
            r_top: 0.075,
            crown: 0.02,
            crown_x: crown_x.clone(),
            end_r: 0.14,
            side: vec![0.66, 0.7, 0.74],
            extra_z: if police { doors.to_vec() } else { Vec::new() },
            mat: Some(Box::new(move |tag, y, z, ax| {
                if tag == 0 {
                    return "trim";
                }
                if hi && tag == 3 && in_range(z, cab.0 + 0.12, cab.1 - 0.12) && ax < 0.64 {
                    return "trim";
                }
                if police && tag != 3 && in_range(z, doors[0], doors[1]) && y > 0.34 {
                    return white;
                }
                if tag == 3 && is_stripe(stripe, ax) {
                    "stripe"
                } else {
                    "paint"
                }
            })),
            ..BodyOpts::new(
                z0,
                z1,
                &top,
                &bot,
                &arches,
                Shaper {
                    nose: 0.035,
                    tail: 0.035,
                    end_r: 0.14,
                    tumble: 0.045,
                    y0: 0.72,
                    y1: 0.98,
                    tuck: 0.045,
                    hips: 0.04,
                    hip_z: -a,
                    hip_w: 0.75,
                    hip_y: [0.5, 0.86],
                    crease: Some(Crease {
                        y: 0.7,
                        d: 0.014,
                        h: 0.06,
                    }),
                    arches: arches.clone(),
                    flare: 0.012,
                    lip: 0.006,
                    ..Shaper::new(w, z0, z1).taper(0.5)
                }
                .build(),
            )
        },
    );
    p.add_all(geo(&mut body));
    let s = &body.s;
    let mut cabin = cabin_loft(
        hi,
        CabinOpts {
            b_pillar: Some([-0.64, -0.56]),
            crown_x: crown(stripe, &[0.3, 0.08]),
            stripe,
            linear: true,
            ..CabinOpts::new(
                cab.0,
                cab.1,
                &[
                    [-1.5, 0.965],
                    [-1.12, 1.3],
                    [-0.95, 1.345],
                    [-0.3, 1.345],
                    [-0.14, 1.3],
                    [0.36, 0.965],
                ],
                top_f.clone(),
                hw2(|y, _| 0.78 * (1.0 - 0.15 * smooth(0.96, 1.34, y))),
                [-1.02, -0.2],
            )
        },
    );
    p.add_all(geo(&mut cabin));
    let c = &cabin.s;
    wheel_wells(p, &[a, -a], r, rr, w, hi);
    // Hood scoop with a black mouth.
    p.add0(
        "paint",
        sweep(
            &[[0.55, 0.955], [1.5, 0.955], [1.42, 1.02], [0.6, 1.06]],
            Axis::X,
            -0.3,
            0.3,
            1,
        ),
    );
    p.box_("trim", 0.5, 0.05, 0.02, [0.0, 1.0, 1.43], R0, None);
    // Grille: full-width black panel on the flat nose with chrome surround
    // and horizontal bars; quad round lamps.
    let grille = [[-0.86, 0.5], [0.86, 0.5], [0.86, 0.84], [-0.86, 0.84]];
    p.add0(
        "trim",
        decal(
            s,
            Front,
            &grille,
            dd(0.003, if hi { 6 } else { 1 }, 1, false, 0.7),
        ),
    );
    if hi {
        p.add0(
            "chrome",
            ribbon(
                s,
                Front,
                &grille,
                0.018,
                RibbonOpts {
                    off: 0.006,
                    closed: true,
                    step: 0.1,
                    ..RibbonOpts::default()
                },
            ),
        );
        for y in [0.58, 0.65, 0.72, 0.79] {
            p.add0(
                "chrome",
                ribbon(
                    s,
                    Front,
                    &[[-0.24, y], [0.24, y]],
                    0.008,
                    ro(0.006, 0.7, false),
                ),
            );
        }
    }
    for x in [0.64, 0.4] {
        let rr = if x > 0.5 { 0.095 } else { 0.085 };
        let n = if hi { 16 } else { 8 };
        p.add0(
            "chrome",
            decal(
                s,
                Front,
                &ellipse(x, 0.67, rr + 0.018, rr + 0.018, n),
                dd(0.006, 2, 2, true, 1.0),
            ),
        );
        fn lens(u: f64, w: f64) -> f64 {
            0.25 + 0.75 * bump(mp_math::kernel::hypot(u - 0.5, w - 0.5), 0.0, 0.28)
        }
        p.add0(
            "head",
            decal(
                s,
                Front,
                &ellipse(x, 0.67, rr, rr, n),
                DecalOpts {
                    off: 0.009,
                    nu: 3,
                    nv: 3,
                    mirror: true,
                    col: if hi {
                        DCol::F(lens)
                    } else {
                        DCol::C(grey(1.0))
                    },
                },
            ),
        );
    }
    // Chrome bumpers.
    let bumper = |zc: f64, dir: f64| {
        sweep(
            &[
                [zc - dir * 0.08, 0.34],
                [zc + dir * 0.035, 0.34],
                [zc + dir * 0.06, 0.4],
                [zc + dir * 0.035, 0.46],
                [zc - dir * 0.08, 0.46],
            ],
            Axis::X,
            -0.97,
            0.97,
            1,
        )
    };
    p.add0("chrome", bumper(2.49, 1.0));
    p.add0("chrome", bumper(-2.37, -1.0));
    let corner = [
        [0.0, 0.34],
        [0.06, 0.34],
        [0.07, 0.4],
        [0.06, 0.46],
        [0.0, 0.46],
    ];
    p.pair(
        "chrome",
        || sweep(&corner, Axis::Z, 0.0, 0.32, 1),
        0.9,
        0.0,
        2.2,
        R0,
        None,
    );
    p.pair(
        "chrome",
        || sweep(&corner, Axis::Z, 0.0, 0.3, 1),
        0.9,
        0.0,
        -2.45,
        R0,
        None,
    );
    // Tail panel: black inset, chrome surround, two long lamps, reverse.
    let panel = [[-0.9, 0.62], [0.9, 0.62], [0.9, 0.86], [-0.9, 0.86]];
    p.add0("trim", decal(s, Rear, &panel, dd(0.003, 2, 1, false, 1.0)));
    if hi {
        p.add0(
            "chrome",
            ribbon(
                s,
                Rear,
                &panel,
                0.016,
                RibbonOpts {
                    off: 0.005,
                    closed: true,
                    step: 0.1,
                    ..RibbonOpts::default()
                },
            ),
        );
    }
    let tl = [[0.16, 0.66], [0.86, 0.66], [0.86, 0.82], [0.16, 0.82]];
    p.add0(
        "tail",
        decal(
            s,
            Rear,
            &tl,
            dd(0.006, if hi { 6 } else { 1 }, 1, true, 0.35),
        ),
    );
    if hi {
        for x in [0.33, 0.51, 0.69] {
            p.add0(
                "chrome",
                ribbon(
                    s,
                    Rear,
                    &[[x, 0.665], [x, 0.815]],
                    0.01,
                    ro(0.009, 1.0, true),
                ),
            );
        }
        glow(
            p,
            s,
            Rear,
            &[[0.18, 0.74], [0.84, 0.74]],
            0.05,
            "tail",
            GlowOpts {
                off: 0.008,
                ..GlowOpts::default()
            },
        );
    }
    p.add0(
        "rev",
        decal(
            s,
            Rear,
            &[[0.04, 0.68], [0.13, 0.68], [0.13, 0.8], [0.04, 0.8]],
            dd(0.006, 1, 1, true, 1.0),
        ),
    );
    p.box_("plate", 0.5, 0.13, 0.02, [0.0, 0.54, -2.38], R0, None);
    if hi {
        gap0(p, s, Side, &[[0.42, 0.95], [0.36, 0.7], [0.4, 0.34]]);
        gap0(p, s, Side, &[[-0.9, 0.96], [-0.94, 0.7], [-0.88, 0.34]]);
        p.add0(
            "chrome",
            ribbon(
                s,
                Side,
                &[[-0.62, 0.86], [-0.76, 0.86]],
                0.028,
                ro(0.004, 0.9, true),
            ),
        );
        gap1(p, s, Top, &[[-0.84, -2.3], [0.84, -2.3]]);
        gap0(p, s, Top, &[[0.84, -2.3], [0.84, -1.58]]);
        // Side marker lights near the corners.
        p.add0(
            "head",
            decal(
                s,
                Side,
                &[[2.1, 0.58], [2.24, 0.58], [2.24, 0.62], [2.1, 0.62]],
                DecalOpts {
                    off: 0.004,
                    nu: 1,
                    nv: 1,
                    mirror: true,
                    col: DCol::C(amber(0.4)),
                },
            ),
        );
        p.add0(
            "tail",
            decal(
                s,
                Side,
                &[[-2.08, 0.62], [-2.2, 0.62], [-2.2, 0.66], [-2.08, 0.66]],
                dd(0.004, 1, 1, true, 0.3),
            ),
        );
        p.add0(
            "chrome",
            ribbon(
                s,
                Side,
                &[[1.9, 0.36], [-1.85, 0.36]],
                0.018,
                RibbonOpts {
                    off: 0.003,
                    mirror: true,
                    step: 0.2,
                    ..RibbonOpts::default()
                },
            ),
        );
        dlo(
            p,
            c,
            &top_f,
            0.2,
            -1.4,
            DloOpts {
                bucket: "chrome",
                w: 0.018,
                rear: false,
            },
        );
        interior(
            p,
            InteriorOpts {
                seat: Seat::Rgb([3.5, 2.2, 1.6]),
                ..InteriorOpts::new(-0.62, 1.24, 0.1, 1.02)
            },
        );
    }
    // Ducktail spoiler.
    if v.spoiler {
        p.add0(
            "paint",
            sweep(
                &[[-2.34, 0.955], [-2.12, 0.975], [-2.14, 1.0], [-2.36, 1.02]],
                Axis::X,
                -0.86,
                0.86,
                1,
            ),
        );
    }
    mirrors(
        p,
        hi,
        MirrorOpts {
            w: 0.14,
            h: 0.08,
            d: 0.12,
            shell: "chrome",
            base: 0.8,
            ..MirrorOpts::at(0.93, 1.03, 0.26)
        },
    );
    tip(p, hi, 0.62, 0.27, -2.28, 0.05, 0.16, true, 1.0);
    let siren = police.then(|| {
        strobes(
            p,
            s,
            hi,
            [0.06, 0.27, 0.645, 0.695],
            [0.34, top_f(-1.64), -1.64],
        )
    });
    Layout {
        dims: dims(4.86, w, 1.35, r, wb, 1.64),
        wheels: WheelLayout {
            w0: Some(0.07),
            w1: Some(0.1),
            dish: Some(0.07),
            rim_mat: Some(RimMat::Chrome),
            ..WheelLayout::new(r, 0.29, 1.64, a, 0.62, 5.0, WheelType::Spoke)
        },
        head: [0.0, 0.67, 2.5],
        exhausts: vec![[0.62, 0.27, -2.45], [-0.62, 0.27, -2.45]],
        siren,
    }
}

// Mid-engined wedge: cab-forward canopy, Y-shaped headlights on a low nose,
// huge side intakes, louvred engine cover, hexagonal tail lamps, centre
// exits and a tall carbon wing.
fn super_(p: &mut Parts, hi: bool, v: Variant) -> Layout {
    let r = 0.35;
    let wb = 2.7;
    let w = 2.05;
    let rr = r + 0.07;
    let a = wb / 2.0;
    let (z0, z1) = (-2.28, 2.29);
    let arches = arches2(a, r, rr);
    let top: [P2; 8] = [
        [-2.28, 0.7],
        [-2.22, 0.88],
        [-2.1, 0.93],
        [-0.95, 0.955],
        [0.95, 0.8],
        [1.6, 0.645],
        [2.1, 0.47],
        [2.29, 0.36],
    ];
    let bot: [P2; 6] = [
        [-2.28, 0.44],
        [-2.18, 0.2],
        [-1.8, 0.16],
        [1.8, 0.16],
        [2.2, 0.19],
        [2.29, 0.25],
    ];
    let top_f = spline(&top);
    let stripe: Stripe = if v.stripes {
        Some(|ax| ax < 0.18)
    } else {
        None
    };
    let cab = (-1.0, 1.02);
    let mut body = body_loft(
        hi,
        BodyOpts {
            r_top: 0.1,
            crown: 0.03,
            crown_x: crown(stripe, &[0.18]),
            end_r: 0.3,
            side: vec![0.5, 0.58, 0.66],
            mat: Some(Box::new(move |tag, _y, z, ax| {
                if tag == 0 {
                    return "trim";
                }
                if hi && tag == 3 && in_range(z, cab.0 + 0.1, cab.1 - 0.2) && ax < 0.56 {
                    return "trim";
                }
                if tag == 3 && z < -1.1 && z > -2.05 && ax < 0.56 {
                    return "trim"; // engine cover glass/louvres
                }
                if tag == 3 && is_stripe(stripe, ax) && z > -0.9 {
                    "stripe"
                } else {
                    "paint"
                }
            })),
            ..BodyOpts::new(
                z0,
                z1,
                &top,
                &bot,
                &arches,
                Shaper {
                    nose: 0.16,
                    tail: 0.04,
                    nose_len: 1.05,
                    tail_len: 0.5,
                    end_r: 0.3,
                    tumble: 0.1,
                    y0: 0.52,
                    y1: 0.95,
                    tuck: 0.06,
                    hips: 0.055,
                    hip_z: -a + 0.1,
                    hip_w: 0.65,
                    hip_y: [0.4, 0.82],
                    crease: Some(Crease {
                        y: 0.58,
                        d: 0.024,
                        h: 0.06,
                    }),
                    arches: arches.clone(),
                    flare: 0.025,
                    lip: 0.01,
                    ..Shaper::new(w, z0, z1)
                }
                .build(),
            )
        },
    );
    p.add_all(geo(&mut body));
    let s = &body.s;
    let mut cabin = cabin_loft(
        hi,
        CabinOpts {
            c_pillar: Some(-0.62),
            pillars: "trim",
            crown_x: crown(stripe, &[0.18]),
            stripe,
            ..CabinOpts::new(
                cab.0,
                cab.1,
                &[
                    [-1.0, 0.95],
                    [-0.45, 1.13],
                    [-0.05, 1.14],
                    [0.22, 1.1],
                    [1.02, 0.79],
                ],
                top_f.clone(),
                hw2(|y, z| {
                    0.75 * (1.0 - 0.25 * smooth(0.8, 1.14, y))
                        * (1.0 - 0.08 * smooth(-0.4, -1.0, z))
                }),
                [-0.52, 0.04],
            )
        },
    );
    p.add_all(geo(&mut cabin));
    let c = &cabin.s;
    wheel_wells(p, &[a, -a], r, rr, w, hi);
    // Engine cover louvres.
    for k in 0..6 {
        let z = -1.22 - k as f64 * 0.14;
        p.box_(
            "trim",
            1.04,
            0.02,
            0.07,
            [0.0, top_f(z) + 0.008, z],
            [-0.35, 0.0, 0.0],
            pc(2.5),
        );
    }
    // Y-shaped headlights on the nose slope.
    let head = [
        [0.5, 2.1],
        [0.66, 2.02],
        [0.86, 1.82],
        [0.88, 1.74],
        [0.8, 1.76],
        [0.62, 1.93],
        [0.48, 2.02],
    ];
    lamp(
        p,
        s,
        Top,
        &head,
        lamp_o("head", 0.08, if hi { 8 } else { 2 }, if hi { 5 } else { 1 }),
    );
    if hi {
        glow(
            p,
            s,
            Top,
            &[[0.52, 2.07], [0.66, 1.99], [0.84, 1.8]],
            0.014,
            "head",
            GlowOpts::default(),
        );
        glow(
            p,
            s,
            Top,
            &[[0.66, 1.99], [0.7, 1.9]],
            0.012,
            "head",
            GlowOpts::default(),
        );
        dot(p, s, Top, 0.76, 1.87, 0.024, "head", dot_c(0.9));
        dot(p, s, Top, 0.82, 1.81, 0.022, "head", dot_c(0.9));
        gap0(p, s, Top, &[[0.46, 1.1], [0.44, 1.7], [0.47, 2.0]]);
        gap1(p, s, Top, &[[-0.44, 2.05], [0.44, 2.05]]);
    }
    // Front: three intakes and a splitter.
    p.add0(
        "trim",
        decal(
            s,
            Front,
            &[[0.4, 0.19], [0.88, 0.19], [0.84, 0.32], [0.42, 0.3]],
            dd(0.003, if hi { 4 } else { 1 }, 1, true, 0.5),
        ),
    );
    p.add0(
        "trim",
        decal(
            s,
            Front,
            &[[-0.32, 0.19], [0.32, 0.19], [0.3, 0.3], [-0.3, 0.3]],
            dd(0.003, if hi { 4 } else { 1 }, 1, false, 0.5),
        ),
    );
    p.add0(
        "carbon",
        sweep(
            &[[2.31, 0.15], [2.0, 0.15], [2.0, 0.17], [2.27, 0.175]],
            Axis::X,
            -0.84,
            0.84,
            1,
        ),
    );
    // Side intakes: black scoop ahead of the rear wheel plus a carbon blade.
    let scoop = [[-0.1, 0.36], [-0.8, 0.38], [-0.82, 0.66], [-0.36, 0.62]];
    p.add0(
        "trim",
        decal(
            s,
            Side,
            &scoop,
            dd(
                0.003,
                if hi { 5 } else { 1 },
                if hi { 4 } else { 1 },
                true,
                0.35,
            ),
        ),
    );
    if hi {
        // Carbon lip round the scoop's leading edge, and slats inside it.
        p.add0(
            "carbon",
            ribbon(
                s,
                Side,
                &[[-0.82, 0.66], [-0.36, 0.62], [-0.1, 0.36]],
                0.03,
                ro(0.008, 1.0, true),
            ),
        );
        for y in [0.45, 0.53] {
            p.add0(
                "trim",
                ribbon(
                    s,
                    Side,
                    &[[-0.3 - (y - 0.36) * 0.9, y], [-0.8, y + 0.01]],
                    0.012,
                    ro(0.006, 3.0, true),
                ),
            );
        }
        gap0(p, s, Side, &[[0.98, 0.78], [0.9, 0.55], [0.92, 0.3]]);
        dlo(p, c, &top_f, 0.85, -0.62, DloOpts::default());
        interior(
            p,
            InteriorOpts {
                seat: Seat::Rgb([4.0, 3.2, 0.6]),
                lean: 0.4,
                ..InteriorOpts::new(-0.36, 1.04, 0.42, 0.9)
            },
        );
    }
    p.pair(
        "carbon",
        || {
            sweep(
                &[[0.0, 0.16], [0.08, 0.16], [0.085, 0.2], [0.0, 0.24]],
                Axis::Z,
                -0.9,
                0.9,
                1,
            )
        },
        0.92,
        0.0,
        0.0,
        R0,
        None,
    );
    mirrors(
        p,
        hi,
        MirrorOpts {
            w: 0.17,
            h: 0.08,
            d: 0.14,
            shell: "carbon",
            base: 0.8,
            ..MirrorOpts::at(1.0, 0.9, 0.72)
        },
    );
    // Rear: hex lamps, grille mesh, big diffuser, centre tips, plate.
    let tl = [
        [0.5, 0.72],
        [0.66, 0.66],
        [0.86, 0.7],
        [0.86, 0.78],
        [0.66, 0.8],
        [0.5, 0.78],
    ];
    p.add0(
        "trim",
        decal(
            s,
            Rear,
            &[[-0.92, 0.6], [0.92, 0.6], [0.92, 0.86], [-0.92, 0.86]],
            dd(0.002, if hi { 6 } else { 1 }, 1, false, 0.8),
        ),
    );
    lamp(
        p,
        s,
        Rear,
        &tl,
        LampOpts {
            bucket: "tail",
            lens: 0.15,
            bezel: None,
            nu: if hi { 6 } else { 2 },
            nv: if hi { 3 } else { 1 },
            off: 0.006,
            ..LampOpts::default()
        },
    );
    if hi {
        let o9 = GlowOpts {
            off: 0.009,
            ..GlowOpts::default()
        };
        glow(
            p,
            s,
            Rear,
            &[[0.52, 0.75], [0.66, 0.78], [0.84, 0.75]],
            0.016,
            "tail",
            o9,
        );
        glow(
            p,
            s,
            Rear,
            &[[0.52, 0.73], [0.66, 0.68], [0.84, 0.72]],
            0.016,
            "tail",
            o9,
        );
        for k in 0..7 {
            let y = 0.63 + k as f64 * 0.03;
            p.add0(
                "chrome",
                ribbon(
                    s,
                    Rear,
                    &[[-0.4, y], [0.4, y]],
                    0.006,
                    ro(0.005, 0.25, false),
                ),
            );
        }
    }
    p.add0(
        "rev",
        decal(
            s,
            Rear,
            &[[0.2, 0.5], [0.34, 0.5], [0.34, 0.54], [0.2, 0.54]],
            dd(0.005, 2, 1, true, 1.0),
        ),
    );
    p.box_("plate", 0.5, 0.12, 0.02, [0.0, 0.47, -2.265], R0, None);
    p.add0(
        "carbon",
        sweep(
            &[[-2.3, 0.16], [-1.85, 0.16], [-2.2, 0.4], [-2.3, 0.4]],
            Axis::X,
            -0.8,
            0.8,
            1,
        ),
    );
    for k in -2..=2 {
        if k != 0 {
            p.box_(
                "carbon",
                0.02,
                0.22,
                0.4,
                [k as f64 * 0.3, 0.27, -2.12],
                R0,
                None,
            );
        }
    }
    tip(p, hi, 0.08, 0.4, -2.26, 0.055, 0.12, true, 0.75);
    wing(
        p,
        hi,
        WingOpts {
            span: 1.86,
            chord: 0.36,
            thick: 0.1,
            y: 1.2,
            z: -2.0,
            pitch: -0.12,
            bucket: "carbon",
            plate_h: 0.24,
            uprights: vec![0.52],
            base_y: 0.92,
        },
    );
    Layout {
        dims: dims(4.57, w, 1.14, r, wb, 1.72),
        wheels: WheelLayout {
            dish: Some(0.035),
            caliper: Some(1),
            ..WheelLayout::new(r, 0.3, 1.72, a, 0.72, 5.0, WheelType::Split)
        },
        head: [0.0, 0.55, 2.2],
        exhausts: vec![[0.08, 0.4, -2.4], [-0.08, 0.4, -2.4]],
        siren: None,
    }
}

// Electric GT: long wheelbase, short overhangs, a glass canopy, no grille,
// four-point LED headlights, full-width light blades, aero-disc wheels.
fn electric(p: &mut Parts, hi: bool, _v: Variant) -> Layout {
    let r = 0.36;
    let wb = 2.9;
    let w = 1.98;
    let rr = r + 0.065;
    let a = wb / 2.0;
    let arches = arches2(a, r, rr);
    let (z0, z1) = (-2.36, 2.38);
    let top: [P2; 9] = [
        [-2.36, 0.64],
        [-2.31, 0.86],
        [-2.18, 0.925],
        [-1.5, 0.94],
        [0.9, 0.9],
        [1.55, 0.81],
        [2.05, 0.68],
        [2.28, 0.56],
        [2.38, 0.44],
    ];
    let bot: [P2; 6] = [
        [-2.36, 0.4],
        [-2.27, 0.24],
        [-1.95, 0.17],
        [1.95, 0.17],
        [2.3, 0.2],
        [2.38, 0.3],
    ];
    let top_f = spline(&top);
    let cab = (-1.98, 1.05);
    let mut body = body_loft(
        hi,
        BodyOpts {
            r_top: 0.14,
            crown: 0.03,
            end_r: 0.3,
            side: vec![0.52, 0.6, 0.66],
            mat: Some(Box::new(move |tag, _y, z, ax| {
                if tag == 0 {
                    return "trim";
                }
                if hi && tag == 3 && in_range(z, cab.0 + 0.15, cab.1 - 0.15) && ax < 0.62 {
                    return "trim";
                }
                "paint"
            })),
            ..BodyOpts::new(
                z0,
                z1,
                &top,
                &bot,
                &arches,
                Shaper {
                    nose: 0.14,
                    tail: 0.07,
                    nose_len: 0.95,
                    tail_len: 0.8,
                    end_r: 0.3,
                    tumble: 0.08,
                    y0: 0.6,
                    y1: 0.92,
                    tuck: 0.06,
                    hips: 0.04,
                    hip_z: -a,
                    hip_w: 0.65,
                    hip_y: [0.45, 0.8],
                    crease: Some(Crease {
                        y: 0.6,
                        d: 0.016,
                        h: 0.07,
                    }),
                    arches: arches.clone(),
                    flare: 0.02,
                    lip: 0.006,
                    ..Shaper::new(w, z0, z1)
                }
                .build(),
            )
        },
    );
    p.add_all(geo(&mut body));
    let s = &body.s;
    let mut cabin = cabin_loft(
        hi,
        CabinOpts {
            b_pillar: Some([-0.5, -0.42]),
            // Panoramic glass roof with black pillars.
            mat: Some(Box::new(|tag, _y, z, _ax| {
                if tag == 2 {
                    return "trim";
                }
                if tag != 3 && in_range(z, -0.5, -0.42) {
                    return "trim";
                }
                "glass"
            })),
            ..CabinOpts::new(
                cab.0,
                cab.1,
                &[
                    [-1.98, 0.93],
                    [-1.35, 1.14],
                    [-0.65, 1.265],
                    [-0.1, 1.275],
                    [0.3, 1.235],
                    [1.05, 0.89],
                ],
                top_f.clone(),
                hw2(|y, z| {
                    0.755
                        * (1.0 - 0.22 * smooth(0.9, 1.27, y))
                        * (1.0 - 0.1 * smooth(-0.9, -1.98, z))
                }),
                [-1.35, 0.3],
            )
        },
    );
    p.add_all(geo(&mut cabin));
    let c = &cabin.s;
    wheel_wells(p, &[a, -a], r, rr, w, hi);
    // Headlights: teardrop lens with four LED points and a DRL arc.
    let head = [
        [0.52, 2.2],
        [0.72, 2.12],
        [0.84, 2.0],
        [0.82, 1.94],
        [0.66, 2.02],
        [0.5, 2.12],
    ];
    lamp(
        p,
        s,
        Top,
        &head,
        lamp_o("head", 0.07, if hi { 8 } else { 2 }, if hi { 4 } else { 1 }),
    );
    if hi {
        for [x, z] in [[0.6, 2.13], [0.66, 2.1], [0.7, 2.06], [0.75, 2.03]] {
            dot(
                p,
                s,
                Top,
                x,
                z,
                0.017,
                "head",
                DotOpts {
                    n: 8,
                    ..DotOpts::default()
                },
            );
        }
        glow(
            p,
            s,
            Top,
            &[[0.53, 2.17], [0.72, 2.09], [0.82, 1.97]],
            0.012,
            "head",
            GlowOpts::default(),
        );
        gap0(p, s, Top, &[[0.6, 1.1], [0.58, 1.7], [0.5, 2.12]]);
        gap1(p, s, Top, &[[-0.46, 2.21], [0.46, 2.21]]);
        gap0(p, s, Side, &[[1.02, 0.86], [0.98, 0.6], [1.0, 0.32]]);
        gap0(p, s, Side, &[[-0.46, 0.9], [-0.48, 0.6], [-0.44, 0.3]]);
        gap0(p, s, Side, &[[-1.62, 0.9], [-1.66, 0.6], [-1.6, 0.34]]);
        p.add0(
            "trim",
            ribbon(
                s,
                Side,
                &[[0.6, 0.76], [0.44, 0.76]],
                0.024,
                ro(0.003, 3.0, true),
            ),
        );
        p.add0(
            "trim",
            ribbon(
                s,
                Side,
                &[[-0.86, 0.78], [-1.02, 0.78]],
                0.024,
                ro(0.003, 3.0, true),
            ),
        );
        dlo(
            p,
            c,
            &top_f,
            0.9,
            -1.85,
            DloOpts {
                rear: false,
                ..DloOpts::default()
            },
        );
        interior(
            p,
            InteriorOpts {
                seat: Seat::Rgb([5.0, 5.0, 5.2]),
                ..InteriorOpts::new(-0.4, 1.16, 0.45, 0.95)
            },
        );
    }
    // Front: lower intake with light hooks.
    p.add0(
        "trim",
        decal(
            s,
            Front,
            &[[-0.62, 0.22], [0.62, 0.22], [0.66, 0.34], [-0.66, 0.34]],
            dd(0.003, if hi { 6 } else { 1 }, 1, false, 0.5),
        ),
    );
    p.pair("accent", bx(0.22, 0.025, 0.04), 0.5, 0.36, 2.34, R0, None);
    // Rear: full-width light bar with a black band, diffuser light.
    p.add0(
        "trim",
        decal(
            s,
            Rear,
            &[[-0.9, 0.72], [0.9, 0.72], [0.9, 0.82], [-0.9, 0.82]],
            dd(0.003, if hi { 8 } else { 1 }, 1, false, 0.8),
        ),
    );
    glow(
        p,
        s,
        Rear,
        &[[-0.88, 0.77], [0.88, 0.77]],
        0.022,
        "tail",
        GlowOpts {
            mirror: false,
            col: grey(0.8),
            off: 0.007,
            ..GlowOpts::default()
        },
    );
    p.add0(
        "tail",
        decal(
            s,
            Rear,
            &[[0.62, 0.735], [0.88, 0.735], [0.88, 0.805], [0.62, 0.805]],
            dd(0.008, 2, 1, true, 1.0),
        ),
    );
    p.box_("accent", 1.3, 0.025, 0.04, [0.0, 0.3, -2.33], R0, None);
    p.add0(
        "carbon",
        sweep(
            &[[-2.4, 0.18], [-2.0, 0.18], [-2.3, 0.36], [-2.4, 0.36]],
            Axis::X,
            -0.76,
            0.76,
            1,
        ),
    );
    p.add0(
        "trim",
        decal(
            s,
            Rear,
            &[[-0.27, 0.46], [0.27, 0.46], [0.27, 0.58], [-0.27, 0.58]],
            dd(0.002, 2, 1, false, 1.0),
        ),
    );
    p.box_("plate", 0.46, 0.11, 0.02, [0.0, 0.52, -2.365], R0, None);
    p.add0(
        "rev",
        decal(
            s,
            Rear,
            &[[0.3, 0.45], [0.44, 0.45], [0.44, 0.48], [0.3, 0.48]],
            dd(0.005, 2, 1, true, 1.0),
        ),
    );
    mirrors(
        p,
        hi,
        MirrorOpts {
            w: 0.14,
            h: 0.07,
            d: 0.12,
            shell: "trim",
            base: 0.76,
            ..MirrorOpts::at(0.93, 0.96, 0.76)
        },
    );
    p.pair("accent", bx(0.012, 0.018, 1.9), 0.965, 0.3, 0.0, R0, None); // sill light line
    Layout {
        dims: dims(4.74, w, 1.28, r, wb, 1.7),
        wheels: WheelLayout {
            rim_mat: Some(RimMat::Aero),
            caliper: Some(2),
            ..WheelLayout::new(r, 0.29, 1.7, a, 0.74, 0.0, WheelType::Aero)
        },
        head: [0.0, 0.6, 2.3],
        exhausts: Vec::new(),
        siren: None,
    }
}

// Rally hatch: boxy flared arches, a roof wing, a bonnet light pod, mud
// flaps and a big single exhaust. Stripes plus a side graphic are the
// livery.
fn rally(p: &mut Parts, hi: bool, v: Variant) -> Layout {
    let r = 0.34;
    let wb = 2.55;
    let w = 1.9;
    let rr = r + 0.08;
    let a = wb / 2.0;
    let arches = arches2(a, r, rr);
    let (z0, z1) = (-2.02, 2.1);
    let top: [P2; 7] = [
        [-2.02, 0.95],
        [-1.96, 1.02],
        [0.75, 0.99],
        [1.55, 0.89],
        [1.96, 0.76],
        [2.07, 0.64],
        [2.1, 0.55],
    ];
    let bot: [P2; 6] = [
        [-2.02, 0.44],
        [-1.95, 0.25],
        [-1.62, 0.21],
        [1.62, 0.21],
        [2.02, 0.26],
        [2.1, 0.38],
    ];
    let top_f = spline(&top);
    let stripe: Stripe = if v.stripes {
        Some(|ax| ax > 0.09 && ax < 0.22)
    } else {
        None
    };
    let base = Shaper {
        nose: 0.1,
        tail: 0.04,
        end_r: 0.16,
        tumble: 0.05,
        y0: 0.72,
        y1: 1.0,
        tuck: 0.04,
        crease: Some(Crease {
            y: 0.8,
            d: 0.01,
            h: 0.05,
        }),
        ..Shaper::new(w - 0.14, z0, z1).taper(0.5)
    }
    .build();
    // Box flares over both axles, squared off with a crisp top edge.
    let flare = move |y: f64, z: f64| {
        0.075
            * (smooth(-a - 0.72, -a - 0.5, z) * (1.0 - smooth(-a + 0.5, -a + 0.72, z))
                + smooth(a - 0.72, a - 0.5, z) * (1.0 - smooth(a + 0.5, a + 0.72, z)))
            * smooth(0.3, 0.42, y)
            * (1.0 - smooth(0.7, 0.78, y))
    };
    let cab = (-1.95, 0.78);
    let mut body = body_loft(
        hi,
        BodyOpts {
            r_top: 0.09,
            crown: 0.02,
            crown_x: crown(stripe, &[0.22, 0.09]),
            end_r: 0.16,
            extra_z: vec![
                -1.94,
                a - 0.72,
                a - 0.5,
                a + 0.5,
                a + 0.72,
                -a - 0.72,
                -a - 0.5,
                -a + 0.5,
                -a + 0.72,
            ],
            side: vec![0.7, 0.78, 0.8],
            mat: Some(Box::new(move |tag, _y, z, ax| {
                if tag == 0 {
                    return "trim";
                }
                if hi && tag == 3 && in_range(z, cab.0 + 0.15, cab.1 - 0.12) && ax < 0.62 {
                    return "trim";
                }
                if tag == 3 && is_stripe(stripe, ax) {
                    "stripe"
                } else {
                    "paint"
                }
            })),
            ..BodyOpts::new(
                z0,
                z1,
                &top,
                &bot,
                &arches,
                hw2(move |y, z| base(y, z) + flare(y, z)),
            )
        },
    );
    p.add_all(geo(&mut body));
    let s = &body.s;
    let mut cabin = cabin_loft(
        hi,
        CabinOpts {
            linear: true,
            crown_x: crown(stripe, &[0.22, 0.09]),
            mat: Some(Box::new(move |tag, _y, z, ax| {
                if tag == 2 {
                    return "paint";
                }
                if tag == 3 {
                    return if in_range(z, -1.55, -0.05) {
                        if is_stripe(stripe, ax) {
                            "stripe"
                        } else {
                            "paint"
                        }
                    } else {
                        "glass"
                    };
                }
                if in_range(z, -0.52, -0.43) {
                    return "trim";
                }
                if z < -1.5 {
                    return "paint";
                }
                "glass"
            })),
            ..CabinOpts::new(
                cab.0,
                cab.1,
                &[
                    [-1.95, 1.0],
                    [-1.87, 1.36],
                    [-1.5, 1.46],
                    [-0.2, 1.47],
                    [0.05, 1.43],
                    [0.78, 0.99],
                ],
                top_f.clone(),
                hw2(|y, _| 0.75 * (1.0 - 0.13 * smooth(1.0, 1.46, y))),
                [-1.55, -0.05],
            )
        },
    );
    p.add_all(geo(&mut cabin));
    let c = &cabin.s;
    wheel_wells(p, &[a, -a], r, rr, w, hi);
    // Headlights, and the four-lamp pod on the front bumper.
    let head = [[0.4, 0.66], [0.76, 0.63], [0.78, 0.72], [0.44, 0.76]];
    lamp(
        p,
        s,
        Front,
        &head,
        lamp_o("head", 0.12, if hi { 6 } else { 2 }, if hi { 3 } else { 1 }),
    );
    if hi {
        glow(
            p,
            s,
            Front,
            &[[0.46, 0.74], [0.76, 0.705]],
            0.014,
            "head",
            GlowOpts::default(),
        );
        dot(p, s, Front, 0.56, 0.69, 0.035, "head", dot_c(0.9));
        dot(p, s, Front, 0.7, 0.675, 0.035, "head", dot_c(0.9));
    }
    p.add0(
        "trim",
        decal(
            s,
            Front,
            &[[-0.36, 0.56], [0.36, 0.56], [0.36, 0.72], [-0.36, 0.72]],
            dd(0.003, 2, 1, false, 0.6),
        ),
    );
    p.box_("trim", 1.36, 0.24, 0.1, [0.0, 0.46, 2.12], R0, None);
    for x in [0.2, 0.52] {
        let seg = if hi { 18.0 } else { 8.0 };
        p.pair(
            "chrome",
            disc(0.118, 0.06, seg),
            x,
            0.46,
            2.18,
            [PI / 2.0, 0.0, 0.0],
            None,
        );
        p.pair(
            "head",
            disc(0.1, 0.05, seg),
            x,
            0.46,
            2.19,
            [PI / 2.0, 0.0, 0.0],
            None,
        );
    }
    p.add0(
        "carbon",
        sweep(
            &[[2.15, 0.21], [1.98, 0.21], [1.98, 0.24], [2.13, 0.25]],
            Axis::X,
            -0.84,
            0.84,
            1,
        ),
    );
    if hi {
        for x in [0.2, 0.52] {
            p.pair("chrome", bx(0.2, 0.012, 0.01), x, 0.46, 2.22, R0, None);
        }
        p.pair(
            "trim",
            bx(0.34, 0.02, 0.26),
            0.32,
            0.935,
            1.25,
            [0.12, 0.0, 0.0],
            pc(2.0),
        ); // bonnet vents
        gap0(p, s, Top, &[[0.62, 0.8], [0.6, 1.8]]);
        gap0(p, s, Side, &[[0.72, 0.98], [0.68, 0.6], [0.72, 0.36]]);
        gap0(p, s, Side, &[[-0.52, 1.0], [-0.54, 0.6], [-0.5, 0.36]]);
        // Side livery: a slanted band in the stripe colour plus a number
        // disc.
        if stripe.is_some() {
            p.add0(
                "stripe",
                decal(
                    s,
                    Side,
                    &[[1.4, 0.42], [1.6, 0.42], [-0.2, 0.9], [-0.45, 0.9]],
                    dd(0.0025, 3, 6, true, 1.0),
                ),
            );
            p.add0(
                "plate",
                decal(
                    s,
                    Side,
                    &ellipse(0.1, 0.66, 0.17, 0.15, 14),
                    dd(0.003, 4, 4, true, 1.0),
                ),
            );
            number(p, s, Side, "27", 0.1, 0.66, 0.15);
        }
        dlo(p, c, &top_f, 0.66, -1.5, DloOpts::default());
        interior(
            p,
            InteriorOpts {
                cage: true,
                seat: Seat::Rgb([1.4, 1.4, 6.0]),
                ..InteriorOpts::new(-0.6, 1.36, 0.3, 1.04)
            },
        );
    }
    p.box_("trim", 0.34, 0.07, 0.3, [0.0, 1.5, -0.15], R0, None); // roof scoop
    // Roof wing.
    wing(
        p,
        hi,
        WingOpts {
            span: 1.66,
            chord: 0.36,
            thick: 0.14,
            y: 1.56,
            z: -1.92,
            pitch: 0.12,
            bucket: "paint",
            plate_h: 0.14,
            uprights: Vec::new(),
            base_y: 1.44,
        },
    );
    p.pair("trim", bx(0.04, 0.12, 0.1), 0.36, 1.5, -1.86, R0, None);
    // Rear: tall corner lamps, bumper, diffuser, single big tip.
    let tl = [[0.6, 0.8], [0.8, 0.8], [0.8, 0.98], [0.68, 0.98]];
    lamp(
        p,
        s,
        Rear,
        &tl,
        lamp_o("tail", 0.2, if hi { 3 } else { 1 }, if hi { 4 } else { 1 }),
    );
    if hi {
        glow(
            p,
            s,
            Rear,
            &[[0.69, 0.95], [0.78, 0.95], [0.78, 0.84]],
            0.02,
            "tail",
            GlowOpts::default(),
        );
    }
    p.add0(
        "rev",
        decal(
            s,
            Rear,
            &[[0.62, 0.72], [0.78, 0.72], [0.78, 0.76], [0.62, 0.76]],
            dd(0.005, 2, 1, true, 1.0),
        ),
    );
    p.box_("trim", 1.84, 0.16, 0.12, [0.0, 0.38, -2.0], R0, None);
    p.box_("plate", 0.5, 0.12, 0.02, [0.0, 0.6, -2.03], R0, None);
    tip(p, hi, -0.55, 0.3, -2.02, 0.07, 0.16, false, 1.0);
    // Mud flaps and side skirts.
    p.pair(
        "trim",
        bx(0.34, 0.28, 0.015),
        0.74,
        0.2,
        -a - rr - 0.03,
        R0,
        None,
    );
    p.pair(
        "trim",
        bx(0.3, 0.22, 0.015),
        0.74,
        0.23,
        a - rr - 0.03,
        R0,
        None,
    );
    p.pair(
        "carbon",
        || {
            sweep(
                &[[0.0, 0.2], [0.07, 0.2], [0.075, 0.25], [0.0, 0.28]],
                Axis::Z,
                -0.75,
                0.75,
                1,
            )
        },
        0.86,
        0.0,
        0.0,
        R0,
        None,
    );
    mirrors(
        p,
        hi,
        MirrorOpts {
            w: 0.16,
            h: 0.1,
            d: 0.12,
            base: 0.76,
            ..MirrorOpts::at(0.92, 1.05, 0.62)
        },
    );
    Layout {
        dims: dims(4.12, w, 1.6, r, wb, 1.62),
        wheels: WheelLayout {
            w0: Some(0.04),
            w1: Some(0.03),
            dish: Some(0.03),
            rim_mat: Some(RimMat::White),
            ..WheelLayout::new(r, 0.26, 1.62, a, 0.66, 8.0, WheelType::Spoke)
        },
        head: [0.0, 0.62, 2.12],
        exhausts: vec![[-0.55, 0.3, -2.18]],
        siren: None,
    }
}

fn sedan(p: &mut Parts, hi: bool, _v: Variant) -> Layout {
    let r = 0.33;
    let wb = 2.75;
    let w = 1.82;
    let rr = r + 0.06;
    let a = wb / 2.0;
    let (z0, z1) = (-2.34, 2.4);
    let arches = arches2(a, r, rr);
    let top: [P2; 7] = [
        [-2.34, 0.88],
        [-2.24, 0.98],
        [-1.2, 1.01],
        [0.75, 0.99],
        [2.1, 0.9],
        [2.33, 0.82],
        [2.4, 0.74],
    ];
    let bot: [P2; 6] = [
        [-2.34, 0.5],
        [-2.28, 0.28],
        [-1.9, 0.24],
        [1.9, 0.24],
        [2.35, 0.3],
        [2.4, 0.46],
    ];
    let mut body = body_loft(
        hi,
        BodyOpts {
            r_top: 0.09,
            end_r: 0.2,
            ..BodyOpts::new(
                z0,
                z1,
                &top,
                &bot,
                &arches,
                Shaper {
                    nose: 0.07,
                    tail: 0.05,
                    end_r: 0.2,
                    tumble: 0.06,
                    y0: 0.72,
                    y1: 1.0,
                    arches: arches.clone(),
                    flare: 0.01,
                    ..Shaper::new(w, z0, z1).taper(0.6)
                }
                .build(),
            )
        },
    );
    p.add_all(geo(&mut body));
    let s = &body.s;
    let mut cabin = cabin_loft(
        hi,
        CabinOpts {
            linear: true,
            b_pillar: Some([-0.42, -0.34]),
            ..CabinOpts::new(
                -1.38,
                0.8,
                &[
                    [-1.38, 1.0],
                    [-0.98, 1.4],
                    [-0.75, 1.45],
                    [-0.1, 1.45],
                    [0.12, 1.41],
                    [0.8, 0.98],
                ],
                spline(&top),
                hw2(|y, _| 0.79 * (1.0 - 0.14 * smooth(1.0, 1.44, y))),
                [-0.82, 0.02],
            )
        },
    );
    p.add_all(geo(&mut cabin));
    lamp(
        p,
        s,
        Front,
        &[[0.38, 0.72], [0.84, 0.72], [0.82, 0.83], [0.42, 0.83]],
        lamp_o("head", 0.2, 2, 1),
    );
    p.add0(
        "trim",
        decal(
            s,
            Front,
            &[[-0.34, 0.6], [0.34, 0.6], [0.36, 0.76], [-0.36, 0.76]],
            dd(0.003, 1, 1, false, 1.0),
        ),
    );
    lamp(
        p,
        s,
        Rear,
        &[[0.46, 0.8], [0.84, 0.8], [0.84, 0.92], [0.5, 0.92]],
        lamp_o("tail", 0.3, 2, 1),
    );
    p.add0(
        "rev",
        decal(
            s,
            Rear,
            &[[0.34, 0.82], [0.44, 0.82], [0.44, 0.88], [0.34, 0.88]],
            dd(0.005, 2, 1, true, 1.0),
        ),
    );
    p.add0(
        "trim",
        sweep(
            &[[2.33, 0.3], [2.45, 0.3], [2.46, 0.42], [2.33, 0.44]],
            Axis::X,
            -0.86,
            0.86,
            1,
        ),
    );
    p.add0(
        "trim",
        sweep(
            &[[-2.28, 0.32], [-2.4, 0.32], [-2.41, 0.46], [-2.28, 0.48]],
            Axis::X,
            -0.86,
            0.86,
            1,
        ),
    );
    p.box_("plate", 0.5, 0.12, 0.02, [0.0, 0.62, -2.345], R0, None);
    gap(p, s, Side, &[[0.78, 0.98], [0.76, 0.34]], gap_step(1.0));
    gap(p, s, Side, &[[-0.38, 1.0], [-0.4, 0.34]], gap_step(1.0));
    mirrors(
        p,
        hi,
        MirrorOpts {
            w: 0.14,
            h: 0.1,
            d: 0.1,
            base: 0.78,
            ..MirrorOpts::at(0.9, 1.04, 0.64)
        },
    );
    Layout {
        dims: dims(4.74, w, 1.45, r, wb, 1.56),
        wheels: WheelLayout::new(r, 0.23, 1.56, a, 0.62, 0.0, WheelType::Hub),
        head: [0.0, 0.76, 2.4],
        exhausts: vec![[0.5, 0.26, -2.3]],
        siren: None,
    }
}

fn hatch(p: &mut Parts, hi: bool, _v: Variant) -> Layout {
    let r = 0.31;
    let wb = 2.5;
    let w = 1.75;
    let rr = r + 0.06;
    let a = wb / 2.0;
    let (z0, z1) = (-1.97, 2.05);
    let arches = arches2(a, r, rr);
    let top: [P2; 6] = [
        [-1.97, 0.95],
        [-1.88, 1.02],
        [0.7, 0.98],
        [1.7, 0.86],
        [1.98, 0.74],
        [2.05, 0.62],
    ];
    let bot: [P2; 6] = [
        [-1.97, 0.5],
        [-1.9, 0.28],
        [-1.6, 0.24],
        [1.6, 0.24],
        [2.0, 0.3],
        [2.05, 0.45],
    ];
    let mut body = body_loft(
        hi,
        BodyOpts {
            r_top: 0.09,
            end_r: 0.18,
            ..BodyOpts::new(
                z0,
                z1,
                &top,
                &bot,
                &arches,
                Shaper {
                    nose: 0.09,
                    tail: 0.04,
                    end_r: 0.18,
                    tumble: 0.05,
                    y0: 0.7,
                    y1: 1.0,
                    arches: arches.clone(),
                    flare: 0.012,
                    ..Shaper::new(w, z0, z1).taper(0.5)
                }
                .build(),
            )
        },
    );
    p.add_all(geo(&mut body));
    let s = &body.s;
    let mut cabin = cabin_loft(
        hi,
        CabinOpts {
            linear: true,
            mat: Some(Box::new(|tag, _y, z, _ax| {
                if tag == 2 {
                    return "paint";
                }
                if tag == 3 {
                    return if in_range(z, -1.55, -0.05) {
                        "paint"
                    } else {
                        "glass"
                    };
                }
                if in_range(z, -0.52, -0.43) {
                    return "trim";
                }
                if z < -1.5 {
                    return "paint";
                }
                "glass"
            })),
            ..CabinOpts::new(
                -1.95,
                0.75,
                &[
                    [-1.95, 1.0],
                    [-1.87, 1.38],
                    [-1.5, 1.48],
                    [-0.2, 1.49],
                    [0.05, 1.45],
                    [0.75, 0.98],
                ],
                spline(&top),
                hw2(|y, _| 0.77 * (1.0 - 0.12 * smooth(1.0, 1.48, y))),
                [-1.55, -0.05],
            )
        },
    );
    p.add_all(geo(&mut cabin));
    lamp(
        p,
        s,
        Front,
        &[[0.36, 0.7], [0.78, 0.7], [0.76, 0.8], [0.42, 0.82]],
        lamp_o("head", 0.2, 2, 1),
    );
    p.add0(
        "trim",
        decal(
            s,
            Front,
            &[[-0.32, 0.56], [0.32, 0.56], [0.3, 0.68], [-0.3, 0.68]],
            dd(0.003, 1, 1, false, 1.0),
        ),
    );
    lamp(
        p,
        s,
        Rear,
        &[[0.62, 0.84], [0.8, 0.84], [0.8, 1.0], [0.68, 1.0]],
        lamp_o("tail", 0.3, 1, 1),
    );
    p.add0(
        "rev",
        decal(
            s,
            Rear,
            &[[0.64, 0.76], [0.78, 0.76], [0.78, 0.8], [0.64, 0.8]],
            dd(0.005, 2, 1, true, 1.0),
        ),
    );
    p.add0(
        "trim",
        sweep(
            &[[1.98, 0.3], [2.1, 0.3], [2.11, 0.42], [1.98, 0.44]],
            Axis::X,
            -0.82,
            0.82,
            1,
        ),
    );
    p.add0(
        "trim",
        sweep(
            &[[-1.92, 0.32], [-2.03, 0.32], [-2.04, 0.46], [-1.92, 0.48]],
            Axis::X,
            -0.82,
            0.82,
            1,
        ),
    );
    p.box_("plate", 0.5, 0.12, 0.02, [0.0, 0.62, -1.975], R0, None);
    gap(p, s, Side, &[[0.72, 0.97], [0.7, 0.34]], gap_step(1.0));
    mirrors(
        p,
        hi,
        MirrorOpts {
            w: 0.13,
            h: 0.1,
            d: 0.1,
            base: 0.74,
            ..MirrorOpts::at(0.86, 1.04, 0.58)
        },
    );
    Layout {
        dims: dims(4.02, w, 1.49, r, wb, 1.5),
        wheels: WheelLayout::new(r, 0.21, 1.5, a, 0.62, 0.0, WheelType::Hub),
        head: [0.0, 0.74, 2.04],
        exhausts: vec![[0.45, 0.25, -1.95]],
        siren: None,
    }
}

fn van(p: &mut Parts, hi: bool, _v: Variant) -> Layout {
    let r = 0.34;
    let wb = 3.0;
    let w = 1.95;
    let rr = r + 0.07;
    let a = wb / 2.0;
    let (z0, z1) = (-2.43, 2.6);
    let arches = arches2(a, r, rr);
    let top: [P2; 7] = [
        [-2.43, 1.98],
        [-2.34, 2.07],
        [1.3, 2.08],
        [1.65, 1.97],
        [2.25, 1.18],
        [2.52, 1.02],
        [2.6, 0.9],
    ];
    let bot: [P2; 6] = [
        [-2.43, 0.34],
        [-2.38, 0.3],
        [-1.9, 0.28],
        [1.9, 0.28],
        [2.55, 0.3],
        [2.6, 0.42],
    ];
    let mut body = body_loft(
        hi,
        BodyOpts {
            r_top: 0.12,
            crown: 0.02,
            end_r: 0.16,
            linear: true,
            side: vec![1.25, 1.92],
            extra_z: vec![0.55, 1.66, 2.22, -1.85, -0.2, -1.05, -0.95],
            mat: Some(Box::new(|tag, y, z, _ax| {
                if tag == 0 {
                    return "trim";
                }
                if tag >= 2 && in_range(z, 1.66, 2.22) {
                    return "glass";
                }
                if tag == 1 && in_range(y, 1.25, 1.92) && in_range(z, 0.55, 2.25) {
                    return "glass";
                }
                if tag == 1
                    && in_range(y, 1.25, 1.92)
                    && (in_range(z, -1.85, -1.05) || in_range(z, -0.95, -0.2))
                {
                    return "glass";
                }
                "paint"
            })),
            ..BodyOpts::new(
                z0,
                z1,
                &top,
                &bot,
                &arches,
                Shaper {
                    nose: 0.07,
                    tail: 0.02,
                    end_r: 0.16,
                    tumble: 0.05,
                    y0: 1.2,
                    y1: 2.08,
                    tuck: 0.03,
                    arches: arches.clone(),
                    flare: 0.012,
                    ..Shaper::new(w, z0, z1).taper(0.55)
                }
                .build(),
            )
        },
    );
    p.add_all(geo(&mut body));
    let s = &body.s;
    p.add0(
        "glass",
        decal(
            s,
            Rear,
            &[[-0.76, 1.3], [0.76, 1.3], [0.74, 1.84], [-0.74, 1.84]],
            dd(0.004, 1, 1, false, 1.0),
        ),
    );
    let step1 = |off: f64| RibbonOpts {
        off,
        step: 1.0,
        ..RibbonOpts::default()
    };
    p.add0(
        "trim",
        ribbon(s, Rear, &[[0.0, 1.3], [0.0, 1.84]], 0.03, step1(0.006)),
    );
    p.add0(
        "trim",
        ribbon(s, Rear, &[[0.0, 0.4], [0.0, 1.3]], 0.008, step1(0.003)),
    );
    lamp(
        p,
        s,
        Front,
        &[[0.44, 0.8], [0.84, 0.8], [0.82, 0.96], [0.48, 0.96]],
        lamp_o("head", 0.2, 2, 1),
    );
    p.add0(
        "trim",
        decal(
            s,
            Front,
            &[[-0.38, 0.66], [0.38, 0.66], [0.38, 0.92], [-0.38, 0.92]],
            dd(0.003, 1, 1, false, 1.0),
        ),
    );
    lamp(
        p,
        s,
        Rear,
        &[[0.8, 0.8], [0.92, 0.8], [0.92, 1.22], [0.8, 1.22]],
        lamp_o("tail", 0.3, 1, 1),
    );
    p.add0(
        "rev",
        decal(
            s,
            Rear,
            &[[0.8, 0.66], [0.92, 0.66], [0.92, 0.74], [0.8, 0.74]],
            dd(0.005, 2, 1, true, 1.0),
        ),
    );
    p.add0(
        "trim",
        sweep(
            &[[2.52, 0.3], [2.66, 0.3], [2.67, 0.5], [2.52, 0.52]],
            Axis::X,
            -0.95,
            0.95,
            1,
        ),
    );
    p.add0(
        "trim",
        sweep(
            &[[-2.36, 0.3], [-2.49, 0.3], [-2.5, 0.5], [-2.36, 0.52]],
            Axis::X,
            -0.95,
            0.95,
            1,
        ),
    );
    p.box_("plate", 0.5, 0.12, 0.02, [0.0, 0.64, -2.445], R0, None);
    // Sliding door rail and shut lines.
    gap(p, s, Side, &[[0.5, 1.95], [0.5, 0.36]], gap_step(2.0));
    gap(
        p,
        s,
        Side,
        &[[-0.98, 1.95], [-0.98, 0.36]],
        GapOpts {
            step: 2.0,
            mirror: false,
            ..GapOpts::default()
        },
    );
    p.add0(
        "trim",
        ribbon(
            s,
            Side,
            &[[-0.98, 1.2], [-2.3, 1.2]],
            0.03,
            RibbonOpts {
                off: 0.004,
                step: 2.0,
                ..RibbonOpts::default()
            },
        ),
    );
    mirrors(
        p,
        hi,
        MirrorOpts {
            w: 0.1,
            h: 0.22,
            d: 0.12,
            shell: "trim",
            base: 0.92,
            ..MirrorOpts::at(1.06, 1.46, 1.42)
        },
    );
    Layout {
        dims: dims(5.03, w, 2.08, r, wb, 1.66),
        wheels: WheelLayout::new(r, 0.23, 1.66, a, 0.58, 0.0, WheelType::Steel),
        head: [0.0, 0.86, 2.6],
        exhausts: vec![[0.55, 0.27, -2.42]],
        siren: None,
    }
}

fn pickup(p: &mut Parts, hi: bool, _v: Variant) -> Layout {
    let r = 0.38;
    let wb = 3.3;
    let w = 2.0;
    let rr = r + 0.08;
    let a = wb / 2.0;
    let (z0, z1) = (-2.58, 2.76);
    let arches = arches2(a, r, rr);
    let top: [P2; 7] = [
        [-2.58, 0.72],
        [-0.63, 0.72],
        [-0.6, 1.12],
        [0.9, 1.12],
        [2.5, 1.08],
        [2.72, 0.98],
        [2.76, 0.84],
    ];
    let bot: [P2; 6] = [
        [-2.58, 0.5],
        [-2.54, 0.34],
        [-2.1, 0.32],
        [2.3, 0.32],
        [2.72, 0.36],
        [2.76, 0.5],
    ];
    let mut body = body_loft(
        hi,
        BodyOpts {
            r_top: 0.07,
            crown: 0.02,
            end_r: 0.12,
            linear: true,
            ..BodyOpts::new(
                z0,
                z1,
                &top,
                &bot,
                &arches,
                Shaper {
                    nose: 0.04,
                    tail: 0.01,
                    end_r: 0.12,
                    tumble: 0.03,
                    y0: 0.9,
                    y1: 1.12,
                    tuck: 0.03,
                    arches: arches.clone(),
                    flare: 0.03,
                    flare_w: 0.16,
                    ..Shaper::new(w, z0, z1).taper(0.45)
                }
                .build(),
            )
        },
    );
    p.add_all(geo(&mut body));
    let s = &body.s;
    let mut cabin = cabin_loft(
        hi,
        CabinOpts {
            linear: true,
            b_pillar: Some([-0.08, 0.02]),
            ..CabinOpts::new(
                -0.64,
                0.96,
                &[
                    [-0.64, 1.78],
                    [-0.55, 1.84],
                    [0.25, 1.84],
                    [0.42, 1.8],
                    [0.96, 1.1],
                ],
                Rc::new(|_z| 1.12),
                hw2(|y, _| 0.9 * (1.0 - 0.08 * smooth(1.1, 1.84, y))),
                [-0.64, 0.3],
            )
        },
    );
    p.add_all(geo(&mut cabin));
    // Load bed: sides, tailgate, floor.
    p.pair(
        "paint",
        bx(0.07, 0.42, 1.95),
        w / 2.0 - 0.035,
        0.93,
        -1.6,
        R0,
        None,
    );
    p.box_("paint", w, 0.42, 0.07, [0.0, 0.93, -2.56], R0, None);
    p.box_("trim", w - 0.14, 0.02, 1.9, [0.0, 0.73, -1.6], R0, None);
    p.pair(
        "trim",
        bx(0.09, 0.03, 1.95),
        w / 2.0 - 0.04,
        1.155,
        -1.6,
        R0,
        None,
    );
    // Grille with chrome surround, lamps, bumpers.
    p.add0(
        "chrome",
        decal(
            s,
            Front,
            &[[-0.56, 0.62], [0.56, 0.62], [0.56, 0.98], [-0.56, 0.98]],
            dd(0.003, 1, 1, false, 1.0),
        ),
    );
    p.add0(
        "trim",
        decal(
            s,
            Front,
            &[[-0.5, 0.66], [0.5, 0.66], [0.5, 0.94], [-0.5, 0.94]],
            dd(0.006, 1, 1, false, 1.0),
        ),
    );
    p.box_("chrome", 1.0, 0.03, 0.02, [0.0, 0.8, 2.77], R0, None);
    lamp(
        p,
        s,
        Front,
        &[[0.62, 0.76], [0.92, 0.76], [0.9, 0.96], [0.62, 0.96]],
        lamp_o("head", 0.25, 1, 1),
    );
    p.pair("tail", bx(0.1, 0.3, 0.05), 0.94, 0.95, -2.59, R0, None);
    p.pair("rev", bx(0.08, 0.08, 0.04), 0.94, 0.75, -2.6, R0, None);
    p.add0(
        "chrome",
        sweep(
            &[
                [2.72, 0.36],
                [2.84, 0.36],
                [2.86, 0.46],
                [2.84, 0.56],
                [2.72, 0.56],
            ],
            Axis::X,
            -1.0,
            1.0,
            1,
        ),
    );
    p.add0(
        "chrome",
        sweep(
            &[
                [-2.56, 0.38],
                [-2.68, 0.38],
                [-2.7, 0.47],
                [-2.68, 0.56],
                [-2.56, 0.56],
            ],
            Axis::X,
            -1.0,
            1.0,
            1,
        ),
    );
    p.box_("plate", 0.5, 0.12, 0.02, [0.0, 0.9, -2.6], R0, None);
    gap(p, s, Side, &[[1.0, 1.1], [0.98, 0.4]], gap_step(1.0));
    gap(p, s, Side, &[[-0.6, 1.1], [-0.6, 0.4]], gap_step(1.0));
    mirrors(
        p,
        hi,
        MirrorOpts {
            w: 0.12,
            h: 0.2,
            d: 0.12,
            shell: "trim",
            base: 0.9,
            ..MirrorOpts::at(1.06, 1.38, 0.84)
        },
    );
    Layout {
        dims: dims(5.36, w, 1.84, r, wb, 1.72),
        wheels: WheelLayout::new(r, 0.27, 1.72, a, 0.6, 0.0, WheelType::Steel),
        head: [0.0, 0.86, 2.76],
        exhausts: vec![[0.7, 0.3, -2.5]],
        siren: None,
    }
}

fn boxtruck(p: &mut Parts, hi: bool, _v: Variant) -> Layout {
    let r = 0.48;
    let wb = 4.2;
    let w = 2.3;
    let rr = r + 0.08;
    let a = wb / 2.0;
    // Cab loft (the cargo box is a plain box behind it).
    let (cab_z0, cab_z1) = (1.5, 3.72);
    let mut body = body_loft(
        hi,
        BodyOpts {
            linear: true,
            r_top: 0.12,
            crown: 0.02,
            side: vec![1.7, 2.5],
            extra_z: vec![2.2, 2.87, 3.28],
            end_r: 0.14,
            mat: Some(Box::new(|tag, y, z, _ax| {
                if tag == 0 {
                    return "trim";
                }
                if tag >= 2 && in_range(z, 2.87, 3.28) {
                    return "glass";
                }
                if tag == 1 && in_range(y, 1.7, 2.5) && in_range(z, 2.2, 3.3) {
                    return "glass";
                }
                "paint"
            })),
            ..BodyOpts::new(
                cab_z0,
                cab_z1,
                &[
                    [1.5, 2.62],
                    [2.85, 2.62],
                    [3.3, 1.62],
                    [3.62, 1.47],
                    [3.72, 1.3],
                ],
                &[[1.5, 0.5], [3.72, 0.5]],
                &[[a, r, rr]],
                Shaper {
                    nose: 0.05,
                    tail: 0.0,
                    end_r: 0.14,
                    tumble: 0.03,
                    y0: 1.5,
                    y1: 2.6,
                    tuck: 0.02,
                    ..Shaper::new(w, cab_z0, cab_z1).taper(0.35)
                }
                .build(),
            )
        },
    );
    p.add_all(geo(&mut body));
    let s = &body.s;
    // Cargo box with corner posts and a roll-up door.
    p.box_("cargo", 2.44, 2.5, 5.0, [0.0, 2.12, -0.95], R0, None);
    for sx in [1.0, -1.0] {
        p.box_(
            "trim",
            0.05,
            2.52,
            0.05,
            [sx * 1.215, 2.12, -3.44],
            R0,
            pc(3.0),
        );
    }
    p.box_("trim", 2.46, 0.06, 0.05, [0.0, 3.36, -3.44], R0, pc(3.0));
    for k in 1..6 {
        p.box_(
            "trim",
            2.3,
            0.015,
            0.012,
            [0.0, 0.9 + k as f64 * 0.4, -3.452],
            R0,
            pc(6.0),
        );
    }
    p.box_("trim", 1.0, 0.25, 4.6, [0.0, 0.72, -0.6], R0, None);
    p.box_("trim", 2.3, 0.2, 0.2, [0.0, 0.62, -3.4], R0, None);
    p.pair("trim", bx(0.08, 0.55, 1.3), 1.08, 0.72, -a, R0, None);
    lamp(
        p,
        s,
        Front,
        &[[0.66, 0.92], [1.0, 0.92], [1.0, 1.1], [0.66, 1.1]],
        lamp_o("head", 0.25, 1, 1),
    );
    p.add0(
        "trim",
        decal(
            s,
            Front,
            &[[-0.56, 0.86], [0.56, 0.86], [0.56, 1.34], [-0.56, 1.34]],
            dd(0.003, 1, 1, false, 1.0),
        ),
    );
    p.add0(
        "chrome",
        sweep(
            &[
                [3.66, 0.5],
                [3.8, 0.5],
                [3.82, 0.6],
                [3.8, 0.72],
                [3.66, 0.72],
            ],
            Axis::X,
            -1.15,
            1.15,
            1,
        ),
    );
    p.pair("tail", bx(0.14, 0.26, 0.05), 1.05, 0.92, -3.46, R0, None);
    p.pair("rev", bx(0.1, 0.1, 0.04), 0.82, 0.92, -3.46, R0, None);
    mirrors(
        p,
        hi,
        MirrorOpts {
            w: 0.1,
            h: 0.34,
            d: 0.12,
            shell: "trim",
            base: 1.1,
            ..MirrorOpts::at(1.28, 2.1, 2.9)
        },
    );
    Layout {
        dims: dims(7.2, 2.44, 3.37, r, wb, 1.9),
        wheels: WheelLayout {
            rear_w: Some(0.5),
            ..WheelLayout::new(r, 0.34, 1.9, a, 0.58, 0.0, WheelType::Steel)
        },
        head: [0.0, 1.0, 3.72],
        exhausts: vec![[0.9, 0.4, -3.3]],
        siren: None,
    }
}

fn tractor(p: &mut Parts, hi: bool, _v: Variant) -> Layout {
    let wb = 2.1;
    let a = wb / 2.0;
    let (r_f, r_r) = (0.42, 0.76);
    // Engine block / hood, rounded on top.
    let d = detail(hi);
    let hood_top: Line = {
        let pts = [[-0.35, 1.58], [1.62, 1.52], [1.8, 1.38]];
        Rc::new(move |z| interp(&pts, z))
    };
    let mut hood = loft(LoftOpts {
        zs: stations(-0.35, 1.8, d.cab_uni, &[1.72]),
        top: hood_top,
        bot: Rc::new(|_| 0.82),
        half_w: hw2(|y, _| 0.37 * (1.0 - 0.1 * smooth(1.3, 1.58, y))),
        r_top: 0.14,
        r_bot: 0.02,
        crown: 0.02,
        nb: 0,
        nt: d.nt,
        side: Vec::new(),
        side_n: 0,
        crown_x: Vec::new(),
        n_crown: 1,
        mat: Box::new(|tag, _, _, _| if tag == 0 { "trim" } else { "paint" }),
        cap_mat: Some(Box::new(|front| if front { "trim" } else { "paint" })),
        col: None,
    });
    p.add_all(geo(&mut hood));
    for k in 0..5 {
        p.box_(
            "chrome",
            0.6,
            0.02,
            0.02,
            [0.0, 0.92 + k as f64 * 0.1, 1.81],
            R0,
            None,
        );
    }
    p.box_("trim", 0.5, 0.22, 2.6, [0.0, 0.7, 0.1], R0, None); // chassis
    p.box_("trim", 1.3, 0.14, 0.18, [0.0, r_f, a], R0, None); // front axle
    p.box_("trim", 1.5, 0.18, 0.2, [0.0, r_r, -a], R0, None); // rear axle housing
    // Fenders over the rear wheels.
    let n = if hi { 10 } else { 6 };
    let mut fender: Vec<P2> = Vec::new();
    for i in 0..=n {
        let t = 0.12 + (0.76 * i as f64) / n as f64;
        fender.push([
            mp_math::kernel::cos(t * PI) * 0.9,
            mp_math::kernel::sin(t * PI) * 0.9,
        ]);
    }
    for i in (0..fender.len()).rev() {
        fender.push([fender[i][0] * 0.94, fender[i][1] * 0.94]);
    }
    for sx in [1.0, -1.0] {
        p.add(
            "paint",
            extrude(&fender, 0.5, 0.01),
            [sx * 0.78, r_r, -a],
            R0,
            None,
        );
    }
    p.box_("paint", 1.06, 0.06, 0.9, [0.0, 1.2, -a + 0.15], R0, None); // platform
    p.box_("seat", 0.5, 0.1, 0.45, [0.0, 1.46, -a - 0.05], R0, None);
    p.box_(
        "seat",
        0.5,
        0.45,
        0.08,
        [0.0, 1.7, -a - 0.3],
        [-0.2, 0.0, 0.0],
        None,
    );
    p.cyl(
        "trim",
        0.03,
        0.03,
        0.7,
        6.0,
        [0.0, 1.62, -0.35],
        [0.6, 0.0, 0.0],
        None,
    );
    p.add(
        "trim",
        torus_geometry(0.19, 0.022, 5.0, if hi { 16.0 } else { 10.0 }, PI * 2.0),
        [0.0, 1.9, -0.55],
        [-0.95, 0.0, 0.0],
        None,
    );
    p.cyl("trim", 0.05, 0.05, 1.0, 8.0, [0.22, 1.95, 1.25], R0, None);
    p.cyl("trim", 0.065, 0.05, 0.12, 8.0, [0.22, 2.48, 1.25], R0, None);
    p.pair("head", bx(0.12, 0.1, 0.06), 0.28, 1.36, 1.79, R0, None);
    p.pair("tail", bx(0.08, 0.08, 0.04), 0.95, 1.5, -a - 0.62, R0, None);
    Layout {
        dims: dims(3.4, 1.95, 2.54, r_r, wb, 1.5),
        wheels: WheelLayout {
            front: Some(FrontWheels {
                r: r_f,
                w: 0.2,
                track: 1.3,
            }),
            rim_mat: Some(RimMat::Tractor),
            ..WheelLayout::new(r_r, 0.42, 1.5, a, 0.55, 0.0, WheelType::Steel)
        },
        head: [0.0, 1.36, 1.84],
        exhausts: vec![[0.22, 2.54, 1.25]],
        siren: None,
    }
}

// Patrol sedan: a full-size four-door between the traffic sedan and the GT
// (lower roof, smoother nose), black with white doors and roof, a push bar
// and a roof lightbar.
fn police(p: &mut Parts, hi: bool, _v: Variant) -> Layout {
    let white = if hi { "stripe" } else { "plate" };
    let r = 0.34;
    let wb = 2.9;
    let w = 1.9;
    let rr = r + 0.065;
    let a = wb / 2.0;
    let (z0, z1) = (-2.42, 2.52);
    let arches = arches2(a, r, rr);
    let top: [P2; 9] = [
        [-2.42, 0.86],
        [-2.34, 0.97],
        [-2.12, 1.0],
        [-1.2, 1.01],
        [0.8, 0.99],
        [1.7, 0.94],
        [2.3, 0.84],
        [2.46, 0.74],
        [2.52, 0.62],
    ];
    let bot: [P2; 6] = [
        [-2.42, 0.48],
        [-2.36, 0.28],
        [-1.95, 0.24],
        [1.95, 0.24],
        [2.42, 0.28],
        [2.52, 0.4],
    ];
    let top_f = spline(&top);
    let cab = (-1.45, 0.9);
    let roof = [-0.95, 0.12];
    let doors = [-1.32, 0.86];
    let bp = [-0.42, -0.34];
    let mut body = body_loft(
        hi,
        BodyOpts {
            r_top: 0.11,
            end_r: 0.22,
            side: vec![0.34],
            extra_z: doors.to_vec(),
            mat: Some(Box::new(move |tag, y, z, ax| {
                if tag == 0 {
                    return "trim";
                }
                if hi && tag == 3 && in_range(z, cab.0 + 0.12, cab.1 - 0.12) && ax < 0.66 {
                    return "trim";
                }
                if tag != 3 && in_range(z, doors[0], doors[1]) && y > 0.34 {
                    return white;
                }
                "paint"
            })),
            ..BodyOpts::new(
                z0,
                z1,
                &top,
                &bot,
                &arches,
                Shaper {
                    nose: 0.08,
                    tail: 0.05,
                    nose_len: 0.8,
                    tail_len: 0.6,
                    end_r: 0.22,
                    tumble: 0.06,
                    y0: 0.7,
                    y1: 0.98,
                    tuck: 0.04,
                    hips: 0.02,
                    hip_z: -a,
                    hip_w: 0.6,
                    hip_y: [0.5, 0.85],
                    crease: Some(Crease {
                        y: 0.72,
                        d: 0.012,
                        h: 0.05,
                    }),
                    arches: arches.clone(),
                    flare: 0.012,
                    ..Shaper::new(w, z0, z1)
                }
                .build(),
            )
        },
    );
    p.add_all(geo(&mut body));
    let s = &body.s;
    let mut cabin = cabin_loft(
        hi,
        CabinOpts {
            b_pillar: Some(bp),
            mat: Some(Box::new(move |tag, _y, z, _ax| {
                if tag >= 2 && in_range(z, roof[0], roof[1]) {
                    return white;
                }
                if tag == 2 {
                    return "paint";
                }
                if tag == 1 && in_range(z, bp[0], bp[1]) {
                    return "trim";
                }
                "glass"
            })),
            ..CabinOpts::new(
                cab.0,
                cab.1,
                &[
                    [-1.45, 1.0],
                    [-1.05, 1.36],
                    [-0.8, 1.435],
                    [-0.1, 1.445],
                    [0.18, 1.4],
                    [0.9, 0.98],
                ],
                top_f.clone(),
                hw2(|y, z| {
                    0.8 * (1.0 - 0.16 * smooth(0.98, 1.44, y))
                        * (1.0 - 0.04 * smooth(-0.6, -1.45, z))
                }),
                roof,
            )
        },
    );
    p.add_all(geo(&mut cabin));
    let c = &cabin.s;
    if hi {
        wheel_wells(p, &[a, -a], r, rr, w, hi);
    }
    // Front: headlights, black grille, bumper and the push bar.
    lamp(
        p,
        s,
        Front,
        &[[0.4, 0.62], [0.8, 0.6], [0.8, 0.7], [0.44, 0.72]],
        lamp_o("head", 0.15, if hi { 6 } else { 2 }, if hi { 3 } else { 1 }),
    );
    if hi {
        glow(
            p,
            s,
            Front,
            &[[0.44, 0.7], [0.78, 0.68]],
            0.014,
            "head",
            GlowOpts::default(),
        );
        dot(p, s, Front, 0.54, 0.66, 0.035, "head", dot_c(0.9));
        dot(p, s, Front, 0.68, 0.655, 0.035, "head", dot_c(0.9));
    }
    p.add0(
        "trim",
        decal(
            s,
            Front,
            &[[-0.36, 0.5], [0.36, 0.5], [0.38, 0.66], [-0.38, 0.66]],
            dd(0.003, if hi { 4 } else { 1 }, 1, false, 0.6),
        ),
    );
    p.add0(
        "trim",
        sweep(
            &[[2.44, 0.26], [2.58, 0.26], [2.59, 0.4], [2.44, 0.42]],
            Axis::X,
            -0.9,
            0.9,
            1,
        ),
    );
    push_bar(p, hi, 2.66, 0.3, 0.86, 0.34);
    // Rear: lamps, reverse, bumper, plate.
    lamp(
        p,
        s,
        Rear,
        &[[0.42, 0.78], [0.84, 0.78], [0.84, 0.9], [0.46, 0.9]],
        lamp_o("tail", 0.3, if hi { 4 } else { 2 }, 1),
    );
    if hi {
        glow(
            p,
            s,
            Rear,
            &[[0.46, 0.84], [0.82, 0.84]],
            0.02,
            "tail",
            GlowOpts::default(),
        );
    }
    p.add0(
        "rev",
        decal(
            s,
            Rear,
            &[[0.3, 0.8], [0.4, 0.8], [0.4, 0.88], [0.3, 0.88]],
            dd(0.005, 2, 1, true, 1.0),
        ),
    );
    p.add0(
        "trim",
        sweep(
            &[[-2.34, 0.3], [-2.48, 0.3], [-2.49, 0.46], [-2.34, 0.48]],
            Axis::X,
            -0.9,
            0.9,
            1,
        ),
    );
    p.box_("plate", 0.5, 0.12, 0.02, [0.0, 0.62, -2.43], R0, None);
    // Four doors: shut lines at the white panel edges and the B-pillar.
    let st = gap_step(if hi { 0.05 } else { 1.0 });
    gap(p, s, Side, &[[doors[1], 0.98], [doors[1] - 0.02, 0.36]], st);
    gap(p, s, Side, &[[bp[0] + 0.04, 1.0], [bp[0] + 0.02, 0.36]], st);
    gap(p, s, Side, &[[doors[0], 1.0], [doors[0], 0.8]], st);
    mirrors(
        p,
        hi,
        MirrorOpts {
            w: 0.15,
            h: 0.1,
            d: 0.11,
            base: 0.78,
            ..MirrorOpts::at(0.93, 1.04, 0.72)
        },
    );
    let zb = -0.28;
    let yb = c.top_y(zb, 0.56) - 0.004;
    let siren = lightbar(p, hi, yb, zb, 1.24, 0.3);
    if hi {
        gap1(p, s, Top, &[[-0.8, -2.3], [0.8, -2.3]]);
        gap0(p, s, Top, &[[0.62, 1.0], [0.6, 2.0], [0.44, 2.38]]);
        for z in [0.28, -0.9] {
            p.add0(
                "chrome",
                ribbon(
                    s,
                    Side,
                    &[[z, 0.8], [z - 0.14, 0.8]],
                    0.026,
                    ro(0.004, 0.8, true),
                ),
            );
        }
        number(p, s, Side, "POLICE", -0.24, 0.62, 0.14);
        dlo(
            p,
            c,
            &top_f,
            0.8,
            -1.38,
            DloOpts {
                rear: false,
                ..DloOpts::default()
            },
        );
        interior(
            p,
            InteriorOpts {
                seat: Seat::Rgb([1.4, 1.4, 1.5]),
                ..InteriorOpts::new(-0.1, 1.33, 0.62, 1.02)
            },
        );
        // Cage partition behind the front seats, pillar spotlight,
        // antennas.
        p.add(
            "trim",
            crate::three_geom::box_geometry(1.4, 0.4, 0.02, 1.0, 1.0, 1.0),
            [0.0, 1.18, -0.42],
            R0,
            pc(0.5),
        );
        p.add(
            "chrome",
            crate::three_geom::cylinder_geometry(
                0.055,
                0.045,
                0.14,
                12.0,
                1.0,
                false,
                0.0,
                PI * 2.0,
            ),
            [0.86, 1.1, 0.74],
            [PI / 2.0, 0.0, 0.0],
            None,
        );
        p.add(
            "chrome",
            crate::three_geom::cylinder_geometry(
                0.012,
                0.012,
                0.18,
                6.0,
                1.0,
                false,
                0.0,
                PI * 2.0,
            ),
            [0.82, 1.08, 0.68],
            R0,
            None,
        );
        for x in [0.25, -0.25] {
            p.add(
                "trim",
                crate::three_geom::cylinder_geometry(
                    0.004,
                    0.006,
                    0.55,
                    4.0,
                    1.0,
                    false,
                    0.0,
                    PI * 2.0,
                ),
                [x, 1.26, -2.0],
                R0,
                None,
            );
        }
    }
    Layout {
        dims: dims(5.1, w, 1.59, r, wb, 1.6),
        wheels: WheelLayout {
            rim_mat: Some(RimMat::Dark),
            ..WheelLayout::new(r, 0.245, 1.6, a, 0.6, 0.0, WheelType::Steel)
        },
        head: [0.0, 0.66, 2.5],
        exhausts: vec![[0.5, 0.26, -2.4]],
        siren: Some(siren),
    }
}

// Police SUV: a tall two-box body with a near-vertical tail, black with
// white doors and roof, a heavy push bar, roof rails and a lightbar.
fn police_suv(p: &mut Parts, hi: bool, _v: Variant) -> Layout {
    let white = if hi { "stripe" } else { "plate" };
    let r = 0.39;
    let wb = 2.95;
    let w = 2.0;
    let rr = r + 0.08;
    let a = wb / 2.0;
    let (z0, z1) = (-2.42, 2.5);
    let arches = arches2(a, r, rr);
    let top: [P2; 6] = [
        [-2.42, 1.12],
        [-2.38, 1.16],
        [1.4, 1.17],
        [2.26, 1.12],
        [2.44, 1.04],
        [2.5, 0.9],
    ];
    let bot: [P2; 6] = [
        [-2.42, 0.56],
        [-2.36, 0.42],
        [-1.95, 0.4],
        [1.95, 0.4],
        [2.44, 0.44],
        [2.5, 0.56],
    ];
    let top_f = linear(&top);
    let doors = [-1.28, 1.02];
    let bp = [-0.2, -0.1];
    let cp = [-1.36, -1.24];
    let roof = [-2.24, 0.52];
    let mut body = body_loft(
        hi,
        BodyOpts {
            r_top: 0.08,
            crown: 0.02,
            end_r: 0.14,
            linear: true,
            side: vec![0.48],
            extra_z: doors.to_vec(),
            mat: Some(Box::new(move |tag, y, z, ax| {
                if tag == 0 {
                    return "trim";
                }
                if hi && tag == 3 && in_range(z, -2.3, 1.35) && ax < 0.84 {
                    return "trim";
                }
                if tag != 3 && in_range(z, doors[0], doors[1]) && y > 0.48 {
                    return white;
                }
                "paint"
            })),
            ..BodyOpts::new(
                z0,
                z1,
                &top,
                &bot,
                &arches,
                Shaper {
                    nose: 0.04,
                    tail: 0.02,
                    end_r: 0.14,
                    tumble: 0.03,
                    y0: 0.95,
                    y1: 1.17,
                    tuck: 0.03,
                    tuck_y: [0.4, 0.6],
                    arches: arches.clone(),
                    flare: 0.03,
                    flare_w: 0.18,
                    ..Shaper::new(w, z0, z1).taper(0.45)
                }
                .build(),
            )
        },
    );
    p.add_all(geo(&mut body));
    let s = &body.s;
    let mut cabin = cabin_loft(
        hi,
        CabinOpts {
            linear: true,
            b_pillar: Some(bp),
            r_top: 0.07,
            mat: Some(Box::new(move |tag, _y, z, _ax| {
                if tag >= 2 && in_range(z, roof[0], roof[1]) {
                    return white;
                }
                if tag >= 2 {
                    return if tag == 2 || z < roof[0] {
                        "paint"
                    } else {
                        "glass"
                    };
                }
                if in_range(z, bp[0], bp[1]) || in_range(z, cp[0], cp[1]) {
                    return "trim";
                }
                if z < -2.3 {
                    return "paint";
                }
                "glass"
            })),
            ..CabinOpts::new(
                -2.4,
                1.45,
                &[
                    [-2.4, 1.16],
                    [-2.36, 1.86],
                    [-2.24, 1.93],
                    [0.45, 1.94],
                    [0.64, 1.9],
                    [1.45, 1.17],
                ],
                top_f.clone(),
                hw2(|y, _| 0.93 * (1.0 - 0.09 * smooth(1.15, 1.93, y))),
                roof,
            )
        },
    );
    p.add_all(geo(&mut cabin));
    let c = &cabin.s;
    if hi {
        wheel_wells(p, &[a, -a], r, rr, w, hi);
    }
    // Front: wide black grille between the lamps, bumper, big push bar.
    lamp(
        p,
        s,
        Front,
        &[[0.52, 0.86], [0.86, 0.84], [0.86, 0.98], [0.54, 1.0]],
        lamp_o("head", 0.15, if hi { 5 } else { 2 }, if hi { 3 } else { 1 }),
    );
    if hi {
        glow(
            p,
            s,
            Front,
            &[[0.55, 0.98], [0.84, 0.96]],
            0.016,
            "head",
            GlowOpts::default(),
        );
        dot(p, s, Front, 0.64, 0.92, 0.04, "head", dot_c(0.9));
        dot(p, s, Front, 0.77, 0.91, 0.04, "head", dot_c(0.9));
    }
    p.add0(
        "trim",
        decal(
            s,
            Front,
            &[[-0.48, 0.66], [0.48, 0.66], [0.48, 1.0], [-0.48, 1.0]],
            dd(0.003, if hi { 4 } else { 1 }, 1, false, 0.6),
        ),
    );
    if hi {
        for y in [0.74, 0.82, 0.9] {
            p.add0(
                "chrome",
                ribbon(
                    s,
                    Front,
                    &[[-0.46, y], [0.46, y]],
                    0.012,
                    ro(0.006, 0.5, false),
                ),
            );
        }
    }
    p.add0(
        "trim",
        sweep(
            &[[2.4, 0.4], [2.58, 0.4], [2.6, 0.6], [2.4, 0.64]],
            Axis::X,
            -1.0,
            1.0,
            1,
        ),
    );
    push_bar(p, hi, 2.68, 0.4, 1.1, 0.42);
    // Rear: tall corner lamps, a black band on the tailgate, bumper, plate.
    lamp(
        p,
        s,
        Rear,
        &[[0.66, 0.78], [0.84, 0.78], [0.84, 1.1], [0.7, 1.1]],
        lamp_o("tail", 0.3, if hi { 2 } else { 1 }, if hi { 3 } else { 1 }),
    );
    if hi {
        glow(
            p,
            s,
            Rear,
            &[[0.76, 0.82], [0.76, 1.07]],
            0.03,
            "tail",
            GlowOpts::default(),
        );
    }
    p.add0(
        "rev",
        decal(
            s,
            Rear,
            &[[0.66, 0.68], [0.84, 0.68], [0.84, 0.74], [0.66, 0.74]],
            dd(0.005, 2, 1, true, 1.0),
        ),
    );
    p.add0(
        "trim",
        decal(
            s,
            Rear,
            &[[-0.56, 0.8], [0.56, 0.8], [0.56, 0.94], [-0.56, 0.94]],
            dd(0.003, 1, 1, false, 1.0),
        ),
    );
    p.add0(
        "trim",
        sweep(
            &[[-2.34, 0.4], [-2.5, 0.4], [-2.51, 0.6], [-2.34, 0.62]],
            Axis::X,
            -1.0,
            1.0,
            1,
        ),
    );
    p.box_("plate", 0.5, 0.12, 0.02, [0.0, 0.87, -2.43], R0, None);
    // Running boards and roof rails.
    p.pair("trim", bx(0.14, 0.04, 1.9), 0.98, 0.44, -0.12, R0, None);
    p.pair("trim", bx(0.04, 0.04, 2.3), 0.76, 1.96, -0.85, R0, None);
    let st = gap_step(if hi { 0.05 } else { 1.0 });
    gap(p, s, Side, &[[doors[1], 1.14], [doors[1] - 0.02, 0.5]], st);
    gap(p, s, Side, &[[bp[0] + 0.04, 1.15], [bp[0] + 0.02, 0.5]], st);
    gap(p, s, Side, &[[doors[0], 1.15], [doors[0], 0.9]], st);
    mirrors(
        p,
        hi,
        MirrorOpts {
            w: 0.14,
            h: 0.18,
            d: 0.12,
            base: 0.9,
            ..MirrorOpts::at(1.06, 1.32, 1.2)
        },
    );
    let zb = -0.02;
    let yb = c.top_y(zb, 0.6) - 0.004;
    let siren = lightbar(p, hi, yb, zb, 1.36, 0.32);
    if hi {
        gap0(p, s, Top, &[[0.7, 1.4], [0.68, 2.36]]);
        gap1(p, s, Rear, &[[-0.84, 0.72], [0.84, 0.72]]);
        for z in [0.52, -0.72] {
            p.add0(
                "chrome",
                ribbon(
                    s,
                    Side,
                    &[[z, 1.0], [z - 0.16, 1.0]],
                    0.03,
                    ro(0.004, 0.8, true),
                ),
            );
        }
        number(p, s, Side, "POLICE", -0.12, 0.8, 0.16);
        dlo(
            p,
            c,
            &top_f,
            1.35,
            -2.28,
            DloOpts {
                rear: false,
                ..DloOpts::default()
            },
        );
        interior(
            p,
            InteriorOpts {
                seat: Seat::Rgb([1.4, 1.4, 1.5]),
                ..InteriorOpts::new(-0.35, 1.8, 0.75, 1.25)
            },
        );
        p.add(
            "trim",
            crate::three_geom::box_geometry(1.6, 0.5, 0.02, 1.0, 1.0, 1.0),
            [0.0, 1.5, -0.65],
            R0,
            pc(0.5),
        );
        p.add(
            "chrome",
            crate::three_geom::cylinder_geometry(0.06, 0.05, 0.15, 12.0, 1.0, false, 0.0, PI * 2.0),
            [0.98, 1.4, 1.2],
            [PI / 2.0, 0.0, 0.0],
            None,
        );
        for x in [0.3, -0.3] {
            p.add(
                "trim",
                crate::three_geom::cylinder_geometry(
                    0.004,
                    0.006,
                    0.5,
                    4.0,
                    1.0,
                    false,
                    0.0,
                    PI * 2.0,
                ),
                [x, 2.2, -1.6],
                R0,
                None,
            );
        }
    }
    Layout {
        dims: dims(5.0, w, 2.1, r, wb, 1.7),
        wheels: WheelLayout {
            rim_mat: Some(RimMat::Dark),
            ..WheelLayout::new(r, 0.27, 1.7, a, 0.6, 0.0, WheelType::Steel)
        },
        head: [0.0, 0.92, 2.5],
        exhausts: vec![[0.6, 0.36, -2.4]],
        siren: Some(siren),
    }
}
