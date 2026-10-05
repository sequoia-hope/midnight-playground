//! `drawMinimap` and `drawPolice` as data: the canvas's transform and what
//! it draws, in the canvas's own pixels (220 × 220, shown at 190 CSS px),
//! for the minimap shader (`hud.wgsl`) to draw. The road is a polyline
//! stroked twice, round caps and joins; the cars and police are dots and
//! bars; the player's arrow is the shader's own, in screen space.

use super::model::HudIn;
use mr_track::track::Track;

/// The canvas: 220 px square.
pub const S: f64 = 220.0;
/// Its centre.
pub const C: f64 = S / 2.0;
/// px per metre.
pub const SCALE: f64 = 0.28;
/// The clip circle's radius (`c - 2`).
pub const CLIP: f64 = C - 2.0;
/// The most road points the shader takes (s − 500 to s + 900 every 6 m is
/// 235, and the runout's end).
pub const MAX_ROAD: usize = 256;
/// The most dots and bars it takes.
pub const MAX_SHAPES: usize = 64;

/// `translate(c, c + 30); rotate(-yaw - π/2); scale(.28); translate(-x, -z)`.
#[derive(Clone, Copy, Debug)]
pub struct Xform {
    px: f64,
    pz: f64,
    cos: f64,
    sin: f64,
}

impl Xform {
    pub fn new(x: f64, z: f64, yaw: f64) -> Xform {
        let a = -yaw - std::f64::consts::FRAC_PI_2;
        Xform {
            px: x,
            pz: z,
            cos: a.cos(),
            sin: a.sin(),
        }
    }

    /// World (x, z) → canvas px.
    pub fn apply(&self, x: f64, z: f64) -> (f64, f64) {
        let dx = (x - self.px) * SCALE;
        let dz = (z - self.pz) * SCALE;
        (
            C + dx * self.cos - dz * self.sin,
            C + 30.0 + dx * self.sin + dz * self.cos,
        )
    }
}

/// The road near the player, in world (x, z): every 6th sample from
/// s − 500 to s + 900, and the straight runout past the last sample.
pub fn road(t: &Track, s: f64) -> Vec<(f64, f64)> {
    let n = t.n as f64;
    let (s0, s1) = if t.is_loop {
        ((s - 500.0).floor(), (s + 900.0).ceil())
    } else {
        (
            (s - 500.0).floor().max(0.0),
            (n - 1.0).min((s + 900.0).ceil()),
        )
    };
    let mut out = Vec::new();
    let mut i = s0;
    while i <= s1 {
        let k = t.idx(i);
        out.push((f64::from(t.px[k]), f64::from(t.pz[k])));
        i += 6.0;
    }
    // The straight runout past the last sample.
    if !t.is_loop && t.runout > 0.0 && s + 900.0 > n - 1.0 {
        let a = t.frame(s0.max(n - 1.0));
        let e = t.frame(t.road_end().min(s + 900.0));
        if s0 > s1 {
            out.push((a.x, a.z));
        }
        out.push((e.x, e.z));
    }
    out
}

/// A dot or a bar, in canvas px, as the shader draws it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// A filled circle with an optional stroke centred on its edge.
    Dot {
        x: f64,
        y: f64,
        r: f64,
        fill: [f32; 4],
        stroke: Option<(f64, [f32; 4])>,
    },
    /// A butt-capped line.
    Bar {
        a: (f64, f64),
        b: (f64, f64),
        w: f64,
        color: [f32; 4],
    },
}

impl Shape {
    /// Whether any of it can show inside the clip circle.
    fn visible(&self) -> bool {
        let near = |x: f64, y: f64, r: f64| (x - C).hypot(y - C) <= CLIP + r + 1.0;
        match *self {
            Shape::Dot {
                x, y, r, stroke, ..
            } => near(x, y, r + stroke.map_or(0.0, |s| s.0 / 2.0)),
            Shape::Bar { a, b, w, .. } => {
                let m = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
                let half = (a.0 - b.0).hypot(a.1 - b.1) / 2.0;
                near(m.0, m.1, half + w)
            }
        }
    }
}

/// `'#rrggbb'` or a colour number → sRGB with alpha.
pub fn rgb(c: u32, a: f32) -> [f32; 4] {
    [
        ((c >> 16) & 255) as f32 / 255.0,
        ((c >> 8) & 255) as f32 / 255.0,
        (c & 255) as f32 / 255.0,
        a,
    ]
}

