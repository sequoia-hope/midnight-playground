//! Shared by the level tests: every level, Seaside with its survey data.

#![allow(dead_code)]

use std::sync::{Arc, OnceLock};

use mp_levels::{SeasideData, levels, seaside};
use mp_track::{Level, Track};

pub const SURVEY: &[u8] = include_bytes!("../../../../assets/seaside/survey.bin");

pub fn survey() -> Arc<SeasideData> {
    static DATA: OnceLock<Arc<SeasideData>> = OnceLock::new();
    DATA.get_or_init(|| Arc::new(SeasideData::parse(SURVEY).expect("survey.bin parses")))
        .clone()
}

/// The menu's levels, Seaside prepared (test/unit/support/levels.js).
pub fn all() -> Vec<Level> {
    let mut all = levels();
    for l in &mut all {
        if l.id == "seaside" {
            seaside::prepare(l, survey());
        }
    }
    all
}

pub fn level(id: &str) -> Level {
    all().into_iter().find(|l| l.id == id).unwrap()
}

pub fn tracks() -> &'static Vec<(Level, Track)> {
    static T: OnceLock<Vec<(Level, Track)>> = OnceLock::new();
    T.get_or_init(|| {
        all()
            .into_iter()
            .map(|l| {
                let t = Track::new(&l).unwrap();
                (l, t)
            })
            .collect()
    })
}
