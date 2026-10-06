//! The main menu (`#menu` in `index.html`, `.level-pick` to `.controls` in
//! `hud.css`, `renderLevelCard` and the option bindings in `main.js`): the
//! level tabs, the level card with its stats, best times and the Race / Hot
//! Pursuit switch, the car picks, the Race button, the options, and the
//! touch or keyboard help. On a phone held sideways it is the two-column
//! grid of `(max-height: 500px) and (orientation: landscape)`.

use super::screens::{self, Cx};
use super::store::{self, Settings, Store};
use super::widgets::{self as w, Bp, Control, T, Value};
use super::{Act, Opt, Sel, Sl, UiState};
use crate::Opts;
use crate::play::Play;
use crate::play::flow::fmt_time;
use bevy::prelude::*;
use bevy::text::{Justify, TextLayout};
use mp_track::{Level, level::Mode as LevelMode};

/// `levelStats(l)`.
pub fn level_stats(l: &Level) -> String {
    if l.mode == LevelMode::Cruise {
        return "Endless loop · heavy traffic · score attack".into();
    }
    if let (Some(laps), Some(len)) = (l.laps, l.lap_length) {
        return format!(
            "{laps} laps of {} km · {} mi · {} rivals · no traffic",
            w::to_fixed(len / 1000.0, 1),
            w::to_fixed(len / 1609.34, 2),
            l.rivals.len()
        );
    }
    let total: f64 = l.segments().map_or(0.0, |s| s.iter().map(|x| x.len).sum());
    let len = total - l.finish_runoff.unwrap_or(180.0);
    format!(
        "{} km · {} mi · {} rivals · traffic",
        w::to_fixed(len / 1000.0, 1),
        w::to_fixed(len / 1609.34, 1),
        l.rivals.len()
    )
}

/// The level card's best line (`#lvl-best`).
pub fn level_best(store: &Store, l: &Level) -> String {
    let cruise = l.mode == LevelMode::Cruise;
    let best = if cruise {
        store.num_or_null(&format!("bestScore.{}", l.id))
    } else {
        let pursuit = store::mode_for(store, l) == "pursuit";
        store.num_or_null(&store::best_key(l.id, pursuit))
    };
    let lap = if l.laps.is_some() {
        store.num_or_null(&format!("bestLap.{}", l.id))
    } else {
        None
    };
    let mut parts = Vec::new();
    if let Some(b) = best {
        parts.push(if cruise {
            format!("Best score {}", w::locale_int(mp_math::js::round(b)))
        } else {
            format!("Best winning time {}", fmt_time(Some(b)))
        });
    }
    if let Some(l) = lap {
        parts.push(format!("Lap record {}", fmt_time(Some(l))));
    }
    parts.join(" · ")
}

/// The Race button's label.
pub fn start_label(store: &Store, l: &Level, forced: Option<bool>) -> &'static str {
    if l.mode == LevelMode::Cruise {
        "Cruise"
    } else if l.police.is_some() && forced.unwrap_or(store::mode_for(store, l) == "pursuit") {
        "Hot Pursuit"
    } else {
        "Race"
    }
}

/// A select's options and its current value.
pub fn select_options(sel: Sel, s: &Settings) -> (Vec<(String, String)>, String) {
    match sel {
        Sel::Track => {
            let mut o = vec![("auto".to_string(), "Level\u{2019}s own".to_string())];
            for t in mp_audio::tracks::TRACKS {
                o.push((t.id.to_string(), format!("{} ({})", t.title, t.style)));
            }
            (o, s.track.clone())
        }
        Sel::Steer => (
            vec![
                ("stick".into(), "Thumb stick".into()),
                // ◂ ▸ are not in the bundled faces (D371): the arrows are.
                ("buttons".into(), "\u{2190} \u{2192} buttons".into()),
                ("tilt".into(), "Tilt".into()),
            ],
            s.steering.clone(),
        ),
        Sel::Pedals => (
            vec![
                ("slider".into(), "Slider".into()),
                ("buttons".into(), "Buttons".into()),
            ],
            s.pedals.clone(),
        ),
        Sel::Guide => (
            vec![
                ("full".into(), "Full".into()),
                ("brake".into(), "Braking only".into()),
                ("off".into(), "Off".into()),
            ],
            s.guide.clone(),
        ),
        Sel::Assist => (
            vec![
                ("off".into(), "Off".into()),
                ("light".into(), "Light".into()),
                ("strong".into(), "Strong".into()),
            ],
            s.assist.clone(),
        ),
    }
}

