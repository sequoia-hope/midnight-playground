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

#[cfg(test)]
mod tests {
    //! The lobby's labels, and the screen built headless from a
    //! `NetView`: which controls it has for the leader and the others,
    //! the players' rows and tags, the status line, the points table.

    use super::super::screens::Cx;
    use super::super::tests::{lobby_of_two, ui_state};
    use super::super::widgets::{Bp, Focused, Icons};
    use super::super::{Act, Screen};
    use super::*;
    use mp_net::proto::{PointsRow, Settings};

    struct Built {
        /// The controls and whether each has the focus ring, in no order.
        controls: Vec<(Control, bool)>,
        /// The focus order the build recorded.
        order: Vec<String>,
        /// Every text node's whole string (its spans joined).
        texts: Vec<String>,
    }

    impl Built {
        fn get(&self, id: &str) -> Option<&Control> {
            self.controls.iter().map(|(c, _)| c).find(|c| c.id == id)
        }

        fn text(&self, id: &str) -> String {
            match self.get(id).map(|c| &c.value) {
                Some(Value::Text(t)) => t.clone(),
                other => panic!("{id}: {other:?}"),
            }
        }

        fn has(&self, id: &str) -> bool {
            self.get(id).is_some()
        }
    }

    fn build(net: &NetView, focus: Option<&str>) -> Built {
        let mut w = World::new();
        let icons = Icons {
            check: Handle::default(),
            star: Handle::default(),
            next: Handle::default(),
            note: Handle::default(),
            chevron: Handle::default(),
        };
        let ui = ui_state(Screen::Lobby);
        let mut cx = Cx {
            bp: Bp::new(1280.0, 800.0, 1.0, false, 0.0),
            icons: &icons,
            focus: focus.map(str::to_string),
            order: Vec::new(),
        };
        {
            let mut c = w.commands();
            c.spawn(Node::default()).with_children(|p| {
                lobby(p, &mut cx, &ui, net);
            });
        }
        w.flush();
        let controls = w
            .query::<(&Control, Has<Focused>)>()
            .iter(&w)
            .map(|(c, f)| (c.clone(), f))
            .collect();
        let mut texts = Vec::new();
        let mut q = w.query::<(&Text, Option<&Children>)>();
        for (t, kids) in q.iter(&w) {
            let mut s = t.0.clone();
            for k in kids.into_iter().flatten() {
                if let Some(span) = w.get::<TextSpan>(*k) {
                    s.push_str(&span.0);
                }
            }
            texts.push(s);
        }
        Built {
            controls,
            order: cx.order,
            texts,
        }
    }

    #[test]
    fn the_labels() {
        let ai = [AiFill::None, AiFill::To6, AiFill::To8].map(ai_label);
        assert_eq!(ai, ["No AI rivals", "AI fills to 6", "AI fills to 8"]);
        let grid = [GridRule::Random, GridRule::Reverse, GridRule::Same].map(grid_label);
        assert_eq!(
            grid,
            [
                "Grid: random",
                "Grid: reverse of last race",
                "Grid: order of last race"
            ]
        );
        assert_eq!(races_label(0), "Races: open-ended");
        assert_eq!(races_label(3), "Races: 3");
        assert_eq!(races_label(255), "Races: 255");
        for (kind, spec) in mp_sim::physics::CAR_SPECS {
            assert_eq!(car_label(kind), spec.label);
        }
        assert_eq!(car_label("hovercraft"), "?");
        assert_eq!(car_label(""), "?");
        let coast = mp_levels::level_by_id("coast");
        assert_eq!(level_title("coast"), coast.title);
        assert_eq!(level_title("moon"), "moon", "an unknown level by its id");
    }

