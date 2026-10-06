//! The detail kit, the police kit and the loft presets of `CarModel.js`
//! (`:756-959`, `:1079-1166`): lamps, glows, shut lines, dots, race
//! numbers, mirrors, exhaust tips, wings, interiors, wheel wells, the
//! lightbar, push bar and strobes, and `bodyLoft`, `cabinLoft`, `shaper`,
//! `glassTop` and `dlo`.

use std::rc::Rc;

use mp_math::{js, kernel};
use mp_scene::BufferData;

use super::kit::{
    Arch, Axis, Buckets, Col, DCol, DecalOpts, Greys, Line, Loft, LoftOpts, MatFn, O, P2, P3, PCol,
    PI, Parts, R0, RibbonOpts, Surf, View, WHITE, airfoil, arch_zs, bump, bx, clamp01, decal,
    ellipse, end_zs, expand_poly, extrude, in_range, interp, lathe_x, loft, pc, ribbon, sill,
    smooth, spline, stations, sweep,
};
use crate::three_geom::{
    BufferAttribute, BufferGeometry, box_geometry, circle_geometry, cylinder_geometry,
    sphere_geometry, torus_geometry,
};

// ── detail kit ──────────────────────────────────────────────────────────

/// `lamp`'s options: `{ bucket = 'head', bezel = 'trim', bezelW = 0.012,
/// lens = 0.1, off = 0.004, mirror = true, nu = 6, nv = 3 }`.
#[derive(Clone, Copy)]
pub struct LampOpts {
    pub bucket: &'static str,
    pub bezel: Option<&'static str>,
    pub bezel_w: f64,
    pub lens: f64,
    pub off: f64,
    pub mirror: bool,
    pub nu: usize,
    pub nv: usize,
}

impl Default for LampOpts {
    fn default() -> Self {
        LampOpts {
            bucket: "head",
            bezel: Some("trim"),
            bezel_w: 0.012,
            lens: 0.1,
            off: 0.004,
            mirror: true,
            nu: 6,
            nv: 3,
        }
    }
}

/// `lamp(P, S, view, poly, opts)`: a light unit: a black bezel, a lens (dim
/// emission) and optional lit elements drawn on top (full emission via
/// vertex colour).
pub fn lamp(p: &mut Parts, s: &Surf, view: View, poly: &[P2], o: LampOpts) {
    // Enough columns that the lens follows a rounded corner instead of
    // cutting a chord through it (and vanishing into the body).
    let nu = o.nu.max(4);
    if let Some(bezel) = o.bezel {
        p.add0(
            bezel,
            decal(
                s,
                view,
                &expand_poly(poly, o.bezel_w),
                DecalOpts {
                    off: o.off * 0.55,
                    nu,
                    nv: o.nv,
                    mirror: o.mirror,
                    ..DecalOpts::default()
                },
            ),
        );
    }
    p.add0(
        o.bucket,
        decal(
            s,
            view,
            poly,
            DecalOpts {
                off: o.off,
                nu,
                nv: o.nv,
                mirror: o.mirror,
                col: DCol::C([o.lens; 3]),
            },
        ),
    );
}

/// `glow`'s options: `{ col = 1, off = 0.0065, mirror = true, closed =
/// false, step }` (`step` undefined is ribbon's 0.04).
#[derive(Clone, Copy)]
pub struct GlowOpts {
    pub col: Col,
    pub off: f64,
    pub mirror: bool,
    pub closed: bool,
    pub step: f64,
}

impl Default for GlowOpts {
    fn default() -> Self {
        GlowOpts {
            col: WHITE,
            off: 0.0065,
            mirror: true,
            closed: false,
            step: 0.04,
        }
    }
}

/// `glow(P, S, view, pts, w, bucket, opts)`.
pub fn glow(
    p: &mut Parts,
    s: &Surf,
    view: View,
    pts: &[P2],
    w: f64,
    bucket: &'static str,
    o: GlowOpts,
) {
    p.add0(
        bucket,
        ribbon(
            s,
            view,
            pts,
            w,
            RibbonOpts {
                off: o.off,
                col: o.col,
                mirror: o.mirror,
                closed: o.closed,
                step: o.step,
            },
        ),
    );
}

/// `gap`'s options: `{ w = 0.0065, mirror = true, closed = false, step =
/// 0.05, bucket = 'trim', col = 1 }`.
#[derive(Clone, Copy)]
pub struct GapOpts {
    pub w: f64,
    pub mirror: bool,
    pub closed: bool,
    pub step: f64,
    pub bucket: &'static str,
    pub col: Col,
}

impl Default for GapOpts {
    fn default() -> Self {
        GapOpts {
            w: 0.0065,
            mirror: true,
            closed: false,
            step: 0.05,
            bucket: "trim",
            col: WHITE,
        }
    }
}

/// `gap(P, S, view, pts, opts)`: a panel shut line, a thin dark ribbon
/// just proud of the paint.
pub fn gap(p: &mut Parts, s: &Surf, view: View, pts: &[P2], o: GapOpts) {
    p.add0(
        o.bucket,
        ribbon(
            s,
            view,
            pts,
            o.w,
            RibbonOpts {
                off: 0.0016,
                mirror: o.mirror,
                closed: o.closed,
                step: o.step,
                col: o.col,
            },
        ),
    );
}

/// `gap(P, S, view, pts)` with the defaults.
pub fn gap0(p: &mut Parts, s: &Surf, view: View, pts: &[P2]) {
    gap(p, s, view, pts, GapOpts::default());
}

