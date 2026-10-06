//! The six levels as data, and Seaside Raceway's survey data loader (port of
//! `src/levels/`, SPEC 4.4).
//!
//! Each level module returns a [`Level`]; [`levels`] lists them in menu order
//! and [`level_by_id`] finds one as `levelById` does. Seaside Raceway needs
//! its survey data ([`seaside::prepare`]) before its Track can be built.

#![forbid(unsafe_code)]
// Index loops stay index loops: they mirror the JS line for line (DECISIONS D52).
#![allow(clippy::needless_range_loop)]

use mp_track::{Level, Police, Zone};

/// A sky keyframe from the JS literal's values, in its field order: s,
/// sunEl, zen, hor, sun, sunI, hemiS, hemiG, hemiI, fog, fogD, exp, night.
macro_rules! sky {
    ($s:expr, $sun_el:expr, $zen:expr, $hor:expr, $sun:expr, $sun_i:expr, $hemi_s:expr, $hemi_g:expr,
     $hemi_i:expr, $fog:expr, $fog_d:expr, $exp:expr, $night:expr) => {
        mp_track::SkyKey {
            s: $s as f64,
            sun_el: $sun_el as f64,
            zen: $zen,
            hor: $hor,
            sun: $sun,
            sun_i: $sun_i as f64,
            hemi_s: $hemi_s,
            hemi_g: $hemi_g,
            hemi_i: $hemi_i as f64,
            fog: $fog,
            fog_d: $fog_d as f64,
            exp: $exp as f64,
            night: $night as f64,
        }
    };
}

/// `{ name, kind, color, skill, power }`.
macro_rules! rival {
    ($name:expr, $kind:expr, $color:expr, $skill:expr, $power:expr) => {
        mp_track::RivalDef {
            name: $name,
            kind: $kind,
            color: $color,
            skill: $skill as f64,
            power: $power as f64,
        }
    };
}

/// `{ gap, mix, oncoming, speed[, opposite] }`.
macro_rules! traffic {
    ([$g0:expr, $g1:expr], [$(($k:expr, $w:expr)),* $(,)?], $oncoming:expr, [$v0:expr, $v1:expr] $(, $opp:expr)?) => {
        mp_track::TrafficRule {
            gap: [$g0 as f64, $g1 as f64],
            mix: vec![$(($k, $w as f64)),*],
            oncoming: $oncoming as f64,
            speed: [$v0 as f64, $v1 as f64],
            opposite: None $(.or(Some($opp as f64)))?,
        }
    };
}

pub(crate) fn zone(
    key: &'static str,
    name: &'static str,
    sub: &'static str,
    landform: &'static str,
    scenery: &'static str,
    color: &'static str,
) -> Zone {
    Zone {
        key,
        name,
        sub,
        landform,
        scenery,
        color,
        blend: None,
        blend_offset: None,
    }
}

pub(crate) fn police(heat_cap: &[u32], los_open_ground: &[bool]) -> Police {
    Police {
        heat_cap: heat_cap.to_vec(),
        los_open_ground: los_open_ground.to_vec(),
    }
}

pub mod coast;
pub mod cruise;
pub mod desert;
pub mod seaside;
pub mod sierra;
pub mod streets;
pub mod survey;
pub mod world;

pub use survey::SeasideData;

/// Everything the menu offers, in order. Coast Highway comes first, which
/// starts in the dark and drives into sunrise, then Sierra (the owner,
/// 2026-10-06; D1102); the JS lists Sierra first.
pub fn levels() -> Vec<Level> {
    vec![
        coast::level(),
        sierra::level(),
        streets::level(),
        desert::level(),
        seaside::level(),
        cruise::level(),
    ]
}

/// The level with this id, or the first (`levelById`).
pub fn level_by_id(id: &str) -> Level {
    let mut all = levels();
    let k = all.iter().position(|l| l.id == id).unwrap_or(0);
    all.swap_remove(k)
}
