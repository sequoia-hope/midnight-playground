//! The composer (`tools/music-lab/compose.js`, docs/vision/sound.md 3.4).
//! What the genre grammars in [`crate::genres`] share, and what makes a
//! generated track hold together where a pile of random steps does not:
//!
//! - keys and chords: a scale, diatonic chords on its degrees with the
//!   extensions a genre wants, progressions as degree lists;
//! - rhythm: Euclidean patterns and per-beat cells with metric accents, so
//!   a lane has a shape instead of a coin flip per step;
//! - motifs: a melodic idea is a rhythm plus a contour (intervals in scale
//!   steps). Realising it over a chord snaps the strong notes to chord
//!   tones, so the same figure follows the harmony: that is what makes a
//!   hook recognisable when it comes back over a different chord. Phrases
//!   are question and answer (open ending, then closed on the root), and a
//!   sparse version of the hook keeps the shape with half the notes, for
//!   breakdowns;
//! - bass: a lane syntax with 'n', a pickup into the next chord's root, so
//!   the bass leads the harmony instead of trailing it;
//! - déjà vu: variations are made from a core once; each 8-bar block plays
//!   the core (probability dejavu) or one of its variations, so the loop
//!   returns to its identity instead of drifting (Marbles locks, it does
//!   not walk);
//! - arrangement: long sections are split into 8-bar blocks that pick their
//!   variations, and the seams (crash, downlifter, riser, fill, gap) follow
//!   the tension curve.
//!
//! Every draw happens in the lab's order: the goldens (`parity/golden/
//! music/tracks.json`) hash the tracks the grammars make from these
//! helpers, so a draw moved or skipped is a different track.

use crate::seq::parse_chord;
use crate::track::{Section, lookup, put};
use crate::{imul, js_round, to_u32};