/// `gap(P, S, view, pts, { mirror: false })`.
pub fn gap1(p: &mut Parts, s: &Surf, view: View, pts: &[P2]) {
    gap(
        p,
        s,
        view,
        pts,
        GapOpts {
            mirror: false,
            ..GapOpts::default()
        },
    );
}

/// `dot`'s options: `{ col = 1, off = 0.007, mirror = true, n = 10 }`.
#[derive(Clone, Copy)]
pub struct DotOpts {
    pub col: Col,
    pub off: f64,
    pub mirror: bool,
    pub n: usize,
}

impl Default for DotOpts {
    fn default() -> Self {
        DotOpts {
            col: WHITE,
            off: 0.007,
            mirror: true,
            n: 10,
        }
    }
}

/// `dot(P, S, view, u, v, r, bucket, opts)`.
#[allow(clippy::too_many_arguments)]
pub fn dot(
    p: &mut Parts,
    s: &Surf,
    view: View,
    u: f64,
    v: f64,
    r: f64,
    bucket: &'static str,
    o: DotOpts,
) {
    p.add0(
        bucket,
        decal(
            s,
            view,
            &ellipse(u, v, r, r, o.n),
            DecalOpts {
                off: o.off,
                nu: 3,
                nv: 3,
                mirror: o.mirror,
                col: DCol::C(o.col),
            },
        ),
    );
}

/// `mirrorGeo(g)`: mirror a triangle-soup geometry across X (keeping it
/// front-facing).
pub fn mirror_geo(mut g: BufferGeometry) -> BufferGeometry {
    for name in ["position", "normal"] {
        let a = f32s(&mut g, name).expect("position and normal");
        let mut i = 0;
        while i < a.len() {
            a[i] = -a[i];
            i += 3;
        }
    }
    for name in ["position", "normal", "color"] {
        let Some(a) = f32s(&mut g, name) else {
            continue;
        };
        let mut t = 0;
        while t < a.len() {
            for e in 0..3 {
                a.swap(t + 3 + e, t + 6 + e);
            }
            t += 9;
        }
    }
    g
}

fn f32s<'a>(g: &'a mut BufferGeometry, name: &str) -> Option<&'a mut Vec<f32>> {
    match &mut g.get_attribute_mut(name)?.array {
        BufferData::F32(a) => Some(a),
        _ => panic!("a Float32Array"),
    }
}

/// `SEG`: seven-segment strokes per character.
fn seg_strokes(ch: char) -> &'static str {
    match ch {
        '0' => "abcdef",
        '1' => "bc",
        '2' => "abged",
        '3' => "abgcd",
        '4' => "fgbc",
        '5' => "afgcd",
        '6' => "afgedc",
        '7' => "abc",
        '8' => "abcdefg",
        '9' => "abcdfg",
        'P' => "abefg",
        'O' => "abcdef",
        'L' => "def",
        'I' => "i",
        'C' => "adef",
        'E' => "adefg",
        _ => "",
    }
}

/// `number(P, S, view, str, cu, cv, h, bucket = 'trim')`: a race number on
/// both flanks from seven-segment strokes. On the side view u runs
/// forward, so on the left flank text reads towards -u; the right flank is
/// laid out the other way round and then mirrored across.
#[allow(clippy::too_many_arguments)]
pub fn number(p: &mut Parts, s: &Surf, view: View, text: &str, cu: f64, cv: f64, h: f64) {
    let bucket = "trim";
    let w = h * 0.5;
    let gap_w = h * 0.22;
    let t = h * 0.14;
    let len = text.chars().count() as f64;
    let total = len * w + (len - 1.0) * gap_w;
    for dir in [-1.0, 1.0] {
        for (i, ch) in text.chars().enumerate() {
            let i = i as f64;
            let left = cu - dir * (total / 2.0 - i * (w + gap_w));
            let right = left + dir * w;
            let top = cv + h / 2.0;
            let mid = cv;
            let bot = cv - h / 2.0;
            for k in seg_strokes(ch).chars() {
                let seg: [P2; 2] = match k {
                    'i' => [[(left + right) / 2.0, top], [(left + right) / 2.0, bot]],
                    'a' => [[left, top], [right, top]],
                    'b' => [[right, top], [right, mid]],
                    'c' => [[right, mid], [right, bot]],
                    'd' => [[left, bot], [right, bot]],
                    'e' => [[left, mid], [left, bot]],
                    'f' => [[left, top], [left, mid]],
                    _ => [[left, mid], [right, mid]],
                };
                let g = ribbon(
                    s,
                    view,
                    &seg,
                    t,
                    RibbonOpts {
                        off: 0.0045,
                        step: 1.0,
                        ..RibbonOpts::default()
                    },
                );
                p.add0(bucket, if dir > 0.0 { mirror_geo(g) } else { g });
            }
        }
    }
}

/// `mirrors`' options: `{ x, y, z, w = 0.19, h = 0.1, d = 0.14, shell =
/// 'paint', base = 0.78 }`.
#[derive(Clone, Copy)]
pub struct MirrorOpts {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
    pub h: f64,
    pub d: f64,
    pub shell: &'static str,
    pub base: f64,
}

impl MirrorOpts {
    pub fn at(x: f64, y: f64, z: f64) -> MirrorOpts {
        MirrorOpts {
            x,
            y,
            z,
            w: 0.19,
            h: 0.1,
            d: 0.14,
            shell: "paint",
            base: 0.78,
        }
    }
}