    /// Every car's own colour is in the palette (a player joins in it, and
    /// the colour button steps on from it), and no colour is there twice.
    #[test]
    fn the_palette_starts_with_the_cars_colours() {
        for (kind, spec) in mp_sim::physics::CAR_SPECS {
            assert!(COLORS.contains(&spec.color), "{kind}: {:06x}", spec.color);
        }
        for (i, c) in COLORS.iter().enumerate() {
            assert!(!COLORS[i + 1..].contains(c), "{c:06x} twice");
        }
    }

    /// The leader gets this player's buttons, every race setting, Start
    /// and Leave, in that focus order, labelled from the settings.
    #[test]
    fn the_leader_sees_the_settings_and_start() {
        let b = build(&lobby_of_two(0), None);
        assert_eq!(
            b.order,
            [
                "mp-name",
                "mp-car",
                "mp-color",
                "mp-ready",
                "mp-level",
                "mp-ai",
                "mp-ghost",
                "mp-rubber",
                "mp-grid",
                "mp-races",
                "mp-go",
                "mp-leave"
            ]
        );
        assert_eq!(b.text("mp-name"), "Name: Ann");
        assert_eq!(b.text("mp-car"), format!("Car: {}", car_label("sports")));
        assert_eq!(b.text("mp-level"), level_title("coast"));
        assert_eq!(b.text("mp-ai"), "AI fills to 6");
        assert_eq!(b.text("mp-ghost"), "Contact on");
        assert_eq!(b.text("mp-rubber"), "Rubber-banding on");
        assert_eq!(b.text("mp-grid"), "Grid: reverse of last race");
        assert_eq!(b.text("mp-races"), "Races: open-ended");
        assert_eq!(b.text("mp-go"), "Start race");
        assert_eq!(b.get("mp-ready").unwrap().value, Value::Bool(false));
        assert!(b.texts.iter().any(|t| t == "READY?"));
        let act = |id: &str| b.get(id).unwrap().act.clone();
        assert_eq!(act("mp-level"), Some(Act::Mp(MpAct::Level(1))));
        assert_eq!(act("mp-car"), Some(Act::Mp(MpAct::Car(1))));
        assert_eq!(act("mp-go"), Some(Act::Mp(MpAct::Go)));
        assert_eq!(act("mp-leave"), Some(Act::Mp(MpAct::Leave)));
        assert!(!b.has("mp-settings"));

        let mut v = lobby_of_two(0);
        v.lobby.settings = Some(Settings {
            ghost: true,
            rubber_band: false,
            races: 4,
            ..Settings::default()
        });
        let b = build(&v, None);
        assert_eq!(b.text("mp-ghost"), "Ghost cars");
        assert_eq!(b.text("mp-rubber"), "Rubber-banding off");
        assert_eq!(b.text("mp-races"), "Races: 4");
    }

    /// Everyone else sees the settings as a line, and no way to change
    /// them or start.
    #[test]
    fn the_others_see_the_settings_but_cannot_change_them() {
        let b = build(&lobby_of_two(1), None);
        assert_eq!(
            b.order,
            ["mp-name", "mp-car", "mp-color", "mp-ready", "mp-leave"]
        );
        for id in [
            "mp-level",
            "mp-ai",
            "mp-ghost",
            "mp-rubber",
            "mp-grid",
            "mp-races",
            "mp-go",
        ] {
            assert!(!b.has(id), "{id}");
        }
        assert_eq!(
            b.text("mp-settings"),
            format!(
                "{} · AI fills to 6 · contact on · Races: open-ended",
                level_title("coast")
            )
        );
        assert!(
            b.texts
                .iter()
                .any(|t| t == "The first player starts the race.")
        );
        assert_eq!(b.text("mp-name"), "Name: Bob");
    }

