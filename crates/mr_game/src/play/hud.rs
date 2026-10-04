//! The race's HUD (roadmap WP 6.3): `HUD.js` and the HUD half of
//! `hud.css`, in Bevy UI. [`model`] is the `HUD` class without the DOM
//! (what each element shows); this file lays the elements out as the CSS
//! does at its breakpoints (desktop, `max-width: 720px`, the touch layout
//! in landscape and portrait) and puts the model's view on them each frame
//! without rebuilding nodes: texts are written only when they change (the
//! class's `set`), widths and colours likewise; the nodes are built again
//! only when the screen, the race or its kind changes.
//!
//! The canvases (the rev counter or power meter, the minimap) and the
//! speed lines are one UI material ([`material`], `hud.wgsl`): signed
//! distances in a fragment shader in place of the canvas's paths, with the
//! dial's labels and needle as nodes above it. The CSS animations (the
//! centre pop, the zone card, the toast's fade, the nitro pulse) are
//! computed here from the real clock, as the browser runs them.
//!
//! The Hot Pursuit furniture (heat stars and the bust/evade bar, the
//! damage bar, the penalty line, the hold card, the radio line) is M8's:
//! the model already keeps its state (`Hud::update_pursuit`, `radio`), and
//! the places its nodes go are marked `M8:` below, in `index.html`'s order.

mod dials;
mod material;
mod minimap;
pub mod model;

use super::Play;
use super::flow::Mode;
use super::touch::Layout;
use crate::ui::widgets::{self, Bp, T};
use bevy::prelude::*;
use bevy::text::{Justify, LineBreak, LineHeight, TextLayout};
use bevy::ui::widget::TextShadow;
use bevy::ui::{
    BackgroundGradient, BoxShadow, ColorStop, LinearGradient, RadialGradient,
    RadialGradientShape, ShadowStyle, UiPosition, UiTransform, Val2,
};
use bevy::ui_render::prelude::MaterialNode;
use bevy::window::PrimaryWindow;
use material::HudMaterial;
use model::{CruiseIn, Dial, Dot, HudIn, LapsIn, RivalDot};
use mr_sim::race::{RaceStateKind, standings};

pub(super) fn plugin(app: &mut App) {
    material::plugin(app);
}

// ── Elements ───────────────────────────────────────────────────────────

/// The nodes the update touches, by their DOM ids.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum El {
    /// `.pos` (hidden in a cruise).
    PosRow,
    Pos,
    Suf,
    Of,
    Time,
    /// `#hud-lap` and its parts.
    LapN,
    LapTime,
    LapBest,
    Zone,
    Score,
    Mult,
    MultFill,
    Dist,
    Best,
    Minimap,
    /// `.rdot`, by racer.
    Dot(usize),
    Dial,
    Needle,
    Speed,
    Unit,
    Gear,
    NitroFill,
    /// The `N₂O` label's three parts.
    NitroLabel,
    CenterBox,
    Center,
    Toast,
    ZoneCard,
    ZcName,
    ZcSub,
    Speedlines,
}

/// The HUD's root.
#[derive(Component)]
pub(super) struct HudRoot;

/// What the nodes were built for: rebuilt when it changes.
#[derive(Clone, Debug, PartialEq)]
struct Key {
    bp: Bp,
    starts: u32,
    level: &'static str,
    cruise: bool,
    laps: bool,
    electric: bool,
    racers: Vec<u32>,
    steer_top: f32,
    in_l: f32,
    in_r: f32,
}

/// A CSS transition's state: from, to, when it started.
#[derive(Clone, Copy, Debug, Default)]
struct Fade {
    from: f64,
    to: f64,
    t0: f64,
}

impl Fade {
    fn at(&self, t: f64, dur: f64) -> f64 {
        let u = ((t - self.t0) / dur).clamp(0.0, 1.0);
        self.from + (self.to - self.from) * EASE.at(u)
    }
    fn go(&mut self, to: f64, t: f64, dur: f64) {
        if to != self.to {
            *self = Fade {
                from: self.at(t, dur),
                to,
                t0: t,
            };
        }
    }
}

#[derive(Resource, Default)]
pub(super) struct HudState {
    model: Option<model::Hud>,
    key: Option<Key>,
    root: Option<Entity>,
    /// The race start the model is for.
    starts: u32,
    /// `race.hud`'s call counts already passed on.
    centers: u32,
    toasts: u32,
    /// When the centre pop and the zone card last started (real time), and
    /// the counts they started at.
    center_t0: f64,
    center_seq: u32,
    zone_t0: f64,
    zone_seq: u32,
    toast: Fade,
    nitro_t0: Option<f64>,
    mats: Option<[Handle<HudMaterial>; 3]>,
    last_dial: Option<dials::DialDraw>,
    last_scene: Option<minimap::Scene>,
    last_lines: f64,
    /// The touch layout's `--steer-top`, CSS px.
    shown: bool,
}

pub(super) fn spawn(mut commands: Commands, play: Option<Res<Play>>) {
    if play.is_some() {
        commands.insert_resource(HudState::default());
    }
}

// ── CSS timing ─────────────────────────────────────────────────────────

/// `cubic-bezier(x1, y1, x2, y2)`.
#[derive(Clone, Copy, Debug)]
pub struct Bezier(f64, f64, f64, f64);

/// `ease` and `ease-out`.
pub const EASE: Bezier = Bezier(0.25, 0.1, 0.25, 1.0);
pub const EASE_OUT: Bezier = Bezier(0.0, 0.0, 0.58, 1.0);

impl Bezier {
    pub fn at(&self, x: f64) -> f64 {
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }
        let c = |a: f64, b: f64, t: f64| {
            3.0 * a * (1.0 - t) * (1.0 - t) * t + 3.0 * b * (1.0 - t) * t * t + t * t * t
        };
        // Bisection on x(t): monotonic for these curves.
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..40 {
            let m = (lo + hi) / 2.0;
            if c(self.0, self.2, m) < x {
                lo = m;
            } else {
                hi = m;
            }
        }
        c(self.1, self.3, (lo + hi) / 2.0)
    }
}

/// A keyframe track: `(offset, value)`, each interval eased by `f`.
pub fn keyframes(k: &[(f64, f64)], u: f64, f: Bezier) -> f64 {
    let u = u.clamp(0.0, 1.0);
    for w in k.windows(2) {
        let ((a, va), (b, vb)) = (w[0], w[1]);
        if u <= b {
            let p = if b > a { (u - a) / (b - a) } else { 1.0 };
            return va + (vb - va) * f.at(p);
        }
    }
    k.last().map_or(0.0, |x| x.1)
}

