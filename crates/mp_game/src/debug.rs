//! The debug overlay (the owner's request of 2026-10-05; DECISIONS D1020) is
//! Playground Kit's `pk_shell::debug` (D1234): the frame-time rows, graphs
//! and the shell's own lines. This module says what this game is doing on
//! it: the mode and screen, the level, the car, the race's tick, speed, lap
//! and gear; and whether the touch controls are up, which moves the panel.
//!
//! `debug=1` (`--debug` natively, `?debug=1` on the web) shows it from the
//! start; F3 shows and hides it at any time (no binding of the game's has
//! F3, so the key reaches nothing else).

use crate::Opts;
use crate::status::Status;
use bevy::prelude::*;
use pk_shell::debug::{DebugLines, DebugOverlayPlugin};
use std::fmt::Write;

/// Whether the overlay is asked for at the start (`debug=1`).
pub fn wanted(o: &crate::options::Options) -> bool {
    o.param("debug").is_some_and(|v| v == "1" || v == "true")
}

pub fn plugin(app: &mut App) {
    let on = wanted(&app.world().resource::<Opts>().o);
    app.add_plugins(DebugOverlayPlugin { on, ..default() })
        .add_systems(Update, lines);
}

/// The game's lines for the overlay.
fn lines(
    mut out: ResMut<DebugLines>,
    status: Res<Status>,
    opts: Res<Opts>,
    play: Option<Res<crate::play::Play>>,
    ui: Option<Res<crate::ui::UiState>>,
) {
    let mut s = String::with_capacity(128);
    let mode = match (&ui, &play) {
        (Some(ui), Some(play)) => format!(
            "{}, screen {}",
            crate::ui::mode_name(ui, play, &status),
            ui.screen.name()
        ),
        _ => status.state.name().to_string(),
    };
    let _ = write!(s, "{mode}  level {}", opts.o.level);
    if let Some(play) = &play {
        let _ = write!(s, "  car {}", play.params.car);
        if let Some(r) = &play.race {
            let c = &r.session.curr;
            let p = &c.players[0];
            let _ = write!(
                s,
                "\ntick {} ({:.1} s, {:?})  {:.0} km/h  lap {}/{}  gear {}",
                c.tick,
                c.race.time,
                c.race.state,
                p.v.speed * 3.6,
                p.rules.lap,
                c.race.laps,
                p.phys.gear
            );
        }
    }
    let touch = play.as_ref().is_some_and(|p| p.touch_ui);
    if out.head != s || out.touch_layout != touch {
        out.head = s;
        out.touch_layout = touch;
    }
}
