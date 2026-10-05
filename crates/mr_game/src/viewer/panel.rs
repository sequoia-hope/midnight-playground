//! The viewer's panel and overlay (SPEC 8.6), in Bevy UI through the
//! widget module's tokens, type and text (`ui::widgets`):
//!
//! - the panel, top left, folding to its header: the cameras, the route
//!   position and zone, the time of day, the toggles, the free camera's
//!   speed (or the ride's speed and height), the scene groups, the
//!   readout, screenshot and link, and the controls for this device;
//! - the overview's route, drawn over the map with its zone boundaries
//!   and names, and the middle of the screen marked (the pad's A);
//! - on a touch screen, the move stick (bottom left) and the rise and sink
//!   buttons (bottom right).
//!
//! The panel is rebuilt when its shape changes (`Viewer::dirty`); the
//! numbers in it, the sliders and the overlay move in place each frame.
//! Every control carries an id (`vw-…`) for the test bridge, as the
//! menus' do.

use super::cams::{self, Pose};
use super::link::Mode;
use super::{Act, GROUP_NAMES, Sl, Viewer};
use crate::TrackRes;
use crate::status::Status;
use crate::ui::widgets::{self as w, Bp, T};
use bevy::math::DVec3;
use bevy::prelude::*;
use bevy::text::TextLayout;
use bevy::ui::{ScrollPosition, UiTransform, Val2};
use mr_track::Track;

/// A control: its id for the bridge, what it does, its value and state.
#[derive(Component, Clone, Debug)]
pub struct VControl {
    pub id: String,
    pub act: Option<Act>,
    pub value: serde_json::Value,
    /// Read by the web's bridge.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub sel: bool,
}

impl VControl {
    fn new(id: impl Into<String>, act: Option<Act>, value: serde_json::Value, sel: bool) -> Self {
        VControl {
            id: id.into(),
            act,
            value,
            sel,
        }
    }
}

/// A slider's track (pressed and dragged).
#[derive(Component)]
pub struct VSlider(pub Sl);

/// A button held: rise (+1) or sink (−1).
#[derive(Component)]
pub struct VHold(pub f64);

/// The panel's box: a pointer that starts in it belongs to the panel.
#[derive(Component)]
pub struct PanelArea;

/// The panel's scrolling body.
#[derive(Component)]
pub struct PanelBody;

/// The touch controls' root.
#[derive(Component)]
pub struct TouchRoot;

/// What moves in place.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub enum Dyn {
    Route,
    Tod,
    Speed,
    Height,
    Readout,
    Fill(Sl),
    Thumb(Sl),
}

#[derive(Component)]
pub struct OverlayRoot;
#[derive(Component)]
pub struct Seg(usize);
#[derive(Component)]
pub struct Mark(usize);
#[derive(Component)]
pub struct Label(usize);
#[derive(Component)]
pub struct Cross;
#[derive(Component)]
pub struct Knob;

/// Route segments drawn on the overview.
const SEGS: usize = 240;
/// The stick's radius and its distance from the corner, CSS px.
const STICK_R: f32 = 52.0;
const STICK_IN: f32 = 26.0;
const HOLD: f32 = 58.0;

/// The breakpoints of the viewer's window.
pub fn bp(v: &Viewer) -> Bp {
    let css = v.css.max(0.01) as f32;
    Bp::new(
        v.view.0 as f32 * css,
        v.view.1 as f32 * css,
        1.0 / css,
        v.touch,
        v.insets[0],
    )
}

/// The stick's centre (logical px) on a touch screen.
pub fn stick_centre(v: &Viewer) -> Option<Vec2> {
    if !v.touch {
        return None;
    }
    let k = 1.0 / v.css.max(0.01) as f32;
    Some(Vec2::new(
        (v.insets[3] + STICK_IN + STICK_R) * k,
        v.view.1 as f32 - (v.insets[2] + STICK_IN + STICK_R) * k,
    ))
}

pub fn stick_radius(v: &Viewer) -> f32 {
    STICK_R / v.css.max(0.01) as f32
}