pub const NOTE: [&str; 12] = [
    "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];

// ── Seeded choices ───────────────────────────────────────────────

/// mulberry32 (`dsp.js` `rng`): the lab's one random stream, 32-bit state.
/// The JS `t + Math.imul(...)` is a double sum then ToInt32, which is the
/// wrapping sum of the two bit patterns.
struct Rng {
    a: u32,
}

impl Rng {
    fn new(seed: u32) -> Rng {
        Rng { a: seed }
    }

    fn next(&mut self) -> f64 {
        self.a = self.a.wrapping_add(0x6d2b79f5);
        let mut t = self.a;
        t = imul(t ^ (t >> 15), t | 1);
        t ^= t.wrapping_add(imul(t ^ (t >> 7), t | 61));
        (t ^ (t >> 14)) as f64 / 4294967296.0
    }
}

/// The lab's `R(seed, salt)`: a seeded stream with the choices the grammars
/// make. `salt` keeps one seed from drawing the same key and tempo in every
/// genre (each grammar's first draws are those).
pub struct R {
    /// The unsalted seed, which `fork` derives its children from.
    seed: u32,
    r: Rng,
}

impl R {
    /// `R(seed, salt)`; `R(seed)` is `R::new(seed, 0)`.
    pub fn new(seed: u32, salt: u32) -> R {
        let s = if salt != 0 {
            let s = imul(to_u32(seed as f64 + salt as f64 * 104729.0), 2654435761u32);
            if s == 0 { 1 } else { s }
        } else {
            seed
        };
        R {
            seed,
            r: Rng::new(s),
        }
    }

    /// The raw draw in [0, 1).
    pub fn f(&mut self) -> f64 {
        self.r.next()
    }

    /// An integer in `a..=b`.
    pub fn int(&mut self, a: i32, b: i32) -> i32 {
        a + (self.f() * (b - a + 1) as f64).floor() as i32
    }

    /// One of `xs`.
    pub fn pick<T: Clone>(&mut self, xs: &[T]) -> T {
        xs[(self.f() * xs.len() as f64).floor() as usize].clone()
    }

    pub fn chance(&mut self, p: f64) -> bool {
        self.f() < p
    }

    /// A value by weight (`[[value, weight], ...]`).
    pub fn weighted<T: Clone>(&mut self, pairs: &[(T, f64)]) -> T {
        let mut t = 0.0;
        for (_, w) in pairs {
            t += w;
        }
        let mut u = self.f() * t;
        for (v, w) in pairs {
            u -= w;
            if u < 0.0 {
                return v.clone();
            }
        }
        pairs[0].0.clone()
    }

    /// Fisher-Yates, as written.
    pub fn shuffle<T: Clone>(&mut self, xs: &[T]) -> Vec<T> {
        let mut a = xs.to_vec();
        let mut i = a.len();
        while i > 1 {
            i -= 1;
            let j = (self.f() * (i + 1) as f64).floor() as usize;
            a.swap(i, j);
        }
        a
    }

    /// A seeded child, so one choice does not shift every later draw. The
    /// JS sum is a double before ToUint32.
    pub fn fork(&self, k: u32) -> R {
        let s = (self.seed as u64 * 7919 + k as u64 * 104729) % 4294967296;
        R::new(if s == 0 { 1 } else { s as u32 }, 0)
    }
}

// ── Keys and chords ──────────────────────────────────────────────

/// A scale: its lab name (`scaleName` finds the key of `SCALES` by
/// identity, so the name travels with the semitones) and its degrees.
#[derive(Debug, PartialEq)]
pub struct Scale {
    pub name: &'static str,
    pub semis: [i32; 7],
}

pub const MINOR: &Scale = &Scale {
    name: "minor",
    semis: [0, 2, 3, 5, 7, 8, 10],
};
pub const DORIAN: &Scale = &Scale {
    name: "dorian",
    semis: [0, 2, 3, 5, 7, 9, 10],
};
pub const PHRYGIAN: &Scale = &Scale {
    name: "phrygian",
    semis: [0, 1, 3, 5, 7, 8, 10],
};
pub const HARMONIC_MINOR: &Scale = &Scale {
    name: "harmonicMinor",
    semis: [0, 2, 3, 5, 7, 8, 11],
};
pub const MAJOR: &Scale = &Scale {
    name: "major",
    semis: [0, 2, 4, 5, 7, 9, 11],
};
pub const MIXOLYDIAN: &Scale = &Scale {
    name: "mixolydian",
    semis: [0, 2, 4, 5, 7, 9, 10],
};
/// `SCALES`, in the lab's order.
pub const SCALES: [&Scale; 6] = [MINOR, DORIAN, PHRYGIAN, HARMONIC_MINOR, MAJOR, MIXOLYDIAN];
pub const MINOR_PENTA: [i32; 5] = [0, 3, 5, 7, 10];

/// Semitones above the tonic of scale degree k (any integer; 7 = the octave).
pub fn deg_semi(scale: &Scale, k: i32) -> i32 {
    scale.semis[k.rem_euclid(7) as usize] + 12 * k.div_euclid(7)
}

/// The diatonic chord on degree d, named as `seq` parses it. `ext`: '' triad,
/// '7', '9', 'add9', 'sus2', 'sus4', '6'. Non-diatonic colour is the caller's
/// business (a genre may write chord names directly).
pub fn chord_on(scale: &Scale, tonic: i32, d: i32, ext: &str) -> String {
    let root = ((tonic + deg_semi(scale, d)) % 12) as usize;
    // 'dom': the dominant seventh on the degree's root whatever the scale says
    // (the V7 of a minor key: cumbia, country, the classical cadence).
    if ext == "dom" {
        return format!("{}7", NOTE[root]);
    }
    let third = deg_semi(scale, d + 2) - deg_semi(scale, d);
    let fifth = deg_semi(scale, d + 4) - deg_semi(scale, d);
    let seventh = deg_semi(scale, d + 6) - deg_semi(scale, d);
    let sixth = deg_semi(scale, d + 5) - deg_semi(scale, d);
    let q = if ext == "sus2" || ext == "sus4" {
        ext
    } else if fifth == 6 {
        if ext == "7" || ext == "9" {
            "m7b5"
        } else {
            "dim"
        }
    } else if third == 3 {
        match ext {
            "7" => "m7",
            "9" => "m9",
            "add9" => "madd9",
            "6" if sixth == 9 => "m6",
            "6" => "m7",
            _ => "m",
        }
    } else {
        match ext {
            "7" => {
                if seventh == 11 {
                    "maj7"
                } else {
                    "7"
                }
            }
            "9" => {
                if seventh == 11 {
                    "maj9"
                } else {
                    "9"
                }
            }
            "add9" => "add9",
            "6" => "6",
            _ => "",
        }
    };
    format!("{}{}", NOTE[root], q)
}

/// A progression item: a degree, a degree with its own extension
/// (`[4, 'dom']`), or several items in one bar (joined with commas, the
/// format's two-chords-a-bar).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Item {
    Deg(i32),
    Ext(i32, &'static str),
    Split(&'static [Item]),
}

use Item::{Deg as D, Ext as X};

/// A progression: one chord per bar from degree items.
pub fn progression(scale: &Scale, tonic: i32, bars: &[Item], ext: &str) -> String {
    let one = |it: &Item| match it {
        Item::Ext(d, e) => chord_on(scale, tonic, *d, e),
        Item::Deg(d) => chord_on(scale, tonic, *d, ext),
        Item::Split(_) => panic!("a split bar inside a split bar"),
    };
    bars.iter()
        .map(|b| match b {
            Item::Split(items) => items.iter().map(one).collect::<Vec<_>>().join(","),
            b => one(b),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Progression families, as degree lists (0 = i). Minor-key dance music lives
/// on a few of these; the names are what producers call them.
pub struct Progs {
    /// The "axis" loops that carry trance, eurobeat choruses, big-room house.
    pub anthem: &'static [&'static [Item]],
    /// Verses that sit still and wait for the chorus.
    pub verse: &'static [&'static [Item]],
    /// Deep house and liquid: two-chord vamps and 7th/9th colour. Some of
    /// these land on a scale's diminished degree (5 in dorian, 1 in minor):
    /// pick through [`no_dim`], which drops those for the scale in hand.
    pub deep: &'static [&'static [Item]],
    /// Techno and psy: one chord, or a step away and back.
    pub drone: &'static [&'static [Item]],
    /// Major keys for the brightest choruses.
    pub bright: &'static [&'static [Item]],
    /// Cumbia and chicha, in minor: i, iv and the dominant V7 ([4, 'dom']),
    /// the VII and VI of the Andean side; two-chord vamps mostly.
    pub cumbia: &'static [&'static [Item]],
}

