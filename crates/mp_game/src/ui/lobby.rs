//! The multiplayer lobby (roadmap WP 10.7, MULTIPLAYER.md section 4): who
//! is here, this player's name, car, colour and ready; the leader's race
//! settings and Start; the session's points table; Leave.
//!
//! Everything shown comes from `crate::net::NetView`; every button asks
//! the connection for something (`crate::net::NetCmds`) through
//! [`super::Act::Mp`].

use bevy::prelude::*;

use mp_net::proto::{AiFill, GridRule};

use super::screens::{Cx, Kind, screen, title};
use super::widgets::{self as w, Control, T, Value};
use super::{Act, UiState};
use crate::net::NetView;

/// What the lobby's buttons do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MpAct {
    /// The menu's Multiplayer button: join the host.
    Open,
    Leave,
    /// Ask for a name (the browser's prompt on the web).
    Name,
    /// The next or previous car.
    Car(i8),
    /// The next colour.
    Color,
    Ready,
    // The leader's settings.
    Level(i8),
    Ai,
    Ghost,
    RubberBand,
    Grid,
    Races,
    Go,
    /// From a finished race's results back to the lobby.
    Back,
}

/// Colours to pick from: the cars' own, then a few more.
pub const COLORS: [u32; 10] = [
    0xd81e36, 0x1f4fd8, 0xf2b705, 0xee5a12, 0xdfe7ee, 0x111111, 0x2e8b57, 0x8a2be2, 0x00a7c4,
    0xb0b0b0,
];

pub fn car_label(kind: &str) -> &'static str {
    mp_sim::physics::car_spec(kind).map_or("?", |s| s.label)
}

fn level_title(id: &str) -> String {
    mp_levels::levels()
        .into_iter()
        .find(|l| l.id == id)
        .map_or_else(|| id.to_string(), |l| l.title.to_string())
}

pub fn ai_label(a: AiFill) -> &'static str {
    match a {
        AiFill::None => "No AI rivals",
        AiFill::To6 => "AI fills to 6",
        AiFill::To8 => "AI fills to 8",
    }
}

pub fn grid_label(g: GridRule) -> &'static str {
    match g {
        GridRule::Random => "Grid: random",
        GridRule::Reverse => "Grid: reverse of last race",
        GridRule::Same => "Grid: order of last race",
    }
}

pub fn races_label(n: u8) -> String {
    if n == 0 {
        "Races: open-ended".into()
    } else {
        format!("Races: {n}")
    }
}