fn shown(sel: Sel, s: &Settings) -> String {
    let (o, v) = select_options(sel, s);
    o.into_iter()
        .find(|(k, _)| *k == v)
        .map_or(v, |(_, label)| label)
}

/// `TILT_NOTES[tilt.state]` when tilt is chosen (`showTiltState`; the
/// sensor is `play::tilt`, WP 6.6).
fn tilt_note(s: &Settings) -> &'static str {
    if s.steering == "tilt" {
        crate::play::tilt::note(crate::play::tilt::state())
    } else {
        ""
    }
}

/// `#rotate-hint`: on a portrait phone's menu.
pub fn rotate_hint(p: &mut ChildSpawnerCommands, bp: &Bp) {
    p.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: bp.px(12f32.max(bp.inset_top)),
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        GlobalZIndex(12),
    ))
    .with_children(|p| {
        p.spawn((
            Node {
                padding: UiRect::axes(bp.px(14.0), bp.px(6.0)),
                border: w::border(bp, 1.0),
                border_radius: w::pill(),
                ..default()
            },
            BorderColor::all(w::white(0.2)),
            BackgroundColor(w::rgba(0x080a12, 0.75)),
            Control::named(
                "rotate-hint",
                Value::Text("Turn your phone sideways for the best view".into()),
            ),
        ))
        .with_children(|p| {
            p.spawn((
                w::text(
                    "Turn your phone sideways for the best view",
                    T::new(13.0).ls(0.08).c(w::dim()),
                    bp.k,
                ),
                TextLayout::no_wrap(),
            ));
        });
    });
}

/// Where a menu part sits in the two-column grid.
fn place(bp: &Bp, n: &mut Node, col: (i16, i16), row: (i16, u16)) {
    if bp.two_columns() {
        n.grid_column = GridPlacement::start_end(col.0, col.1);
        n.grid_row = GridPlacement::start_span(row.0, row.1);
    }
}