pub const PROGS: Progs = Progs {
    anthem: &[
        &[D(0), D(5), D(2), D(6)],
        &[D(0), D(6), D(5), D(6)],
        &[D(0), D(5), D(6), D(0)],
        &[D(5), D(6), D(0), D(0)],
        &[D(0), D(3), D(5), D(6)],
        &[D(0), D(2), D(6), D(5)],
        &[D(0), D(4), D(5), D(3)],
        &[D(5), D(3), D(0), D(6)],
    ],
    verse: &[
        &[D(0), D(0), D(5), D(5)],
        &[D(0), D(0), D(6), D(6)],
        &[D(0), D(5), D(0), D(5)],
        &[D(0), D(6), D(0), D(6)],
        &[D(0), D(0), D(3), D(3)],
        &[D(0), D(0), D(0), D(6)],
    ],
    deep: &[
        &[D(0), D(0), D(3), D(3)],
        &[D(0), D(3), D(0), D(4)],
        &[D(0), D(5), D(2), D(6)],
        &[D(0), D(0), D(5), D(6)],
        &[D(3), D(4), D(0), D(0)],
        &[D(0), D(6), D(5), D(4)],
        &[D(0), D(1), D(0), D(1)],
        &[D(0), D(0), D(1), D(3)],
        &[D(0), D(3), D(6), D(0)],
        &[D(0), D(1), D(3), D(4)],
    ],
    drone: &[
        &[D(0), D(0), D(0), D(0)],
        &[D(0), D(0), D(0), D(6)],
        &[D(0), D(0), D(1), D(0)],
        &[D(0), D(0), D(0), D(5)],
        &[D(0), D(1), D(0), D(1)],
    ],
    bright: &[
        &[D(0), D(4), D(5), D(3)],
        &[D(5), D(3), D(0), D(4)],
        &[D(0), D(5), D(3), D(4)],
        &[D(3), D(4), D(5), D(5)],
        &[D(0), D(3), D(5), D(4)],
    ],
    cumbia: &[
        &[D(0), D(0), D(3), D(3)],
        &[D(0), D(3), X(4, "dom"), D(0)],
        &[D(0), D(6), D(0), D(6)],
        &[D(0), D(0), X(4, "dom"), X(4, "dom")],
        &[D(0), D(3), D(0), X(4, "dom")],
        &[D(3), X(4, "dom"), D(0), D(0)],
        &[D(0), X(4, "dom"), D(0), X(4, "dom")],
        &[D(5), D(6), D(0), D(0)],
    ],
};

/// Whether the diatonic chord on degree d of a scale is diminished (a
/// tritone for a fifth: ii° in minor, vi° in dorian, vii° in major).
pub fn is_dim(scale: &Scale, d: i32) -> bool {
    deg_semi(scale, d + 4) - deg_semi(scale, d) == 6
}

