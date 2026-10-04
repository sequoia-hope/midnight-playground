//! The screens other than the menu (`#loading`, `#pause`, `#results`,
//! `#padsetup` in `index.html`), and what every `.screen` shares: the
//! full-window, scrolling column over a dark radial gradient, its gaps and
//! paddings at each breakpoint, and the logo.

use super::widgets::{self as w, Bp, Control, Icons, T, Value};
use super::{Act, Opt, Sl, UiState};
use crate::play::Play;
use crate::status::Status;
use bevy::prelude::*;
use bevy::text::{Justify, TextLayout};

/// What a screen builder has: the breakpoints, the icons, the focus and
/// the focus order it records, the logo picture.
pub struct Cx<'a> {
    pub bp: Bp,
    pub icons: &'a Icons,
    pub focus: Option<String>,
    pub order: Vec<String>,
}

impl Cx<'_> {
    /// Notes a focusable control, in order; true if it has the focus.
    pub fn f(&mut self, id: &str) -> bool {
        self.order.push(id.to_owned());
        self.focus.as_deref() == Some(id)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Loading,
    Menu,
    Other,
}

/// `.logo`'s font size: `clamp(48px, 9vw, 110px)`, `clamp(40px, 7vw,
/// 76px)` under 780 px tall, 34 px under 500.
pub fn logo_px(bp: &Bp) -> f32 {
    if bp.compact {
        34.0
    } else if bp.short {
        bp.vw(7.0).clamp(40.0, 76.0)
    } else {
        bp.vw(9.0).clamp(48.0, 110.0)
    }
}

/// A `.screen`: fills the window, scrolls, and centres its column when it
/// fits (`justify-content: safe center`), or starts at the top on short
/// screens. The menu on a phone held sideways is a two-column grid.
/// Returns the scrolling node.
pub fn screen(
    p: &mut ChildSpawnerCommands,
    cx: &mut Cx,
    kind: Kind,
    body: impl FnOnce(&mut ChildSpawnerCommands, &mut Cx),
) -> Entity {
    let bp = cx.bp;
    let gap = if bp.compact {
        8.0
    } else if bp.short {
        10.0
    } else {
        16.0
    };
    let mut pad = if bp.compact {
        (10.0, 16.0, 10.0)
    } else if bp.short {
        (18.0, 16.0, 16.0)
    } else {
        (16.0, 16.0, 16.0)
    };
    let mut top_aligned = bp.short;
    if kind == Kind::Menu && bp.touch && bp.portrait {
        // `body.touch #menu` in portrait: under the rotate hint.
        pad.0 = bp.inset_top.max(12.0) + 40.0;
        top_aligned = true;
    }
    let bg = if kind == Kind::Loading {
        w::screen_bg(40.0, w::rgb(0x1c1030), w::rgb(0x05060a), Some(70.0))
    } else {
        w::screen_bg(30.0, w::rgba(0x28143c, 0.55), w::rgba(0x03040a, 0.88), None)
    };
    let grid = kind == Kind::Menu && bp.two_columns();
    let mut inner = Node {
        width: Val::Percent(100.0),
        flex_shrink: 0.0,
        margin: UiRect::new(
            Val::Px(0.0),
            Val::Px(0.0),
            if top_aligned { Val::Px(0.0) } else { Val::Auto },
            Val::Auto,
        ),
        ..default()
    };
    if grid {
        inner.display = Display::Grid;
        inner.grid_template_columns = vec![RepeatedGridTrack::flex(2, 1.0)];
        inner.column_gap = bp.px(22.0);
        inner.row_gap = bp.px(8.0);
        inner.align_content = AlignContent::Start;
        inner.justify_items = JustifyItems::Stretch;
    } else {
        inner.flex_direction = FlexDirection::Column;
        inner.align_items = AlignItems::Center;
        inner.row_gap = bp.px(gap);
    }
    p.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::new(bp.px(pad.1), bp.px(pad.1), bp.px(pad.0), bp.px(pad.2)),
            overflow: Overflow::scroll_y(),
            ..default()
        },
        bg,
    ))
    .with_children(|p| {
        p.spawn(inner).with_children(|p| body(p, cx));
    })
    .id()
}

