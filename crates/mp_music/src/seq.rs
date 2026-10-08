//! The sequencer (`tools/music-lab/seq.js`). Reads the song format
//! ([`Track`]) and turns each 16th into events, the way the game's Music.js
//! does it, but with no audio nodes: the engine plays the events
//! sample-accurately on the lab's instruments, and the tests can read them
//! directly (`parity/golden/music/events.json` holds a few runs).
//!
//! Compilation, voicing, fills and the per-hit humanising are Music.js's.
//! Lab extensions to the format, used by the generated tracks:
//!   section.auto  { 'part.param': [from, to] } ramps a patch parameter over
//!                 the section (techno's slow filter motion);
//!   T.lay         { lane or part: energy } mutes that lane or part while the
//!                 live energy (0..1) is below the value;
//!   bass 'n'      the next chord's root (a pickup into the change), 'N'
//!                 accented; the same note as 'r' when the chord stays.

use crate::json::Val;
use crate::track::{Pairs, Track, lookup, put};

fn pc_letter(c: u8) -> Option<i32> {
    Some(match c {
        b'C' => 0,
        b'D' => 2,
        b'E' => 4,
        b'F' => 5,
        b'G' => 7,
        b'A' => 9,
        b'B' => 11,
        _ => return None,
    })
}

fn pc_of(s: &str) -> i32 {
    let b = s.as_bytes();
    let mut p = b.first().and_then(|c| pc_letter(*c)).expect("a chord root");
    match b.get(1) {
        Some(b'#') => p += 1,
        Some(b'b') => p -= 1,
        _ => {}
    }
    (p + 12) % 12
}

/// `noteToMidi`: 'C4' is 60; `None` for anything but
/// `^([A-G])([#b]?)(-?\d)$`.
pub fn note_to_midi(tok: &str) -> Option<f64> {
    let b = tok.as_bytes();
    let letter = pc_letter(*b.first()?)?;
    let mut i = 1;
    let acc = match b.get(i) {
        Some(b'#') => {
            i += 1;
            1
        }
        Some(b'b') => {
            i += 1;
            -1
        }
        _ => 0,
    };
    let neg = b.get(i) == Some(&b'-');
    if neg {
        i += 1;
    }
    let digit = *b.get(i)?;
    if !digit.is_ascii_digit() || i + 1 != b.len() {
        return None;
    }
    let oct = (digit - b'0') as i32 * if neg { -1 } else { 1 };
    Some((12 * (oct + 1) + letter + acc) as f64)
}

/// `QUAL[quality]`: the intervals of a chord quality; `None` for an unknown
/// one (the caller falls back to the triad).
fn qual(q: &str) -> Option<&'static [i32]> {
    Some(match q {
        "" => &[0, 4, 7],
        "m" => &[0, 3, 7],
        "7" => &[0, 4, 7, 10],
        "m7" => &[0, 3, 7, 10],
        "maj7" => &[0, 4, 7, 11],
        "sus2" => &[0, 2, 7],
        "sus4" => &[0, 5, 7],
        "5" => &[0, 7, 12],
        "add9" => &[0, 4, 7, 14],
        "madd9" => &[0, 3, 7, 14],
        "m9" => &[0, 3, 7, 10, 14],
        "maj9" => &[0, 4, 7, 11, 14],
        "dim" => &[0, 3, 6],
        "6" => &[0, 4, 7, 9],
        "m6" => &[0, 3, 7, 9],
        "7sus4" => &[0, 5, 7, 10],
        "aug" => &[0, 4, 8],
        "9" => &[0, 4, 7, 10, 14],
        "m11" => &[0, 3, 7, 10, 14, 17],
        // lab: the half-diminished 7th on a scale's diminished degree (compose)
        "m7b5" => &[0, 3, 6, 10],
        _ => return None,
    })
}

/// A parsed chord name: root pitch class, intervals, the slash bass (the
/// root without one).
#[derive(Clone, Debug, PartialEq)]
pub struct Chord {
    pub name: String,
    pub root: i32,
    pub iv: Vec<i32>,
    pub bass: i32,
}