/// The progressions of a family that stay off the scale's diminished
/// degree: a half-diminished chord is not a vamp anyone dances to. (A
/// split bar is never diminished, as the JS reads it: `it[0]` of an array
/// of arrays is no degree.)
pub fn no_dim(scale: &Scale, progs: &[&'static [Item]]) -> Vec<&'static [Item]> {
    progs
        .iter()
        .copied()
        .filter(|p| {
            p.iter().all(|it| match it {
                Item::Deg(d) | Item::Ext(d, _) => !is_dim(scale, *d),
                Item::Split(_) => true,
            })
        })
        .collect()
}

/// Pitch class set of a chord name, and the chord's root pitch class.
#[derive(Clone, Debug, PartialEq)]
pub struct ChordPcs {
    pub root: i32,
    pub pcs: Vec<i32>,
    pub bass: i32,
}

pub fn chord_pcs(name: &str) -> ChordPcs {
    let c = parse_chord(name);
    ChordPcs {
        root: c.root,
        pcs: c.iv.iter().map(|i| (c.root + i) % 12).collect(),
        bass: c.bass,
    }
}

// ── Rhythm ───────────────────────────────────────────────────────

/// A lane of n steps from a function of the step index.
pub fn lane<S: AsRef<str>>(n: usize, mut f: impl FnMut(usize) -> S) -> String {
    let mut s = String::new();
    for i in 0..n {
        s.push_str(f(i).as_ref());
    }
    s
}

/// n bars of 16 steps from a function of the bar index.
pub fn bars<S: AsRef<str>>(n: usize, mut f: impl FnMut(usize) -> S) -> String {
    let mut s = String::new();
    for b in 0..n {
        s.push_str(f(b).as_ref());
    }
    s
}

/// Repeats a one-bar lane over n bars, with a different last bar (the JS
/// `loop`; a keyword here).
pub fn loop_(n: usize, one: &str, last: &str) -> String {
    bars(n, |b| if b == n - 1 { last } else { one })
}

/// Bjorklund's algorithm: k hits as evenly spread over n steps as possible
/// (E(3,8) is the tresillo, E(5,16) the "x..x..x." family), rotated.
pub fn euclid(k: i32, n: usize, rot: i32, hit: char) -> String {
    if k <= 0 {
        return ".".repeat(n);
    }
    if k as usize >= n {
        return hit.to_string().repeat(n);
    }
    let k = k as usize;
    let mut a: Vec<Vec<u8>> = (0..k).map(|_| vec![1]).collect();
    let mut b: Vec<Vec<u8>> = (0..n - k).map(|_| vec![0]).collect();
    while b.len() > 1 {
        let m = a.len().min(b.len());
        let mut next = Vec::new();
        for i in 0..m {
            let mut x = a[i].clone();
            x.extend_from_slice(&b[i]);
            next.push(x);
        }
        let rest = if a.len() > m {
            a[m..].to_vec()
        } else {
            b[m..].to_vec()
        };
        a = next;
        b = rest;
    }
    a.extend(b);
    let s: String = a
        .iter()
        .flatten()
        .map(|v| if *v != 0 { hit } else { '.' })
        .collect();
    let rr = rot.rem_euclid(n as i32) as usize;
    format!("{}{}", &s[n - rr..], &s[..n - rr])
}

/// Metric weight of a 16th: downbeat, beats, off-beat 8ths, then the rest.
pub fn weight(i: usize) -> f64 {
    if i.is_multiple_of(16) {
        1.0
    } else if i.is_multiple_of(4) {
        0.8
    } else if i.is_multiple_of(2) {
        0.6
    } else {
        0.4
    }
}

/// A hit character by metric weight: accents on the beats.
pub fn accent(i: usize, strong: f64) -> char {
    if weight(i) >= strong { 'x' } else { 'o' }
}

/// A bar from per-beat cells: `table` is [[cell, weight]...] of 4-char cells.
/// The same cell set every beat keeps the pattern one idea; `last` is the
/// cell set for beat 4 (the turnaround), if different.
pub fn cells(r: &mut R, table: &[(&str, f64)], last: Option<&[(&str, f64)]>) -> String {
    let last = last.unwrap_or(table);
    let mut s = String::new();
    for b in 0..4 {
        s.push_str(r.weighted(if b == 3 { last } else { table }));
    }
    s
}

/// Rhythm templates: whole-bar figures dance music is built on.
pub struct Figures {
    pub tresillo: &'static str,
    pub tresillo_b: &'static str,
    pub clave: &'static str,
    pub dotted: &'static str,
    pub offbeat: &'static str,
    pub offbeat16: &'static str,
    pub eighths: &'static str,
    pub gallop: &'static str,
    pub push: &'static str,
    pub broken: &'static str,
    pub stabs: &'static str,
    pub drop1: &'static str,
}

pub const FIGURES: Figures = Figures {
    tresillo: "x..x..x.x..x..x.",
    tresillo_b: "x..x..x...x...x.",
    clave: "x..x..x...x.x...",
    dotted: "x..x..x..x..x..x",
    offbeat: "..x...x...x...x.",
    offbeat16: ".x.x.x.x.x.x.x.x",
    eighths: "x.x.x.x.x.x.x.x.",
    gallop: "x..xx..xx..xx..x",
    push: "x...x...x..x..x.",
    broken: "x.x..x..x.x..x..",
    stabs: "x..x..x.........",
    drop1: "x...............",
};

/// Mutates a few positions of a lane within an alphabet (a variation of a
/// core pattern, never a walk away from it: callers mutate the core).
pub fn mutate(r: &mut R, s: &str, amount: f64, alphabet: &[char]) -> String {
    let mut a: Vec<char> = s.chars().collect();
    let n = js_round(a.len() as f64 * amount).max(1.0) as usize;
    for _ in 0..n {
        let i = r.int(0, a.len() as i32 - 1) as usize;
        a[i] = r.pick(alphabet);
    }
    a.into_iter().collect()
}

// ── Motifs ───────────────────────────────────────────────────────

/// A motif's rhythmic density.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Density {
    Sparse,
    Medium,
    Dense,
}

/// Onset cells by density, for the motif's rhythm.
fn mel_cells(d: Density) -> &'static [(&'static str, f64)] {
    match d {
        Density::Sparse => &[
            ("x...", 5.0),
            ("....", 2.0),
            ("x.x.", 1.0),
            ("..x.", 1.0),
            ("x..x", 1.0),
        ],
        Density::Medium => &[
            ("x...", 3.0),
            ("x.x.", 3.0),
            ("x..x", 2.0),
            ("..x.", 1.0),
            (".x.x", 1.0),
            ("x.xx", 1.0),
            ("xx..", 1.0),
            ("....", 1.0),
        ],
        Density::Dense => &[
            ("x.x.", 3.0),
            ("xx.x", 2.0),
            ("x.xx", 2.0),
            ("xxx.", 1.0),
            ("xxxx", 1.0),
            ("x..x", 1.0),
        ],
    }
}