/// The logo node, at this breakpoint's size.
pub fn logo(p: &mut ChildSpawnerCommands, cx: &Cx, place: impl FnOnce(&mut Node)) {
    let bp = cx.bp;
    let e = w::logo(p, &bp, logo_px(&bp));
    let mut n = Node {
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        padding: UiRect::horizontal(bp.px(0.18 * logo_px(&bp))),
        flex_shrink: 0.0,
        ..default()
    };
    place(&mut n);
    p.commands_mut().entity(e).insert(n);
}

/// `.title`.
fn title(p: &mut ChildSpawnerCommands, bp: &Bp, id: &str, s: &str) {
    p.spawn((
        w::text(
            s,
            T::new(if bp.compact { 38.0 } else { 56.0 }).w(900).italic(),
            bp.k,
        ),
        TextLayout::justify(Justify::Center),
        Control::named(id, Value::Text(s.into())),
    ));
}

/// The loading line under the bar: what the client is doing.
pub fn loading_label(status: &Status) -> String {
    match status.state {
        "waiting" => "Loading the scene…".into(),
        "building" => "Building the scene…".into(),
        "warming" => "Preparing the shaders…".into(),
        "failed" => status.error.clone().unwrap_or_else(|| "Failed".into()),
        _ => "Starting…".into(),
    }
}

/// `#loading`: the logo, the bar, the line.
pub fn loading(p: &mut ChildSpawnerCommands, cx: &mut Cx, status: &Status) -> Entity {
    let bp = cx.bp;
    let frac = match status.state {
        "building" => 0.8 + 0.2 * status.progress,
        "warming" | "running" => 1.0,
        _ => 0.3,
    };
    screen(p, cx, Kind::Loading, |p, cx| {
        logo(p, cx, |_| {});
        p.spawn((
            Node {
                width: bp.px(420f32.min(bp.vw(80.0))),
                height: bp.px(6.0),
                border_radius: w::radius(&bp, 3.0),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(w::white(0.1)),
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    width: Val::Percent(100.0 * frac),
                    height: Val::Percent(100.0),
                    ..default()
                },
                w::hgrad(w::accent(), w::rgb(0xff9a3c)),
            ));
        });
        let label = loading_label(status);
        p.spawn((
            w::text(label.to_uppercase(), T::new(13.0).ls(0.2).c(w::dim()), bp.k),
            TextLayout::justify(Justify::Center),
            Control::named("load-label", Value::Text(label)),
        ));
    })
}

/// `.menu-opts` with the two volume sliders (pause).
fn volumes(p: &mut ChildSpawnerCommands, cx: &mut Cx, ui: &UiState) {
    let bp = cx.bp;
    let ot = T::new(if bp.compact { 14.0 } else { 15.0 }).c(w::dim());
    p.spawn(Node {
        flex_wrap: FlexWrap::Wrap,
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        column_gap: bp.px(if bp.compact { 14.0 } else { 18.0 }),
        row_gap: bp.px(8.0),
        ..default()
    })
    .with_children(|p| {
        for (label, id, sl, v) in [
            ("Music", "vol-music", Sl::Music, ui.settings.music),
            ("SFX", "vol-sfx", Sl::Sfx, ui.settings.sfx),
        ] {
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
        }
    });
}

