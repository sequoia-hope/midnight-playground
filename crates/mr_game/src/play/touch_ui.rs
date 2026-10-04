//! Draws the touch controls (`#touch` in `index.html` and `hud.css`) with
//! Bevy UI: the thumb stick under the left thumb (or waiting in its
//! corner), the pedal slider with its BRAKE, GAS and N2O bands, the fill to
//! the thumb and the knob, the DRIFT strip beside it, and the reset, camera
//! and pause buttons. Positions come from [`super::touch::Layout`], in CSS
//! px.

use super::Play;
use super::flow::Mode;
use super::touch::{Rect, SLIDER, TAPS};
use bevy::prelude::*;
use bevy::text::{FontWeight, Justify, TextLayout};

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
}

const LINE: Color = Color::srgba(1.0, 1.0, 1.0, 0.3);
const PAD: Color = Color::srgba(0.04, 0.05, 0.09, 0.42);

fn abs() -> Node {
    Node {
        position_type: PositionType::Absolute,
        ..default()
    }
}

fn label(s: &'static str, size: f32) -> impl Bundle {
    (
        Text::new(s),
        TextFont::from_font_size(size).with_font_weight(FontWeight::BOLD),
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.88)),
        TextLayout::justify(Justify::Center),
    )
}

/// Spawned hidden for every race: whether this is a touch device is known
/// only once the page has said (`web::setup`).
pub(super) fn spawn(mut commands: Commands, play: Option<Res<Play>>) {
    if play.is_none() {
        return;
    }
    let centred = |n: Node| Node {
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..n
    };
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
    let stick_track = commands
        .spawn((
            Node {
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::MAX,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(12.0)),
                ..abs()
            },
            BackgroundColor(PAD),
            BorderColor::all(LINE),
            Part::StickTrack,
            children![label("<", 20.0), label(">", 20.0)],
        ))
        .id();
    let knob = commands
        .spawn((
            Node {
                width: Val::Px(56.0),
                height: Val::Px(56.0),
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::MAX,
                ..abs()
            },
            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.12)),
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.6)),
            Part::StickKnob,
        ))
        .id();
    let band = |c: Color, part: Part, text: &'static str| {
        (
            centred(abs()),
            BackgroundColor(c),
            part,
            children![label(text, 13.0)],
        )
    };
    let slider = commands
        .spawn((
            Node {
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(22.0)),
                overflow: Overflow::clip(),
                ..abs()
            },
            BackgroundColor(PAD),
            BorderColor::all(LINE),
            Part::Slider,
        ))
        .id();
    let parts = [
        commands
            .spawn(band(
                Color::srgba(1.0, 0.22, 0.38, 0.14),
                Part::BandBrake,
                "BRAKE",
            ))
            .id(),
        commands
            .spawn(band(
                Color::srgba(0.3, 1.0, 0.54, 0.1),
                Part::BandGas,
                "GAS",
            ))
            .id(),
        commands
            .spawn(band(
                Color::srgba(0.22, 0.71, 1.0, 0.18),
                Part::BandNitro,
                "N2O",
            ))
            .id(),
        commands
            .spawn((
                abs(),
                BackgroundColor(Color::srgba(0.3, 1.0, 0.54, 0.6)),
                Part::Mark,
            ))
            .id(),
        commands
            .spawn((
                abs(),
                BackgroundColor(Color::srgba(0.3, 1.0, 0.54, 0.45)),
                Part::FillGas,
            ))
            .id(),
        commands
            .spawn((
                abs(),
                BackgroundColor(Color::srgba(1.0, 0.22, 0.38, 0.55)),
                Part::FillBrake,
            ))
            .id(),
        commands
            .spawn((
                Node {
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..abs()
                },
                BackgroundColor(Color::WHITE),
                Part::Knob,
            ))
            .id(),
    ];
    commands.entity(slider).add_children(&parts);
    let drift = commands
        .spawn((
            centred(Node {
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(22.0)),
                flex_direction: FlexDirection::Column,
                ..abs()
            }),
            BackgroundColor(PAD),
            BorderColor::all(LINE),
            Part::Drift,
            children![label("DRIFT", 12.0), label(">", 18.0)],
        ))
        .id();
    let mut kids = vec![stick_track, knob, slider, drift];
    for (i, name) in TAPS.iter().enumerate() {
        let text = match *name {
            "reset" => "R",
            "camera" => "C",
            _ => "II",
        };
        kids.push(
            commands
                .spawn((
                    centred(Node {
                        border: UiRect::all(Val::Px(2.0)),
                        border_radius: BorderRadius::MAX,
                        ..abs()
                    }),
                    BackgroundColor(PAD),
                    BorderColor::all(LINE),
                    Part::Tap(i),
                    children![label(text, 15.0)],
                ))
                .id(),
        );
    }
    commands.entity(root).add_children(&kids);
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