fn chip(p: &mut ChildSpawnerCommands, bp: &Bp, id: &str, act: Act, label: &str, sel: bool) {
    let c = bp.compact;
    p.spawn((
        Node {
            padding: UiRect::axes(
                bp.px(if c { 8.0 } else { 10.0 }),
                bp.px(if c { 3.0 } else { 4.0 }),
            ),
            border: w::border(bp, 1.0),
            border_radius: w::pill(),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BorderColor::all(if sel { w::accent() } else { w::white(0.22) }),
        BackgroundColor(if sel {
            w::rgba(0xff3860, 0.28)
        } else {
            w::white(0.06)
        }),
        VControl::new(id, Some(act), serde_json::Value::Bool(sel), sel),
    ))
    .with_children(|p| {
        p.spawn((
            w::text(
                label.to_uppercase(),
                T::new(if c { 11.0 } else { 12.0 })
                    .bold()
                    .ls(0.1)
                    .c(if sel { Color::WHITE } else { w::dim() }),
                bp.k,
            ),
            TextLayout::no_wrap(),
        ));
    });
}

fn label_row(p: &mut ChildSpawnerCommands, bp: &Bp, label: &str, value: Option<(Dyn, &str)>) {
    p.spawn(Node {
        justify_content: JustifyContent::SpaceBetween,
        align_items: AlignItems::Baseline,
        column_gap: bp.px(8.0),
        ..default()
    })
    .with_children(|p| {
        p.spawn((
            w::text(label, T::new(10.5).bold().ls(0.3).c(w::accent2()), bp.k),
            TextLayout::no_wrap(),
        ));
        if let Some((d, s)) = value {
            p.spawn((
                w::text(s, T::new(12.0).c(w::fg()), bp.k),
                TextLayout::no_wrap(),
                d,
            ));
        }
    });
}

fn slider(p: &mut ChildSpawnerCommands, bp: &Bp, id: &str, sl: Sl, v01: f64) {
    let v = (v01.clamp(0.0, 1.0) * 100.0) as f32;
    p.spawn((
        Node {
            width: Val::Percent(100.0),
            height: bp.px(20.0),
            align_items: AlignItems::Center,
            ..default()
        },
        VSlider(sl),
        VControl::new(id, None, serde_json::json!((v01 * 100.0).round()), false),
    ))
    .with_children(|p| {
        p.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                height: bp.px(4.0),
                border_radius: w::radius(bp, 2.0),
                ..default()
            },
            BackgroundColor(w::white(0.25)),
        ));
        p.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                width: Val::Percent(v),
                height: bp.px(4.0),
                border_radius: w::radius(bp, 2.0),
                ..default()
            },
            BackgroundColor(w::accent()),
            Dyn::Fill(sl),
        ));
        p.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(v),
                margin: UiRect::left(bp.px(-8.0)),
                width: bp.px(16.0),
                height: bp.px(16.0),
                border_radius: w::pill(),
                ..default()
            },
            BackgroundColor(w::accent()),
            Dyn::Thumb(sl),
        ));
    });
}

fn button(p: &mut ChildSpawnerCommands, bp: &Bp, id: &str, act: Act, label: &str) {
    p.spawn((
        Node {
            flex_grow: 1.0,
            padding: UiRect::axes(bp.px(10.0), bp.px(if bp.compact { 4.0 } else { 6.0 })),
            border: w::border(bp, 1.0),
            border_radius: w::pill(),
            justify_content: JustifyContent::Center,
            ..default()
        },
        BorderColor::all(w::white(0.25)),
        BackgroundColor(w::white(0.08)),
        VControl::new(id, Some(act), serde_json::json!(label), false),
    ))
    .with_children(|p| {
        p.spawn((
            w::text(label.to_uppercase(), T::new(12.0).bold().ls(0.12), bp.k),
            TextLayout::no_wrap(),
        ));
    });
}