fn mel_figures(d: Density) -> &'static [&'static str] {
    match d {
        Density::Sparse => &[
            "x.......x.......",
            "x...........x...",
            "x.....x.........",
            "x.......x...x...",
        ],
        Density::Medium => &[
            FIGURES.tresillo,
            FIGURES.tresillo_b,
            FIGURES.push,
            FIGURES.broken,
            "x.x...x.x.x...x.",
            "x...x.x.x...x...",
        ],
        Density::Dense => &[
            "x.x.x.x.x..x..x.",
            "x.xx.x.xx.x.x.x.",
            "xx.x.xx.x.x.x...",
            "x.x.x.x.x.x.x..x",
            FIGURES.gallop,
        ],
    }
}

/// An interval step for the contour: mostly steps, some thirds, few leaps;
/// after a leap the line turns back by step. `bias` leans the direction.
fn interval(r: &mut R, prev: i32, bias: f64) -> i32 {
    if prev.abs() >= 3 {
        return -prev.signum() * r.weighted(&[(1, 3.0), (2, 1.0)]);
    }
    let size = r.weighted(&[(0, 1.2), (1, 4.0), (2, 2.0), (3, 0.8), (4, 0.5), (7, 0.15)]);
    if size == 0 {
        return 0;
    }
    let up = r.chance(0.5 + 0.35 * bias);
    if up { size } else { -size }
}

/// A motif: a rhythm over `bars` bars (onsets as 16th indexes) and a contour
/// (intervals in scale steps between consecutive onsets). The shape is a
/// rise, fall, arch or wave over the motif, which is what the ear follows.
#[derive(Clone, Debug, PartialEq)]
pub struct Motif {
    pub bars: usize,
    pub onsets: Vec<usize>,
    pub ivs: Vec<i32>,
    pub shape: &'static str,
    pub len: usize,
}

/// `motif`'s options, with the lab's defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotifOpts {
    pub bars: usize,
    pub density: Density,
    pub figure: f64,
    pub shape: Option<&'static str>,
}

impl Default for MotifOpts {
    fn default() -> MotifOpts {
        MotifOpts {
            bars: 1,
            density: Density::Medium,
            figure: 0.5,
            shape: None,
        }
    }
}

pub fn motif(r: &mut R, o: MotifOpts) -> Motif {
    let nb = o.bars;
    let mut rhythm = String::new();
    for _ in 0..nb {
        if r.chance(o.figure) {
            rhythm.push_str(r.pick(mel_figures(o.density)));
        } else {
            rhythm.push_str(&cells(r, mel_cells(o.density), None));
        }
    }
    if !rhythm.starts_with('x') {
        // a motif states itself on the downbeat
        rhythm.replace_range(0..1, "x");
    }
    let onsets: Vec<usize> = rhythm
        .char_indices()
        .filter(|(_, c)| *c == 'x')
        .map(|(i, _)| i)
        .collect();
    let shape = match o.shape {
        Some(s) => s,
        None => r.pick(&["arch", "rise", "fall", "wave", "valley"]),
    };
    let mut ivs = Vec::new();
    let mut prev = 0;
    for k in 1..onsets.len() {
        let u = k as f64 / (onsets.len() - 1).max(1) as f64;
        let bias = match shape {
            "rise" => 0.6,
            "fall" => -0.6,
            "arch" => {
                if u < 0.5 {
                    0.8
                } else {
                    -0.8
                }
            }
            "valley" => {
                if u < 0.5 {
                    -0.8
                } else {
                    0.8
                }
            }
            _ => (u * std::f64::consts::PI * 2.0).sin() * 0.8,
        };
        let iv = interval(r, prev, bias);
        ivs.push(iv);
        prev = if iv != 0 { iv } else { prev };
    }
    Motif {
        bars: nb,
        onsets,
        ivs,
        shape,
        len: rhythm.len(),
    }
}