/// `parseChord`: 'Am7/E'. The head must start with a note letter
/// (`^([A-G][#b]?)(.*)$`); an unknown quality is the major triad.
pub fn parse_chord(name: &str) -> Chord {
    let mut parts = name.split('/');
    let head = parts.next().unwrap_or("");
    let slash = parts.next();
    let b = head.as_bytes();
    assert!(
        b.first().and_then(|c| pc_letter(*c)).is_some(),
        "bad chord {name}"
    );
    let n = if matches!(b.get(1), Some(b'#') | Some(b'b')) {
        2
    } else {
        1
    };
    let root = pc_of(&head[..n]);
    let iv = qual(&head[n..])
        .unwrap_or(qual("").expect("the triad"))
        .to_vec();
    Chord {
        name: name.to_owned(),
        root,
        iv,
        bass: match slash {
            Some(s) => pc_of(s),
            None => root,
        },
    }
}

/// Chord tones from lo up, in the inversion that moves least from the last.
pub fn voice(ch: &Chord, lo: f64, prev: Option<&[f64]>) -> Vec<f64> {
    let mut pcs: Vec<i32> = Vec::new();
    for i in &ch.iv {
        let pc = (ch.root + i) % 12;
        if !pcs.contains(&pc) {
            pcs.push(pc);
        }
    }
    let mut best = Vec::new();
    let mut best_score = f64::INFINITY;
    for r in 0..pcs.len() {
        let mut notes = Vec::new();
        let mut m = lo + ((pcs[r] as f64 - lo) % 12.0 + 12.0) % 12.0;
        for k in 0..pcs.len() {
            let pc = pcs[(r + k) % pcs.len()] as f64;
            if k > 0 {
                m += 1.0;
                while (m % 12.0 + 12.0) % 12.0 != pc {
                    m += 1.0;
                }
            }
            notes.push(m);
        }
        let score = match prev {
            Some(prev) if !prev.is_empty() => notes.iter().enumerate().fold(0.0, |a, (i, n)| {
                a + (n - prev.get(i).copied().unwrap_or(prev[prev.len() - 1])).abs()
            }),
            _ => notes[0] - lo,
        };
        if score < best_score {
            best_score = score;
            best = notes;
        }
    }
    best
}

/// `VEL[ch]`: a hit character's velocity (NaN for one that is not a hit, as
/// the JS's undefined would be).
fn vel_of(ch: char) -> f64 {
    match ch {
        'X' => 1.0,
        'x' => 0.8,
        'o' => 0.42,
        _ => f64::NAN,
    }
}

/// `BASS_DEG[d] ?? 0`.
fn bass_deg(d: char) -> i32 {
    match d {
        'r' => 0,
        'o' => 12,
        'f' => 7,
        'l' => -12,
        'u' => 19,
        _ => 0,
    }
}

/// One compiled step of a part's pattern. A bass step reads `deg`,
/// `accent`, `len`, `slide`; a chord hit `vel`, `len`; a `mel` token
/// `midi`, `len`, `accent`; an `arp` token `idx`, `len`, `accent`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Step {
    pub len: usize,
    pub accent: bool,
    pub deg: char,
    pub slide: bool,
    pub vel: f64,
    pub midi: f64,
    pub idx: f64,
}