/// `@keyframes pop` over .8 s: scale and opacity.
pub fn pop(t: f64) -> (f64, f64) {
    let u = t / 0.8;
    (
        keyframes(&[(0.0, 1.8), (0.2, 1.0), (1.0, 0.9)], u, EASE_OUT),
        keyframes(&[(0.0, 0.0), (0.2, 1.0), (1.0, 0.0)], u, EASE_OUT),
    )
}

/// `@keyframes zoneCard` over 3.6 s: x offset (CSS px) and opacity.
pub fn zone_card(t: f64) -> (f64, f64) {
    let u = t / 3.6;
    (
        keyframes(&[(0.0, -40.0), (0.12, 0.0), (1.0, 30.0)], u, EASE_OUT),
        keyframes(
            &[(0.0, 0.0), (0.12, 1.0), (0.8, 1.0), (1.0, 0.0)],
            u,
            EASE_OUT,
        ),
    )
}

/// `@keyframes nitroPulse` (.18 s, alternate, `ease`): the brightness.
pub fn nitro_pulse(t: f64) -> f64 {
    let p = t / 0.18;
    let i = p.floor();
    let f = p - i;
    let d = if (i as i64) % 2 == 0 { f } else { 1.0 - f };
    1.0 + 0.7 * EASE.at(d)
}

// ── Layout (`hud.css`) ─────────────────────────────────────────────────

/// Where things go for a screen, CSS px.
#[derive(Clone, Copy, Debug)]
struct Lay {
    touch: bool,
    narrow: bool,
    portrait: bool,
    w: f32,
    tl: (f32, f32),
    pos: f32,
    sup: f32,
    time: f32,
    lap: f32,
    lap_best: f32,
    zone: f32,
    /// `#minimap`: right, top, size.
    mini: (f32, f32, f32),
    in_l: f32,
    steer_top: f32,
    center: f32,
    toast: f32,
    zc: (f32, f32),
}

impl Lay {
    fn new(bp: &Bp, in_l: f32, in_r: f32, top: f32, steer_top: f32) -> Lay {
        let touch = bp.touch;
        let narrow = bp.narrow;
        Lay {
            touch,
            narrow,
            portrait: bp.portrait,
            w: bp.w,
            tl: if touch { (in_l, 8.0) } else { (28.0, 22.0) },
            pos: if touch {
                40.0
            } else if narrow {
                44.0
            } else {
                64.0
            },
            sup: if touch { 18.0 } else { 26.0 },
            time: if touch { 20.0 } else { 28.0 },
            lap: if touch { 15.0 } else { 20.0 },
            lap_best: if touch { 12.0 } else { 15.0 },
            zone: if touch { 11.0 } else { 14.0 },
            mini: if touch {
                (in_r, top.max(8.0), 104.0)
            } else if narrow {
                (26.0, 22.0, 120.0)
            } else {
                (26.0, 22.0, 190.0)
            },
            in_l,
            steer_top,
            center: if touch { 84.0 } else { 120.0 },
            toast: if touch { 20.0 } else { 26.0 },
            zc: if touch { (40.0, 13.0) } else { (64.0, 18.0) },
        }
    }
}

/// `--steer-top`: the top of the steering under the left thumb.
fn steer_top(lay: &Layout, steering: &str) -> f32 {
    let in_b = (lay.h - lay.stick_home.1 - 0.6 * lay.b) as f32;
    let b = lay.b as f32;
    match steering {
        // body.touch:has(#touch.buttons) / :has(#touch.tilt)
        "buttons" => in_b + b * 1.08,
        "tilt" => in_b + b * 1.3 + 8.0,
        _ => in_b + b * 0.6 + 32.0,
    }
}

// ── Building ───────────────────────────────────────────────────────────

/// Rajdhani's ascent and its normal line height, in em.
const ASCENT: f32 = 0.93;
const LINE: f32 = 1.276;

/// Where the baseline sits in a line box of `lh` em at `size` px.
fn baseline(size: f32, lh: f32) -> f32 {
    (lh - LINE) * size / 2.0 + ASCENT * size
}

/// `text-shadow: 0 y blur rgba(0,0,0,a)`: Bevy's text shadow has no blur,
/// so a hard copy, its alpha thinned as the blur spreads it (D822).
fn tshadow(k: f32, y: f32, blur: f32, a: f32) -> TextShadow {
    TextShadow {
        offset: Vec2::new(0.0, y.max(1.0) * k),
        color: Color::srgba(0.0, 0.0, 0.0, a * (4.0 / blur.max(4.0))),
    }
}

fn txt(s: impl Into<String>, t: T, k: f32) -> impl Bundle {
    (
        Text::new(s),
        t.font(k),
        TextColor(t.color),
        t.spacing(k),
        LineHeight::RelativeToFont(t.line),
        TextLayout::new(Justify::Left, LineBreak::NoWrap),
    )
}

fn abs() -> Node {
    Node {
        position_type: PositionType::Absolute,
        ..default()
    }
}

/// A full-width row at `top`, its content centred.
fn band(top: Val) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(0.0),
        right: Val::Px(0.0),
        top,
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        ..default()
    }
}

fn css_color(s: &str) -> Color {
    let h = s.trim_start_matches('#');
    let v = u32::from_str_radix(h, 16).unwrap_or(0x888888);
    match h.len() {
        3 => {
            let (r, g, b) = ((v >> 8) & 15, (v >> 4) & 15, v & 15);
            widgets::rgb((r * 17) << 16 | (g * 17) << 8 | b * 17)
        }
        6 => widgets::rgb(v),
        _ => widgets::rgb(0x888888),
    }
}

/// `name.toLowerCase().replace(/\b\w/g, c => c.toUpperCase())`, then the
/// CSS's `text-transform: uppercase`: the name in capitals.
fn seg_name(s: &str) -> String {
    s.to_uppercase()
}

struct Build<'a> {
    lay: Lay,
    k: f32,
    track: &'a mr_track::track::Track,
    key: &'a Key,
    racer_colors: &'a [u32],
    mats: &'a [Handle<HudMaterial>; 3],
    labels: &'a [dials::Label],
}