/// Keeps the strong onsets of a motif (beats, and the last onset of each
/// bar), for breakdowns: the hook at half the notes, same shape.
pub fn thin(m: &Motif) -> Motif {
    let mut keep: Vec<usize> = Vec::new();
    for b in 0..m.bars {
        let in_bar: Vec<usize> = m.onsets.iter().copied().filter(|o| o / 16 == b).collect();
        for &o in &in_bar {
            if o.is_multiple_of(4) && !keep.contains(&o) {
                keep.push(o);
            }
        }
        if let Some(&last) = in_bar.last()
            && !keep.contains(&last)
        {
            keep.push(last);
        }
    }
    let onsets = m
        .onsets
        .iter()
        .copied()
        .filter(|o| keep.contains(o))
        .collect();
    // The contour between kept onsets is the sum of the skipped intervals.
    let mut ivs = Vec::new();
    let mut acc = 0;
    for k in 1..m.onsets.len() {
        acc += m.ivs[k - 1];
        if keep.contains(&m.onsets[k]) {
            ivs.push(acc);
            acc = 0;
        }
    }
    Motif {
        onsets,
        ivs,
        ..m.clone()
    }
}

/// The nearest scale degree to k whose pitch class is in `pcs` (ties go the
/// way the line was moving).
fn snap(scale: &Scale, tonic_pc: i32, k: i32, pcs: &[i32], dir: i32) -> i32 {
    for d in 0..7 {
        let steps: &[i32] = if d == 0 { &[0] } else { &[dir * d, -dir * d] };
        for &s in steps {
            let kk = k + s;
            if pcs.contains(&(tonic_pc + deg_semi(scale, kk)).rem_euclid(12)) {
                return kk;
            }
        }
    }
    k
}

/// `realise`'s options:
///   chords   one chord name per bar (the motif's bars cycle over them);
///   tonic    the key's tonic as a midi note near the wanted register;
///   lo, hi   the register, in scale degrees from the tonic;
///   phrase   bars per phrase: the last onset of a phrase ends it, 'open'
///            (3rd or 5th) on odd phrases and 'closed' (root) on even ones;
///   gate     longest note in 16ths (None: until the next onset);
///   home     the degree the motif starts on (None: the nearest chord tone to
///            the middle of the register);
///   avoid    scale degrees (0..6) the line steps over when it is not on a
///            chord tone: [1, 5] in minor leaves the minor pentatonic, the
///            Andean side of chicha; [3, 6] in major the major pentatonic.
#[derive(Clone, Debug, PartialEq)]
pub struct Realise<'a> {
    pub scale: &'static Scale,
    pub tonic: i32,
    pub chords: &'a [String],
    pub lo: i32,
    pub hi: i32,
    pub phrase: usize,
    pub gate: Option<usize>,
    pub home: Option<i32>,
    pub start: i32,
    pub accents: bool,
    pub avoid: &'a [i32],
}

impl Default for Realise<'_> {
    fn default() -> Self {
        Realise {
            scale: MINOR,
            tonic: 0,
            chords: &[],
            lo: -3,
            hi: 9,
            phrase: 4,
            gate: None,
            home: None,
            start: 0,
            accents: true,
            avoid: &[],
        }
    }
}

