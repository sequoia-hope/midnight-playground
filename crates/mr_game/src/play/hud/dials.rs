//! `drawTach` and `drawPower` as numbers: the 260 px canvas's arcs, ticks,
//! labels and needle. The shader (`hud.wgsl`) draws the backplate, the
//! track, the redline (or regen) zone, the fill and the ticks from
//! [`DialDraw`]; the labels are text nodes and the needle a turned node,
//! above it as the canvas draws them last.

use std::f64::consts::PI;

/// The canvas: 260 px square, the dial's centre and radius.
#[cfg_attr(not(test), allow(dead_code))]
pub const W: f64 = 260.0;
pub const CX: f64 = 130.0;
pub const CY: f64 = 130.0;
pub const R: f64 = 112.0;
/// The arc runs from 0.75π to 2.25π (clockwise, y down).
pub const A0: f64 = PI * 0.75;
pub const A1: f64 = PI * 2.25;
/// The rev counter's scale and redline.
pub const MAX_RPM: f64 = 8000.0;
pub const REDLINE: f64 = 7000.0;
/// The power meter's scale, kW.
pub const LO: f64 = -250.0;
pub const HI: f64 = 1000.0;

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    mr_math::clamp(v, lo, hi)
}

/// How far round the rev counter is: `clamp(rpm / maxR, 0, 1)`.
pub fn tach_fill(rpm: f64) -> f64 {
    clamp(rpm / MAX_RPM, 0.0, 1.0)
}

/// The angle a fraction of the way round.
pub fn angle(f: f64) -> f64 {
    A0 + (A1 - A0) * f
}

/// The power meter's `at(v)`.
pub fn power_at(kw: f64) -> f64 {
    angle((clamp(kw, LO, HI) - LO) / (HI - LO))
}

/// `#3ad7ff → #b36bff (.7) → #ff3860` (the rev counter) or `#3ad7ff →
/// #9ff3ff (.75) → #fff` (drive power): stops along the canvas's diagonal,
/// bottom left to top right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grad {
    pub c: [u32; 3],
    pub mid: f64,
}

pub const TACH_GRAD: Grad = Grad {
    c: [0x3ad7ff, 0xb36bff, 0xff3860],
    mid: 0.7,
};
pub const POWER_GRAD: Grad = Grad {
    c: [0x3ad7ff, 0x9ff3ff, 0xffffff],
    mid: 0.75,
};

impl Grad {
    /// The colour at canvas point (x, y): `createLinearGradient(0, W, W,
    /// 0)` projects the point on the diagonal, the stops interpolated in
    /// sRGB as the canvas does. (`hud.wgsl` computes the same; this
    /// copy is its test.)
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn at(&self, x: f64, y: f64) -> [f64; 3] {
        let t = clamp((x - y + W) / (2.0 * W), 0.0, 1.0);
        let (a, b, u) = if t <= self.mid {
            (self.c[0], self.c[1], t / self.mid)
        } else {
            (self.c[1], self.c[2], (t - self.mid) / (1.0 - self.mid))
        };
        let ch = |c: u32, s: u32| ((c >> s) & 255) as f64 / 255.0;
        [16, 8, 0].map(|s| ch(a, s) + (ch(b, s) - ch(a, s)) * u)
    }
}

/// How the fill is painted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fill {
    Grad(Grad),
    /// Regen: `#4dff8a`.
    Solid(u32),
}

/// What the shader draws of a dial.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DialDraw {
    /// The fill's arc, from and to.
    pub fill: (f64, f64),
    pub paint: Fill,
    /// The redline or the regen zone: from, to, colour, alpha.
    pub zone: (f64, f64, u32, f64),
    /// Ticks: the first angle, the step, how many.
    pub ticks: (f64, f64, u32),
}

