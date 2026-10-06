//! The widget module (SPEC 8.1, roadmap WP 6.1): the one place the screens
//! speak Bevy UI, so a Bevy UI change touches this file. It holds the
//! stylesheet's tokens (`hud.css` `:root`), the layout breakpoints (the CSS
//! media queries), the font (Rajdhani, as `--font`), text styles, and the
//! controls the DOM had as elements: buttons, checkboxes, range sliders,
//! selects, the level tabs and car picks. Every control carries its DOM id
//! ([`Control::id`]) so the test bridge finds it (`window.__mp.ui(id)`).
//!
//! Sizes are given in CSS px and turned into Bevy UI px by `Bp::k` (Bevy UI
//! px per CSS px, from the page's measured scale; 1 natively).

use super::Act;
use bevy::asset::{AssetId, RenderAssetUsages};
use bevy::ecs::spawn::SpawnIter;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::{
    Font, FontSize, FontSource, FontStyle, FontWeight, Justify, LetterSpacing, LineHeight,
    TextLayout, TextSpan,
};
use bevy::ui::{
    BackgroundGradient, BoxShadow, ColorStop, InColorSpace, LinearGradient, RadialGradient,
    RadialGradientShape, ShadowStyle, UiPosition,
};
use std::sync::Arc;

// ── Tokens (`hud.css` `:root` and the colours the rules name) ──────────

pub const FAMILY: &str = "Rajdhani";
/// The slanted Bold, as its own family (`assets/fonts/oblique.py`).
pub const FAMILY_OBLIQUE: &str = "Rajdhani Oblique";

fn hex(c: u32, a: f32) -> Color {
    Color::srgba(
        ((c >> 16) & 255) as f32 / 255.0,
        ((c >> 8) & 255) as f32 / 255.0,
        (c & 255) as f32 / 255.0,
        a,
    )
}

/// `--hud-fg`.
pub fn fg() -> Color {
    hex(0xf4f6fb, 1.0)
}
/// `--hud-dim`.
pub fn dim() -> Color {
    hex(0xf4f6fb, 0.62)
}
/// `--accent`.
pub fn accent() -> Color {
    hex(0xff3860, 1.0)
}
/// `--accent2`.
pub fn accent2() -> Color {
    hex(0x3ad7ff, 1.0)
}
/// `.lvl-best`, the listening controller row.
pub fn gold() -> Color {
    hex(0xffcf4d, 1.0)
}
pub fn white(a: f32) -> Color {
    Color::srgba(1.0, 1.0, 1.0, a)
}
pub fn rgb(c: u32) -> Color {
    hex(c, 1.0)
}
pub fn rgba(c: u32, a: f32) -> Color {
    hex(c, a)
}

// ── Breakpoints (the media queries of `hud.css`) ───────────────────────

/// The viewport and the media queries it matches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bp {
    /// CSS px.
    pub w: f32,
    pub h: f32,
    /// Bevy UI px per CSS px.
    pub k: f32,
    /// `body.touch`.
    pub touch: bool,
    /// `(max-width: 720px)`.
    pub narrow: bool,
    /// `(max-height: 780px)`.
    pub short: bool,
    /// `(max-height: 500px)`: phones held sideways.
    pub compact: bool,
    /// `(orientation: portrait)`.
    pub portrait: bool,
    /// `env(safe-area-inset-top)`, CSS px.
    pub inset_top: f32,
}

impl Bp {
    pub fn new(w: f32, h: f32, k: f32, touch: bool, inset_top: f32) -> Bp {
        Bp {
            w,
            h,
            k,
            touch,
            narrow: w <= 720.0,
            short: h <= 780.0,
            compact: h <= 500.0,
            portrait: h >= w,
            inset_top,
        }
    }

    /// The two-column phone menu (`(max-height: 500px) and (orientation:
    /// landscape)`).
    pub fn two_columns(&self) -> bool {
        self.compact && !self.portrait
    }

    /// CSS px → `Val`.
    pub fn px(&self, v: f32) -> Val {
        Val::Px(v * self.k)
    }

    /// `vw` in CSS px.
    pub fn vw(&self, v: f32) -> f32 {
        self.w * v / 100.0
    }