/// Realises a motif over chords as a `mel` lane (res 1: one token per 16th).
pub fn realise(m: &Motif, o: &Realise) -> String {
    let nb = o.chords.len();
    let tonic_pc = o.tonic.rem_euclid(12);
    let mut toks: Vec<String> = vec![".".to_owned(); nb * 16];
    // Over a dominant in a minor key (the V7's major third is the leading
    // tone) the bar's scale is the harmonic minor: the line's 7th degree is
    // the leading tone there, as a cumbia or a cadence sings it, not the b7
    // a semitone from the chord's third.
    let bar_scale = |ch: &ChordPcs| -> &'static Scale {
        if o.scale == MINOR && ch.pcs.contains(&((tonic_pc + 11) % 12)) {
            HARMONIC_MINOR
        } else {
            o.scale
        }
    };
    let mut scale: &'static Scale;
    let deg = |kk: i32| kk.rem_euclid(7);
    // Steps past the avoided degrees the way the line is moving, turning back
    // at the register's edge.
    let skip = |mut kk: i32, mut d: i32| {
        let mut n = 0;
        while n < 7 && o.avoid.contains(&deg(kk)) {
            kk += d;
            if kk > o.hi || kk < o.lo {
                d = -d;
                kk += 2 * d;
            }
            n += 1;
        }
        kk
    };
    let middle = js_round((o.lo + o.hi) as f64 / 2.0) as i32;
    let mut k = o.home.unwrap_or(middle);
    let mut b0 = 0;
    while b0 < nb {
        let onsets: Vec<usize> = m
            .onsets
            .iter()
            .map(|o| o + b0 * 16)
            .filter(|o| *o < nb * 16)
            .collect();
        let mut dir = 1;
        for i in 0..onsets.len() {
            let on = onsets[i];
            let bar = on / 16;
            let ch = chord_pcs(&o.chords[bar % nb]);
            scale = bar_scale(&ch);
            if i == 0 {
                // Each statement starts where the hook lives, on a chord tone.
                k = snap(
                    scale,
                    tonic_pc,
                    o.home.unwrap_or(middle + o.start),
                    &ch.pcs,
                    1,
                );
            } else {
                let mut iv = m.ivs[i - 1];
                if k + iv > o.hi || k + iv < o.lo {
                    iv = -iv;
                }
                k += iv;
                if iv != 0 {
                    dir = iv.signum();
                }
                k = k.min(o.hi).max(o.lo);
            }
            let last = i == onsets.len() - 1 || onsets[i + 1] / 16 != bar;
            let phrase_end = last && (bar % o.phrase == o.phrase - 1 || bar == nb - 1);
            let next = if i + 1 < onsets.len() {
                onsets[i + 1]
            } else {
                (b0 + m.bars) * 16
            };
            // A note that lasts a dotted 8th or more is a chord tone; passing
            // and neighbour tones are the short ones, off the beat.
            let long = next - on >= 3;
            if phrase_end {
                let closed = (bar / o.phrase) % 2 == 1 || bar == nb - 1;
                let want: Vec<i32> = if closed {
                    vec![ch.root]
                } else {
                    ch.pcs.iter().copied().filter(|p| *p != ch.root).collect()
                };
                k = snap(
                    scale,
                    tonic_pc,
                    k,
                    if want.is_empty() { &ch.pcs } else { &want },
                    dir,
                );
            } else if on.is_multiple_of(4) || last || long {
                k = snap(scale, tonic_pc, k, &ch.pcs, dir);
            } else if !o.avoid.is_empty() {
                k = skip(k, dir);
            }
            let mut len = next.saturating_sub(on).max(1);
            if let Some(gate) = o.gate.filter(|g| *g != 0) {
                len = len.min(gate);
            }
            if phrase_end {
                len = len.min(8);
            }
            let midi = o.tonic + deg_semi(scale, k);
            toks[on] = format!(
                "{}{}{}",
                NOTE[midi.rem_euclid(12) as usize],
                midi.div_euclid(12) - 1,
                if o.accents && on.is_multiple_of(16) {
                    "!"
                } else {
                    ""
                }
            );
            for j in 1..len {
                if on + j >= toks.len() {
                    break;
                }
                toks[on + j] = "_".to_owned();
            }
        }
        b0 += m.bars;
    }
    toks.join(" ")
}

// ── Bass ─────────────────────────────────────────────────────────
// Bass lanes use seq's degree syntax (r o f l u t s) plus the lab's 'n':
// the next chord's root, a pickup.

/// A one-bar maker from per-beat cells with a pickup cell on beat 4 of the
/// bars before a chord change.
pub fn bass_bar(r: &mut R, table: &[(&str, f64)], pickup: f64, changes: bool) -> String {
    let mut s = String::new();
    for b in 0..4 {
        if b == 3 && changes && r.chance(pickup) {
            s.push_str(r.pick(&["..rn", "..n.", "r.nn", "r..n", ".r.n"]));
        } else {
            s.push_str(r.weighted(table));
        }
    }
    s
}

/// Whether the chord changes after bar b of a progression.
pub fn changes_after(prog: &str, b: usize) -> bool {
    let bars_: Vec<&str> = prog.split_whitespace().collect();
    bars_[b % bars_.len()] != bars_[(b + 1) % bars_.len()]
}

// ── Déjà vu and arrangement ──────────────────────────────────────

/// A variation maker for [`variants`]: the stream, the core and the index.
pub type Variant<'a> = &'a dyn Fn(&mut R, &str, usize) -> String;

/// Variations of a core lane: each is the core mutated once (not a walk).
/// `f`, when given, makes variation i from the core instead of `mutate`
/// (it takes the stream, so its own draws stay in order).
pub fn variants(
    r: &mut R,
    core: &str,
    n: usize,
    amount: f64,
    alphabet: &[char],
    f: Option<Variant<'_>>,
) -> Vec<String> {
    let mut out = vec![core.to_owned()];
    for i in 0..n {
        out.push(match f {
            Some(f) => f(r, core, i),
            None => mutate(r, core, amount, alphabet),
        });
    }
    out
}