fn strip_ws(s: &str) -> Vec<char> {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

fn compile_bass(s: &str) -> Vec<Option<Step>> {
    let s = strip_ws(s);
    let n = s.len();
    let mut out = vec![None; n];
    for i in 0..n {
        let c = s[i];
        if "-~.".contains(c) {
            continue;
        }
        let mut len = 1;
        let mut slide = false;
        while i + len < n && (s[i + len] == '-' || s[i + len] == '~') {
            if s[i + len] == '~' {
                slide = true;
            }
            len += 1;
        }
        let lower = c.to_ascii_lowercase();
        out[i] = Some(Step {
            deg: lower,
            accent: c != lower,
            len,
            slide,
            ..Default::default()
        });
    }
    out
}

fn compile_hits(s: &str) -> Vec<Option<Step>> {
    let s = strip_ws(s);
    let n = s.len();
    let mut out = vec![None; n];
    for i in 0..n {
        if s[i] != 'x' && s[i] != 'X' {
            continue;
        }
        let mut len = 1;
        while i + len < n && s[i + len] == '-' {
            len += 1;
        }
        out[i] = Some(Step {
            vel: if s[i] == 'X' { 1.0 } else { 0.8 },
            len,
            ..Default::default()
        });
    }
    out
}

fn compile_tokens(s: &str, res: usize, arp: bool) -> Vec<Option<Step>> {
    let toks: Vec<&str> = s.split_whitespace().collect();
    let n = toks.len() * res;
    let mut out = vec![None; n];
    for i in 0..toks.len() {
        let mut t = toks[i];
        if t == "_" || t == "." {
            continue;
        }
        let accent = t.ends_with('!');
        if accent {
            t = &t[..t.len() - 1];
        }
        let mut len = 1;
        while i + len < toks.len() && toks[i + len] == "_" {
            len += 1;
        }
        let mut step = Step {
            len: len * res,
            accent,
            ..Default::default()
        };
        if arp {
            step.idx = t.parse().unwrap_or(f64::NAN);
        } else {
            step.midi = note_to_midi(t).unwrap_or_else(|| panic!("bad note {t}"));
        }
        out[i * res] = Some(step);
    }
    out
}

/// A compiled drum lane: its voice and its steps.
#[derive(Clone, Debug, PartialEq)]
pub struct Lane {
    pub voice: String,
    pub steps: Vec<char>,
}

/// A compiled part: what the sequencer reads of it, and its patterns.
#[derive(Clone, Debug, PartialEq)]
pub struct CPart {
    pub kind: String,
    pub lo: Option<f64>,
    pub gate: Option<f64>,
    pub legato: bool,
    pub pats: Pairs<Vec<Option<Step>>>,
}

/// `compileTrack(T)`: progressions as chords per bar, drum lanes as steps,
/// part patterns as steps.
#[derive(Clone, Debug, PartialEq)]
pub struct Compiled {
    pub prog: Pairs<Vec<Vec<Chord>>>,
    pub drums: Pairs<Vec<Lane>>,
    pub parts: Pairs<CPart>,
}

pub fn compile_track(t: &Track) -> Compiled {
    let mut prog = Vec::new();
    for (k, v) in &t.prog {
        prog.push((
            k.clone(),
            v.split_whitespace()
                .map(|bar| bar.split(',').map(parse_chord).collect())
                .collect(),
        ));
    }
    let mut drums = Vec::new();
    for (k, lanes) in &t.drums {
        drums.push((
            k.clone(),
            lanes
                .iter()
                .map(|(v, s)| Lane {
                    voice: v.clone(),
                    steps: strip_ws(s),
                })
                .collect(),
        ));
    }
    let mut parts = Vec::new();
    for (name, p) in &t.parts {
        let mut pats = Vec::new();
        for (k, s) in &p.pat {
            let pat = if p.kind == "bass" {
                compile_bass(s)
            } else if p.kind == "chord" {
                compile_hits(s)
            } else {
                let res = p.res.filter(|r| *r != 0.0).unwrap_or(1.0) as usize;
                compile_tokens(s, res, p.kind == "arp")
            };
            pats.push((k.clone(), pat));
        }
        parts.push((
            name.clone(),
            CPart {
                kind: p.kind.clone(),
                lo: p.lo,
                gate: p.gate,
                legato: p.legato == Some(true),
                pats,
            },
        ));
    }
    Compiled { prog, drums, parts }
}

/// A drum fill over the last bar of a section: from which 16th, whether its
/// velocity ramps, and its lanes (16 steps each).
#[derive(Debug, PartialEq)]
pub struct Fill {
    pub name: &'static str,
    pub from: usize,
    pub ramp: bool,
    pub lanes: &'static [(&'static str, &'static str)],
}

pub const FILLS: &[Fill] = &[
    Fill {
        name: "snare",
        from: 8,
        ramp: false,
        lanes: &[("snare", "........x.xxXxXX"), ("kick", "x.......x.......")],
    },
    Fill {
        name: "tom",
        from: 8,
        ramp: false,
        lanes: &[
            ("tomH", "........x.x....."),
            ("tomM", "............x.x."),
            ("tomL", "..............xX"),
            ("kick", "x.......x......."),
        ],
    },
    Fill {
        name: "roll",
        from: 0,
        ramp: true,
        lanes: &[("snare", "x.x.x.x.xxxxxxxx"), ("kick", "x...x...x...x...")],
    },
    Fill {
        name: "dnb",
        from: 8,
        ramp: false,
        lanes: &[("snare", "........X.oXxoXX"), ("kick", "x.........x.....")],
    },
    Fill {
        name: "west",
        from: 8,
        ramp: false,
        lanes: &[
            ("tomL", "........x..x..x."),
            ("tomM", "..........x..x.."),
            ("snare", "..............xX"),
            ("kick", "x.......x......."),
        ],
    },
    Fill {
        name: "crash",
        from: 12,
        ramp: false,
        lanes: &[("snare", "............XXXX"), ("kick", "x.......x...x...")],
    },
    // Lab: a clap build for house, a hat-and-rim one for techno, a kick roll
    // for trance and psy, and a 2-step turnaround for garage.
    Fill {
        name: "clap",
        from: 8,
        ramp: false,
        lanes: &[("clap", "........x.x.xxxx"), ("kick", "x...x...x.......")],
    },
    Fill {
        name: "perc",
        from: 8,
        ramp: false,
        lanes: &[
            ("rim", "........x..x.x.x"),
            ("hat", "........xxxxxxxx"),
            ("kick", "x...x...x...x..."),
        ],
    },
    Fill {
        name: "kickroll",
        from: 0,
        ramp: true,
        lanes: &[("kick", "x...x...x.x.xxxx"), ("snare", "........x.x.xxxx")],
    },
    Fill {
        name: "skip",
        from: 8,
        ramp: false,
        lanes: &[("snare", "........x..x.xoX"), ("kick", "x.....x.x...x...")],
    },
    // Chicha: the timbalero's abanico (a roll on the high drum opening onto
    // the low one and the bell) into the chorus; the congas and güiro play on.
    Fill {
        name: "abanico",
        from: 8,
        ramp: false,
        lanes: &[
            ("timbaleH", "........xxxxxxx."),
            ("timbaleL", "...............X"),
            ("cowbell", "...............x"),
        ],
    },
];

/// `FILLS[name]`.
pub fn fill(name: &str) -> Option<&'static Fill> {
    FILLS.iter().find(|f| f.name == name)
}