/// The controls, for this device.
fn help_lines(touch: bool) -> &'static [&'static str] {
    if touch {
        &[
            "One finger looks (orbits, drags the map); two pinch to zoom and pan.",
            "The stick moves; \u{2191} \u{2193} rise and sink.",
            "Tap the overview to fly there.",
        ]
    } else {
        &[
            "Free: WASD move, E or Space up, Q or C down, Shift fast, drag to look, wheel speed.",
            "Orbit: drag to circle, wheel to zoom, WASD moves the centre.",
            "Overview: drag to pan, right-drag turns, wheel zooms, click to fly there (Enter: the middle).",
            "Ride: W/S speed, Q/E height, A/D side, drag to look.",
            "1-4 cameras, M next; F fog, G far plane, T animate, Y time follows; [ ] time; PgUp/PgDn route; Home start; K shot; L link; P panel.",
            "Pad: left stick moves, right looks, LT/RT down and up, LB/RB speed or zoom, Y camera, X panel, A dives, D-pad route and time, Start shot.",
        ]
    }
}

fn speed_text(v: &Viewer) -> String {
    match v.mode {
        Mode::Ride => format!("{:.0} m/s", v.ride.speed),
        _ => format!("{:.0} m/s", v.free.speed),
    }
}

fn route_text(v: &Viewer) -> String {
    format!("{:.2} km  {}", v.s / 1000.0, v.zone)
}

fn tod_text(v: &Viewer, track: Option<&Track>) -> String {
    let p = track.map_or(0.0, |t| v.tod_now(t));
    match v.tod {
        Some(_) => format!("pinned at {:.0}%", p * 100.0),
        None => format!("follows the camera ({:.0}%)", p * 100.0),
    }
}

fn mem_mb() -> Option<f64> {
    #[cfg(target_arch = "wasm32")]
    {
        Some(core::arch::wasm32::memory_size(0) as f64 * 65536.0 / 1048576.0)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // The resident set, natively.
        let s = std::fs::read_to_string("/proc/self/statm").ok()?;
        let pages: f64 = s.split_whitespace().nth(1)?.parse().ok()?;
        Some(pages * 4096.0 / 1048576.0)
    }
}

/// The readout's lines.
pub fn readout(v: &Viewer) -> String {
    let p = v.pose;
    let c = super::stats::counts();
    let mem = mem_mb().map_or(String::new(), |m| {
        if cfg!(target_arch = "wasm32") {
            format!("wasm {m:.0} MB")
        } else {
            format!("memory {m:.0} MB")
        }
    });
    format!(
        "x {:.1}  y {:.1}  z {:.1}\nyaw {:.0}\u{b0}  pitch {:.0}\u{b0}  route {:.0} m\n{:.1} ms (worst {:.0})  {:.0} fps\n{} draws  {:.2} M tris (shadow {}, {:.2} M)\n{}",
        p.pos.x,
        p.pos.y,
        p.pos.z,
        p.yaw.to_degrees(),
        p.pitch.to_degrees(),
        v.s,
        v.frame_ms,
        v.worst_ms,
        1000.0 / v.frame_ms.max(0.1),
        c.draws,
        c.tris as f64 / 1e6,
        c.shadow_draws,
        c.shadow_tris as f64 / 1e6,
        mem
    )
}