/// `#pause`.
pub fn pause(p: &mut ChildSpawnerCommands, cx: &mut Cx, ui: &UiState, play: &Play) -> Entity {
    let bp = cx.bp;
    let cruise = play
        .race
        .as_ref()
        .is_some_and(|r| r.session.curr.race.cruise);
    screen(p, cx, Kind::Other, |p, cx| {
        title(p, &bp, "pause-title", "Paused");
        let b = |p: &mut ChildSpawnerCommands,
                 cx: &mut Cx,
                 id: &str,
                 act: Act,
                 label: &str,
                 primary: bool| {
            let f = cx.f(id);
            w::button(p, &bp, Control::act(id, act), label, primary, f);
        };
        b(p, cx, "btn-resume", Act::Resume, "Resume", true);
        if cruise {
            b(p, cx, "btn-end", Act::EndRun, "End run", false);
        }
        b(p, cx, "btn-restart", Act::Restart, "Restart", false);
        b(p, cx, "btn-quit", Act::Quit, "Main menu", false);
        if ui.pads {
            b(p, cx, "btn-pad-pause", Act::PadSetup, "Controller", false);
        }
        volumes(p, cx, ui);
        // `.np-line`: the now-playing text (empty until the music plays,
        // M5) and Next track.
        let f = cx.f("btn-next-track");
        p.spawn(Node {
            flex_wrap: FlexWrap::Wrap,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            column_gap: bp.px(12.0),
            row_gap: bp.px(8.0),
            ..default()
        })
        .with_children(|p| {
            w::button_mini(
                p,
                &bp,
                Control::act("btn-next-track", Act::NextTrack),
                "Next track",
                Some(cx.icons.next.clone()),
                f,
            );
        });
    })
}

