//! On-screen controls for touch screens (port of the core of
//! `src/game/TouchControls.js`, SPEC 8.2): steering with the left-thumb
//! stick, the pedal slider for the right thumb with DRIFT beside it, and the
//! reset, camera and pause taps at the top left. Held controls feed
//! [`super::input::Input::update`]; taps post one-shot actions the same way
//! the keyboard does.
//!
//! - 'stick': an analogue thumb stick. A thumb put down anywhere on the
//!   left of the screen sets the centre there (never so near the edge that
//!   full left lock is out of reach), and sliding it left or right steers in
//!   proportion. Slide past full lock and the centre follows, so coming
//!   back steers the other way at once.
//! - 'slider': one vertical slider for the right thumb. From the bottom:
//!   BRAKE (full at the bottom, lighter going up), a gap to coast in, GAS
//!   (light just above the gap, flat out from about two thirds up) and N₂O
//!   at the top. DRIFT is a strip beside it in the same touch space: slide
//!   the thumb right onto it for the handbrake, still on the gas. A thumb
//!   that starts on the slider keeps working it until it lifts.
//!
//! The DOM measured the controls; here [`Layout`] computes the same boxes
//! from `hud.css`'s rules (`--b`, `--pedal-h`, the insets), in CSS pixels.
//! The ◂ ▸ and pedal-button modes, tilt and auto gas wait for M6
//! (DECISIONS D436).

use mr_math::{clamp, kernel};

use super::input::TouchSource;

/// px of forgiveness round each button.
pub const SLOP: f64 = 14.0;
/// Of the screen's width, from the left.
pub const STICK_ZONE: f64 = 0.45;
/// Of full travel, round the centre.
const STICK_DEAD: f64 = 0.06;
/// And at the ends, so full lock is easy to hold.
const STICK_END: f64 = 0.04;

/// The slider's bands, as fractions of its height from the bottom.
#[derive(Clone, Copy, Debug)]
pub struct Slider {
    pub brake_full: f64,
    pub brake_top: f64,
    pub gas_bottom: f64,
    pub gas_full: f64,
    pub nitro: f64,
}

pub const SLIDER: Slider = Slider {
    brake_full: 0.06,
    brake_top: 0.3,
    gas_bottom: 0.36,
    gas_full: 0.62,
    nitro: 0.82,
};
/// The lightest brake or gas, at the gap.
const LIGHT: f64 = 0.15;

/// JS `Math.sign`.
fn sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        x
    }
}

/// Stick travel as a fraction of full lock (−1…1) to steering: a small dead
/// zone, then a gentle curve for fine corrections near the centre, and the
/// last few per cent are full lock.
pub fn stick_steer(d: f64) -> f64 {
    let a = d.abs();
    if a <= STICK_DEAD {
        return 0.0;
    }
    sign(d)
        * kernel::pow(
            f64::min(1.0, (a - STICK_DEAD) / (1.0 - STICK_DEAD - STICK_END)),
            1.25,
        )
}

/// Full lock on the stick, in CSS px: about a sixth of the screen's short side.
pub fn stick_range(w: f64, h: f64) -> f64 {
    clamp(w.min(h) * 0.15, 44.0, 84.0)
}

/// What a thumb at height `u` on the slider (0 bottom, 1 top) asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pedals {
    pub throttle: f64,
    pub brake: f64,
    pub nitro: bool,
}

pub fn slider_at(u: f64) -> Pedals {
    let s = SLIDER;
    let lerp = |a: f64, b: f64, t: f64| a + (b - a) * clamp(t, 0.0, 1.0);
    if u >= s.nitro {
        return Pedals {
            throttle: 1.0,
            brake: 0.0,
            nitro: true,
        };
    }
    if u >= s.gas_bottom {
        return Pedals {
            throttle: lerp(LIGHT, 1.0, (u - s.gas_bottom) / (s.gas_full - s.gas_bottom)),
            brake: 0.0,
            nitro: false,
        };
    }
    if u > s.brake_top {
        return Pedals {
            throttle: 0.0,
            brake: 0.0,
            nitro: false,
        };
    }
    Pedals {
        throttle: 0.0,
        brake: lerp(
            1.0,
            LIGHT,
            (u - s.brake_full) / (s.brake_top - s.brake_full),
        ),
        nitro: false,
    }
}

