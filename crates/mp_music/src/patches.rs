//! The lab's patches (`instruments.js` `BPATCH`), generated from the
//! evaluated JS objects by `tools/music-lab/test/gen-rust-tables.py` over
//! `node tools/music-lab/test/dump-tables.mjs`, not transcribed from the
//! source: an object literal that writes a key twice keeps the last value
//! (the FM operators' `r`: the ratio, then the release), and that is what
//! the lab plays with. The table's canonical JSON hashes to the lab's
//! (`tests/patches.rs`). Regenerate whenever `BPATCH` changes.

use crate::track::{Lab, Op, OscSpec};
use std::sync::OnceLock;

/// The lab's `BPATCH[name]`, a copy.
pub fn bpatch(name: &str) -> Option<Lab> {
    all()
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, l)| l.clone())
}

/// Every patch, in the lab's order.
pub fn all() -> &'static [(&'static str, Lab)] {
    static ALL: OnceLock<Vec<(&'static str, Lab)>> = OnceLock::new();
    ALL.get_or_init(build)
}

/// The SHA-256 of the lab's `canon(BPATCH)` (`dump-tables.mjs` prints it).
pub const LAB_SHA256: &str = "6a4f0edecc3a35ac286a2ab0428c82a40c4f3b48f937b58b7e5b369b97f76547";

fn build() -> Vec<(&'static str, Lab)> {
    vec![
        (
            "sawBass",
            Lab {
                kind: "mono".into(),
                osc: Some(vec![
                    OscSpec {
                        w: "saw".into(),
                        ..Default::default()
                    },
                    OscSpec {
                        w: "saw".into(),
                        det: Some(8.0),
                        ..Default::default()
                    },
                ]),
                sub: Some(0.6),
                cutoff: Some(260.0),
                res: Some(0.35),
                fenv: Some(2.6),
                fd: Some(0.2),
                fs: Some(0.15),
                drive: Some(1.2),
                a: Some(0.002),
                d: Some(0.3),
                s: Some(0.85),
                r: Some(0.05),
                gain: Some(0.1094),
                ..Default::default()
            },
        ),
        (
            "pluckBass",
            Lab {
                kind: "tb303".into(),
                wave: Some("square".into()),
                cutoff: Some(320.0),
                res: Some(0.55),
                env: Some(0.45),
                decay: Some(0.18),
                accent: Some(0.5),
                drive: Some(0.4),
                gain: Some(0.1997),
                ..Default::default()
            },
        ),
        (
            "reese",
            Lab {
                kind: "mono".into(),
                osc: Some(vec![
                    OscSpec {
                        w: "saw".into(),
                        det: Some(-22.0),
                        ..Default::default()
                    },
                    OscSpec {
                        w: "saw".into(),
                        det: Some(22.0),
                        ..Default::default()
                    },
                    OscSpec {
                        w: "saw".into(),
                        oct: Some(-1.0),
                        lvl: Some(0.6),
                        ..Default::default()
                    },
                ]),
                cutoff: Some(520.0),
                res: Some(0.2),
                fenv: Some(0.4),
                fd: Some(0.4),
                fs: Some(0.6),
                drive: Some(2.2),
                a: Some(0.01),
                s: Some(1.0),
                r: Some(0.08),
                gain: Some(0.1739),
                keytrack: Some(0.2),
                ..Default::default()
            },
        ),
        (
            "sub",
            Lab {
                kind: "mono".into(),
                osc: Some(vec![OscSpec {
                    w: "sine".into(),
                    ..Default::default()
                }]),
                cutoff: Some(900.0),
                res: Some(0.0),
                fenv: Some(0.0),
                a: Some(0.004),
                d: Some(0.5),
                s: Some(0.9),
                r: Some(0.08),
                glide: Some(0.08),
                gain: Some(0.1148),
                ..Default::default()
            },
        ),
        (
            "darkBass",
            Lab {
                kind: "mono".into(),
                osc: Some(vec![
                    OscSpec {
                        w: "saw".into(),
                        ..Default::default()
                    },
                    OscSpec {
                        w: "pulse".into(),
                        det: Some(-6.0),
                        pw: Some(0.4),
                        ..Default::default()
                    },
                ]),
                sub: Some(0.5),
                cutoff: Some(420.0),
                res: Some(0.3),
                fenv: Some(2.0),
                fd: Some(0.14),
                fs: Some(0.2),
                drive: Some(2.4),
                a: Some(0.002),
                d: Some(0.3),
                s: Some(0.8),
                r: Some(0.04),
                gain: Some(0.127),
                ..Default::default()
            },
        ),
        (
            "acid",
            Lab {
                kind: "tb303".into(),
                wave: Some("saw".into()),
                cutoff: Some(300.0),
                res: Some(0.82),
                env: Some(0.62),
                decay: Some(0.32),
                accent: Some(0.75),
                drive: Some(0.9),
                glide: Some(0.06),
                gain: Some(0.127),
                ..Default::default()
            },
        ),
        (
            "roundBass",
            Lab {
                kind: "mono".into(),
                osc: Some(vec![OscSpec {
                    w: "tri".into(),
                    ..Default::default()
                }]),
                sub: Some(0.7),
                cutoff: Some(800.0),
                res: Some(0.05),
                fenv: Some(1.0),
                fd: Some(0.12),
                fs: Some(0.3),
                a: Some(0.003),
                d: Some(0.3),
                s: Some(0.85),
                r: Some(0.08),
                gain: Some(0.1892),
                ..Default::default()
            },
        ),
        (
            "superPad",
            Lab {
                kind: "juno".into(),
                saw: Some(0.9),
                pulse: Some(0.5),
                pwm: Some(0.7),
                sub: Some(0.25),
                unison: Some(2.0),
                detune: Some(14.0),
                cutoff: Some(1900.0),
                res: Some(0.12),
                fenv: Some(0.6),
                fa: Some(1.2),
                fd: Some(1.5),
                fs: Some(0.7),
                a: Some(0.35),
                d: Some(1.0),
                s: Some(0.85),
                r: Some(1.0),
                hpf: Some(140.0),
                chorus: Some(2.0),
                lfo_rate: Some(0.45),
                gain: Some(0.1973),
                ..Default::default()
            },
        ),
        (
            "warmPad",
            Lab {
                kind: "juno".into(),
                saw: Some(0.0),
                pulse: Some(0.9),
                pwm: Some(0.6),
                sub: Some(0.35),
                cutoff: Some(1300.0),
                res: Some(0.1),
                fenv: Some(0.3),
                a: Some(0.8),
                d: Some(1.0),
                s: Some(1.0),
                r: Some(1.6),
                hpf: Some(120.0),
                chorus: Some(1.0),
                lfo_rate: Some(0.3),
                gain: Some(0.1177),
                ..Default::default()
            },
        ),
        (
            "darkPad",
            Lab {
                kind: "juno".into(),
                saw: Some(1.0),
                pulse: Some(0.3),
                pwm: Some(0.5),
                sub: Some(0.2),
                unison: Some(2.0),
                detune: Some(10.0),
                cutoff: Some(750.0),
                res: Some(0.25),
                fenv: Some(0.4),
                fa: Some(1.5),
                fd: Some(2.0),
                fs: Some(0.6),
                a: Some(0.7),
                s: Some(1.0),
                r: Some(1.1),
                hpf: Some(110.0),
                chorus: Some(2.0),
                lfo_rate: Some(0.25),
                gain: Some(0.1654),
                ..Default::default()
            },
        ),
        (
            "stab",
            Lab {
                kind: "juno".into(),
                saw: Some(1.0),
                pulse: Some(0.4),
                sub: Some(0.15),
                unison: Some(2.0),
                detune: Some(12.0),
                cutoff: Some(1300.0),
                res: Some(0.2),
                fenv: Some(1.6),
                fa: Some(0.001),
                fd: Some(0.18),
                fs: Some(0.15),
                a: Some(0.002),
                d: Some(0.25),
                s: Some(0.3),
                r: Some(0.15),
                chorus: Some(1.0),
                gain: Some(0.5114),
                ..Default::default()
            },
        ),
        (
            "hit",
            Lab {
                kind: "juno".into(),
                saw: Some(1.0),
                pulse: Some(0.2),
                sub: Some(0.45),
                unison: Some(3.0),
                detune: Some(22.0),
                cutoff: Some(2200.0),
                res: Some(0.15),
                fenv: Some(1.2),
                fa: Some(0.001),
                fd: Some(0.35),
                fs: Some(0.3),
                a: Some(0.004),
                d: Some(0.6),
                s: Some(0.25),
                r: Some(0.5),
                chorus: Some(2.0),
                gain: Some(0.3034),
                ..Default::default()
            },
        ),
        (
            "ep",
            Lab {
                kind: "fm".into(),
                algo: Some("ep".into()),
                ops: Some(vec![
                    Some(Op {
                        r: Some(0.5),
                        l: Some(1.0),
                        a: Some(0.002),
                        d: Some(1.6),
                        s: Some(0.15),
                        ..Default::default()
                    }),
                    Some(Op {
                        r: Some(1.0),
                        l: Some(1.6),
                        d: Some(0.9),
                        s: Some(0.12),
                        v: Some(0.8),
                        ..Default::default()
                    }),
                    Some(Op {
                        r: Some(0.4),
                        l: Some(0.6),
                        a: Some(0.002),
                        d: Some(1.1),
                        s: Some(0.1),
                        det: Some(7.0),
                        ..Default::default()
                    }),
                    Some(Op {
                        r: Some(14.0),
                        l: Some(0.9),
                        d: Some(0.05),
                        s: Some(0.0),
                        v: Some(0.9),
                        ..Default::default()
                    }),
                ]),
                r: Some(0.45),
                chorus: Some(1.0),
                gain: Some(0.2335),
                ..Default::default()
            },
        ),
        (
            "bell",
            Lab {
                kind: "fm".into(),
                algo: Some("bell".into()),
                ops: Some(vec![
                    Some(Op {
                        r: Some(0.8),
                        l: Some(1.0),
                        d: Some(1.6),
                        s: Some(0.05),
                        ..Default::default()
                    }),
                    Some(Op {
                        r: Some(3.5),
                        l: Some(2.4),
                        d: Some(0.8),
                        s: Some(0.05),
                        ..Default::default()
                    }),
                    Some(Op {
                        r: Some(0.9),
                        l: Some(0.5),
                        d: Some(2.4),
                        s: Some(0.0),
                        det: Some(4.0),
                        ..Default::default()
                    }),
                    Some(Op {
                        r: Some(7.07),
                        l: Some(1.2),
                        d: Some(0.4),
                        s: Some(0.0),
                        ..Default::default()
                    }),
                ]),
                r: Some(0.7),
                chorus: Some(1.0),
                gain: Some(0.0914),
                ..Default::default()
            },
        ),
        (
            "glass",
            Lab {
                kind: "fm".into(),
                algo: Some("pair".into()),
                ops: Some(vec![
                    Some(Op {
                        r: Some(0.35),
                        l: Some(1.0),
                        d: Some(0.5),
                        s: Some(0.1),
                        ..Default::default()
                    }),
                    Some(Op {
                        r: Some(2.0),
                        l: Some(1.3),
                        d: Some(0.3),
                        s: Some(0.1),
                        ..Default::default()
                    }),
                ]),
                r: Some(0.3),
                chorus: Some(1.0),
                gain: Some(0.069),
                ..Default::default()
            },
        ),
        (
            "pluck",
            Lab {
                kind: "juno".into(),
                saw: Some(1.0),
                pulse: Some(0.0),
                unison: Some(2.0),
                detune: Some(8.0),
                cutoff: Some(500.0),
                res: Some(0.3),
                fenv: Some(3.2),
                fa: Some(0.001),
                fd: Some(0.11),
                fs: Some(0.0),
                a: Some(0.002),
                d: Some(0.2),
                s: Some(0.0),
                r: Some(0.15),
                chorus: Some(1.0),
                gain: Some(0.2148),
                ..Default::default()
            },
        ),
        (
            "sqArp",
            Lab {
                kind: "juno".into(),
                saw: Some(0.0),
                pulse: Some(1.0),
                pw: Some(0.25),
                cutoff: Some(2400.0),
                res: Some(0.15),
                fenv: Some(0.8),
                fa: Some(0.001),
                fd: Some(0.09),
                fs: Some(0.3),
                a: Some(0.002),
                d: Some(0.14),
                s: Some(0.5),
                r: Some(0.1),
                chorus: Some(1.0),
                gain: Some(0.0932),
                ..Default::default()
            },
        ),
        (
            "twang",
            Lab {
                kind: "fm".into(),
                algo: Some("stack".into()),
                ops: Some(vec![
                    Some(Op {
                        r: Some(0.3),
                        l: Some(1.0),
                        d: Some(0.8),
                        s: Some(0.2),
                        ..Default::default()
                    }),
                    Some(Op {
                        r: Some(1.0),
                        l: Some(2.4),
                        d: Some(0.25),
                        s: Some(0.15),
                        ..Default::default()
                    }),
                    Some(Op {
                        r: Some(3.0),
                        l: Some(0.9),
                        d: Some(0.08),
                        s: Some(0.0),
                        ..Default::default()
                    }),
                    Some(Op {
                        r: Some(1.0),
                        l: Some(0.3),
                        d: Some(0.1),
                        s: Some(0.0),
                        ..Default::default()
                    }),
                ]),
                bend: Some(0.7),
                bend_t: Some(0.07),
                r: Some(0.3),
                chorus: Some(0.0),
                gain: Some(0.081),
                ..Default::default()
            },
        ),
        (
            "brassLead",
            Lab {
                kind: "mono".into(),
                osc: Some(vec![
                    OscSpec {
                        w: "saw".into(),
                        ..Default::default()
                    },
                    OscSpec {
                        w: "saw".into(),
                        det: Some(9.0),
                        ..Default::default()
                    },
                ]),
                cutoff: Some(1300.0),
                res: Some(0.2),
                fenv: Some(1.5),
                fa: Some(0.04),
                fd: Some(0.35),
                fs: Some(0.55),
                a: Some(0.02),
                d: Some(0.35),
                s: Some(0.85),
                r: Some(0.2),
                vib: Some(14.0),
                vib_rate: Some(5.3),
                vib_delay: Some(0.25),
                drive: Some(0.8),
                gain: Some(0.0693),
                ..Default::default()
            },
        ),
        (
            "sawLead",
            Lab {
                kind: "mono".into(),
                osc: Some(vec![
                    OscSpec {
                        w: "saw".into(),
                        det: Some(-12.0),
                        ..Default::default()
                    },
                    OscSpec {
                        w: "saw".into(),
                        det: Some(12.0),
                        ..Default::default()
                    },
                    OscSpec {
                        w: "saw".into(),
                        oct: Some(1.0),
                        lvl: Some(0.35),
                        ..Default::default()
                    },
                ]),
                cutoff: Some(3200.0),
                res: Some(0.1),
                fenv: Some(0.6),
                fd: Some(0.3),
                fs: Some(0.6),
                a: Some(0.008),
                s: Some(0.9),
                r: Some(0.14),
                vib: Some(10.0),
                vib_delay: Some(0.3),
                glide: Some(0.05),
                drive: Some(0.6),
                gain: Some(0.0682),
                ..Default::default()
            },
        ),
        (
            "sqLead",
            Lab {
                kind: "mono".into(),
                osc: Some(vec![
                    OscSpec {
                        w: "pulse".into(),
                        pw: Some(0.35),
                        ..Default::default()
                    },
                    OscSpec {
                        w: "pulse".into(),
                        det: Some(7.0),
                        lvl: Some(0.7),
                        pw: Some(0.5),
                        ..Default::default()
                    },
                ]),
                cutoff: Some(2800.0),
                res: Some(0.25),
                fenv: Some(0.8),
                fd: Some(0.2),
                fs: Some(0.5),
                a: Some(0.006),
                s: Some(0.85),
                r: Some(0.12),
                vib: Some(12.0),
                vib_delay: Some(0.2),
                glide: Some(0.04),
                gain: Some(0.0838),
                ..Default::default()
            },
        ),
        (
            "whistle",
            Lab {
                kind: "mono".into(),
                osc: Some(vec![OscSpec {
                    w: "sine".into(),
                    ..Default::default()
                }]),
                noise: Some(0.04),
                cutoff: Some(6000.0),
                res: Some(0.0),
                fenv: Some(0.0),
                a: Some(0.03),
                s: Some(0.9),
                r: Some(0.25),
                vib: Some(24.0),
                vib_rate: Some(5.8),
                vib_delay: Some(0.16),
                bend: Some(0.5),
                bend_t: Some(0.08),
                glide: Some(0.06),
                gain: Some(0.0931),
                ..Default::default()
            },
        ),
        (
            "flute",
            Lab {
                kind: "mono".into(),
                osc: Some(vec![
                    OscSpec {
                        w: "tri".into(),
                        ..Default::default()
                    },
                    OscSpec {
                        w: "sine".into(),
                        oct: Some(1.0),
                        lvl: Some(0.15),
                        ..Default::default()
                    },
                ]),
                noise: Some(0.12),
                cutoff: Some(2600.0),
                res: Some(0.05),
                fenv: Some(0.5),
                fa: Some(0.06),
                fd: Some(0.3),
                fs: Some(0.6),
                a: Some(0.05),
                s: Some(0.85),
                r: Some(0.22),
                vib: Some(16.0),
                vib_delay: Some(0.25),
                glide: Some(0.05),
                gain: Some(0.1238),
                ..Default::default()
            },
        ),
        (
            "organ",
            Lab {
                kind: "fm".into(),
                algo: Some("organ".into()),
                ops: Some(vec![
                    Some(Op {
                        r: Some(0.08),
                        l: Some(1.0),
                        a: Some(0.002),
                        d: Some(0.3),
                        s: Some(0.7),
                        ..Default::default()
                    }),
                    Some(Op {
                        r: Some(0.08),
                        l: Some(0.6),
                        a: Some(0.002),
                        d: Some(0.2),
                        s: Some(0.5),
                        ..Default::default()
                    }),
                    Some(Op {
                        r: Some(0.06),
                        l: Some(0.35),
                        a: Some(0.001),
                        d: Some(0.08),
                        s: Some(0.2),
                        ..Default::default()
                    }),
                    Some(Op {
                        r: Some(0.05),
                        l: Some(0.25),
                        a: Some(0.001),
                        d: Some(0.05),
                        s: Some(0.0),
                        ..Default::default()
                    }),
                ]),
                r: Some(0.08),
                chorus: Some(1.0),
                gain: Some(0.19),
                ..Default::default()
            },
        ),
        (
            "dubChord",
            Lab {
                kind: "juno".into(),
                saw: Some(1.0),
                pulse: Some(0.5),
                pw: Some(0.4),
                unison: Some(2.0),
                detune: Some(9.0),
                cutoff: Some(900.0),
                res: Some(0.35),
                fenv: Some(1.8),
                fa: Some(0.001),
                fd: Some(0.16),
                fs: Some(0.05),
                a: Some(0.002),
                d: Some(0.22),
                s: Some(0.15),
                r: Some(0.2),
                hpf: Some(220.0),
                chorus: Some(1.0),
                gain: Some(0.45),
                ..Default::default()
            },
        ),
        (
            "rumble",
            Lab {
                kind: "mono".into(),
                osc: Some(vec![
                    OscSpec {
                        w: "sine".into(),
                        ..Default::default()
                    },
                    OscSpec {
                        w: "tri".into(),
                        lvl: Some(0.4),
                        ..Default::default()
                    },
                ]),
                cutoff: Some(260.0),
                res: Some(0.1),
                fenv: Some(0.8),
                fd: Some(0.12),
                fs: Some(0.0),
                drive: Some(2.5),
                a: Some(0.002),
                d: Some(0.22),
                s: Some(0.2),
                r: Some(0.1),
                gain: Some(0.12),
                ..Default::default()
            },
        ),
        (
            "supersaw",
            Lab {
                kind: "juno".into(),
                saw: Some(1.0),
                pulse: Some(0.0),
                unison: Some(7.0),
                detune: Some(36.0),
                cutoff: Some(5000.0),
                res: Some(0.05),
                fenv: Some(0.8),
                fa: Some(0.01),
                fd: Some(0.4),
                fs: Some(0.6),
                a: Some(0.01),
                d: Some(0.3),
                s: Some(0.85),
                r: Some(0.25),
                hpf: Some(260.0),
                chorus: Some(2.0),
                lfo_rate: Some(0.4),
                gain: Some(0.11),
                ..Default::default()
            },
        ),
        (
            "superPadWide",
            Lab {
                kind: "juno".into(),
                saw: Some(1.0),
                pulse: Some(0.2),
                pwm: Some(0.4),
                unison: Some(5.0),
                detune: Some(30.0),
                cutoff: Some(1600.0),
                res: Some(0.1),
                fenv: Some(0.9),
                fa: Some(1.5),
                fd: Some(2.0),
                fs: Some(0.7),
                a: Some(0.6),
                d: Some(1.0),
                s: Some(0.9),
                r: Some(1.4),
                hpf: Some(200.0),
                chorus: Some(2.0),
                lfo_rate: Some(0.3),
                gain: Some(0.1),
                ..Default::default()
            },
        ),
        (
            "tranceBass",
            Lab {
                kind: "mono".into(),
                osc: Some(vec![
                    OscSpec {
                        w: "saw".into(),
                        ..Default::default()
                    },
                    OscSpec {
                        w: "pulse".into(),
                        det: Some(4.0),
                        lvl: Some(0.5),
                        pw: Some(0.5),
                        ..Default::default()
                    },
                ]),
                sub: Some(0.5),
                cutoff: Some(180.0),
                res: Some(0.3),
                fenv: Some(2.4),
                fd: Some(0.1),
                fs: Some(0.05),
                drive: Some(1.4),
                a: Some(0.002),
                d: Some(0.14),
                s: Some(0.5),
                r: Some(0.04),
                gain: Some(0.13),
                ..Default::default()
            },
        ),
        (
            "psyBass",
            Lab {
                kind: "mono".into(),
                osc: Some(vec![
                    OscSpec {
                        w: "saw".into(),
                        ..Default::default()
                    },
                    OscSpec {
                        w: "pulse".into(),
                        oct: Some(-1.0),
                        lvl: Some(0.7),
                        pw: Some(0.5),
                        ..Default::default()
                    },
                ]),
                cutoff: Some(150.0),
                res: Some(0.42),
                fenv: Some(2.8),
                fd: Some(0.07),
                fs: Some(0.0),
                drive: Some(2.2),
                a: Some(0.001),
                d: Some(0.1),
                s: Some(0.3),
                r: Some(0.02),
                keytrack: Some(0.2),
                gain: Some(0.15),
                ..Default::default()
            },
        ),
        (
            "euroLead",
            Lab {
                kind: "mono".into(),
                osc: Some(vec![
                    OscSpec {
                        w: "saw".into(),
                        det: Some(-14.0),
                        ..Default::default()
                    },
                    OscSpec {
                        w: "saw".into(),
                        det: Some(14.0),
                        ..Default::default()
                    },
                    OscSpec {
                        w: "pulse".into(),
                        oct: Some(1.0),
                        lvl: Some(0.3),
                        pw: Some(0.3),
                        ..Default::default()
                    },
                ]),
                cutoff: Some(4200.0),
                res: Some(0.12),
                fenv: Some(0.8),
                fa: Some(0.005),
                fd: Some(0.25),
                fs: Some(0.7),
                a: Some(0.004),
                d: Some(0.2),
                s: Some(0.9),
                r: Some(0.12),
                vib: Some(18.0),
                vib_rate: Some(6.0),
                vib_delay: Some(0.18),
                glide: Some(0.03),
                drive: Some(0.7),
                gain: Some(0.07),
                ..Default::default()
            },
        ),
        (
            "piano",
            Lab {
                kind: "fm".into(),
                algo: Some("ep".into()),
                ops: Some(vec![
                    Some(Op {
                        r: Some(0.3),
                        l: Some(1.0),
                        a: Some(0.001),
                        d: Some(1.4),
                        s: Some(0.1),
                        ..Default::default()
                    }),
                    Some(Op {
                        r: Some(1.0),
                        l: Some(2.0),
                        d: Some(0.22),
                        s: Some(0.08),
                        v: Some(0.8),
                        ..Default::default()
                    }),
                    Some(Op {
                        r: Some(0.3),
                        l: Some(0.45),
                        a: Some(0.001),
                        d: Some(0.9),
                        s: Some(0.05),
                        det: Some(3.0),
                        ..Default::default()
                    }),
                    Some(Op {
                        r: Some(7.0),
                        l: Some(1.1),
                        d: Some(0.06),
                        s: Some(0.0),
                        v: Some(0.9),
                        ..Default::default()
                    }),
                ]),
                r: Some(0.3),
                chorus: Some(0.0),
                gain: Some(0.2),
                ..Default::default()
            },
        ),
        (
            "choir",
            Lab {
                kind: "juno".into(),
                vowel: Some("a".into()),
                saw: Some(0.4),
                pulse: Some(0.8),
                pwm: Some(0.5),
                sub: Some(0.0),
                unison: Some(3.0),
                detune: Some(12.0),
                cutoff: Some(3500.0),
                res: Some(0.05),
                fenv: Some(0.0),
                a: Some(0.5),
                d: Some(1.0),
                s: Some(1.0),
                r: Some(1.2),
                hpf: Some(150.0),
                chorus: Some(2.0),
                lfo_rate: Some(0.35),
                vowel_mix: Some(0.85),
                vowel_q: Some(8.0),
                gain: Some(0.07),
                ..Default::default()
            },
        ),
        (
            "pluckTrance",
            Lab {
                kind: "juno".into(),
                saw: Some(1.0),
                pulse: Some(0.3),
                pw: Some(0.3),
                unison: Some(3.0),
                detune: Some(14.0),
                cutoff: Some(900.0),
                res: Some(0.25),
                fenv: Some(3.0),
                fa: Some(0.001),
                fd: Some(0.13),
                fs: Some(0.0),
                a: Some(0.001),
                d: Some(0.22),
                s: Some(0.0),
                r: Some(0.15),
                hpf: Some(200.0),
                chorus: Some(2.0),
                gain: Some(0.16),
                ..Default::default()
            },
        ),
        (
            "zap",
            Lab {
                kind: "fm".into(),
                algo: Some("stack".into()),
                ops: Some(vec![
                    Some(Op {
                        r: Some(0.1),
                        l: Some(1.0),
                        a: Some(0.001),
                        d: Some(0.3),
                        s: Some(0.2),
                        ..Default::default()
                    }),
                    Some(Op {
                        r: Some(2.0),
                        l: Some(2.6),
                        d: Some(0.07),
                        s: Some(0.1),
                        v: Some(0.9),
                        ..Default::default()
                    }),
                    Some(Op {
                        r: Some(3.0),
                        l: Some(1.2),
                        d: Some(0.04),
                        s: Some(0.0),
                        ..Default::default()
                    }),
                    Some(Op {
                        r: Some(1.0),
                        l: Some(0.4),
                        d: Some(0.05),
                        s: Some(0.0),
                        ..Default::default()
                    }),
                ]),
                fbk: Some(0.3),
                r: Some(0.1),
                chorus: Some(1.0),
                gain: Some(0.1),
                ..Default::default()
            },
        ),
        (
            "surfGuitar",
            Lab {
                kind: "string".into(),
                decay: Some(1.8),
                damp: Some(0.3),
                pick: Some(0.65),
                body: Some(2900.0),
                body_q: Some(1.4),
                body_mix: Some(0.6),
                tone: Some(5200.0),
                drive: Some(0.5),
                trem: Some(0.45),
                trem_rate: Some(5.6),
                bend: Some(0.25),
                bend_t: Some(0.04),
                glide: Some(0.04),
                r: Some(0.5),
                chorus: Some(0.0),
                gain: Some(0.3),
                ..Default::default()
            },
        ),
        (
            "wahGuitar",
            Lab {
                kind: "string".into(),
                decay: Some(1.8),
                damp: Some(0.3),
                pick: Some(0.7),
                body: Some(2600.0),
                body_q: Some(1.2),
                body_mix: Some(0.5),
                tone: Some(6000.0),
                drive: Some(0.9),
                wah: Some(2.2),
                wah_hz: Some(380.0),
                wah_q: Some(4.5),
                wah_rate: Some(1.6),
                bend: Some(0.2),
                bend_t: Some(0.04),
                glide: Some(0.04),
                r: Some(0.4),
                chorus: Some(0.0),
                gain: Some(0.22),
                ..Default::default()
            },
        ),
        (
            "rhythmGuitar",
            Lab {
                kind: "string".into(),
                decay: Some(0.35),
                damp: Some(0.55),
                pick: Some(0.45),
                body: Some(2200.0),
                body_q: Some(1.0),
                body_mix: Some(0.4),
                tone: Some(4200.0),
                drive: Some(0.3),
                strum: Some(0.014),
                r: Some(0.06),
                chorus: Some(0.0),
                gain: Some(0.6),
                ..Default::default()
            },
        ),
        (
            "fingerBass",
            Lab {
                kind: "string".into(),
                decay: Some(1.2),
                damp: Some(0.75),
                pick: Some(0.25),
                body: Some(320.0),
                body_q: Some(0.8),
                body_mix: Some(0.5),
                tone: Some(1800.0),
                drive: Some(0.6),
                r: Some(0.08),
                chorus: Some(0.0),
                gain: Some(0.5),
                ..Default::default()
            },
        ),
        (
            "comboOrgan",
            Lab {
                kind: "fm".into(),
                algo: Some("organ".into()),
                ops: Some(vec![
                    Some(Op {
                        r: Some(0.06),
                        l: Some(1.0),
                        a: Some(0.004),
                        d: Some(0.3),
                        s: Some(0.8),
                        ..Default::default()
                    }),
                    Some(Op {
                        r: Some(0.06),
                        l: Some(0.8),
                        a: Some(0.004),
                        d: Some(0.2),
                        s: Some(0.7),
                        ..Default::default()
                    }),
                    Some(Op {
                        r: Some(0.05),
                        l: Some(0.55),
                        a: Some(0.003),
                        d: Some(0.1),
                        s: Some(0.45),
                        ..Default::default()
                    }),
                    Some(Op {
                        r: Some(0.05),
                        l: Some(0.4),
                        a: Some(0.002),
                        d: Some(0.08),
                        s: Some(0.3),
                        ..Default::default()
                    }),
                ]),
                r: Some(0.06),
                chorus: Some(3.0),
                gain: Some(0.14),
                ..Default::default()
            },
        ),
    ]
}