/// The lanes a fill silences in the pattern under it.
pub const FILL_REPLACES: &[&str] = &[
    "snare", "clap", "hat", "ohat", "ride", "shaker", "rim", "snap", "tomL", "tomM", "tomH",
    "timbaleH", "timbaleL", "cascara",
];

pub const DRUM_LANES: &[&str] = &[
    "kick", "snare", "clap", "hat", "ohat", "ride", "crash", "revCrash", "shaker", "rim", "snap",
    "tomL", "tomM", "tomH", "boom", "cowbell",
    // The latin kit's (instruments KITS.latin).
    "congaO", "congaS", "tumba", "bongoH", "bongoL", "timbaleH", "timbaleL", "cascara", "guiroL",
    "guiroS", "clave",
];

pub fn is_drum_lane(name: &str) -> bool {
    DRUM_LANES.contains(&name)
}

/// An event of one 16th, shaped as the JS object literals (a `None` member
/// is a missing key; `glide_from` is `Some(None)` for the JS `null`).
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Drum {
        lane: String,
        vel: f64,
        dt: f64,
        /// The swell's reverse crash: how long before its end.
        len: Option<f64>,
    },
    Note {
        part: String,
        midis: Vec<f64>,
        vel: f64,
        dur: f64,
        glide_from: Option<Option<f64>>,
        accent: Option<bool>,
        dt: f64,
    },
    Section {
        sec: usize,
    },
    Lp {
        from: f64,
        to: f64,
        dur: f64,
    },
    Auto {
        target: String,
        from: f64,
        to: f64,
        dur: f64,
    },
    Riser {
        dur: f64,
    },
    Down {
        dur: f64,
    },
    End,
}