/// Rebuilds the panel (and the touch controls) when its shape changed.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn build(
    mut commands: Commands,
    mut v: ResMut<Viewer>,
    tr: Res<TrackRes>,
    roots: Query<Entity, Or<(With<PanelArea>, With<TouchRoot>)>>,
    body: Query<&ScrollPosition, With<PanelBody>>,
    fonts: Option<Res<w::UiFonts>>,
) {
    if !v.dirty || fonts.is_none() {
        return;
    }
    v.dirty = false;
    let scroll = body.single().map_or(0.0, |s| s.0.y);
    for e in &roots {
        commands.entity(e).despawn();
    }
    let bp = bp(&v);
    let v = &*v;
    let track = tr.track.as_ref();
    let c = bp.compact;
    let width = (if c { 250.0f32 } else { 300.0 }).min(bp.w - 16.0);
    let m = 8.0;
    super::stats::COUNTING.store(!v.folded || v.count, std::sync::atomic::Ordering::Relaxed);
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: bp.px(m + v.insets[3]),
                top: bp.px(m + v.insets[0]),
                width: bp.px(width),
                max_height: bp.px(bp.h - 2.0 * m - v.insets[0] - v.insets[2]),
                flex_direction: FlexDirection::Column,
                row_gap: bp.px(8.0),
                padding: UiRect::all(bp.px(if c { 8.0 } else { 10.0 })),
                border: w::border(&bp, 1.0),
                border_radius: w::radius(&bp, 12.0),
                ..default()
            },
            BackgroundColor(w::rgba(0x080a12, 0.8)),
            BorderColor::all(w::white(0.14)),
            GlobalZIndex(20),
            PanelArea,
            VControl::new("vw-panel", None, serde_json::json!(!v.folded), false),
        ))
        .with_children(|p| {
            // The header.
            p.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: bp.px(8.0),
                ..default()
            })
            .with_children(|p| {
                chip(
                    p,
                    &bp,
                    "vw-fold",
                    Act::Fold,
                    if v.folded { "+" } else { "\u{2013}" },
                    false,
                );
                p.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    ..default()
                })
                .with_children(|p| {
                    p.spawn((
                        w::text(
                            "LEVEL VIEWER",
                            T::new(11.0).bold().ls(0.3).c(w::accent()),
                            bp.k,
                        ),
                        TextLayout::no_wrap(),
                    ));
                    let title = mr_levels::level_by_id(v.level()).title;
                    p.spawn((
                        w::text(title, T::new(if c { 14.0 } else { 16.0 }).bold(), bp.k),
                        TextLayout::no_wrap(),
                        VControl::new("vw-level", None, serde_json::json!(v.level()), false),
                    ));
                });
                chip(p, &bp, "vw-menu", Act::Menu, "Menu", false);
            });
            if v.folded {
                return;
            }
            p.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: bp.px(if c { 6.0 } else { 8.0 }),
                    overflow: Overflow::scroll_y(),
                    flex_shrink: 1.0,
                    min_height: Val::Px(0.0),
                    ..default()
                },
                ScrollPosition(Vec2::new(0.0, scroll)),
                PanelBody,
            ))
            .with_children(|p| {
                // The cameras.
                p.spawn(Node {
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: bp.px(5.0),
                    row_gap: bp.px(5.0),
                    ..default()
                })
                .with_children(|p| {
                    for m in Mode::ALL {
                        chip(
                            p,
                            &bp,
                            &format!("vw-mode-{}", m.key()),
                            Act::Mode(m),
                            m.label(),
                            v.mode == m,
                        );
                    }
                });
                // The route.
                label_row(p, &bp, "ROUTE", Some((Dyn::Route, &route_text(v))));
                slider(p, &bp, "vw-route", Sl::Route, v.slider(Sl::Route, track));
                // The time of day.
                label_row(p, &bp, "TIME OF DAY", Some((Dyn::Tod, &tod_text(v, track))));
                p.spawn(Node {
                    align_items: AlignItems::Center,
                    column_gap: bp.px(8.0),
                    ..default()
                })
                .with_children(|p| {
                    chip(p, &bp, "vw-follow", Act::Follow, "Follow", v.tod.is_none());
                    p.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    })
                    .with_children(|p| slider(p, &bp, "vw-tod", Sl::Tod, v.slider(Sl::Tod, track)));
                });
                // The toggles.
                p.spawn(Node {
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: bp.px(5.0),
                    row_gap: bp.px(5.0),
                    ..default()
                })
                .with_children(|p| {
                    chip(p, &bp, "vw-fog", Act::Fog, "Fog", v.fog);
                    chip(p, &bp, "vw-far", Act::Far, "Far plane", v.far);
                    chip(p, &bp, "vw-anim", Act::Anim, "Animate", v.anim);
                });
                // Speed, and the ride's height.
                if matches!(v.mode, Mode::Free | Mode::Ride) {
                    label_row(p, &bp, "SPEED", Some((Dyn::Speed, &speed_text(v))));
                    slider(p, &bp, "vw-speed", Sl::Speed, v.slider(Sl::Speed, track));
                }
                if v.mode == Mode::Ride {
                    label_row(
                        p,
                        &bp,
                        "HEIGHT",
                        Some((Dyn::Height, &format!("{:.1} m", v.ride.h))),
                    );
                    slider(p, &bp, "vw-height", Sl::Height, v.slider(Sl::Height, track));
                }
                // The scene groups.
                GROUP_NAMES.with_names(|names| {
                    if names.is_empty() {
                        return;
                    }
                    label_row(p, &bp, "SCENE GROUPS", None);
                    p.spawn(Node {
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: bp.px(5.0),
                        row_gap: bp.px(5.0),
                        ..default()
                    })
                    .with_children(|p| {
                        for (i, n) in names.iter().enumerate() {
                            let shown = !v.hidden.get(i).copied().unwrap_or(false);
                            chip(p, &bp, &format!("vw-group-{i}"), Act::Group(i), n, shown);
                        }
                    });
                });
                // The readout.
                p.spawn((
                    w::text(
                        readout(v),
                        T::new(if c { 11.0 } else { 12.0 }).c(w::dim()).lh(1.25),
                        bp.k,
                    ),
                    Dyn::Readout,
                    VControl::new("vw-readout", None, serde_json::Value::Null, false),
                ));
                // Screenshot and link.
                p.spawn(Node {
                    column_gap: bp.px(6.0),
                    ..default()
                })
                .with_children(|p| {
                    button(p, &bp, "vw-shot", Act::Shot, "Screenshot");
                    button(p, &bp, "vw-link", Act::Link, "Copy link");
                });
                if let Some((note, _)) = &v.note {
                    p.spawn((
                        w::text(note.as_str(), T::new(12.0).c(w::gold()), bp.k),
                        VControl::new("vw-note", None, serde_json::json!(note), false),
                    ));
                }
                chip(
                    p,
                    &bp,
                    "vw-help",
                    Act::Help,
                    if v.help { "Hide controls" } else { "Controls" },
                    v.help,
                );
                if v.help {
                    for l in help_lines(v.touch) {
                        p.spawn(w::text(*l, T::new(12.0).c(w::dim()).lh(1.3), bp.k));
                    }
                }
            });
        });
    if v.touch {
        touch_controls(&mut commands, &bp, v);
    }
}

