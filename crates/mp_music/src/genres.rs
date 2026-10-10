//! The genre grammars (`tools/music-lab/gen.js`, docs/vision/sound.md 3.2
//! and 3.4; the module is `genres` because `gen` is reserved in Rust 2024).
//!
//! A seed and a genre make a whole track in the song format (with lab
//! patches and the lab extensions in [`crate::seq`]), so the same sequencer
//! plays it. The musical machinery (keys, rhythm, motifs, pickups, déjà vu,
//! arrangement) is [`crate::compose`]; each grammar here is the genre's own
//! vocabulary: tempo, kit, progressions, the lanes it is built from, its
//! instruments and its form.
//!
//! What every grammar does, because it is what makes a track grab:
//!   - one hook: a motif made once, stated sparse in the breakdown and in
//!     full at the drop, answered in the drop's second half;
//!   - patterns with a shape: cells and Euclidean figures, 4-bar phrases with
//!     a turnaround, 8-bar blocks with the kick out before the crash;
//!   - bass that leads into the next chord ('n' pickups);
//!   - a tension curve: intro, groove, break, build, drop, and the seams
//!     (crash, downlifter, riser, roll, gap) placed by the form;
//!   - déjà vu: every long section plays its core and returns to it.
//!
//! Genres: house (deep, anthem), techno (acid, melodic), trance, eurobeat
//! (the touge: Initial D's mountain roads), psytrance, drum & bass (liquid,
//! roller), UK garage and chicha.
//!
//! Every draw is in the JS's order, including the ones inside object
//! literals (a part's `pat` before its `lab`), so the goldens hash the same.

// A grammar's form is a list of `S.push({...})` lines in the JS; the port
// keeps them as lines (`sx.push`), one section each, instead of one `vec!`.
#![allow(clippy::vec_init_then_push)]

use crate::compose::Item::{Deg as D, Ext as X};
use crate::compose::{
    DORIAN, Density, FIGURES, Item, MINOR, Motif, MotifOpts, NOTE, PHRYGIAN, PROGS, R, Realise,
    Scale, bars, bass_bar, cells, changes_after, euclid, expand, lane, loop_, motif, mutate,
    no_dim, progression, realise, thin, variants,
};
use crate::patches::bpatch;
use crate::track::{Ch, KitTweak, Lab, Pairs, Part, Pump, Section, Track, lookup, put};

/// A lab patch by name, with overrides (`P(name, over)`): the patch's
/// `name` is the lab name unless the overrides set one.
fn p(name: &str, over: impl FnOnce(&mut Lab)) -> Lab {
    let mut lab = bpatch(name).unwrap_or_else(|| panic!("no patch {name}"));
    lab.name = Some(name.to_owned());
    over(&mut lab);
    lab
}

/// `P(name)`.
fn p0(name: &str) -> Lab {
    p(name, |_| {})
}

fn scale_name(s: &Scale) -> &'static str {
    s.name
}

/// A bass register: the key's root between A1 and G#2.
fn bass_lo(tonic: i32) -> f64 {
    (33 + ((tonic - 9 + 12) % 12)) as f64
}

/// Holds each hit over the rests after it, up to max steps ('x--.').
fn hold(s: &str, max: usize) -> String {
    let mut a: Vec<char> = s.chars().collect();
    for i in 0..a.len() {
        if a[i] != 'x' {
            continue;
        }
        let mut j = 1;
        while j <= max && i + j < a.len() && a[i + j] == '.' {
            a[i + j] = '-';
            j += 1;
        }
    }
    a.into_iter().collect()
}

/// Chord names per bar of a progression (the first of a two-chord bar).
fn bar_chords(prog: &str) -> Vec<String> {
    prog.split_whitespace()
        .map(|b| b.split(',').next().unwrap_or("").to_owned())
        .collect()
}

/// `hooks`' options; `phrase` defaults to 4 and `avoid` to none.
struct HookOpts<'a> {
    scale: &'static Scale,
    tonic: i32,
    prog: &'a str,
    lo: i32,
    hi: i32,
    gate: Option<usize>,
    phrase: usize,
    avoid: &'a [i32],
}

impl Default for HookOpts<'_> {
    fn default() -> Self {
        HookOpts {
            scale: MINOR,
            tonic: 0,
            prog: "",
            lo: -3,
            hi: 9,
            gate: None,
            phrase: 4,
            avoid: &[],
        }
    }
}

/// The hook's three realisations.
struct Hooks {
    hook: String,
    sparse: String,
    answer: String,
}

impl Hooks {
    /// As a part's `pat` table, in the JS key order.
    fn pat(self) -> Pairs<String> {
        vec![
            ("hook".to_owned(), self.hook),
            ("sparse".to_owned(), self.sparse),
            ("answer".to_owned(), self.answer),
        ]
    }
}

/// The hook: one motif realised three ways over eight bars of a progression
/// (two statements of a four-bar loop): full, thinned, and answered (the
/// same figure a third higher). The stream is unused here, as in the JS.
fn hooks(_r: &mut R, m: &Motif, o: HookOpts) -> Hooks {
    let c = bar_chords(o.prog);
    let chords: Vec<String> = if c.len() >= 8 {
        c[..8].to_vec()
    } else {
        c.iter().chain(c.iter()).take(8).cloned().collect()
    };
    let over = |mm: &Motif, gate: Option<usize>, start: i32| {
        realise(
            mm,
            &Realise {
                scale: o.scale,
                tonic: o.tonic,
                chords: &chords,
                lo: o.lo,
                hi: o.hi,
                gate,
                phrase: o.phrase,
                avoid: o.avoid,
                start,
                ..Default::default()
            },
        )
    };
    Hooks {
        hook: over(m, o.gate, 0),
        sparse: over(&thin(m), None, 0),
        answer: over(m, o.gate, 2),
    }
}

/// `acidLine`'s default degree weights.
const ACID_DEGS: &[(&str, f64)] = &[
    ("r", 6.0),
    ("o", 4.0),
    ("f", 2.0),
    ("s", 2.0),
    ("t", 1.0),
    ("l", 1.0),
    ("u", 1.0),
];

/// The acid line: steps with notes, rests, accents and slides (bass lane
/// syntax: degrees from the chord root, uppercase accents, ~ slide-holds).
fn acid_line(r: &mut R, density: f64, degs: &[(&str, f64)]) -> String {
    let mut out = String::new();
    for i in 0..16 {
        if out.len() > i {
            continue;
        }
        if !r.chance(density) {
            out.push('.');
            continue;
        }
        let mut d = r.weighted(degs).to_owned();
        if r.chance(0.28) {
            d = d.to_uppercase();
        }
        out.push_str(&d);
        if i < 14 && r.chance(0.2) {
            out.push('~');
        }
    }
    out.truncate(16);
    out
}

/// Sets pattern variants on a part (`b0`, `b1`, ...) and returns the count.
fn set_pats(part: &mut Part, key: &str, list: Vec<String>) -> u32 {
    let n = list.len();
    for (i, s) in list.into_iter().enumerate() {
        put(&mut part.pat, &format!("{key}{i}"), s);
    }
    n as u32
}

fn set_drums(t: &mut Track, key: &str, list: Vec<Pairs<String>>) -> u32 {
    let n = list.len();
    for (i, d) in list.into_iter().enumerate() {
        put(&mut t.drums, &format!("{key}{i}"), d);
    }
    n as u32
}

// ── Literals ─────────────────────────────────────────────────────
// A JS object literal of strings, a spread with overrides, a merge of
// spreads, and the small option builders a section line uses.

fn sp(list: &[(&str, &str)]) -> Pairs<String> {
    list.iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

/// `{ ...base, k: v, ... }`: an existing key keeps its place.
fn with(base: &Pairs<String>, over: &[(&str, &str)]) -> Pairs<String> {
    let mut out = base.clone();
    for (k, v) in over {
        put(&mut out, k, (*v).to_owned());
    }
    out
}

/// `{ ...a, ...b, ... }`.
fn merge(parts: &[&Pairs<String>]) -> Pairs<String> {
    let mut out = Vec::new();
    for part in parts {
        for (k, v) in part.iter() {
            put(&mut out, k, v.clone());
        }
    }
    out
}

fn get<'a>(pairs: &'a Pairs<String>, k: &str) -> &'a str {
    lookup(pairs, k)
        .map(String::as_str)
        .unwrap_or_else(|| panic!("no {k}"))
}

fn s(x: &str) -> Option<String> {
    Some(x.to_owned())
}

fn lay(list: &[(&str, f64)]) -> Pairs<f64> {
    list.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect()
}

fn vv(list: &[(&str, u32)]) -> Option<Pairs<u32>> {
    Some(list.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect())
}

fn au(list: &[(&str, [f64; 2])]) -> Option<Pairs<[f64; 2]>> {
    Some(list.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect())
}

fn kit(list: &[(&str, KitTweak)]) -> Option<Pairs<KitTweak>> {
    Some(
        list.iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect(),
    )
}

fn tweak(sample: Option<&str>, g: Option<f64>, rev: Option<f64>) -> KitTweak {
    KitTweak {
        s: sample.map(str::to_owned),
        g,
        rev,
    }
}

/// The snare build every four-to-the-floor grammar shares: on the beats,
/// then 8ths, 16ths.
fn snare_build(b: usize, soft: bool) -> &'static str {
    if b < 4 {
        if soft {
            "....x.......x..."
        } else {
            "....X.......X..."
        }
    } else if b < 6 {
        "x...x...x...x..."
    } else {
        "x.x.x.x.x.x.x.x."
    }
}

const HOLD: &str = "x---------------";