fn build(commands: &mut Commands, b: &Build) -> Entity {
    let (k, lay) = (b.k, b.lay);
    let fg = widgets::fg();
    let dim = widgets::dim();
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                ..default()
            },
            GlobalZIndex(-1),
            HudRoot,
        ))
        .id();
    let px = |v: f32| Val::Px(v * k);

    // .hud-tl: position, clock, lap, penalty, zone.
    let sh = tshadow(k, 2.0, 8.0, 0.6);
    let tl = commands
        .spawn((
            Node {
                left: px(lay.tl.0),
                top: px(lay.tl.1),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                ..abs()
            },
            ChildOf(root),
        ))
        .id();
    {
        let big = T::new(lay.pos).w(800).italic().lh(0.9).ls(-1.0 / lay.pos);
        let small = T::new(lay.sup).w(800).italic().lh(0.9);
        let base = baseline(lay.pos, 0.9);
        let row = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::FlexStart,
                    height: px(0.9 * lay.pos),
                    display: if b.key.cruise {
                        Display::None
                    } else {
                        Display::Flex
                    },
                    ..default()
                },
                El::PosRow,
                ChildOf(tl),
            ))
            .id();
        commands.spawn((txt("1", big, k), sh, El::Pos, ChildOf(row)));
        // `sup`: vertical-align top, then 8 px down.
        commands.spawn((
            txt("st", small, k),
            sh,
            Node {
                margin: UiRect {
                    left: px(2.0),
                    top: px(8.0),
                    ..default()
                },
                ..default()
            },
            El::Suf,
            ChildOf(row),
        ));
        // `.of`, on the baseline.
        commands.spawn((
            txt(b.racer_colors.len().to_string(), small.c(dim), k),
            sh,
            Node {
                margin: UiRect {
                    left: px(6.0),
                    top: px(base - baseline(lay.sup, 0.9)),
                    ..default()
                },
                ..default()
            },
            El::Of,
            ChildOf(row),
        ));
    }
    commands.spawn((
        txt("0:00.00", T::new(lay.time).w(600), k),
        sh,
        Node {
            margin: UiRect::top(px(4.0)),
            ..default()
        },
        El::Time,
        ChildOf(tl),
    ));
    if b.key.laps {
        let lap = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    margin: UiRect::top(px(2.0)),
                    ..default()
                },
                ChildOf(tl),
            ))
            .id();
        let line = commands
            .spawn((Node::default(), ChildOf(lap)))
            .id();
        commands.spawn((
            txt("LAP 1/3", T::new(lay.lap).w(800).italic().ls(0.04), k),
            sh,
            Node {
                margin: UiRect::right(px(4.0)),
                ..default()
            },
            El::LapN,
            ChildOf(line),
        ));
        commands.spawn((
            txt(" 0:00.00", T::new(lay.lap).w(600), k),
            sh,
            El::LapTime,
            ChildOf(line),
        ));
        commands.spawn((
            txt("", T::new(lay.lap_best).w(600).ls(0.04).c(dim), k),
            sh,
            Node {
                display: Display::None,
                ..default()
            },
            El::LapBest,
            ChildOf(lap),
        ));
    }
    // M8: `#hud-pen`, the penalty served, here.
    // (On touch, the lap line takes the zone name's place.)
    if !(lay.touch && b.key.laps) {
        commands.spawn((
            txt("", T::new(lay.zone).ls(0.3).c(dim), k),
            sh,
            Node {
                margin: UiRect::top(px(2.0)),
                ..default()
            },
            El::Zone,
            ChildOf(tl),
        ));
    }

    // .hud-cruise: top centre (scaled .7 on touch).
    if b.key.cruise {
        let s = if lay.touch { 0.7 } else { 1.0 };
        let kc = k * s;
        let pc = |v: f32| Val::Px(v * kc);
        let shc = tshadow(kc, 2.0, 10.0, 0.6);
        let c = commands
            .spawn((
                Node {
                    left: Val::Percent(50.0),
                    top: px(if lay.touch { 6.0 } else { 14.0 }),
                    min_width: pc(260.0),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    ..abs()
                },
                UiTransform::from_translation(Val2::percent(-50.0, 0.0)),
                ChildOf(root),
            ))
            .id();
        commands.spawn((txt("SCORE", T::new(12.0).ls(0.5).c(dim), kc), shc, ChildOf(c)));
        commands.spawn((
            txt("0", T::new(56.0).w(800).italic().lh(1.0), kc),
            shc,
            El::Score,
            ChildOf(c),
        ));
        let m = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    column_gap: pc(10.0),
                    margin: UiRect::top(pc(4.0)),
                    ..default()
                },
                ChildOf(c),
            ))
            .id();
        commands.spawn((
            txt("×1", T::new(26.0).w(800).c(widgets::gold()), kc),
            shc,
            El::Mult,
            ChildOf(m),
        ));
        let bar = commands
            .spawn((
                Node {
                    width: pc(120.0),
                    height: pc(6.0),
                    border_radius: BorderRadius::all(pc(3.0)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(widgets::white(0.14)),
                ChildOf(m),
            ))
            .id();
        commands.spawn((
            Node {
                height: Val::Percent(100.0),
                width: Val::Percent(0.0),
                ..default()
            },
            widgets::hgrad(widgets::rgb(0xff9a3c), widgets::gold()),
            El::MultFill,
            ChildOf(bar),
        ));
        let st = T::new(15.0).ls(0.08).c(dim);
        let stats = commands
            .spawn((
                Node {
                    margin: UiRect::top(pc(2.0)),
                    ..default()
                },
                ChildOf(c),
            ))
            .id();
        commands.spawn((txt("0.0 mi", st, kc), shc, El::Dist, ChildOf(stats)));
        commands.spawn((txt(" · best ", st, kc), shc, ChildOf(stats)));
        commands.spawn((txt("0", st, kc), shc, El::Best, ChildOf(stats)));
    }

    // #minimap.
    {
        let (right, top, size) = lay.mini;
        let r = size / 2.0 * std::f32::consts::SQRT_2;
        commands.spawn((
            Node {
                right: px(right),
                top: px(top),
                width: px(size),
                height: px(size),
                border_radius: BorderRadius::MAX,
                ..abs()
            },
            BackgroundGradient::from(
                RadialGradient::new(
                    UiPosition::CENTER,
                    RadialGradientShape::Circle(px(r)),
                    vec![
                        ColorStop::auto(widgets::rgba(0x0a0e18, 0.72)),
                        ColorStop::auto(widgets::rgba(0x0a0e18, 0.55)),
                    ],
                )
                .in_srgb(),
            ),
            // `box-shadow: 0 0 0 2px rgba(255,255,255,.18), 0 6px 24px
            // rgba(0,0,0,.4)`: the ring as an outline; the soft shadow is
            // left out (Bevy draws it under the translucent disc too).
            Outline::new(px(2.0), Val::Px(0.0), widgets::white(0.18)),
            MaterialNode(b.mats[1].clone()),
            El::Minimap,
            ChildOf(root),
        ));
    }

    // M8: `#hud-pz` (heat stars, the bust/evade bar) here.

    // .hud-route: one segment per zone, widths following the zone lengths.
    if !b.key.cruise {
        let (node, tf) = if lay.touch {
            (
                Node {
                    left: Val::Percent(50.0),
                    top: px(if lay.portrait { 166.0 } else { 14.0 }),
                    width: px(if lay.portrait {
                        lay.w * 0.7
                    } else {
                        300f32.min(lay.w * 0.3)
                    }),
                    ..abs()
                },
                UiTransform::from_translation(Val2::percent(-50.0, 0.0)),
            )
        } else if lay.narrow {
            (
                Node {
                    left: px(16.0),
                    bottom: px(16.0),
                    width: px(lay.w * 0.5),
                    ..abs()
                },
                UiTransform::IDENTITY,
            )
        } else {
            (
                Node {
                    left: Val::Percent(50.0),
                    top: px(20.0),
                    width: px(520f32.min(lay.w * 0.46)),
                    ..abs()
                },
                UiTransform::from_translation(Val2::percent(-50.0, 0.0)),
            )
        };
        let route = commands.spawn((node, tf, ChildOf(root))).id();
        let bar = commands
            .spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: px(8.0),
                    flex_direction: FlexDirection::Row,
                    border_radius: BorderRadius::all(px(4.0)),
                    ..default()
                },
                BackgroundColor(widgets::white(0.08)),
                ChildOf(route),
            ))
            .id();
        let zones = &b.track.zones;
        for (i, z) in zones.iter().enumerate() {
            let last = i == zones.len() - 1;
            let mut radius = BorderRadius::ZERO;
            if i == 0 {
                radius.top_left = px(4.0);
                radius.bottom_left = px(4.0);
            }
            if last {
                radius.top_right = px(4.0);
                radius.bottom_right = px(4.0);
            }
            let color = if z.zone.color.is_empty() {
                "#888"
            } else {
                z.zone.color
            };
            let seg = commands
                .spawn((
                    Node {
                        flex_grow: (z.s1 - z.s0).max(1.0) as f32,
                        flex_shrink: 0.0,
                        flex_basis: Val::Px(0.0),
                        height: Val::Percent(100.0),
                        border: UiRect::right(if last { Val::Px(0.0) } else { px(2.0) }),
                        border_radius: radius,
                        ..default()
                    },
                    BackgroundColor(css_color(color)),
                    BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.5)),
                    ChildOf(bar),
                ))
                .id();
            commands.spawn((
                txt(seg_name(z.zone.name), T::new(11.0).ls(0.15).c(dim), k),
                Node {
                    left: Val::Px(0.0),
                    top: px(12.0),
                    ..abs()
                },
                ChildOf(seg),
            ));
        }
        // #route-dots: the player's on top.
        let dots = commands
            .spawn((
                Node {
                    left: Val::Px(0.0),
                    right: Val::Px(0.0),
                    top: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    ..abs()
                },
                ChildOf(bar),
            ))
            .id();
        for (i, c) in b.racer_colors.iter().enumerate() {
            let me = i == 0;
            let s = if me { 14.0 } else { 10.0 };
            commands.spawn((
                Node {
                    top: Val::Percent(50.0),
                    left: Val::Percent(0.0),
                    width: px(s),
                    height: px(s),
                    margin: UiRect {
                        left: px(-s / 2.0),
                        top: px(-s / 2.0),
                        ..default()
                    },
                    border: UiRect::all(px(2.0)),
                    border_radius: BorderRadius::MAX,
                    ..abs()
                },
                BackgroundColor(widgets::rgb(*c)),
                BorderColor::all(if me {
                    Color::WHITE
                } else {
                    widgets::rgb(0x111111)
                }),
                ZIndex(if me { 2 } else { 0 }),
                El::Dot(i),
                ChildOf(dots),
            ));
        }
    }

    // .hud-br: the dial, speed, gear and nitro (a compact box on touch).
    build_br(commands, b, root);

    // #hud-center, #hud-toast.
    let center_box = commands
        .spawn((band(Val::Percent(34.0)), El::CenterBox, ChildOf(root)))
        .id();
    commands.spawn((
        txt("", T::new(lay.center).w(800).italic(), k),
        tshadow(k, 4.0, 30.0, 0.6),
        El::Center,
        ChildOf(center_box),
    ));
    let toast = commands
        .spawn((band(Val::Percent(24.0)), ChildOf(root)))
        .id();
    commands.spawn((
        txt(
            "",
            T::new(lay.toast).bold().ls(0.12).c(widgets::rgb(0x38b6ff)),
            k,
        ),
        // `text-shadow: 0 0 14px rgba(56,182,255,.6)`: a glow round the
        // glyphs, which a hard shadow cannot be; left out.
        El::Toast,
        ChildOf(toast),
    ));
    // M8: `#hud-radio` and `#hud-hold` here.
    // #zone-card.
    let zc = commands
        .spawn((band(Val::Percent(16.0)), El::ZoneCard, ChildOf(root)))
        .id();
    commands.spawn((
        txt("", T::new(lay.zc.0).w(800).italic().ls(0.06), k),
        tshadow(k, 4.0, 30.0, 0.7),
        El::ZcName,
        ChildOf(zc),
    ));
    commands.spawn((
        txt("", T::new(lay.zc.1).ls(0.5).c(dim), k),
        El::ZcSub,
        ChildOf(zc),
    ));
    // #speedlines, over the rest.
    commands.spawn((
        Node {
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            ..abs()
        },
        MaterialNode(b.mats[2].clone()),
        Visibility::Hidden,
        El::Speedlines,
        ChildOf(root),
    ));
    let _ = fg;
    root
}