fn touch_controls(commands: &mut Commands, bp: &Bp, v: &Viewer) {
    let ink = w::white(0.18);
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            GlobalZIndex(15),
            TouchRoot,
        ))
        .with_children(|p| {
            // The stick.
            let d = 2.0 * STICK_R;
            p.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: bp.px(v.insets[3] + STICK_IN),
                    bottom: bp.px(v.insets[2] + STICK_IN),
                    width: bp.px(d),
                    height: bp.px(d),
                    border: w::border(bp, 2.0),
                    border_radius: w::pill(),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BorderColor::all(w::white(0.35)),
                BackgroundColor(ink),
                VControl::new("vw-stick", None, serde_json::Value::Null, false),
            ))
            .with_children(|p| {
                p.spawn((
                    Node {
                        width: bp.px(46.0),
                        height: bp.px(46.0),
                        border_radius: w::pill(),
                        ..default()
                    },
                    BackgroundColor(w::white(0.55)),
                    UiTransform::IDENTITY,
                    Knob,
                ));
            });
            // Rise and sink.
            for (i, (id, label, dir)) in
                [("vw-rise", "\u{2191}", 1.0), ("vw-sink", "\u{2193}", -1.0)]
                    .into_iter()
                    .enumerate()
            {
                p.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        right: bp.px(v.insets[1] + STICK_IN),
                        bottom: bp.px(v.insets[2] + STICK_IN + (1 - i) as f32 * (HOLD + 14.0)),
                        width: bp.px(HOLD),
                        height: bp.px(HOLD),
                        border: w::border(bp, 2.0),
                        border_radius: w::pill(),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BorderColor::all(w::white(0.35)),
                    BackgroundColor(ink),
                    VHold(dir),
                    VControl::new(id, None, serde_json::Value::Null, false),
                ))
                .with_children(|p| {
                    p.spawn(w::text(label, T::new(26.0).bold(), bp.k));
                });
            }
        });
}