pub(super) fn update(
    play: Res<Play>,
    mut parts: Query<(
        &Part,
        &mut Node,
        Option<&mut BackgroundColor>,
        Option<&mut Visibility>,
    )>,
) {
    let Some(race) = &play.race else { return };
    let t = &race.touch;
    let lay = &t.layout;
    let k = play.css_scale.max(0.01);
    let shown = play.touch_ui && race.mode == Mode::Race && t.visible;
    let slide = t.slide;
    let s = SLIDER;
    for (part, mut n, bg, vis) in &mut parts {
        match *part {
            Part::Root => {
                if let Some(mut v) = vis {
                    let want = if shown {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    };
                    if *v != want {
                        *v = want;
                    }
                }
            }
            Part::StickTrack => {
                let r = lay.stick_r;
                let (cx, cy) = t.stick.map_or(lay.stick_home, |st| (st.x0, st.y0));
                let w = 2.0 * r + 64.0;
                place(
                    &mut n,
                    Rect {
                        left: cx - w / 2.0,
                        top: cy - 24.0,
                        right: cx + w / 2.0,
                        bottom: cy + 24.0,
                    },
                    k,
                );
                if let Some(mut bg) = bg {
                    bg.0 = PAD.with_alpha(if t.stick.is_some() { 0.55 } else { 0.32 });
                }
            }
            Part::StickKnob => {
                let (cx, cy) = t.stick.map_or(lay.stick_home, |st| (st.x0, st.y0));
                let x = cx + t.stick_offset();
                place(
                    &mut n,
                    Rect {
                        left: x - 28.0,
                        top: cy - 28.0,
                        right: x + 28.0,
                        bottom: cy + 28.0,
                    },
                    k,
                );
                if let Some(mut bg) = bg {
                    bg.0 = Color::srgba(1.0, 1.0, 1.0, if t.stick.is_some() { 0.3 } else { 0.12 });
                }
            }
            Part::Slider => place(&mut n, lay.track, k),
            Part::Drift => {
                place(&mut n, lay.drift, k);
                if let Some(mut bg) = bg {
                    bg.0 = if slide.is_some_and(|s| s.drift) {
                        Color::srgba(1.0, 0.7, 0.33, 0.42)
                    } else {
                        PAD
                    };
                }
            }
            Part::BandBrake => band(&mut n, lay.track, 0.0, s.brake_top, k),
            Part::BandGas => band(&mut n, lay.track, s.gas_bottom, s.nitro, k),
            Part::BandNitro => {
                band(&mut n, lay.track, s.nitro, 1.0, k);
                if let Some(mut bg) = bg {
                    bg.0 = Color::srgba(
                        0.22,
                        0.71,
                        1.0,
                        if slide.is_some_and(|s| s.nitro) {
                            0.55
                        } else {
                            0.18
                        },
                    );
                }
            }
            Part::Mark => {
                band(&mut n, lay.track, s.gas_full, s.gas_full, k);
                n.height = Val::Px(1.5);
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
                n.height = Val::Px(if u.is_some() { 8.0 } else { 0.0 });
                n.left = Val::Px(5.0);
                n.right = Val::Px(5.0);
                if let Some(u) = u {
                    let h = lay.track.height() - 4.0;
                    n.bottom = Val::Px(((u * h) as f32 - 4.0) / k);
                }
            }
            Part::Tap(i) => {
                place(&mut n, lay.taps[i], k);
                if let Some(mut bg) = bg {
                    bg.0 = if t.lit.is_some_and(|(j, _)| j == i) {
                        Color::srgba(1.0, 1.0, 1.0, 0.26)
                    } else {
                        PAD
                    };
                }
            }
        }
    }
}