    /// `min(560px, 92vw)`, the width of the menu's columns.
    pub fn column(&self) -> f32 {
        560f32.min(self.vw(92.0))
    }
}

// ── Fonts ──────────────────────────────────────────────────────────────

/// Rajdhani Bold slanted as Chrome slants it for `font-style: italic`
/// (`assets/fonts/oblique.py`; DECISIONS D571).
pub const RAJDHANI_BOLD_OBLIQUE: &[u8] =
    include_bytes!("../../../../assets/fonts/rajdhani/Rajdhani-BoldOblique.ttf");

/// The bundled Rajdhani faces (`mp_canvas`'s copies, so the wasm carries
/// one), registered with Bevy's text under their family name; and Rajdhani
/// Bold in place of Bevy's default font, which the M4 HUD and the touch
/// controls draw with (D431).
pub fn load_fonts(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    let book = mp_canvas::FontBook::bundled();
    let mut bold: Option<Arc<[u8]>> = None;
    let mut keep = Vec::new();
    for f in book.faces.iter().filter(|f| f.family == "arimo") {
        keep.push(fonts.add(Font::from_bytes(f.data.to_vec())));
    }
    for f in book.faces.iter().filter(|f| f.family == "rajdhani") {
        keep.push(fonts.add(Font::from_bytes(f.data.to_vec())));
        if f.weight.0 == 700.0 {
            bold = Some(f.data.clone());
        }
    }
    keep.push(fonts.add(Font::from_bytes(RAJDHANI_BOLD_OBLIQUE.to_vec())));
    // An asset goes when its last handle does.
    commands.insert_resource(UiFonts(keep));
    if let Some(b) = bold {
        let _ = fonts.insert(AssetId::default(), Font::from_bytes(b.to_vec()));
    }
}

/// The fonts' handles, held so the assets stay.
#[derive(Resource)]
pub struct UiFonts(pub Vec<Handle<Font>>);

/// A text style: CSS `font-size`, `font-weight`, `font-style: italic`,
/// `letter-spacing` (in em), `color` and `line-height` (relative; the
/// font's normal is 1.276).
#[derive(Clone, Copy, Debug)]
pub struct T {
    pub size: f32,
    pub weight: u16,
    pub italic: bool,
    pub spacing: f32,
    pub color: Color,
    pub line: f32,
}

/// Rajdhani's `line-height: normal`: (ascent + descent) / em = 1.276.
pub const LINE_NORMAL: f32 = 1.276;

impl T {
    pub fn new(size: f32) -> T {
        T {
            size,
            weight: 500,
            italic: false,
            spacing: 0.0,
            color: fg(),
            line: LINE_NORMAL,
        }
    }
    pub fn w(mut self, weight: u16) -> T {
        self.weight = weight;
        self
    }
    pub fn bold(self) -> T {
        self.w(700)
    }
    pub fn italic(mut self) -> T {
        self.italic = true;
        self
    }
    pub fn ls(mut self, em: f32) -> T {
        self.spacing = em;
        self
    }
    pub fn c(mut self, color: Color) -> T {
        self.color = color;
        self
    }
    pub fn lh(mut self, line: f32) -> T {
        self.line = line;
        self
    }

    /// The face Chrome picks: the page loads Rajdhani 500, 600 and 700, so
    /// a normal weight (400) draws Medium and anything heavier than 700
    /// draws Bold.
    fn face_weight(&self) -> u16 {
        match self.weight {
            0..=500 => 500,
            501..=600 => 600,
            _ => 700,
        }
    }

    pub fn font(&self, k: f32) -> TextFont {
        TextFont {
            font: FontSource::Family(if self.italic { FAMILY_OBLIQUE } else { FAMILY }.into()),
            font_size: FontSize::Px(self.size * k),
            weight: FontWeight(if self.italic { 700 } else { self.face_weight() }),
            style: FontStyle::Normal,
            ..default()
        }
    }

    pub fn spacing(&self, k: f32) -> LetterSpacing {
        LetterSpacing::Px(self.spacing * self.size * k)
    }
}

/// Characters Rajdhani lacks, drawn from the fallback face (`fonts.json`'s
/// `fallback`, Arimo's symbols: D372), as Chrome falls back for them.
fn needs_fallback(c: char) -> bool {
    matches!(c, '\u{2190}'..='\u{2193}' | '\u{25cf}')
}