/// `mirrors(P, hi, opts)`: door mirrors, a stalk plus a rounded pod with
/// the glass facing back.
pub fn mirrors(p: &mut Parts, hi: bool, o: MirrorOpts) {
    let seg = if hi { [12.0, 8.0] } else { [6.0, 4.0] };
    for sx in [1.0, -1.0] {
        let mut pod = sphere_geometry(1.0, seg[0], seg[1], 0.0, PI * 2.0, 0.0, PI);
        pod.scale(o.w / 2.0, o.h / 2.0, o.d / 2.0);
        p.add(o.shell, pod, [sx * o.x, o.y, o.z], R0, None);
        p.add(
            "trim",
            box_geometry(js::max(0.02, o.x - o.base), 0.03, 0.07, 1.0, 1.0, 1.0),
            [sx * (o.x + o.base) / 2.0, o.y - o.h * 0.3, o.z + 0.02],
            R0,
            None,
        );
        if hi {
            let mut gl = circle_geometry(1.0, 12.0, 0.0, PI * 2.0);
            gl.scale(o.w * 0.42, o.h * 0.36, 1.0);
            p.add(
                "chrome",
                gl,
                [sx * o.x, o.y, o.z - o.d * 0.46],
                [0.0, PI, 0.0],
                pc(0.55),
            );
        }
    }
}

/// `tip(P, hi, x, y, z, r, len = 0.14, { mirror = true, oval = 1 })`: an
/// exhaust tip: polished tube, rolled lip and a sooty inside, pointing
/// back.
#[allow(clippy::too_many_arguments)]
pub fn tip(
    p: &mut Parts,
    hi: bool,
    x: f64,
    y: f64,
    z: f64,
    r: f64,
    len: f64,
    mirror: bool,
    oval: f64,
) {
    let seg = if hi { 14.0 } else { 8.0 };
    let parts = [
        lathe_x(&[[r, 0.0], [r, len]], seg, Greys::One(1.0)),
        lathe_x(&[[r, len], [r * 0.8, len + 0.004]], seg, Greys::One(1.3)),
        lathe_x(
            &[[r * 0.8, len + 0.004], [r * 0.8, 0.02]],
            seg,
            Greys::One(0.08),
        ),
    ];
    let sides: &[f64] = if mirror { &[1.0, -1.0] } else { &[1.0] };
    for &sx in sides {
        for g0 in &parts {
            let mut g = g0.clone();
            g.rotate_y(PI / 2.0); // X axis → -Z (pointing back)
            g.scale(1.0, oval, 1.0);
            p.add("chrome", g, [sx * x, y, z], R0, None);
        }
    }
}

/// `wing`'s options.
#[derive(Clone)]
pub struct WingOpts {
    pub span: f64,
    pub chord: f64,
    pub thick: f64,
    pub y: f64,
    pub z: f64,
    pub pitch: f64,
    pub bucket: &'static str,
    pub plate_h: f64,
    pub uprights: Vec<f64>,
    pub base_y: f64,
}

/// `wing(P, hi, opts)`: a rear wing, a cambered carbon/paint airfoil with
/// end plates and uprights (`plates` is always true, `upBucket` always
/// carbon).
pub fn wing(p: &mut Parts, hi: bool, o: WingOpts) {
    let (span, chord, thick, y, z) = (o.span, o.chord, o.thick, o.y, o.z);
    let sec = airfoil(chord, thick, if hi { 8 } else { 4 });
    p.add(
        o.bucket,
        sweep(&sec, Axis::X, -span / 2.0, span / 2.0, 1),
        [0.0, y, z],
        [o.pitch, 0.0, 0.0],
        None,
    );
    let plate_h = o.plate_h;
    let pl = [
        [chord * 0.55, plate_h * 0.25],
        [-chord * 0.6, plate_h * 0.45],
        [-chord * 0.65, -plate_h * 0.55],
        [chord * 0.3, -plate_h * 0.5],
    ];
    p.pair(
        o.bucket,
        || extrude(&pl, 0.014, 0.003),
        span / 2.0 + 0.007,
        y,
        z,
        R0,
        None,
    );
    for &ux in &o.uprights {
        let top = y - thick * chord * 0.3;
        let bot = o.base_y;
        let sh: Vec<P2> = [
            [chord * 0.15, top],
            [-chord * 0.2, top],
            [-chord * 0.12, bot],
            [chord * 0.28, bot],
        ]
        .iter()
        .map(|&[a, b]| [a + z, b])
        .collect();
        p.pair(
            "carbon",
            || extrude(&sh, 0.022, 0.004),
            ux,
            0.0,
            0.0,
            R0,
            None,
        );
    }
}

/// The seats' colour: a grey level or an `[r, g, b]` array.
#[derive(Clone, Copy)]
pub enum Seat {
    Grey(f64),
    Rgb(Col),
}

impl Seat {
    fn pcol(self) -> PCol {
        match self {
            Seat::Grey(v) => pc(v),
            Seat::Rgb(c) => Some(c),
        }
    }
}

/// `interior`'s options: `{ z, top, x = 0.36, dashZ, dashY, seat = 2.2,
/// cage = false, lean = 0.28, wheelR = 0.17 }`.
#[derive(Clone, Copy)]
pub struct InteriorOpts {
    pub z: f64,
    pub top: f64,
    pub x: f64,
    pub dash_z: f64,
    pub dash_y: f64,
    pub seat: Seat,
    pub cage: bool,
    pub lean: f64,
    pub wheel_r: f64,
}

impl InteriorOpts {
    pub fn new(z: f64, top: f64, dash_z: f64, dash_y: f64) -> InteriorOpts {
        InteriorOpts {
            z,
            top,
            x: 0.36,
            dash_z,
            dash_y,
            seat: Seat::Grey(2.2),
            cage: false,
            lean: 0.28,
            wheel_r: 0.17,
        }
    }
}