impl Event {
    pub fn to_val(&self) -> Val {
        match self {
            Event::Drum { lane, vel, dt, len } => Val::obj(vec![
                ("k", Val::str("drum")),
                ("lane", Val::str(lane)),
                ("vel", Val::num(*vel)),
                ("dt", Val::num(*dt)),
                ("len", Val::onum(*len)),
            ]),
            Event::Note {
                part,
                midis,
                vel,
                dur,
                glide_from,
                accent,
                dt,
            } => Val::obj(vec![
                ("k", Val::str("note")),
                ("part", Val::str(part)),
                (
                    "midis",
                    Some(Val::Arr(midis.iter().map(|m| Val::Num(*m)).collect())),
                ),
                ("vel", Val::num(*vel)),
                ("dur", Val::num(*dur)),
                (
                    "glideFrom",
                    glide_from.map(|g| g.map_or(Val::Null, Val::Num)),
                ),
                ("accent", Val::obool(*accent)),
                ("dt", Val::num(*dt)),
            ]),
            Event::Section { sec } => Val::obj(vec![
                ("k", Val::str("section")),
                ("sec", Val::num(*sec as f64)),
            ]),
            Event::Lp { from, to, dur } => Val::obj(vec![
                ("k", Val::str("lp")),
                ("from", Val::num(*from)),
                ("to", Val::num(*to)),
                ("dur", Val::num(*dur)),
            ]),
            Event::Auto {
                target,
                from,
                to,
                dur,
            } => Val::obj(vec![
                ("k", Val::str("auto")),
                ("target", Val::str(target)),
                ("from", Val::num(*from)),
                ("to", Val::num(*to)),
                ("dur", Val::num(*dur)),
            ]),
            Event::Riser { dur } => {
                Val::obj(vec![("k", Val::str("riser")), ("dur", Val::num(*dur))])
            }
            Event::Down { dur } => Val::obj(vec![("k", Val::str("down")), ("dur", Val::num(*dur))]),
            Event::End => Val::obj(vec![("k", Val::str("end"))]),
        }
    }
}

/// The play position: section, bar in it, 16th in the bar.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Pos {
    pub sec: usize,
    pub bar: u32,
    pub step: u32,
}

/// The identity of a compiled chord (the JS compares `voicing.chord !==
/// chord` by object identity: the same slot of the same progression is the
/// same chord, another slot with the same name is not).
type ChordId = (usize, usize, usize);

#[derive(Clone, Debug)]
struct Voicing {
    chord: ChordId,
    notes: Vec<f64>,
}

/// A part's last note (`this.last[name]`): a bass part reads `slide`, a mel
/// part `end`.
#[derive(Clone, Copy, Debug)]
struct Last {
    midi: f64,
    slide: bool,
    end: f64,
}

/// What `_part` keeps between steps.
#[derive(Default)]
struct Voices {
    voicing: Pairs<Voicing>,
    last: Pairs<Last>,
}

impl Voices {
    #[allow(clippy::too_many_arguments)]
    fn part(
        &mut self,
        name: &str,
        part: &CPart,
        e: &Step,
        chord: &Chord,
        chord_id: ChordId,
        chord_start: bool,
        dt: f64,
        next_chord: &Chord,
        clock: f64,
        step_dur: f64,
    ) -> Event {
        let dur = e.len as f64 * step_dur;
        if part.kind == "bass" {
            let lo = part.lo.unwrap_or(33.0);
            let root = lo + ((chord.bass as f64 - lo) % 12.0 + 12.0) % 12.0;
            let d = e.deg;
            // 'f' is the chord's own fifth (a diminished or augmented one too).
            let iv = if d == 't' {
                chord.iv[1]
            } else if d == 's' {
                chord.iv.get(3).copied().unwrap_or(10)
            } else if d == 'f' {
                chord
                    .iv
                    .iter()
                    .copied()
                    .find(|x| (6..=8).contains(x))
                    .unwrap_or(7)
            } else {
                bass_deg(d)
            };
            let midi = if d == 'n' {
                lo + ((next_chord.bass as f64 - lo) % 12.0 + 12.0) % 12.0
            } else {
                root + iv as f64
            };
            let prev = lookup(&self.last, name);
            let glide_from = match prev {
                Some(p) if p.slide => Some(p.midi),
                _ => None,
            };
            put(
                &mut self.last,
                name,
                Last {
                    midi,
                    slide: e.slide,
                    end: 0.0,
                },
            );
            return Event::Note {
                part: name.to_owned(),
                midis: vec![midi],
                vel: if e.accent { 1.0 } else { 0.82 },
                dur: dur
                    * if e.slide {
                        1.05
                    } else {
                        part.gate.unwrap_or(0.9)
                    },
                glide_from: Some(glide_from),
                accent: Some(e.accent),
                dt,
            };
        }
        if part.kind == "chord" {
            let cur = lookup(&self.voicing, name);
            if chord_start || cur.is_none_or(|v| v.chord != chord_id) {
                let notes = voice(
                    chord,
                    part.lo.unwrap_or(55.0),
                    cur.map(|v| v.notes.as_slice()),
                );
                put(
                    &mut self.voicing,
                    name,
                    Voicing {
                        chord: chord_id,
                        notes,
                    },
                );
            }
            let v = lookup(&self.voicing, name).expect("the voicing just set");
            return Event::Note {
                part: name.to_owned(),
                midis: v.notes.clone(),
                vel: e.vel,
                dur: dur * part.gate.unwrap_or(1.0),
                glide_from: None,
                accent: None,
                dt,
            };
        }
        if part.kind == "arp" {
            let v = voice(chord, part.lo.unwrap_or(60.0), None);
            let k = v.len() as f64;
            let i = e.idx;
            let midi = v[(((i % k) + k) % k) as usize] + 12.0 * (i / k).floor();
            return Event::Note {
                part: name.to_owned(),
                midis: vec![midi],
                vel: if e.accent { 1.0 } else { 0.8 },
                dur: dur * part.gate.unwrap_or(0.8),
                glide_from: None,
                accent: None,
                dt,
            };
        }
        let prev = lookup(&self.last, name);
        let now = clock;
        let glide_from = match prev {
            Some(p) if part.legato && p.end > now - 0.01 => Some(p.midi),
            _ => None,
        };
        put(
            &mut self.last,
            name,
            Last {
                midi: e.midi,
                slide: false,
                end: now + dur,
            },
        );
        Event::Note {
            part: name.to_owned(),
            midis: vec![e.midi],
            vel: if e.accent { 1.0 } else { 0.85 },
            dur: dur * part.gate.unwrap_or(0.95),
            glide_from: Some(glide_from),
            accent: None,
            dt,
        }
    }
}