/// `#results`.
pub fn results(p: &mut ChildSpawnerCommands, cx: &mut Cx, ui: &UiState) -> Entity {
    let bp = cx.bp;
    let v = ui.results.clone().unwrap_or_default();
    screen(p, cx, Kind::Other, |p, cx| {
        title(p, &bp, "res-title", &v.title);
        // `#res-table`: the place (40 px), the name with its swatch, the
        // time; at least `min(460px, 92vw)` wide, the free width shared by
        // the last two columns (a table shares it in proportion to their
        // content).
        let size = if bp.compact { 16.0 } else { 20.0 };
        let pad = if bp.compact { (3.0, 10.0) } else { (6.0, 14.0) };
        p.spawn((
            Node {
                display: Display::Grid,
                min_width: bp.px(460f32.min(bp.vw(92.0))),
                grid_template_columns: vec![
                    // `width: 40px`, padding included (`box-sizing:
                    // border-box` on everything).
                    GridTrack::px(40.0 * bp.k),
                    GridTrack::auto(),
                    GridTrack::auto(),
                ],
                ..default()
            },
            Control::named("res-table", Value::Num(v.rows.len() as f64)),
        ))
        .with_children(|p| {
            for (i, r) in v.rows.iter().enumerate() {
                let t = if r.me {
                    T::new(size).bold().c(w::accent2())
                } else {
                    T::new(size)
                };
                let cell = |right: bool| {
                    (
                        Node {
                            padding: UiRect::axes(bp.px(pad.1), bp.px(pad.0)),
                            border: UiRect::bottom(bp.px(1.0)),
                            align_items: AlignItems::Center,
                            justify_content: if right {
                                JustifyContent::End
                            } else {
                                JustifyContent::Start
                            },
                            ..default()
                        },
                        BorderColor::all(w::white(0.08)),
                    )
                };
                p.spawn((
                    cell(false),
                    Control::named(
                        format!("res-row-{i}"),
                        Value::Text(format!("{}|{}|{}", r.place, r.name, r.value)),
                    )
                    .sel(r.me),
                ))
                .with_children(|p| {
                    p.spawn((
                        w::text(
                            r.place.clone(),
                            t.c(if r.me { w::accent2() } else { w::dim() }),
                            bp.k,
                        ),
                        TextLayout::no_wrap(),
                    ));
                });
                p.spawn(cell(false)).with_children(|p| {
                    if let Some(c) = r.color {
                        p.spawn((
                            Node {
                                width: bp.px(10.0),
                                height: bp.px(10.0),
                                margin: UiRect::right(bp.px(8.0)),
                                border_radius: w::pill(),
                                flex_shrink: 0.0,
                                ..default()
                            },
                            BackgroundColor(w::rgb(c)),
                        ));
                    }
                    p.spawn((w::text(r.name.clone(), t, bp.k), TextLayout::no_wrap()));
                });
                p.spawn(cell(true)).with_children(|p| {
                    p.spawn((w::text(r.value.clone(), t, bp.k), TextLayout::no_wrap()));
                });
            }
        });
        if !v.tiles.is_empty() {
            p.spawn(Node {
                width: bp.px(460f32.min(bp.vw(92.0))),
                flex_wrap: FlexWrap::Wrap,
                justify_content: JustifyContent::Center,
                column_gap: bp.px(10.0),
                row_gap: bp.px(10.0),
                ..default()
            })
            .with_children(|p| {
                for (i, tl) in v.tiles.iter().enumerate() {
                    p.spawn((
                        Node {
                            flex_grow: 1.0,
                            flex_shrink: 1.0,
                            flex_basis: bp.px(90.0),
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            padding: UiRect::axes(bp.px(10.0), bp.px(6.0)),
                            border: w::border(&bp, 1.0),
                            border_radius: w::radius(&bp, 10.0),
                            ..default()
                        },
                        BackgroundColor(w::white(0.05)),
                        BorderColor::all(w::white(0.12)),
                        Control::named(
                            format!("res-stat-{i}"),
                            Value::Text(format!("{}|{}", tl.value, tl.label)),
                        ),
                    ))
                    .with_children(|p| {
                        let vs = if bp.compact { 20.0 } else { 26.0 };
                        if tl.stars > 0 {
                            p.spawn(Node {
                                height: bp.px(vs * w::LINE_NORMAL),
                                align_items: AlignItems::Center,
                                ..default()
                            })
                            .with_children(|p| {
                                for _ in 0..tl.stars {
                                    w::icon_node(p, &bp, &cx.icons.star, vs * 0.8, w::fg());
                                }
                            });
                        } else {
                            p.spawn(w::text(tl.value.clone(), T::new(vs).w(800).italic(), bp.k));
                        }
                        p.spawn(Node {
                            align_items: AlignItems::Center,
                            column_gap: bp.px(3.0),
                            ..default()
                        })
                        .with_children(|p| {
                            p.spawn(w::text(
                                tl.label.to_uppercase(),
                                T::new(11.0).ls(0.25).c(w::dim()),
                                bp.k,
                            ));
                            if tl.label_star {
                                w::icon_node(p, &bp, &cx.icons.star, 9.0, w::dim());
                            }
                        });
                    });
                }
            });
        }
        p.spawn((
            w::text(v.best.clone(), T::new(16.0).ls(0.1).c(w::dim()), bp.k),
            TextLayout::justify(Justify::Center),
            Control::named("res-best", Value::Text(v.best.clone())),
        ));
        let f = cx.f("btn-again");
        w::button(
            p,
            &bp,
            Control::act("btn-again", Act::Again),
            "Race again",
            true,
            f,
        );
        let f = cx.f("btn-menu");
        w::button(
            p,
            &bp,
            Control::act("btn-menu", Act::Menu),
            "Main menu",
            false,
            f,
        );
    })
}