/// `#lobby`.
pub fn lobby(p: &mut ChildSpawnerCommands, cx: &mut Cx, _ui: &UiState, net: &NetView) -> Entity {
    let bp = cx.bp;
    screen(p, cx, Kind::Other, |p, cx| {
        title(p, &bp, "lobby-title", "Multiplayer");
        let dim = T::new(16.0).ls(0.05).c(w::dim());
        let status = if !net.connected && net.status.is_empty() {
            "Connecting…".to_string()
        } else {
            net.status.clone()
        };
        if !status.is_empty() {
            p.spawn((
                w::text(status.clone(), dim, bp.k),
                Control::named("lobby-status", Value::Text(status)),
            ));
        }
        if net.lobby.racing && net.pending_level.is_none() {
            p.spawn(w::text(
                "A race is on: you'll join the next one.",
                dim,
                bp.k,
            ));
        }

        // Who is here.
        let leader = net
            .lobby
            .players
            .iter()
            .filter(|p| p.connected)
            .map(|p| p.slot)
            .min();
        p.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: bp.px(6.0),
                min_width: bp.px(420f32.min(bp.vw(92.0))),
                ..default()
            },
            Control::named("lobby-players", Value::Num(net.lobby.players.len() as f64)),
        ))
        .with_children(|p| {
            for (i, pl) in net.lobby.players.iter().enumerate() {
                let me = Some(pl.slot) == net.slot;
                let t = if me {
                    T::new(18.0).bold().c(w::accent2())
                } else {
                    T::new(18.0)
                };
                let mut tags = Vec::new();
                if Some(pl.slot) == leader {
                    tags.push("leads");
                }
                if pl.ready {
                    tags.push("ready");
                }
                if !pl.connected {
                    tags.push("away");
                }
                p.spawn((
                    Node {
                        align_items: AlignItems::Center,
                        column_gap: bp.px(10.0),
                        ..default()
                    },
                    Control::named(
                        format!("lobby-player-{i}"),
                        Value::Text(format!("{}|{}|{}", pl.name, pl.car, tags.join(","))),
                    )
                    .sel(me),
                ))
                .with_children(|p| {
                    p.spawn((
                        Node {
                            width: bp.px(12.0),
                            height: bp.px(12.0),
                            border_radius: w::pill(),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        BackgroundColor(w::rgb(pl.color)),
                    ));
                    p.spawn((w::text(pl.name.clone(), t, bp.k), TextLayout::no_wrap()));
                    p.spawn(w::text(car_label(&pl.car), dim, bp.k));
                    if !tags.is_empty() {
                        p.spawn(w::text(tags.join(" · "), dim, bp.k));
                    }
                });
            }
        });

        // This player.
        let me = net.me();
        let row = |p: &mut ChildSpawnerCommands, f: &mut dyn FnMut(&mut ChildSpawnerCommands)| {
            p.spawn(Node {
                flex_wrap: FlexWrap::Wrap,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                column_gap: bp.px(10.0),
                row_gap: bp.px(8.0),
                ..default()
            })
            .with_children(|p| f(p));
        };
        if let Some(me) = me {
            let ready = me.ready;
            let car = me.car.clone();
            row(p, &mut |p| {
                let f = cx.f("mp-name");
                w::button(
                    p,
                    &bp,
                    Control::act("mp-name", Act::Mp(MpAct::Name)),
                    &format!("Name: {}", me.name),
                    false,
                    f,
                );
                let f = cx.f("mp-car");
                w::button(
                    p,
                    &bp,
                    Control::act("mp-car", Act::Mp(MpAct::Car(1))),
                    &format!("Car: {}", car_label(&car)),
                    false,
                    f,
                );
                let f = cx.f("mp-color");
                w::button(
                    p,
                    &bp,
                    Control::act("mp-color", Act::Mp(MpAct::Color)),
                    "Colour",
                    false,
                    f,
                );
                let f = cx.f("mp-ready");
                w::button(
                    p,
                    &bp,
                    Control::act("mp-ready", Act::Mp(MpAct::Ready)).value(Value::Bool(ready)),
                    if ready { "Ready ✓" } else { "Ready?" },
                    !net.leader,
                    f,
                );
            });
        }

        // The leader's race settings.
        if let Some(s) = &net.lobby.settings {
            if net.leader {
                row(p, &mut |p| {
                    let items: [(&str, MpAct, String); 6] = [
                        ("mp-level", MpAct::Level(1), level_title(&s.level)),
                        ("mp-ai", MpAct::Ai, ai_label(s.ai).into()),
                        (
                            "mp-ghost",
                            MpAct::Ghost,
                            if s.ghost {
                                "Ghost cars".into()
                            } else {
                                "Contact on".into()
                            },
                        ),
                        (
                            "mp-rubber",
                            MpAct::RubberBand,
                            if s.rubber_band {
                                "Rubber-banding on".into()
                            } else {
                                "Rubber-banding off".into()
                            },
                        ),
                        ("mp-grid", MpAct::Grid, grid_label(s.grid).into()),
                        ("mp-races", MpAct::Races, races_label(s.races)),
                    ];
                    for (id, act, label) in items {
                        let f = cx.f(id);
                        w::button(p, &bp, Control::act(id, Act::Mp(act)), &label, false, f);
                    }
                });
                let f = cx.f("mp-go");
                w::button(
                    p,
                    &bp,
                    Control::act("mp-go", Act::Mp(MpAct::Go)),
                    "Start race",
                    true,
                    f,
                );
            } else {
                let line = format!(
                    "{} · {} · {} · {}",
                    level_title(&s.level),
                    ai_label(s.ai),
                    if s.ghost { "ghost cars" } else { "contact on" },
                    races_label(s.races)
                );
                p.spawn((
                    w::text(line.clone(), dim, bp.k),
                    Control::named("mp-settings", Value::Text(line)),
                ));
                p.spawn(w::text("The first player starts the race.", dim, bp.k));
            }
        }

        // The session's points.
        if !net.lobby.points.is_empty() {
            p.spawn(w::text(
                format!("Points after {} races", net.lobby.raced),
                T::new(16.0).ls(0.15).c(w::dim()),
                bp.k,
            ));
            p.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: bp.px(4.0),
                    min_width: bp.px(320f32.min(bp.vw(92.0))),
                    ..default()
                },
                Control::named("mp-points", Value::Num(net.lobby.points.len() as f64)),
            ))
            .with_children(|p| {
                for (i, r) in net.lobby.points.iter().enumerate() {
                    let me = r.slot.is_some() && r.slot == net.slot;
                    let t = if me {
                        T::new(17.0).bold().c(w::accent2())
                    } else {
                        T::new(17.0)
                    };
                    let moved = match r.moved {
                        m if m > 0 => format!(" ▲{m}"),
                        m if m < 0 => format!(" ▼{}", -m),
                        _ => String::new(),
                    };
                    p.spawn((
                        w::text(
                            format!("{}. {}  {} pts{moved}", i + 1, r.name, r.points),
                            t,
                            bp.k,
                        ),
                        Control::named(
                            format!("mp-points-{i}"),
                            Value::Text(format!("{}|{}", r.name, r.points)),
                        ),
                    ));
                }
            });
        }

        let f = cx.f("mp-leave");
        w::button(
            p,
            &bp,
            Control::act("mp-leave", Act::Mp(MpAct::Leave)),
            "Leave",
            false,
            f,
        );
    })
}