/// Picks which variant an 8-bar block plays: the core with probability
/// dejavu, else one of the others. Block 0 of a section is always the core.
pub fn pick_variant(r: &mut R, dejavu: f64, n: u32, block: u32) -> u32 {
    if block == 0 || n <= 1 || r.chance(dejavu) {
        return 0;
    }
    r.int(1, n as i32 - 1) as u32
}

/// JS `Math.pow` as V8 computes it (fdlibm): the special exponents return
/// exactly, before the general path. `expand` only raises to 0, 1/2 and 1
/// (sections are 8 or 16 bars), so the lab's `lp` splits are reproduced
/// to the bit; other exponents take the platform's `pow`.
fn js_pow(x: f64, y: f64) -> f64 {
    if y == 0.0 {
        1.0
    } else if y == 1.0 {
        x
    } else if y == 2.0 {
        x * x
    } else if y == 0.5 && x >= 0.0 {
        x.sqrt()
    } else {
        x.powf(y)
    }
}

/// Expands sections into 8-bar blocks. A section may carry `v`: { part:
/// nVariants } and `vd`: nDrumVariants; the keys are `<base><index>` (the
/// pattern names a genre wrote). Events stay where they belong: crash, drop,
/// down and lp on the first block; riser, swell, fill and gap on the last;
/// `auto` ramps are split across the blocks. `v` and `vd` are not copied.
pub fn expand(sections: &[Section], r: &mut R, dejavu: f64) -> Vec<Section> {
    let mut out = Vec::new();
    for s in sections {
        let n = if s.bars > 8 && s.bars % 8 == 0 {
            s.bars / 8
        } else {
            1
        };
        for b in 0..n {
            let first = b == 0;
            let last = b == n - 1;
            let mut o = Section {
                bars: s.bars / n,
                drums: s.drums.clone(),
                p: s.p.clone(),
                ..Default::default()
            };
            if s.prog.as_deref().is_some_and(|p| !p.is_empty()) {
                o.prog = s.prog.clone();
            }
            if let (Some(vd), Some(drums)) = (
                s.vd.filter(|vd| *vd != 0),
                s.drums.as_deref().filter(|d| !d.is_empty()),
            ) {
                o.drums = Some(format!("{drums}{}", pick_variant(r, dejavu, vd, b)));
            }
            for (part, nv) in s.v.iter().flatten() {
                let cur = lookup(&o.p, part).filter(|k| !k.is_empty()).cloned();
                if let Some(cur) = cur {
                    let v = format!("{cur}{}", pick_variant(r, dejavu, *nv, b));
                    put(&mut o.p, part, v);
                }
            }
            if first {
                if s.crash == Some(true) {
                    o.crash = s.crash;
                }
                if s.drop == Some(true) {
                    o.drop = s.drop;
                }
                if s.down.is_some_and(|d| d != 0.0) {
                    o.down = s.down;
                }
            }
            if last {
                if s.riser.is_some_and(|x| x != 0.0) {
                    o.riser = s.riser;
                }
                if s.swell == Some(true) {
                    o.swell = s.swell;
                }
                if s.fill.as_deref().is_some_and(|f| !f.is_empty()) {
                    o.fill = s.fill.clone();
                }
                if s.gap.is_some_and(|x| x != 0.0) {
                    o.gap = s.gap;
                }
            }
            if let Some(lp) = s.lp {
                o.lp = Some(if n == 1 {
                    lp
                } else {
                    [
                        lp[0] * js_pow(lp[1] / lp[0], b as f64 / n as f64),
                        lp[0] * js_pow(lp[1] / lp[0], (b + 1) as f64 / n as f64),
                    ]
                });
            }
            if let Some(auto) = &s.auto {
                o.auto = Some(
                    auto.iter()
                        .map(|(k, [a, z])| {
                            (
                                k.clone(),
                                [
                                    a + (z - a) * (b as f64 / n as f64),
                                    a + (z - a) * ((b + 1) as f64 / n as f64),
                                ],
                            )
                        })
                        .collect(),
                );
            }
            out.push(o);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mulberry32_matches_the_lab() {
        // node -e "…rng(1)()…": the first three draws of seed 1.
        let mut r = Rng::new(1);
        let a = r.next();
        assert!((0.0..1.0).contains(&a));
        // Deterministic: the same seed draws the same stream.
        let mut s = Rng::new(1);
        assert_eq!(s.next(), a);
    }

    #[test]
    fn salted_seed_is_the_labs() {
        // (Math.imul(1 + 1 * 104729, 2654435761) >>> 0) for house seed 1.
        let r = R::new(1, 1);
        assert_eq!(r.r.a, imul(104730, 2654435761));
        let f = r.fork(1);
        assert_eq!(f.r.a, 7919 + 104729);
    }
}