/// `interior(P, opts)`: the interior glimpsed through the glass: seat
/// backs and headrests (whose top sits at `top`, a hand under the roof),
/// dash and steering wheel. Only the parts above the belt line matter; the
/// rest hides inside the body.
pub fn interior(p: &mut Parts, o: InteriorOpts) {
    let (z, top, x, lean, wheel_r) = (o.z, o.top, o.x, o.lean, o.wheel_r);
    let col = o.seat.pcol();
    let hz = z - 0.2 * kernel::sin(lean);
    for sx in [1.0, -1.0] {
        p.add(
            "trim",
            box_geometry(0.24, 0.16, 0.1, 1.0, 1.0, 1.0),
            [sx * x, top - 0.08, hz - 0.06],
            [-lean, 0.0, 0.0],
            col,
        );
        p.add(
            "trim",
            box_geometry(0.1, 0.05, 0.05, 1.0, 1.0, 1.0),
            [sx * x, top - 0.18, hz - 0.04],
            [-lean, 0.0, 0.0],
            pc(0.5),
        );
        p.add(
            "trim",
            box_geometry(0.44, 0.55, 0.12, 1.0, 1.0, 1.0),
            [sx * x, top - 0.47, z],
            [-lean, 0.0, 0.0],
            col,
        );
        p.pair(
            "trim",
            bx(0.07, 0.4, 0.16),
            sx * x + 0.2,
            top - 0.52,
            z + 0.04,
            [-lean, 0.0, 0.0],
            col,
        );
    }
    p.add(
        "trim",
        box_geometry(1.36, 0.12, 0.4, 1.0, 1.0, 1.0),
        [0.0, o.dash_y, o.dash_z],
        [0.12, 0.0, 0.0],
        pc(1.3),
    );
    p.add(
        "trim",
        torus_geometry(wheel_r, 0.022, 5.0, 16.0, PI * 2.0),
        [x, o.dash_y + 0.02, o.dash_z - 0.3],
        [-0.45, 0.0, 0.0],
        pc(0.7),
    );
    p.add(
        "trim",
        box_geometry(0.03, 0.05, wheel_r * 2.0, 1.0, 1.0, 1.0),
        [x, o.dash_y + 0.02, o.dash_z - 0.3],
        [-0.45 - PI / 2.0, 0.0, 0.0],
        pc(0.7),
    );
    if o.cage {
        p.add(
            "chrome",
            torus_geometry(0.64, 0.022, 5.0, 14.0, PI),
            [0.0, top - 0.64, z - 0.28],
            [0.0, 0.0, 0.0],
            pc(0.4),
        );
        p.pair(
            "chrome",
            || cylinder_geometry(0.02, 0.02, 0.9, 5.0, 1.0, false, 0.0, PI * 2.0),
            0.6,
            top - 0.3,
            z - 0.7,
            [0.9, 0.0, 0.0],
            pc(0.4),
        );
    }
}

/// `wheelWells(P, zs, yc, R, W, hi)`: black wheel-arch liners so the
/// arches read as holes, not see-through.
pub fn wheel_wells(p: &mut Parts, zs: &[f64], yc: f64, r: f64, w: f64, hi: bool) {
    for &z in zs {
        let g = cylinder_geometry(
            r - 0.012,
            r - 0.012,
            w - 0.16,
            if hi { 14.0 } else { 8.0 },
            1.0,
            true,
            PI / 2.0,
            PI,
        );
        p.add("trim", g, [0.0, yc, z], [0.0, 0.0, PI / 2.0], pc(0.5));
    }
}

// ── police kit ──────────────────────────────────────────────────────────

/// `roundRect(cu, cv, hu, hv, r, n = 3)`: a rounded rectangle [u, v]
/// (counter-clockwise), for swept sections.
pub fn round_rect(cu: f64, cv: f64, hu: f64, hv: f64, r: f64, n: usize) -> Vec<P2> {
    let mut out = Vec::new();
    for [sx, sv, a0] in [
        [1.0, 1.0, 0.0],
        [-1.0, 1.0, PI / 2.0],
        [-1.0, -1.0, PI],
        [1.0, -1.0, 1.5 * PI],
    ] {
        for k in 0..=n {
            let a = a0 + (PI / 2.0) * (k as f64 / n as f64);
            out.push([
                cu + sx * (hu - r) + r * kernel::cos(a),
                cv + sv * (hv - r) + r * kernel::sin(a),
            ]);
        }
    }
    out
}

/// A siren glow spot: `{ p, blue, size }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlowSpot {
    pub p: P3,
    pub blue: f64,
    pub size: f64,
}

/// The siren layout: glow spots over each half and the anchor for the
/// game's shared flash light.
#[derive(Clone, Debug, PartialEq)]
pub struct SirenLayout {
    pub anchor: P3,
    pub glows: Vec<GlowSpot>,
}