pub fn menu(
    p: &mut ChildSpawnerCommands,
    cx: &mut Cx,
    ui: &UiState,
    store: &Store,
    play: &Play,
    opts: &Opts,
) -> Entity {
    let bp = cx.bp;
    let s = &ui.settings;
    let level = mp_levels::level_by_id(&s.level);
    let forced = opts.o.param("pursuit").map(|v| v == "1");
    let col = bp.column();
    let two = bp.two_columns();
    screens::screen(p, cx, screens::Kind::Menu, |p, cx| {
        // The logo.
        screens::logo(p, cx, |n| {
            if two {
                n.grid_column = GridPlacement::start_end(1, -1);
                n.grid_row = GridPlacement::start(1);
                n.justify_self = JustifySelf::Center;
            }
        });

        // The level tabs.
        let mut n = Node {
            width: if two { Val::Auto } else { bp.px(col) },
            flex_wrap: FlexWrap::Wrap,
            column_gap: bp.px(8.0),
            row_gap: bp.px(8.0),
            ..default()
        };
        place(&bp, &mut n, (1, 2), (2, 1));
        p.spawn((n, Control::named("level-pick", Value::None)))
            .with_children(|p| {
                for l in mp_levels::levels() {
                    let sel = l.id == s.level;
                    let id = format!("lvl-tab-{}", l.id);
                    let focused = cx.f(&id);
                    let mut e = p.spawn((
                        Node {
                            flex_grow: 1.0,
                            flex_shrink: 1.0,
                            flex_basis: bp.px(if bp.compact { 120.0 } else { 150.0 }),
                            flex_direction: FlexDirection::Column,
                            padding: if bp.compact {
                                UiRect::axes(bp.px(10.0), bp.px(5.0))
                            } else {
                                UiRect::axes(bp.px(12.0), bp.px(8.0))
                            },
                            border: w::border(&bp, 1.0),
                            border_radius: w::radius(&bp, 10.0),
                            ..default()
                        },
                        BackgroundColor(w::white(0.05)),
                        BorderColor::all(if sel { w::accent() } else { w::white(0.12) }),
                        Control::act(id, Act::Level(l.id))
                            .sel(sel)
                            .value(Value::Text(format!("{}{}", l.num, l.title))),
                    ));
                    if sel {
                        e.insert(w::sel_ring(&bp));
                    }
                    w::focus_ring(&mut e, &bp, focused);
                    e.with_children(|p| {
                        p.spawn(w::text(
                            l.num,
                            T::new(if bp.compact { 9.0 } else { 11.0 })
                                .bold()
                                .ls(0.3)
                                .c(w::accent()),
                            bp.k,
                        ));
                        p.spawn(w::text(
                            l.title,
                            T::new(if bp.compact { 14.0 } else { 17.0 }).bold(),
                            bp.k,
                        ));
                    });
                }
            });

        // The level card.
        let mut n = Node {
            width: if two { Val::Auto } else { bp.px(col) },
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            padding: if bp.compact {
                UiRect::axes(bp.px(14.0), bp.px(10.0))
            } else {
                UiRect::axes(bp.px(20.0), bp.px(16.0))
            },
            border: w::border(&bp, 1.0),
            border_radius: w::radius(&bp, 14.0),
            align_self: if two {
                AlignSelf::Start
            } else {
                AlignSelf::Auto
            },
            ..default()
        };
        place(&bp, &mut n, (1, 2), (3, 3));
        let stats = level_stats(&level);
        let best = level_best(store, &level);
        p.spawn((
            n,
            BackgroundColor(w::white(0.05)),
            BorderColor::all(w::white(0.12)),
        ))
        .with_children(|p| {
            // The level viewer (SPEC 8.6, D679): a pill in the card's
            // corner, out of the layout, so nothing else on the menu moves.
            let f = cx.f("btn-viewer");
            let mut e = p.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    top: bp.px(if bp.compact { 6.0 } else { 10.0 }),
                    right: bp.px(if bp.compact { 10.0 } else { 14.0 }),
                    padding: UiRect::axes(bp.px(10.0), bp.px(3.0)),
                    border: w::border(&bp, 1.0),
                    border_radius: w::pill(),
                    ..default()
                },
                BorderColor::all(w::white(0.25)),
                BackgroundColor(w::white(0.06)),
                Control::act("btn-viewer", Act::Viewer).value(Value::Text("Level viewer".into())),
            ));
            w::focus_ring(&mut e, &bp, f);
            e.with_children(|p| {
                p.spawn((
                    w::text(
                        "LEVEL VIEWER",
                        T::new(11.0).bold().ls(0.12).c(w::accent2()),
                        bp.k,
                    ),
                    TextLayout::no_wrap(),
                ));
            });
            p.spawn((
                w::text(level.num, T::new(12.0).bold().ls(0.4).c(w::accent()), bp.k),
                Control::named("lvl-num", Value::Text(level.num.into())),
            ));
            p.spawn((
                w::text(
                    level.title,
                    T::new(if bp.compact { 22.0 } else { 34.0 }).w(800).italic(),
                    bp.k,
                ),
                Control::named("lvl-name", Value::Text(level.title.into())),
            ));
            let desc = if bp.compact {
                T::new(13.0).lh(1.3)
            } else if bp.short {
                T::new(15.0).lh(1.4)
            } else {
                T::new(16.0).lh(1.4)
            };
            p.spawn((
                w::text(level.desc, desc.c(w::dim()), bp.k),
                Node {
                    margin: UiRect::top(bp.px(4.0)),
                    ..default()
                },
                Control::named("lvl-desc", Value::Text(level.desc.into())),
            ));
            let st = T::new(if bp.compact { 11.0 } else { 13.0 })
                .ls(0.15)
                .c(w::accent2());
            p.spawn(Node {
                margin: UiRect::top(bp.px(8.0)),
                flex_wrap: FlexWrap::Wrap,
                column_gap: bp.px(12.0),
                ..default()
            })
            .with_children(|p| {
                p.spawn((
                    w::text(stats.to_uppercase(), st, bp.k),
                    Control::named("lvl-len", Value::Text(stats.clone())),
                ));
                p.spawn((
                    w::text(best.to_uppercase(), st.c(w::gold()), bp.k),
                    Control::named("lvl-best", Value::Text(best.clone())),
                ));
            });
            if level.police.is_some() {
                let m = if forced.unwrap_or(store::mode_for(store, &level) == "pursuit") {
                    "pursuit"
                } else {
                    "race"
                };
                p.spawn((
                    Node {
                        margin: UiRect::top(bp.px(if bp.compact { 6.0 } else { 10.0 })),
                        padding: UiRect::all(bp.px(3.0)),
                        border: w::border(&bp, 1.0),
                        border_radius: w::pill(),
                        align_self: AlignSelf::Start,
                        ..default()
                    },
                    BackgroundColor(w::rgba(0, 0.25)),
                    BorderColor::all(w::white(0.16)),
                    Control::named("mode-pick", Value::Text(m.into())),
                ))
                .with_children(|p| {
                    for (mode, label) in [("race", "Race"), ("pursuit", "Hot Pursuit")] {
                        let sel = mode == m;
                        let id = format!("mode-{mode}");
                        let focused = cx.f(&id);
                        let mut e = p.spawn((
                            Node {
                                padding: if bp.compact {
                                    UiRect::axes(bp.px(12.0), bp.px(3.0))
                                } else {
                                    UiRect::axes(bp.px(18.0), bp.px(5.0))
                                },
                                border_radius: w::pill(),
                                ..default()
                            },
                            Control::act(id, Act::Mode(mode))
                                .sel(sel)
                                .value(Value::Text(label.into())),
                        ));
                        if sel {
                            if mode == "pursuit" {
                                e.insert((
                                    w::hgrad(w::rgb(0xff3040), w::rgb(0x2f6bff)),
                                    w::shadow(
                                        &bp,
                                        &[(0.0, 4.0, 16.0, 0.0, w::rgba(0x2f6bff, 0.3))],
                                    ),
                                ));
                            } else {
                                e.insert((
                                    w::hgrad(w::accent(), w::rgb(0xff7a3c)),
                                    w::shadow(
                                        &bp,
                                        &[(0.0, 4.0, 16.0, 0.0, w::rgba(0xff3860, 0.3))],
                                    ),
                                ));
                            }
                        }
                        w::focus_ring(&mut e, &bp, focused);
                        e.with_children(|p| {
                            p.spawn((
                                w::text(
                                    label.to_uppercase(),
                                    T::new(if bp.compact { 12.0 } else { 15.0 })
                                        .bold()
                                        .ls(0.14)
                                        .c(if sel { Color::WHITE } else { w::dim() }),
                                    bp.k,
                                ),
                                TextLayout::no_wrap(),
                            ));
                        });
                    }
                });
            }
        });

        // The car picks.
        let mut n = Node {
            width: if two { Val::Auto } else { bp.px(col) },
            flex_direction: FlexDirection::Column,
            ..default()
        };
        place(&bp, &mut n, (2, 3), (2, 2));
        p.spawn(n).with_children(|p| {
            p.spawn((
                w::text("YOUR CAR", T::new(12.0).ls(0.35).c(w::dim()), bp.k),
                Node {
                    margin: UiRect::bottom(bp.px(6.0)),
                    ..default()
                },
            ));
            p.spawn((
                Node {
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: bp.px(10.0),
                    row_gap: bp.px(10.0),
                    ..default()
                },
                Control::named("car-pick", Value::None),
            ))
            .with_children(|p| {
                for (key, spec) in mp_sim::physics::CAR_SPECS.iter() {
                    let sel = *key == s.car;
                    let id = format!("pick-{key}");
                    let focused = cx.f(&id);
                    let mut e = p.spawn((
                        Node {
                            flex_grow: 1.0,
                            flex_shrink: 1.0,
                            flex_basis: bp.px(if bp.compact { 140.0 } else { 150.0 }),
                            flex_direction: FlexDirection::Column,
                            padding: if bp.compact {
                                UiRect::axes(bp.px(10.0), bp.px(6.0))
                            } else {
                                UiRect::axes(bp.px(12.0), bp.px(10.0))
                            },
                            border: w::border(&bp, 1.0),
                            border_radius: w::radius(&bp, 12.0),
                            ..default()
                        },
                        BackgroundColor(w::white(0.05)),
                        BorderColor::all(if sel { w::accent() } else { w::white(0.12) }),
                        Control::act(id, Act::Car(key))
                            .sel(sel)
                            .value(Value::Text(format!("{}{}", spec.label, spec.blurb))),
                    ));
                    if sel {
                        e.insert(w::sel_ring(&bp));
                    }
                    w::focus_ring(&mut e, &bp, focused);
                    let size = if bp.compact { 15.0 } else { 18.0 };
                    e.with_children(|p| {
                        p.spawn(Node {
                            align_items: AlignItems::Center,
                            ..default()
                        })
                        .with_children(|p| {
                            p.spawn((
                                Node {
                                    width: bp.px(14.0),
                                    height: bp.px(14.0),
                                    margin: UiRect::right(bp.px(8.0)),
                                    border_radius: w::pill(),
                                    flex_shrink: 0.0,
                                    ..default()
                                },
                                BackgroundColor(w::rgb(spec.color)),
                            ));
                            p.spawn(w::text(spec.label, T::new(size).bold(), bp.k));
                        });
                        p.spawn(w::text(
                            spec.blurb,
                            T::new(if bp.compact { 11.0 } else { 13.0 }).c(w::dim()),
                            bp.k,
                        ));
                    });
                }
            });
        });

        // Race.
        let label = start_label(store, &level, forced);
        let f = cx.f("btn-start");
        let mut holder = Node {
            align_self: AlignSelf::Center,
            ..default()
        };
        place(&bp, &mut holder, (2, 3), (4, 1));
        if two {
            holder.justify_self = JustifySelf::Center;
        }
        // Multiplayer beside Race: on the web the host is the server the
        // page came from; natively it is `--join ws://host:port/ws`.
        let mp = cfg!(target_arch = "wasm32") || opts.o.param("join").is_some();
        let fm = cx.f("btn-mp");
        p.spawn(holder).with_children(|p| {
            p.spawn(Node {
                flex_wrap: FlexWrap::Wrap,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                column_gap: bp.px(12.0),
                row_gap: bp.px(8.0),
                ..default()
            })
            .with_children(|p| {
                w::button(
                    p,
                    &bp,
                    Control::act("btn-start", Act::Start),
                    label,
                    true,
                    f,
                );
                if mp {
                    w::button(
                        p,
                        &bp,
                        Control::act("btn-mp", Act::Mp(super::lobby::MpAct::Open)),
                        "Multiplayer",
                        false,
                        fm,
                    );
                }
            });
        });

        // The options.
        let mut n = Node {
            flex_wrap: FlexWrap::Wrap,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            column_gap: bp.px(if bp.compact { 14.0 } else { 18.0 }),
            row_gap: bp.px(if bp.compact { 6.0 } else { 8.0 }),
            max_width: Val::Percent(100.0),
            ..default()
        };
        place(&bp, &mut n, (2, 3), (5, 1));
        let ot = T::new(if bp.compact { 14.0 } else { 15.0 }).c(w::dim());
        p.spawn(n).with_children(|p| {
            options(p, cx, s, ot, play.touch_ui, ui.pads);
        });

        // Help: touch or keyboard.
        if bp.touch {
            let mut n = Node {
                width: if two { Val::Auto } else { bp.px(col) },
                flex_direction: FlexDirection::Column,
                ..default()
            };
            place(&bp, &mut n, (1, -1), (6, 1));
            p.spawn((n, Control::named("touch-help", Value::None)))
                .with_children(|p| touch_help(p, &bp));
        } else if !bp.compact {
            p.spawn((
                Node {
                    width: bp.px(col),
                    display: Display::Grid,
                    grid_template_columns: vec![RepeatedGridTrack::flex(
                        ((col + 20.0) / 250.0).floor().max(1.0) as u16,
                        1.0,
                    )],
                    column_gap: bp.px(20.0),
                    row_gap: bp.px(4.0),
                    ..default()
                },
                Control::named("menu-controls", Value::None),
            ))
            .with_children(|p| keyboard_help(p, &bp, ui.pads));
        }

        // Quit, natively (the owner's request, D1060): last on the menu,
        // where a desktop game keeps it, a secondary `.btn` as the pause
        // screen's. The page has none: it cannot close itself.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let f = cx.f("btn-exit");
            let mut holder = Node {
                align_self: AlignSelf::Center,
                ..default()
            };
            place(&bp, &mut holder, (1, -1), (7, 1));
            if two {
                holder.justify_self = JustifySelf::Center;
            }
            p.spawn(holder).with_children(|p| {
                w::button(
                    p,
                    &bp,
                    Control::act("btn-exit", Act::Exit),
                    "Quit",
                    false,
                    f,
                );
            });
        }
    })
}

