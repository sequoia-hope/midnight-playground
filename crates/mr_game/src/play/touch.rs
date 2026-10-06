//! On-screen controls for touch screens (port of `src/game/TouchControls.js`,
//! SPEC 8.2): steering on the left, the pedals bottom-right, reset, camera
//! and pause at the top. Held controls feed
//! [`super::input::Input::update`]; tap buttons post one-shot actions the
//! same way the keyboard does.
//!
//! Steering ([`Steering`], `mode`) is one of:
//! - 'stick': an analogue thumb stick. A thumb put down anywhere on the
//!   left of the screen sets the centre there (never so near the edge that
//!   full left lock is out of reach), and sliding it left or right steers in
//!   proportion. Slide past full lock and the centre follows, so coming
//!   back steers the other way at once.
//! - 'buttons': ◂ ▸ pads, which Input ramps like keys.
//! - 'tilt': a [`TiltSteer`](super::tilt::TiltSteer) (as `.tilt`), with a
//!   wheel showing the lock; the stick stands in until the sensor answers.
//!
//! Pedals ([`PedalKind`], `pedals`) are one of:
//! - 'slider': one vertical slider for the right thumb. From the bottom:
//!   BRAKE (full at the bottom, lighter going up), a gap to coast in, GAS
//!   (light just above the gap, flat out from about two thirds up) and N₂O
//!   at the top. DRIFT is a strip beside it in the same touch space: slide
//!   the thumb right onto it for the handbrake, still on the gas. A thumb
//!   that starts on the slider keeps working it until it lifts.
//! - 'buttons': GAS, BRAKE, DRIFT and N₂O pads.
//!
//! Fingers on the pads are hit-tested against every pad on each move, so a
//! thumb can slide from gas to brake (or ◂ to ▸) without lifting.
//!
//! The DOM measured its boxes; here [`Layout`] computes the same boxes from
//! `hud.css`'s rules (`--b`, `--pedal-h`, `--stick-r`, the insets), in CSS
//! pixels (DECISIONS D436, D840).

use mr_math::{clamp, kernel};

use super::input::TouchSource;
use super::tilt::SharedTilt;

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

/// Full lock on the stick, in CSS px: the JS's sixth of the screen's short
/// side, made longer for finer steering (D1084): 0.17 of the short side
/// held sideways, 0.185 of the width upright (where the pedal panel beside
/// it leaves room for no more).
pub fn stick_range(w: f64, h: f64) -> f64 {
    if h > w {
        clamp(w * 0.185, 46.0, 92.0)
    } else {
        clamp(w.min(h) * 0.17, 46.0, 92.0)
    }
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
    fn centre(&self) -> (f64, f64) {
        (
            (self.left + self.right) / 2.0,
            (self.top + self.bottom) / 2.0,
        )
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

/// The held controls (`HOLD`), in the JS order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hold {
    Throttle,
    Brake,
    Left,
    Right,
    Handbrake,
    Nitro,
}

impl Hold {
    /// The pad's `data-act`.
    pub fn name(self) -> &'static str {
        match self {
            Hold::Throttle => "throttle",
            Hold::Brake => "brake",
            Hold::Left => "left",
            Hold::Right => "right",
            Hold::Handbrake => "handbrake",
            Hold::Nitro => "nitro",
        }
    }
}

/// The ◂ ▸ pads (`.t-steer`), in DOM order.
pub const DIRS: [Hold; 2] = [Hold::Left, Hold::Right];
/// The pedal pads (`.t-pedals`), in DOM order: DRIFT, N₂O, BRAKE, GAS.
pub const PEDAL_PADS: [Hold; 4] = [Hold::Handbrake, Hold::Nitro, Hold::Brake, Hold::Throttle];

/// What steers (`mode`, and `steering`: what steers right now).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Steering {
    #[default]
    Stick,
    Buttons,
    Tilt,
}

