//! The soundtrack (`src/game/audio/tracks.js`): seven arranged songs for the
//! Music sequencer ([`crate::music`]).
//!
//! A track has a tempo, chord progressions (one chord per bar, or several
//! per bar joined by commas), drum patterns (one char per 16th: X accent,
//! x hit, o ghost, . rest; patterns longer than a bar loop over bars), parts
//! (instrument patch + mixer channel + patterns) and a list of sections that
//! say which patterns play, for how many bars, and what happens at the
//! seams: crash, drop (sub boom), fill, riser / swell (noise riser and
//! reverse crash into the next section), down (noise downlifter), gap (the
//! last 16ths go silent) and lp (a low-pass sweep over the whole section).
//!
//! Part types:
//!   bass  one char per 16th relative to the chord's bass note: r root,
//!         o octave, f fifth, t third, s seventh, l low root, u high fifth;
//!         uppercase accents, '-' holds, '~' holds and slides into the next.
//!   chord x / X hits of the voiced chord, '-' holds.
//!   arp   space-separated chord-tone indexes (0 = lowest; past the top wraps up
//!         an octave), '_' hold, '.' rest; `res` 16ths per token.
//!   mel   space-separated notes (C#5), '_' hold, '.' rest, ! accent; `res`.
//!
//! Patch fields (Music.note): type saw|square|tri|sine|pulse|fm, voices,
//! detune (cents across the stack), width (stereo), sub, oct, cutoff, q,
//! fenv / fdec (filter envelope), fattack, keytrack, a d s r, gain, vib /
//! vibRate / vibDelay, bend / bendT, glide; fm: mods [{ratio, index, dec,
//! sus}], fmDetune. Channel fields: level, pan, rev, dly, pump, drive, hp,
//! chorus.
//!
//! Port: JS objects become structs with `Option` for each optional field
//! (`None` is `undefined`), and every object the JS iterates with
//! `Object.entries` a slice of pairs in its source order (DECISIONS D252).

/// An FM modulator.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FmMod {
    pub ratio: f64,
    pub index: f64,
    pub dec: Option<f64>,
    pub sus: Option<f64>,
}

/// A synth patch (`Music.note`'s `P`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Patch {
    pub kind: &'static str,
    pub voices: Option<f64>,
    pub detune: Option<f64>,
    pub width: Option<f64>,
    pub sub: Option<f64>,
    pub sub_type: Option<&'static str>,
    pub oct: Option<f64>,
    pub cutoff: Option<f64>,
    pub q: Option<f64>,
    pub fenv: Option<f64>,
    pub fdec: Option<f64>,
    pub fattack: Option<f64>,
    pub keytrack: Option<f64>,
    pub a: Option<f64>,
    pub d: Option<f64>,
    pub s: Option<f64>,
    pub r: Option<f64>,
    pub gain: Option<f64>,
    pub vib: Option<f64>,
    pub vib_rate: Option<f64>,
    pub vib_delay: Option<f64>,
    pub bend: Option<f64>,
    pub bend_t: Option<f64>,
    pub glide: Option<f64>,
    pub mods: Option<&'static [FmMod]>,
    pub fm_detune: Option<f64>,
}

impl Patch {
    /// Every field missing (spread it: `Patch { kind: "saw", ..Patch::NONE }`).
    pub const NONE: Patch = Patch {
        kind: "",
        voices: None,
        detune: None,
        width: None,
        sub: None,
        sub_type: None,
        oct: None,
        cutoff: None,
        q: None,
        fenv: None,
        fdec: None,
        fattack: None,
        keytrack: None,
        a: None,
        d: None,
        s: None,
        r: None,
        gain: None,
        vib: None,
        vib_rate: None,
        vib_delay: None,
        bend: None,
        bend_t: None,
        glide: None,
        mods: None,
        fm_detune: None,
    };
}

/// A part's mixer channel (`ch`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Channel {
    pub level: Option<f64>,
    pub pan: Option<f64>,
    pub rev: Option<f64>,
    pub dly: Option<f64>,
    pub pump: bool,
    pub drive: Option<f64>,
    pub drive_lp: Option<f64>,
    pub hp: Option<f64>,
    pub chorus: Option<f64>,
}

impl Channel {
    pub const NONE: Channel = Channel {
        level: None,
        pan: None,
        rev: None,
        dly: None,
        pump: false,
        drive: None,
        drive_lp: None,
        hp: None,
        chorus: None,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartKind {
    Bass,
    Chord,
    Arp,
    Mel,
}

/// A part: an instrument, a channel and its patterns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Part {
    pub name: &'static str,
    pub kind: PartKind,
    pub inst: Patch,
    pub lo: Option<f64>,
    pub res: Option<usize>,
    pub legato: bool,
    pub gate: Option<f64>,
    pub ch: Channel,
    pub pat: &'static [(&'static str, &'static str)],
}

const PART: Part = Part {
    name: "",
    kind: PartKind::Mel,
    inst: Patch::NONE,
    lo: None,
    res: None,
    legato: false,
    gate: None,
    ch: Channel::NONE,
    pat: &[],
};

/// A section of the arrangement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Section {
    pub bars: u32,
    pub prog: Option<&'static str>,
    pub drums: Option<&'static str>,
    pub lp: Option<[f64; 2]>,
    pub swell: bool,
    pub crash: bool,
    pub drop: bool,
    pub fill: Option<&'static str>,
    pub riser: Option<u32>,
    pub down: Option<u32>,
    pub gap: Option<u32>,
    pub p: &'static [(&'static str, &'static str)],
}

const SEC: Section = Section {
    bars: 0,
    prog: None,
    drums: None,
    lp: None,
    swell: false,
    crash: false,
    drop: false,
    fill: None,
    riser: None,
    down: None,
    gap: None,
    p: &[],
};

/// A track's override of a kit voice (`kit: { voice: { s, g, rev, rate } }`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KitVoice {
    pub s: Option<&'static str>,
    pub g: Option<f64>,
    pub rev: Option<f64>,
    pub rate: Option<f64>,
}

const KV: KitVoice = KitVoice {
    s: None,
    g: None,
    rev: None,
    rate: None,
};

/// The sidechain pump (`pump: { depth, release }`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pump {
    pub depth: Option<f64>,
    pub release: Option<f64>,
}

/// Drum patterns: name → `[(voice, lane)]`.
pub type Drums = &'static [(&'static str, &'static [(&'static str, &'static str)])];

/// A song.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Track {
    pub id: &'static str,
    pub title: &'static str,
    pub style: &'static str,
    pub bpm: f64,
    pub swing: Option<f64>,
    pub gain: Option<f64>,
    pub delay: Option<f64>,
    pub delay_fb: Option<f64>,
    /// `T.rev` (the song's reverb send; no track sets it).
    pub rev: Option<f64>,
    /// `T.riserGain` (no track sets it).
    pub riser_gain: Option<f64>,
    pub pump: Option<Pump>,
    pub kit: &'static [(&'static str, KitVoice)],
    pub prog: &'static [(&'static str, &'static str)],
    pub drums: Drums,
    pub parts: &'static [Part],
    pub sections: &'static [Section],
}

