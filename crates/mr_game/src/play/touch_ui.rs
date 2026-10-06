//! Draws the touch controls (`#touch` in `index.html` and `hud.css`) with
//! Bevy UI: the thumb stick under the left thumb (or waiting in its
//! corner), the ◂ ▸ pads, or the wheel that tilt turns; the pedal slider
//! with its BRAKE, GAS and N2O bands, the fill to the thumb and the knob,
//! and the DRIFT strip beside it, or the GAS, BRAKE, DRIFT and N2O pads;
//! and the reset, camera and pause buttons. Positions come from
//! [`super::touch::Layout`], sizes are CSS px (fonts, borders and icons
//! too), turned into Bevy UI px by the page's scale. The SVG icons are
//! drawn once with `mr_canvas`'s paths from the same path data (DECISIONS
//! D842).

use std::f64::consts::PI;
use std::sync::Arc;

use super::Play;
use super::flow::Mode;
use super::touch::{Hold, PEDAL_PADS, PedalKind, Rect, SLIDER, Steering, TAPS};
use bevy::prelude::*;
use bevy::text::{FontWeight, Justify, TextLayout};
use bevy::ui::UiTransform;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum Part {
    Root,
    StickTrack,
    StickKnob,
    Slider,
    BandBrake,
    BandGas,
    BandNitro,
    Mark,
    FillGas,
    FillBrake,
    Knob,
    Drift,
    Tap(usize),
    /// `.t-dir`: ◂ (0) and ▸ (1).
    Dir(usize),
    /// `.t-pedals`' pads, in [`PEDAL_PADS`] order.
    Pad(usize),
    Wheel,
}

/// A label's font size, CSS px.
#[derive(Component, Clone, Copy)]
pub(super) struct Label(f32);

/// An icon's size, CSS px.
#[derive(Component, Clone, Copy)]
pub(super) struct Icon(f32);

/// A node's border width, CSS px.
#[derive(Component, Clone, Copy)]
pub(super) struct Border(f32);

const LINE: Color = Color::srgba(1.0, 1.0, 1.0, 0.3);
const PAD: Color = Color::srgba(10.0 / 255.0, 12.0 / 255.0, 22.0 / 255.0, 0.42);
const ON: Color = Color::srgba(1.0, 1.0, 1.0, 0.26);
const GREEN: (f32, f32, f32) = (77.0 / 255.0, 1.0, 138.0 / 255.0);
const RED: (f32, f32, f32) = (1.0, 56.0 / 255.0, 96.0 / 255.0);
const BLUE: (f32, f32, f32) = (56.0 / 255.0, 182.0 / 255.0, 1.0);
const ORANGE: (f32, f32, f32) = (1.0, 180.0 / 255.0, 84.0 / 255.0);
/// `--accent`.
const ACCENT: Color = Color::srgb(1.0, 56.0 / 255.0, 96.0 / 255.0);

fn rgba(c: (f32, f32, f32), a: f32) -> Color {
    Color::srgba(c.0, c.1, c.2, a)
}

/// The SVG icons of `#touch`, drawn once.
#[derive(Resource, Clone)]
pub(super) struct TouchIcons {
    left: Handle<Image>,
    right: Handle<Image>,
    reset: Handle<Image>,
    camera: Handle<Image>,
    pause: Handle<Image>,
    wheel: Handle<Image>,
}

/// An SVG of `view` units drawn into `n` px: stroked, round caps and
/// joins, `stroke-width` in the SVG's units.
fn svg(
    images: &mut Assets<Image>,
    view: f64,
    width: f64,
    draw: impl Fn(&mut mr_canvas::Canvas, f64),
) -> Handle<Image> {
    let n = 96u32;
    let mut c = mr_canvas::Canvas::with_fonts(n, n, Arc::new(mr_canvas::FontBook::new()));
    let k = f64::from(n) / view;
    c.set_stroke_style("#ffffff");
    c.set_line_width(width * k);
    c.set_line_cap("round");
    c.set_line_join("round");
    draw(&mut c, k);
    images.add(crate::ui::widgets::image_from_canvas(&c, n, n))
}