impl Steering {
    pub fn from_name(s: &str) -> Steering {
        match s {
            "buttons" => Steering::Buttons,
            "tilt" => Steering::Tilt,
            _ => Steering::Stick,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Steering::Stick => "stick",
            Steering::Buttons => "buttons",
            Steering::Tilt => "tilt",
        }
    }
}

/// The pedals chosen (`pedals`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PedalKind {
    #[default]
    Slider,
    Buttons,
}

impl PedalKind {
    pub fn from_name(s: &str) -> PedalKind {
        if s == "buttons" {
            PedalKind::Buttons
        } else {
            PedalKind::Slider
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            PedalKind::Slider => "slider",
            PedalKind::Buttons => "buttons",
        }
    }
}

/// Where the controls sit for a screen (`hud.css`, `body.touch`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Layout {
    pub w: f64,
    pub h: f64,
    /// `--b`, the button size.
    pub b: f64,
    /// `--stick-r`.
    pub stick_r: f64,
    /// The stick's knob, its diameter (56 px in the JS; D1084).
    pub knob: f64,
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
    /// `.t-steer`'s ◂ ▸ pads ([`DIRS`]).
    pub dirs: [Rect; 2],
    /// `.t-pedals`' pads ([`PEDAL_PADS`]).
    pub pedals: [Rect; 4],
    /// `.t-wheel`.
    pub wheel: Rect,
}