const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const COP_RED: u32 = 0xff3040;
const COP_BLUE: u32 = 0x2f6bff;

/// What the minimap draws this frame, in canvas px.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    pub road: Vec<(f64, f64)>,
    pub shapes: Vec<Shape>,
}

/// `drawMinimap(st)` (and `drawPolice`), `clock` the HUD's.
pub fn scene(t: &Track, st: &HudIn, clock: f64) -> Scene {
    let (px, pz, yaw) = st.player;
    let xf = Xform::new(px, pz, yaw);
    let mut road: Vec<(f64, f64)> = road(t, st.s)
        .into_iter()
        .map(|(x, z)| xf.apply(x, z))
        .collect();
    road.truncate(MAX_ROAD);
    let mut shapes = Vec::new();
    let dot = |x: f64, z: f64, r: f64, fill, stroke: Option<(f64, [f32; 4])>| {
        let (x, y) = xf.apply(x, z);
        Shape::Dot {
            x,
            y,
            r: r * SCALE,
            fill,
            stroke: stroke.map(|(w, c)| (w * SCALE, c)),
        }
    };
    // Finish marker.
    if !t.is_loop {
        let fi = t.idx(t.finish_s);
        shapes.push(dot(
            f64::from(t.px[fi]),
            f64::from(t.pz[fi]),
            22.0,
            rgb(0x4dff8a, 1.0),
            None,
        ));
    }
    // Traffic.
    for o in &st.traffic {
        shapes.push(dot(
            o.x,
            o.z,
            10.0,
            [180.0 / 255.0, 190.0 / 255.0, 210.0 / 255.0, 0.8],
            None,
        ));
    }
    // Rivals.
    for r in &st.rivals {
        shapes.push(dot(r.x, r.z, 16.0, rgb(r.color, 1.0), Some((4.0, BLACK))));
    }
    if let Some(pu) = &st.pursuit {
        // Roadblocks as red bars across the road, spikes as thin amber
        // ones, drawn wider and thicker than life so they read on a 190 px
        // map.
        for (list, w, col) in [(&pu.roadblocks, 18.0, COP_RED), (&pu.spikes, 9.0, 0xffb43c)] {
            for b in list.iter() {
                let half = b.width.max(36.0) / 2.0;
                let (ax, az) = (-b.yaw.sin() * half, b.yaw.cos() * half);
                let a = xf.apply(b.x - ax, b.z - az);
                let e = xf.apply(b.x + ax, b.z + az);
                shapes.push(Shape::Bar {
                    a,
                    b: e,
                    w: (w + 6.0) * SCALE,
                    color: BLACK,
                });
                shapes.push(Shape::Bar {
                    a,
                    b: e,
                    w: w * SCALE,
                    color: rgb(col, 1.0),
                });
            }
        }
        // Units as red/blue dots that blink unless flashing is off.
        let phase = if pu.flash {
            ((clock * 4.0).floor() as i64).rem_euclid(2) as usize
        } else {
            0
        };
        for (i, u) in pu.units.iter().enumerate() {
            if u.disabled {
                shapes.push(dot(
                    u.x,
                    u.z,
                    16.0,
                    [120.0 / 255.0, 126.0 / 255.0, 140.0 / 255.0, 0.6],
                    None,
                ));
                continue;
            }
            let red = (i + phase) % 2 == 0;
            let fill = if red { COP_RED } else { COP_BLUE };
            let stroke = if pu.flash {
                BLACK
            } else {
                rgb(if red { COP_BLUE } else { COP_RED }, 1.0)
            };
            shapes.push(dot(u.x, u.z, 16.0, rgb(fill, 1.0), Some((5.0, stroke))));
        }
    }
    shapes.retain(Shape::visible);
    shapes.truncate(MAX_SHAPES);
    Scene { road, shapes }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::play::hud::model::{Dot, HudIn, PursuitIn, RivalDot, Unit};

    /// A straight road along +x, sample i at (i, 0), one zone named SIERRA
    /// PASS: Sierra's track with its arrays replaced.
    pub(crate) fn straight_track(n: usize) -> Track {
        use std::sync::OnceLock;
        static SIERRA: OnceLock<Track> = OnceLock::new();
        let mut t = SIERRA
            .get_or_init(|| Track::new(&mr_levels::level_by_id("sierra")).expect("track"))
            .clone();
        t.n = n;
        t.is_loop = false;
        t.laps = 0;
        t.runout = 0.0;
        t.length = n as f64 - 1.0;
        let z = |_| 0f32;
        t.px = (0..n).map(|i| i as f32).collect();
        t.pz = (0..n).map(z).collect();
        t.py = (0..n).map(z).collect();
        t.fx = vec![1.0; n];
        t.fz = vec![0.0; n];
        for a in [
            &mut t.hw,
            &mut t.bank,
            &mut t.grade,
            &mut t.k_smooth,
            &mut t.wall_l,
            &mut t.wall_r,
        ] {
            *a = vec![0.0; n];
        }
        t.zone = vec![0; n];
        let mut zone = t.zones[0].clone();
        zone.zone.name = "SIERRA PASS";
        zone.zone.sub = "x";
        zone.s0 = 0.0;
        zone.s1 = n as f64;
        t.zones = vec![zone];
        t.finish_s = n as f64 - 100.0;
        t
    }

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
    }

    // The player sits 30 px below the centre and the road ahead points up,
    // whatever the heading; 0.28 px per metre.
    #[test]
    fn the_projection_turns_the_world_to_the_heading() {
        for yaw in [0.0, 0.7, -2.0, std::f64::consts::PI] {
            let xf = Xform::new(100.0, -50.0, yaw);
            assert!(close(xf.apply(100.0, -50.0), (110.0, 140.0)));
            // 100 m ahead: 28 px straight up.
            let ahead = (100.0 + 100.0 * yaw.cos(), -50.0 + 100.0 * yaw.sin());
            assert!(close(xf.apply(ahead.0, ahead.1), (110.0, 112.0)), "{yaw}");
            // 50 m to the right (the vehicle's right, -sin, cos... rotated a
            // quarter turn clockwise on screen): 14 px right.
            let right = (100.0 - 50.0 * yaw.sin(), -50.0 + 50.0 * yaw.cos());
            assert!(close(xf.apply(right.0, right.1), (124.0, 140.0)), "{yaw}");
        }
    }

    #[test]
    fn the_road_from_500_m_behind_to_900_m_ahead_every_6_m() {
        let t = straight_track(3000);
        let r = road(&t, 1000.0);
        assert_eq!(r.first(), Some(&(500.0, 0.0)));
        assert_eq!(r.len(), 234); // 500, 506, …, 1898
        assert_eq!(r.last(), Some(&(1898.0, 0.0)));
        // Clipped at the ends of a point-to-point road.
        let r = road(&t, 100.0);
        assert_eq!(r[0], (0.0, 0.0));
        assert_eq!(r.len(), 167); // 0..=996
        let r = road(&t, 2500.0);
        assert_eq!(r.last(), Some(&(2996.0, 0.0)));
    }

    #[test]
    fn the_runout_carries_the_road_on_past_the_last_sample() {
        let mut t = straight_track(1000);
        t.runout = 300.0;
        let r = road(&t, 700.0);
        // ... 998 (the last sample in the 6 m steps), then the runout's end
        // at min(roadEnd, s + 900).
        assert_eq!(r[r.len() - 2], (998.0, 0.0));
        let e = r[r.len() - 1];
        assert!((e.0 - 1299.0).abs() < 1e-9 && e.1 == 0.0, "{e:?}");
        // Past the last sample: only the runout, from frame(s − 500) (on
        // the runout itself, as `Math.max(s0, t.n - 1)` gives) to its end.
        let r = road(&t, 1600.0);
        assert_eq!(r.len(), 2);
        assert!((r[0].0 - 1100.0).abs() < 1e-9 && (r[1].0 - 1299.0).abs() < 1e-9);
    }

    #[test]
    fn loops_wrap_round() {
        let mut t = straight_track(1000);
        t.is_loop = true;
        let r = road(&t, 100.0);
        assert_eq!(r[0], (600.0, 0.0)); // idx(-400)
        assert_eq!(r.len(), 234);
    }

    fn st() -> HudIn {
        HudIn {
            s: 100.0,
            player: (100.0, 0.0, 0.0),
            ..HudIn::default()
        }
    }

    #[test]
    fn what_it_draws_in_order() {
        let t = straight_track(1000);
        let mut s = st();
        s.traffic = vec![Dot { x: 150.0, z: 0.0 }, Dot { x: 5000.0, z: 0.0 }];
        s.rivals = vec![RivalDot {
            x: 120.0,
            z: 10.0,
            color: 0xff8800,
        }];
        let sc = scene(&t, &s, 0.0);
        // The finish (900 m ahead) is off the map; the far traffic too.
        assert_eq!(sc.shapes.len(), 2);
        match sc.shapes[0] {
            Shape::Dot {
                x,
                y,
                r,
                fill,
                stroke,
            } => {
                // 50 m ahead along +x with yaw 0: up.
                assert!(close((x, y), (110.0, 126.0)));
                assert!((r - 2.8).abs() < 1e-12);
                assert_eq!(fill[3], 0.8);
                assert!(stroke.is_none());
            }
            _ => panic!(),
        }
        match sc.shapes[1] {
            Shape::Dot {
                r, fill, stroke, ..
            } => {
                assert!((r - 16.0 * 0.28).abs() < 1e-12);
                assert_eq!(fill, rgb(0xff8800, 1.0));
                let (w, c) = stroke.unwrap();
                assert!((w - 1.12).abs() < 1e-12);
                assert_eq!(c, [0.0, 0.0, 0.0, 1.0]);
            }
            _ => panic!(),
        }
        // Near the finish it shows, first.
        s.s = 850.0;
        s.player = (850.0, 0.0, 0.0);
        let sc = scene(&t, &s, 0.0);
        assert!(matches!(sc.shapes[0], Shape::Dot { fill, .. } if fill == rgb(0x4dff8a, 1.0)));
    }

    #[test]
    fn police_bars_and_blinking_units() {
        let t = straight_track(1000);
        let mut s = st();
        s.pursuit = Some(PursuitIn {
            units: vec![
                Unit {
                    x: 110.0,
                    z: 0.0,
                    disabled: false,
                },
                Unit {
                    x: 115.0,
                    z: 0.0,
                    disabled: false,
                },
                Unit {
                    x: 120.0,
                    z: 0.0,
                    disabled: true,
                },
            ],
            roadblocks: vec![crate::play::hud::model::Bar {
                x: 200.0,
                z: 0.0,
                yaw: 0.0,
                width: 16.0,
            }],
            ..PursuitIn::default()
        });
        let fills = |sc: &Scene| -> Vec<[f32; 4]> {
            sc.shapes
                .iter()
                .filter_map(|s| match s {
                    Shape::Dot { fill, .. } => Some(*fill),
                    _ => None,
                })
                .collect()
        };
        let sc = scene(&t, &s, 0.1);
        // The roadblock: black under red, 36 m wide at least, across the
        // road (yaw 0: along z).
        match (sc.shapes[0], sc.shapes[1]) {
            (
                Shape::Bar { a, b, w, color },
                Shape::Bar {
                    w: w2, color: c2, ..
                },
            ) => {
                assert_eq!(color, BLACK);
                assert!((w - 24.0 * 0.28).abs() < 1e-12 && (w2 - 18.0 * 0.28).abs() < 1e-12);
                assert_eq!(c2, rgb(COP_RED, 1.0));
                assert!(((a.0 - b.0).hypot(a.1 - b.1) - 36.0 * 0.28).abs() < 1e-9);
            }
            _ => panic!(),
        }
        let f = fills(&sc);
        assert_eq!(f[0], rgb(COP_RED, 1.0));
        assert_eq!(f[1], rgb(COP_BLUE, 1.0));
        assert_eq!(f[2][3], 0.6);
        // A quarter second later they swap.
        let f = fills(&scene(&t, &s, 0.3));
        assert_eq!(f[0], rgb(COP_BLUE, 1.0));
        // Flash off: no blinking, strokes in the other colour.
        s.pursuit.as_mut().unwrap().flash = false;
        let sc = scene(&t, &s, 0.3);
        assert_eq!(fills(&sc)[0], rgb(COP_RED, 1.0));
        assert!(
            matches!(sc.shapes[2], Shape::Dot { stroke: Some((_, c)), .. } if c == rgb(COP_BLUE, 1.0))
        );
    }
}