/// The numbers, the sliders and the stick's knob, in place.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn refresh(
    v: Res<Viewer>,
    tr: Res<TrackRes>,
    mut texts: Query<(&Dyn, &mut Text)>,
    mut nodes: Query<(&Dyn, &mut Node), Without<Text>>,
    mut ctls: Query<(&mut VControl, Option<&VSlider>)>,
    mut knob: Query<&mut UiTransform, With<Knob>>,
    ptr: Res<super::input::Pointers>,
    mut last_readout: Local<f64>,
) {
    let track = tr.track.as_ref();
    let fresh = v.clock - *last_readout >= 0.25;
    if fresh {
        *last_readout = v.clock;
    }
    for (d, mut t) in &mut texts {
        let s = match d {
            Dyn::Route => route_text(&v),
            Dyn::Tod => tod_text(&v, track),
            Dyn::Speed => speed_text(&v),
            Dyn::Height => format!("{:.1} m", v.ride.h),
            Dyn::Readout if fresh => readout(&v),
            _ => continue,
        };
        if t.0 != s {
            t.0 = s;
        }
    }
    for (d, mut n) in &mut nodes {
        let (sl, thumb) = match d {
            Dyn::Fill(sl) => (*sl, false),
            Dyn::Thumb(sl) => (*sl, true),
            _ => continue,
        };
        let x = Val::Percent((v.slider(sl, track) * 100.0) as f32);
        if thumb {
            if n.left != x {
                n.left = x;
            }
        } else if n.width != x {
            n.width = x;
        }
    }
    for (mut c, s) in &mut ctls {
        if let Some(s) = s {
            let x = serde_json::json!((v.slider(s.0, track) * 100.0).round());
            if c.value != x {
                c.value = x;
            }
        } else if c.id == "vw-readout" && fresh {
            c.value = serde_json::json!(readout(&v));
        }
    }
    if let Ok(mut k) = knob.single_mut() {
        let r = stick_radius(&v);
        let off = ptr.stick.unwrap_or(Vec2::ZERO) * r;
        let t = Val2::px(off.x, off.y);
        if k.translation != t {
            k.translation = t;
        }
    }
}