// ── House ────────────────────────────────────────────────────────
// Deep: dorian, 9th chords, an organ or EP riff, no fuss. Anthem: minor,
// the axis progression, piano stabs, a choir and a hook over the drop.
pub fn house(seed: u32, dejavu: Option<f64>) -> Track {
    let dejavu = dejavu.unwrap_or(0.7);
    let mut r = R::new(seed, 1);
    let r = &mut r;
    let sub = if r.fork(1).chance(0.5) {
        "deep"
    } else {
        "anthem"
    };
    let deep = sub == "deep";
    let tonic = r.int(0, 11);
    let bpm = r.int(122, 126);
    let scale = if deep { DORIAN } else { MINOR };
    let ext = if deep { "9" } else { r.pick(&["", "7"]) };
    let prog_a = progression(
        scale,
        tonic,
        r.pick(&if deep {
            no_dim(scale, PROGS.deep)
        } else {
            PROGS.anthem.to_vec()
        }),
        ext,
    );
    let prog_b = progression(scale, tonic, r.pick(&no_dim(scale, PROGS.verse)), ext);
    let mut t = Track::skeleton(&format!("house-{seed}"));
    t.title = Some(format!("House {seed}"));
    t.style = Some(format!(
        "{} · {} {} · {}",
        if deep { "Deep house" } else { "Piano house" },
        NOTE[tonic as usize],
        scale_name(scale),
        prog_a
    ));
    t.bpm = bpm as f64;
    t.gain = 0.9;
    t.swing = Some(r.pick(&[0.0, 0.05, 0.08]));
    t.kit_name = s(if r.chance(0.7) { "tr909" } else { "tr808" });
    t.kit = kit(&[
        ("kick", tweak(Some("kickHouse"), None, None)),
        ("clap", tweak(None, None, Some(0.3))),
        ("ohat", tweak(None, Some(0.9), None)),
    ]);
    t.prog = sp(&[("a", &prog_a), ("b", &prog_b)]);
    t.lay = lay(&[
        ("shaker", 0.45),
        ("rim", 0.55),
        ("ride", 0.7),
        ("ohat", 0.25),
        ("hat", 0.15),
        ("lead", 0.3),
        ("choir", 0.5),
        ("stab", 0.2),
    ]);

    // Drums: an 8-bar groove with the turnaround and the kick out in bar 8.
    let hat_cell = r.pick(&["x...", "xo..", "xo.o", "x..o", "xx.o", "xo.x"]); // step 2 is the open hat's
    let hat = format!(
        "{}{}",
        &lane(4, |_| hat_cell)[..12],
        r.pick(&[hat_cell, "xo.o", "x..o"])
    );
    let kick8 = loop_(
        8,
        "x...x...x...x...",
        r.pick(&["x...x...x.......", "x...x...x...x..x", "x...x...x...x..."]),
    );
    let clap8 = loop_(
        8,
        "....x.......x...",
        r.pick(&["....x.......x.xx", "....x.......x...", "....x.......x..o"]),
    );
    let rim: String = euclid(r.pick(&[3, 5]), 16, r.int(0, 3), 'o')
        .chars()
        .map(|c| {
            if c == 'o' {
                if r.chance(0.3) { 'x' } else { 'o' }
            } else {
                c
            }
        })
        .collect();
    let groove = sp(&[
        ("kick", &kick8),
        ("clap", &clap8),
        ("ohat", "..x...x...x...x."),
        ("hat", &hat),
        ("shaker", "xoxoxoxoxoxoxoxo"),
        ("rim", &rim),
    ]);
    let g1 = with(
        &groove,
        &[
            ("hat", &mutate(r, &hat, 0.12, &['x', 'o', '.'])),
            ("rim", &euclid(r.pick(&[3, 5, 7]), 16, r.int(0, 5), 'o')),
        ],
    );
    let g2 = with(
        &groove,
        &[
            ("rim", &mutate(r, &rim, 0.15, &['x', 'o', '.', '.'])),
            ("ride", "..x...x...x...x."),
        ],
    );
    let nd = set_drums(&mut t, "g", vec![groove.clone(), g1, g2]);
    put(
        &mut t.drums,
        "intro",
        sp(&[("kick", "x...x...x...x..."), ("hat", &hat)]),
    );
    put(
        &mut t.drums,
        "intro2",
        sp(&[
            ("kick", "x...x...x...x..."),
            ("hat", &hat),
            ("ohat", get(&groove, "ohat")),
            ("shaker", get(&groove, "shaker")),
        ]),
    );
    put(
        &mut t.drums,
        "brk",
        sp(&[
            ("hat", "..x...x...x...x."),
            ("shaker", get(&groove, "shaker")),
        ]),
    );
    put(
        &mut t.drums,
        "brk2",
        sp(&[
            ("shaker", get(&groove, "shaker")),
            ("rim", &rim),
            ("ohat", "..............x."),
        ]),
    );
    put(
        &mut t.drums,
        "build",
        sp(&[
            ("kick", "x...x...x...x..."),
            ("clap", "....x.......x..."),
            ("hat", "x.x.x.x.x.x.x.x."),
        ]),
    );

    // Bass: off-beat roots and octaves, a pickup before each change.
    let table: &[(&str, f64)] = if deep {
        &[
            ("..r.", 6.0),
            ("..o.", 2.0),
            ("..rr", 1.0),
            ("..ro", 1.0),
            ("....", 0.5),
        ]
    } else {
        &[
            ("..r.", 5.0),
            ("r.r.", 2.0),
            ("..o.", 2.0),
            ("..rr", 1.0),
            ("r..r", 1.0),
        ]
    };
    let bass_core = bars(4, |b| bass_bar(r, table, 0.75, changes_after(&prog_a, b)));
    let mut bass = Part {
        kind: "bass".into(),
        lo: Some(bass_lo(tonic)),
        ch: Ch {
            pump: Some(true),
            level: Some(1.0),
            ..Default::default()
        },
        pat: vec![],
        lab: p("pluckBass", |l| {
            l.cutoff = Some(260.0);
            l.res = Some(0.45);
            l.decay = Some(0.16);
            l.name = s("house bass");
        }),
        ..Default::default()
    };
    let nb = set_pats(
        &mut bass,
        "b",
        variants(r, &bass_core, 2, 0.1, &['r', 'o', '.', 'f'], None),
    );
    put(&mut t.parts, "bass", bass);

    // Stabs: a Euclidean figure, held longer on the deep side.
    let k = r.pick(&[3, 5, 6]);
    let rot = r.int(0, 2);
    let stab_bar = if deep {
        hold(
            &r.pick(&[
                euclid(k, 16, rot, 'x'),
                FIGURES.tresillo_b.to_owned(),
                FIGURES.stabs.to_owned(),
            ]),
            5,
        )
    } else {
        hold(&euclid(k, 16, rot, 'x'), 2)
    };
    let stab_core = loop_(
        4,
        &stab_bar,
        &hold(&euclid(k, 16, rot + 1, 'x'), if deep { 5 } else { 2 }),
    );
    let mut stab = Part {
        kind: "chord".into(),
        lo: Some(if deep { 55.0 } else { 58.0 }),
        ch: Ch {
            pump: Some(true),
            rev: Some(0.3),
            dly: Some(0.25),
            level: Some(0.45),
            ..Default::default()
        },
        pat: vec![],
        lab: if deep {
            if r.chance(0.5) { p0("organ") } else { p0("ep") }
        } else if r.chance(0.6) {
            p0("piano")
        } else {
            p0("organ")
        },
        ..Default::default()
    };
    let ns = set_pats(
        &mut stab,
        "s",
        variants(r, &stab_core, 2, 0.08, &['x', '.'], None),
    );
    put(&mut t.parts, "stab", stab);

    put(
        &mut t.parts,
        "pad",
        Part {
            kind: "chord".into(),
            lo: Some(55.0),
            ch: Ch {
                pump: Some(true),
                rev: Some(0.5),
                hp: Some(180.0),
                level: Some(0.8),
                ..Default::default()
            },
            pat: sp(&[("hold", HOLD)]),
            lab: p("warmPad", |l| l.r = Some(0.6)),
            ..Default::default()
        },
    );
    // The choir: 'ah' over the anthem, a quieter 'oo' under the deep one.
    put(
        &mut t.parts,
        "choir",
        Part {
            kind: "chord".into(),
            lo: Some(60.0),
            ch: Ch {
                pump: Some(true),
                rev: Some(0.55),
                hp: Some(200.0),
                level: Some(if deep { 0.35 } else { 0.5 }),
                ..Default::default()
            },
            pat: sp(&[("hold", HOLD)]),
            lab: p("choir", |l| {
                l.vowel = s(if deep { "u" } else { r.pick(&["a", "o"]) });
                l.r = Some(0.5);
            }),
            ..Default::default()
        },
    );

    // The hook: a riff on keys (deep: low and sparse) or a melody over the
    // drop; its answer is realised over the verse progression it plays on.
    let m_bars = if r.chance(0.5) { 1 } else { 2 };
    let m = motif(
        r,
        MotifOpts {
            bars: m_bars,
            density: if deep {
                Density::Sparse
            } else {
                Density::Medium
            },
            figure: 0.5,
            ..Default::default()
        },
    );
    let lead_patch = if deep {
        p0("ep")
    } else {
        r.pick(&[p0("ep"), p0("glass"), p0("piano"), p0("bell")])
    };
    let h = hooks(
        r,
        &m,
        HookOpts {
            scale,
            tonic: (if deep { 48 } else { 60 }) + tonic,
            prog: &prog_a,
            lo: -2,
            hi: 9,
            gate: Some(if deep { 6 } else { 4 }),
            ..Default::default()
        },
    );
    let hb = hooks(
        r,
        &m,
        HookOpts {
            scale,
            tonic: (if deep { 48 } else { 60 }) + tonic,
            prog: &prog_b,
            lo: -2,
            hi: 9,
            gate: Some(if deep { 6 } else { 4 }),
            ..Default::default()
        },
    );
    let mut lead_pat = h.pat();
    lead_pat.push(("answerB".to_owned(), hb.answer));
    put(
        &mut t.parts,
        "lead",
        Part {
            kind: "mel".into(),
            res: Some(1.0),
            ch: Ch {
                rev: Some(0.35),
                dly: Some(0.3),
                pan: Some(0.1),
                pump: Some(true),
                hp: Some(250.0),
                level: Some(0.9),
                ..Default::default()
            },
            pat: lead_pat,
            lab: lead_patch,
            ..Default::default()
        },
    );

    let mut sx: Vec<Section> = Vec::new();
    sx.push(Section {
        bars: 8,
        drums: s("intro"),
        lp: Some([600.0, 16000.0]),
        p: vec![],
        ..Default::default()
    });
    sx.push(Section {
        bars: 8,
        drums: s("intro2"),
        p: sp(&[("bass", "b0")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("g"),
        vd: Some(nd),
        crash: Some(true),
        fill: s("clap"),
        p: sp(&[("bass", "b"), ("stab", "s"), ("pad", "hold")]),
        v: vv(&[("bass", nb), ("stab", ns)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 8,
        drums: s("brk"),
        down: Some(2.0),
        auto: au(&[("pad.cutoff", [600.0, 2400.0])]),
        p: sp(&[("pad", "hold"), ("lead", "sparse")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 8,
        drums: s("build"),
        riser: Some(8.0),
        swell: Some(true),
        fill: s("roll"),
        gap: Some(2.0),
        p: sp(&[("pad", "hold"), ("stab", "s0"), ("bass", "b0")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("g"),
        vd: Some(nd),
        crash: Some(true),
        drop: Some(true),
        fill: s("clap"),
        p: sp(&[
            ("bass", "b"),
            ("stab", "s"),
            ("pad", "hold"),
            ("lead", "hook"),
            ("choir", "hold"),
        ]),
        v: vv(&[("bass", nb), ("stab", ns)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("g"),
        vd: Some(nd),
        prog: s("b"),
        fill: s("clap"),
        p: sp(&[
            ("bass", "b"),
            ("stab", "s"),
            ("lead", "answerB"),
            ("choir", "hold"),
        ]),
        v: vv(&[("bass", nb), ("stab", ns)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("brk2"),
        down: Some(2.0),
        auto: au(&[("pad.cutoff", [500.0, 2800.0])]),
        p: sp(&[("pad", "hold"), ("lead", "hook"), ("choir", "hold")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 8,
        drums: s("build"),
        riser: Some(8.0),
        swell: Some(true),
        fill: s("roll"),
        gap: Some(2.0),
        p: sp(&[("pad", "hold"), ("stab", "s0"), ("choir", "hold")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("g"),
        vd: Some(nd),
        crash: Some(true),
        drop: Some(true),
        fill: s("clap"),
        p: sp(&[
            ("bass", "b"),
            ("stab", "s"),
            ("pad", "hold"),
            ("lead", "hook"),
            ("choir", "hold"),
        ]),
        v: vv(&[("bass", nb), ("stab", ns)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("intro2"),
        lp: Some([16000.0, 500.0]),
        p: sp(&[("bass", "b0"), ("stab", "s0")]),
        ..Default::default()
    });
    t.sections = expand(&sx, r, dejavu);
    t
}

// ── Techno ───────────────────────────────────────────────────────
// Acid: phrygian drone, a 303 motif that the filter carries for ten minutes
// of intent, dub chords, odd-length percussion that phases against the bar.
// Melodic: slower, a 16th arpeggio on a pluck over chords that change every
// two bars, a wide pad, a long lead note now and then.
pub fn techno(seed: u32, dejavu: Option<f64>) -> Track {
    let dejavu = dejavu.unwrap_or(0.8);
    let mut r = R::new(seed, 2);
    let r = &mut r;
    let sub = if r.fork(1).chance(0.5) {
        "acid"
    } else {
        "melodic"
    };
    let acid = sub == "acid";
    let tonic = r.int(0, 11);
    let bpm = if acid {
        r.int(132, 138)
    } else {
        r.int(124, 128)
    };
    let scale = if acid {
        if r.chance(0.6) { PHRYGIAN } else { MINOR }
    } else {
        MINOR
    };
    let degs: Vec<Item> = if acid {
        r.pick(PROGS.drone).to_vec()
    } else {
        r.pick(PROGS.anthem).iter().flat_map(|d| [*d, *d]).collect()
    };
    let ext = if acid {
        r.pick(&["", "7"])
    } else {
        r.pick(&["", "add9"])
    };
    let prog = progression(scale, tonic, &degs, ext);
    let mut t = Track::skeleton(&format!("techno-{seed}"));
    t.title = Some(format!("Techno {seed}"));
    t.style = Some(format!(
        "{} · {} {} · {}",
        if acid {
            "Acid techno"
        } else {
            "Melodic techno"
        },
        NOTE[tonic as usize],
        scale_name(scale),
        prog
    ));
    t.bpm = bpm as f64;
    t.gain = 1.3;
    t.delay = r.pick(&[0.75, 0.5, 1.5]);
    t.delay_fb = 0.45;
    t.pump = Pump {
        depth: 0.3,
        release: 0.12,
    };
    t.kit_name = s(if r.chance(0.6) { "tr909" } else { "tr808" });
    t.kit = kit(&[
        ("kick", tweak(Some("kickTight"), Some(1.05), None)),
        ("clap", tweak(None, None, Some(0.45))),
    ]);
    t.prog = sp(&[("a", &prog)]);
    t.lay = lay(&[
        ("ride", 0.55),
        ("rim", 0.4),
        ("clap", 0.3),
        ("ohat", 0.2),
        ("tomL", 0.5),
        ("snap", 0.6),
        ("acid", 0.3),
        ("arp", 0.3),
        ("chord", 0.5),
        ("blip", 0.6),
        ("lead", 0.4),
    ]);

    // Drums: the kick out on the last beats of bar 8; hats with ghosts; a
    // Euclidean rim and a low tom; a 5- or 7-step snap that phases.
    let hat = cells(
        r,
        &[("..x.", 5.0), (".ox.", 2.0), ("..xo", 2.0), ("o.x.", 1.0)],
        None,
    );
    let kick8 = loop_(
        8,
        "x...x...x...x...",
        r.pick(&["x...x...x.......", "x...x...x...x...", "x...x..........."]),
    );
    let rim: String = euclid(r.pick(&[5, 7]), 16, r.int(0, 4), 'o')
        .chars()
        .map(|c| {
            if c == 'o' {
                if r.chance(0.25) { 'x' } else { 'o' }
            } else {
                c
            }
        })
        .collect();
    let tom = euclid(3, 16, r.int(0, 15), 'o');
    let snap_len = r.pick(&[5usize, 7, 12]);
    let snap = lane(snap_len, |i| {
        if i == 0 {
            "x"
        } else if i == snap_len - 2 && r.chance(0.5) {
            "o"
        } else {
            "."
        }
    });
    let clap = if r.chance(0.6) {
        "....x.......x..."
    } else {
        "............x..."
    };
    let base = sp(&[
        ("kick", &kick8),
        ("hat", &hat),
        ("ohat", "..x...x...x...x."),
        ("clap", clap),
        ("rim", &rim),
    ]);
    let d0 = with(&base, &[("tomL", &tom)]);
    let d1 = with(
        &base,
        &[
            ("hat", &mutate(r, &hat, 0.1, &['x', 'o', '.'])),
            ("ride", "..x...x...x...x."),
            ("snap", &snap),
        ],
    );
    let d2 = with(
        &base,
        &[
            ("rim", &euclid(r.pick(&[5, 7, 9]), 16, r.int(0, 6), 'o')),
            ("tomL", &tom),
            ("ride", "..x...x...x...x."),
        ],
    );
    let nd = set_drums(&mut t, "d", vec![d0, d1, d2]);
    put(
        &mut t.drums,
        "intro",
        sp(&[("kick", "x...x...x...x..."), ("hat", &hat)]),
    );
    put(
        &mut t.drums,
        "intro2",
        sp(&[
            ("kick", "x...x...x...x..."),
            ("hat", &hat),
            ("clap", clap),
            ("ohat", get(&base, "ohat")),
        ]),
    );
    put(
        &mut t.drums,
        "brk",
        sp(&[("hat", &hat), ("ride", "..x...x...x...x."), ("rim", &rim)]),
    );
    put(
        &mut t.drums,
        "build",
        sp(&[
            ("kick", "x...x...x...x..."),
            ("hat", "x.x.x.x.x.x.x.x."),
            ("clap", "....x.......x..."),
            ("rim", &rim),
        ]),
    );

    let lo = (28 + ((tonic - 4 + 12) % 12)) as f64;
    put(
        &mut t.parts,
        "rumble",
        Part {
            kind: "bass".into(),
            lo: Some(lo),
            ch: Ch {
                pump: Some(true),
                rev: Some(0.15),
                ..Default::default()
            },
            pat: sp(&[("off", "..l...l...l...l."), ("roll", ".ll..ll..ll..ll.")]),
            lab: p0("rumble"),
            ..Default::default()
        },
    );
    put(
        &mut t.parts,
        "chord",
        Part {
            kind: "chord".into(),
            lo: Some(60.0),
            ch: Ch {
                rev: Some(0.45),
                dly: Some(0.6),
                pan: Some(0.2),
                level: Some(1.6),
                ..Default::default()
            },
            pat: sp(&[(
                "dub",
                r.pick(&[
                    "..x.............",
                    "......x.........",
                    "..x.......x.....",
                    "...x......x.....",
                ]),
            )]),
            lab: p0("dubChord"),
            ..Default::default()
        },
    );
    put(
        &mut t.parts,
        "pad",
        Part {
            kind: "chord".into(),
            lo: Some(52.0),
            ch: Ch {
                rev: Some(0.55),
                hp: Some(140.0),
                level: Some(1.5),
                pump: Some(true),
                ..Default::default()
            },
            pat: sp(&[("hold", HOLD)]),
            lab: if acid {
                p0("darkPad")
            } else {
                p0("superPadWide")
            },
            ..Default::default()
        },
    );
    let mut sx: Vec<Section> = Vec::new();
    if acid {
        // The acid motif: one bar that stays; the variations toggle accents
        // and slides and move two steps.
        let core = acid_line(r, 0.8, ACID_DEGS);
        let mut acid_part = Part {
            kind: "bass".into(),
            lo: Some(lo + 12.0),
            ch: Ch {
                drive: Some(1.6),
                drive_lp: Some(7000.0),
                dly: Some(0.2),
                level: Some(0.75),
                ..Default::default()
            },
            pat: vec![],
            lab: p("acid", |l| {
                l.cutoff = Some(220.0);
                l.res = Some(0.86);
                l.env = Some(0.5);
                l.decay = Some(0.3);
            }),
            ..Default::default()
        };
        let na = set_pats(
            &mut acid_part,
            "a",
            variants(
                r,
                &core,
                3,
                0.12,
                &['r', 'o', 'R', 'f', '.', 's', 'O'],
                None,
            ),
        );
        put(&mut t.parts, "acid", acid_part);
        // A dub-techno blip: two notes with the delay, in and out.
        let bm = motif(
            r,
            MotifOpts {
                bars: 2,
                density: Density::Sparse,
                figure: 0.6,
                ..Default::default()
            },
        );
        let bh = hooks(
            r,
            &bm,
            HookOpts {
                scale,
                tonic: 72 + tonic,
                prog: &prog,
                lo: -2,
                hi: 6,
                gate: Some(2),
                ..Default::default()
            },
        );
        put(
            &mut t.parts,
            "blip",
            Part {
                kind: "mel".into(),
                res: Some(1.0),
                ch: Ch {
                    rev: Some(0.4),
                    dly: Some(0.7),
                    pan: Some(-0.3),
                    hp: Some(400.0),
                    level: Some(1.4),
                    ..Default::default()
                },
                pat: bh.pat(),
                lab: p0("glass"),
                ..Default::default()
            },
        );
        sx.push(Section {
            bars: 16,
            drums: s("intro"),
            lp: Some([300.0, 16000.0]),
            p: sp(&[("rumble", "off")]),
            ..Default::default()
        });
        sx.push(Section {
            bars: 16,
            drums: s("intro2"),
            crash: Some(true),
            auto: au(&[("acid.cutoff", [150.0, 400.0])]),
            p: sp(&[("rumble", "off"), ("acid", "a0")]),
            ..Default::default()
        });
        sx.push(Section {
            bars: 16,
            drums: s("d"),
            vd: Some(nd),
            fill: s("perc"),
            auto: au(&[("acid.cutoff", [400.0, 900.0])]),
            p: sp(&[
                ("rumble", "off"),
                ("acid", "a"),
                ("chord", "dub"),
                ("blip", "sparse"),
            ]),
            v: vv(&[("acid", na)]),
            ..Default::default()
        });
        sx.push(Section {
            bars: 16,
            drums: s("brk"),
            down: Some(2.0),
            auto: au(&[
                ("acid.cutoff", [500.0, 2000.0]),
                ("acid.decay", [0.25, 0.8]),
                ("acid.res", [0.86, 0.92]),
            ]),
            p: sp(&[("acid", "a0"), ("chord", "dub"), ("pad", "hold")]),
            ..Default::default()
        });
        sx.push(Section {
            bars: 8,
            drums: s("build"),
            riser: Some(8.0),
            swell: Some(true),
            gap: Some(1.0),
            fill: s("perc"),
            auto: au(&[("acid.cutoff", [1200.0, 2400.0])]),
            p: sp(&[("rumble", "roll"), ("acid", "a0"), ("pad", "hold")]),
            ..Default::default()
        });
        sx.push(Section {
            bars: 16,
            drums: s("d"),
            vd: Some(nd),
            crash: Some(true),
            drop: Some(true),
            auto: au(&[("acid.cutoff", [1800.0, 900.0]), ("acid.decay", [0.5, 0.3])]),
            p: sp(&[
                ("rumble", "off"),
                ("acid", "a"),
                ("chord", "dub"),
                ("blip", "hook"),
            ]),
            v: vv(&[("acid", na)]),
            ..Default::default()
        });
        sx.push(Section {
            bars: 16,
            drums: s("d"),
            vd: Some(nd),
            fill: s("perc"),
            auto: au(&[("acid.cutoff", [900.0, 500.0]), ("acid.env", [0.5, 0.75])]),
            p: sp(&[
                ("rumble", "off"),
                ("acid", "a"),
                ("chord", "dub"),
                ("blip", "answer"),
            ]),
            v: vv(&[("acid", na)]),
            ..Default::default()
        });
        sx.push(Section {
            bars: 16,
            drums: s("d"),
            vd: Some(nd),
            auto: au(&[("acid.cutoff", [500.0, 250.0])]),
            p: sp(&[("rumble", "off"), ("acid", "a")]),
            v: vv(&[("acid", na)]),
            ..Default::default()
        });
        sx.push(Section {
            bars: 16,
            drums: s("intro"),
            lp: Some([16000.0, 300.0]),
            p: sp(&[("rumble", "off")]),
            ..Default::default()
        });
    } else {
        // The sequence: a 16th arpeggio over chord tones, with its own shape.
        let arps = [
            "0 0 1 0 2 0 1 0 0 0 1 0 3 0 1 0",
            "0 1 2 1 3 1 2 1 0 1 2 1 4 1 2 1",
            "0 . 1 . 2 . 1 . 0 . 1 . 3 . 1 .",
            "0 0 2 0 0 2 0 0 2 0 0 2 0 0 1 0",
            "0 2 1 3 0 2 4 3 0 2 1 3 0 2 5 3",
        ];
        let arp_core = r.pick(&arps);
        let swap_arp = |r: &mut R, s: &str, _i: usize| {
            s.split(' ')
                .map(|t| {
                    if t != "." && r.chance(0.15) {
                        r.int(0, 4).to_string()
                    } else {
                        t.to_owned()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        let mut arp = Part {
            kind: "arp".into(),
            lo: Some(60.0),
            res: Some(1.0),
            ch: Ch {
                rev: Some(0.35),
                dly: Some(0.5),
                pan: Some(-0.15),
                pump: Some(true),
                level: Some(0.85),
                ..Default::default()
            },
            pat: vec![],
            lab: p("pluckTrance", |l| {
                l.cutoff = Some(700.0);
                l.gain = Some(0.5);
                l.name = s("sequence");
            }),
            ..Default::default()
        };
        let na = set_pats(
            &mut arp,
            "a",
            variants(r, arp_core, 3, 0.0, &[], Some(&swap_arp)),
        );
        put(&mut t.parts, "arp", arp);
        put(
            &mut t.parts,
            "sub",
            Part {
                kind: "bass".into(),
                lo: Some(lo + 12.0),
                ch: Ch {
                    pump: Some(true),
                    level: Some(1.6),
                    ..Default::default()
                },
                pat: sp(&[("off", "..r...r...r...r."), ("roll", "r.r.r.r.r.r.r.r.")]),
                lab: p0("sub"),
                ..Default::default()
            },
        );
        let lm = motif(
            r,
            MotifOpts {
                bars: 2,
                density: Density::Sparse,
                figure: 0.7,
                shape: Some("arch"),
            },
        );
        let lh = hooks(
            r,
            &lm,
            HookOpts {
                scale,
                tonic: 60 + tonic,
                prog: &prog,
                lo: -2,
                hi: 8,
                gate: Some(12),
                phrase: 4,
                ..Default::default()
            },
        );
        put(
            &mut t.parts,
            "lead",
            Part {
                kind: "mel".into(),
                res: Some(1.0),
                legato: Some(true),
                ch: Ch {
                    rev: Some(0.5),
                    dly: Some(0.55),
                    hp: Some(300.0),
                    pump: Some(true),
                    level: Some(0.8),
                    ..Default::default()
                },
                pat: lh.pat(),
                lab: p("sawLead", |l| {
                    l.cutoff = Some(1800.0);
                    l.glide = Some(0.08);
                }),
                ..Default::default()
            },
        );
        sx.push(Section {
            bars: 16,
            drums: s("intro"),
            lp: Some([300.0, 16000.0]),
            p: sp(&[("rumble", "off")]),
            ..Default::default()
        });
        sx.push(Section {
            bars: 16,
            drums: s("intro2"),
            crash: Some(true),
            auto: au(&[("arp.cutoff", [500.0, 900.0])]),
            p: sp(&[("rumble", "off"), ("sub", "off"), ("arp", "a0")]),
            ..Default::default()
        });
        sx.push(Section {
            bars: 16,
            drums: s("d"),
            vd: Some(nd),
            fill: s("perc"),
            auto: au(&[("arp.cutoff", [900.0, 1600.0])]),
            p: sp(&[
                ("rumble", "off"),
                ("sub", "off"),
                ("arp", "a"),
                ("chord", "dub"),
            ]),
            v: vv(&[("arp", na)]),
            ..Default::default()
        });
        sx.push(Section {
            bars: 16,
            drums: s("brk"),
            down: Some(2.0),
            auto: au(&[
                ("pad.cutoff", [600.0, 2400.0]),
                ("arp.cutoff", [800.0, 2400.0]),
            ]),
            p: sp(&[("arp", "a0"), ("pad", "hold"), ("lead", "sparse")]),
            ..Default::default()
        });
        sx.push(Section {
            bars: 8,
            drums: s("build"),
            riser: Some(8.0),
            swell: Some(true),
            gap: Some(1.0),
            fill: s("perc"),
            p: sp(&[
                ("sub", "roll"),
                ("arp", "a0"),
                ("pad", "hold"),
                ("lead", "hook"),
            ]),
            ..Default::default()
        });
        sx.push(Section {
            bars: 16,
            drums: s("d"),
            vd: Some(nd),
            crash: Some(true),
            drop: Some(true),
            p: sp(&[
                ("rumble", "off"),
                ("sub", "off"),
                ("arp", "a"),
                ("chord", "dub"),
                ("pad", "hold"),
                ("lead", "hook"),
            ]),
            v: vv(&[("arp", na)]),
            ..Default::default()
        });
        sx.push(Section {
            bars: 16,
            drums: s("d"),
            vd: Some(nd),
            fill: s("perc"),
            p: sp(&[
                ("rumble", "off"),
                ("sub", "off"),
                ("arp", "a"),
                ("chord", "dub"),
                ("pad", "hold"),
                ("lead", "answer"),
            ]),
            v: vv(&[("arp", na)]),
            ..Default::default()
        });
        sx.push(Section {
            bars: 16,
            drums: s("d"),
            vd: Some(nd),
            auto: au(&[("arp.cutoff", [1600.0, 500.0])]),
            p: sp(&[("rumble", "off"), ("sub", "off"), ("arp", "a")]),
            v: vv(&[("arp", na)]),
            ..Default::default()
        });
        sx.push(Section {
            bars: 16,
            drums: s("intro"),
            lp: Some([16000.0, 300.0]),
            p: sp(&[("rumble", "off"), ("sub", "off")]),
            ..Default::default()
        });
    }
    t.sections = expand(&sx, r, dejavu);
    t
}

// ── Trance ───────────────────────────────────────────────────────
// Uplifting: the rolling off-beat bass from bar one, a pluck arpeggio, the
// supersaw hook alone over pads in a long breakdown, a snare build, and the
// drop with everything.
pub fn trance(seed: u32, dejavu: Option<f64>) -> Track {
    let dejavu = dejavu.unwrap_or(0.75);
    let mut r = R::new(seed, 3);
    let r = &mut r;
    let tonic = r.int(0, 11);
    let bpm = r.int(136, 140);
    let scale = MINOR;
    let prog = progression(scale, tonic, r.pick(PROGS.anthem), "");
    let mut t = Track::skeleton(&format!("trance-{seed}"));
    t.title = Some(format!("Trance {seed}"));
    t.style = Some(format!(
        "Trance · {} minor · {}",
        NOTE[tonic as usize], prog
    ));
    t.bpm = bpm as f64;
    t.gain = 1.3;
    t.delay = 0.75;
    t.delay_fb = 0.4;
    t.pump = Pump {
        depth: 0.5,
        release: 0.16,
    };
    t.kit_name = s("tr909");
    t.kit = kit(&[
        ("kick", tweak(Some("kickTight"), Some(1.05), None)),
        ("clap", tweak(None, None, Some(0.35))),
        ("snare", tweak(Some("snareCrisp"), None, Some(0.3))),
    ]);
    t.prog = sp(&[("a", &prog)]);
    t.lay = lay(&[
        ("clap", 0.2),
        ("snare", 0.6),
        ("ohat", 0.3),
        ("ride", 0.7),
        ("hat", 0.1),
        ("arp", 0.35),
        ("lead", 0.3),
        ("pad", 0.15),
    ]);

    let hat = cells(r, &[("o.x.", 5.0), ("oox.", 2.0), ("o.xo", 2.0)], None);
    let kick8 = loop_(
        8,
        "x...x...x...x...",
        r.pick(&["x...x...x.......", "x...x...x...x..."]),
    );
    let groove = sp(&[
        ("kick", &kick8),
        ("clap", "....x.......x..."),
        ("hat", &hat),
    ]);
    let d0 = with(
        &groove,
        &[("snare", "....x.......x..."), ("ohat", "..x...x...x...x.")],
    );
    let d1 = with(
        &d0,
        &[
            ("hat", &mutate(r, &hat, 0.1, &['x', 'o', '.'])),
            ("ride", "..x...x...x...x."),
        ],
    );
    let nd = set_drums(&mut t, "d", vec![d0, d1]);
    let g1 = with(&groove, &[("hat", &mutate(r, &hat, 0.1, &['x', 'o', '.']))]);
    let ng = set_drums(&mut t, "g", vec![groove, g1]);
    put(
        &mut t.drums,
        "intro",
        sp(&[("kick", "x...x...x...x..."), ("hat", "..x...x...x...x.")]),
    );
    put(
        &mut t.drums,
        "brk",
        sp(&[("hat", &lane(16, |i| if i % 2 == 1 { "." } else { "o" }))]),
    );
    // The build: snare on the beats, then 8ths, 16ths, and the kick roll.
    put(
        &mut t.drums,
        "build",
        sp(&[
            (
                "kick",
                &format!(
                    "{}x...x...x...x...",
                    loop_(7, "x...x...x...x...", "x.......x.......")
                ),
            ),
            (
                "snare",
                &bars(8, |b| {
                    if b < 2 {
                        "....x.......x..."
                    } else if b < 4 {
                        "x...x...x...x..."
                    } else if b < 6 {
                        "x.x.x.x.x.x.x.x."
                    } else {
                        "xxxxxxxxxxxxxxxx"
                    }
                }),
            ),
            ("hat", "x.x.x.x.x.x.x.x."),
        ]),
    );

    let table: &[(&str, f64)] = &[
        (".r.r", 5.0),
        (".rr.", 1.0),
        (".rrr", 1.5),
        (".R.r", 1.5),
        ("..r.", 0.5),
    ];
    let bass_core = bars(4, |b| {
        replace_pickups(&bass_bar(r, table, 0.6, changes_after(&prog, b)))
    });
    let mut bass = Part {
        kind: "bass".into(),
        lo: Some(bass_lo(tonic)),
        ch: Ch {
            pump: Some(true),
            level: Some(1.4),
            ..Default::default()
        },
        pat: vec![],
        lab: p0("tranceBass"),
        ..Default::default()
    };
    let nb = set_pats(
        &mut bass,
        "b",
        variants(r, &bass_core, 2, 0.08, &['r', 'R', 'o', '.'], None),
    );
    put(&mut t.parts, "bass", bass);

    let arps = [
        "0 0 1 0 2 0 1 0 0 0 1 0 3 0 1 0",
        "0 1 2 1 3 1 2 1 0 1 2 1 4 1 2 1",
        "0 . 1 . 2 . 1 . 0 . 1 . 3 . 1 .",
        "0 0 2 0 0 2 0 0 2 0 0 2 0 0 1 0",
        "0 2 1 2 0 2 1 2 0 2 1 2 3 2 1 2",
    ];
    let swap_arp = |r: &mut R, s: &str, _i: usize| {
        s.split(' ')
            .map(|t| {
                if t != "." && r.chance(0.15) {
                    r.int(0, 4).to_string()
                } else {
                    t.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut arp = Part {
        kind: "arp".into(),
        lo: Some(64.0),
        res: Some(1.0),
        ch: Ch {
            rev: Some(0.35),
            dly: Some(0.45),
            pan: Some(-0.2),
            pump: Some(true),
            level: Some(0.8),
            ..Default::default()
        },
        pat: vec![],
        lab: p("pluckTrance", |l| l.gain = Some(0.5)),
        ..Default::default()
    };
    let arp_core = r.pick(&arps);
    let na = set_pats(
        &mut arp,
        "a",
        variants(r, arp_core, 2, 0.0, &[], Some(&swap_arp)),
    );
    put(&mut t.parts, "arp", arp);

    put(
        &mut t.parts,
        "pad",
        Part {
            kind: "chord".into(),
            lo: Some(55.0),
            ch: Ch {
                rev: Some(0.55),
                hp: Some(200.0),
                level: Some(1.8),
                pump: Some(true),
                ..Default::default()
            },
            pat: sp(&[("hold", HOLD)]),
            lab: p0("superPadWide"),
            ..Default::default()
        },
    );
    let m = motif(
        r,
        MotifOpts {
            bars: 2,
            density: Density::Medium,
            figure: 0.6,
            ..Default::default()
        },
    );
    let h = hooks(
        r,
        &m,
        HookOpts {
            scale,
            tonic: 60 + tonic,
            prog: &prog,
            lo: -1,
            hi: 9,
            gate: Some(8),
            ..Default::default()
        },
    );
    put(
        &mut t.parts,
        "lead",
        Part {
            kind: "mel".into(),
            res: Some(1.0),
            ch: Ch {
                rev: Some(0.45),
                dly: Some(0.5),
                hp: Some(300.0),
                pump: Some(true),
                level: Some(0.95),
                ..Default::default()
            },
            pat: h.pat(),
            lab: p("supersaw", |l| l.gain = Some(0.13)),
            ..Default::default()
        },
    );

    let mut sx: Vec<Section> = Vec::new();
    sx.push(Section {
        bars: 16,
        drums: s("intro"),
        lp: Some([500.0, 16000.0]),
        p: sp(&[("bass", "b0")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("g"),
        vd: Some(ng),
        crash: Some(true),
        fill: s("clap"),
        p: sp(&[("bass", "b"), ("arp", "a")]),
        v: vv(&[("bass", nb), ("arp", na)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("brk"),
        down: Some(2.0),
        auto: au(&[("pad.cutoff", [400.0, 1600.0])]),
        p: sp(&[("pad", "hold"), ("lead", "sparse")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("brk"),
        auto: au(&[
            ("lead.cutoff", [1500.0, 5000.0]),
            ("pad.cutoff", [1600.0, 3000.0]),
        ]),
        p: sp(&[("pad", "hold"), ("lead", "hook"), ("arp", "a0")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 8,
        drums: s("build"),
        riser: Some(8.0),
        swell: Some(true),
        fill: s("kickroll"),
        gap: Some(2.0),
        auto: au(&[("lead.cutoff", [3000.0, 8000.0])]),
        p: sp(&[("pad", "hold"), ("arp", "a0"), ("lead", "hook")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("d"),
        vd: Some(nd),
        crash: Some(true),
        drop: Some(true),
        fill: s("clap"),
        p: sp(&[
            ("bass", "b"),
            ("arp", "a"),
            ("lead", "hook"),
            ("pad", "hold"),
        ]),
        v: vv(&[("bass", nb), ("arp", na)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("d"),
        vd: Some(nd),
        fill: s("clap"),
        p: sp(&[
            ("bass", "b"),
            ("arp", "a"),
            ("lead", "answer"),
            ("pad", "hold"),
        ]),
        v: vv(&[("bass", nb), ("arp", na)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("g"),
        vd: Some(ng),
        p: sp(&[("bass", "b"), ("arp", "a")]),
        v: vv(&[("bass", nb), ("arp", na)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("intro"),
        lp: Some([16000.0, 400.0]),
        p: sp(&[("bass", "b0")]),
        ..Default::default()
    });
    t.sections = expand(&sx, r, dejavu);
    t
}

/// Trance's `.replace(/\.\.rn|\.\.n\./g, '.r.n')`: every leftmost match,
/// scanning on past it.
fn replace_pickups(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        if b[i..].starts_with(b"..rn") || b[i..].starts_with(b"..n.") {
            out.push_str(".r.n");
            i += 4;
        } else {
            out.push(b[i] as char);
            i += 1;
        }
    }
    out
}

// ── Eurobeat ─────────────────────────────────────────────────────
// The touge. Octave bass on the 8ths, gated snare, a bright saw lead with
// a dense chorus hook, brass hits, a 16th riff, and the final chorus a
// whole step up.
pub fn eurobeat(seed: u32, dejavu: Option<f64>) -> Track {
    let dejavu = dejavu.unwrap_or(0.8);
    let mut r = R::new(seed, 4);
    let r = &mut r;
    let tonic = r.int(0, 11);
    let bpm = r.int(152, 158);
    let scale = MINOR;
    const CHORUSES: &[&[Item]] = &[
        &[D(5), D(6), D(0), D(0)],
        &[D(5), D(6), D(0), D(6)],
        &[D(0), D(5), D(6), D(0)],
        &[D(3), D(6), D(0), D(0)],
        &[D(0), D(5), D(2), D(6)],
    ];
    let verse_degs = r.pick(PROGS.verse);
    let chorus_degs = r.pick(CHORUSES);
    let prog_v = progression(scale, tonic, verse_degs, "");
    let prog_c = progression(scale, tonic, chorus_degs, "");
    let up = (tonic + 2) % 12;
    let prog_c2 = progression(scale, up, chorus_degs, "");
    let mut t = Track::skeleton(&format!("eurobeat-{seed}"));
    t.title = Some(format!("Eurobeat {seed}"));
    t.style = Some(format!(
        "Eurobeat · {} minor · {} → {}",
        NOTE[tonic as usize], prog_c, NOTE[up as usize]
    ));
    t.bpm = bpm as f64;
    t.gain = 1.25;
    t.delay = 0.5;
    t.delay_fb = 0.28;
    t.pump = Pump {
        depth: 0.25,
        release: 0.14,
    };
    t.kit_name = s("tr909");
    t.kit = kit(&[
        ("kick", tweak(Some("kickTight"), None, None)),
        ("snare", tweak(Some("snareGated"), None, Some(0.35))),
        ("clap", tweak(None, None, Some(0.3))),
    ]);
    t.prog = sp(&[("v", &prog_v), ("c", &prog_c), ("c2", &prog_c2)]);
    t.lay = lay(&[
        ("clap", 0.4),
        ("ohat", 0.3),
        ("hat", 0.1),
        ("riff", 0.35),
        ("hits", 0.5),
        ("lead", 0.2),
        ("pad", 0.15),
    ]);

    let verse = sp(&[
        ("kick", "x...x...x...x..."),
        ("snare", "....X.......X..."),
        ("hat", "..x...x...x...x."),
    ]);
    let chorus = sp(&[
        ("kick", &loop_(8, "x...x...x...x...", "x...x...x...x.x.")),
        ("snare", "....X.......X..."),
        ("clap", "....x.......x..."),
        ("hat", "xoxoxoxoxoxoxoxo"),
        ("ohat", "..............x."),
    ]);
    let nc = set_drums(
        &mut t,
        "c",
        vec![
            chorus.clone(),
            with(
                &chorus,
                &[("hat", "x.x.x.x.x.x.x.x."), ("ohat", "......x.......x.")],
            ),
        ],
    );
    put(&mut t.drums, "v", verse);
    // the snare arrives with the verse
    put(
        &mut t.drums,
        "intro",
        sp(&[("kick", "x...x...x...x..."), ("hat", "x.x.x.x.x.x.x.x.")]),
    );
    put(
        &mut t.drums,
        "pre",
        sp(&[
            ("kick", "x...x...x...x..."),
            ("snare", "....X.......X..."),
            ("hat", "x.x.x.x.x.x.x.x."),
            ("clap", "....x.......x..."),
        ]),
    );
    put(
        &mut t.drums,
        "brk",
        sp(&[("hat", "..x...x...x...x."), ("shaker", "xoxoxoxoxoxoxoxo")]),
    );
    put(
        &mut t.drums,
        "build",
        sp(&[
            ("kick", "x...x...x...x..."),
            ("snare", &bars(8, |b| snare_build(b, false))),
            ("hat", "x.x.x.x.x.x.x.x."),
        ]),
    );

    let octave = "r.o.r.o.r.o.r.o.";
    let last = r.pick(&[
        "r.o.r.o.r.o.rr.n",
        "r.o.r.o.f.o.n.n.",
        "r.o.r.o.r.o.r.n.",
        "r.o.r.o.r.o.u.n.",
    ]);
    let mut bass = Part {
        kind: "bass".into(),
        lo: Some(bass_lo(tonic)),
        ch: Ch {
            pump: Some(true),
            level: Some(1.0),
            ..Default::default()
        },
        pat: vec![],
        lab: p("sawBass", |l| {
            l.cutoff = Some(320.0);
            l.fenv = Some(2.2);
        }),
        ..Default::default()
    };
    let b1_last = r.pick(&["r.o.r.o.r.o.f.n.", "r.o.r.o.rr.o.n.n"]);
    let nb = set_pats(
        &mut bass,
        "b",
        vec![
            loop_(4, octave, last),
            loop_(4, octave, b1_last),
            loop_(4, octave, octave),
        ],
    );
    put(&mut t.parts, "bass", bass);

    let riffs = [
        "0 1 2 1 0 1 2 1 0 1 3 1 0 1 2 1",
        "0 2 1 2 0 2 1 2 3 2 1 2 0 2 1 2",
        "0 0 1 1 2 2 1 1 0 0 1 1 3 3 1 1",
        "0 1 2 3 2 1 0 1 2 3 2 1 0 1 2 1",
    ];
    let mut riff = Part {
        kind: "arp".into(),
        lo: Some(64.0),
        res: Some(1.0),
        ch: Ch {
            rev: Some(0.25),
            dly: Some(0.3),
            pan: Some(0.25),
            pump: Some(true),
            level: Some(0.7),
            ..Default::default()
        },
        pat: vec![],
        lab: p("sqArp", |l| {
            l.cutoff = Some(3000.0);
            l.gain = Some(0.2);
        }),
        ..Default::default()
    };
    let riff_core = r.pick(&riffs);
    let swap_riff = |r: &mut R, s: &str, _i: usize| {
        s.split(' ')
            .map(|t| {
                if r.chance(0.2) {
                    r.int(0, 3).to_string()
                } else {
                    t.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    let nr = set_pats(
        &mut riff,
        "r",
        variants(r, riff_core, 1, 0.0, &[], Some(&swap_riff)),
    );
    put(&mut t.parts, "riff", riff);
    put(
        &mut t.parts,
        "hits",
        Part {
            kind: "chord".into(),
            lo: Some(57.0),
            ch: Ch {
                rev: Some(0.35),
                pump: Some(true),
                level: Some(0.8),
                ..Default::default()
            },
            pat: sp(&[(
                "h",
                &loop_(
                    2,
                    r.pick(&["x..x..x.........", "x.....x.x.......", "x..x............"]),
                    "x..x..x...x.x...",
                ),
            )]),
            lab: p0("hit"),
            ..Default::default()
        },
    );
    put(
        &mut t.parts,
        "pad",
        Part {
            kind: "chord".into(),
            lo: Some(57.0),
            ch: Ch {
                rev: Some(0.5),
                hp: Some(200.0),
                level: Some(1.1),
                pump: Some(true),
                ..Default::default()
            },
            pat: sp(&[("hold", HOLD)]),
            lab: p0("superPad"),
            ..Default::default()
        },
    );

    // Two motifs: the chorus hook, dense; the verse, calmer and lower.
    let mc = motif(
        r,
        MotifOpts {
            bars: 2,
            density: Density::Dense,
            figure: 0.7,
            ..Default::default()
        },
    );
    let mv = motif(
        r,
        MotifOpts {
            bars: 2,
            density: Density::Medium,
            figure: 0.5,
            ..Default::default()
        },
    );
    let hc = hooks(
        r,
        &mc,
        HookOpts {
            scale,
            tonic: 60 + tonic,
            prog: &prog_c,
            lo: -1,
            hi: 10,
            gate: Some(4),
            ..Default::default()
        },
    );
    let hc2 = hooks(
        r,
        &mc,
        HookOpts {
            scale,
            tonic: 60 + up,
            prog: &prog_c2,
            lo: -1,
            hi: 10,
            gate: Some(4),
            ..Default::default()
        },
    );
    let hv = hooks(
        r,
        &mv,
        HookOpts {
            scale,
            tonic: 60 + tonic,
            prog: &prog_v,
            lo: -3,
            hi: 6,
            gate: Some(6),
            ..Default::default()
        },
    );
    put(
        &mut t.parts,
        "lead",
        Part {
            kind: "mel".into(),
            res: Some(1.0),
            legato: Some(true),
            ch: Ch {
                rev: Some(0.3),
                dly: Some(0.3),
                hp: Some(300.0),
                level: Some(0.95),
                ..Default::default()
            },
            pat: sp(&[
                ("hook", &hc.hook),
                ("answer", &hc.answer),
                ("sparse", &hc.sparse),
                ("hook2", &hc2.hook),
                ("verse", &hv.hook),
                ("verse2", &hv.answer),
            ]),
            lab: p("euroLead", |l| l.gain = Some(0.09)),
            ..Default::default()
        },
    );

    let mut sx: Vec<Section> = Vec::new();
    sx.push(Section {
        bars: 8,
        prog: s("v"),
        drums: s("intro"),
        crash: Some(true),
        lp: Some([800.0, 16000.0]),
        p: sp(&[("riff", "r0"), ("bass", "b0")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        prog: s("v"),
        drums: s("v"),
        fill: s("snare"),
        p: sp(&[("bass", "b"), ("pad", "hold"), ("lead", "verse")]),
        v: vv(&[("bass", nb)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 8,
        prog: s("v"),
        drums: s("pre"),
        riser: Some(4.0),
        swell: Some(true),
        fill: s("tom"),
        gap: Some(1.0),
        p: sp(&[("bass", "b0"), ("pad", "hold"), ("hits", "h")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        prog: s("c"),
        drums: s("c"),
        vd: Some(nc),
        crash: Some(true),
        drop: Some(true),
        fill: s("snare"),
        p: sp(&[
            ("bass", "b"),
            ("lead", "hook"),
            ("riff", "r"),
            ("hits", "h"),
            ("pad", "hold"),
        ]),
        v: vv(&[("bass", nb), ("riff", nr)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        prog: s("v"),
        drums: s("v"),
        fill: s("snare"),
        p: sp(&[
            ("bass", "b"),
            ("pad", "hold"),
            ("lead", "verse2"),
            ("riff", "r0"),
        ]),
        v: vv(&[("bass", nb)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 8,
        prog: s("v"),
        drums: s("brk"),
        down: Some(2.0),
        auto: au(&[("pad.cutoff", [600.0, 2400.0])]),
        p: sp(&[("pad", "hold"), ("lead", "sparse")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 8,
        prog: s("v"),
        drums: s("build"),
        riser: Some(8.0),
        swell: Some(true),
        fill: s("roll"),
        gap: Some(2.0),
        p: sp(&[("bass", "b0"), ("pad", "hold"), ("hits", "h")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        prog: s("c"),
        drums: s("c"),
        vd: Some(nc),
        crash: Some(true),
        drop: Some(true),
        fill: s("snare"),
        p: sp(&[
            ("bass", "b"),
            ("lead", "hook"),
            ("riff", "r"),
            ("hits", "h"),
            ("pad", "hold"),
        ]),
        v: vv(&[("bass", nb), ("riff", nr)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        prog: s("c2"),
        drums: s("c"),
        vd: Some(nc),
        crash: Some(true),
        fill: s("snare"),
        p: sp(&[
            ("bass", "b"),
            ("lead", "hook2"),
            ("riff", "r"),
            ("hits", "h"),
            ("pad", "hold"),
        ]),
        v: vv(&[("bass", nb), ("riff", nr)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 8,
        prog: s("c2"),
        drums: s("intro"),
        lp: Some([16000.0, 600.0]),
        p: sp(&[("riff", "r0"), ("bass", "b0")]),
        ..Default::default()
    });
    t.sections = expand(&sx, r, dejavu);
    t
}

// ── Psytrance ────────────────────────────────────────────────────
// Full-on: the rolling bass between the kicks, all root, a 303 squelch under
// slow filter motion, FM zaps for the hook in phrygian, a dark pad in the
// break, odd-length percussion.
pub fn psytrance(seed: u32, dejavu: Option<f64>) -> Track {
    let dejavu = dejavu.unwrap_or(0.8);
    let mut r = R::new(seed, 5);
    let r = &mut r;
    let tonic = r.int(0, 11);
    let bpm = r.int(142, 146);
    let scale = PHRYGIAN;
    let prog = progression(scale, tonic, r.pick(PROGS.drone), "");
    let mut t = Track::skeleton(&format!("psy-{seed}"));
    t.title = Some(format!("Psytrance {seed}"));
    t.style = Some(format!(
        "Psytrance · {} phrygian · {}",
        NOTE[tonic as usize], prog
    ));
    t.bpm = bpm as f64;
    t.gain = 1.25;
    t.delay = 0.75;
    t.delay_fb = 0.42;
    t.pump = Pump {
        depth: 0.4,
        release: 0.1,
    };
    t.kit_name = s("tr909");
    t.kit = kit(&[
        ("kick", tweak(Some("kickTight"), Some(1.1), None)),
        ("clap", tweak(None, None, Some(0.35))),
    ]);
    t.prog = sp(&[("a", &prog)]);
    t.lay = lay(&[
        ("clap", 0.4),
        ("ohat", 0.3),
        ("rim", 0.5),
        ("snap", 0.6),
        ("hat", 0.1),
        ("acid", 0.3),
        ("lead", 0.35),
        ("pad", 0.2),
    ]);

    let hat = cells(r, &[("..x.", 5.0), (".ox.", 2.0), ("..xo", 2.0)], None);
    let kick8 = loop_(
        8,
        "x...x...x...x...",
        r.pick(&["x...x...x.......", "x...x...x...x..."]),
    );
    let rim = euclid(7, 16, r.int(0, 5), 'o');
    let snap_len = r.pick(&[6usize, 10]);
    let snap = lane(snap_len, |i| if i == 0 { "x" } else { "." });
    let g0 = sp(&[
        ("kick", &kick8),
        ("hat", &hat),
        ("ohat", "......x.......x."),
        ("rim", &rim),
    ]);
    // 'xoxoxoxoxoxoxoxo'.replace(/x/g, (c, i) => (i % 4 === 2 ? 'x' : 'o'))
    let g1_hat: String = "xoxoxoxoxoxoxoxo"
        .char_indices()
        .map(|(i, c)| {
            if c == 'x' {
                if i % 4 == 2 { 'x' } else { 'o' }
            } else {
                c
            }
        })
        .collect();
    let g1 = with(&g0, &[("hat", &g1_hat), ("snap", &snap)]);
    let ng = set_drums(&mut t, "g", vec![g0.clone(), g1.clone()]);
    let nd = set_drums(
        &mut t,
        "d",
        vec![
            with(&g0, &[("clap", "....x.......x...")]),
            with(&g1, &[("clap", "....x.......x...")]),
        ],
    );
    put(
        &mut t.drums,
        "intro",
        sp(&[("kick", "x...x...x...x..."), ("hat", &hat)]),
    );
    put(
        &mut t.drums,
        "brk",
        sp(&[
            ("hat", &lane(16, |i| if i % 2 == 1 { "." } else { "o" })),
            ("rim", &rim),
        ]),
    );
    put(
        &mut t.drums,
        "build",
        sp(&[
            (
                "kick",
                &format!(
                    "{}x...x...x...x...",
                    loop_(7, "x...x...x...x...", "x.......x.......")
                ),
            ),
            ("hat", "xxxxxxxxxxxxxxxx"),
            ("snare", &bars(8, |b| snare_build(b, true))),
        ]),
    );

    let lo = bass_lo(tonic);
    let beat = r.pick(&[".rrr", ".rrr", ".rr."]);
    let bass_core = loop_(
        4,
        &beat.repeat(4),
        &format!(
            "{}{}",
            beat.repeat(3),
            r.pick(&[".ooo", ".fff", ".rrr", ".rRr"])
        ),
    );
    let mut bass = Part {
        kind: "bass".into(),
        lo: Some(lo),
        ch: Ch {
            pump: Some(true),
            level: Some(1.0),
            ..Default::default()
        },
        pat: vec![],
        lab: p0("psyBass"),
        ..Default::default()
    };
    let nb = set_pats(
        &mut bass,
        "b",
        vec![
            bass_core,
            loop_(4, &beat.repeat(4), &format!("{}.ooo", ".rr.".repeat(3))),
            loop_(4, &beat.repeat(4), &format!("{}.r.r.rrr", beat.repeat(2))),
        ],
    );
    put(&mut t.parts, "bass", bass);

    let core = acid_line(
        r,
        0.7,
        &[
            ("r", 6.0),
            ("o", 3.0),
            ("f", 2.0),
            ("t", 2.0),
            ("s", 1.0),
            ("u", 1.0),
        ],
    );
    let mut acid = Part {
        kind: "bass".into(),
        lo: Some(lo + 12.0),
        ch: Ch {
            drive: Some(1.4),
            drive_lp: Some(8000.0),
            dly: Some(0.25),
            level: Some(0.7),
            ..Default::default()
        },
        pat: vec![],
        lab: p("acid", |l| {
            l.cutoff = Some(260.0);
            l.res = Some(0.84);
            l.env = Some(0.55);
            l.decay = Some(0.25);
        }),
        ..Default::default()
    };
    let na = set_pats(
        &mut acid,
        "a",
        variants(r, &core, 2, 0.12, &['r', 'o', 'R', 'f', '.', 't'], None),
    );
    put(&mut t.parts, "acid", acid);

    put(
        &mut t.parts,
        "pad",
        Part {
            kind: "chord".into(),
            lo: Some(48.0),
            ch: Ch {
                rev: Some(0.6),
                hp: Some(150.0),
                level: Some(0.8),
                pump: Some(true),
                ..Default::default()
            },
            pat: sp(&[("hold", HOLD)]),
            lab: p0("darkPad"),
            ..Default::default()
        },
    );
    let m = motif(
        r,
        MotifOpts {
            bars: 2,
            density: Density::Dense,
            figure: 0.5,
            ..Default::default()
        },
    );
    let h = hooks(
        r,
        &m,
        HookOpts {
            scale,
            tonic: 60 + tonic,
            prog: &prog,
            lo: -3,
            hi: 9,
            gate: Some(2),
            ..Default::default()
        },
    );
    put(
        &mut t.parts,
        "lead",
        Part {
            kind: "mel".into(),
            res: Some(1.0),
            ch: Ch {
                rev: Some(0.3),
                dly: Some(0.45),
                hp: Some(400.0),
                pan: Some(-0.15),
                level: Some(0.9),
                ..Default::default()
            },
            pat: h.pat(),
            lab: p("zap", |l| l.gain = Some(0.16)),
            ..Default::default()
        },
    );

    let mut sx: Vec<Section> = Vec::new();
    sx.push(Section {
        bars: 16,
        drums: s("intro"),
        lp: Some([250.0, 16000.0]),
        p: sp(&[("bass", "b0")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("g"),
        vd: Some(ng),
        crash: Some(true),
        auto: au(&[("acid.cutoff", [200.0, 600.0])]),
        p: sp(&[("bass", "b"), ("acid", "a")]),
        v: vv(&[("bass", nb), ("acid", na)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("g"),
        vd: Some(ng),
        fill: s("perc"),
        auto: au(&[("acid.cutoff", [600.0, 1400.0])]),
        p: sp(&[("bass", "b"), ("acid", "a"), ("lead", "sparse")]),
        v: vv(&[("bass", nb), ("acid", na)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("brk"),
        down: Some(2.0),
        auto: au(&[
            ("acid.cutoff", [1200.0, 2400.0]),
            ("acid.res", [0.84, 0.92]),
            ("pad.cutoff", [500.0, 1800.0]),
        ]),
        p: sp(&[("pad", "hold"), ("lead", "sparse"), ("acid", "a0")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 8,
        drums: s("build"),
        riser: Some(8.0),
        swell: Some(true),
        fill: s("kickroll"),
        gap: Some(2.0),
        p: sp(&[("bass", "b0"), ("pad", "hold"), ("acid", "a0")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("d"),
        vd: Some(nd),
        crash: Some(true),
        drop: Some(true),
        auto: au(&[("acid.cutoff", [1800.0, 900.0])]),
        p: sp(&[("bass", "b"), ("acid", "a"), ("lead", "hook")]),
        v: vv(&[("bass", nb), ("acid", na)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("d"),
        vd: Some(nd),
        fill: s("perc"),
        auto: au(&[("acid.cutoff", [900.0, 600.0])]),
        p: sp(&[
            ("bass", "b"),
            ("acid", "a"),
            ("lead", "answer"),
            ("pad", "hold"),
        ]),
        v: vv(&[("bass", nb), ("acid", na)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("g"),
        vd: Some(ng),
        auto: au(&[("acid.cutoff", [600.0, 300.0])]),
        p: sp(&[("bass", "b"), ("acid", "a")]),
        v: vv(&[("bass", nb), ("acid", na)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("intro"),
        lp: Some([16000.0, 300.0]),
        p: sp(&[("bass", "b0")]),
        ..Default::default()
    });
    t.sections = expand(&sx, r, dejavu);
    t
}

// ── Drum & bass ──────────────────────────────────────────────────
// Liquid: 2-step breaks with ghost snares, a sub and a reese holding long
// notes, 7th chords on EP and a wide pad, a pretty hook. Roller: darker,
// the bass on the 8ths, a minimal chord, the hook on a square lead.
pub fn dnb(seed: u32, dejavu: Option<f64>) -> Track {
    let dejavu = dejavu.unwrap_or(0.75);
    let mut r = R::new(seed, 6);
    let r = &mut r;
    let sub = if r.fork(1).chance(0.6) {
        "liquid"
    } else {
        "roller"
    };
    let liquid = sub == "liquid";
    let tonic = r.int(0, 11);
    let bpm = r.int(172, 176);
    let scale = if liquid {
        if r.chance(0.5) { DORIAN } else { MINOR }
    } else {
        MINOR
    };
    let degs = r.pick(&if liquid {
        no_dim(scale, PROGS.deep)
    } else {
        PROGS.verse.to_vec()
    });
    let ext = if liquid {
        r.pick(&["7", "9"])
    } else {
        r.pick(&["", "7"])
    };
    let prog = progression(scale, tonic, degs, ext);
    let mut t = Track::skeleton(&format!("dnb-{seed}"));
    t.title = Some(format!("Drum & bass {seed}"));
    t.style = Some(format!(
        "{} drum & bass · {} {} · {}",
        if liquid { "Liquid" } else { "Roller" },
        NOTE[tonic as usize],
        scale_name(scale),
        prog
    ));
    t.bpm = bpm as f64;
    t.gain = 1.2;
    t.delay = 0.75;
    t.delay_fb = 0.35;
    t.pump = Pump {
        depth: 0.3,
        release: 0.1,
    };
    t.kit_name = s("tr909");
    t.kit = kit(&[
        ("kick", tweak(Some("kickTight"), Some(1.2), None)),
        ("snare", tweak(Some("snareCrisp"), Some(1.25), Some(0.25))),
        ("hat", tweak(None, Some(0.9), None)),
    ]);
    t.prog = sp(&[("a", &prog)]);
    t.lay = lay(&[
        ("shaker", 0.5),
        ("ride", 0.7),
        ("hat", 0.1),
        ("ep", 0.3),
        ("lead", 0.4),
        ("pad", 0.15),
        ("reese", 0.25),
    ]);

    let kick4 = loop_(
        4,
        "x.........x.....",
        r.pick(&["x.........x..x..", "x.........x.....", "x.......x.x....."]),
    );
    let ghosts = r.pick(&[
        "....x..o....x..o",
        "....x.......x.o.",
        "....x..o....x...",
        "....x.....o.x..o",
    ]);
    let hat = r.pick(&["xoxoxoxoxoxoxoxo", "x.x.x.x.x.x.x.x.", "xox.xoxoxox.xoxo"]);
    let mut d0 = sp(&[("kick", &kick4), ("snare", ghosts), ("hat", hat)]);
    if liquid {
        put(&mut d0, "shaker", "oooooooooooooooo".to_owned());
    }
    let d1 = with(
        &d0,
        &[
            ("hat", &mutate(r, hat, 0.12, &['x', 'o', '.'])),
            ("ride", "x.x.x.x.x.x.x.x."),
        ],
    );
    let d2 = with(
        &d0,
        &[
            ("kick", &loop_(4, "x.........x.....", "x.........x..x.x")),
            ("snare", &mutate(r, ghosts, 0.1, &['o', '.', '.'])),
        ],
    );
    let nd = set_drums(&mut t, "d", vec![d0, d1, d2]);
    let intro = sp(&[
        ("hat", &lane(16, |i| if i % 2 == 1 { "." } else { "o" })),
        ("shaker", "oooooooooooooooo"),
    ]);
    put(
        &mut t.drums,
        "intro2",
        with(
            &intro,
            &[("kick", "x..............."), ("snare", "............x...")],
        ),
    );
    put(&mut t.drums, "intro", intro);
    put(
        &mut t.drums,
        "brk",
        sp(&[
            ("hat", &lane(16, |i| if i % 2 == 1 { "." } else { "o" })),
            ("shaker", "o.o.o.o.o.o.o.o."),
        ]),
    );
    put(
        &mut t.drums,
        "build",
        sp(&[
            ("kick", "x.........x....."),
            ("snare", &bars(8, |b| snare_build(b, true))),
            ("hat", "x.x.x.x.x.x.x.x."),
        ]),
    );

    let lo = bass_lo(tonic) - 12.0;
    let mut sub_part = Part {
        kind: "bass".into(),
        lo: Some(lo + 12.0),
        ch: Ch {
            pump: Some(false),
            level: Some(1.0),
            ..Default::default()
        },
        pat: vec![],
        lab: p0("sub"),
        ..Default::default()
    };
    let sub_core = bars(2, |b| {
        if b == 0 {
            "r---------o-----".to_owned()
        } else {
            format!(
                "r---------{}",
                if changes_after(&prog, 1) {
                    "n-----"
                } else {
                    "o-----"
                }
            )
        }
    });
    let ns = set_pats(
        &mut sub_part,
        "s",
        vec![
            sub_core,
            bars(2, |b| {
                if b == 0 {
                    "r-------........"
                } else {
                    "r---------f---n-"
                }
            }),
        ],
    );
    put(&mut t.parts, "sub", sub_part);
    if liquid {
        let mut reese = Part {
            kind: "bass".into(),
            lo: Some(lo + 12.0),
            ch: Ch {
                level: Some(0.42),
                hp: Some(60.0),
                pump: Some(true),
                ..Default::default()
            },
            pat: vec![],
            lab: p("reese", |l| l.drive = Some(1.4)),
            ..Default::default()
        };
        set_pats(
            &mut reese,
            "r",
            vec![
                loop_(2, "r---------------", "r---------------"),
                loop_(2, "r-------........", "r---------n-----"),
            ],
        );
        put(&mut t.parts, "reese", reese);
        put(
            &mut t.parts,
            "ep",
            Part {
                kind: "chord".into(),
                lo: Some(57.0),
                ch: Ch {
                    rev: Some(0.4),
                    dly: Some(0.3),
                    pump: Some(true),
                    level: Some(0.8),
                    ..Default::default()
                },
                pat: sp(&[(
                    "comp",
                    r.pick(&["x---.x--..x-....", "x-----.x------..", "x--.x--.x-......"]),
                )]),
                lab: p("ep", |l| l.gain = Some(0.14)),
                ..Default::default()
            },
        );
    } else {
        let mut reese = Part {
            kind: "bass".into(),
            lo: Some(lo + 12.0),
            ch: Ch {
                level: Some(0.42),
                hp: Some(60.0),
                pump: Some(true),
                drive: Some(1.2),
                drive_lp: Some(6000.0),
                ..Default::default()
            },
            pat: vec![],
            lab: p0("darkBass"),
            ..Default::default()
        };
        set_pats(
            &mut reese,
            "r",
            vec![
                loop_(2, "r.r..r.r.r.r..r.", "r.r..r.r.r.r.rn."),
                loop_(2, "r...r...r...r...", "r.r.r.r.r.r.r.n."),
            ],
        );
        put(&mut t.parts, "reese", reese);
        put(
            &mut t.parts,
            "ep",
            Part {
                kind: "chord".into(),
                lo: Some(60.0),
                ch: Ch {
                    rev: Some(0.5),
                    dly: Some(0.5),
                    pump: Some(true),
                    level: Some(0.7),
                    ..Default::default()
                },
                pat: sp(&[(
                    "comp",
                    r.pick(&["......x.........", "x.......x.......", "..x.......x....."]),
                )]),
                lab: p("dubChord", |l| l.gain = Some(1.4)),
                ..Default::default()
            },
        );
    }
    put(
        &mut t.parts,
        "pad",
        Part {
            kind: "chord".into(),
            lo: Some(60.0),
            ch: Ch {
                rev: Some(0.55),
                hp: Some(200.0),
                level: Some(0.8),
                pump: Some(true),
                ..Default::default()
            },
            pat: sp(&[("hold", HOLD)]),
            lab: if liquid {
                p0("superPadWide")
            } else {
                p0("darkPad")
            },
            ..Default::default()
        },
    );
    let m = motif(
        r,
        MotifOpts {
            bars: 2,
            density: if liquid {
                Density::Medium
            } else {
                Density::Dense
            },
            figure: 0.5,
            ..Default::default()
        },
    );
    let lead_patch = if liquid {
        r.pick(&[
            p("glass", |l| l.gain = Some(0.14)),
            p0("bell"),
            p("ep", |l| l.gain = Some(0.105)),
        ])
    } else {
        p0("sqLead")
    };
    let h = hooks(
        r,
        &m,
        HookOpts {
            scale,
            tonic: 60 + tonic,
            prog: &prog,
            lo: -2,
            hi: 9,
            gate: Some(if liquid { 6 } else { 3 }),
            ..Default::default()
        },
    );
    put(
        &mut t.parts,
        "lead",
        Part {
            kind: "mel".into(),
            res: Some(1.0),
            legato: Some(sub == "roller"),
            ch: Ch {
                rev: Some(0.45),
                dly: Some(0.4),
                hp: Some(300.0),
                pan: Some(0.15),
                level: Some(0.85),
                ..Default::default()
            },
            pat: h.pat(),
            lab: lead_patch,
            ..Default::default()
        },
    );

    let mut sx: Vec<Section> = Vec::new();
    sx.push(Section {
        bars: 16,
        drums: s("intro"),
        lp: Some([500.0, 16000.0]),
        p: sp(&[("pad", "hold"), ("ep", "comp")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("intro2"),
        p: sp(&[("pad", "hold"), ("ep", "comp"), ("lead", "sparse")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 8,
        drums: s("build"),
        riser: Some(8.0),
        swell: Some(true),
        fill: s("roll"),
        gap: Some(1.0),
        auto: au(&[("pad.cutoff", [800.0, 3000.0])]),
        p: sp(&[("pad", "hold"), ("ep", "comp"), ("lead", "hook")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("d"),
        vd: Some(nd),
        crash: Some(true),
        drop: Some(true),
        fill: s("dnb"),
        p: sp(&[
            ("sub", "s"),
            ("reese", "r0"),
            ("ep", "comp"),
            ("lead", "hook"),
        ]),
        v: vv(&[("sub", ns)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("d"),
        vd: Some(nd),
        fill: s("dnb"),
        p: sp(&[
            ("sub", "s"),
            ("reese", "r1"),
            ("ep", "comp"),
            ("lead", "answer"),
            ("pad", "hold"),
        ]),
        v: vv(&[("sub", ns)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("brk"),
        down: Some(2.0),
        auto: au(&[("pad.cutoff", [500.0, 2500.0])]),
        p: sp(&[("pad", "hold"), ("ep", "comp"), ("lead", "sparse")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 8,
        drums: s("build"),
        riser: Some(8.0),
        swell: Some(true),
        fill: s("roll"),
        gap: Some(1.0),
        p: sp(&[("pad", "hold"), ("ep", "comp"), ("lead", "hook")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("d"),
        vd: Some(nd),
        crash: Some(true),
        drop: Some(true),
        fill: s("dnb"),
        p: sp(&[
            ("sub", "s"),
            ("reese", "r0"),
            ("ep", "comp"),
            ("lead", "hook"),
            ("pad", "hold"),
        ]),
        v: vv(&[("sub", ns)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("d"),
        vd: Some(nd),
        fill: s("dnb"),
        p: sp(&[
            ("sub", "s"),
            ("reese", "r1"),
            ("ep", "comp"),
            ("lead", "answer"),
            ("pad", "hold"),
        ]),
        v: vv(&[("sub", ns)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("intro"),
        lp: Some([16000.0, 500.0]),
        p: sp(&[("pad", "hold"), ("ep", "comp")]),
        ..Default::default()
    });
    t.sections = expand(&sx, r, dejavu);
    t
}

// ── UK garage ────────────────────────────────────────────────────
// 2-step: the kick skips, the snare cracks on 2 and 4, skippy swung hats, an
// organ bass with pickups, chopped chords, vowel stabs from the choir.
pub fn garage(seed: u32, dejavu: Option<f64>) -> Track {
    let dejavu = dejavu.unwrap_or(0.7);
    let mut r = R::new(seed, 7);
    let r = &mut r;
    let tonic = r.int(0, 11);
    let bpm = r.int(132, 136);
    let scale = if r.chance(0.6) { DORIAN } else { MINOR };
    let degs = r.pick(&no_dim(scale, PROGS.deep));
    let ext = r.pick(&["7", "9"]);
    let prog = progression(scale, tonic, degs, ext);
    let prog_b = progression(scale, tonic, r.pick(&no_dim(scale, PROGS.verse)), "7");
    let mut t = Track::skeleton(&format!("garage-{seed}"));
    t.title = Some(format!("Garage {seed}"));
    t.style = Some(format!(
        "UK garage · {} {} · {}",
        NOTE[tonic as usize],
        scale_name(scale),
        prog
    ));
    t.bpm = bpm as f64;
    t.swing = Some(r.pick(&[0.18, 0.22, 0.25]));
    t.gain = 1.0;
    t.delay = 0.75;
    t.delay_fb = 0.3;
    t.pump = Pump {
        depth: 0.35,
        release: 0.14,
    };
    t.kit_name = s("tr909");
    t.kit = kit(&[
        ("kick", tweak(Some("kickTight"), None, None)),
        ("snare", tweak(Some("snareCrisp"), None, Some(0.3))),
        ("rim", tweak(None, None, Some(0.3))),
    ]);
    t.prog = sp(&[("a", &prog), ("b", &prog_b)]);
    t.lay = lay(&[
        ("shaker", 0.45),
        ("rim", 0.5),
        ("ohat", 0.3),
        ("hat", 0.1),
        ("vox", 0.5),
        ("lead", 0.4),
        ("chords", 0.2),
        ("pad", 0.15),
    ]);

    let kick = r.pick(&["x.....x...x.....", "x......x..x.....", "x.....x.x.x....."]);
    let hat = cells(
        r,
        &[
            ("x.xx", 3.0),
            ("x.x.", 3.0),
            (".x.x", 2.0),
            ("xx.x", 1.0),
            ("x..x", 1.0),
        ],
        None,
    );
    let g0 = sp(&[
        ("kick", &loop_(4, kick, &format!("{}x.x.", &kick[..12]))),
        ("snare", "....X.......X..."),
        ("hat", &hat),
        (
            "rim",
            r.pick(&["..o...o..o..o..o", "......o.......o.", "..o......o......"]),
        ),
        ("shaker", "o.o.o.o.o.o.o.o."),
    ]);
    let g1 = with(
        &g0,
        &[
            ("hat", &mutate(r, &hat, 0.12, &['x', 'o', '.'])),
            ("ohat", "......x.......x."),
        ],
    );
    let g2 = with(
        &g0,
        &[
            ("kick", &loop_(4, kick, kick)),
            ("rim", &euclid(5, 16, r.int(0, 3), 'o')),
        ],
    );
    let g0_shaker = get(&g0, "shaker").to_owned();
    let g0_rim = get(&g0, "rim").to_owned();
    let nd = set_drums(&mut t, "g", vec![g0, g1, g2]);
    put(
        &mut t.drums,
        "intro",
        sp(&[("hat", &hat), ("shaker", &g0_shaker)]),
    );
    put(
        &mut t.drums,
        "intro2",
        sp(&[
            ("kick", kick),
            ("hat", &hat),
            ("shaker", &g0_shaker),
            ("rim", &g0_rim),
        ]),
    );
    put(
        &mut t.drums,
        "brk",
        sp(&[
            ("hat", "..x...x...x...x."),
            ("shaker", &g0_shaker),
            ("rim", &g0_rim),
        ]),
    );
    put(
        &mut t.drums,
        "build",
        sp(&[
            ("kick", kick),
            ("snare", &bars(8, |b| snare_build(b, false))),
            ("hat", "x.x.x.x.x.x.x.x."),
        ]),
    );

    let table: &[(&str, f64)] = &[
        ("r..r", 3.0),
        (".r.r", 2.0),
        ("r...", 2.0),
        ("..r.", 2.0),
        ("r.o.", 1.0),
        ("....", 0.5),
    ];
    let bass_core = bars(4, |b| bass_bar(r, table, 0.8, changes_after(&prog, b)));
    let mut bass = Part {
        kind: "bass".into(),
        lo: Some(bass_lo(tonic)),
        ch: Ch {
            pump: Some(true),
            level: Some(0.8),
            ..Default::default()
        },
        pat: vec![],
        lab: p("pluckBass", |l| {
            l.cutoff = Some(300.0);
            l.res = Some(0.4);
            l.decay = Some(0.14);
            l.name = s("organ bass");
        }),
        ..Default::default()
    };
    let nb = set_pats(
        &mut bass,
        "b",
        variants(r, &bass_core, 2, 0.1, &['r', 'o', '.', '.'], None),
    );
    put(&mut t.parts, "bass", bass);

    let chop = r.pick(&["x.x..x..x...x.x.", "x..x..x...x.x...", ".x..x..x.x..x..."]);
    let mut chords = Part {
        kind: "chord".into(),
        lo: Some(58.0),
        gate: Some(0.6),
        ch: Ch {
            pump: Some(true),
            rev: Some(0.3),
            dly: Some(0.3),
            level: Some(0.4),
            ..Default::default()
        },
        pat: vec![],
        lab: if r.chance(0.5) { p0("organ") } else { p0("ep") },
        ..Default::default()
    };
    let nc = set_pats(
        &mut chords,
        "c",
        variants(
            r,
            &loop_(4, chop, &format!("{}{}", &chop[1..], &chop[..1])),
            2,
            0.1,
            &['x', '.'],
            None,
        ),
    );
    put(&mut t.parts, "chords", chords);
    put(
        &mut t.parts,
        "vox",
        Part {
            kind: "chord".into(),
            lo: Some(64.0),
            gate: Some(0.5),
            ch: Ch {
                rev: Some(0.45),
                dly: Some(0.35),
                pan: Some(-0.2),
                pump: Some(true),
                level: Some(0.8),
                ..Default::default()
            },
            pat: sp(&[(
                "stab",
                r.pick(&["..x...x.....x...", "......x.......x.", "..x.......x....."]),
            )]),
            lab: p("choir", |l| {
                l.a = Some(0.01);
                l.r = Some(0.2);
                l.vowel = s(r.pick(&["o", "e"]));
                l.name = s("vox");
            }),
            ..Default::default()
        },
    );
    put(
        &mut t.parts,
        "pad",
        Part {
            kind: "chord".into(),
            lo: Some(55.0),
            ch: Ch {
                pump: Some(true),
                rev: Some(0.5),
                hp: Some(180.0),
                level: Some(0.75),
                ..Default::default()
            },
            pat: sp(&[("hold", HOLD)]),
            lab: p0("warmPad"),
            ..Default::default()
        },
    );
    let m = motif(
        r,
        MotifOpts {
            bars: 2,
            density: Density::Sparse,
            figure: 0.5,
            ..Default::default()
        },
    );
    // The hook, and its answer realised over the verse progression it plays on.
    let h = hooks(
        r,
        &m,
        HookOpts {
            scale,
            tonic: 60 + tonic,
            prog: &prog,
            lo: -2,
            hi: 8,
            gate: Some(4),
            ..Default::default()
        },
    );
    let hb = hooks(
        r,
        &m,
        HookOpts {
            scale,
            tonic: 60 + tonic,
            prog: &prog_b,
            lo: -2,
            hi: 8,
            gate: Some(4),
            ..Default::default()
        },
    );
    let mut lead_pat = h.pat();
    lead_pat.push(("answerB".to_owned(), hb.answer));
    put(
        &mut t.parts,
        "lead",
        Part {
            kind: "mel".into(),
            res: Some(1.0),
            ch: Ch {
                rev: Some(0.4),
                dly: Some(0.4),
                pan: Some(0.2),
                pump: Some(true),
                hp: Some(300.0),
                level: Some(0.85),
                ..Default::default()
            },
            pat: lead_pat,
            lab: p("glass", |l| l.gain = Some(0.2)),
            ..Default::default()
        },
    );

    let mut sx: Vec<Section> = Vec::new();
    sx.push(Section {
        bars: 8,
        drums: s("intro"),
        lp: Some([600.0, 16000.0]),
        p: sp(&[("chords", "c0")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 8,
        drums: s("intro2"),
        p: sp(&[("bass", "b0"), ("chords", "c0")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("g"),
        vd: Some(nd),
        crash: Some(true),
        fill: s("skip"),
        p: sp(&[("bass", "b"), ("chords", "c"), ("pad", "hold")]),
        v: vv(&[("bass", nb), ("chords", nc)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 8,
        drums: s("brk"),
        down: Some(2.0),
        auto: au(&[("pad.cutoff", [600.0, 2400.0])]),
        p: sp(&[("pad", "hold"), ("lead", "sparse"), ("vox", "stab")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 8,
        drums: s("build"),
        riser: Some(8.0),
        swell: Some(true),
        fill: s("roll"),
        gap: Some(2.0),
        p: sp(&[("pad", "hold"), ("chords", "c0"), ("bass", "b0")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("g"),
        vd: Some(nd),
        crash: Some(true),
        drop: Some(true),
        fill: s("skip"),
        p: sp(&[
            ("bass", "b"),
            ("chords", "c"),
            ("lead", "hook"),
            ("vox", "stab"),
        ]),
        v: vv(&[("bass", nb), ("chords", nc)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("g"),
        vd: Some(nd),
        prog: s("b"),
        fill: s("skip"),
        p: sp(&[("bass", "b"), ("chords", "c"), ("lead", "answerB")]),
        v: vv(&[("bass", nb), ("chords", nc)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("brk"),
        down: Some(2.0),
        auto: au(&[("pad.cutoff", [500.0, 2800.0])]),
        p: sp(&[("pad", "hold"), ("lead", "hook"), ("vox", "stab")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 8,
        drums: s("build"),
        riser: Some(8.0),
        swell: Some(true),
        fill: s("roll"),
        gap: Some(2.0),
        p: sp(&[("pad", "hold"), ("chords", "c0")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("g"),
        vd: Some(nd),
        crash: Some(true),
        drop: Some(true),
        fill: s("skip"),
        p: sp(&[
            ("bass", "b"),
            ("chords", "c"),
            ("lead", "hook"),
            ("vox", "stab"),
            ("pad", "hold"),
        ]),
        v: vv(&[("bass", nb), ("chords", nc)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        drums: s("intro2"),
        lp: Some([16000.0, 500.0]),
        p: sp(&[("bass", "b0"), ("chords", "c0")]),
        ..Default::default()
    });
    t.sections = expand(&sx, r, dejavu);
    t
}

// ── Chicha ───────────────────────────────────────────────────────
// Peruvian cumbia of the late sixties on (sound.md 3.5.3). Costeña (Los
// Destellos, Los Mirlos): the surf guitar with tremolo carries the melody,
// two guitars in thirds in the chorus, a combo organ underneath. Amazónica
// (Juaneco y su Combo): the lead through a wah rocked once a beat, the
// organ up front. Under both: the cumbia bass (root on the beat, the fifth
// on the off-beat before the next), the güiro's long-short-short, congas
// and bongos, the timbales' cáscara and the bell in the chorus. The
// melodies are minor pentatonic (the huayno in it), the harmony i, iv and
// V7 vamps. Form: the guitar hook alone, verse, chorus, verse, an organ
// solo over the rhythm, the abanico back into the chorus, the chorus with
// the organ in unison, and the hook again to close.
pub fn chicha(seed: u32, dejavu: Option<f64>) -> Track {
    let dejavu = dejavu.unwrap_or(0.75);
    let mut r = R::new(seed, 8);
    let r = &mut r;
    let sub = if r.fork(1).chance(0.35) {
        "amazonica"
    } else {
        "costena"
    };
    let amazonica = sub == "amazonica";
    let tonic = r.int(0, 11);
    let bpm = if amazonica {
        r.int(90, 98)
    } else {
        r.int(96, 104)
    };
    let scale = MINOR;
    let prog_v = progression(scale, tonic, r.pick(PROGS.cumbia), "");
    const CHORUSES: &[&[Item]] = &[
        &[D(0), X(4, "dom"), D(0), X(4, "dom")],
        &[D(0), D(3), X(4, "dom"), D(0)],
        &[D(0), D(0), X(4, "dom"), X(4, "dom")],
        &[D(3), X(4, "dom"), D(0), D(0)],
    ];
    let prog_c = progression(scale, tonic, r.pick(CHORUSES), "");
    let mut t = Track::skeleton(&format!("chicha-{seed}"));
    t.title = Some(format!("Chicha {seed}"));
    t.style = Some(format!(
        "{} · {} minor · {}",
        if amazonica {
            "Cumbia amazónica"
        } else {
            "Chicha"
        },
        NOTE[tonic as usize],
        prog_c
    ));
    t.bpm = bpm as f64;
    t.gain = 1.1;
    t.delay = 0.5;
    t.delay_fb = 0.22;
    t.pump = Pump {
        depth: 0.08,
        release: 0.12,
    };
    t.kit_name = s("latin");
    t.prog = sp(&[("v", &prog_v), ("c", &prog_c)]);
    t.lay = lay(&[
        ("guiroS", 0.3),
        ("cascara", 0.45),
        ("shaker", 0.5),
        ("bongoH", 0.4),
        ("bongoL", 0.4),
        ("cowbell", 0.55),
        ("clave", 0.6),
        ("lead2", 0.35),
        ("rhythm", 0.2),
        ("organ", 0.15),
    ]);

    // Percussion. The güiro's stroke per beat: long on the beat, two shorts
    // after; the conga's open tones on the "and" of 2 and the end of the bar,
    // the slap on 2 and 4; the bongos' martillo in the verse; the cáscara and
    // the bell in the chorus.
    let guiro = sp(&[
        ("guiroL", "x...x...x...x..."),
        (
            "guiroS",
            r.pick(&["..xx..xx..xx..xx", ".x.x.x.x.x.x.x.x", "..xx..xx..xx.xxx"]),
        ),
    ]);
    let congas = sp(&[
        (
            "congaO",
            r.pick(&["......x.......xx", "......x.......x.", "..x...x.......xx"]),
        ),
        ("congaS", "....x.......x..."),
        ("tumba", r.pick(&["............x...", "............x..x"])),
    ]);
    let bongos = sp(&[
        ("bongoH", "o.x.o.x.o.x.o.x."),
        ("bongoL", r.pick(&["......x.......x.", "..x.......x....."])),
    ]);
    let kick = sp(&[("kick", r.pick(&["x.......x.......", "x.......x.....x."]))]);
    let cascara = r.pick(&["x.x.xx.x.x.xx.x.", "x.xx.x.xx.xx.x.x"]);
    let verse = merge(&[&kick, &guiro, &congas, &bongos]);
    let chorus = with(
        &merge(&[&kick, &guiro, &congas]),
        &[
            ("cascara", cascara),
            ("cowbell", "x...x...x...x..."),
            ("shaker", "x.x.x.x.x.x.x.x."),
        ],
    );
    let nv = set_drums(
        &mut t,
        "v",
        vec![
            verse.clone(),
            with(
                &verse,
                &[(
                    "congaO",
                    &mutate(r, get(&congas, "congaO"), 0.1, &['x', '.', '.']),
                )],
            ),
        ],
    );
    let nc = set_drums(
        &mut t,
        "c",
        vec![
            chorus.clone(),
            with(&chorus, &[("cowbell", "x..xx..xx..xx..x")]),
            with(
                &chorus,
                &[("cascara", &mutate(r, cascara, 0.1, &['x', '.']))],
            ),
        ],
    );
    put(
        &mut t.drums,
        "intro",
        with(&guiro, &[("clave", "x..x..x...x.x...")]),
    );
    put(
        &mut t.drums,
        "brk",
        with(&merge(&[&guiro, &congas]), &[("clave", "x..x..x...x.x...")]),
    );
    put(
        &mut t.drums,
        "pre",
        with(&merge(&[&kick, &guiro, &congas]), &[("cascara", cascara)]),
    );
    put(&mut t.drums, "coda", merge(&[&guiro, &congas]));

    // The bass: the tumbao, root on the beat and the fifth on the off-beat
    // before the next, a pickup into every change; the bordoneo (a repeated
    // root) now and then.
    let lo = bass_lo(tonic);
    let plain: &[(&str, f64)] = &[
        ("r.....f.r.....f.", 6.0),
        ("r.....o.r.....f.", 1.0),
        ("r..r..f.r.....f.", 1.5),
        ("r.....f.r..r..f.", 1.0),
    ];
    let pickup: &[(&str, f64)] = &[
        ("r.....f.r.....n.", 5.0),
        ("r.....f.r...r.n.", 1.0),
        ("r.....o.r.....n.", 1.0),
    ];
    let bass_over = |r: &mut R, prog: &str| {
        bars(4, |b| {
            r.weighted(if changes_after(prog, b) {
                pickup
            } else {
                plain
            })
        })
    };
    let mut bass = Part {
        kind: "bass".into(),
        lo: Some(lo),
        gate: Some(0.85),
        ch: Ch {
            pump: Some(true),
            level: Some(1.05),
            ..Default::default()
        },
        pat: vec![],
        lab: p0("fingerBass"),
        ..Default::default()
    };
    // `nb` is unused by the form (the verses carry `v: { bass: 2 }`).
    let b0 = bass_over(r, &prog_v);
    let b1 = bass_over(r, &prog_v);
    let b2 = bass_over(r, &prog_c);
    set_pats(&mut bass, "b", vec![b0, b1, b2]);
    put(&mut t.parts, "bass", bass);

    // The rhythm guitar's "chaka": muted strums on the off-beat 8ths, and a
    // 16th pair at the end of the phrase.
    let chaka = r.pick(&["..x...x...x...x.", "..x...x...x...xx", "..x..xx...x...x."]);
    let mut rhythm = Part {
        kind: "chord".into(),
        lo: Some(55.0),
        gate: Some(0.45),
        ch: Ch {
            rev: Some(0.15),
            pan: Some(0.3),
            level: Some(0.7),
            ..Default::default()
        },
        pat: vec![],
        lab: p0("rhythmGuitar"),
        ..Default::default()
    };
    let nr = set_pats(
        &mut rhythm,
        "r",
        variants(
            r,
            &loop_(4, chaka, &format!("{}x.xx", &chaka[..12])),
            1,
            0.1,
            &['x', '.'],
            None,
        ),
    );
    put(&mut t.parts, "rhythm", rhythm);

    // The organ: held chords under the verse, stabs on 2 and 4 in the chorus.
    put(
        &mut t.parts,
        "organ",
        Part {
            kind: "chord".into(),
            lo: Some(60.0),
            ch: Ch {
                rev: Some(0.3),
                pan: Some(-0.25),
                level: Some(if amazonica { 0.7 } else { 0.45 }),
                ..Default::default()
            },
            pat: sp(&[("pad", HOLD), ("stab", "....x---....x---")]),
            lab: p0("comboOrgan"),
            ..Default::default()
        },
    );

    // The guitars. The hook is the chorus; the verse has its own melody, lower
    // and calmer; the second guitar answers a third up, at the same time. The
    // costeña lead is the bright surf guitar or the same guitar on the neck
    // pickup (a forked draw, so the melodies of a seed stay as they were); the
    // second guitar is the same instrument.
    let guitar = if r.fork(2).chance(0.5) {
        "neckGuitar"
    } else {
        "surfGuitar"
    };
    let lead_patch = if amazonica {
        p("wahGuitar", |l| l.wah_rate = Some(bpm as f64 / 60.0))
    } else {
        p(guitar, |l| l.trem_rate = Some(r.pick(&[5.2, 5.8, 6.4])))
    };
    let pent: &[i32] = &[1, 5];
    let mc = motif(
        r,
        MotifOpts {
            bars: 2,
            density: Density::Medium,
            figure: 0.5,
            ..Default::default()
        },
    );
    let mv_density = r.pick(&[Density::Sparse, Density::Medium]);
    let mv = motif(
        r,
        MotifOpts {
            bars: 2,
            density: mv_density,
            figure: 0.4,
            ..Default::default()
        },
    );
    let hc = hooks(
        r,
        &mc,
        HookOpts {
            scale,
            tonic: 60 + tonic,
            prog: &prog_c,
            lo: -3,
            hi: 9,
            gate: Some(5),
            avoid: pent,
            ..Default::default()
        },
    );
    let hv = hooks(
        r,
        &mv,
        HookOpts {
            scale,
            tonic: 60 + tonic,
            prog: &prog_v,
            lo: -5,
            hi: 6,
            gate: Some(6),
            avoid: pent,
            ..Default::default()
        },
    );
    put(
        &mut t.parts,
        "lead",
        Part {
            kind: "mel".into(),
            res: Some(1.0),
            legato: Some(true),
            ch: Ch {
                rev: Some(0.4),
                dly: Some(0.2),
                pan: Some(0.1),
                level: Some(1.4),
                ..Default::default()
            },
            pat: sp(&[
                ("hook", &hc.hook),
                ("sparse", &hc.sparse),
                ("answer", &hc.answer),
                ("verse", &hv.hook),
                ("verse2", &hv.answer),
            ]),
            lab: lead_patch,
            ..Default::default()
        },
    );
    put(
        &mut t.parts,
        "lead2",
        Part {
            kind: "mel".into(),
            res: Some(1.0),
            legato: Some(true),
            ch: Ch {
                rev: Some(0.4),
                dly: Some(0.15),
                pan: Some(-0.3),
                level: Some(0.8),
                ..Default::default()
            },
            pat: sp(&[("third", &hc.answer)]),
            lab: p(guitar, |l| {
                l.trem = Some(0.3);
                l.trem_rate = Some(5.2);
                l.gain = Some(if guitar == "neckGuitar" { 0.12 } else { 0.26 });
            }),
            ..Default::default()
        },
    );
    // The organ solo: its own figure over the verse chords, pentatonic too.
    let mo = motif(
        r,
        MotifOpts {
            bars: 2,
            density: Density::Dense,
            figure: 0.6,
            ..Default::default()
        },
    );
    let ho = hooks(
        r,
        &mo,
        HookOpts {
            scale,
            tonic: 60 + tonic,
            prog: &prog_v,
            lo: -3,
            hi: 7,
            gate: Some(3),
            avoid: pent,
            ..Default::default()
        },
    );
    put(
        &mut t.parts,
        "organLead",
        Part {
            kind: "mel".into(),
            res: Some(1.0),
            ch: Ch {
                rev: Some(0.3),
                dly: Some(0.2),
                pan: Some(-0.2),
                level: Some(0.8),
                ..Default::default()
            },
            pat: ho.pat(),
            lab: p("comboOrgan", |l| l.gain = Some(0.15)),
            ..Default::default()
        },
    );

    let mut sx: Vec<Section> = Vec::new();
    sx.push(Section {
        bars: 8,
        prog: s("c"),
        drums: s("intro"),
        p: sp(&[("lead", "hook")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        prog: s("v"),
        drums: s("v"),
        vd: Some(nv),
        fill: s("abanico"),
        p: sp(&[
            ("bass", "b"),
            ("rhythm", "r"),
            ("organ", "pad"),
            ("lead", "verse"),
        ]),
        v: vv(&[("bass", 2), ("rhythm", nr)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        prog: s("c"),
        drums: s("c"),
        vd: Some(nc),
        crash: Some(true),
        drop: Some(true),
        p: sp(&[
            ("bass", "b2"),
            ("rhythm", "r"),
            ("organ", "stab"),
            ("lead", "hook"),
            ("lead2", "third"),
        ]),
        v: vv(&[("rhythm", nr)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        prog: s("v"),
        drums: s("v"),
        vd: Some(nv),
        fill: s("abanico"),
        p: sp(&[
            ("bass", "b"),
            ("rhythm", "r"),
            ("organ", "pad"),
            ("lead", "verse2"),
        ]),
        v: vv(&[("bass", 2), ("rhythm", nr)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        prog: s("v"),
        drums: s("brk"),
        p: sp(&[("bass", "b0"), ("organLead", "hook")]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 8,
        prog: s("v"),
        drums: s("pre"),
        fill: s("abanico"),
        p: sp(&[
            ("bass", "b0"),
            ("rhythm", "r0"),
            ("organ", "pad"),
            ("lead", "sparse"),
        ]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        prog: s("c"),
        drums: s("c"),
        vd: Some(nc),
        crash: Some(true),
        drop: Some(true),
        p: sp(&[
            ("bass", "b2"),
            ("rhythm", "r"),
            ("organ", "stab"),
            ("lead", "hook"),
            ("lead2", "third"),
        ]),
        v: vv(&[("rhythm", nr)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 16,
        prog: s("c"),
        drums: s("c"),
        vd: Some(nc),
        crash: Some(true),
        fill: s("abanico"),
        p: sp(&[
            ("bass", "b2"),
            ("rhythm", "r"),
            ("organ", "stab"),
            ("lead", "answer"),
            ("lead2", "third"),
            ("organLead", "answer"),
        ]),
        v: vv(&[("rhythm", nr)]),
        ..Default::default()
    });
    sx.push(Section {
        bars: 8,
        prog: s("c"),
        drums: s("coda"),
        p: sp(&[("bass", "b2"), ("lead", "sparse")]),
        ..Default::default()
    });
    t.sections = expand(&sx, r, dejavu);
    t
}

/// A genre: its key (`GENRES[key]`), display name (`GENRE_NAMES`) and
/// grammar (`make(seed, dejavu)`; `None` is the genre's default déjà vu).
pub struct Genre {
    pub key: &'static str,
    pub name: &'static str,
    pub make: fn(u32, Option<f64>) -> Track,
}

/// Every genre, in the lab's order.
pub const GENRES: &[Genre] = &[
    Genre {
        key: "house",
        name: "House",
        make: house,
    },
    Genre {
        key: "techno",
        name: "Techno",
        make: techno,
    },
    Genre {
        key: "trance",
        name: "Trance",
        make: trance,
    },
    Genre {
        key: "eurobeat",
        name: "Eurobeat",
        make: eurobeat,
    },
    Genre {
        key: "psytrance",
        name: "Psytrance",
        make: psytrance,
    },
    Genre {
        key: "dnb",
        name: "Drum & bass",
        make: dnb,
    },
    Genre {
        key: "garage",
        name: "UK garage",
        make: garage,
    },
    Genre {
        key: "chicha",
        name: "Chicha",
        make: chicha,
    },
];

/// A genre by key.
pub fn genre(key: &str) -> Option<&'static Genre> {
    GENRES.iter().find(|g| g.key == key)
}