/// `s` cut into runs of the main face and of the fallback face.
fn runs(s: &str) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    for c in s.chars() {
        let fb = needs_fallback(c);
        match out.last_mut() {
            Some((r, f)) if *f == fb => r.push(c),
            _ => out.push((c.to_string(), fb)),
        }
    }
    out
}

impl T {
    fn font_for(&self, k: f32, fallback: bool) -> TextFont {
        let mut f = self.font(k);
        if fallback {
            f.font = FontSource::Family(FALLBACK.into());
            f.weight = FontWeight(self.face_weight().clamp(400, 700));
        }
        f
    }
}

/// The fallback family.
pub const FALLBACK: &str = "Arimo";

fn span_run(s: String, t: T, k: f32, fallback: bool) -> impl Bundle {
    (
        TextSpan::new(s),
        t.font_for(k, fallback),
        TextColor(t.color),
        t.spacing(k),
        LineHeight::RelativeToFont(t.line),
    )
}

/// Runs after the first, as spans.
fn rest(rs: Vec<(String, bool)>, t: T, k: f32) -> impl Bundle {
    Children::spawn(SpawnIter(
        rs.into_iter()
            .map(move |(r, fb)| span_run(r, t, k, fb))
            .collect::<Vec<_>>()
            .into_iter(),
    ))
}

/// A text node.
pub fn text(s: impl Into<String>, t: T, k: f32) -> impl Bundle {
    let mut rs = runs(&s.into());
    let (first, fb) = if rs.is_empty() {
        (String::new(), false)
    } else {
        rs.remove(0)
    };
    (
        Text::new(first),
        t.font_for(k, fb),
        TextColor(t.color),
        t.spacing(k),
        LineHeight::RelativeToFont(t.line),
        rest(rs, t, k),
    )
}

/// A text span (a child of a text node), for runs in another style.
pub fn span(s: impl Into<String>, t: T, k: f32) -> impl Bundle {
    let mut rs = runs(&s.into());
    let (first, fb) = if rs.is_empty() {
        (String::new(), false)
    } else {
        rs.remove(0)
    };
    (span_run(first, t, k, fb), rest(rs, t, k))
}

/// A line of text in several styles: `[(text, style)]`.
pub fn rich(p: &mut ChildSpawnerCommands, runs: &[(String, T)], k: f32, justify: Justify) {
    let Some(((first, t0), rest)) = runs.split_first() else {
        return;
    };
    p.spawn((text(first.clone(), *t0, k), TextLayout::justify(justify)))
        .with_children(|p| {
            for (s, t) in rest {
                p.spawn(span(s.clone(), *t, k));
            }
        });
}

// ── Nodes ──────────────────────────────────────────────────────────────

/// What a node is to the test bridge and the input: its DOM id, and for a
/// control what activating it does.
#[derive(Component, Clone, Debug)]
pub struct Control {
    pub id: String,
    pub act: Option<Act>,
    /// The bridge's `value`: a checkbox's state, a slider's 0–100, a
    /// select's value, a text's content.
    pub value: Value,
    /// `.sel` on a tab, pick or mode button.
    pub sel: bool,
    /// Focusable from the keyboard (Tab), in build order.
    pub focus: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    None,
    Bool(bool),
    Num(f64),
    Text(String),
}

impl Control {
    pub fn act(id: impl Into<String>, act: Act) -> Control {
        Control {
            id: id.into(),
            act: Some(act),
            value: Value::None,
            sel: false,
            focus: true,
        }
    }

    /// A named node the tests read (`lvl-name`, `res-title`, …).
    pub fn named(id: impl Into<String>, value: Value) -> Control {
        Control {
            id: id.into(),
            act: None,
            value,
            sel: false,
            focus: false,
        }
    }

    pub fn value(mut self, v: Value) -> Control {
        self.value = v;
        self
    }

    pub fn sel(mut self, sel: bool) -> Control {
        self.sel = sel;
        self
    }
}

/// The control with the keyboard focus draws the focus ring.
#[derive(Component)]
pub struct Focused;