/// The sequencer: owns a track and its compiled form, and steps through it
/// one 16th at a time.
pub struct Seq {
    t: Track,
    c: Compiled,
    /// Seconds per 16th.
    pub step_dur: f64,
    voices: Voices,
    pub pos: Pos,
    /// The live energy (0..1): lanes and parts under their `lay` are silent.
    pub energy: f64,
    /// Steps played so far (the clock `_part` reads for legato).
    n: u64,
    clock: f64,
    /// Muted lanes and parts; 'drums' mutes every drum lane.
    pub mute: Vec<String>,
    /// A soloed lane or part; 'drums' solos every drum lane.
    pub solo: Option<String>,
    /// Total bars of the song.
    pub bars: u32,
}

impl Seq {
    pub fn new(t: Track) -> Seq {
        let c = compile_track(&t);
        let step_dur = 60.0 / t.bpm / 4.0;
        let bars = t.sections.iter().map(|s| s.bars).sum();
        Seq {
            t,
            c,
            step_dur,
            voices: Voices::default(),
            pos: Pos::default(),
            energy: 1.0,
            n: 0,
            clock: 0.0,
            mute: Vec::new(),
            solo: None,
            bars,
        }
    }

    pub fn track(&self) -> &Track {
        &self.t
    }

    pub fn compiled(&self) -> &Compiled {
        &self.c
    }

    /// Moves to the start of a bar of the song (clamped into the last
    /// section).
    pub fn seek_bar(&mut self, bar: u32) {
        let t = &self.t;
        self.pos = Pos::default();
        let mut b = bar;
        while b > 0 && self.pos.sec < t.sections.len() - 1 && b >= t.sections[self.pos.sec].bars {
            b -= t.sections[self.pos.sec].bars;
            self.pos.sec += 1;
        }
        self.pos.bar = b.min(t.sections[self.pos.sec].bars - 1);
        self.voices.last.clear();
    }

    /// The bar of the song the position is in.
    pub fn bar_index(&self) -> u32 {
        let mut b = self.pos.bar;
        for i in 0..self.pos.sec {
            b += self.t.sections[i].bars;
        }
        b
    }

    fn on(&self, name: &str) -> bool {
        if let Some(solo) = &self.solo
            && solo != name
            && !(solo == "drums" && is_drum_lane(name))
        {
            return false;
        }
        if self.mute.iter().any(|m| m == name)
            || (is_drum_lane(name) && self.mute.iter().any(|m| m == "drums"))
        {
            return false;
        }
        match lookup(&self.t.lay, name) {
            None => true,
            Some(lay) => self.energy >= *lay,
        }
    }

