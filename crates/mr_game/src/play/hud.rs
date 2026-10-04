//! The race's plain HUD and screens (Bevy UI, the bundled font): the
//! centre text and toasts (`hud.center`, `hud.toast`), position, clock and
//! lap, speed and gear, the pause card and the results list. The styled
//! HUD and menus of `HUD.js` and `hud.css` are roadmap M6.

use super::flow::{Mode, fmt_time, ordinal};
use super::{Play, touch::Layout};
use bevy::prelude::*;
use bevy::text::{FontWeight, Justify, TextLayout};
use bevy::ui::widget::TextShadow;
use mr_sim::race::{RaceStateKind, standings};

#[derive(Component)]
pub(super) enum HudText {
    Center,
    Toast,
    TopLeft,
    Speed,
    Help,
    PanelTitle,
    PanelBody,
    PanelHint,
}

#[derive(Component)]
pub(super) struct Panel;

fn text(size: f32, color: Color) -> (TextFont, TextColor, TextShadow) {
    (
        TextFont::from_font_size(size).with_font_weight(FontWeight::BOLD),
        TextColor(color),
        TextShadow {
            offset: Vec2::new(1.5, 2.0),
            color: Color::srgba(0.0, 0.0, 0.0, 0.75),
        },
    )
}

fn full_width(top: Val) -> Node {
    Node {
        position_type: PositionType::Absolute,
        top,
        left: Val::Px(0.0),
        right: Val::Px(0.0),
        justify_content: JustifyContent::Center,
        ..default()
    }
}

pub(super) fn spawn(mut commands: Commands, play: Option<Res<Play>>) {
    if play.is_none() {
        return;
    }
    let fg = Color::srgb(0.91, 0.93, 0.96);
    let center = Text::default();
    commands.spawn((
        full_width(Val::Percent(24.0)),
        children![(
            center,
            text(84.0, Color::WHITE),
            TextLayout::justify(Justify::Center),
            HudText::Center
        )],
    ));
    commands.spawn((
        full_width(Val::Percent(42.0)),
        children![(
            Text::default(),
            text(26.0, Color::srgb(1.0, 0.82, 0.35)),
            TextLayout::justify(Justify::Center),
            HudText::Toast
        )],
    ));
    commands.spawn((
        Text::default(),
        text(22.0, fg),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            left: Val::Px(16.0),
            ..default()
        },
        HudText::TopLeft,
    ));
    commands.spawn((
        Text::default(),
        text(30.0, fg),
        TextLayout::justify(Justify::Right),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(16.0),
            right: Val::Px(20.0),
            ..default()
        },
        HudText::Speed,
    ));
    commands
        .spawn((
            full_width(Val::Auto),
            children![(
                Text::default(),
                text(15.0, Color::srgba(0.85, 0.88, 0.95, 0.85)),
                TextLayout::justify(Justify::Center),
                HudText::Help
            )],
        ))
        .insert(Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(14.0),
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            justify_content: JustifyContent::Center,
            ..default()
        });
    // The pause card and the results list.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        Visibility::Hidden,
        Panel,
        children![(
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(14.0),
                padding: UiRect::axes(Val::Px(36.0), Val::Px(24.0)),
                border_radius: BorderRadius::all(Val::Px(18.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.025, 0.05, 0.82)),
            children![
                (
                    Text::default(),
                    text(44.0, Color::WHITE),
                    TextLayout::justify(Justify::Center),
                    HudText::PanelTitle
                ),
                (
                    Text::default(),
                    text(22.0, fg),
                    TextLayout::justify(Justify::Left),
                    HudText::PanelBody
                ),
                (
                    Text::default(),
                    text(17.0, Color::srgb(1.0, 0.24, 0.43)),
                    TextLayout::justify(Justify::Center),
                    HudText::PanelHint
                ),
            ],
        )],
    ));
}

/// Writes a text only when it changed.
fn set(t: &mut Text, s: String) {
    if t.0 != s {
        t.0 = s;
    }
}

