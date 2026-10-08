//! The lab's kits (`instruments.js` `KITS`) and the game's sample tweaks
//! (`SAMPLE_TWEAK`), generated from the evaluated JS objects by
//! `tools/music-lab/test/gen-rust-tables.py` over `dump-tables.mjs`, not
//! transcribed. The table's canonical JSON hashes to the lab's
//! (`tests/kits.rs`). Regenerate whenever `KITS` or `SAMPLE_TWEAK` change.

use crate::instruments::{HitType, KitVoice};

/// The SHA-256 of the lab's `canon(KITS)`.
pub const KITS_SHA256: &str = "1df980cd5208e4c39a01661aa6a5dbfd3fb71d620afda591aeeff881462d346b";

/// `KITS[kit]`: the lanes of a kit, in the lab's order.
pub fn kit(name: &str) -> Option<&'static [(&'static str, KitVoice)]> {
    KITS.iter().find(|(n, _)| *n == name).map(|(_, k)| *k)
}

/// `SAMPLE_TWEAK[sample]`: the tweak's keys and factors, in the lab's order.
pub fn sample_tweak(name: &str) -> Option<&'static [(&'static str, f64)]> {
    SAMPLE_TWEAK
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, t)| *t)
}

/// Every kit, in the lab's order: per lane name, a voice type and its
/// parameters.
pub static KITS: &[(&str, &[(&str, KitVoice)])] = &[
    (
        "tr808",
        &[
            (
                "kick",
                KitVoice {
                    t: HitType::Kick808,
                    tune: Some(49.0),
                    pitch: Some(1.7),
                    pt: Some(0.014),
                    decay: Some(0.75),
                    click: Some(0.25),
                    click_hz: Some(2500.0),
                    drive: Some(1.3),
                    lvl: Some(1.0),
                    ..KitVoice::NONE
                },
            ),
            (
                "snare",
                KitVoice {
                    t: HitType::Snare808,
                    tune: Some(185.0),
                    tone: Some(0.45),
                    snappy: Some(0.13),
                    decay: Some(0.11),
                    nhp: Some(1800.0),
                    nlp: Some(9000.0),
                    lvl: Some(0.8),
                    ..KitVoice::NONE
                },
            ),
            (
                "clap",
                KitVoice {
                    t: HitType::Clap,
                    bp: Some(1050.0),
                    tail: Some(0.24),
                    bursts: Some(3.0),
                    spacing: Some(0.012),
                    lvl: Some(0.75),
                    ..KitVoice::NONE
                },
            ),
            (
                "hat",
                KitVoice {
                    t: HitType::Metal,
                    tune: Some(1.0),
                    decay: Some(0.045),
                    bp: Some(10500.0),
                    hp: Some(7500.0),
                    lvl: Some(0.45),
                    ..KitVoice::NONE
                },
            ),
            (
                "ohat",
                KitVoice {
                    t: HitType::Metal,
                    tune: Some(1.0),
                    decay: Some(0.38),
                    bp: Some(10000.0),
                    hp: Some(7000.0),
                    lvl: Some(0.35),
                    ..KitVoice::NONE
                },
            ),
            (
                "ride",
                KitVoice {
                    t: HitType::Metal,
                    tune: Some(1.3),
                    decay: Some(0.9),
                    bp: Some(8000.0),
                    hp: Some(5200.0),
                    noise: Some(0.15),
                    lvl: Some(0.28),
                    ..KitVoice::NONE
                },
            ),
            (
                "crash",
                KitVoice {
                    t: HitType::Cymbal,
                    tune: Some(1.1),
                    decay: Some(1.7),
                    bp: Some(7500.0),
                    hp: Some(4200.0),
                    bp2: Some(3500.0),
                    decay2: Some(0.25),
                    noise: Some(0.35),
                    lvl: Some(0.35),
                    ..KitVoice::NONE
                },
            ),
            (
                "revCrash",
                KitVoice {
                    t: HitType::Swell,
                    lvl: Some(0.32),
                    ..KitVoice::NONE
                },
            ),
            (
                "rim",
                KitVoice {
                    t: HitType::Rim,
                    f1: Some(455.0),
                    f2: Some(1667.0),
                    decay: Some(0.03),
                    lvl: Some(0.5),
                    ..KitVoice::NONE
                },
            ),
            (
                "cowbell",
                KitVoice {
                    t: HitType::Cowbell,
                    bp: Some(800.0),
                    q: Some(2.5),
                    decay: Some(0.28),
                    lvl: Some(0.35),
                    ..KitVoice::NONE
                },
            ),
            (
                "tomL",
                KitVoice {
                    t: HitType::Tom,
                    tune: Some(82.0),
                    pitch: Some(1.25),
                    pt: Some(0.05),
                    decay: Some(0.45),
                    noise: Some(0.03),
                    lvl: Some(0.7),
                    pan: Some(-0.3),
                    ..KitVoice::NONE
                },
            ),
            (
                "tomM",
                KitVoice {
                    t: HitType::Tom,
                    tune: Some(118.0),
                    pitch: Some(1.25),
                    pt: Some(0.05),
                    decay: Some(0.38),
                    noise: Some(0.03),
                    lvl: Some(0.65),
                    ..KitVoice::NONE
                },
            ),
            (
                "tomH",
                KitVoice {
                    t: HitType::Tom,
                    tune: Some(165.0),
                    pitch: Some(1.25),
                    pt: Some(0.05),
                    decay: Some(0.32),
                    noise: Some(0.03),
                    lvl: Some(0.6),
                    pan: Some(0.3),
                    ..KitVoice::NONE
                },
            ),
            (
                "shaker",
                KitVoice {
                    t: HitType::Shaker,
                    bp: Some(6500.0),
                    decay: Some(0.06),
                    attack: Some(0.006),
                    lvl: Some(0.35),
                    ..KitVoice::NONE
                },
            ),
            (
                "snap",
                KitVoice {
                    t: HitType::Snap,
                    bp: Some(2200.0),
                    q: Some(0.8),
                    decay: Some(0.05),
                    attack: Some(0.0005),
                    lvl: Some(0.55),
                    ..KitVoice::NONE
                },
            ),
            (
                "boom",
                KitVoice {
                    t: HitType::Boom,
                    tune: Some(38.0),
                    pitch: Some(2.2),
                    pt: Some(0.05),
                    decay: Some(1.6),
                    drive: Some(1.6),
                    lvl: Some(0.8),
                    ..KitVoice::NONE
                },
            ),
        ],
    ),
    (
        "tr909",
        &[
            (
                "kick",
                KitVoice {
                    t: HitType::Kick909,
                    tune: Some(54.0),
                    pitch: Some(3.6),
                    pt: Some(0.009),
                    decay: Some(0.42),
                    click: Some(0.7),
                    click_hz: Some(4500.0),
                    drive: Some(1.6),
                    lvl: Some(1.0),
                    ..KitVoice::NONE
                },
            ),
            (
                "snare",
                KitVoice {
                    t: HitType::Snare909,
                    tune: Some(195.0),
                    tone: Some(0.55),
                    snappy: Some(0.17),
                    decay: Some(0.09),
                    nhp: Some(2200.0),
                    nlp: Some(11000.0),
                    lvl: Some(0.8),
                    ..KitVoice::NONE
                },
            ),
            (
                "clap",
                KitVoice {
                    t: HitType::Clap,
                    bp: Some(1250.0),
                    tail: Some(0.18),
                    bursts: Some(4.0),
                    spacing: Some(0.009),
                    lvl: Some(0.75),
                    ..KitVoice::NONE
                },
            ),
            (
                "hat",
                KitVoice {
                    t: HitType::Metal,
                    tune: Some(1.45),
                    decay: Some(0.05),
                    bp: Some(12000.0),
                    hp: Some(8500.0),
                    noise: Some(0.35),
                    lvl: Some(0.45),
                    ..KitVoice::NONE
                },
            ),
            (
                "ohat",
                KitVoice {
                    t: HitType::Metal,
                    tune: Some(1.45),
                    decay: Some(0.3),
                    bp: Some(11000.0),
                    hp: Some(8000.0),
                    noise: Some(0.35),
                    lvl: Some(0.35),
                    ..KitVoice::NONE
                },
            ),
            (
                "ride",
                KitVoice {
                    t: HitType::Metal,
                    tune: Some(1.9),
                    decay: Some(1.1),
                    bp: Some(9000.0),
                    hp: Some(5500.0),
                    noise: Some(0.25),
                    lvl: Some(0.27),
                    ..KitVoice::NONE
                },
            ),
            (
                "crash",
                KitVoice {
                    t: HitType::Cymbal,
                    tune: Some(1.25),
                    decay: Some(1.5),
                    bp: Some(8500.0),
                    hp: Some(4800.0),
                    bp2: Some(4200.0),
                    decay2: Some(0.2),
                    noise: Some(0.45),
                    lvl: Some(0.35),
                    ..KitVoice::NONE
                },
            ),
            (
                "revCrash",
                KitVoice {
                    t: HitType::Swell,
                    lvl: Some(0.32),
                    ..KitVoice::NONE
                },
            ),
            (
                "rim",
                KitVoice {
                    t: HitType::Rim,
                    f1: Some(500.0),
                    f2: Some(1900.0),
                    decay: Some(0.025),
                    lvl: Some(0.5),
                    ..KitVoice::NONE
                },
            ),
            (
                "cowbell",
                KitVoice {
                    t: HitType::Cowbell,
                    bp: Some(800.0),
                    q: Some(2.5),
                    decay: Some(0.22),
                    lvl: Some(0.35),
                    ..KitVoice::NONE
                },
            ),
            (
                "tomL",
                KitVoice {
                    t: HitType::Tom,
                    tune: Some(90.0),
                    pitch: Some(1.5),
                    pt: Some(0.03),
                    decay: Some(0.35),
                    noise: Some(0.08),
                    lvl: Some(0.7),
                    pan: Some(-0.3),
                    ..KitVoice::NONE
                },
            ),
            (
                "tomM",
                KitVoice {
                    t: HitType::Tom,
                    tune: Some(130.0),
                    pitch: Some(1.5),
                    pt: Some(0.03),
                    decay: Some(0.3),
                    noise: Some(0.08),
                    lvl: Some(0.65),
                    ..KitVoice::NONE
                },
            ),
            (
                "tomH",
                KitVoice {
                    t: HitType::Tom,
                    tune: Some(180.0),
                    pitch: Some(1.5),
                    pt: Some(0.03),
                    decay: Some(0.26),
                    noise: Some(0.08),
                    lvl: Some(0.6),
                    pan: Some(0.3),
                    ..KitVoice::NONE
                },
            ),
            (
                "shaker",
                KitVoice {
                    t: HitType::Shaker,
                    bp: Some(7500.0),
                    decay: Some(0.05),
                    attack: Some(0.005),
                    lvl: Some(0.35),
                    ..KitVoice::NONE
                },
            ),
            (
                "snap",
                KitVoice {
                    t: HitType::Snap,
                    bp: Some(2600.0),
                    q: Some(0.8),
                    decay: Some(0.045),
                    attack: Some(0.0005),
                    lvl: Some(0.55),
                    ..KitVoice::NONE
                },
            ),
            (
                "boom",
                KitVoice {
                    t: HitType::Boom,
                    tune: Some(40.0),
                    pitch: Some(2.5),
                    pt: Some(0.05),
                    decay: Some(1.5),
                    drive: Some(1.8),
                    lvl: Some(0.8),
                    ..KitVoice::NONE
                },
            ),
        ],
    ),
    (
        "latin",
        &[
            (
                "kick",
                KitVoice {
                    t: HitType::Kick808,
                    tune: Some(52.0),
                    pitch: Some(1.5),
                    pt: Some(0.012),
                    decay: Some(0.3),
                    click: Some(0.1),
                    click_hz: Some(2000.0),
                    drive: Some(1.1),
                    lvl: Some(0.7),
                    ..KitVoice::NONE
                },
            ),
            (
                "congaO",
                KitVoice {
                    t: HitType::Conga,
                    tune: Some(190.0),
                    decay: Some(0.4),
                    lvl: Some(0.7),
                    pan: Some(-0.3),
                    ..KitVoice::NONE
                },
            ),
            (
                "congaS",
                KitVoice {
                    t: HitType::Conga,
                    tune: Some(200.0),
                    decay: Some(0.12),
                    slap: Some(1.0),
                    lvl: Some(0.85),
                    pan: Some(-0.3),
                    ..KitVoice::NONE
                },
            ),
            (
                "tumba",
                KitVoice {
                    t: HitType::Conga,
                    tune: Some(140.0),
                    decay: Some(0.55),
                    lvl: Some(0.75),
                    pan: Some(-0.2),
                    ..KitVoice::NONE
                },
            ),
            (
                "bongoH",
                KitVoice {
                    t: HitType::Conga,
                    tune: Some(420.0),
                    decay: Some(0.09),
                    lvl: Some(0.45),
                    pan: Some(0.35),
                    ..KitVoice::NONE
                },
            ),
            (
                "bongoL",
                KitVoice {
                    t: HitType::Conga,
                    tune: Some(290.0),
                    decay: Some(0.12),
                    lvl: Some(0.5),
                    pan: Some(0.3),
                    ..KitVoice::NONE
                },
            ),
            (
                "timbaleH",
                KitVoice {
                    t: HitType::Tom,
                    tune: Some(240.0),
                    pitch: Some(1.3),
                    pt: Some(0.02),
                    decay: Some(0.22),
                    noise: Some(0.3),
                    lvl: Some(0.6),
                    pan: Some(0.25),
                    ..KitVoice::NONE
                },
            ),
            (
                "timbaleL",
                KitVoice {
                    t: HitType::Tom,
                    tune: Some(170.0),
                    pitch: Some(1.3),
                    pt: Some(0.02),
                    decay: Some(0.3),
                    noise: Some(0.25),
                    lvl: Some(0.65),
                    pan: Some(0.2),
                    ..KitVoice::NONE
                },
            ),
            (
                "cascara",
                KitVoice {
                    t: HitType::Rim,
                    f1: Some(1200.0),
                    f2: Some(3100.0),
                    decay: Some(0.022),
                    lvl: Some(0.4),
                    pan: Some(0.25),
                    ..KitVoice::NONE
                },
            ),
            (
                "cowbell",
                KitVoice {
                    t: HitType::Cowbell,
                    bp: Some(850.0),
                    q: Some(2.2),
                    decay: Some(0.3),
                    lvl: Some(0.14),
                    pan: Some(0.15),
                    ..KitVoice::NONE
                },
            ),
            (
                "guiroL",
                KitVoice {
                    t: HitType::Guiro,
                    len: Some(0.17),
                    rate0: Some(55.0),
                    rate1: Some(150.0),
                    bp: Some(2800.0),
                    q: Some(1.4),
                    lvl: Some(0.5),
                    pan: Some(-0.2),
                    ..KitVoice::NONE
                },
            ),
            (
                "guiroS",
                KitVoice {
                    t: HitType::Guiro,
                    len: Some(0.04),
                    rate0: Some(90.0),
                    rate1: Some(120.0),
                    bp: Some(3200.0),
                    q: Some(1.6),
                    lvl: Some(0.45),
                    pan: Some(-0.2),
                    ..KitVoice::NONE
                },
            ),
            (
                "shaker",
                KitVoice {
                    t: HitType::Shaker,
                    bp: Some(5500.0),
                    decay: Some(0.07),
                    attack: Some(0.008),
                    lvl: Some(0.35),
                    ..KitVoice::NONE
                },
            ),
            (
                "clave",
                KitVoice {
                    t: HitType::Rim,
                    f1: Some(2500.0),
                    f2: Some(3300.0),
                    decay: Some(0.035),
                    lvl: Some(0.22),
                    pan: Some(0.1),
                    ..KitVoice::NONE
                },
            ),
            (
                "crash",
                KitVoice {
                    t: HitType::Cymbal,
                    tune: Some(1.15),
                    decay: Some(1.4),
                    bp: Some(8000.0),
                    hp: Some(4500.0),
                    bp2: Some(3800.0),
                    decay2: Some(0.2),
                    noise: Some(0.4),
                    lvl: Some(0.3),
                    ..KitVoice::NONE
                },
            ),
            (
                "revCrash",
                KitVoice {
                    t: HitType::Swell,
                    lvl: Some(0.28),
                    ..KitVoice::NONE
                },
            ),
        ],
    ),
];

/// The game's kit sample names (samples.js) as tweaks of the lab's voices.
pub static SAMPLE_TWEAK: &[(&str, &[(&str, f64)])] = &[
    ("kickSoft", &[("decay", 0.6), ("click", 0.3), ("lvl", 0.85)]),
    ("kickBoom", &[("decay", 1.5), ("tune", 0.92)]),
    ("kickTight", &[("decay", 0.55), ("tune", 1.12)]),
    ("snareGated", &[("gate", 0.2), ("snappy", 1.5)]),
    ("snareSoft", &[("tone", 0.7), ("lvl", 0.8)]),
    ("snareCrisp", &[("snappy", 1.3), ("tune", 1.08)]),
    ("snareFat", &[("tune", 0.85), ("decay", 1.5)]),
    ("clapBig", &[("tail", 1.9)]),
    ("hatSoft", &[("decay", 0.8), ("hp", 0.8), ("lvl", 0.8)]),
];