// ── Patches ──────────────────────────────────────────────────────
pub mod patches {
    use super::{FmMod, Patch};

    const N: Patch = Patch::NONE;

    pub const SAW_BASS: Patch = Patch {
        kind: "saw",
        voices: Some(2.0),
        detune: Some(10.0),
        sub: Some(0.7),
        cutoff: Some(360.0),
        q: Some(3.0),
        fenv: Some(4.0),
        fdec: Some(0.14),
        a: Some(0.003),
        d: Some(0.2),
        s: Some(0.8),
        r: Some(0.05),
        gain: Some(0.13),
        ..N
    };
    pub const PLUCK_BASS: Patch = Patch {
        kind: "square",
        sub: Some(0.7),
        cutoff: Some(320.0),
        q: Some(5.0),
        fenv: Some(5.0),
        fdec: Some(0.09),
        d: Some(0.16),
        s: Some(0.45),
        r: Some(0.04),
        gain: Some(0.22),
        ..N
    };
    pub const REESE: Patch = Patch {
        kind: "saw",
        voices: Some(3.0),
        detune: Some(30.0),
        width: Some(0.3),
        sub: Some(0.9),
        cutoff: Some(560.0),
        q: Some(1.6),
        a: Some(0.01),
        s: Some(1.0),
        r: Some(0.08),
        gain: Some(0.2),
        ..N
    };
    pub const SUB: Patch = Patch {
        kind: "sine",
        a: Some(0.004),
        d: Some(0.5),
        s: Some(0.85),
        r: Some(0.08),
        gain: Some(0.15),
        glide: Some(0.08),
        ..N
    };
    pub const DARK_BASS: Patch = Patch {
        kind: "saw",
        voices: Some(2.0),
        detune: Some(14.0),
        sub: Some(0.6),
        cutoff: Some(650.0),
        q: Some(2.5),
        fenv: Some(3.0),
        fdec: Some(0.1),
        s: Some(0.7),
        r: Some(0.04),
        gain: Some(0.2),
        ..N
    };
    pub const ACID: Patch = Patch {
        kind: "saw",
        cutoff: Some(380.0),
        q: Some(13.0),
        fenv: Some(8.0),
        fdec: Some(0.17),
        a: Some(0.002),
        d: Some(0.22),
        s: Some(0.55),
        r: Some(0.03),
        gain: Some(0.14),
        glide: Some(0.07),
        ..N
    };
    pub const ROUND_BASS: Patch = Patch {
        kind: "tri",
        sub: Some(0.8),
        cutoff: Some(900.0),
        fenv: Some(1.0),
        fdec: Some(0.1),
        s: Some(0.8),
        r: Some(0.08),
        gain: Some(0.25),
        ..N
    };
    pub const SUPER_PAD: Patch = Patch {
        kind: "saw",
        voices: Some(4.0),
        detune: Some(26.0),
        width: Some(0.75),
        cutoff: Some(1900.0),
        q: Some(0.7),
        fattack: Some(1.2),
        a: Some(0.35),
        d: Some(1.0),
        s: Some(0.85),
        r: Some(0.9),
        gain: Some(0.075),
        ..N
    };
    pub const WARM_PAD: Patch = Patch {
        kind: "tri",
        voices: Some(2.0),
        detune: Some(12.0),
        width: Some(0.6),
        cutoff: Some(1500.0),
        a: Some(0.8),
        s: Some(1.0),
        r: Some(1.6),
        gain: Some(0.12),
        ..N
    };
    pub const DARK_PAD: Patch = Patch {
        kind: "saw",
        voices: Some(3.0),
        detune: Some(18.0),
        width: Some(0.8),
        cutoff: Some(850.0),
        q: Some(1.2),
        a: Some(0.7),
        s: Some(1.0),
        r: Some(1.1),
        gain: Some(0.065),
        ..N
    };
    pub const STAB: Patch = Patch {
        kind: "saw",
        voices: Some(3.0),
        detune: Some(22.0),
        width: Some(0.6),
        cutoff: Some(2300.0),
        q: Some(1.0),
        fenv: Some(1.6),
        fdec: Some(0.12),
        a: Some(0.003),
        d: Some(0.18),
        s: Some(0.3),
        r: Some(0.12),
        gain: Some(0.17),
        ..N
    };
    pub const HIT: Patch = Patch {
        kind: "saw",
        voices: Some(4.0),
        detune: Some(30.0),
        width: Some(0.8),
        sub: Some(0.3),
        cutoff: Some(3000.0),
        q: Some(1.0),
        fenv: Some(1.0),
        fdec: Some(0.3),
        a: Some(0.004),
        d: Some(0.5),
        s: Some(0.2),
        r: Some(0.4),
        gain: Some(0.15),
        ..N
    };
    pub const EP: Patch = Patch {
        kind: "fm",
        mods: Some(&[
            FmMod {
                ratio: 1.0,
                index: 1.5,
                dec: Some(0.9),
                sus: Some(0.15),
            },
            FmMod {
                ratio: 14.0,
                index: 0.16,
                dec: Some(0.05),
                sus: Some(0.0),
            },
        ]),
        fm_detune: Some(6.0),
        a: Some(0.003),
        d: Some(1.3),
        s: Some(0.25),
        r: Some(0.45),
        gain: Some(0.145),
        ..N
    };
    pub const BELL: Patch = Patch {
        kind: "fm",
        mods: Some(&[FmMod {
            ratio: 3.5,
            index: 2.2,
            dec: Some(0.6),
            sus: Some(0.05),
        }]),
        a: Some(0.002),
        d: Some(0.9),
        s: Some(0.1),
        r: Some(0.6),
        gain: Some(0.14),
        ..N
    };
    pub const GLASS: Patch = Patch {
        kind: "fm",
        mods: Some(&[FmMod {
            ratio: 2.0,
            index: 1.2,
            dec: Some(0.3),
            sus: Some(0.1),
        }]),
        a: Some(0.002),
        d: Some(0.35),
        s: Some(0.1),
        r: Some(0.3),
        gain: Some(0.12),
        ..N
    };
    pub const PLUCK: Patch = Patch {
        kind: "saw",
        voices: Some(2.0),
        detune: Some(8.0),
        cutoff: Some(650.0),
        q: Some(2.0),
        fenv: Some(6.0),
        fdec: Some(0.08),
        a: Some(0.002),
        d: Some(0.18),
        s: Some(0.0),
        r: Some(0.12),
        gain: Some(0.085),
        ..N
    };
    pub const SQ_ARP: Patch = Patch {
        kind: "pulse",
        cutoff: Some(3000.0),
        q: Some(1.0),
        fenv: Some(1.0),
        fdec: Some(0.08),
        d: Some(0.12),
        s: Some(0.5),
        r: Some(0.08),
        gain: Some(0.1),
        ..N
    };
    pub const TWANG: Patch = Patch {
        kind: "fm",
        mods: Some(&[
            FmMod {
                ratio: 1.0,
                index: 2.6,
                dec: Some(0.25),
                sus: Some(0.12),
            },
            FmMod {
                ratio: 3.0,
                index: 0.7,
                dec: Some(0.08),
                sus: Some(0.0),
            },
        ]),
        bend: Some(0.7),
        bend_t: Some(0.07),
        a: Some(0.002),
        d: Some(0.7),
        s: Some(0.2),
        r: Some(0.3),
        gain: Some(0.16),
        ..N
    };
    pub const BRASS_LEAD: Patch = Patch {
        kind: "saw",
        voices: Some(3.0),
        detune: Some(14.0),
        width: Some(0.4),
        cutoff: Some(2400.0),
        q: Some(1.5),
        fenv: Some(1.3),
        fdec: Some(0.25),
        a: Some(0.02),
        d: Some(0.35),
        s: Some(0.8),
        r: Some(0.2),
        vib: Some(14.0),
        vib_rate: Some(5.3),
        vib_delay: Some(0.25),
        gain: Some(0.12),
        ..N
    };
    pub const SAW_LEAD: Patch = Patch {
        kind: "saw",
        voices: Some(4.0),
        detune: Some(32.0),
        width: Some(0.6),
        cutoff: Some(4000.0),
        q: Some(1.0),
        a: Some(0.008),
        s: Some(0.9),
        r: Some(0.14),
        vib: Some(10.0),
        vib_delay: Some(0.3),
        gain: Some(0.1),
        glide: Some(0.05),
        ..N
    };
    pub const SQ_LEAD: Patch = Patch {
        kind: "pulse",
        voices: Some(2.0),
        detune: Some(10.0),
        width: Some(0.3),
        cutoff: Some(3400.0),
        q: Some(2.0),
        fenv: Some(0.8),
        fdec: Some(0.2),
        a: Some(0.006),
        s: Some(0.8),
        r: Some(0.12),
        vib: Some(12.0),
        vib_delay: Some(0.2),
        gain: Some(0.15),
        glide: Some(0.04),
        ..N
    };
    pub const WHISTLE: Patch = Patch {
        kind: "sine",
        a: Some(0.03),
        s: Some(0.9),
        r: Some(0.25),
        vib: Some(24.0),
        vib_rate: Some(5.8),
        vib_delay: Some(0.16),
        bend: Some(0.5),
        bend_t: Some(0.08),
        gain: Some(0.12),
        glide: Some(0.06),
        ..N
    };
    pub const FLUTE: Patch = Patch {
        kind: "tri",
        cutoff: Some(3200.0),
        a: Some(0.05),
        s: Some(0.85),
        r: Some(0.22),
        vib: Some(16.0),
        vib_delay: Some(0.25),
        gain: Some(0.14),
        glide: Some(0.05),
        ..N
    };