/// A polyline in SVG units.
fn poly(c: &mut mr_canvas::Canvas, k: f64, pts: &[(f64, f64)]) {
    c.begin_path();
    for (i, &(x, y)) in pts.iter().enumerate() {
        if i == 0 {
            c.move_to(x * k, y * k);
        } else {
            c.line_to(x * k, y * k);
        }
    }
    c.stroke();
}

fn make_icons(images: &mut Assets<Image>) -> TouchIcons {
    // `<path d="M15 4 7 12l8 8"/>` and `M9 4l8 8-8 8`.
    let left = svg(images, 24.0, 2.6, |c, k| {
        poly(c, k, &[(15.0, 4.0), (7.0, 12.0), (15.0, 20.0)])
    });
    let right = svg(images, 24.0, 2.6, |c, k| {
        poly(c, k, &[(9.0, 4.0), (17.0, 12.0), (9.0, 20.0)])
    });
    // `M4.5 12a7.5 7.5 0 1 0 2.2-5.3M4.5 4.5v4h4`: the long way round
    // anticlockwise from the left to the top left, and the arrow head.
    let reset = svg(images, 24.0, 2.6, |c, k| {
        c.begin_path();
        c.arc(12.0 * k, 12.0 * k, 7.5 * k, PI, 1.25 * PI, true);
        c.stroke();
        poly(c, k, &[(4.5, 4.5), (4.5, 8.5), (8.5, 8.5)]);
    });
    // `M3.5 8h3.5l2-2.8h6L17 8h3.5v10.5h-17z` and a 3.2 circle at 12, 13.
    let camera = svg(images, 24.0, 2.6, |c, k| {
        poly(
            c,
            k,
            &[
                (3.5, 8.0),
                (7.0, 8.0),
                (9.0, 5.2),
                (15.0, 5.2),
                (17.0, 8.0),
                (20.5, 8.0),
                (20.5, 18.5),
                (3.5, 18.5),
                (3.5, 8.0),
                (7.0, 8.0),
            ],
        );
        c.begin_path();
        c.arc(12.0 * k, 13.0 * k, 3.2 * k, 0.0, 2.0 * PI, false);
        c.stroke();
    });
    // `M8.5 5v14M15.5 5v14`.
    let pause = svg(images, 24.0, 2.6, |c, k| {
        poly(c, k, &[(8.5, 5.0), (8.5, 19.0)]);
        poly(c, k, &[(15.5, 5.0), (15.5, 19.0)]);
    });
    // `.t-wheel`: a 48 box, rim r 19, hub r 4.5, three spokes, the top
    // mark in the accent (stroke 4).
    let wheel = svg(images, 48.0, 3.0, |c, k| {
        c.set_stroke_style("rgba(255,255,255,0.7)");
        for r in [19.0, 4.5] {
            c.begin_path();
            c.arc(24.0 * k, 24.0 * k, r * k, 0.0, 2.0 * PI, false);
            c.stroke();
        }
        poly(c, k, &[(5.5, 22.0), (19.5, 22.0)]);
        poly(c, k, &[(28.5, 22.0), (42.5, 22.0)]);
        poly(c, k, &[(24.0, 28.5), (24.0, 43.0)]);
        c.set_stroke_style("#ff3860");
        c.set_line_width(4.0 * k);
        poly(c, k, &[(24.0, 3.5), (24.0, 8.5)]);
    });
    TouchIcons {
        left,
        right,
        reset,
        camera,
        pause,
        wheel,
    }
}

fn abs() -> Node {
    Node {
        position_type: PositionType::Absolute,
        ..default()
    }
}

fn centred(n: Node) -> Node {
    Node {
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..n
    }
}

fn label(s: &'static str, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(s),
        TextFont::from_font_size(size).with_font_weight(FontWeight::BOLD),
        TextColor(color),
        TextLayout::justify(Justify::Center),
        Label(size),
    )
}

fn icon(img: &Handle<Image>, size: f32, color: Color) -> impl Bundle {
    (
        ImageNode::new(img.clone()).with_color(color),
        Node {
            width: Val::Px(size),
            height: Val::Px(size),
            flex_shrink: 0.0,
            ..default()
        },
        Icon(size),
    )
}

