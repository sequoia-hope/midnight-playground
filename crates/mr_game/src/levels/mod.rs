//! The client's world build of the levels that need more than Level 1's
//! setup (roadmap WP 7.4 and 7.5; DECISIONS D680 to D699): what `animate`
//! asks of a level before it builds it, and the build itself.
//!
//! - Seaside Raceway ([`seaside`]): its survey (the Track's, from
//!   `make_track`), the ground photo (decoded by the page on the web, from
//!   the file natively), Raceway as its scenery, and the start lights the
//!   race's countdown drives (`world.onCountdown`).
//! - The Night City Cruise: Level 1's setup and City, so `animate`'s own
//!   build serves it; nothing here.
//! - Desert Run ([`desert`]) and Downtown Streets ([`streets`]): their
//!   scenery modules for `animate`'s build, so the wasm links only the
//!   levels the client builds, and what each needs of the client beyond
//!   the shared animator path (D720 to D723).
//! - The Coast Highway ([`coast`]): its scenery modules (D700 to D703).
//!
//! The race's countdown reaches the world through [`RaceCountdown`], which
//! `animate::run_animators` hands to `WorldBuild::countdown` after
//! `WorldBuild::update`, as `Race.update` calls `world.onCountdown`.

pub mod coast;
pub mod desert;
pub mod seaside;
pub mod streets;

use bevy::prelude::*;
use mr_sim::race::RaceStateKind;
use mr_worldgen::world::Build;

/// Whether the level's own inputs are in, so `animate` may start its build
/// (`draws`: the build is the drawn scene, `?world=gen`). A level with no
/// inputs of its own is always ready. A failed input is ready too: the
/// build then fails and says why.
pub fn inputs_ready(level: &str, draws: bool) -> bool {
    match level {
        "seaside" => seaside::inputs_ready(draws),
        _ => true,
    }
}

/// The level's world jobs, where this module makes them (else `animate`'s
/// Level 1 setup).
pub fn new_build(level: &str, draws: bool) -> Option<Build> {
    match level {
        "seaside" => Some(seaside::new_build(draws)),
        _ => None,
    }
}

/// `world.onCountdown(started ? -1 : this.countdown)`, which `Race.update`
/// calls every frame of a race: the countdown's seconds left while it runs,
/// -1 once racing; `None` with no race (the menu, the fly camera), when
/// the JS does not call it.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct RaceCountdown(pub Option<f64>);

fn race_countdown(play: Option<Res<crate::play::Play>>, mut cd: ResMut<RaceCountdown>) {
    let v = play.as_ref().and_then(|p| p.race.as_ref()).map(|r| {
        let st = &r.session.curr.race;
        if st.state == RaceStateKind::Countdown {
            st.countdown
        } else {
            -1.0
        }
    });
    if cd.0 != v {
        cd.0 = v;
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<RaceCountdown>().add_systems(
        PostUpdate,
        race_countdown.before(crate::animate::run_animators),
    );
}