/// `lightbar(P, hi, { y, z, w = 1.24, d = 0.3, h = 0.1 })`: a roof lightbar
/// sitting on y at z: a black base on feet, 2×4 lens segments (red on the
/// driver's side, +X; blue on the right) in the lightRed and lightBlue
/// buckets, chrome end caps.
pub fn lightbar(p: &mut Parts, hi: bool, y: f64, z: f64, w: f64, d: f64) -> SirenLayout {
    let h = 0.1;
    let yb = y + 0.03;
    let gap_c = 0.07;
    p.box_("trim", w, 0.03, d, [0.0, y + 0.015, z], R0, None);
    let seg = (w / 2.0 - gap_c / 2.0 - 0.03) / 4.0;
    if hi {
        p.pair(
            "trim",
            bx(0.07, 0.06, d * 0.7),
            w * 0.36,
            y - 0.02,
            z,
            R0,
            None,
        );
        let sec = round_rect(z, yb + h / 2.0, d / 2.0 - 0.005, h / 2.0, 0.03, 3);
        for i in 0..4 {
            let x0 = gap_c / 2.0 + i as f64 * seg + 0.004;
            let x1 = x0 + seg - 0.008;
            p.add0("lightRed", sweep(&sec, Axis::X, x0, x1, 1));
            p.add0("lightBlue", sweep(&sec, Axis::X, -x1, -x0, 1));
        }
        // Dividers, centre block and end caps.
        p.add(
            "trim",
            sweep(
                &round_rect(
                    z,
                    yb + h / 2.0 - 0.004,
                    d / 2.0 - 0.012,
                    h / 2.0 - 0.006,
                    0.026,
                    3,
                ),
                Axis::X,
                -w / 2.0 + 0.03,
                w / 2.0 - 0.03,
                1,
            ),
            O,
            R0,
            pc(0.6),
        );
        p.add(
            "trim",
            sweep(
                &round_rect(
                    z,
                    yb + h / 2.0 + 0.003,
                    d / 2.0 - 0.002,
                    h / 2.0 + 0.003,
                    0.03,
                    3,
                ),
                Axis::X,
                -gap_c / 2.0,
                gap_c / 2.0,
                1,
            ),
            O,
            R0,
            pc(0.5),
        );
        for sx in [1.0, -1.0] {
            p.add(
                "chrome",
                sweep(
                    &round_rect(z, yb + h / 2.0, d / 2.0, h / 2.0 + 0.004, 0.035, 3),
                    Axis::X,
                    if sx > 0.0 { w / 2.0 - 0.03 } else { -w / 2.0 },
                    if sx > 0.0 { w / 2.0 } else { -w / 2.0 + 0.03 },
                    1,
                ),
                O,
                R0,
                pc(0.8),
            );
        }
    } else {
        let hw = seg * 2.0;
        p.box_(
            "lightRed",
            hw * 2.0 - 0.01,
            h,
            d - 0.01,
            [gap_c / 2.0 + hw, yb + h / 2.0, z],
            R0,
            None,
        );
        p.box_(
            "lightBlue",
            hw * 2.0 - 0.01,
            h,
            d - 0.01,
            [-gap_c / 2.0 - hw, yb + h / 2.0, z],
            R0,
            None,
        );
        p.box_(
            "trim",
            gap_c + 0.01,
            h + 0.006,
            d,
            [0.0, yb + h / 2.0, z],
            R0,
            None,
        );
    }
    let gy = yb + h * 0.6;
    let gx = gap_c / 2.0 + seg * 2.0;
    SirenLayout {
        anchor: [0.0, yb + h + 0.25, z],
        glows: vec![
            GlowSpot {
                p: [gx, gy, z],
                blue: 0.0,
                size: 2.0,
            },
            GlowSpot {
                p: [-gx, gy, z],
                blue: 1.0,
                size: 2.0,
            },
        ],
    }
}

/// `pushBar(P, hi, { z, y0, y1, x = 0.36, back = 0.14 })`: a push bar in
/// front of the nose: two padded uprights and two cross bars on arms back
/// to the bumper, with a pair of small strobes on the top bar.
pub fn push_bar(p: &mut Parts, hi: bool, z: f64, y0: f64, y1: f64, x: f64) {
    let back = 0.14;
    let my = (y0 + y1) / 2.0;
    let hgt = y1 - y0;
    p.pair("trim", bx(0.07, hgt, 0.06), x, my, z, R0, pc(1.4));
    p.box_(
        "trim",
        2.0 * x + 0.12,
        0.07,
        0.05,
        [0.0, y0 + hgt * 0.3, z - 0.005],
        R0,
        pc(1.4),
    );
    p.box_(
        "trim",
        2.0 * x + 0.02,
        0.05,
        0.05,
        [0.0, y1 - 0.05, z],
        R0,
        pc(1.4),
    );
    p.pair(
        "trim",
        bx(0.05, 0.05, back),
        x,
        y0 + hgt * 0.3,
        z - back / 2.0,
        R0,
        None,
    );
    p.box_(
        "lightRed",
        0.16,
        0.035,
        0.03,
        [x * 0.45, y1 - 0.05, z + 0.03],
        R0,
        None,
    );
    p.box_(
        "lightBlue",
        0.16,
        0.035,
        0.03,
        [-x * 0.45, y1 - 0.05, z + 0.03],
        R0,
        None,
    );
    if hi {
        p.pair(
            "trim",
            bx(0.085, hgt * 0.82, 0.03),
            x,
            my,
            z + 0.04,
            R0,
            pc(0.45),
        ); // rubber pads
        p.pair(
            "trim",
            bx(0.05, 0.05, back),
            x,
            y1 - 0.05,
            z - back / 2.0,
            R0,
            None,
        );
    }
}

