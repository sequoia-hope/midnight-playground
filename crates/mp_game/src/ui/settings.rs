//! The Settings screen (D1196): every option the main menu used to carry
//! in its `.menu-opts` row, as a list of labelled rows in three groups,
//! Sound, Picture and Driving, over the menu or the pause screen it was
//! opened from. The controls keep their ids (`opt-*`, `vol-*`, `btn-pad`,
//! `link-music`), so the bridge and the suites know them.

use super::menu::{select_options, shown};
use super::screens::{self, Cx};
use super::store::Settings;
use super::widgets::{self as w, Bp, Control, T, Value};
use super::{Act, Opt, Sel, Sl, UiState};
use crate::play::Play;
use bevy::prelude::*;
use bevy::text::{Justify, TextLayout};

/// A row's label on the left and its control on the right.
fn row(
    p: &mut ChildSpawnerCommands,
    bp: &Bp,
    label: &str,
    ot: T,
    control: impl FnOnce(&mut ChildSpawnerCommands),
) {
    p.spawn((
        Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            column_gap: bp.px(12.0),
            padding: UiRect::vertical(bp.px(if bp.compact { 5.0 } else { 7.0 })),
            border: UiRect::bottom(bp.px(1.0)),
            ..default()
        },
        BorderColor::all(w::white(0.08)),
    ))
    .with_children(|p| {
        p.spawn(w::text(label, ot, bp.k));
        control(p);
    });
}

/// A row that is itself the checkbox: the whole row toggles it, as a
/// `<label>` does, and the box sits at the right.
fn check_row(
    p: &mut ChildSpawnerCommands,
    cx: &mut Cx,
    id: &str,
    o: Opt,
    on: bool,
    label: &str,
    ot: T,
) {
    let bp = cx.bp;
    let f = cx.f(id);
    let mut e = p.spawn((
        Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            column_gap: bp.px(12.0),
            padding: UiRect::vertical(bp.px(if bp.compact { 5.0 } else { 7.0 })),
            border: UiRect::bottom(bp.px(1.0)),
            ..default()
        },
        BorderColor::all(w::white(0.08)),
        Control::act(id, Act::Toggle(o)).value(Value::Bool(on)),
    ));
    w::focus_ring(&mut e, &bp, f);
    e.with_children(|p| {
        p.spawn(w::text(label, ot, bp.k));
        w::check_mark(p, &bp, on, cx.icons);
    });
}

fn select_row(
    p: &mut ChildSpawnerCommands,
    cx: &mut Cx,
    sel: Sel,
    label: &str,
    s: &Settings,
    ot: T,
) {
    let bp = cx.bp;
    let id = super::sel_id(sel);
    let f = cx.f(id);
    let (_, value) = select_options(sel, s);
    let shown = shown(sel, s);
    let icons = cx.icons;
    row(p, &bp, label, ot, |p| {
        w::select(
            p,
            &bp,
            Control::act(id, Act::Open(sel)).value(Value::Text(value)),
            &shown,
            icons,
            f,
        );
    });
}

fn slider_row(
    p: &mut ChildSpawnerCommands,
    cx: &mut Cx,
    id: &str,
    sl: Sl,
    v: f64,
    label: &str,
    ot: T,
) {
    let bp = cx.bp;
    let f = cx.f(id);
    let width = if bp.compact { 110.0 } else { 150.0 };
    row(p, &bp, label, ot, |p| {
        w::slider(p, &bp, Control::act(id, Act::Slide(sl)), sl, v, width, f);
    });
}

/// A group's heading (`YOUR CAR`'s style on the menu).
fn heading(p: &mut ChildSpawnerCommands, bp: &Bp, s: &str, first: bool) {
    p.spawn((
        w::text(s, T::new(12.0).ls(0.35).c(w::accent2()), bp.k),
        Node {
            margin: UiRect::new(
                Val::Px(0.0),
                Val::Px(0.0),
                bp.px(if first { 0.0 } else { 16.0 }),
                bp.px(4.0),
            ),
            ..default()
        },
    ));
}