/// A box on screen, CSS px, y down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl Rect {
    pub fn width(&self) -> f64 {
        self.right - self.left
    }
    pub fn height(&self) -> f64 {
        self.bottom - self.top
    }
    /// Contains the point, grown by `slop`.
    pub fn hit(&self, x: f64, y: f64, slop: f64) -> bool {
        self.width() > 0.0
            && x >= self.left - slop
            && x <= self.right + slop
            && y >= self.top - slop
            && y <= self.bottom + slop
    }
}

/// The page's safe-area insets (`env(safe-area-inset-*)`), CSS px.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

/// The one-shot buttons at the top left, in their DOM order.
pub const TAPS: [&str; 3] = ["reset", "camera", "pause"];

/// Where the controls sit for a screen (`hud.css`, `body.touch`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Layout {
    pub w: f64,
    pub h: f64,
    /// `--b`, the button size.
    pub b: f64,
    /// `--stick-r`.
    pub stick_r: f64,
    /// The stick's resting centre (`.t-stick` while it waits).
    pub stick_home: (f64, f64),
    /// `.t-pedal`: the slider and the drift strip together.
    pub panel: Rect,
    /// `.t-slider`.
    pub track: Rect,
    /// `.t-drift-strip`.
    pub drift: Rect,
    /// `.t-util`'s buttons: reset, camera, pause.
    pub taps: [Rect; 3],
}

impl Layout {
    pub fn new(w: f64, h: f64, ins: Insets) -> Layout {
        let vmin = w.min(h) / 100.0;
        // --b: clamp(58px, calc(12vmin + 26px), 96px)
        let b = clamp(12.0 * vmin + 26.0, 58.0, 96.0);
        let in_l = ins.left.max(16.0);
        let in_r = ins.right.max(16.0);
        let in_b = ins.bottom.max(14.0);
        // --pedal-h: clamp(190px, 62vmin, 300px)
        let pedal_h = clamp(62.0 * vmin, 190.0, 300.0);
        let stick_r = stick_range(w, h);
        // .t-pedal: right: --inR; bottom: --inB; the slider (.95 b), a 6 px
        // gap, the drift strip (.7 b).
        let bottom = h - in_b;
        let top = bottom - pedal_h;
        let drift = Rect {
            left: w - in_r - 0.7 * b,
            top,
            right: w - in_r,
            bottom,
        };
        let track = Rect {
            left: drift.left - 6.0 - 0.95 * b,
            top,
            right: drift.left - 6.0,
            bottom,
        };
        let panel = Rect {
            left: track.left,
            top,
            right: drift.right,
            bottom,
        };
        // .t-util: top: max(8px, safe top) + 96px; left: --inL; 44 px
        // buttons 10 px apart.
        let ty = ins.top.max(8.0) + 96.0;
        let tap = |i: f64| Rect {
            left: in_l + i * 54.0,
            top: ty,
            right: in_l + i * 54.0 + 44.0,
            bottom: ty + 44.0,
        };
        Layout {
            w,
            h,
            b,
            stick_r,
            // .t-stick: left: --inL + --stick-r + 32px; top: 100% - --inB - .6 b
            stick_home: (in_l + stick_r + 32.0, h - in_b - 0.6 * b),
            panel,
            track,
            drift,
            taps: [tap(0.0), tap(1.0), tap(2.0)],
        }
    }
}

/// The thumb on the stick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stick {
    pub id: u64,
    pub x0: f64,
    pub y0: f64,
    pub x: f64,
}

/// What the slider reads while a thumb is on it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Slide {
    pub throttle: f64,
    pub brake: f64,
    pub nitro: bool,
    pub drift: bool,
    /// The first thumb's height on the track.
    pub u: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Held {
    pub throttle: bool,
    pub brake: bool,
    pub handbrake: bool,
    pub nitro: bool,
}

/// `TouchControls` in 'stick' steering and 'slider' pedals.
#[derive(Clone, Debug, Default)]
pub struct TouchControls {
    pub visible: bool,
    pub layout: Layout,
    pub stick: Option<Stick>,
    /// Thumbs on the slider: pointer id → position, in order.
    pub sliding: Vec<(u64, f64, f64)>,
    pub slide: Option<Slide>,
    pub held: Held,
    pub amount_throttle: f64,
    pub amount_brake: f64,
    /// The last tap and how long it stays lit (`.on` for 140 ms).
    pub lit: Option<(usize, f64)>,
}