/// A range slider: the setting it edits and its track's node, for dragging.
#[derive(Component, Clone, Copy, Debug)]
pub struct SliderTrack {
    pub which: super::Sl,
}

pub fn border(bp: &Bp, w: f32) -> UiRect {
    UiRect::all(bp.px(w))
}

pub fn radius(bp: &Bp, r: f32) -> BorderRadius {
    BorderRadius::all(bp.px(r))
}

pub fn pill() -> BorderRadius {
    BorderRadius::MAX
}

/// `linear-gradient(90deg, a, b)`.
pub fn hgrad(a: Color, b: Color) -> BackgroundGradient {
    BackgroundGradient::from(
        LinearGradient::to_right(vec![ColorStop::auto(a), ColorStop::auto(b)]).in_srgb(),
    )
}

/// A `.screen`'s background: `radial-gradient(ellipse at 50% y%, a, b
/// stop%)` (ellipse, farthest corner).
pub fn screen_bg(y: f32, a: Color, b: Color, b_at: Option<f32>) -> BackgroundGradient {
    let end = match b_at {
        Some(p) => ColorStop::percent(b, p),
        None => ColorStop::auto(b),
    };
    BackgroundGradient::from(
        RadialGradient::new(
            UiPosition::anchor(Vec2::new(0.0, y / 100.0 - 0.5)),
            RadialGradientShape::FarthestCorner,
            vec![ColorStop::auto(a), end],
        )
        .in_srgb(),
    )
}

/// `box-shadow` layers: `(x, y, blur, spread, colour)` in CSS px.
pub fn shadow(bp: &Bp, layers: &[(f32, f32, f32, f32, Color)]) -> BoxShadow {
    BoxShadow(
        layers
            .iter()
            .map(|&(x, y, blur, spread, color)| ShadowStyle {
                color,
                x_offset: bp.px(x),
                y_offset: bp.px(y),
                spread_radius: bp.px(spread),
                // CSS's blur radius is twice the Gaussian's deviation;
                // Bevy's is the deviation.
                blur_radius: bp.px(blur / 2.0),
            })
            .collect(),
    )
}

/// The selected look of a tab or pick: `border-color: var(--accent);
/// box-shadow: 0 0 0 1px var(--accent), 0 0 24px rgba(255,56,96,.25)`.
/// The ring is an outline; the glow is left out, because Bevy draws a box
/// shadow under the whole node and these nodes are translucent (it would
/// tint them, where CSS draws a shadow only outside the box).
pub fn sel_ring(bp: &Bp) -> Outline {
    Outline::new(bp.px(1.0), Val::Px(0.0), accent())
}