fn build_br(commands: &mut Commands, b: &Build, root: Entity) {
    let lay = b.lay;
    let px0 = |v: f32| Val::Px(v * b.k);
    if lay.touch {
        // A compact digital speedo just above the steering.
        let k = b.k;
        let px = px0;
        let br = commands
            .spawn((
                Node {
                    left: px(lay.in_l),
                    bottom: px(lay.steer_top + 8.0),
                    width: px(176.0),
                    height: px(64.0),
                    border_radius: BorderRadius::all(px(14.0)),
                    ..abs()
                },
                BackgroundColor(widgets::rgba(0x080a12, 0.45)),
                ChildOf(root),
            ))
            .id();
        // M8: `.hud-br.pz-on` grows the box to 74 px for the damage bar.
        let num = T::new(42.0).w(800).italic().lh(1.0);
        let unit = T::new(11.0).w(800).ls(0.2).lh(1.0).c(widgets::dim());
        let sh = tshadow(k, 2.0, 10.0, 0.6);
        let row = commands
            .spawn((
                Node {
                    left: Val::Px(0.0),
                    right: Val::Px(0.0),
                    top: px(2.0),
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::FlexStart,
                    ..abs()
                },
                ChildOf(br),
            ))
            .id();
        commands.spawn((txt("0", num, k), sh, El::Speed, ChildOf(row)));
        commands.spawn((
            txt("MPH", unit, k),
            sh,
            Node {
                margin: UiRect {
                    left: px(6.0),
                    top: px(baseline(42.0, 1.0) - baseline(11.0, 1.0)),
                    ..default()
                },
                ..default()
            },
            El::Unit,
            ChildOf(row),
        ));
        commands.spawn((
            txt("N", T::new(22.0).w(800).c(widgets::accent2()), k),
            Node {
                top: px(12.0),
                right: px(12.0),
                ..abs()
            },
            El::Gear,
            ChildOf(br),
        ));
        nitro(commands, b.k, br, (12.0, 12.0, 7.0, 7.0), false);
        return;
    }
    // Scaled .7 from the bottom right under 720 px.
    let s = if lay.narrow { 0.7 } else { 1.0 };
    let k = b.k * s;
    let px = |v: f32| Val::Px(v * k);
    let br = commands
        .spawn((
            Node {
                right: px0(26.0),
                bottom: px0(20.0),
                width: px(260.0),
                height: px(260.0),
                ..abs()
            },
            ChildOf(root),
        ))
        .id();
    commands.spawn((
        Node {
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            width: px(260.0),
            height: px(260.0),
            ..abs()
        },
        MaterialNode(b.mats[0].clone()),
        El::Dial,
        ChildOf(br),
    ));
    // The dial's labels (`fillText`, centred on their point).
    for l in b.labels {
        commands.spawn((
            txt(
                l.text.clone(),
                T::new(l.size as f32)
                    .w(l.weight)
                    .c(widgets::rgba(l.color, l.alpha as f32)),
                k,
            ),
            Node {
                left: px(l.x as f32),
                top: px(l.y as f32),
                ..abs()
            },
            UiTransform::from_translation(Val2::percent(-50.0, -50.0)),
            ChildOf(br),
        ));
    }
    // The needle: 30 px out to R − 4, 3 px wide, turned about the centre.
    let (n0, n1, nw) = dials::NEEDLE;
    let len = (n1 - n0) as f32;
    commands.spawn((
        Node {
            left: px(dials::CX as f32 - len / 2.0),
            top: px(dials::CY as f32 - nw as f32 / 2.0),
            width: px(len),
            height: px(nw as f32),
            ..abs()
        },
        BackgroundColor(widgets::accent()),
        UiTransform::IDENTITY,
        El::Needle,
        ChildOf(br),
    ));
    let sh = tshadow(k, 2.0, 10.0, 0.6);
    let speed = commands
        .spawn((
            Node {
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: px(92.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..abs()
            },
            ChildOf(br),
        ))
        .id();
    commands.spawn((
        txt("0", T::new(68.0).w(800).italic().lh(1.0), k),
        sh,
        El::Speed,
        ChildOf(speed),
    ));
    commands.spawn((
        txt("MPH", T::new(15.0).w(800).ls(0.3).lh(1.0).c(widgets::dim()), k),
        sh,
        Node {
            margin: UiRect::top(px(2.0)),
            ..default()
        },
        El::Unit,
        ChildOf(speed),
    ));
    let gear = commands
        .spawn((
            Node {
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: px(182.0),
                justify_content: JustifyContent::Center,
                ..abs()
            },
            ChildOf(br),
        ))
        .id();
    commands.spawn((
        txt("N", T::new(30.0).w(800).c(widgets::accent2()), k),
        El::Gear,
        ChildOf(gear),
    ));
    nitro(commands, k, br, (40.0, 40.0, -4.0, 10.0), true);
    // M8: `#hud-dmg`, the damage bar, here.
}

/// `.nitro`: left, right, bottom, height; with its `N₂O` label.
fn nitro(commands: &mut Commands, k: f32, br: Entity, g: (f32, f32, f32, f32), label: bool) {
    let px = |v: f32| Val::Px(v * k);
    let bar = commands
        .spawn((
            Node {
                left: px(g.0),
                right: px(g.1),
                bottom: px(g.2),
                height: px(g.3),
                border_radius: BorderRadius::all(px(5.0)),
                overflow: Overflow::clip(),
                ..abs()
            },
            BackgroundColor(widgets::white(0.12)),
            ChildOf(br),
        ))
        .id();
    commands.spawn((
        Node {
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            width: Val::Percent(50.0),
            ..abs()
        },
        nitro_grad(1.0),
        BoxShadow(vec![ShadowStyle {
            color: widgets::rgb(0x38b6ff),
            x_offset: Val::Px(0.0),
            y_offset: Val::Px(0.0),
            spread_radius: Val::Px(0.0),
            blur_radius: px(6.0),
        }]),
        El::NitroFill,
        ChildOf(bar),
    ));
    if label {
        // `N₂O`, 10 px bold, right 6, top -2; Rajdhani has no ₂, so a
        // small 2 set low (D821).
        let row = commands
            .spawn((
                Node {
                    right: px(6.0),
                    top: px(-2.0),
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::FlexStart,
                    ..abs()
                },
                ChildOf(bar),
            ))
            .id();
        let t = T::new(10.0).bold().c(Color::WHITE);
        commands.spawn((txt("N", t, k), El::NitroLabel, ChildOf(row)));
        commands.spawn((
            txt("2", T::new(6.5).bold().c(Color::WHITE), k),
            Node {
                margin: UiRect::top(px(5.5)),
                ..default()
            },
            El::NitroLabel,
            ChildOf(row),
        ));
        commands.spawn((txt("O", t, k), El::NitroLabel, ChildOf(row)));
    }
}

/// `.nitro-fill`'s gradient, under `filter: brightness(b)`.
fn nitro_grad(b: f32) -> BackgroundGradient {
    let c = |v: u32| {
        let s = widgets::rgb(v).to_srgba();
        Color::srgb(
            (s.red * b).min(1.0),
            (s.green * b).min(1.0),
            (s.blue * b).min(1.0),
        )
    };
    BackgroundGradient::from(
        LinearGradient::to_right(vec![
            ColorStop::auto(c(0x1b6cff)),
            ColorStop::auto(c(0x38b6ff)),
            ColorStop::auto(c(0xb0f0ff)),
        ])
        .in_srgb(),
    )
}

// ── The frame ──────────────────────────────────────────────────────────

/// `hud.update`'s `st` from the race (Race.js `updateHud`).
fn hud_in(race: &super::flow::Race) -> HudIn {
    let st = &race.session.curr;
    let p = &st.players[0];
    let t = &race.session.lr.track;
    let laps = st.race.laps > 0;
    // (On a circuit's grid, behind the line, you're at the start of lap
    // 1.)
    let lap_s = |prog: Option<f64>, s: f64| {
        if laps && prog.is_some_and(|g| g < t.start_s) {
            t.start_s
        } else {
            s
        }
    };
    let list = standings(st);
    let position = list.iter().position(|r| r.player).map_or(0, |i| i + 1);
    let s = lap_s(p.v.prog, p.v.s);
    let mut racers = vec![s];
    racers.extend(st.rivals.iter().map(|a| lap_s(a.prog, a.k.s)));
    let rivals = list
        .iter()
        .filter(|r| !r.player)
        .filter_map(|r| {
            st.rivals
                .iter()
                .find(|a| a.name == r.name && a.color == r.color)
                .map(|a| RivalDot {
                    x: a.k.v.x,
                    z: a.k.v.z,
                    color: a.color,
                })
        })
        .collect();
    let r = &p.rules;
    HudIn {
        position,
        time: if r.finished {
            r.finish_time
        } else {
            Some(st.race.time)
        },
        speed: mr_math::kernel::hypot(p.v.vx, p.v.vz),
        gear: p.phys.gear,
        rpm: p.phys.rpm,
        nitro: p.phys.nitro,
        nitro_active: p.phys.nitro_active,
        electric: p.phys.electric,
        power: p.phys.power_out,
        s,
        started: st.race.state != RaceStateKind::Countdown,
        racers,
        laps: laps.then(|| LapsIn {
            lap: r.lap.min(st.race.laps as i32),
            of: st.race.laps,
            time: if r.finished {
                None
            } else {
                Some(st.race.time - r.lap_start)
            },
            best: (!r.lap_times.is_empty())
                .then(|| r.lap_times.iter().copied().fold(f64::INFINITY, f64::min)),
        }),
        player: (p.v.x, p.v.z, p.v.yaw),
        traffic: st
            .traffic
            .cars
            .iter()
            .filter(|c| c.active)
            .map(|c| Dot {
                x: c.k.v.x,
                z: c.k.v.z,
            })
            .collect(),
        rivals,
        cruise: st.race.cruise.then_some(CruiseIn {
            score: r.score,
            mult: r.mult,
            mult_timer: r.mult_timer,
            dist: r.dist,
        }),
        // M8: `pv.hudState()`.
        pursuit: None,
    }
}

type Parts<'a> = (
    &'a El,
    Option<&'a mut Text>,
    Option<&'a mut TextColor>,
    Option<&'a mut TextShadow>,
    Option<&'a mut TextFont>,
    Option<&'a mut Node>,
    Option<&'a mut UiTransform>,
    Option<&'a mut Visibility>,
    Option<&'a mut BackgroundGradient>,
    Option<&'a mut BackgroundColor>,
);