impl TouchControls {
    pub fn new(layout: Layout) -> TouchControls {
        TouchControls {
            layout,
            ..TouchControls::default()
        }
    }

    /// `show(on)`: hiding lets go of everything.
    pub fn show(&mut self, on: bool) {
        if on == self.visible {
            return;
        }
        self.visible = on;
        if !on {
            self.release();
        }
    }

    pub fn release(&mut self) {
        self.sliding.clear();
        self.stick = None;
        self.refresh();
    }

    /// The screen changed size (`measure`).
    pub fn resize(&mut self, layout: Layout) {
        self.layout = layout;
    }

    /// The tap button under (x, y): the nearest centre among those whose
    /// rect (grown by SLOP) contains the point.
    fn pick_tap(&self, x: f64, y: f64) -> Option<usize> {
        let mut best = None;
        let mut bd = f64::INFINITY;
        for (i, r) in self.layout.taps.iter().enumerate() {
            if !r.hit(x, y, SLOP) {
                continue;
            }
            let d = kernel::hypot(x - (r.left + r.right) / 2.0, y - (r.top + r.bottom) / 2.0);
            if d < bd {
                bd = d;
                best = Some(i);
            }
        }
        best
    }

    /// `onDown`: a finger lands. Returns the one-shot action it tapped.
    pub fn down(&mut self, id: u64, x: f64, y: f64) -> Option<&'static str> {
        if !self.visible {
            return None;
        }
        if let Some(i) = self.pick_tap(x, y) {
            self.lit = Some((i, 0.14));
            return Some(TAPS[i]);
        }
        if self.stick.is_none() && x < self.layout.w * STICK_ZONE {
            self.stick = Some(Stick {
                id,
                x0: x.max(self.layout.stick_r + 4.0),
                y0: y,
                x,
            });
            return None;
        }
        if self.layout.panel.hit(x, y, SLOP) {
            self.sliding.push((id, x, y));
            self.refresh();
        }
        None
    }

    /// `onMove`.
    pub fn moved(&mut self, id: u64, x: f64, y: f64) {
        if let Some(s) = self.stick.as_mut().filter(|s| s.id == id) {
            let r = self.layout.stick_r;
            s.x = x;
            s.x0 = clamp(s.x0, s.x - r, s.x + r); // past full lock, the centre follows
            return;
        }
        if let Some(p) = self.sliding.iter_mut().find(|p| p.0 == id) {
            p.1 = x;
            p.2 = y;
            self.refresh();
        }
    }

    /// `onUp` (and `pointercancel`).
    pub fn up(&mut self, id: u64) {
        if self.stick.is_some_and(|s| s.id == id) {
            self.stick = None;
            return;
        }
        let n = self.sliding.len();
        self.sliding.retain(|p| p.0 != id);
        if self.sliding.len() != n {
            self.refresh();
        }
    }

    /// Time passes for the tap's highlight.
    pub fn tick(&mut self, dt: f64) {
        if let Some((i, t)) = self.lit {
            self.lit = (t - dt > 0.0).then_some((i, t - dt));
        }
    }

    fn refresh(&mut self) {
        self.held = Held::default();
        self.amount_throttle = 0.0;
        self.amount_brake = 0.0;
        self.slide = (!self.sliding.is_empty()).then(|| self.read_slider());
        if let Some(s) = self.slide {
            let h = &mut self.held;
            h.throttle |= s.throttle > 0.0;
            h.brake |= s.brake > 0.0;
            h.nitro |= s.nitro;
            h.handbrake |= s.drift;
            self.amount_throttle = self.amount_throttle.max(s.throttle);
            self.amount_brake = self.amount_brake.max(s.brake);
        }
    }

    /// Every thumb on the slider: its height sets the pedals (the hardest
    /// wins if there are two), and past the slider's right edge is DRIFT.
    fn read_slider(&self) -> Slide {
        let r = self.layout.track;
        let mut out = Slide::default();
        for &(_, x, y) in &self.sliding {
            let u = clamp((r.bottom - y) / r.height(), 0.0, 1.0);
            let p = slider_at(u);
            out.throttle = out.throttle.max(p.throttle);
            out.brake = out.brake.max(p.brake);
            out.nitro |= p.nitro;
            out.drift |= x > r.right + 3.0;
            out.u = out.u.or(Some(u));
        }
        out
    }

    /// The knob's offset from the stick's centre, px (for drawing).
    pub fn stick_offset(&self) -> f64 {
        self.stick.map_or(0.0, |s| {
            clamp(s.x - s.x0, -self.layout.stick_r, self.layout.stick_r)
        })
    }
}