/// `#settings`: the title, the groups, the buttons under them.
pub fn settings(p: &mut ChildSpawnerCommands, cx: &mut Cx, ui: &UiState, play: &Play) -> Entity {
    let bp = cx.bp;
    let s = &ui.settings;
    let touch = play.touch_ui;
    let col = bp.column().min(560.0);
    let ot = T::new(if bp.compact { 14.0 } else { 15.0 }).c(w::fg());
    screens::screen(p, cx, screens::Kind::Other, |p, cx| {
        screens::title(p, &bp, "settings-title", "Settings");
        p.spawn((
            Node {
                width: bp.px(col),
                flex_direction: FlexDirection::Column,
                padding: if bp.compact {
                    UiRect::axes(bp.px(14.0), bp.px(10.0))
                } else {
                    UiRect::axes(bp.px(20.0), bp.px(16.0))
                },
                border: w::border(&bp, 1.0),
                border_radius: w::radius(&bp, 14.0),
                ..default()
            },
            BackgroundColor(w::white(0.05)),
            BorderColor::all(w::white(0.12)),
            Control::named("settings-card", Value::None),
        ))
        .with_children(|p| {
            // Sound.
            heading(p, &bp, "SOUND", true);
            slider_row(p, cx, "vol-music", Sl::Music, s.music, "Music", ot);
            slider_row(p, cx, "vol-sfx", Sl::Sfx, s.sfx, "SFX", ot);
            select_row(p, cx, Sel::Track, "Track", s, ot);
            // The radio (D1160): off by default (the music); on, its station.
            let radio = s.station != "playlist";
            check_row(p, cx, "opt-radio", Opt::Radio, radio, "Radio", ot);
            if radio {
                select_row(p, cx, Sel::Station, "Station", s, ot);
            }
            check_row(
                p,
                cx,
                "opt-classic-engine",
                Opt::ClassicEngine,
                s.classic_engine,
                "Classic engine sound",
                ot,
            );

            // Picture.
            heading(p, &bp, "PICTURE", false);
            check_row(p, cx, "opt-hq", Opt::Hq, s.hq, "High quality", ot);
            // The render scale (D1194), on the web, where the canvas's
            // pixel ratio is the client's to set (natively the window
            // draws at 1).
            #[cfg(target_arch = "wasm32")]
            select_row(p, cx, Sel::Scale, "Render scale", s, ot);
            check_row(
                p,
                cx,
                "opt-flash",
                Opt::Flash,
                s.flash,
                "Police lights flash",
                ot,
            );
            if touch {
                check_row(
                    p,
                    cx,
                    "opt-fullscreen",
                    Opt::Fullscreen,
                    s.fullscreen,
                    "Fullscreen",
                    ot,
                );
            }
            check_row(p, cx, "opt-mph", Opt::Mph, s.mph, "Speed in MPH", ot);

            // Driving: the aids (Rust only, D1083), and the touch controls.
            heading(p, &bp, "DRIVING", false);
            select_row(p, cx, Sel::Guide, "Guide line", s, ot);
            select_row(p, cx, Sel::Assist, "Steer assist", s, ot);
            select_row(p, cx, Sel::Physics, "Handling", s, ot);
            if touch {
                check_row(
                    p,
                    cx,
                    "opt-autogas",
                    Opt::Autogas,
                    s.autogas,
                    "Auto gas",
                    ot,
                );
                select_row(p, cx, Sel::Steer, "Steering", s, ot);
                select_row(p, cx, Sel::Pedals, "Pedals", s, ot);
                if s.steering == "tilt" {
                    slider_row(
                        p,
                        cx,
                        "opt-tilt-sens",
                        Sl::TiltSens,
                        s.tilt_sens,
                        "Tilt sensitivity",
                        ot,
                    );
                }
                let note = super::menu::tilt_note(s);
                if !note.is_empty() {
                    p.spawn((
                        Node {
                            margin: UiRect::top(bp.px(8.0)),
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                        Control::named("tilt-note", Value::Text(note.into())),
                    ))
                    .with_children(|p| {
                        p.spawn((
                            w::text(note, T::new(14.0).c(w::rgb(0xffb454)), bp.k),
                            TextLayout::justify(Justify::Center),
                        ));
                    });
                }
            }
        });

        // Under the card: the controller, the music player, Done.
        p.spawn(Node {
            flex_wrap: FlexWrap::Wrap,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            column_gap: bp.px(14.0),
            row_gap: bp.px(10.0),
            max_width: bp.px(col),
            ..default()
        })
        .with_children(|p| {
            if ui.pads {
                let f = cx.f("btn-pad");
                w::button_mini(
                    p,
                    &bp,
                    Control::act("btn-pad", Act::PadSetup),
                    "Controller setup",
                    None,
                    f,
                );
            }
            #[cfg(target_arch = "wasm32")]
            {
                // The music player is the JS page until M5's player screen.
                let f = cx.f("link-music");
                let mut e = p.spawn((
                    Node {
                        align_items: AlignItems::Center,
                        column_gap: bp.px(4.0),
                        ..default()
                    },
                    Control::act("link-music", Act::MusicLink)
                        .value(Value::Text("Music player".into())),
                ));
                w::focus_ring(&mut e, &bp, f);
                e.with_children(|p| {
                    w::icon_node(p, &bp, &cx.icons.note, 14.0, Color::WHITE);
                    p.spawn(w::text(
                        "Music player",
                        T::new(ot.size).bold().ls(0.04).c(w::accent2()),
                        bp.k,
                    ));
                });
            }
        });
        let f = cx.f("btn-settings-done");
        w::button(
            p,
            &bp,
            Control::act("btn-settings-done", Act::SettingsDone),
            "Done",
            true,
            f,
        );
    })
}