/// `strobes(P, S, hi, { grille: [gx0, gx1, gy0, gy1], deck: [dx, dy, dz] })`:
/// slicktop strobes for the interceptor livery: a red/blue pair in the
/// grille and a pair of bars on the rear deck.
pub fn strobes(p: &mut Parts, s: &Surf, hi: bool, grille: [f64; 4], deck: P3) -> SirenLayout {
    let [gx0, gx1, gy0, gy1] = grille;
    let [dx, dy, dz] = deck;
    let rect = |u0: f64, u1: f64| -> Vec<P2> { vec![[u0, gy0], [u1, gy0], [u1, gy1], [u0, gy1]] };
    let o = DecalOpts {
        off: 0.012,
        nu: if hi { 3 } else { 1 },
        nv: 1,
        ..DecalOpts::default()
    };
    p.add0("lightRed", decal(s, View::Front, &rect(gx0, gx1), o));
    p.add0("lightBlue", decal(s, View::Front, &rect(-gx1, -gx0), o));
    let seg = if hi { 3 } else { 1 };
    let sec = round_rect(dz, dy + 0.025, 0.035, 0.025, 0.012, seg);
    p.add0(
        "trim",
        sweep(
            &round_rect(dz, dy + 0.012, 0.045, 0.014, 0.01, seg),
            Axis::X,
            -dx - 0.2,
            dx + 0.2,
            1,
        ),
    );
    p.add0("lightRed", sweep(&sec, Axis::X, dx - 0.18, dx + 0.18, 1));
    p.add0("lightBlue", sweep(&sec, Axis::X, -dx - 0.18, -dx + 0.18, 1));
    let gy = (gy0 + gy1) / 2.0;
    let gz = s.z1 + 0.02;
    let gxm = (gx0 + gx1) / 2.0;
    SirenLayout {
        anchor: [0.0, gy, s.z1 + 0.3],
        glows: vec![
            GlowSpot {
                p: [gxm, gy, gz],
                blue: 0.0,
                size: 0.9,
            },
            GlowSpot {
                p: [-gxm, gy, gz],
                blue: 1.0,
                size: 0.9,
            },
            GlowSpot {
                p: [dx, dy + 0.03, dz],
                blue: 0.0,
                size: 1.3,
            },
            GlowSpot {
                p: [-dx, dy + 0.03, dz],
                blue: 1.0,
                size: 1.3,
            },
        ],
    }
}

// ── loft presets ────────────────────────────────────────────────────────

/// `detail(lod)`: the loft's resolution at each level.
#[derive(Clone, Copy)]
pub struct Detail {
    pub nb: usize,
    pub nt: usize,
    pub side_n: usize,
    pub n_crown: usize,
    pub uni: usize,
    pub arch_n: usize,
    pub cab_uni: usize,
    pub cab_side: usize,
    pub end_n: usize,
}

pub fn detail(hi: bool) -> Detail {
    if hi {
        Detail {
            nb: 2,
            nt: 5,
            side_n: 4,
            n_crown: 4,
            uni: 32,
            arch_n: 8,
            cab_uni: 20,
            cab_side: 2,
            end_n: 5,
        }
    } else {
        Detail {
            nb: 1,
            nt: 2,
            side_n: 1,
            n_crown: 1,
            uni: 8,
            arch_n: 4,
            cab_uni: 6,
            cab_side: 0,
            end_n: 2,
        }
    }
}

/// A half-width function `(y, z) => hw`.
pub type HalfW = Rc<dyn Fn(f64, f64) -> f64>;

/// `bodyLoft`'s options.
pub struct BodyOpts {
    pub z0: f64,
    pub z1: f64,
    pub top: Vec<P2>,
    pub bot: Vec<P2>,
    pub arches: Vec<Arch>,
    pub half_w: HalfW,
    pub r_top: f64,
    pub r_bot: f64,
    pub crown: f64,
    pub crown_x: Vec<f64>,
    pub extra_z: Vec<f64>,
    pub mat: Option<MatFn>,
    pub side: Vec<f64>,
    pub end_r: f64,
    pub linear: bool,
}

impl BodyOpts {
    /// The required fields; the rest at the JS defaults (`rTop = 0.1, rBot =
    /// 0.05, crown = 0.025, extraZ = [], side = [], endR = 0.2, linear =
    /// false`).
    pub fn new(
        z0: f64,
        z1: f64,
        top: &[P2],
        bot: &[P2],
        arches: &[Arch],
        half_w: HalfW,
    ) -> BodyOpts {
        BodyOpts {
            z0,
            z1,
            top: top.to_vec(),
            bot: bot.to_vec(),
            arches: arches.to_vec(),
            half_w,
            r_top: 0.1,
            r_bot: 0.05,
            crown: 0.025,
            crown_x: Vec::new(),
            extra_z: Vec::new(),
            mat: None,
            side: Vec::new(),
            end_r: 0.2,
            linear: false,
        }
    }
}

/// `bodyLoft(lod, opts)`: the standard car body: paint everywhere, black
/// underside/arch liners. The roof/bonnet line is splined through the
/// profile points. (No caller passes its own `col`.)
pub fn body_loft(hi: bool, o: BodyOpts) -> Loft {
    let d = detail(hi);
    let bot_f = sill(&o.bot, &o.arches);
    let top_f = if o.linear {
        linear_line(&o.top)
    } else {
        spline(&o.top)
    };
    let arch_y: Vec<f64> = o
        .arches
        .iter()
        .flat_map(|&[_, yc, r]| [yc + r + 0.02, yc + r + 0.07])
        .collect();
    let mut extra: Vec<f64> = o.top.iter().map(|p| p[0]).collect();
    extra.extend(o.bot.iter().map(|p| p[0]));
    extra.extend(arch_zs(&o.arches, d.arch_n));
    extra.extend(end_zs(o.z0, o.z1, o.end_r, d.end_n));
    extra.extend_from_slice(&o.extra_z);
    let zs = stations(o.z0, o.z1, d.uni, &extra);
    let mut side = o.side.clone();
    if hi {
        side.extend(arch_y);
    }
    loft(LoftOpts {
        zs,
        top: top_f,
        bot: bot_f,
        half_w: o.half_w,
        r_top: o.r_top,
        r_bot: o.r_bot,
        crown: o.crown,
        crown_x: o.crown_x,
        side,
        nb: d.nb,
        nt: d.nt,
        side_n: d.side_n,
        n_crown: d.n_crown,
        mat: o
            .mat
            .unwrap_or_else(|| Box::new(|tag, _, _, _| if tag == 0 { "trim" } else { "paint" })),
        cap_mat: Some(Box::new(|_| "paint")),
        // A touch of ambient occlusion low on the body and under the sills.
        col: Some(Box::new(|_x, y, _z| {
            let v = 0.62 + 0.38 * smooth(0.14, 0.5, y);
            [v, v, v]
        })),
    })
}