impl TouchSource for TouchControls {
    /// Nitro needs throttle, so it holds the gas flat too.
    fn throttle(&self) -> f64 {
        if self.held.nitro {
            return 1.0;
        }
        if self.held.throttle {
            return self.amount_throttle;
        }
        0.0
    }
    fn brake(&self) -> f64 {
        if self.held.brake {
            self.amount_brake
        } else {
            0.0
        }
    }
    fn steer(&self) -> f64 {
        0.0
    }
    fn handbrake(&self) -> bool {
        self.held.handbrake
    }
    fn nitro(&self) -> bool {
        self.held.nitro
    }
    fn analog_steer(&mut self, _dt: f64) -> Option<f64> {
        if !self.visible {
            return None;
        }
        Some(
            self.stick
                .map_or(0.0, |s| stick_steer((s.x - s.x0) / self.layout.stick_r)),
        )
    }
}

#[cfg(test)]
mod tests {
    //! `test/unit/touch.test.js`, and the controls' flow.
    use super::*;
    use crate::play::input::Input;

    fn near(a: f64, b: f64, eps: f64) {
        assert!((a - b).abs() <= eps, "{a} ≉ {b}");
    }

    #[test]
    fn stick_dead_zone_curve_full_lock_symmetric() {
        assert_eq!(stick_steer(0.0), 0.0);
        assert_eq!(stick_steer(0.05), 0.0, "inside the dead zone");
        assert_eq!(stick_steer(1.0), 1.0);
        assert_eq!(stick_steer(-1.0), -1.0);
        assert_eq!(stick_steer(3.0), 1.0, "no further than full lock");
        assert_eq!(
            stick_steer(0.97),
            1.0,
            "the last few per cent are full lock"
        );
        assert_eq!(stick_steer(0.9999999999999992), 1.0);
        let mut prev = 0.0;
        let mut d = 0.07;
        while d <= 0.955 {
            let s = stick_steer(d);
            assert!(s > prev && s < 1.0, "rises with the travel ({d:.2})");
            near(stick_steer(-d), -s, 1e-12);
            prev = s;
            d += 0.01;
        }
        let h = stick_steer(0.5);
        assert!(
            h < 0.5 && h > 0.3,
            "half travel is a bit under half lock ({h:.2})"
        );
    }

    #[test]
    fn stick_range_is_a_sixth_of_the_short_side() {
        near(stick_range(915.0, 412.0), 61.8, 1e-9);
        assert_eq!(stick_range(412.0, 915.0), stick_range(915.0, 412.0));
        assert_eq!(
            stick_range(640.0, 280.0),
            44.0,
            "never too short to control"
        );
        assert_eq!(
            stick_range(1366.0, 1024.0),
            84.0,
            "nor too long on a tablet"
        );
    }