/// `.btn` and `.btn.primary`: a pill with an upper-case label.
pub fn button(
    p: &mut ChildSpawnerCommands,
    bp: &Bp,
    ctl: Control,
    label: &str,
    primary: bool,
    focused: bool,
) {
    let c = bp.compact;
    let size = if c { 16.0 } else { 20.0 };
    let pad = if c { (9.0, 28.0) } else { (12.0, 42.0) };
    let b = if primary { 0.0 } else { 1.0 };
    let mut e = p.spawn((
        Node {
            padding: UiRect::axes(bp.px(pad.1), bp.px(pad.0)),
            border: border(bp, b),
            border_radius: pill(),
            min_width: bp.px(if c { 170.0 } else { 220.0 }),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BorderColor::all(white(0.25)),
        ctl.value(Value::Text(label.to_owned())),
    ));
    if primary {
        e.insert((
            hgrad(accent(), rgb(0xff7a3c)),
            shadow(bp, &[(0.0, 8.0, 30.0, 0.0, rgba(0xff3860, 0.35))]),
        ));
    } else {
        e.insert(BackgroundColor(white(0.06)));
    }
    focus_ring(&mut e, bp, focused);
    e.with_children(|p| {
        p.spawn((
            text(
                label.to_uppercase(),
                T::new(size).bold().ls(0.2).c(Color::WHITE),
                bp.k,
            ),
            TextLayout::no_wrap(),
        ));
    });
}

/// `.btn-mini`.
pub fn button_mini(
    p: &mut ChildSpawnerCommands,
    bp: &Bp,
    ctl: Control,
    label: &str,
    icon: Option<Handle<Image>>,
    focused: bool,
) {
    let mut e = p.spawn((
        Node {
            padding: UiRect::axes(bp.px(14.0), bp.px(6.0)),
            border: border(bp, 1.0),
            border_radius: pill(),
            align_items: AlignItems::Center,
            column_gap: bp.px(5.0),
            ..default()
        },
        BorderColor::all(white(0.25)),
        BackgroundColor(white(0.06)),
        ctl.value(Value::Text(label.to_owned())),
    ));
    focus_ring(&mut e, bp, focused);
    e.with_children(|p| {
        p.spawn((
            text(
                label.to_uppercase(),
                T::new(14.0).bold().ls(0.12).c(Color::WHITE),
                bp.k,
            ),
            TextLayout::no_wrap(),
        ));
        if let Some(i) = icon {
            p.spawn((
                ImageNode::new(i),
                Node {
                    width: bp.px(14.0),
                    height: bp.px(14.0),
                    ..default()
                },
            ));
        }
    });
}

pub fn focus_ring(e: &mut EntityCommands, bp: &Bp, focused: bool) {
    if focused {
        // `body.pad-nav .pad-focus`'s ring (the keyboard's, here), #ffcf4d
        // while the pad adjusts a slider or drop-down (`.pad-edit`).
        let c = if super::nav::EDITING.load(std::sync::atomic::Ordering::Relaxed) {
            gold()
        } else {
            accent2()
        };
        e.insert((Outline::new(bp.px(3.0), bp.px(3.0), c), Focused));
    }
}

/// A checkbox with its label after it (`<label><input type=checkbox>
/// Label</label>`): Chrome's 13 px box, filled with the accent and a white
/// tick when checked (`accent-color`).
#[allow(clippy::too_many_arguments)]
pub fn checkbox(
    p: &mut ChildSpawnerCommands,
    bp: &Bp,
    ctl: Control,
    label: &str,
    on: bool,
    t: T,
    icons: &Icons,
    focused: bool,
) {
    let mut e = p.spawn((
        Node {
            align_items: AlignItems::Center,
            ..default()
        },
        ctl.value(Value::Bool(on)),
    ));
    focus_ring(&mut e, bp, focused);
    e.with_children(|p| {
        let mut b = p.spawn((
            Node {
                width: bp.px(13.0),
                height: bp.px(13.0),
                margin: UiRect::new(bp.px(4.0), bp.px(3.0), bp.px(3.0), bp.px(3.0)),
                border: border(bp, if on { 0.0 } else { 1.0 }),
                border_radius: radius(bp, 2.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BorderColor::all(rgb(0x767676)),
            BackgroundColor(if on { accent() } else { Color::WHITE }),
        ));
        if on {
            b.with_children(|p| {
                p.spawn((
                    ImageNode::new(icons.check.clone()),
                    Node {
                        width: bp.px(11.0),
                        height: bp.px(11.0),
                        ..default()
                    },
                ));
            });
        }
        p.spawn(text(format!(" {label}"), t, bp.k));
    });
}

/// `<input type=range>` with `accent-color`: Chrome's track (4 px, the
/// filled part in the accent) and its 16 px thumb, 110 px wide (`.menu-opts
/// input[type=range]`).
pub fn slider(
    p: &mut ChildSpawnerCommands,
    bp: &Bp,
    ctl: Control,
    which: super::Sl,
    v01: f64,
    width: f32,
    focused: bool,
) {
    let v = v01.clamp(0.0, 1.0) as f32;
    let thumb = 16.0;
    let mut e = p.spawn((
        Node {
            width: bp.px(width),
            height: bp.px(thumb),
            margin: UiRect::axes(bp.px(2.0), bp.px(2.0)),
            align_items: AlignItems::Center,
            ..default()
        },
        SliderTrack { which },
        ctl.value(Value::Num((v01 * 100.0).round())),
    ));
    focus_ring(&mut e, bp, focused);
    e.with_children(|p| {
        // The track: filled up to the thumb's centre.
        let inner = width - thumb;
        let at = thumb / 2.0 + inner * v;
        p.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: bp.px(0.0),
                width: bp.px(at),
                height: bp.px(4.0),
                border_radius: radius(bp, 2.0),
                ..default()
            },
            BackgroundColor(accent()),
        ));
        p.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: bp.px(at),
                right: bp.px(0.0),
                height: bp.px(4.0),
                border_radius: radius(bp, 2.0),
                ..default()
            },
            BackgroundColor(rgb(0xefefef)),
        ));
        p.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: bp.px(at - thumb / 2.0),
                width: bp.px(thumb),
                height: bp.px(thumb),
                border_radius: pill(),
                ..default()
            },
            BackgroundColor(accent()),
        ));
    });
}