impl Layout {
    pub fn new(w: f64, h: f64, ins: Insets) -> Layout {
        let vmin = w.min(h) / 100.0;
        let portrait = h > w;
        // Bigger than the JS's (D1084). The JS: --b: clamp(58px, calc(12vmin
        // + 26px), 96px), --pedal-h: clamp(190px, 62vmin, 300px), the slider
        // .95 b and the drift strip .7 b wide, the same both ways up.
        // Sideways: --b clamp(60px, 14vmin + 26px, 104px) and the slider
        // 66vmin (to 320 px). Upright, where the width is short and the
        // height long: --b a fifth of the width, held to what lets the
        // Buttons choices' four pads share a row; the slider panel's unit
        // .22 of the width; the slider .36 of the height (240 to 380 px);
        // the pads taller (below).
        let b = if portrait {
            clamp(0.2 * w, 58.0, 104.0).min(((w - 62.0) / 4.16).max(58.0))
        } else {
            clamp(14.0 * vmin + 26.0, 60.0, 104.0)
        };
        let pb = if portrait {
            clamp(0.22 * w, 60.0, 110.0)
        } else {
            b
        };
        let in_l = ins.left.max(16.0);
        let in_r = ins.right.max(16.0);
        let in_b = ins.bottom.max(14.0);
        let pedal_h = if portrait {
            clamp(0.36 * h, 240.0, 380.0)
        } else {
            clamp(66.0 * vmin, 200.0, 320.0)
        };
        let stick_r = stick_range(w, h);
        // .t-pedal: right: --inR; bottom: --inB; the slider (.95 b), a 6 px
        // gap, the drift strip (.7 b).
        let bottom = h - in_b;
        let top = bottom - pedal_h;
        let drift = Rect {
            left: w - in_r - 0.7 * pb,
            top,
            right: w - in_r,
            bottom,
        };
        let track = Rect {
            left: drift.left - 6.0 - 0.95 * pb,
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
        // .t-steer: left: --inL; bottom: --inB; two 1.08 b pads 14 px apart.
        let d = 1.08 * b;
        // Upright, the ◂ ▸ pads and the pedals are taller (D1084).
        let tall_k = if portrait { 1.4 } else { 1.0 };
        let dir = |i: f64| Rect {
            left: in_l + i * (d + 14.0),
            top: bottom - d * tall_k,
            right: in_l + i * (d + 14.0) + d,
            bottom,
        };
        // .t-pedals: right: --inR; bottom: --inB; a grid of two b-wide
        // columns 14 px apart and two rows 12 px apart, its items at the
        // bottom of their row and centred in their column: DRIFT and N₂O
        // (.74 b round) over BRAKE and GAS (b by 1.3 b).
        let c2 = w - in_r - b;
        let c1 = c2 - 14.0 - b;
        let row2 = bottom - 1.3 * b * tall_k;
        let small = if portrait { 0.84 * b } else { 0.74 * b };
        let row1 = row2 - 12.0;
        let round = |c: f64| Rect {
            left: c + (b - small) / 2.0,
            top: row1 - small,
            right: c + (b + small) / 2.0,
            bottom: row1,
        };
        let tall = |c: f64| Rect {
            left: c,
            top: row2,
            right: c + b,
            bottom,
        };
        // .t-wheel: left: --inL + 8px; bottom: --inB + 8px; 1.3 b square.
        let wheel = Rect {
            left: in_l + 8.0,
            top: bottom - 8.0 - 1.3 * b,
            right: in_l + 8.0 + 1.3 * b,
            bottom: bottom - 8.0,
        };
        Layout {
            w,
            h,
            b,
            stick_r,
            knob: clamp(stick_r, 56.0, 72.0),
            // .t-stick: left: --inL + --stick-r + 32px; top: 100% - --inB - .6 b
            stick_home: (in_l + stick_r + 32.0, h - in_b - 0.6 * b),
            panel,
            track,
            drift,
            taps: [tap(0.0), tap(1.0), tap(2.0)],
            dirs: [dir(0.0), dir(1.0)],
            pedals: [round(c1), round(c2), tall(c1), tall(c2)],
            wheel,
        }
    }

    /// `.t-stick`'s box while it waits (two full locks wide plus 64 px, 64
    /// tall, centred on its resting place).
    pub fn stick_box(&self) -> Rect {
        let (cx, cy) = self.stick_home;
        let hw = self.stick_r + 32.0;
        let hh = 32.0f64.max(self.knob / 2.0 + 4.0);
        Rect {
            left: cx - hw,
            top: cy - hh,
            right: cx + hw,
            bottom: cy + hh,
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

/// `held`: what each held control reads.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Held {
    pub throttle: bool,
    pub brake: bool,
    pub left: bool,
    pub right: bool,
    pub handbrake: bool,
    pub nitro: bool,
}

impl Held {
    pub fn get(&self, h: Hold) -> bool {
        match h {
            Hold::Throttle => self.throttle,
            Hold::Brake => self.brake,
            Hold::Left => self.left,
            Hold::Right => self.right,
            Hold::Handbrake => self.handbrake,
            Hold::Nitro => self.nitro,
        }
    }
    fn set(&mut self, h: Hold) {
        match h {
            Hold::Throttle => self.throttle = true,
            Hold::Brake => self.brake = true,
            Hold::Left => self.left = true,
            Hold::Right => self.right = true,
            Hold::Handbrake => self.handbrake = true,
            Hold::Nitro => self.nitro = true,
        }
    }
}

/// `TouchControls`.
#[derive(Clone, Debug, Default)]
pub struct TouchControls {
    pub visible: bool,
    pub auto_gas: bool,
    /// The player's steering choice.
    pub mode: Steering,
    pub pedals: PedalKind,
    /// The tilt sensor, shared with `play::tilt`.
    pub tilt: Option<SharedTilt>,
    /// What steers right now (`None` before the first `layout`).
    pub steering: Option<Steering>,
    pub layout: Layout,
    pub stick: Option<Stick>,
    /// Fingers on the pads: pointer id → position, in order (a JS `Map`).
    pub pointers: Vec<(u64, f64, f64)>,
    /// Thumbs on the slider: pointer id → position, in order.
    pub sliding: Vec<(u64, f64, f64)>,
    pub slide: Option<Slide>,
    pub held: Held,
    pub amount_throttle: f64,
    pub amount_brake: f64,
    /// The last tap and how long it stays lit (`.on` for 140 ms).
    pub lit: Option<(usize, f64)>,
    /// The wheel's turn, degrees (`rotate(s * 90deg)`).
    pub wheel: f64,
    /// A tap wants the haptic tick (`navigator.vibrate(8)`); the web glue
    /// takes it.
    pub buzz: bool,
}

impl TouchControls {
    pub fn new(layout: Layout) -> TouchControls {
        let mut t = TouchControls {
            layout,
            ..TouchControls::default()
        };
        t.lay_out(false);
        t
    }

    pub fn set_mode(&mut self, mode: Steering) {
        self.mode = mode;
        self.lay_out(false);
    }

    pub fn set_pedals(&mut self, kind: PedalKind) {
        self.pedals = kind;
        self.sliding.clear();
        self.pointers.clear();
        self.lay_out(true);
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
        self.pointers.clear();
        self.sliding.clear();
        self.stick = None;
        self.refresh();
    }

    /// The screen changed size (`measure`).
    pub fn resize(&mut self, layout: Layout) {
        self.layout = layout;
    }

    fn tilt_live(&self) -> bool {
        self.tilt
            .as_ref()
            .is_some_and(|t| t.lock().unwrap_or_else(|e| e.into_inner()).live())
    }

    /// `layout(force)`: show the controls for whatever steers now (tilt
    /// goes live when the sensor first answers) and for the pedals chosen.
    pub fn lay_out(&mut self, force: bool) {
        let s = if self.mode == Steering::Tilt && self.tilt_live() {
            Steering::Tilt
        } else if self.mode == Steering::Buttons {
            Steering::Buttons
        } else {
            Steering::Stick
        };
        if Some(s) == self.steering && !force {
            return;
        }
        self.steering = Some(s);
        if s != Steering::Stick {
            self.stick = None;
        }
        self.refresh(); // a thumb resting on a pad that just hid lets go
    }

    /// The held pads shown now, with their boxes (a hidden pad has no box).
    pub fn hold_pads(&self) -> Vec<(Hold, Rect)> {
        let mut v = Vec::new();
        if self.steering == Some(Steering::Buttons) {
            v.extend(DIRS.iter().copied().zip(self.layout.dirs));
        }
        if self.pedals == PedalKind::Buttons {
            v.extend(PEDAL_PADS.iter().copied().zip(self.layout.pedals));
        }
        v
    }

    /// The button under (x, y): the nearest centre among those whose rect
    /// (grown by SLOP) contains the point, so neighbours don't both light.
    fn pick<T: Copy>(els: impl IntoIterator<Item = (T, Rect)>, x: f64, y: f64) -> Option<T> {
        let mut best = None;
        let mut bd = f64::INFINITY;
        for (k, r) in els {
            if !r.hit(x, y, SLOP) {
                continue;
            }
            let (cx, cy) = r.centre();
            let d = kernel::hypot(x - cx, y - cy);
            if d < bd {
                bd = d;
                best = Some(k);
            }
        }
        best
    }

    /// `onDown`: a finger lands. Returns the one-shot action it tapped.
    pub fn down(&mut self, id: u64, x: f64, y: f64) -> Option<&'static str> {
        if !self.visible {
            return None;
        }
        let taps = (0..TAPS.len()).zip(self.layout.taps);
        let tap = Self::pick(taps, x, y);
        if let Some(i) = tap {
            self.lit = Some((i, 0.14));
            self.buzz = true;
        } else if self.steering == Some(Steering::Stick)
            && self.stick.is_none()
            && x < self.layout.w * STICK_ZONE
        {
            self.stick = Some(Stick {
                id,
                x0: x.max(self.layout.stick_r + 4.0),
                y0: y,
                x,
            });
            return None;
        } else if self.pedals == PedalKind::Slider && self.layout.panel.hit(x, y, SLOP) {
            self.sliding.push((id, x, y));
            self.refresh();
            return None;
        }
        set_pointer(&mut self.pointers, id, x, y);
        self.refresh();
        tap.map(|i| TAPS[i])
    }

    /// `onMove`.
    pub fn moved(&mut self, id: u64, x: f64, y: f64) {
        if let Some(s) = self.stick.as_mut().filter(|s| s.id == id) {
            let r = self.layout.stick_r;
            s.x = x;
            s.x0 = clamp(s.x0, s.x - r, s.x + r); // past full lock, the centre follows
            return;
        }
        let map = if self.sliding.iter().any(|p| p.0 == id) {
            &mut self.sliding
        } else if self.pointers.iter().any(|p| p.0 == id) {
            &mut self.pointers
        } else {
            return;
        };
        set_pointer(map, id, x, y);
        self.refresh();
    }

    /// `onUp` (and `pointercancel`).
    pub fn up(&mut self, id: u64) {
        if self.stick.is_some_and(|s| s.id == id) {
            self.stick = None;
            return;
        }
        let n = self.sliding.len() + self.pointers.len();
        self.sliding.retain(|p| p.0 != id);
        if self.sliding.len() + self.pointers.len() == n {
            self.pointers.retain(|p| p.0 != id);
        }
        if self.sliding.len() + self.pointers.len() != n {
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
        let mut h = Held::default();
        let pads = self.hold_pads();
        for &(_, x, y) in &self.pointers {
            if let Some(k) = Self::pick(pads.iter().copied(), x, y) {
                h.set(k);
            }
        }
        self.amount_throttle = if h.throttle { 1.0 } else { 0.0 };
        self.amount_brake = if h.brake { 1.0 } else { 0.0 };
        self.slide = (!self.sliding.is_empty()).then(|| self.read_slider());
        if let Some(s) = self.slide {
            h.throttle |= s.throttle > 0.0;
            h.brake |= s.brake > 0.0;
            h.nitro |= s.nitro;
            h.handbrake |= s.drift;
            self.amount_throttle = self.amount_throttle.max(s.throttle);
            self.amount_brake = self.amount_brake.max(s.brake);
        }
        self.held = h;
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

    /// The pedal panel's classes (`.t-pedal.active.gas…`), for drawing
    /// and the tests.
    pub fn panel_classes(&self) -> Vec<&'static str> {
        let Some(s) = self.slide else {
            return Vec::new();
        };
        let mut v = vec!["active"];
        if s.throttle > 0.0 {
            v.push("gas");
        }
        if s.brake > 0.0 {
            v.push("brake");
        }
        if s.nitro {
            v.push("nitro");
        }
        if s.drift {
            v.push("drift");
        }
        v
    }
}

/// `map.set(id, [x, y])`: a new key goes last, an old one keeps its place.
fn set_pointer(map: &mut Vec<(u64, f64, f64)>, id: u64, x: f64, y: f64) {
    match map.iter_mut().find(|p| p.0 == id) {
        Some(p) => {
            p.1 = x;
            p.2 = y;
        }
        None => map.push((id, x, y)),
    }
}

impl TouchSource for TouchControls {
    /// Gas (or auto gas unless braking). Nitro needs throttle, so it holds
    /// the gas flat too. A thumb on the slider sets the gas itself, so auto
    /// gas waits while it's there.
    fn throttle(&self) -> f64 {
        if self.held.nitro {
            return 1.0;
        }
        if self.held.throttle {
            return self.amount_throttle;
        }
        if self.auto_gas && self.visible && !self.held.brake && self.slide.is_none() {
            1.0
        } else {
            0.0
        }
    }
    fn brake(&self) -> f64 {
        if self.held.brake {
            self.amount_brake
        } else {
            0.0
        }
    }
    /// For the ◂ ▸ pads, a steering target of −1, 0 or +1.
    fn steer(&self) -> f64 {
        f64::from(u8::from(self.held.right)) - f64::from(u8::from(self.held.left))
    }
    fn handbrake(&self) -> bool {
        self.held.handbrake
    }
    fn nitro(&self) -> bool {
        self.held.nitro
    }
    /// This tick's analogue steering (the stick, or tilt), or `None` when
    /// steering is on the ◂ ▸ pads.
    fn analog_steer(&mut self, dt: f64) -> Option<f64> {
        self.lay_out(false);
        if !self.visible || self.steering == Some(Steering::Buttons) {
            return None;
        }
        if self.steering != Some(Steering::Tilt) {
            return Some(
                self.stick
                    .map_or(0.0, |s| stick_steer((s.x - s.x0) / self.layout.stick_r)),
            );
        }
        let s = self.tilt.as_ref().map_or(0.0, |t| {
            t.lock().unwrap_or_else(|e| e.into_inner()).update(dt)
        });
        self.wheel = s * 90.0;
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    //! `test/unit/touch.test.js`, and the controls' flow
    //! (`test/e2e/touch-controls.test.js`' and `analog-controls.test.js`'
    //! touch logic, without a browser).
    use super::*;
    use crate::play::input::Input;
    use crate::play::tilt::{NoSensor, TiltSteer};
    use std::sync::{Arc, Mutex};

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
    fn stick_range_from_the_screen() {
        // Sideways, 0.17 of the short side (the JS's 0.15, D1084).
        near(stick_range(915.0, 412.0), 70.04, 1e-9);
        // Upright, 0.185 of the width: longer than sideways.
        near(stick_range(412.0, 915.0), 76.22, 1e-9);
        assert_eq!(
            stick_range(640.0, 250.0),
            46.0,
            "never too short to control"
        );
        assert_eq!(
            stick_range(1366.0, 1024.0),
            92.0,
            "nor too long on a tablet"
        );
    }

    /// The bigger controls (D1084) on the owner's iPhone, both ways up:
    /// bigger than the JS's boxes, inside the screen, and upright the
    /// stick at rest clears the pedal panel.
    #[test]
    fn bigger_controls_on_an_iphone() {
        let side = Layout::new(844.0, 390.0, Insets::default());
        let up = Layout::new(390.0, 844.0, Insets::default());
        // The JS's: b 72.8, the slider 241.8 tall and 126.1 wide, full
        // lock 58.5 px, the knob 56.
        assert!(side.b > 80.0 && side.panel.height() > 255.0 && side.stick_r > 66.0);
        assert!(up.panel.height() > 300.0 && up.panel.width() > 140.0);
        assert!(up.stick_r > 72.0 && up.knob >= 72.0);
        assert!(
            up.pedals[2].height() > 1.3 * 72.8 * 1.3,
            "taller pedals upright"
        );
        for lay in [side, up] {
            let mut all = vec![lay.panel, lay.wheel, lay.stick_box()];
            all.extend(lay.dirs);
            all.extend(lay.pedals);
            for r in all {
                assert!(r.left >= 0.0 && r.top >= 0.0 && r.right <= lay.w && r.bottom <= lay.h);
            }
        }
        assert!(up.stick_box().right < up.panel.left);
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
        assert_eq!(t.panel_classes(), ["active", "gas"]);
        // Slide down into the brake band, then right onto DRIFT.
        t.moved(3, cx, at(0.03));
        let s = input.update(1.0 / 120.0, Some(&mut t));
        assert_eq!(s.throttle, 0.0);
        assert_eq!(s.brake, 1.0);
        t.moved(3, lay.drift.left + 10.0, at(0.5));
        let s = input.update(1.0 / 120.0, Some(&mut t));
        assert!(s.handbrake && s.throttle > 0.0);
        // Wandering off to the left: still the pedal thumb.
        t.moved(3, r.left - 70.0, at(0.72));
        let s = input.update(1.0 / 120.0, Some(&mut t));
        assert!(s.throttle == 1.0 && !s.handbrake);
        // Top: N₂O, which holds the gas flat.
        t.moved(3, cx, at(0.95));
        let s = input.update(1.0 / 120.0, Some(&mut t));
        assert!(s.nitro && s.throttle == 1.0);
        t.up(3);
        assert_eq!(input.update(1.0 / 120.0, Some(&mut t)).throttle, 0.0);
        assert!(t.panel_classes().is_empty());
        // The reset button.
        let b = lay.taps[0];
        assert_eq!(t.down(4, b.left + 5.0, b.top + 5.0), Some("reset"));
        assert!(t.buzz, "the haptic tick");
    }

    fn centre(r: Rect) -> (f64, f64) {
        r.centre()
    }

    /// The Buttons choices: ◂ ▸ steer through the ramp; GAS, BRAKE, DRIFT
    /// and N₂O pads; a finger slides from pad to pad; two thumbs at once.
    #[test]
    fn the_buttons_pads_and_sliding_between_them() {
        for (w, h) in [(915.0, 412.0), (412.0, 915.0)] {
            let lay = Layout::new(w, h, Insets::default());
            let mut t = TouchControls::new(lay);
            t.set_mode(Steering::Buttons);
            t.set_pedals(PedalKind::Buttons);
            t.show(true);
            assert_eq!(t.steering, Some(Steering::Buttons));
            // Every pad on screen, big enough, none overlapping.
            let mut all: Vec<(String, Rect)> = t
                .hold_pads()
                .into_iter()
                .map(|(k, r)| (k.name().to_string(), r))
                .collect();
            for (i, r) in lay.taps.iter().enumerate() {
                all.push((TAPS[i].into(), *r));
            }
            for (n, r) in &all {
                assert!(
                    r.left >= 0.0 && r.top >= 0.0 && r.right <= w && r.bottom <= h,
                    "{n}"
                );
                assert!(r.width() >= 40.0 && r.height() >= 40.0, "{n} big enough");
            }
            for (a, ra) in &all {
                for (b, rb) in &all {
                    if a < b {
                        let apart = ra.right <= rb.left
                            || rb.right <= ra.left
                            || ra.bottom <= rb.top
                            || rb.bottom <= ra.top;
                        assert!(apart, "{a} and {b} overlap ({w}×{h})");
                    }
                }
            }
            // Each pad at its centre holds that pad and nothing else.
            for (k, r) in t.hold_pads() {
                let (x, y) = centre(r);
                t.down(9, x, y);
                let held: Vec<Hold> = [
                    Hold::Throttle,
                    Hold::Brake,
                    Hold::Left,
                    Hold::Right,
                    Hold::Handbrake,
                    Hold::Nitro,
                ]
                .into_iter()
                .filter(|h| t.held.get(*h))
                .collect();
                assert_eq!(held, [k], "{w}×{h}");
                t.up(9);
                assert_eq!(t.held, Held::default());
            }
        }
        let lay = Layout::new(915.0, 412.0, Insets::default());
        let mut t = TouchControls::new(lay);
        t.set_mode(Steering::Buttons);
        t.set_pedals(PedalKind::Buttons);
        t.show(true);
        let mut input = Input::new();
        // GAS is on or off; BRAKE likewise.
        let (gx, gy) = centre(lay.pedals[3]);
        t.down(1, gx, gy);
        let s = input.update(0.1, Some(&mut t));
        assert_eq!((s.throttle, s.brake), (1.0, 0.0));
        // Slide onto BRAKE without lifting.
        let (bx, by) = centre(lay.pedals[2]);
        t.moved(1, (gx + bx) / 2.0, (gy + by) / 2.0);
        t.moved(1, bx, by);
        assert!(!t.held.throttle && t.held.brake);
        let s = input.update(0.1, Some(&mut t));
        assert_eq!((s.throttle, s.brake), (0.0, 1.0));
        t.up(1);
        // ◂ then ▸ with the same finger: the ramp, not analogue.
        let (lx, ly) = centre(lay.dirs[0]);
        t.down(2, lx, ly);
        let s = input.update(0.1, Some(&mut t));
        assert!(!s.analog);
        near(s.steer, -0.36, 1e-9);
        let (rx, _) = centre(lay.dirs[1]);
        t.moved(2, rx, ly);
        assert!(!t.held.left && t.held.right);
        // Two thumbs: steer and gas together.
        t.down(3, gx, gy);
        let s = input.update(0.1, Some(&mut t));
        assert_eq!(s.throttle, 1.0);
        assert!(s.steer > -0.36);
        // N₂O on its own holds the gas.
        t.release();
        let (nx, ny) = centre(lay.pedals[1]);
        t.down(4, nx, ny);
        assert!(t.held.nitro && !t.held.throttle);
        let s = input.update(0.1, Some(&mut t));
        assert!(s.nitro && s.throttle == 1.0);
        t.up(4);
        // DRIFT is the handbrake.
        let (dx, dy) = centre(lay.pedals[0]);
        t.down(5, dx, dy);
        assert!(input.update(0.1, Some(&mut t)).handbrake);
        t.up(5);
        // No stick in Buttons mode: a thumb on the left holds nothing.
        t.down(6, 300.0, 150.0);
        assert!(t.stick.is_none());
        assert_eq!(t.held, Held::default());
    }

    #[test]
    fn auto_gas_drives_until_braking_or_a_thumb_on_the_slider() {
        let lay = Layout::new(915.0, 412.0, Insets::default());
        let mut t = TouchControls::new(lay);
        t.show(true);
        t.auto_gas = true;
        let mut input = Input::new();
        assert_eq!(
            input.update(0.1, Some(&mut t)).throttle,
            1.0,
            "no finger down"
        );
        // A thumb in the slider's gap coasts.
        let r = lay.track;
        let cx = (r.left + r.right) / 2.0;
        t.down(1, cx, r.bottom - 0.33 * r.height());
        assert_eq!(input.update(0.1, Some(&mut t)).throttle, 0.0);
        t.up(1);
        assert_eq!(input.update(0.1, Some(&mut t)).throttle, 1.0);
        // BRAKE overrides it (the pedal buttons).
        t.set_pedals(PedalKind::Buttons);
        let (bx, by) = centre(lay.pedals[2]);
        t.down(2, bx, by);
        let s = input.update(0.1, Some(&mut t));
        assert_eq!((s.throttle, s.brake), (0.0, 1.0));
        t.up(2);
        assert_eq!(
            input.update(0.1, Some(&mut t)).throttle,
            1.0,
            "back after BRAKE"
        );
        // Hidden controls: no auto gas.
        t.show(false);
        assert_eq!(input.update(0.1, Some(&mut t)).throttle, 0.0);
    }

    /// Tilt: the stick stands in until the sensor answers, then the wheel
    /// steers; the stick and the ◂ ▸ pads can't be touched meanwhile.
    #[test]
    fn tilt_takes_over_from_the_stick_when_the_sensor_answers() {
        let lay = Layout::new(915.0, 412.0, Insets::default());
        let tilt = Arc::new(Mutex::new(TiltSteer::new()));
        let mut t = TouchControls::new(lay);
        t.tilt = Some(tilt.clone());
        t.set_mode(Steering::Tilt);
        t.show(true);
        let mut input = Input::new();
        tilt.lock().unwrap().enable(true, &mut NoSensor, 0.0);
        input.update(0.1, Some(&mut t));
        assert_eq!(t.steering, Some(Steering::Stick), "no sensor: the stick");
        t.down(1, 200.0, 300.0);
        t.moved(1, 240.0, 300.0);
        assert!(
            input.update(0.1, Some(&mut t)).steer > 0.0,
            "the stick steers"
        );
        // A sensor that answers later takes over (and lets go of the stick).
        // Sideways (angle 90), its top end lowered 20°: a left turn.
        tilt.lock()
            .unwrap()
            .on_orientation(Some(-20.0), Some(-90.0), 90.0);
        let s = input.update(1.0 / 120.0, Some(&mut t));
        assert_eq!(t.steering, Some(Steering::Tilt));
        assert!(t.stick.is_none());
        assert!(s.analog && s.steer < 0.0, "the phone turned left");
        for _ in 0..120 {
            input.update(1.0 / 120.0, Some(&mut t));
        }
        let full = input.state.steer;
        assert!(full < -0.5);
        near(t.wheel, full * 90.0, 1e-12);
        // A corner touch holds no pad, and no stick starts.
        t.down(2, 4.0, 4.0);
        t.down(3, 200.0, 300.0);
        assert_eq!(t.held, Held::default());
        assert!(t.stick.is_none());
    }
}