    #[test]
    fn slider_bands() {
        let s = SLIDER;
        let p = |throttle, brake, nitro| Pedals {
            throttle,
            brake,
            nitro,
        };
        assert_eq!(slider_at(0.0), p(0.0, 1.0, false), "the bottom: full brake");
        assert_eq!(slider_at(s.brake_full).brake, 1.0, "and a little way up");
        near(slider_at(s.brake_top).brake, 0.15, 1e-12);
        assert_eq!(
            slider_at((s.brake_top + s.gas_bottom) / 2.0),
            p(0.0, 0.0, false),
            "the gap: coasting"
        );
        near(slider_at(s.gas_bottom).throttle, 0.15, 1e-12);
        assert_eq!(
            slider_at(s.gas_full).throttle,
            1.0,
            "flat out from the mark"
        );
        assert_eq!(
            slider_at((s.gas_full + s.nitro) / 2.0),
            p(1.0, 0.0, false),
            "flat out, no nitro"
        );
        assert_eq!(slider_at(s.nitro), p(1.0, 0.0, true), "N₂O");
        assert_eq!(slider_at(1.0), p(1.0, 0.0, true));
        // Steady all the way: the brake eases off going up, the gas comes on.
        let (mut brake, mut gas) = (2.0, -1.0);
        let mut u = 0.0;
        while u <= 1.0001 {
            let q = slider_at(u);
            assert!(!(q.brake > 0.0 && q.throttle > 0.0), "never both ({u:.2})");
            if u <= s.brake_top {
                assert!(q.brake <= brake, "brake eases off ({u:.2})");
                brake = q.brake;
            }
            if u >= s.gas_bottom {
                assert!(q.throttle >= gas, "gas comes on ({u:.2})");
                gas = q.throttle;
            }
            u += 0.01;
        }
        assert!(s.brake_top >= 1.0 / 6.0 && s.nitro - s.gas_bottom >= 1.0 / 6.0);
        assert!(1.0 - s.nitro >= 1.0 / 6.0);
        assert!(s.gas_bottom - s.brake_top >= 1.0 / 20.0);
    }

    /// A phone sideways: thumbs on the stick and the slider drive the
    /// input layer; DRIFT past the slider's edge; taps post actions.
    #[test]
    fn thumbs_drive_the_input_layer() {
        let lay = Layout::new(915.0, 412.0, Insets::default());
        let mut t = TouchControls::new(lay);
        let mut input = Input::new();
        // Hidden controls do nothing.
        assert_eq!(t.down(1, 200.0, 300.0), None);
        assert!(t.stick.is_none());
        t.show(true);
        // Left thumb: the stick, centred where it lands.
        t.down(1, 200.0, 300.0);
        t.moved(1, 200.0 + lay.stick_r, 300.0);
        let s = input.update(1.0 / 120.0, Some(&mut t));
        assert!(s.analog);
        assert_eq!(s.steer, 1.0, "full right lock");
        // Past full lock the centre follows: coming back steers left at once.
        t.moved(1, 200.0 + 2.0 * lay.stick_r, 300.0);
        t.moved(1, 200.0 + lay.stick_r * 0.5, 300.0);
        assert!(input.update(1.0 / 120.0, Some(&mut t)).steer < 0.0);
        // Never so near the edge that full left lock is out of reach.
        t.up(1);
        t.down(2, 3.0, 300.0);
        assert_eq!(t.stick.unwrap().x0, lay.stick_r + 4.0);
        t.up(2);
        assert_eq!(input.update(1.0 / 120.0, Some(&mut t)).steer, 0.0);
        // Right thumb on the slider: flat out near the top of the gas band.
        let r = lay.track;
        let at = |u: f64| r.bottom - u * r.height();
        let cx = (r.left + r.right) / 2.0;
        t.down(3, cx, at(0.7));
        let s = input.update(1.0 / 120.0, Some(&mut t));
        assert_eq!(
            (s.throttle, s.brake, s.nitro, s.handbrake),
            (1.0, 0.0, false, false)
        );
        // Slide down into the brake band, then right onto DRIFT.
        t.moved(3, cx, at(0.03));
        let s = input.update(1.0 / 120.0, Some(&mut t));
        assert_eq!(s.throttle, 0.0);
        assert_eq!(s.brake, 1.0);
        t.moved(3, lay.drift.left + 10.0, at(0.5));
        let s = input.update(1.0 / 120.0, Some(&mut t));
        assert!(s.handbrake && s.throttle > 0.0);
        // Top: N₂O, which holds the gas flat.
        t.moved(3, cx, at(0.95));
        let s = input.update(1.0 / 120.0, Some(&mut t));
        assert!(s.nitro && s.throttle == 1.0);
        t.up(3);
        assert_eq!(input.update(1.0 / 120.0, Some(&mut t)).throttle, 0.0);
        // The reset button.
        let b = lay.taps[0];
        assert_eq!(t.down(4, b.left + 5.0, b.top + 5.0), Some("reset"));
    }
}