/// A `<select>` showing its current choice, styled as `.menu-opts select`
/// (14 px, a light border, a chevron); tapping it opens the list
/// ([`dropdown`]).
pub fn select(
    p: &mut ChildSpawnerCommands,
    bp: &Bp,
    ctl: Control,
    shown: &str,
    icons: &Icons,
    focused: bool,
) {
    let mut e = p.spawn((
        Node {
            padding: UiRect::new(bp.px(6.0), bp.px(4.0), bp.px(2.0), bp.px(2.0)),
            border: border(bp, 1.0),
            border_radius: radius(bp, 6.0),
            max_width: bp.px(190.0),
            align_items: AlignItems::Center,
            column_gap: bp.px(4.0),
            overflow: Overflow::clip(),
            ..default()
        },
        BorderColor::all(white(0.2)),
        BackgroundColor(white(0.08)),
        ctl,
    ));
    focus_ring(&mut e, bp, focused);
    e.with_children(|p| {
        p.spawn((
            text(shown, T::new(14.0), bp.k),
            TextLayout::no_wrap(),
            Node {
                flex_shrink: 1.0,
                overflow: Overflow::clip(),
                ..default()
            },
        ));
        p.spawn((
            ImageNode::new(icons.chevron.clone()),
            Node {
                width: bp.px(10.0),
                height: bp.px(10.0),
                flex_shrink: 0.0,
                ..default()
            },
        ));
    });
}

/// A select's open list: the options under the select, the current one
/// marked; a tap outside closes it.
pub fn dropdown(
    commands: &mut Commands,
    bp: &Bp,
    at: (f32, f32, f32),
    options: &[(String, String)],
    current: &str,
    make: impl Fn(&str) -> Act,
) -> Entity {
    let (x, y, w) = at;
    let row_h = 14.0 * LINE_NORMAL + 8.0;
    let total = row_h * options.len() as f32 + 8.0;
    // Below the select if it fits, else above it.
    let top = if y + total <= bp.h - 4.0 {
        y
    } else {
        (y - total - 24.0).max(4.0)
    };
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                ..default()
            },
            GlobalZIndex(50),
            Control {
                id: "dropdown-backdrop".into(),
                act: Some(Act::CloseDropdown),
                value: Value::None,
                sel: false,
                focus: false,
            },
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: bp.px(x),
                    top: bp.px(top),
                    min_width: bp.px(w.max(120.0)),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(bp.px(0.0), bp.px(4.0)),
                    border: border(bp, 1.0),
                    border_radius: radius(bp, 6.0),
                    ..default()
                },
                BorderColor::all(white(0.2)),
                BackgroundColor(rgb(0x14161e)),
                shadow(bp, &[(0.0, 6.0, 18.0, 0.0, rgba(0, 0.6))]),
            ))
            .with_children(|p| {
                for (value, label) in options {
                    let on = value == current;
                    p.spawn((
                        Node {
                            padding: UiRect::axes(bp.px(10.0), bp.px(4.0)),
                            ..default()
                        },
                        BackgroundColor(if on { white(0.12) } else { Color::NONE }),
                        GlobalZIndex(51),
                        Control::act(format!("option-{value}"), make(value)).sel(on),
                    ))
                    .with_children(|p| {
                        p.spawn((
                            text(label.clone(), T::new(14.0), bp.k),
                            TextLayout::no_wrap(),
                        ));
                    });
                }
            });
        })
        .id()
}

// ── Icons (characters the bundled faces lack, drawn) ───────────────────