/// A `.t-btn`: round unless `radius` says, centred content.
fn button(part: Part, radius: Option<f32>, border: Color) -> impl Bundle {
    (
        centred(Node {
            border_radius: radius.map_or(BorderRadius::MAX, |r| BorderRadius::all(Val::Px(r))),
            ..abs()
        }),
        BackgroundColor(PAD),
        BorderColor::all(border),
        Border(2.0),
        part,
        Visibility::Inherited,
    )
}

/// Spawned hidden for every race: whether this is a touch device is known
/// only once the page has said (`web::setup`).
pub(super) fn spawn(
    mut commands: Commands,
    play: Option<Res<Play>>,
    mut images: ResMut<Assets<Image>>,
) {
    if play.is_none() {
        return;
    }
    let icons = make_icons(&mut images);
    let white = Color::srgba(1.0, 1.0, 1.0, 0.88);
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                ..default()
            },
            Visibility::Hidden,
            Part::Root,
        ))
        .id();
    let dim = Color::srgba(1.0, 1.0, 1.0, 0.6);
    let stick_track = commands
        .spawn((
            Node {
                border_radius: BorderRadius::MAX,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(8.0)),
                ..abs()
            },
            BackgroundColor(PAD),
            BorderColor::all(LINE),
            Border(2.0),
            Part::StickTrack,
            Visibility::Inherited,
        ))
        .with_children(|p| {
            p.spawn(icon(&icons.left, 22.0, dim));
            p.spawn(icon(&icons.right, 22.0, dim));
        })
        .id();
    let knob = commands
        .spawn((
            Node {
                border_radius: BorderRadius::MAX,
                ..abs()
            },
            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.16)),
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.8)),
            Border(2.0),
            Part::StickKnob,
            Visibility::Inherited,
        ))
        .id();
    let band = |c: Color, part: Part, text: &'static str, tc: Color| {
        (
            centred(abs()),
            BackgroundColor(c),
            part,
            children![label(text, 12.0, tc)],
        )
    };
    let slider = commands
        .spawn((
            Node {
                border_radius: BorderRadius::all(Val::Px(22.0)),
                overflow: Overflow::clip(),
                ..abs()
            },
            BackgroundColor(PAD),
            BorderColor::all(LINE),
            Border(2.0),
            Part::Slider,
            Visibility::Inherited,
        ))
        .id();
    let parts = [
        commands
            .spawn(band(rgba(RED, 0.14), Part::BandBrake, "BRAKE", white))
            .id(),
        commands
            .spawn(band(rgba(GREEN, 0.1), Part::BandGas, "GAS", white))
            .id(),
        commands
            .spawn(band(
                rgba(BLUE, 0.18),
                Part::BandNitro,
                "N2O",
                Color::srgb(200.0 / 255.0, 236.0 / 255.0, 1.0),
            ))
            .id(),
        commands
            .spawn((abs(), BackgroundColor(rgba(GREEN, 0.6)), Part::Mark))
            .id(),
        commands
            .spawn((abs(), BackgroundColor(rgba(GREEN, 0.42)), Part::FillGas))
            .id(),
        commands
            .spawn((abs(), BackgroundColor(rgba(RED, 0.5)), Part::FillBrake))
            .id(),
        commands
            .spawn((
                Node {
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..abs()
                },
                BackgroundColor(Color::WHITE),
                BoxShadow::new(
                    Color::srgba(1.0, 1.0, 1.0, 0.8),
                    Val::Px(0.0),
                    Val::Px(0.0),
                    Val::Px(0.0),
                    Val::Px(10.0),
                ),
                Part::Knob,
            ))
            .id(),
    ];
    commands.entity(slider).add_children(&parts);
    let drift = commands
        .spawn((
            centred(Node {
                border_radius: BorderRadius::all(Val::Px(22.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                ..abs()
            }),
            BackgroundColor(PAD),
            BorderColor::all(rgba(ORANGE, 0.55)),
            Border(2.0),
            Part::Drift,
            Visibility::Inherited,
        ))
        .with_children(|p| {
            p.spawn(label("DRIFT", 11.0, white));
            p.spawn(icon(&icons.right, 18.0, white));
        })
        .id();
    let mut kids = vec![stick_track, knob, slider, drift];
    for (i, name) in TAPS.iter().enumerate() {
        let img = match *name {
            "reset" => &icons.reset,
            "camera" => &icons.camera,
            _ => &icons.pause,
        };
        kids.push(
            commands
                .spawn(button(Part::Tap(i), None, LINE))
                .with_children(|p| {
                    p.spawn(icon(img, 44.0 * 0.46, white));
                })
                .id(),
        );
    }
    for (i, img) in [&icons.left, &icons.right].into_iter().enumerate() {
        kids.push(
            commands
                .spawn(button(Part::Dir(i), None, LINE))
                .with_children(|p| {
                    p.spawn(icon(img, 30.0, white));
                })
                .id(),
        );
    }
    for (i, h) in PEDAL_PADS.iter().enumerate() {
        let (text, size, radius, border, tc) = match h {
            Hold::Handbrake => ("DRIFT", 11.0, None, LINE, white),
            Hold::Nitro => (
                "N2O",
                11.0,
                None,
                rgba(BLUE, 0.8),
                Color::srgb(200.0 / 255.0, 236.0 / 255.0, 1.0),
            ),
            Hold::Brake => ("BRAKE", 13.0, Some(22.0), rgba(RED, 0.65), white),
            _ => ("GAS", 13.0, Some(22.0), rgba(GREEN, 0.65), white),
        };
        kids.push(
            commands
                .spawn(button(Part::Pad(i), radius, border))
                .with_children(|p| {
                    p.spawn(label(text, size, tc));
                })
                .id(),
        );
    }
    kids.push(
        commands
            .spawn((
                ImageNode::new(icons.wheel.clone()),
                abs(),
                UiTransform::IDENTITY,
                Part::Wheel,
                Visibility::Inherited,
            ))
            .id(),
    );
    commands.entity(root).add_children(&kids);
    commands.insert_resource(icons);
}

fn place(n: &mut Node, r: Rect, k: f32) {
    n.left = Val::Px(r.left as f32 / k);
    n.top = Val::Px(r.top as f32 / k);
    n.width = Val::Px(r.width() as f32 / k);
    n.height = Val::Px(r.height() as f32 / k);
}

/// A band of the slider, `from`..`to` of its height from the bottom, in
/// the slider's own box (inside its 2 px border).
fn band(n: &mut Node, track: Rect, from: f64, to: f64, k: f32) {
    let h = track.height() - 4.0;
    n.left = Val::Px(0.0);
    n.right = Val::Px(0.0);
    n.width = Val::Auto;
    n.bottom = Val::Px((from * h) as f32 / k);
    n.top = Val::Auto;
    n.height = Val::Px(((to - from) * h).max(0.0) as f32 / k);
}

/// A box `scale`d about its centre (`.t-btn.on`'s `scale(.94)`).
fn scaled(r: Rect, s: f64) -> Rect {
    let (cx, cy) = ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0);
    let (hw, hh) = (r.width() * s / 2.0, r.height() * s / 2.0);
    Rect {
        left: cx - hw,
        top: cy - hh,
        right: cx + hw,
        bottom: cy + hh,
    }
}