pub(super) fn update(
    play: Res<Play>,
    mut texts: Query<(&HudText, &mut Text, &mut TextColor, Option<&mut Node>)>,
    mut panel: Query<&mut Visibility, With<Panel>>,
) {
    let Some(race) = &play.race else { return };
    let st = &race.session.curr;
    let p = &st.players[0];
    let lay: &Layout = &race.touch.layout;
    let touch = play.touch_ui;
    let scale = play.css_scale.max(0.01);
    let mode = race.mode;
    let speed = vel(p.v.vx, p.v.vz);
    for (which, mut t, mut color, node) in &mut texts {
        match which {
            HudText::Center => {
                let c = race.hud.center.clone().unwrap_or_default();
                color.0 = match c.as_str() {
                    "GO!" | "WINNER!" => Color::srgb(0.3, 1.0, 0.54),
                    "WRONG WAY" => Color::srgb(1.0, 0.24, 0.43),
                    _ => Color::WHITE,
                };
                set(&mut t, if mode == Mode::Race { c } else { String::new() });
            }
            HudText::Toast => {
                let s = race.hud.toast.clone().unwrap_or_default();
                set(&mut t, if mode == Mode::Race { s } else { String::new() });
            }
            HudText::TopLeft => {
                let list = standings(st);
                let pos = list.iter().position(|r| r.player).map_or(0, |i| i + 1);
                let time = if p.rules.finished {
                    p.rules.finish_time
                } else {
                    Some(st.race.time)
                };
                let mut s = if st.race.cruise {
                    format!(
                        "SCORE {}\n{}",
                        mr_math::js::round(p.rules.score),
                        fmt_time(time)
                    )
                } else {
                    format!("POS {pos}/{}\n{}", list.len(), fmt_time(time))
                };
                if st.race.laps > 0 {
                    s.push_str(&format!(
                        "\nLAP {}/{}",
                        p.rules.lap.min(st.race.laps as i32),
                        st.race.laps
                    ));
                }
                set(
                    &mut t,
                    if mode == Mode::Results {
                        String::new()
                    } else {
                        s
                    },
                );
                if let Some(mut n) = node {
                    let left = if touch { lay.taps[0].left } else { 16.0 };
                    n.left = Val::Px((left as f32) / scale);
                    n.top = Val::Px(if touch { 8.0 } else { 12.0 } / scale);
                }
            }
            HudText::Speed => {
                let mph = speed * 2.23694;
                let gear = match p.phys.gear {
                    -1 => "R".to_string(),
                    0 => "N".to_string(),
                    _ if p.phys.electric => "D".to_string(),
                    g => g.to_string(),
                };
                let nitro = (p.phys.nitro * 10.0).round() as usize;
                let bar = format!("{}{}", "|".repeat(nitro), ".".repeat(10 - nitro.min(10)));
                set(
                    &mut t,
                    if mode == Mode::Results {
                        String::new()
                    } else {
                        format!("{:.0} MPH  {gear}\nN2O {bar}", mph.round())
                    },
                );
                if let Some(mut n) = node {
                    if touch {
                        // Above the steering, off to the side (hud.css,
                        // body.touch .hud-br).
                        // --steer-top + 8: the stick's resting centre is
                        // .6 b above --inB, its top 32 px over that.
                        let bottom = (lay.h - lay.stick_home.1) + 40.0;
                        n.left = Val::Px((lay.taps[0].left as f32) / scale);
                        n.right = Val::Auto;
                        n.bottom = Val::Px((bottom as f32) / scale);
                    } else {
                        n.left = Val::Auto;
                        n.right = Val::Px(20.0);
                        n.bottom = Val::Px(16.0);
                    }
                }
            }
            HudText::Help => {
                let show = st.race.state == RaceStateKind::Countdown && mode == Mode::Race;
                let s = if !show {
                    String::new()
                } else if touch {
                    "Left thumb: steer   Right thumb: slide up for gas, down to brake, right to drift"
                        .to_string()
                } else {
                    "W gas   S brake   A D steer   Space drift   Shift nitro   C camera   B look back   R reset   Esc pause"
                        .to_string()
                };
                set(&mut t, s);
            }
            HudText::PanelTitle => set(
                &mut t,
                match mode {
                    Mode::Paused => "PAUSED".into(),
                    Mode::Results => results_title(race),
                    Mode::Race => String::new(),
                },
            ),
            HudText::PanelBody => set(
                &mut t,
                match mode {
                    Mode::Results => results_body(race),
                    _ => String::new(),
                },
            ),
            HudText::PanelHint => set(
                &mut t,
                match (mode, touch) {
                    (Mode::Paused, false) => "Esc to resume".into(),
                    (Mode::Paused, true) => "Tap to resume".into(),
                    (Mode::Results, false) => "Enter: race again".into(),
                    (Mode::Results, true) => "Tap: race again".into(),
                    _ => String::new(),
                },
            ),
        }
    }
    if let Ok(mut v) = panel.single_mut() {
        let want = if mode == Mode::Race {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *v != want {
            *v = want;
        }
    }
}

fn vel(vx: f64, vz: f64) -> f64 {
    mr_math::kernel::hypot(vx, vz)
}

/// `showResults`' title: "You win!" or "3rd place"; a cruise's "Run over".
fn results_title(race: &super::flow::Race) -> String {
    if race.session.curr.race.cruise {
        return "Run over".into();
    }
    let Some(r) = &race.results else {
        return String::new();
    };
    match r.iter().find(|r| r.player) {
        Some(me) if me.place == 1 => "You win!".into(),
        Some(me) => format!("{} place", ordinal(me.place)),
        None => String::new(),
    }
}

/// The results table: place, name, time (`~` for an estimate), and on a
/// circuit each lap's time.
fn results_body(race: &super::flow::Race) -> String {
    let st = &race.session.curr;
    if st.race.cruise {
        let r = mr_sim::race::cruise_results(st);
        return format!(
            "Score        {}\nDistance     {:.2} mi\nTop speed    {} mph\nNear misses  {}\nTime         {}",
            r.score,
            r.dist / 1609.34,
            mr_math::js::round(r.top * 2.23694),
            r.near_misses,
            fmt_time(Some(r.time))
        );
    }
    let Some(rows) = &race.results else {
        return String::new();
    };
    let mut out: Vec<String> = rows
        .iter()
        .map(|r| {
            format!(
                "{} {:>2}  {:<9} {}{}",
                if r.player { ">" } else { " " },
                r.place,
                r.name,
                if r.estimated { "~" } else { " " },
                fmt_time(Some(r.time))
            )
        })
        .collect();
    let laps = &st.players[0].rules.lap_times;
    if st.race.laps > 0 && !laps.is_empty() {
        let best = laps.iter().cloned().fold(f64::INFINITY, f64::min);
        out.push(String::new());
        for (i, t) in laps.iter().enumerate() {
            out.push(format!(
                "   Lap {}{}  {}",
                i + 1,
                if *t == best { "*" } else { " " },
                fmt_time(Some(*t))
            ));
        }
    }
    out.join("\n")
}