    /// The per-hit humanising: a velocity factor around 1 from the step and
    /// the lane. The JS sum is a double then ToUint32; `h ^= h >>> 15` makes
    /// an int32, and `%` keeps its sign, so the factor may dip under 0.94.
    fn human(&self, idx: u32, voice_name: &str) -> f64 {
        let h = (idx as u64 * 2654435761
            + voice_name.len() as u64 * 97
            + self.pos.sec as u64 * 131
            + self.pos.bar as u64 * 17)
            % 4294967296;
        let h = h as u32;
        let h = (h as i32) ^ ((h >> 15) as i32);
        0.94 + ((h % 1000) as f64 / 1000.0) * 0.12
    }

    /// The events of the current 16th, then advance. Each event's `dt` is its
    /// offset from the step's grid time, in seconds (swing).
    pub fn step(&mut self) -> Vec<Event> {
        let mut ev = Vec::new();
        let sec_i = self.pos.sec;
        let st = self.pos.step;
        let bar = self.pos.bar;
        let sec = &self.t.sections[sec_i];
        let sec_bars = sec.bars;
        let dt = if st % 2 == 1 {
            self.t.swing.unwrap_or(0.0) * self.step_dur
        } else {
            0.0
        };
        self.clock = self.n as f64 * self.step_dur + dt;
        self.n += 1;
        let bar_dur = self.step_dur * 16.0;
        let last_bar = bar == sec.bars - 1;
        if st == 0 {
            self.bar_start(&mut ev, sec_i, bar, bar_dur);
        }
        let sec = &self.t.sections[sec_i];
        let gap = last_bar && sec.gap.is_some_and(|g| g != 0.0 && st as f64 >= 16.0 - g);
        if !gap {
            let c = &self.c;
            let prog_key = sec.prog.as_deref().filter(|p| !p.is_empty()).unwrap_or("a");
            let (pi, prog) = c
                .prog
                .iter()
                .enumerate()
                .find(|(_, (k, _))| k == prog_key)
                .map(|(i, (_, p))| (i, p))
                .unwrap_or_else(|| panic!("no progression {prog_key}"));
            let bi = bar as usize % prog.len();
            let pbar = &prog[bi];
            let ci = (st as usize * pbar.len()) / 16;
            let chord = &pbar[ci];
            let chord_id = (pi, bi, ci);
            let chord_start = (st as usize * pbar.len()).is_multiple_of(16);
            // The chord after this one (for bass pickups): later in the bar, else
            // the next bar's first; at the section's end, the section's own loop.
            let next_chord = if ci + 1 < pbar.len() {
                &pbar[ci + 1]
            } else {
                &prog[(bar as usize + 1) % prog.len()][0]
            };
            let idx = bar * 16 + st;
            let fill = if last_bar {
                sec.fill.as_deref().and_then(fill)
            } else {
                None
            };
            let in_fill = fill.is_some_and(|f| st as usize >= f.from);
            let lanes = sec
                .drums
                .as_deref()
                .filter(|d| !d.is_empty())
                .and_then(|d| lookup(&c.drums, d));
            if let Some(lanes) = lanes {
                for l in lanes {
                    if in_fill {
                        let f = fill.expect("a fill");
                        if FILL_REPLACES.contains(&l.voice.as_str())
                            || (l.voice == "kick" && f.lanes.iter().any(|(v, _)| *v == "kick"))
                        {
                            continue;
                        }
                    }
                    if !self.on(&l.voice) || l.steps.is_empty() {
                        continue;
                    }
                    let ch = l.steps[idx as usize % l.steps.len()];
                    if ch != '.' {
                        ev.push(Event::Drum {
                            lane: l.voice.clone(),
                            vel: vel_of(ch) * self.human(idx, &l.voice),
                            dt,
                            len: None,
                        });
                    }
                }
            }
            if in_fill {
                let f = fill.expect("a fill");
                for (voice, steps) in f.lanes {
                    let ch = steps.as_bytes()[st as usize] as char;
                    if ch == '.' || !self.on(voice) {
                        continue;
                    }
                    let ramp = if f.ramp {
                        0.35 + 0.65 * (st as f64 / 15.0)
                    } else {
                        1.0
                    };
                    ev.push(Event::Drum {
                        lane: (*voice).to_owned(),
                        vel: vel_of(ch) * ramp,
                        dt,
                        len: None,
                    });
                }
            }
            for (name, key) in &sec.p {
                if !self.on(name) {
                    continue;
                }
                let Some(part) = lookup(&c.parts, name) else {
                    continue;
                };
                if key.is_empty() {
                    continue;
                }
                let Some(pat) = lookup(&part.pats, key) else {
                    continue;
                };
                if pat.is_empty() {
                    continue;
                }
                let Some(e) = &pat[(bar as usize * 16 + st as usize) % pat.len()] else {
                    continue;
                };
                let n = self.voices.part(
                    name,
                    part,
                    e,
                    chord,
                    chord_id,
                    chord_start,
                    dt,
                    next_chord,
                    self.clock,
                    self.step_dur,
                );
                ev.push(n);
            }
        }
        // Advance.
        self.pos.step += 1;
        if self.pos.step == 16 {
            self.pos.step = 0;
            self.pos.bar += 1;
            if self.pos.bar >= sec_bars {
                self.pos.bar = 0;
                self.pos.sec += 1;
                if self.pos.sec >= self.t.sections.len() {
                    self.pos.sec = 0;
                    ev.push(Event::End);
                }
            }
        }
        ev
    }