/// `.menu-opts` on the menu.
fn options(
    p: &mut ChildSpawnerCommands,
    cx: &mut Cx,
    s: &Settings,
    ot: T,
    touch: bool,
    pads: bool,
) {
    let bp = cx.bp;
    let vol = |p: &mut ChildSpawnerCommands, cx: &mut Cx, label: &str, id: &str, sl: Sl, v: f64| {
        let f = cx.f(id);
        p.spawn(Node {
            align_items: AlignItems::Center,
            column_gap: bp.px(8.0),
            ..default()
        })
        .with_children(|p| {
            p.spawn(w::text(label, ot, bp.k));
            w::slider(p, &bp, Control::act(id, Act::Slide(sl)), sl, v, 110.0, f);
        });
    };
    vol(p, cx, "Music", "vol-music", Sl::Music, s.music);
    vol(p, cx, "SFX", "vol-sfx", Sl::Sfx, s.sfx);
    let f = cx.f("opt-track");
    p.spawn(Node {
        align_items: AlignItems::Center,
        column_gap: bp.px(8.0),
        ..default()
    })
    .with_children(|p| {
        p.spawn(w::text("Track", ot, bp.k));
        w::select(
            p,
            &bp,
            Control::act("opt-track", Act::Open(Sel::Track)).value(Value::Text(s.track.clone())),
            &shown(Sel::Track, s),
            cx.icons,
            f,
        );
    });
    let check =
        |p: &mut ChildSpawnerCommands, cx: &mut Cx, id: &str, o: Opt, on: bool, label: &str| {
            let f = cx.f(id);
            w::checkbox(
                p,
                &bp,
                Control::act(id, Act::Toggle(o)),
                label,
                on,
                ot,
                cx.icons,
                f,
            );
        };
    check(p, cx, "opt-mph", Opt::Mph, s.mph, "MPH");
    check(p, cx, "opt-hq", Opt::Hq, s.hq, "High quality");
    check(
        p,
        cx,
        "opt-flash",
        Opt::Flash,
        s.flash,
        "Police lights flash",
    );
    // The driving aids (Rust only, D1083), on every device.
    for (sel, label, value) in [
        (Sel::Guide, "Guide line", &s.guide),
        (Sel::Assist, "Steer assist", &s.assist),
    ] {
        let id = super::sel_id(sel);
        let f = cx.f(id);
        p.spawn(Node {
            align_items: AlignItems::Center,
            column_gap: bp.px(4.0),
            ..default()
        })
        .with_children(|p| {
            p.spawn(w::text(label, ot, bp.k));
            w::select(
                p,
                &bp,
                Control::act(id, Act::Open(sel)).value(Value::Text(value.clone())),
                &shown(sel, s),
                cx.icons,
                f,
            );
        });
    }
    if touch {
        check(p, cx, "opt-autogas", Opt::Autogas, s.autogas, "Auto gas");
        for (sel, label) in [(Sel::Steer, "Steering"), (Sel::Pedals, "Pedals")] {
            let id = super::sel_id(sel);
            let f = cx.f(id);
            let value = if sel == Sel::Steer {
                s.steering.clone()
            } else {
                s.pedals.clone()
            };
            p.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: bp.px(4.0),
                ..default()
            })
            .with_children(|p| {
                p.spawn(w::text(label, ot, bp.k));
                w::select(
                    p,
                    &bp,
                    Control::act(id, Act::Open(sel)).value(Value::Text(value)),
                    &shown(sel, s),
                    cx.icons,
                    f,
                );
            });
        }
        if s.steering == "tilt" {
            vol(p, cx, "Tilt", "opt-tilt-sens", Sl::TiltSens, s.tilt_sens);
        }
        check(
            p,
            cx,
            "opt-fullscreen",
            Opt::Fullscreen,
            s.fullscreen,
            "Fullscreen",
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
            Control::act("link-music", Act::MusicLink).value(Value::Text("Music player".into())),
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
    if pads {
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
    let note = tilt_note(s);
    if touch && !note.is_empty() {
        p.spawn((
            Node {
                flex_basis: Val::Percent(100.0),
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

/// `.touch-help`.
fn touch_help(p: &mut ChildSpawnerCommands, bp: &Bp) {
    let size = if bp.compact { 12.0 } else { 14.0 };
    let n = T::new(size).lh(1.5).c(w::dim());
    let b = T::new(size).bold().lh(1.5).ls(0.06).c(w::fg());
    let lines: [&[(&str, bool)]; 3] = [
        &[
            ("Steer", true),
            (
                ": put your left thumb down anywhere and slide it \u{b7} or \u{2190} \u{2192} buttons, or tilt the phone like a wheel",
                false,
            ),
        ],
        &[
            ("Pedals", true),
            (": one slider for your right thumb, ", false),
            ("BRAKE", true),
            (" at the bottom, ", false),
            ("GAS", true),
            (" above, ", false),
            ("N2O", true),
            (" at the top \u{b7} slide right onto ", false),
            ("DRIFT", true),
            (" for the handbrake", false),
        ],
        &[
            ("DRIFT", true),
            (
                " charges nitro \u{b7} reset, camera and pause up top",
                false,
            ),
        ],
    ];
    for line in lines {
        let runs: Vec<(String, T)> = line
            .iter()
            .map(|(s, bold)| (s.to_string(), if *bold { b } else { n }))
            .collect();
        w::rich(p, &runs, bp.k, Justify::Center);
    }
}

/// `.controls`: the keyboard help, `<kbd>` keys in boxes.
fn keyboard_help(p: &mut ChildSpawnerCommands, bp: &Bp, pads: bool) {
    let size = if bp.short { 13.0 } else { 14.0 };
    let t = T::new(size).c(w::dim());
    // Each line: keys (boxed) and words, in order.
    enum Bit {
        K(&'static str),
        W(&'static str),
    }
    use Bit::{K, W};
    let mut lines: Vec<Vec<Bit>> = vec![
        vec![K("W"), W("/"), K("\u{2191}"), W(" throttle")],
        vec![K("S"), W("/"), K("\u{2193}"), W(" brake / reverse")],
        vec![
            K("A"),
            K("D"),
            W("/"),
            K("\u{2190}"),
            K("\u{2192}"),
            W(" steer"),
        ],
        vec![K("Space"), W(" handbrake \u{2014} drift to charge nitro")],
        vec![K("Shift"), W("/"), K("N"), W(" nitro")],
        vec![
            K("C"),
            W(" camera \u{b7} "),
            K("B"),
            W(" look back \u{b7} "),
            K("R"),
            W(" reset \u{b7} "),
            K("Esc"),
            W(" pause"),
        ],
        vec![K("M"), W(" music on/off \u{b7} "), K("T"), W(" next track")],
    ];
    if pads {
        lines.push(vec![
            W("Controller: D-pad or stick to move \u{b7} "),
            K("A"),
            W(" select \u{b7} "),
            K("B"),
            W(" back \u{b7} "),
            K("Start"),
            W(" race"),
        ]);
    }
    for line in lines {
        p.spawn(Node {
            flex_wrap: FlexWrap::Wrap,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|p| {
            for bit in line {
                match bit {
                    K(k) => {
                        p.spawn((
                            Node {
                                min_width: bp.px(20.0),
                                padding: UiRect::axes(bp.px(5.0), bp.px(0.0)),
                                margin: UiRect::right(bp.px(2.0)),
                                border: w::border(bp, 1.0),
                                border_radius: w::radius(bp, 4.0),
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            BorderColor::all(w::white(0.3)),
                            BackgroundColor(w::white(0.08)),
                        ))
                        .with_children(|p| {
                            p.spawn(w::text(k, T::new(12.0).c(Color::WHITE), bp.k));
                        });
                    }
                    W(s) => {
                        for (i, word) in s.split(' ').enumerate() {
                            let word = if i > 0 {
                                format!(" {word}")
                            } else {
                                word.into()
                            };
                            if !word.is_empty() {
                                p.spawn((w::text(word, t, bp.k), TextLayout::no_wrap()));
                            }
                        }
                    }
                }
            }
        });
    }
}