/// Small pictures for the characters no bundled face has (★ ⏭ ♪ ✓ and the
/// select's chevron), drawn once with `mp_canvas`'s paths and shown as
/// images.
#[derive(Resource, Clone)]
pub struct Icons {
    pub check: Handle<Image>,
    pub star: Handle<Image>,
    pub next: Handle<Image>,
    pub note: Handle<Image>,
    pub chevron: Handle<Image>,
}

pub fn image_from_canvas(c: &mp_canvas::Canvas, w: u32, h: u32) -> Image {
    Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        c.to_rgba(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
}

fn icon(images: &mut Assets<Image>, draw: impl Fn(&mut mp_canvas::Canvas, f64)) -> Handle<Image> {
    let n = 64u32;
    // No fonts: shapes only (the text stack stays out of the wasm).
    let mut c = mp_canvas::Canvas::with_fonts(n, n, Arc::new(mp_canvas::FontBook::new()));
    draw(&mut c, f64::from(n));
    images.add(image_from_canvas(&c, n, n))
}

pub fn make_icons(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let check = icon(&mut images, |c, n| {
        c.set_stroke_style("#ffffff");
        c.set_line_width(n * 0.16);
        c.set_line_cap("round");
        c.set_line_join("round");
        c.begin_path();
        c.move_to(n * 0.2, n * 0.52);
        c.line_to(n * 0.42, n * 0.74);
        c.line_to(n * 0.8, n * 0.28);
        c.stroke();
    });
    let star = icon(&mut images, |c, n| {
        c.set_fill_style("#ffffff");
        c.begin_path();
        for i in 0..10 {
            let r = if i % 2 == 0 { n * 0.48 } else { n * 0.2 };
            let a = std::f64::consts::PI * (f64::from(i) / 5.0 - 0.5);
            let (x, y) = (n / 2.0 + r * a.cos(), n * 0.53 + r * a.sin());
            if i == 0 {
                c.move_to(x, y);
            } else {
                c.line_to(x, y);
            }
        }
        c.close_path();
        c.fill();
    });
    let next = icon(&mut images, |c, n| {
        c.set_fill_style("#ffffff");
        for x0 in [0.1, 0.42] {
            c.begin_path();
            c.move_to(n * x0, n * 0.2);
            c.line_to(n * (x0 + 0.34), n * 0.5);
            c.line_to(n * x0, n * 0.8);
            c.close_path();
            c.fill();
        }
        c.fill_rect(n * 0.78, n * 0.2, n * 0.1, n * 0.6);
    });
    let note = icon(&mut images, |c, n| {
        c.set_fill_style("#3ad7ff");
        c.begin_path();
        c.ellipse(
            n * 0.35,
            n * 0.74,
            n * 0.17,
            n * 0.13,
            -0.4,
            0.0,
            6.3,
            false,
        );
        c.fill();
        c.fill_rect(n * 0.46, n * 0.12, n * 0.08, n * 0.62);
        c.begin_path();
        c.move_to(n * 0.46, n * 0.12);
        c.line_to(n * 0.82, n * 0.28);
        c.line_to(n * 0.82, n * 0.4);
        c.line_to(n * 0.54, n * 0.28);
        c.close_path();
        c.fill();
    });
    let chevron = icon(&mut images, |c, n| {
        c.set_stroke_style("#f4f6fb");
        c.set_line_width(n * 0.12);
        c.set_line_cap("round");
        c.set_line_join("round");
        c.begin_path();
        c.move_to(n * 0.2, n * 0.38);
        c.line_to(n * 0.5, n * 0.66);
        c.line_to(n * 0.8, n * 0.38);
        c.stroke();
    });
    commands.insert_resource(Icons {
        check,
        star,
        next,
        note,
        chevron,
    });
}

/// An icon inline with text.
pub fn icon_node(
    p: &mut ChildSpawnerCommands,
    bp: &Bp,
    img: &Handle<Image>,
    size: f32,
    color: Color,
) {
    p.spawn((
        ImageNode::new(img.clone()).with_color(color),
        Node {
            width: bp.px(size),
            height: bp.px(size),
            flex_shrink: 0.0,
            ..default()
        },
    ));
}

// ── The logo ───────────────────────────────────────────────────────────

/// The logo (`.logo`) at font size `f` (CSS px): "MIDNIGHT", italic 900
/// (Rajdhani Bold, slanted), letter-spacing -.02em, line-height .9; under
/// it the second word ("RACER" in the JS; "PLAYGROUND", D1100) at half the size, spaced .45em, set in .3em. The CSS paints
/// both with gradients through the glyphs (`background-clip: text`) and
/// adds a pink glow; Bevy UI has no gradient text, so MIDNIGHT takes the
/// colour its glyphs mostly show (the white top of its gradient) and each
/// letter of the second word the colour of the accent-to-orange gradient where it
/// stands; the glow is left out (DECISIONS D573). (Drawing it with
/// mp_canvas's text instead linked a second copy of the font stack, 1 MB
/// of wasm.)
pub fn logo(p: &mut ChildSpawnerCommands, bp: &Bp, f: f32) -> Entity {
    let top = T::new(f).w(900).italic().ls(-0.02).lh(0.9).c(rgb(0xf6f7fa));
    p.spawn(Node {
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        padding: UiRect::horizontal(bp.px(0.18 * f)),
        flex_shrink: 0.0,
        ..default()
    })
    .with_children(|p| {
        p.spawn((text("MIDNIGHT", top, bp.k), TextLayout::no_wrap()));
        let half = T::new(f * 0.5).w(900).italic().ls(0.45).lh(0.9);
        p.spawn((
            Node {
                margin: UiRect::left(bp.px(0.15 * f)),
                ..default()
            },
            Text::default(),
            half.font(bp.k),
            half.spacing(bp.k),
            LineHeight::RelativeToFont(0.9),
            TextLayout::no_wrap(),
        ))
        .with_children(|p| {
            // Where each letter sits along the span's gradient: RACER's
            // five ran from 0.22 to 0.78 in steps of 0.14; any word spans
            // the same stretch.
            let (a, b) = (rgb(0xff3860).to_srgba(), rgb(0xff9a3c).to_srgba());
            let word = "PLAYGROUND";
            let step = 0.56 / (word.len() - 1) as f32;
            for (i, ch) in word.chars().enumerate() {
                let t = 0.22 + step * i as f32;
                let c = Color::srgb(
                    a.red + (b.red - a.red) * t,
                    a.green + (b.green - a.green) * t,
                    a.blue + (b.blue - a.blue) * t,
                );
                p.spawn(span(ch.to_string(), half.c(c), bp.k));
            }
        });
    })
    .id()
}

/// `x.toFixed(d)`: rounds half away from zero on the decimal value, as
/// JS does for the numbers the menus show.
pub fn to_fixed(x: f64, d: usize) -> String {
    let p = 10f64.powi(d as i32);
    let v = (x * p).abs().round() / p * x.signum();
    format!("{v:.d$}")
}

/// `n.toLocaleString()` for a whole number in en-US: thousands separated
/// by commas.
pub fn locale_int(n: f64) -> String {
    let neg = n < 0.0;
    let digits = format!("{}", n.abs().round() as u64);
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if neg { format!("-{out}") } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_formats() {
        assert_eq!(locale_int(12345.0), "12,345");
        assert_eq!(locale_int(999.0), "999");
        assert_eq!(locale_int(1234567.0), "1,234,567");
        assert_eq!(locale_int(0.0), "0");
        assert_eq!(to_fixed(0.25, 1), "0.3");
        assert_eq!(to_fixed(3.5949, 1), "3.6");
        assert_eq!(to_fixed(2.2338, 2), "2.23");
        assert_eq!(to_fixed(12.0, 1), "12.0");
    }

    #[test]
    fn breakpoints_follow_the_media_queries() {
        let desk = Bp::new(1280.0, 800.0, 1.0, false, 0.0);
        assert!(!desk.short && !desk.compact && !desk.narrow && !desk.portrait);
        let phone = Bp::new(844.0, 390.0, 1.0, true, 0.0);
        assert!(phone.short && phone.compact && !phone.narrow && phone.two_columns());
        let tall = Bp::new(412.0, 915.0, 1.0, true, 0.0);
        assert!(tall.narrow && tall.portrait && !tall.short && !tall.two_columns());
        assert_eq!(phone.column(), 560.0);
        assert!((tall.column() - 379.04).abs() < 0.01);
    }
}