fn linear_line(pts: &[P2]) -> Line {
    let pts = pts.to_vec();
    Rc::new(move |z| interp(&pts, z))
}

/// `stripe(ax)`: whether a top face at |x| = ax is in the racing stripe.
pub type Stripe = Option<fn(f64) -> bool>;

/// `cabinLoft`'s options.
pub struct CabinOpts {
    pub z0: f64,
    pub z1: f64,
    pub top: Vec<P2>,
    pub body_top: Line,
    pub half_w: HalfW,
    pub roof: [f64; 2],
    pub b_pillar: Option<[f64; 2]>,
    pub c_pillar: Option<f64>,
    pub pillars: &'static str,
    pub r_top: f64,
    pub crown: f64,
    pub crown_x: Vec<f64>,
    pub stripe: Stripe,
    pub mat: Option<MatFn>,
    pub linear: bool,
}

impl CabinOpts {
    /// The required fields; the rest at the JS defaults (`pillars =
    /// 'paint', rTop = 0.08, crown = 0.02, extraZ = [], linear = false`).
    pub fn new(
        z0: f64,
        z1: f64,
        top: &[P2],
        body_top: Line,
        half_w: HalfW,
        roof: [f64; 2],
    ) -> CabinOpts {
        CabinOpts {
            z0,
            z1,
            top: top.to_vec(),
            body_top,
            half_w,
            roof,
            b_pillar: None,
            c_pillar: None,
            pillars: "paint",
            r_top: 0.08,
            crown: 0.02,
            crown_x: Vec::new(),
            stripe: None,
            mat: None,
            linear: false,
        }
    }
}

/// `cabinLoft(lod, opts)`: the greenhouse sitting on the body. Default:
/// glass sides and screens, painted roof and pillars; `cPillar` paints the
/// rear quarter; `pillars` colours the A-pillars/roof rails ('paint' or
/// 'trim'). (No caller passes `extraZ`.)
pub fn cabin_loft(hi: bool, o: CabinOpts) -> Loft {
    let d = detail(hi);
    let top_f = if o.linear {
        linear_line(&o.top)
    } else {
        spline(&o.top)
    };
    let body_f = o.body_top.clone();
    let bot_f: Line = Rc::new(move |z| body_f(z) - 0.035);
    let mut extra: Vec<f64> = o.top.iter().map(|p| p[0]).collect();
    extra.push(o.roof[0]);
    extra.push(o.roof[1]);
    if let Some(bp) = o.b_pillar {
        extra.extend(bp);
    }
    if let Some(cp) = o.c_pillar {
        extra.push(cp);
    }
    let zs = stations(o.z0, o.z1, d.cab_uni, &extra);
    let (roof, b_pillar, c_pillar, pillars, stripe) =
        (o.roof, o.b_pillar, o.c_pillar, o.pillars, o.stripe);
    let mat: MatFn = o.mat.unwrap_or_else(|| {
        Box::new(move |tag, _y, z, ax| {
            if tag == 2 {
                return if in_range(z, roof[0], roof[1]) {
                    "paint"
                } else {
                    pillars
                };
            }
            if tag == 3 {
                return if in_range(z, roof[0], roof[1]) {
                    if stripe.is_some_and(|f| f(ax)) {
                        "stripe"
                    } else {
                        "paint"
                    }
                } else {
                    "glass"
                };
            }
            if let Some(bp) = b_pillar
                && in_range(z, bp[0], bp[1])
            {
                return "trim";
            }
            if let Some(cp) = c_pillar
                && z < cp
            {
                return "paint";
            }
            "glass"
        })
    });
    loft(LoftOpts {
        zs,
        top: top_f,
        bot: bot_f,
        half_w: o.half_w,
        r_top: o.r_top,
        r_bot: 0.01,
        crown: o.crown,
        crown_x: o.crown_x,
        side: Vec::new(),
        nb: 0,
        nt: d.nt,
        side_n: d.cab_side,
        n_crown: d.n_crown,
        mat,
        cap_mat: Some(Box::new(|_| "glass")),
        col: None,
    })
}

/// `crease: { y, d, h }`: a ridge along the flank (the character line).
#[derive(Clone, Copy)]
pub struct Crease {
    pub y: f64,
    pub d: f64,
    pub h: f64,
}

/// `shaper`'s options (every JS default written out by [`Shaper::new`]).
#[derive(Clone)]
pub struct Shaper {
    pub w: f64,
    pub z0: f64,
    pub z1: f64,
    pub nose: f64,
    pub tail: f64,
    pub nose_len: f64,
    pub tail_len: f64,
    pub end_r: f64,
    pub tumble: f64,
    pub y0: f64,
    pub y1: f64,
    pub tuck: f64,
    pub tuck_y: [f64; 2],
    pub hips: f64,
    pub hip_z: f64,
    pub hip_w: f64,
    pub hip_y: [f64; 2],
    pub crease: Option<Crease>,
    pub arches: Vec<Arch>,
    pub flare: f64,
    pub flare_w: f64,
    pub lip: f64,
}