fn set_text(t: &mut Option<Mut<Text>>, s: &str) {
    if let Some(t) = t
        && t.0 != s
    {
        t.0 = s.to_string();
    }
}

fn set_vis(v: &mut Option<Mut<Visibility>>, on: bool) {
    let want = if on {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if let Some(v) = v
        && **v != want
    {
        **v = want;
    }
}

fn with_alpha(c: Color, a: f32) -> Color {
    c.with_alpha(a)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn update(
    mut commands: Commands,
    play: Res<Play>,
    ui: Option<Res<crate::ui::UiState>>,
    time: Res<Time<Real>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    hs: Option<ResMut<HudState>>,
    mut mats: ResMut<Assets<HudMaterial>>,
    mut roots: Query<(Entity, &mut Visibility), (With<HudRoot>, Without<El>)>,
    mut parts: Query<Parts>,
) {
    let Some(mut hs) = hs else { return };
    let hs = &mut *hs;
    // No race (the menu), or one waiting for its pipelines behind it: no
    // HUD.
    let Some(race) = play.race.as_ref().filter(|_| !play.hold) else {
        if hs.shown {
            for (_, mut v) in &mut roots {
                *v = Visibility::Hidden;
            }
            hs.shown = false;
        }
        return;
    };
    let Ok(w) = windows.single() else { return };
    let now = time.elapsed_secs_f64();
    let css = play.css_scale.max(0.01);
    let bp = Bp::new(
        w.width() * css,
        w.height() * css,
        1.0 / css,
        play.touch_ui,
        play.insets.top as f32,
    );
    let st = &race.session.curr;
    let track = &race.session.lr.track;
    let steering = ui
        .as_ref()
        .map_or("stick".to_string(), |u| u.settings.steering.clone());
    let mut colors = vec![st.players[0].spec.color];
    colors.extend(st.rivals.iter().map(|a| a.color));
    let key = Key {
        bp,
        starts: race.starts,
        level: track.level.id,
        cruise: st.race.cruise,
        laps: st.race.laps > 0,
        electric: st.players[0].phys.electric,
        racers: colors.clone(),
        steer_top: steer_top(&race.touch.layout, &steering),
        in_l: (play.insets.left as f32).max(16.0),
        in_r: (play.insets.right as f32).max(16.0),
    };

    // A new race: a new `HUD` (`race.hud.mph = settings.mph`,
    // `race.hud.bestScore = store.get('bestScore.' + level, 0)`).
    if hs.model.is_none() || hs.starts != race.starts {
        let mut m = model::Hud::new(st.race.cruise, colors.len());
        if st.race.cruise {
            let k = format!("bestScore.{}", track.level.id);
            m.best_score = crate::ui::store::Store::platform().num(&k, 0.0);
        }
        hs.model = Some(m);
        hs.starts = race.starts;
        hs.centers = 0;
        hs.toasts = 0;
        hs.toast = Fade::default();
        hs.center_t0 = f64::NEG_INFINITY;
        hs.zone_t0 = f64::NEG_INFINITY;
        hs.center_seq = 0;
        hs.zone_seq = 0;
        hs.last_dial = None;
        hs.last_scene = None;
    }
    let mats_h = hs
        .mats
        .get_or_insert_with(|| {
            [
                mats.add(HudMaterial {
                    p: material::HudParams::new(material::DIAL),
                }),
                mats.add(HudMaterial {
                    p: material::HudParams::new(material::MINIMAP),
                }),
                mats.add(HudMaterial {
                    p: material::HudParams::new(material::SPEEDLINES),
                }),
            ]
        })
        .clone();
    if hs.key.as_ref() != Some(&key) {
        if let Some(r) = hs.root.take() {
            commands.entity(r).despawn();
        }
        let lay = Lay::new(&bp, key.in_l, key.in_r, play.insets.top as f32, key.steer_top);
        let labels = if key.electric {
            dials::power(0.0).labels
        } else {
            dials::tach(0.0).labels
        };
        let b = Build {
            lay,
            k: bp.k,
            track,
            key: &key,
            racer_colors: &colors,
            mats: &mats_h,
            labels: &labels,
        };
        hs.root = Some(build(&mut commands, &b));
        hs.key = Some(key.clone());
        // The new nodes take the view on the next frame.
        if let Some(m) = hs.model.as_mut() {
            m.last = model::Texts::default();
        }
        hs.last_dial = None;
        hs.last_scene = None;
        hs.last_lines = -1.0;
        hs.shown = true;
        return;
    }
    if !hs.shown {
        for (_, mut v) in &mut roots {
            *v = Visibility::Inherited;
        }
        hs.shown = true;
    }

    // `hud.update(dt, st)`: not while paused (the JS's loop skips the
    // race's update), so the timers hold.
    let model = hs.model.as_mut().expect("model");
    model.mph = ui.as_ref().is_none_or(|u| u.settings.mph);
    let h = &race.hud;
    let dt = if race.mode == Mode::Paused {
        0.0
    } else {
        time.delta_secs_f64() * play.params.timescale
    };
    if h.centers != hs.centers {
        hs.centers = h.centers;
        if let Some(c) = &h.center {
            model.center(c, model::center_class(c), h.center_timer + dt);
        }
    }
    if h.toasts != hs.toasts {
        hs.toasts = h.toasts;
        if let Some(t) = &h.toast {
            model.toast(t, h.toast_timer + dt);
        }
    }
    let input = hud_in(race);
    if race.mode != Mode::Paused {
        model.update(dt, &input, track);
    }
    if model.center_seq != hs.center_seq {
        hs.center_seq = model.center_seq;
        hs.center_t0 = now;
    }
    if model.zone_card_seq != hs.zone_seq {
        hs.zone_seq = model.zone_card_seq;
        hs.zone_t0 = now;
    }
    hs.toast.go(if model.toast_show { 1.0 } else { 0.0 }, now, 0.25);
    let toast_op = hs.toast.at(now, 0.25) as f32;
    let (c_scale, c_op) = pop(now - hs.center_t0);
    let (z_dx, z_op) = zone_card(now - hs.zone_t0);
    if model.nitro_active {
        hs.nitro_t0.get_or_insert(now);
    } else {
        hs.nitro_t0 = None;
    }
    let pulse = hs.nitro_t0.map(|t0| nitro_pulse(now - t0) as f32);

    // The materials.
    let dial = match model.dial {
        Dial::Tach { rpm } => dials::tach(rpm),
        Dial::Power { kw } => dials::power(kw),
    };
    if !key.bp.touch && hs.last_dial != Some(dial.draw) {
        hs.last_dial = Some(dial.draw);
        if let Some(mut m) = mats.get_mut(&mats_h[0]) {
            m.p = material::dial(&dial.draw);
        }
    }
    let scene = minimap::scene(track, &input, model.clock);
    if hs.last_scene.as_ref() != Some(&scene) {
        if let Some(mut m) = mats.get_mut(&mats_h[1]) {
            m.p = material::minimap(&scene);
        }
        hs.last_scene = Some(scene);
    }
    if model.speedlines != hs.last_lines {
        hs.last_lines = model.speedlines;
        if model.speedlines > 0.0
            && let Some(mut m) = mats.get_mut(&mats_h[2])
        {
            m.p = material::speedlines(model.speedlines);
        }
    }

    let k = bp.k;
    let lay_scale = if key.bp.narrow && !key.bp.touch { 0.7 } else { 1.0 };
    let l = &model.last;
    for (el, mut text, color, shadow, font, node, tf, mut vis, grad, _bg) in &mut parts {
        match *el {
            El::Pos => set_text(&mut text, &l.pos),
            El::Suf => set_text(&mut text, &l.suf),
            El::Of => set_text(&mut text, &format!("/{}", l.of)),
            El::Time => set_text(&mut text, &l.time),
            El::Zone => set_text(&mut text, &l.zone),
            El::LapN => set_text(&mut text, &l.lap_n),
            El::LapTime => set_text(
                &mut text,
                &if l.lap_time.is_empty() {
                    String::new()
                } else {
                    format!(" {}", l.lap_time)
                },
            ),
            El::LapBest => {
                set_text(&mut text, &l.lap_best);
                if let Some(mut n) = node {
                    let d = if l.lap_best.is_empty() {
                        Display::None
                    } else {
                        Display::Flex
                    };
                    if n.display != d {
                        n.display = d;
                    }
                }
            }
            El::Score => set_text(&mut text, &l.score),
            El::Mult => set_text(&mut text, &l.mult),
            El::Dist => set_text(&mut text, &l.dist),
            El::Best => set_text(&mut text, &l.best),
            El::MultFill => {
                if let Some(mut n) = node {
                    let v = Val::Percent(model.mult_fill as f32);
                    if n.width != v {
                        n.width = v;
                    }
                }
            }
            El::Dot(i) => {
                if let (Some(mut n), Some(d)) = (node, model.dots.get(i)) {
                    let v = Val::Percent(*d as f32);
                    if n.left != v {
                        n.left = v;
                    }
                }
            }
            El::Speed => set_text(&mut text, &l.speed),
            El::Unit => set_text(&mut text, &l.unit),
            El::Gear => set_text(&mut text, &l.gear),
            El::NitroFill => {
                if let Some(mut n) = node {
                    let v = Val::Percent(model.nitro as f32);
                    if n.width != v {
                        n.width = v;
                    }
                }
                // `.nitro.active .nitro-fill`: pulsing brightness.
                if let Some(mut g) = grad {
                    let want = nitro_grad(pulse.unwrap_or(1.0));
                    if *g != want {
                        *g = want;
                    }
                }
            }
            El::NitroLabel => {
                // `mix-blend-mode: difference` on white: dark where the
                // fill (near its light end) is under it, white elsewhere.
                if let Some(mut c) = color {
                    let want = if model.nitro > 88.0 {
                        Color::srgb(0.31, 0.06, 0.0)
                    } else {
                        Color::WHITE
                    };
                    if c.0 != want {
                        c.0 = want;
                    }
                }
            }
            El::Needle => {
                let (a, col) = dial.needle;
                if let Some(mut t) = tf {
                    let mid = ((dials::NEEDLE.0 + dials::NEEDLE.1) / 2.0) as f32;
                    let want = UiTransform {
                        translation: Val2::px(
                            a.cos() as f32 * mid * k * lay_scale,
                            a.sin() as f32 * mid * k * lay_scale,
                        ),
                        scale: Vec2::ONE,
                        rotation: Rot2::radians(a as f32),
                    };
                    if *t != want {
                        *t = want;
                    }
                }
                if let Some(mut b) = _bg {
                    let want = BackgroundColor(widgets::rgb(col));
                    if *b != want {
                        *b = want;
                    }
                }
            }
            El::Center => {
                set_text(&mut text, &model.center_text);
                let (size, c) = match model.center_cls {
                    model::CenterCls::Pop => (key_center(&key, false), widgets::fg()),
                    model::CenterCls::Go => (key_center(&key, false), widgets::rgb(0x4dff8a)),
                    model::CenterCls::Warn => (key_center(&key, true), widgets::rgb(0xff4d4d)),
                };
                if let Some(mut f) = font {
                    let want = bevy::text::FontSize::Px(size * k);
                    if f.font_size != want {
                        f.font_size = want;
                    }
                }
                fade(color, shadow, c, c_op as f32, 0.6 * 4.0 / 30.0);
            }
            El::CenterBox => {
                if let Some(mut t) = tf {
                    let want = UiTransform::from_scale(Vec2::splat(c_scale as f32));
                    if *t != want {
                        *t = want;
                    }
                }
            }
            El::Toast => {
                set_text(&mut text, &model.toast_text);
                fade(color, shadow, widgets::rgb(0x38b6ff), toast_op, 0.0);
            }
            El::ZoneCard => {
                if let Some(mut t) = tf {
                    let want = UiTransform::from_translation(Val2::px(z_dx as f32 * k, 0.0));
                    if *t != want {
                        *t = want;
                    }
                }
            }
            El::ZcName => {
                set_text(&mut text, &model.zone_card.0);
                fade(color, shadow, widgets::fg(), z_op as f32, 0.7 * 4.0 / 30.0);
            }
            El::ZcSub => {
                set_text(&mut text, &model.zone_card.1.to_uppercase());
                fade(color, shadow, widgets::dim(), z_op as f32, 0.0);
            }
            El::Speedlines => set_vis(&mut vis, model.speedlines > 0.0),
            El::PosRow | El::Minimap | El::Dial => {}
        }
    }
}

/// `#hud-center`'s size: `.warn` is 54 px, but on touch `body.touch
/// #hud-center` outranks it.
fn key_center(key: &Key, warn: bool) -> f32 {
    match (key.bp.touch, warn) {
        (true, _) => 84.0,
        (false, true) => 54.0,
        (false, false) => 120.0,
    }
}

/// Opacity on a text and its shadow (`opacity` on the element).
fn fade(
    color: Option<Mut<TextColor>>,
    shadow: Option<Mut<TextShadow>>,
    c: Color,
    op: f32,
    shadow_a: f32,
) {
    if let Some(mut tc) = color {
        let want = with_alpha(c, c.alpha() * op);
        if tc.0 != want {
            tc.0 = want;
        }
    }
    if let Some(mut s) = shadow {
        let want = Color::srgba(0.0, 0.0, 0.0, shadow_a * op);
        if s.color != want {
            s.color = want;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_timing_functions() {
        assert_eq!(EASE.at(0.0), 0.0);
        assert_eq!(EASE.at(1.0), 1.0);
        // ease at .5 is about .8024.
        assert!((EASE.at(0.5) - 0.8024).abs() < 1e-3);
        assert!((EASE_OUT.at(0.5) - 0.6846).abs() < 1e-3);
    }

    #[test]
    fn the_centre_pop() {
        assert_eq!(pop(0.0), (1.8, 0.0));
        let (s, o) = pop(0.16);
        assert!((s - 1.0).abs() < 1e-9 && (o - 1.0).abs() < 1e-9);
        let (s, o) = pop(0.8);
        assert!((s - 0.9).abs() < 1e-9 && o.abs() < 1e-9);
        // `both`: it stays at the end.
        assert_eq!(pop(5.0), pop(0.8));
    }

    #[test]
    fn the_zone_card() {
        let (x, o) = zone_card(0.0);
        assert_eq!((x, o), (-40.0, 0.0));
        let (x, o) = zone_card(0.432);
        assert!(x.abs() < 1e-9 && (o - 1.0).abs() < 1e-9);
        let (_, o) = zone_card(2.88);
        assert!((o - 1.0).abs() < 1e-9);
        let (x, o) = zone_card(3.6);
        assert!((x - 30.0).abs() < 1e-9 && o.abs() < 1e-9);
        // Between 12 % and 100 % it slides on.
        assert!(zone_card(2.0).0 > 0.0);
    }

    #[test]
    fn the_nitro_pulse_alternates() {
        assert_eq!(nitro_pulse(0.0), 1.0);
        assert!((nitro_pulse(0.18) - 1.7).abs() < 1e-9);
        assert!((nitro_pulse(0.36) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn toast_fade() {
        let mut f = Fade::default();
        f.go(1.0, 10.0, 0.25);
        assert_eq!(f.at(10.0, 0.25), 0.0);
        assert_eq!(f.at(10.25, 0.25), 1.0);
        // Turned off half way: it fades back from where it was.
        f.go(0.0, 10.1, 0.25);
        let mid = f.at(10.1, 0.25);
        assert!(mid > 0.0 && mid < 1.0);
        assert_eq!(f.at(10.35, 0.25), 0.0);
    }

    #[test]
    fn the_baseline_of_a_line_box() {
        // `.pos`: 64 px at line-height .9: 47.5 px down.
        assert!((baseline(64.0, 0.9) - 47.488).abs() < 1e-3);
    }

    #[test]
    fn colours_from_the_zones() {
        assert_eq!(css_color("#888"), widgets::rgb(0x888888));
        assert_eq!(css_color("#3ad7ff"), widgets::rgb(0x3ad7ff));
    }
}