/// The driving actions the Controller screen lists (`ACTIONS` in
/// `Gamepad.js`) and the standard layout's labels for their default
/// bindings (`DEFAULT_MAP`, `bindingLabel`).
/// `#padsetup`: what each action is bound to on the pad last touched,
/// lit while held (`.on`), the row being picked up (`.listening`), the hint
/// (`super::pad_setup`, WP 6.4).
pub fn padsetup(
    p: &mut ChildSpawnerCommands,
    cx: &mut Cx,
    ui: &UiState,
    view: &super::pad_setup::PadView,
) -> Entity {
    let bp = cx.bp;
    screen(p, cx, Kind::Other, |p, cx| {
        title(p, &bp, "pad-title", "Controller");
        p.spawn((
            w::text(&view.name, T::new(16.0).ls(0.1).c(w::dim()), bp.k),
            TextLayout::justify(Justify::Center),
            Control::named("pad-name", Value::Text(view.name.clone())),
        ));
        let col = bp.column();
        p.spawn(Node {
            width: bp.px(col),
            display: Display::Grid,
            grid_template_columns: vec![RepeatedGridTrack::flex(
                if bp.w <= 520.0 { 1 } else { 2 },
                1.0,
            )],
            column_gap: bp.px(10.0),
            row_gap: bp.px(8.0),
            ..default()
        })
        .with_children(|p| {
            for a in crate::play::gamepad::Action::ALL {
                let (id, label, bind) = (a.key(), a.label(), view.bind(a));
                let on = view.on.contains(&a);
                let listening = view.listening == Some(a);
                let cid = format!("pad-bind-{id}");
                let f = cx.f(&cid);
                let mut e = p.spawn((
                    Node {
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Baseline,
                        column_gap: bp.px(10.0),
                        padding: UiRect::axes(bp.px(14.0), bp.px(8.0)),
                        border: w::border(&bp, 1.0),
                        border_radius: w::radius(&bp, 10.0),
                        ..default()
                    },
                    // `.pad-bind.on`, `.pad-bind.listening` (its 1 px ring
                    // left out: the focus ring is the outline).
                    BackgroundColor(if on {
                        Color::srgba(58.0 / 255.0, 215.0 / 255.0, 1.0, 0.16)
                    } else {
                        w::white(0.05)
                    }),
                    BorderColor::all(if listening {
                        w::gold()
                    } else if on {
                        w::accent2()
                    } else {
                        w::white(0.12)
                    }),
                    Control::act(cid, Act::PadBind(id)).value(Value::Text(bind.into())),
                ));
                w::focus_ring(&mut e, &bp, f);
                e.with_children(|p| {
                    p.spawn(w::text(label, T::new(16.0), bp.k));
                    p.spawn((
                        w::text(
                            bind,
                            T::new(16.0)
                                .bold()
                                .c(if listening { w::gold() } else { w::accent2() }),
                            bp.k,
                        ),
                        TextLayout::justify(Justify::Right),
                    ));
                });
            }
        });
        let hint = view.hint.as_str();
        p.spawn((
            w::text(hint, T::new(15.0).c(w::dim()), bp.k),
            TextLayout::justify(Justify::Center),
            Node {
                min_height: bp.px(15.0 * 1.4),
                max_width: bp.px(col),
                ..default()
            },
            Control::named("pad-hint", Value::Text(hint.into())),
        ));
        let f = cx.f("opt-rumble");
        p.spawn(Node::default()).with_children(|p| {
            w::checkbox(
                p,
                &bp,
                Control::act("opt-rumble", Act::Toggle(Opt::Rumble)),
                "Rumble",
                ui.settings.rumble,
                T::new(15.0).c(w::dim()),
                cx.icons,
                f,
            );
        });
        p.spawn(Node {
            flex_wrap: FlexWrap::Wrap,
            justify_content: JustifyContent::Center,
            column_gap: bp.px(12.0),
            row_gap: bp.px(12.0),
            ..default()
        })
        .with_children(|p| {
            let f = cx.f("pad-defaults");
            w::button(
                p,
                &bp,
                Control::act("pad-defaults", Act::PadDefaults),
                "Defaults",
                false,
                f,
            );
            let f = cx.f("pad-done");
            w::button(
                p,
                &bp,
                Control::act("pad-done", Act::PadDone),
                "Done",
                true,
                f,
            );
        });
    })
}