impl Shaper {
    /// `{ W, z0, z1 }` and the defaults: nose 0.1, tail 0.06, taperLen 0.6
    /// (noseLen and tailLen), endR 0.2, tumble 0.05, y0 0.6, y1 0.9, tuck
    /// 0.04, tuckY [0.12, 0.38], hips 0, hipZ 0, hipW 0.6, hipY [0.3, 0.8],
    /// no crease, no arches, flare 0, flareW 0.22, lip 0.
    pub fn new(w: f64, z0: f64, z1: f64) -> Shaper {
        Shaper {
            w,
            z0,
            z1,
            nose: 0.1,
            tail: 0.06,
            nose_len: 0.6,
            tail_len: 0.6,
            end_r: 0.2,
            tumble: 0.05,
            y0: 0.6,
            y1: 0.9,
            tuck: 0.04,
            tuck_y: [0.12, 0.38],
            hips: 0.0,
            hip_z: 0.0,
            hip_w: 0.6,
            hip_y: [0.3, 0.8],
            crease: None,
            arches: Vec::new(),
            flare: 0.0,
            flare_w: 0.22,
            lip: 0.0,
        }
    }

    /// `taperLen`: both `noseLen` and `tailLen`.
    pub fn taper(mut self, len: f64) -> Shaper {
        self.nose_len = len;
        self.tail_len = len;
        self
    }

    /// `shaper(opts)`: the half-width function shared by the body presets.
    pub fn build(self) -> HalfW {
        Rc::new(move |y, z| self.half_w(y, z))
    }

    fn half_w(&self, y: f64, z: f64) -> f64 {
        let s = self;
        let mut f = 1.0;
        let tn = clamp01((s.z1 - z) / s.nose_len);
        let tt = clamp01((z - s.z0) / s.tail_len);
        f -= s.nose * (1.0 - tn) * (1.0 - tn) + s.tail * (1.0 - tt) * (1.0 - tt);
        f -= s.tumble * smooth(s.y0, s.y1, y);
        f -= s.tuck * (1.0 - smooth(s.tuck_y[0], s.tuck_y[1], y));
        let mut hw = (s.w / 2.0) * f;
        let d = js::min(s.z1 - z, z - s.z0);
        if d < s.end_r {
            hw -= s.end_r
                - f64::sqrt(js::max(
                    0.0,
                    s.end_r * s.end_r - kernel::pow(s.end_r - d, 2.0),
                ));
        }
        if s.hips != 0.0 && !s.hips.is_nan() {
            hw += s.hips
                * bump(z, s.hip_z, s.hip_w)
                * smooth(s.hip_y[0] - 0.2, s.hip_y[0], y)
                * (1.0 - smooth(s.hip_y[1], s.hip_y[1] + 0.15, y));
        }
        if let Some(c) = s.crease {
            hw += c.d * js::max(0.0, 1.0 - (y - c.y).abs() / c.h) * 1.0;
        }
        for &[zc, yc, r] in &s.arches {
            let dist = kernel::hypot(z - zc, y - yc);
            hw += s.flare * (1.0 - smooth(r, r + s.flare_w, dist)) * smooth(yc - r * 0.6, yc, y)
                + s.lip * bump(dist, r + 0.035, 0.045);
        }
        js::max(0.004, hw)
    }
}

/// `glassTop(C, z)`: the side glass's top edge (where the cabin's flank
/// meets its shoulder).
pub fn glass_top(c: &Surf, z: f64) -> f64 {
    let h = c.ring(z);
    for p in h.iter() {
        if p.t == 2 {
            return p.y;
        }
    }
    h[h.len() - 1].y
}

/// `dlo`'s options: `{ w = 0.022, bucket = 'trim', col = 1, rear = true }`.
#[derive(Clone, Copy)]
pub struct DloOpts {
    pub w: f64,
    pub bucket: &'static str,
    pub rear: bool,
}

impl Default for DloOpts {
    fn default() -> Self {
        DloOpts {
            w: 0.022,
            bucket: "trim",
            rear: true,
        }
    }
}

/// `dlo(P, C, bodyF, zA, zB, opts)`: the black window surround, belt line
/// plus the upper frame of the side glass. (No caller passes `col`.)
pub fn dlo(p: &mut Parts, c: &Surf, body_f: &Line, za: f64, zb: f64, o: DloOpts) {
    let n = 14;
    let w = o.w;
    let mut belt = Vec::new();
    let mut upper = Vec::new();
    for i in 0..=n {
        let z = zb + ((za - zb) * i as f64) / n as f64;
        belt.push([z, body_f(z) + w * 0.4]);
        upper.push([z, glass_top(c, z) - w * 0.45]);
    }
    let ro = RibbonOpts {
        off: 0.003,
        mirror: true,
        ..RibbonOpts::default()
    };
    p.add0(o.bucket, ribbon(c, View::Side, &belt, w, ro));
    p.add0(o.bucket, ribbon(c, View::Side, &upper, w, ro));
    if o.rear {
        p.add0(
            o.bucket,
            ribbon(
                c,
                View::Side,
                &[
                    [zb + w * 0.4, body_f(zb) + 0.01],
                    [zb + w * 0.4, glass_top(c, zb) - 0.01],
                ],
                w,
                ro,
            ),
        );
    }
}

/// The buckets of a loft, for `P.addAll`.
pub fn geo(l: &mut Loft) -> Buckets {
    std::mem::take(&mut l.geo)
}

/// An empty geometry with only a position, as `new BufferGeometry()
/// .setAttribute('position', ...)` (the wheels' stand-in group).
pub fn dummy_group() -> BufferGeometry {
    let mut g = BufferGeometry::new();
    g.set_attribute("position", BufferAttribute::from_f64(&[0.0; 9], 3));
    g.set_attribute(
        "normal",
        BufferAttribute::from_f64(&[1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0], 3),
    );
    g
}