/// The overview's route, zone marks and names, and the middle of the
/// screen.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn overlay(
    mut commands: Commands,
    v: Res<Viewer>,
    tr: Res<TrackRes>,
    status: Res<Status>,
    root: Query<Entity, With<OverlayRoot>>,
    mut segs: Query<(&Seg, &mut UiTransform, &mut Visibility), (Without<Mark>, Without<Label>)>,
    mut marks: Query<(&Mark, &mut UiTransform, &mut Visibility), (Without<Seg>, Without<Label>)>,
    mut labels: Query<(&Label, &mut UiTransform, &mut Visibility), (Without<Seg>, Without<Mark>)>,
    mut root_vis: Query<
        &mut Visibility,
        (
            With<OverlayRoot>,
            Without<Seg>,
            Without<Mark>,
            Without<Label>,
        ),
    >,
    fonts: Option<Res<w::UiFonts>>,
    mut last: Local<Option<(Pose, (f64, f64))>>,
) {
    let Some(track) = &tr.track else { return };
    if fonts.is_none() || !status.ready {
        return;
    }
    let bp = bp(&v);
    let k = bp.k;
    if root.is_empty() {
        // Built once: the segments, then a mark and a name per zone.
        commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                GlobalZIndex(5),
                Visibility::Hidden,
                OverlayRoot,
            ))
            .with_children(|p| {
                for i in 0..SEGS {
                    p.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            top: Val::Px(0.0),
                            width: Val::Px(1.0),
                            height: Val::Px(3.0 * k),
                            ..default()
                        },
                        BackgroundColor(w::rgba(0x3ad7ff, 0.9)),
                        UiTransform::IDENTITY,
                        Visibility::Hidden,
                        Seg(i),
                    ));
                }
                for (i, z) in track.zones.iter().enumerate() {
                    p.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            top: Val::Px(0.0),
                            width: Val::Px(1.0),
                            height: Val::Px(3.0 * k),
                            ..default()
                        },
                        BackgroundColor(w::gold()),
                        UiTransform::IDENTITY,
                        Visibility::Hidden,
                        Mark(i),
                    ));
                    p.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            top: Val::Px(0.0),
                            padding: UiRect::axes(bp.px(6.0), bp.px(1.0)),
                            border_radius: w::pill(),
                            ..default()
                        },
                        BackgroundColor(w::rgba(0x080a12, 0.7)),
                        UiTransform::IDENTITY,
                        Visibility::Hidden,
                        Label(i),
                        VControl::new(
                            format!("vw-zone-{i}"),
                            None,
                            serde_json::json!(z.zone.name),
                            false,
                        ),
                    ))
                    .with_children(|p| {
                        p.spawn((
                            w::text(
                                format!("{} {}", i + 1, z.zone.name),
                                T::new(12.0).bold().c(w::gold()),
                                k,
                            ),
                            TextLayout::no_wrap(),
                        ));
                    });
                }
                // The middle of the screen.
                for (wd, ht) in [(18.0, 2.0), (2.0, 18.0)] {
                    p.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Percent(50.0),
                            top: Val::Percent(50.0),
                            margin: UiRect::new(
                                bp.px(-wd / 2.0),
                                Val::Px(0.0),
                                bp.px(-ht / 2.0),
                                Val::Px(0.0),
                            ),
                            width: bp.px(wd),
                            height: bp.px(ht),
                            ..default()
                        },
                        BackgroundColor(w::white(0.6)),
                        Cross,
                    ));
                }
            });
        return;
    }
    let show = v.mode == Mode::Overview && v.flight.is_none();
    if let Ok(mut rv) = root_vis.single_mut() {
        let want = if show {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *rv != want {
            *rv = want;
        }
    }
    if !show {
        *last = None;
        return;
    }
    let key = (v.pose, v.view);
    if *last == Some(key) {
        return;
    }
    *last = Some(key);
    let (vw, vh) = v.view;
    let pose = v.pose;
    let at = |s: f64| {
        let f = track.frame(s);
        cams::project(&pose, vw, vh, DVec3::new(f.x, f.y, f.z))
    };
    let len = if track.is_loop {
        track.length
    } else {
        v.route_len
    };
    let pts: Vec<Option<Vec2>> = (0..=SEGS)
        .map(|i| {
            let s = len * i as f64 / SEGS as f64;
            at(s).map(|(x, y)| Vec2::new(x as f32, y as f32))
        })
        .collect();
    let line = |a: Vec2, b: Vec2, thick: f32, t: &mut UiTransform| {
        let mid = (a + b) / 2.0;
        let d = b - a;
        *t = UiTransform {
            translation: Val2::px(mid.x - 0.5, mid.y - thick / 2.0),
            scale: Vec2::new(d.length().max(0.5), 1.0),
            rotation: Rot2::radians(d.y.atan2(d.x)),
        };
    };
    for (seg, mut t, mut vis) in &mut segs {
        let (Some(a), Some(b)) = (pts[seg.0], pts[seg.0 + 1]) else {
            *vis = Visibility::Hidden;
            continue;
        };
        line(a, b, 3.0 * k, &mut t);
        *vis = Visibility::Inherited;
    }
    for (m, mut t, mut vis) in &mut marks {
        let Some(z) = track.zones.get(m.0) else {
            continue;
        };
        let f = track.frame(z.s0);
        let half = f.hw + 24.0;
        let a = cams::project(
            &pose,
            vw,
            vh,
            DVec3::new(f.x - f.rx * half, f.y, f.z - f.rz * half),
        );
        let b = cams::project(
            &pose,
            vw,
            vh,
            DVec3::new(f.x + f.rx * half, f.y, f.z + f.rz * half),
        );
        match (a, b) {
            (Some(a), Some(b)) => {
                let (a, b) = (
                    Vec2::new(a.0 as f32, a.1 as f32),
                    Vec2::new(b.0 as f32, b.1 as f32),
                );
                // At least 18 CSS px across, centred.
                let mid = (a + b) / 2.0;
                let d = (b - a).normalize_or(Vec2::X) * ((b - a).length().max(18.0 * k) / 2.0);
                line(mid - d, mid + d, 3.0 * k, &mut t);
                *vis = Visibility::Inherited;
            }
            _ => *vis = Visibility::Hidden,
        }
    }
    for (l, mut t, mut vis) in &mut labels {
        let Some(z) = track.zones.get(l.0) else {
            continue;
        };
        match at(z.s0) {
            Some((x, y)) => {
                let tr = Val2::px(x as f32 + 10.0 * k, y as f32 - 9.0 * k);
                if t.translation != tr {
                    t.translation = tr;
                }
                *vis = Visibility::Inherited;
            }
            None => *vis = Visibility::Hidden,
        }
    }
}