    /// One row per player, this player's selected; the tags say who leads
    /// (the connected player with the lowest slot), who is ready and who is
    /// away.
    #[test]
    fn the_players_and_their_tags() {
        let mut v = lobby_of_two(1);
        v.lobby.players[1].ready = true;
        let b = build(&v, None);
        assert_eq!(b.get("lobby-players").unwrap().value, Value::Num(2.0));
        assert_eq!(b.text("lobby-player-0"), "Ann|sports|leads");
        assert_eq!(b.text("lobby-player-1"), "Bob|rally|ready");
        assert!(!b.get("lobby-player-0").unwrap().sel);
        assert!(b.get("lobby-player-1").unwrap().sel);
        assert_eq!(b.get("mp-ready").unwrap().value, Value::Bool(true));
        assert!(b.texts.iter().any(|t| t == "READY ✓"));

        // Ann drops: Bob leads, Ann is away.
        v.lobby.players[0].connected = false;
        v.leader = true;
        let b = build(&v, None);
        assert_eq!(b.text("lobby-player-0"), "Ann|sports|away");
        assert_eq!(b.text("lobby-player-1"), "Bob|rally|leads,ready");
        assert!(b.has("mp-go"));
    }

    /// The ready button carries this player's ready state (the test
    /// bridge's `value`), as the checkboxes carry theirs.
    #[test]
    fn the_ready_button_carries_the_state() {
        let mut v = lobby_of_two(1);
        v.lobby.players[1].ready = true;
        let b = build(&v, None);
        assert_eq!(b.get("mp-ready").unwrap().value, Value::Bool(true));
    }

    /// The status line: "Connecting…" until the host answers, then only
    /// what there is to say; a race already on is said too.
    #[test]
    fn the_status_line() {
        let mut v = NetView {
            active: true,
            ..NetView::default()
        };
        let b = build(&v, None);
        assert_eq!(b.text("lobby-status"), "Connecting…");
        assert_eq!(b.order, ["mp-leave"], "only Leave before the welcome");
        v.status = "The host said no: the lobby is full".into();
        assert_eq!(
            build(&v, None).text("lobby-status"),
            "The host said no: the lobby is full"
        );
        let mut v = lobby_of_two(1);
        assert!(!build(&v, None).has("lobby-status"));
        v.lobby.racing = true;
        let race_on = "A race is on: you'll join the next one.";
        assert!(build(&v, None).texts.iter().any(|t| t == race_on));
        v.pending_level = Some("coast".into());
        assert!(
            !build(&v, None).texts.iter().any(|t| t == race_on),
            "not when that race is this player's"
        );
    }

    /// The session's points, in order, this player's row in bold, with
    /// the places moved since the last race.
    #[test]
    fn the_points_table() {
        let mut v = lobby_of_two(1);
        assert!(!build(&v, None).has("mp-points"));
        v.lobby.raced = 2;
        v.lobby.points = vec![
            PointsRow {
                slot: Some(1),
                name: "Bob".into(),
                points: 18,
                moved: 1,
            },
            PointsRow {
                slot: None,
                name: "VIPER".into(),
                points: 16,
                moved: 0,
            },
            PointsRow {
                slot: Some(0),
                name: "Ann".into(),
                points: 14,
                moved: -2,
            },
        ];
        let b = build(&v, None);
        assert_eq!(b.get("mp-points").unwrap().value, Value::Num(3.0));
        assert_eq!(b.text("mp-points-0"), "Bob|18");
        assert_eq!(b.text("mp-points-2"), "Ann|14");
        for want in [
            "Points after 2 races",
            "1. Bob  18 pts ▲1",
            "2. VIPER  16 pts",
            "3. Ann  14 pts ▼2",
        ] {
            assert!(b.texts.iter().any(|t| t == want), "{want}: {:?}", b.texts);
        }
    }

    /// The focus ring goes on the control that has the focus, only.
    #[test]
    fn the_focus_ring_follows_the_focus() {
        let b = build(&lobby_of_two(0), Some("mp-ai"));
        let ringed: Vec<&str> = b
            .controls
            .iter()
            .filter(|(_, f)| *f)
            .map(|(c, _)| c.id.as_str())
            .collect();
        assert_eq!(ringed, ["mp-ai"]);
    }
}