    fn bar_start(&self, ev: &mut Vec<Event>, sec_i: usize, bar: u32, bar_dur: f64) {
        let sec = &self.t.sections[sec_i];
        let sec_dur = bar_dur * sec.bars as f64;
        if bar == 0 {
            ev.push(Event::Section { sec: self.pos.sec });
            if sec.crash == Some(true) && self.on("crash") {
                ev.push(Event::Drum {
                    lane: "crash".to_owned(),
                    vel: 1.0,
                    dt: 0.0,
                    len: None,
                });
            }
            if sec.drop == Some(true) && self.on("boom") {
                ev.push(Event::Drum {
                    lane: "boom".to_owned(),
                    vel: 1.0,
                    dt: 0.0,
                    len: None,
                });
            }
            ev.push(match sec.lp {
                Some(lp) => Event::Lp {
                    from: lp[0],
                    to: lp[1],
                    dur: sec_dur,
                },
                None => Event::Lp {
                    from: 20000.0,
                    to: 20000.0,
                    dur: 0.0,
                },
            });
            for (target, [from, to]) in sec.auto.iter().flatten() {
                ev.push(Event::Auto {
                    target: target.clone(),
                    from: *from,
                    to: *to,
                    dur: sec_dur,
                });
            }
        }
        let left = sec.bars - bar;
        if let Some(riser) = sec.riser.filter(|r| *r != 0.0)
            && left as f64 == riser
        {
            ev.push(Event::Riser {
                dur: bar_dur * riser,
            });
        }
        if let Some(down) = sec.down.filter(|d| *d != 0.0)
            && bar == 0
        {
            ev.push(Event::Down {
                dur: bar_dur * down,
            });
        }
        // The reverse crash ends on the next section's downbeat.
        if sec.swell == Some(true) && left == 1 && self.on("revCrash") {
            ev.push(Event::Drum {
                lane: "revCrash".to_owned(),
                vel: 0.9,
                dt: 0.0,
                len: Some(bar_dur),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_and_chords_parse() {
        assert_eq!(note_to_midi("C4"), Some(60.0));
        assert_eq!(note_to_midi("A-1"), Some(9.0));
        assert_eq!(note_to_midi("Bb3"), Some(58.0));
        assert_eq!(note_to_midi("C#-1"), Some(1.0));
        assert_eq!(note_to_midi("H4"), None);
        assert_eq!(note_to_midi("C44"), None);
        assert_eq!(note_to_midi("C4!"), None);
        let c = parse_chord("Am7/E");
        assert_eq!((c.root, c.bass), (9, 4));
        assert_eq!(c.iv, vec![0, 3, 7, 10]);
        assert_eq!(parse_chord("Bbmaj9").iv, vec![0, 4, 7, 11, 14]);
        assert_eq!(parse_chord("Cweird").iv, vec![0, 4, 7]);
        assert_eq!(parse_chord("E7").root, 4);
    }

    #[test]
    fn voicing_moves_least() {
        let am = parse_chord("Am");
        let v = voice(&am, 55.0, None);
        assert_eq!(v, vec![57.0, 60.0, 64.0]);
        let f = parse_chord("F");
        let w = voice(&f, 55.0, Some(&v));
        // F A C around A C E: the inversion A C F moves least.
        assert_eq!(w, vec![57.0, 60.0, 65.0]);
    }
}