/// A dial as numbers: the shader's part, the labels and the needle.
#[derive(Clone, Debug, PartialEq)]
pub struct Dial {
    pub draw: DialDraw,
    /// Text, canvas x, y (the centre, `textAlign center`, `textBaseline
    /// middle`), size, weight, colour, alpha.
    pub labels: Vec<Label>,
    /// The needle's angle and colour.
    pub needle: (f64, u32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub size: f64,
    pub weight: u16,
    pub color: u32,
    pub alpha: f64,
}

fn tick_label(text: String, a: f64) -> Label {
    Label {
        text,
        x: CX + a.cos() * (R - 22.0),
        y: CY + a.sin() * (R - 22.0),
        size: 13.0,
        weight: 600,
        color: 0xffffff,
        alpha: 0.7,
    }
}

/// `drawTach(rpm)`.
pub fn tach(rpm: f64) -> Dial {
    let f = tach_fill(rpm);
    Dial {
        draw: DialDraw {
            fill: (A0, angle(f)),
            paint: Fill::Grad(TACH_GRAD),
            zone: (angle(REDLINE / MAX_RPM), A1, 0xff3860, 0.55),
            ticks: (A0, (A1 - A0) / 8.0, 9),
        },
        labels: (0..=8)
            .map(|k| tick_label(k.to_string(), angle(f64::from(k) / 8.0)))
            .collect(),
        needle: (angle(f), 0xff3860),
    }
}

/// `drawPower(kw)`: regen in green below zero, drive power up to 1000 kW.
pub fn power(kw: f64) -> Dial {
    let mut labels: Vec<Label> = (0..=5)
        .map(|i| {
            let v = f64::from(i) * 200.0;
            tick_label((v / 100.0).to_string(), power_at(v))
        })
        .collect();
    labels.push(Label {
        text: "REGEN".into(),
        x: CX + A0.cos() * (R - 26.0) + 10.0,
        y: CY + A0.sin() * (R - 26.0) + 4.0,
        size: 10.0,
        weight: 700,
        color: 0x4dff8a,
        alpha: 0.85,
    });
    labels.push(Label {
        text: "kW ×100".into(),
        x: CX + A1.cos() * (R - 30.0) - 10.0,
        y: CY + A1.sin() * (R - 30.0) + 4.0,
        size: 10.0,
        weight: 700,
        color: 0xffffff,
        alpha: 0.55,
    });
    let (fill, paint) = if kw >= 0.0 {
        ((power_at(0.0), power_at(kw)), Fill::Grad(POWER_GRAD))
    } else {
        ((power_at(kw), power_at(0.0)), Fill::Solid(0x4dff8a))
    };
    Dial {
        draw: DialDraw {
            fill,
            paint,
            zone: (A0, power_at(0.0), 0x4dff8a, 0.28),
            ticks: (power_at(0.0), power_at(200.0) - power_at(0.0), 6),
        },
        labels,
        needle: (power_at(kw), if kw < 0.0 { 0x4dff8a } else { 0x3ad7ff }),
    }
}

/// The needle: from 30 px out to R − 4, 3 px wide.
pub const NEEDLE: (f64, f64, f64) = (30.0, R - 4.0, 3.0);

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn the_rev_counter() {
        assert_eq!(tach_fill(4000.0), 0.5);
        assert_eq!(tach_fill(-10.0), 0.0);
        assert_eq!(tach_fill(9000.0), 1.0);
        let d = tach(4000.0);
        // Half way round a 270° arc from bottom left: straight up.
        assert!(near(d.needle.0, 1.5 * PI));
        assert!(near(d.draw.fill.1, 1.5 * PI));
        assert_eq!(d.needle.1, 0xff3860);
        // The redline from 7000.
        assert!(near(d.draw.zone.0, A0 + (A1 - A0) * 0.875));
        assert!(near(d.draw.zone.1, A1));
        // Labels 0..8 at R − 22; 0 bottom left, 4 at the top, 8 bottom
        // right.
        assert_eq!(d.labels.len(), 9);
        assert_eq!(d.labels[4].text, "4");
        assert!(near(d.labels[4].x, 130.0) && near(d.labels[4].y, 40.0));
        let h = 90.0 / 2f64.sqrt();
        assert!(near(d.labels[0].x, 130.0 - h) && near(d.labels[0].y, 130.0 + h));
        assert!(near(d.labels[8].x, 130.0 + h) && near(d.labels[8].y, 130.0 + h));
        assert_eq!(d.draw.ticks.2, 9);
    }

    #[test]
    fn the_power_meter() {
        // Zero is a fifth of the way round.
        assert!(near(power_at(0.0), A0 + (A1 - A0) * 0.2));
        assert!(near(power_at(-500.0), A0));
        assert!(near(power_at(2000.0), A1));
        let d = power(300.0);
        assert!(near(d.draw.fill.0, power_at(0.0)) && near(d.draw.fill.1, power_at(300.0)));
        assert_eq!(d.draw.paint, Fill::Grad(POWER_GRAD));
        assert_eq!(d.needle.1, 0x3ad7ff);
        let d = power(-100.0);
        assert!(near(d.draw.fill.0, power_at(-100.0)) && near(d.draw.fill.1, power_at(0.0)));
        assert_eq!(d.draw.paint, Fill::Solid(0x4dff8a));
        assert_eq!(d.needle.1, 0x4dff8a);
        let texts: Vec<&str> = d.labels.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["0", "2", "4", "6", "8", "10", "REGEN", "kW ×100"]);
        // The last tick is at the end of the arc.
        let (a, step, n) = d.draw.ticks;
        assert!(near(a + step * f64::from(n - 1), A1));
    }

    #[test]
    fn the_fill_gradient_runs_along_the_diagonal() {
        let g = TACH_GRAD;
        assert_eq!(g.at(0.0, 260.0), [0x3a as f64 / 255.0, 0xd7 as f64 / 255.0, 1.0]);
        assert_eq!(g.at(260.0, 0.0), [1.0, 0x38 as f64 / 255.0, 0x60 as f64 / 255.0]);
        // The middle of the canvas is half way: 0.5 / 0.7 of the first
        // stretch.
        let c = g.at(130.0, 130.0);
        let u = 0.5 / 0.7;
        assert!(near(c[0], (0x3a as f64 + (0xb3 - 0x3a) as f64 * u) / 255.0));
        // The point at 0.7 is the middle stop.
        let x = 0.7 * 2.0 * W - W;
        let c = g.at(x, 0.0);
        assert!(near(c[1], 0x6b as f64 / 255.0));
    }
}