/// CSS px sizes (fonts, icons, borders) at the page's scale.
#[allow(clippy::type_complexity)]
pub(super) fn sizes(
    play: Res<Play>,
    mut last: Local<f32>,
    mut labels: Query<(&Label, &mut TextFont)>,
    mut icons: Query<(&Icon, &mut Node), Without<Border>>,
    mut borders: Query<(&Border, &mut Node), Without<Icon>>,
) {
    let k = play.css_scale.max(0.01);
    if (*last - k).abs() < 1e-4 {
        return;
    }
    *last = k;
    for (l, mut f) in &mut labels {
        f.font_size = (l.0 / k).into();
    }
    for (i, mut n) in &mut icons {
        n.width = Val::Px(i.0 / k);
        n.height = Val::Px(i.0 / k);
    }
    for (b, mut n) in &mut borders {
        n.border = UiRect::all(Val::Px(b.0 / k));
    }
}

#[allow(clippy::type_complexity)]
pub(super) fn update(
    play: Res<Play>,
    mut parts: Query<(
        &Part,
        &mut Node,
        Option<&mut BackgroundColor>,
        Option<&mut BorderColor>,
        Option<&mut Visibility>,
        Option<&mut UiTransform>,
    )>,
) {
    // No race (the menu), or one waiting behind it: no pads.
    let Some(race) = play.race.as_ref().filter(|_| !play.hold) else {
        for (part, _, _, _, vis, _) in &mut parts {
            if *part == Part::Root
                && let Some(mut v) = vis
                && *v != Visibility::Hidden
            {
                *v = Visibility::Hidden;
            }
        }
        return;
    };
    let t = &race.touch;
    let lay = &t.layout;
    let k = play.css_scale.max(0.01);
    let shown = play.touch_ui && race.mode == Mode::Race && t.visible;
    let steering = t.steering.unwrap_or(Steering::Stick);
    let slide = t.slide;
    let s = SLIDER;
    let hold_on = |h: Hold| t.held.get(h);
    let show = |v: Option<Mut<Visibility>>, on: bool| {
        if let Some(mut v) = v {
            let want = if on {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            if *v != want {
                *v = want;
            }
        }
    };
    for (part, mut n, bg, border, vis, tf) in &mut parts {
        match *part {
            Part::Root => show(vis, shown),
            Part::StickTrack => {
                show(vis, steering == Steering::Stick);
                let r = lay.stick_r;
                let (cx, cy) = t.stick.map_or(lay.stick_home, |st| (st.x0, st.y0));
                let w = 2.0 * r + 64.0;
                // 48 px tall in the JS; with the knob (D1084).
                let th = (lay.knob - 8.0).max(48.0) / 2.0;
                place(
                    &mut n,
                    Rect {
                        left: cx - w / 2.0,
                        top: cy - th,
                        right: cx + w / 2.0,
                        bottom: cy + th,
                    },
                    k,
                );
                // Idle, the whole stick is at .75 opacity.
                let a = if t.stick.is_some() { 1.0 } else { 0.75 };
                if let Some(mut bg) = bg {
                    bg.0 = PAD.with_alpha(0.42 * a);
                }
                if let Some(mut b) = border {
                    *b = BorderColor::all(LINE.with_alpha(0.3 * a));
                }
            }
            Part::StickKnob => {
                show(vis, steering == Steering::Stick);
                let (cx, cy) = t.stick.map_or(lay.stick_home, |st| (st.x0, st.y0));
                let x = cx + t.stick_offset();
                // 56 px in the JS (D1084).
                let kr = lay.knob.max(56.0) / 2.0;
                place(
                    &mut n,
                    Rect {
                        left: x - kr,
                        top: cy - kr,
                        right: x + kr,
                        bottom: cy + kr,
                    },
                    k,
                );
                let active = t.stick.is_some();
                let lock = t.stick_offset().abs() >= lay.stick_r - 0.5;
                if let Some(mut bg) = bg {
                    bg.0 = Color::srgba(1.0, 1.0, 1.0, if active { 0.3 } else { 0.12 });
                }
                if let Some(mut b) = border {
                    *b = BorderColor::all(if lock {
                        ACCENT
                    } else if active {
                        Color::WHITE
                    } else {
                        Color::srgba(1.0, 1.0, 1.0, 0.6)
                    });
                }
            }
            Part::Slider => {
                show(vis, t.pedals == PedalKind::Slider);
                place(&mut n, lay.track, k);
                if let Some(mut b) = border {
                    *b = BorderColor::all(if slide.is_some() {
                        Color::srgba(1.0, 1.0, 1.0, 0.75)
                    } else {
                        LINE
                    });
                }
            }
            Part::Drift => {
                show(vis, t.pedals == PedalKind::Slider);
                place(&mut n, lay.drift, k);
                let on = slide.is_some_and(|s| s.drift);
                if let Some(mut bg) = bg {
                    bg.0 = if on { rgba(ORANGE, 0.42) } else { PAD };
                }
                if let Some(mut b) = border {
                    *b = BorderColor::all(rgba(ORANGE, if on { 1.0 } else { 0.55 }));
                }
            }
            Part::BandBrake => band(&mut n, lay.track, 0.0, s.brake_top, k),
            Part::BandGas => band(&mut n, lay.track, s.gas_bottom, s.nitro, k),
            Part::BandNitro => {
                band(&mut n, lay.track, s.nitro, 1.0, k);
                if let Some(mut bg) = bg {
                    let on = slide.is_some_and(|s| s.nitro);
                    bg.0 = rgba(BLUE, if on { 0.55 } else { 0.18 });
                }
            }
            Part::Mark => {
                band(&mut n, lay.track, s.gas_full, s.gas_full, k);
                n.height = Val::Px(1.0 / k);
                n.left = Val::Percent(14.0);
                n.right = Val::Percent(14.0);
            }
            Part::FillGas => {
                let u = slide.and_then(|s| s.u).unwrap_or(0.0);
                band(&mut n, lay.track, s.gas_bottom, u.max(s.gas_bottom), k);
            }
            Part::FillBrake => {
                let u = slide.and_then(|s| s.u).unwrap_or(1.0);
                band(&mut n, lay.track, u.min(s.brake_top), s.brake_top, k);
            }
            Part::Knob => {
                let u = slide.and_then(|s| s.u);
                band(&mut n, lay.track, u.unwrap_or(0.0), u.unwrap_or(0.0), k);
                n.height = Val::Px(if u.is_some() { 8.0 / k } else { 0.0 });
                n.left = Val::Px(5.0 / k);
                n.right = Val::Px(5.0 / k);
                if let Some(u) = u {
                    let h = lay.track.height() - 4.0;
                    n.bottom = Val::Px(((u * h) as f32 - 4.0) / k);
                }
            }
            Part::Tap(i) => {
                let on = t.lit.is_some_and(|(j, _)| j == i);
                place(&mut n, scaled(lay.taps[i], if on { 0.94 } else { 1.0 }), k);
                if let Some(mut bg) = bg {
                    bg.0 = if on { ON } else { PAD };
                }
                if let Some(mut b) = border {
                    *b = BorderColor::all(if on { Color::WHITE } else { LINE });
                }
            }
            Part::Dir(i) => {
                show(vis, steering == Steering::Buttons);
                let h = super::touch::DIRS[i];
                let on = hold_on(h);
                place(&mut n, scaled(lay.dirs[i], if on { 0.94 } else { 1.0 }), k);
                if let Some(mut bg) = bg {
                    bg.0 = if on { ON } else { PAD };
                }
                if let Some(mut b) = border {
                    *b = BorderColor::all(if on { Color::WHITE } else { LINE });
                }
            }
            Part::Pad(i) => {
                show(vis, t.pedals == PedalKind::Buttons);
                let h = PEDAL_PADS[i];
                let on = hold_on(h);
                place(
                    &mut n,
                    scaled(lay.pedals[i], if on { 0.94 } else { 1.0 }),
                    k,
                );
                let (idle, lit) = match h {
                    Hold::Throttle => (rgba(GREEN, 0.65), rgba(GREEN, 0.34)),
                    Hold::Brake => (rgba(RED, 0.65), rgba(RED, 0.38)),
                    Hold::Nitro => (rgba(BLUE, 0.8), rgba(BLUE, 0.45)),
                    _ => (LINE, ON),
                };
                if let Some(mut bg) = bg {
                    bg.0 = if on { lit } else { PAD };
                }
                if let Some(mut b) = border {
                    *b = BorderColor::all(if on && h != Hold::Nitro {
                        Color::WHITE
                    } else {
                        idle
                    });
                }
            }
            Part::Wheel => {
                show(vis, steering == Steering::Tilt);
                place(&mut n, lay.wheel, k);
                if let Some(mut tf) = tf {
                    let want = Rot2::degrees(t.wheel as f32);
                    if tf.rotation != want {
                        tf.rotation = want;
                    }
                }
            }
        }
    }
}