    /// `PATCHES`, by name in the JS order (the music player shows them).
    pub const ALL: &[(&str, Patch)] = &[
        ("sawBass", SAW_BASS),
        ("pluckBass", PLUCK_BASS),
        ("reese", REESE),
        ("sub", SUB),
        ("darkBass", DARK_BASS),
        ("acid", ACID),
        ("roundBass", ROUND_BASS),
        ("superPad", SUPER_PAD),
        ("warmPad", WARM_PAD),
        ("darkPad", DARK_PAD),
        ("stab", STAB),
        ("hit", HIT),
        ("ep", EP),
        ("bell", BELL),
        ("glass", GLASS),
        ("pluck", PLUCK),
        ("sqArp", SQ_ARP),
        ("twang", TWANG),
        ("brassLead", BRASS_LEAD),
        ("sawLead", SAW_LEAD),
        ("sqLead", SQ_LEAD),
        ("whistle", WHISTLE),
        ("flute", FLUTE),
    ];
}

use patches as p;

const fn ch() -> Channel {
    Channel::NONE
}

const fn kv() -> KitVoice {
    KV
}

// ── Songs ────────────────────────────────────────────────────────
pub const TRACKS: &[Track] = &[
    Track {
        id: "midnight-run", title: "Midnight Run", style: "Outrun", bpm: 110.0, swing: None, gain: Some(1.0),
        delay: None, delay_fb: None, rev: None, riser_gain: None,
        pump: Some(Pump { depth: Some(0.35), release: Some(0.2) }),
        kit: &[("kick", KitVoice { s: Some("kickPunch"), ..kv() }), ("snare", KitVoice { s: Some("snareGated"), rev: Some(0.3), ..kv() })],
        prog: &[("v", "Am Am F F C C G G"), ("c", "F G Am Am F G C E"), ("b", "Dm Am F G")],
        drums: &[
            ("hats", &[("hat", "..x...x...x...x.")]),
            ("verse", &[("kick", "x.......x.x....."), ("snare", "....X.......X..."), ("hat", "x.x.x.x.x.x.x.x.")]),
            ("chorus", &[("kick", "x...x...x...x..."), ("snare", "....X.......X..."), ("hat", "xoxoxoxoxoxoxoxo"), ("ohat", "..x...x...x...x.")]),
            ("build", &[("kick", "x...x...x...x..."), ("snare", "............x..."), ("hat", "x.x.x.x.x.x.x.x.")]),
            ("brk", &[("kick", "x..............."), ("hat", "..x...x...x...x.")]),
        ],
        parts: &[
            Part { name: "bass", kind: PartKind::Bass, inst: p::SAW_BASS, lo: Some(33.0), ch: Channel { pump: true, ..ch() }, pat: &[("drive", "rrrrrrrrrrrrrrrr"), ("oct", "r.o.r.o.r.o.r.o.")], ..PART },
            Part { name: "pad", kind: PartKind::Chord, inst: p::SUPER_PAD, lo: Some(57.0), ch: Channel { pump: true, rev: Some(0.5), hp: Some(160.0), ..ch() }, pat: &[("hold", "x---------------")], ..PART },
            Part { name: "arp", kind: PartKind::Arp, inst: p::SQ_ARP, lo: Some(64.0), ch: Channel { rev: Some(0.2), dly: Some(0.45), pan: Some(0.15), ..ch() }, pat: &[("a", "0 1 2 3 1 2 3 4 2 3 4 5 3 4 5 6"), ("b", "0 2 1 3 2 4 3 5 0 2 1 3 2 4 3 5")], ..PART },
            Part {
                name: "lead", kind: PartKind::Mel, inst: p::BRASS_LEAD, res: Some(2), ch: Channel { rev: Some(0.35), dly: Some(0.3), ..ch() },
                pat: &[("hook", "A4 _ _ G4 _ _ F4 _   G4 _ _ B4 _ _ D5 _   E5 _ _ _ _ _ D5 C5   E5 _ _ _ A4 _ _ _
                 A4 _ _ G4 _ _ F4 _   G4 _ _ B4 _ _ D5 _   E5 _ _ G5 _ _ E5 _   G#5 _ _ _ _ _ B4 _")],
                ..PART
            },
            Part { name: "bell", kind: PartKind::Mel, inst: p::BELL, res: Some(2), ch: Channel { rev: Some(0.5), dly: Some(0.4), pan: Some(-0.15), ..ch() }, pat: &[("m", "F5 _ E5 _ D5 _ A4 _   C5 _ _ _ E5 _ _ _   A5 _ G5 _ F5 _ C5 _   D5 _ _ _ B4 _ _ _")], ..PART },
        ],
        sections: &[
            Section { bars: 8, prog: Some("v"), drums: Some("hats"), lp: Some([600.0, 14000.0]), swell: true, p: &[("pad", "hold"), ("arp", "a")], ..SEC },
            Section { bars: 16, prog: Some("v"), drums: Some("verse"), crash: true, fill: Some("snare"), p: &[("bass", "drive"), ("pad", "hold"), ("arp", "a")], ..SEC },
            Section { bars: 16, prog: Some("c"), drums: Some("chorus"), crash: true, fill: Some("tom"), p: &[("bass", "oct"), ("pad", "hold"), ("lead", "hook"), ("arp", "b")], ..SEC },
            Section { bars: 8, prog: Some("b"), drums: Some("brk"), down: Some(2), p: &[("pad", "hold"), ("bell", "m")], ..SEC },
            Section { bars: 8, prog: Some("v"), drums: Some("build"), riser: Some(4), swell: true, fill: Some("roll"), gap: Some(2), p: &[("bass", "drive"), ("pad", "hold"), ("arp", "a")], ..SEC },
            Section { bars: 16, prog: Some("c"), drums: Some("chorus"), crash: true, drop: true, fill: Some("tom"), p: &[("bass", "oct"), ("pad", "hold"), ("lead", "hook"), ("arp", "b"), ("bell", "m")], ..SEC },
            Section { bars: 8, prog: Some("v"), drums: Some("hats"), crash: true, lp: Some([14000.0, 500.0]), p: &[("pad", "hold"), ("arp", "a")], ..SEC },
        ],
    },
    Track {
        id: "seabright", title: "Seabright Dawn", style: "Chill drive", bpm: 92.0, swing: Some(0.1), gain: Some(1.0),
        delay: Some(0.75), delay_fb: None, rev: None, riser_gain: None,
        pump: Some(Pump { depth: Some(0.15), release: Some(0.25) }),
        kit: &[
            ("kick", KitVoice { s: Some("kickSoft"), ..kv() }),
            ("snare", KitVoice { s: Some("snareSoft"), g: Some(0.75), rev: Some(0.35), ..kv() }),
            ("rim", KitVoice { rev: Some(0.35), ..kv() }),
            ("hat", KitVoice { s: Some("hatSoft"), g: Some(0.9), ..kv() }),
            ("crash", KitVoice { g: Some(0.7), ..kv() }),
        ],
        prog: &[("a", "Dmaj7 Bm7 Gmaj7 A"), ("c", "Gmaj7 A F#m7 Bm7 Gmaj7 A D D"), ("b", "Em7 F#m7 Gmaj7 A")],
        drums: &[
            ("intro", &[("shaker", "xoxoxoxoxoxoxoxo")]),
            ("groove", &[("kick", "x......x..x....."), ("rim", "....x.......x..."), ("shaker", "xoxoxoxoxoxoxoxo"), ("hat", "..x...x...x...x.")]),
            ("groove2", &[("kick", "x......x..x...x."), ("snare", "....x.......x..."), ("hat", "x.x.x.x.x.x.x.x."), ("shaker", "xoxoxoxoxoxoxoxo"), ("ohat", "..............x.")]),
        ],
        parts: &[
            Part { name: "bass", kind: PartKind::Bass, inst: p::ROUND_BASS, lo: Some(38.0), ch: Channel { level: Some(0.8), ..ch() }, pat: &[("a", "r.....rr..f.o..."), ("b", "r..r...ro..f.r..")], ..PART },
            Part { name: "ep", kind: PartKind::Chord, inst: p::EP, lo: Some(57.0), ch: Channel { chorus: Some(1.0), rev: Some(0.35), pump: true, ..ch() }, pat: &[("comp", "x-----.x------..")], ..PART },
            Part { name: "pad", kind: PartKind::Chord, inst: p::WARM_PAD, lo: Some(62.0), ch: Channel { rev: Some(0.5), hp: Some(200.0), pump: true, level: Some(0.8), ..ch() }, pat: &[("hold", "x---------------")], ..PART },
            Part { name: "pluck", kind: PartKind::Arp, inst: p::GLASS, lo: Some(69.0), res: Some(2), ch: Channel { rev: Some(0.4), dly: Some(0.5), pan: Some(0.25), ..ch() }, pat: &[("a", "0 2 4 2 3 5 4 2"), ("b", "0 1 2 4 3 2 1 2")], ..PART },
            Part {
                name: "lead", kind: PartKind::Mel, inst: Patch { oct: Some(1.0), ..p::FLUTE }, res: Some(2), legato: true, ch: Channel { rev: Some(0.45), dly: Some(0.3), pan: Some(-0.1), ..ch() },
                pat: &[("hook", "B4 _ _ A4 _ _ F#4 _   E4 _ _ _ A4 _ C#5 _   C#5 _ _ _ _ _ A4 _   B4 _ _ _ D5 _ _ _
                 B4 _ _ A4 _ _ F#4 _   E4 _ _ F#4 _ _ A4 _   F#4 _ _ _ _ _ E4 D4   D4 _ _ _ _ _ . .")],
                ..PART
            },
            Part { name: "bell", kind: PartKind::Mel, inst: p::BELL, res: Some(2), ch: Channel { rev: Some(0.55), dly: Some(0.45), pan: Some(0.2), ..ch() }, pat: &[("m", "G5 _ F#5 _ E5 _ B4 _   A5 _ _ _ C#5 _ _ _   B5 _ A5 _ F#5 _ D5 _   E5 _ _ _ _ _ . .")], ..PART },
        ],
        sections: &[
            Section { bars: 8, prog: Some("a"), drums: Some("intro"), lp: Some([700.0, 16000.0]), p: &[("ep", "comp"), ("pad", "hold")], ..SEC },
            Section { bars: 16, prog: Some("a"), drums: Some("groove"), p: &[("bass", "a"), ("ep", "comp"), ("pad", "hold"), ("pluck", "a")], ..SEC },
            Section { bars: 16, prog: Some("c"), drums: Some("groove2"), crash: true, fill: Some("snare"), p: &[("bass", "b"), ("ep", "comp"), ("pad", "hold"), ("lead", "hook"), ("pluck", "b")], ..SEC },
            Section { bars: 8, prog: Some("b"), drums: Some("intro"), down: Some(2), swell: true, p: &[("ep", "comp"), ("pad", "hold"), ("bell", "m")], ..SEC },
            Section { bars: 16, prog: Some("c"), drums: Some("groove2"), crash: true, p: &[("bass", "b"), ("ep", "comp"), ("pad", "hold"), ("lead", "hook"), ("pluck", "a")], ..SEC },
            Section { bars: 8, prog: Some("a"), drums: Some("intro"), lp: Some([16000.0, 600.0]), p: &[("ep", "comp"), ("pad", "hold"), ("pluck", "a")], ..SEC },
        ],
    },
    Track {
        id: "neon-rush", title: "Neon Rush", style: "Drum & bass", bpm: 172.0, swing: None, gain: Some(1.0),
        delay: None, delay_fb: None, rev: None, riser_gain: None,
        pump: Some(Pump { depth: Some(0.3), release: Some(0.12) }),
        kit: &[
            ("kick", KitVoice { s: Some("kickTight"), ..kv() }),
            ("snare", KitVoice { s: Some("snareCrisp"), rev: Some(0.2), ..kv() }),
            ("ride", KitVoice { g: Some(0.9), ..kv() }),
            ("hat", KitVoice { g: Some(0.9), ..kv() }),
        ],
        prog: &[("a", "Fm Fm Db Db Ab Ab Eb Eb"), ("b", "Bbm Bbm Db Db Fm Fm C C")],
        drums: &[
            ("ride", &[("ride", "x.x.x.x.x.x.x.x."), ("shaker", "xoxoxoxoxoxoxoxo")]),
            ("main", &[("kick", "x.........x....."), ("snare", "....X..o.o..X..."), ("hat", "x.x.x.x.x.xox.x."), ("ohat", "..............x.")]),
            ("main2", &[("kick", "x.x.......x..x.."), ("snare", "....X..o.o..X..o"), ("hat", "xoxoxoxoxoxoxoxo"), ("ride", "x...x...x...x...")]),
            ("half", &[("kick", "x..............."), ("snare", "........X......."), ("hat", "x.x.x.x.x.x.x.x.")]),
            ("build", &[("kick", "x...x...x...x..."), ("snare", "....x.......x..."), ("hat", "x.x.x.x.x.x.x.x.")]),
        ],
        parts: &[
            Part { name: "bass", kind: PartKind::Bass, inst: p::REESE, lo: Some(41.0), ch: Channel { drive: Some(1.6), drive_lp: Some(3000.0), level: Some(0.25), ..ch() }, pat: &[("reese", "r-----------r-o-"), ("roll", "r..r..r...r.o.r.")], ..PART },
            Part { name: "pad", kind: PartKind::Chord, inst: p::DARK_PAD, lo: Some(60.0), ch: Channel { rev: Some(0.6), hp: Some(220.0), pump: true, ..ch() }, pat: &[("hold", "x-------------------------------")], ..PART },
            Part { name: "stab", kind: PartKind::Chord, inst: p::STAB, lo: Some(60.0), ch: Channel { rev: Some(0.3), dly: Some(0.25), ..ch() }, pat: &[("s", "..x.......x..x..")], ..PART },
            Part { name: "arp", kind: PartKind::Arp, inst: p::SQ_ARP, lo: Some(65.0), ch: Channel { rev: Some(0.25), dly: Some(0.4), pan: Some(-0.2), ..ch() }, pat: &[("a", "0 1 2 3 4 3 2 1 0 1 2 3 4 3 2 1")], ..PART },
            Part {
                name: "lead", kind: PartKind::Mel, inst: p::SAW_LEAD, res: Some(2), legato: true, ch: Channel { rev: Some(0.35), dly: Some(0.25), ..ch() },
                pat: &[("hook", "C5 _ _ Ab4 _ _ F4 _   G4 _ Ab4 _ C5 _ _ _   Db5 _ _ C5 _ _ Ab4 _   F4 _ _ _ _ _ . .
                 Eb5 _ _ C5 _ _ Ab4 _   Bb4 _ C5 _ Eb5 _ _ _   G5 _ _ F5 _ _ Eb5 _   Bb4 _ _ _ G4 _ _ _")],
                ..PART
            },
            Part {
                name: "air", kind: PartKind::Mel, inst: Patch { gain: Some(0.1), ..p::FLUTE }, res: Some(4), legato: true, ch: Channel { rev: Some(0.6), dly: Some(0.35), ..ch() },
                pat: &[("m", "F5 _ Db5 _   Bb4 _ _ _   Ab4 _ F5 _   Eb5 _ Db5 _   C5 _ _ _   Ab4 _ _ _   G4 _ E5 _   _ _ . .")],
                ..PART
            },
        ],
        sections: &[
            Section { bars: 16, prog: Some("a"), drums: Some("ride"), lp: Some([500.0, 7000.0]), riser: Some(4), swell: true, gap: Some(2), p: &[("pad", "hold"), ("arp", "a")], ..SEC },
            Section { bars: 32, prog: Some("a"), drums: Some("main"), crash: true, drop: true, fill: Some("dnb"), p: &[("bass", "reese"), ("pad", "hold"), ("stab", "s")], ..SEC },
            Section { bars: 16, prog: Some("b"), drums: Some("main2"), crash: true, fill: Some("dnb"), p: &[("bass", "roll"), ("pad", "hold"), ("arp", "a")], ..SEC },
            Section { bars: 16, prog: Some("b"), drums: Some("half"), down: Some(2), p: &[("pad", "hold"), ("air", "m")], ..SEC },
            Section { bars: 8, prog: Some("a"), drums: Some("build"), riser: Some(8), swell: true, fill: Some("roll"), gap: Some(2), p: &[("pad", "hold"), ("arp", "a")], ..SEC },
            Section { bars: 32, prog: Some("a"), drums: Some("main"), crash: true, drop: true, fill: Some("dnb"), p: &[("bass", "reese"), ("pad", "hold"), ("stab", "s"), ("lead", "hook")], ..SEC },
            Section { bars: 8, prog: Some("a"), drums: Some("ride"), crash: true, lp: Some([12000.0, 400.0]), p: &[("pad", "hold"), ("arp", "a")], ..SEC },
        ],
    },
    Track {
        id: "mirage", title: "Mirage Highway", style: "Desert western", bpm: 96.0, swing: Some(0.14), gain: Some(0.92),
        delay: Some(0.5), delay_fb: Some(0.3), rev: None, riser_gain: None, pump: None,
        kit: &[
            ("kick", KitVoice { s: Some("kickBoom"), g: Some(0.9), ..kv() }),
            ("snare", KitVoice { s: Some("snareFat"), g: Some(0.85), rev: Some(0.45), ..kv() }),
            ("snap", KitVoice { rev: Some(0.4), ..kv() }),
            ("tomL", KitVoice { g: Some(0.9), rev: Some(0.35), ..kv() }),
            ("tomM", KitVoice { g: Some(0.9), rev: Some(0.35), ..kv() }),
            ("shaker", KitVoice { g: Some(1.2), ..kv() }),
        ],
        prog: &[("a", "Em D C B"), ("b", "Am Em B7 Em"), ("c", "C D Em Em C D B B")],
        drums: &[
            ("wind", &[("shaker", "x..ox..ox..ox..o"), ("tomL", "x...............")]),
            ("trot", &[("kick", "x.....x...x....."), ("snap", "....x.......x..."), ("shaker", "x.xox.xox.xox.xo")]),
            ("big", &[("kick", "x.....x...x....."), ("snare", "....X.......X..."), ("shaker", "x.xox.xox.xox.xo"), ("ohat", "..x...x...x...x."), ("tomM", "..............x.")]),
            ("half", &[("kick", "x..............."), ("snare", "........X......."), ("shaker", "x.xox.xox.xox.xo")]),
        ],
        parts: &[
            Part { name: "bass", kind: PartKind::Bass, inst: p::ROUND_BASS, lo: Some(40.0), ch: ch(), pat: &[("a", "r.....r.f...r..."), ("b", "r...f...r...f...")], ..PART },
            Part { name: "twang", kind: PartKind::Arp, inst: p::TWANG, lo: Some(52.0), res: Some(2), ch: Channel { rev: Some(0.45), dly: Some(0.35), pan: Some(0.2), ..ch() }, pat: &[("arp", "0 1 2 3 2 1 2 1"), ("arp2", "0 . 2 . 4 . 2 ."), ("strum", "0 1 2 3 . 3 2 1")], ..PART },
            Part { name: "pad", kind: PartKind::Chord, inst: p::DARK_PAD, lo: Some(52.0), ch: Channel { rev: Some(0.6), hp: Some(180.0), level: Some(0.85), ..ch() }, pat: &[("hold", "x---------------")], ..PART },
            Part {
                name: "lead", kind: PartKind::Mel, inst: p::WHISTLE, res: Some(2), legato: true, ch: Channel { rev: Some(0.6), dly: Some(0.35), pan: Some(-0.15), ..ch() },
                pat: &[("whistle", "E5 _ _ _ _ _ G5 _   F#5 _ _ _ A5 _ _ _   B5 _ _ _ _ _ _ _   G5 _ F#5 _ E5 _ _ _
                    E5 _ _ _ _ _ G5 _   A5 _ _ _ F#5 _ D5 _   D#5 _ _ _ _ _ _ _   F#5 _ _ _ D#5 _ B4 _")],
                ..PART
            },
        ],
        sections: &[
            Section { bars: 8, prog: Some("a"), drums: Some("wind"), lp: Some([900.0, 16000.0]), p: &[("twang", "arp"), ("pad", "hold")], ..SEC },
            Section { bars: 16, prog: Some("a"), drums: Some("trot"), crash: true, fill: Some("west"), p: &[("bass", "a"), ("twang", "arp"), ("pad", "hold")], ..SEC },
            Section { bars: 16, prog: Some("c"), drums: Some("big"), crash: true, fill: Some("west"), p: &[("bass", "b"), ("pad", "hold"), ("lead", "whistle"), ("twang", "strum")], ..SEC },
            Section { bars: 8, prog: Some("b"), drums: Some("half"), down: Some(2), swell: true, p: &[("pad", "hold"), ("twang", "arp2")], ..SEC },
            Section { bars: 16, prog: Some("c"), drums: Some("big"), crash: true, drop: true, fill: Some("west"), p: &[("bass", "b"), ("pad", "hold"), ("lead", "whistle"), ("twang", "strum")], ..SEC },
            Section { bars: 8, prog: Some("a"), drums: Some("wind"), lp: Some([16000.0, 700.0]), p: &[("twang", "arp"), ("pad", "hold")], ..SEC },
        ],
    },
    Track {
        id: "interstate", title: "Interstate Nights", style: "Night drive house", bpm: 122.0, swing: None, gain: Some(1.0),
        delay: None, delay_fb: None, rev: None, riser_gain: None,
        pump: Some(Pump { depth: Some(0.6), release: Some(0.17) }),
        kit: &[
            ("kick", KitVoice { s: Some("kickHouse"), ..kv() }),
            ("clap", KitVoice { s: Some("clap"), rev: Some(0.3), ..kv() }),
            ("ohat", KitVoice { g: Some(0.9), ..kv() }),
            ("ride", KitVoice { g: Some(0.8), ..kv() }),
        ],
        prog: &[("a", "Gm7 Gm7 Ebmaj7 F"), ("c", "Ebmaj7 F Dm7 Gm7"), ("b", "Cm7 Dm7 Ebmaj7 F")],
        drums: &[
            ("intro", &[("kick", "x...x...x...x..."), ("hat", "..x...x...x...x.")]),
            ("full", &[("kick", "x...x...x...x..."), ("clap", "....x.......x..."), ("hat", "xo.oxo.oxo.oxo.o"), ("ohat", "..x...x...x...x.")]),
            ("full2", &[("kick", "x...x...x...x..."), ("clap", "....x.......x..."), ("hat", "xo.oxo.oxo.oxo.o"), ("ohat", "..x...x...x...x."), ("ride", "x...x...x...x..."), ("shaker", "oxoxoxoxoxoxoxox")]),
            ("brk", &[("hat", "..x...x...x...x."), ("clap", "....o.......o...")]),
            ("build", &[("kick", "x...x...x...x..."), ("clap", "....x.......x..."), ("hat", "x.x.x.x.x.x.x.x.")]),
        ],
        parts: &[
            Part { name: "bass", kind: PartKind::Bass, inst: p::PLUCK_BASS, lo: Some(43.0), ch: Channel { pump: true, ..ch() }, pat: &[("a", "..r...r...r...r."), ("b", "..rr..o...rr..o.")], ..PART },
            Part { name: "stab", kind: PartKind::Chord, inst: p::STAB, lo: Some(60.0), ch: Channel { pump: true, rev: Some(0.3), dly: Some(0.2), ..ch() }, pat: &[("s", "x..x..x...x..x..")], ..PART },
            Part { name: "pad", kind: PartKind::Chord, inst: p::SUPER_PAD, lo: Some(55.0), ch: Channel { pump: true, rev: Some(0.5), hp: Some(180.0), level: Some(0.85), ..ch() }, pat: &[("hold", "x---------------")], ..PART },
            Part {
                name: "bell", kind: PartKind::Mel, inst: p::BELL, res: Some(2), ch: Channel { rev: Some(0.4), dly: Some(0.45), pan: Some(0.15), ..ch() },
                pat: &[
                    ("hook", "G5 _ Bb5 _ . D6 _ C6   _ _ A5 _ F5 _ . .   F5 _ A5 _ . C6 _ Bb5   _ _ G5 _ D5 _ . ."),
                    ("hook2", "G5 _ _ _ Eb5 _ _ _   F5 _ _ _ D5 _ _ _   G5 _ Bb5 _ D6 _ C6 _   A5 _ _ _ . . . ."),
                ],
                ..PART
            },
            Part { name: "arp", kind: PartKind::Arp, inst: p::PLUCK, lo: Some(67.0), ch: Channel { rev: Some(0.25), dly: Some(0.4), pan: Some(-0.25), pump: true, ..ch() }, pat: &[("a", "0 1 2 3 0 1 2 3 0 1 2 3 0 1 2 3")], ..PART },
        ],
        sections: &[
            Section { bars: 16, prog: Some("a"), drums: Some("intro"), lp: Some([350.0, 16000.0]), riser: Some(4), p: &[("bass", "a")], ..SEC },
            Section { bars: 16, prog: Some("a"), drums: Some("full"), crash: true, p: &[("bass", "a"), ("stab", "s"), ("pad", "hold")], ..SEC },
            Section { bars: 16, prog: Some("c"), drums: Some("full2"), crash: true, fill: Some("snare"), p: &[("bass", "b"), ("stab", "s"), ("pad", "hold"), ("bell", "hook")], ..SEC },
            Section { bars: 16, prog: Some("b"), drums: Some("brk"), down: Some(2), p: &[("pad", "hold"), ("bell", "hook2")], ..SEC },
            Section { bars: 8, prog: Some("c"), drums: Some("build"), riser: Some(8), swell: true, fill: Some("roll"), gap: Some(2), p: &[("pad", "hold"), ("stab", "s")], ..SEC },
            Section { bars: 16, prog: Some("c"), drums: Some("full2"), crash: true, drop: true, p: &[("bass", "b"), ("stab", "s"), ("pad", "hold"), ("bell", "hook"), ("arp", "a")], ..SEC },
            Section { bars: 8, prog: Some("a"), drums: Some("intro"), lp: Some([16000.0, 400.0]), p: &[("bass", "a")], ..SEC },
        ],
    },
    Track {
        id: "chrome-heart", title: "Chrome Heart", style: "Darksynth", bpm: 118.0, swing: None, gain: Some(0.88),
        delay: None, delay_fb: None, rev: None, riser_gain: None,
        pump: Some(Pump { depth: Some(0.4), release: Some(0.15) }),
        kit: &[
            ("kick", KitVoice { s: Some("kickBoom"), g: Some(0.95), ..kv() }),
            ("snare", KitVoice { s: Some("snareGated"), rev: Some(0.35), ..kv() }),
            ("clap", KitVoice { s: Some("clapBig"), g: Some(0.8), rev: Some(0.4), ..kv() }),
            ("crash", KitVoice { g: Some(1.1), ..kv() }),
        ],
        prog: &[("a", "Cm Cm Ab Ab Fm Fm G G"), ("b", "Cm Db Cm Bb"), ("c", "Ab Bb Cm Cm Ab Bb G G")],
        drums: &[
            ("pulse", &[("kick", "x...x...x...x...")]),
            ("drive", &[("kick", "x...x...x...x..."), ("snare", "....X.......X..."), ("hat", "xoxoxoxoxoxoxoxo")]),
            ("big", &[("kick", "x...x...x...x..."), ("snare", "....X.......X..."), ("clap", "....x.......x..."), ("hat", "xoxoxoxoxoxoxoxo"), ("ohat", "..x...x...x...x.")]),
            ("half", &[("kick", "x.........x....."), ("snare", "........X......."), ("hat", "x.x.x.x.x.x.x.x.")]),
            ("build", &[("kick", "x...x...x...x..."), ("hat", "x.x.x.x.x.x.x.x.")]),
        ],
        parts: &[
            Part { name: "bass", kind: PartKind::Bass, inst: p::DARK_BASS, lo: Some(36.0), ch: Channel { drive: Some(3.2), drive_lp: Some(4200.0), pump: true, level: Some(0.19), ..ch() }, pat: &[("drive", "rrorrrorrrorrror"), ("pulse", "r.r.r.r.r.r.r.r.")], ..PART },
            Part { name: "pad", kind: PartKind::Chord, inst: p::DARK_PAD, lo: Some(55.0), ch: Channel { pump: true, rev: Some(0.55), hp: Some(200.0), ..ch() }, pat: &[("hold", "x---------------")], ..PART },
            Part { name: "stab", kind: PartKind::Chord, inst: p::HIT, lo: Some(48.0), ch: Channel { rev: Some(0.5), ..ch() }, pat: &[("hits", "X-......x-......")], ..PART },
            Part {
                name: "lead", kind: PartKind::Mel, inst: p::SAW_LEAD, res: Some(2), legato: true, ch: Channel { rev: Some(0.4), dly: Some(0.3), drive: Some(1.4), level: Some(0.75), ..ch() },
                pat: &[("hook", "C5 _ _ _ Eb5 _ _ _   D5 _ _ _ F5 _ _ _   G5 _ _ _ _ _ F5 Eb5   G5 _ _ _ C5 _ _ _
                 Ab5 _ _ _ G5 _ Eb5 _   F5 _ _ _ D5 _ Bb4 _   B4 _ _ _ D5 _ _ _   G5 _ _ _ F5 _ D5 _")],
                ..PART
            },
            Part { name: "dark", kind: PartKind::Mel, inst: Patch { oct: Some(-1.0), gain: Some(0.09), ..p::BRASS_LEAD }, res: Some(4), legato: true, ch: Channel { rev: Some(0.6), dly: Some(0.3), ..ch() }, pat: &[("m", "G4 _ _ _   Ab4 _ _ _   G4 _ Eb4 _   F4 _ _ _")], ..PART },
        ],
        sections: &[
            Section { bars: 8, prog: Some("b"), drums: Some("pulse"), lp: Some([300.0, 6000.0]), swell: true, p: &[("bass", "pulse"), ("pad", "hold")], ..SEC },
            Section { bars: 16, prog: Some("a"), drums: Some("drive"), crash: true, fill: Some("tom"), p: &[("bass", "drive"), ("pad", "hold")], ..SEC },
            Section { bars: 16, prog: Some("c"), drums: Some("big"), crash: true, drop: true, fill: Some("crash"), p: &[("bass", "drive"), ("pad", "hold"), ("lead", "hook"), ("stab", "hits")], ..SEC },
            Section { bars: 8, prog: Some("b"), drums: Some("half"), down: Some(2), p: &[("pad", "hold"), ("dark", "m")], ..SEC },
            Section { bars: 8, prog: Some("a"), drums: Some("build"), riser: Some(8), swell: true, fill: Some("roll"), gap: Some(2), p: &[("bass", "pulse"), ("pad", "hold")], ..SEC },
            Section { bars: 16, prog: Some("c"), drums: Some("big"), crash: true, drop: true, fill: Some("tom"), p: &[("bass", "drive"), ("pad", "hold"), ("lead", "hook"), ("stab", "hits")], ..SEC },
            Section { bars: 8, prog: Some("a"), drums: Some("drive"), crash: true, p: &[("bass", "drive"), ("pad", "hold"), ("dark", "m")], ..SEC },
            Section { bars: 8, prog: Some("b"), drums: Some("pulse"), lp: Some([8000.0, 300.0]), p: &[("bass", "pulse"), ("pad", "hold")], ..SEC },
        ],
    },
    Track {
        id: "afterburner", title: "Afterburner", style: "Breakbeat acid", bpm: 128.0, swing: None, gain: Some(1.0),
        delay: Some(0.75), delay_fb: None, rev: None, riser_gain: None, pump: None,
        kit: &[
            ("kick", KitVoice { s: Some("kickPunch"), ..kv() }),
            ("snare", KitVoice { s: Some("snareCrisp"), rev: Some(0.25), ..kv() }),
            ("clap", KitVoice { g: Some(0.8), ..kv() }),
        ],
        prog: &[("a", "Dm Dm Bb C"), ("b", "Gm Bb C A")],
        drums: &[
            ("hats", &[("hat", "x.x.x.x.x.x.x.x."), ("ohat", "..............x.")]),
            ("brk", &[("kick", "x.x.......x....."), ("snare", "....X..o.o..X..."), ("hat", "x.x.x.x.x.x.x.x."), ("ohat", "..............x.")]),
            ("brk2", &[("kick", "x.........x..x.."), ("snare", "....X..o.o..X..o"), ("hat", "xoxoxoxoxoxoxoxo"), ("clap", "............x...")]),
            ("half", &[("kick", "x..............."), ("snare", "........x......."), ("hat", "x.x.x.x.x.x.x.x.")]),
            ("build", &[("kick", "x...x...x...x..."), ("snare", "....x.......x..."), ("hat", "x.x.x.x.x.x.x.x.")]),
        ],
        parts: &[
            Part { name: "acid", kind: PartKind::Bass, inst: p::ACID, lo: Some(38.0), ch: Channel { drive: Some(2.2), drive_lp: Some(6000.0), dly: Some(0.15), level: Some(0.55), ..ch() }, pat: &[("a", "r.or.rOr.fr~o.rRr.or.rOr.sr~o.fF"), ("b", "rRr.o.rr.f~rO.rs")], ..PART },
            Part { name: "bass", kind: PartKind::Bass, inst: p::SUB, lo: Some(38.0), ch: ch(), pat: &[("a", "r-----r---r-----")], ..PART },
            Part { name: "stab", kind: PartKind::Chord, inst: p::STAB, lo: Some(62.0), ch: Channel { rev: Some(0.35), dly: Some(0.3), ..ch() }, pat: &[("s", "....x.......x...")], ..PART },
            Part { name: "pad", kind: PartKind::Chord, inst: p::WARM_PAD, lo: Some(57.0), ch: Channel { rev: Some(0.5), hp: Some(200.0), ..ch() }, pat: &[("hold", "x---------------")], ..PART },
            Part {
                name: "lead", kind: PartKind::Mel, inst: p::SQ_LEAD, res: Some(2), legato: true, ch: Channel { rev: Some(0.3), dly: Some(0.35), ..ch() },
                pat: &[
                    ("hook", "A5 _ _ _ F5 _ D5 _   E5 _ F5 _ E5 _ D5 _   D5 _ _ _ F5 _ Bb5 _   A5 _ _ _ G5 _ E5 _"),
                    ("hook2", "G5 _ _ _ Bb5 _ D6 _   C6 _ Bb5 _ A5 _ F5 _   G5 _ _ _ E5 _ C5 _   C#5 _ _ _ E5 _ A5 _"),
                ],
                ..PART
            },
        ],
        sections: &[
            Section { bars: 8, prog: Some("a"), drums: Some("hats"), lp: Some([500.0, 9000.0]), p: &[("acid", "a")], ..SEC },
            Section { bars: 16, prog: Some("a"), drums: Some("brk"), crash: true, fill: Some("snare"), p: &[("acid", "a"), ("bass", "a")], ..SEC },
            Section { bars: 16, prog: Some("b"), drums: Some("brk2"), fill: Some("tom"), p: &[("acid", "b"), ("bass", "a"), ("stab", "s")], ..SEC },
            Section { bars: 8, prog: Some("a"), drums: Some("half"), down: Some(2), p: &[("pad", "hold"), ("acid", "a")], ..SEC },
            Section { bars: 8, prog: Some("b"), drums: Some("build"), riser: Some(8), swell: true, fill: Some("roll"), gap: Some(2), p: &[("acid", "b"), ("pad", "hold")], ..SEC },
            Section { bars: 16, prog: Some("a"), drums: Some("brk"), crash: true, drop: true, p: &[("acid", "a"), ("bass", "a"), ("stab", "s"), ("lead", "hook")], ..SEC },
            Section { bars: 16, prog: Some("b"), drums: Some("brk2"), fill: Some("snare"), p: &[("acid", "b"), ("bass", "a"), ("stab", "s"), ("lead", "hook2")], ..SEC },
            Section { bars: 8, prog: Some("a"), drums: Some("hats"), crash: true, lp: Some([9000.0, 400.0]), p: &[("acid", "a")], ..SEC },
        ],
    },
];

/// Each level's own track; the playlist then moves on through the others.
pub const LEVEL_TRACK: &[(&str, &str)] = &[
    ("sierra", "midnight-run"), // sunset to midnight, mountains into the city
    ("coast", "seabright"),     // dawn on the coast road
    ("streets", "neon-rush"),   // a flat-out sprint through the neon grid
    ("desert", "mirage"),       // golden hour to moonrise on Route 66
    ("seaside", "afterburner"), // three laps of the raceway
    ("cruise", "interstate"),   // endless night freeway
];

pub const PLAYLIST: &[&str] = &[
    "midnight-run",
    "interstate",
    "chrome-heart",
    "seabright",
    "neon-rush",
    "afterburner",
    "mirage",
];

/// `TRACKS.find((t) => t.id === id)`.
pub fn track(id: &str) -> Option<&'static Track> {
    TRACKS.iter().find(|t| t.id == id)
}
