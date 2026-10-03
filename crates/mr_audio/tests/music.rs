//! `test/unit/music.test.js`, ported (SPEC 7.5): the notes, the song data,
//! the instruments and every song played start to finish on the null
//! backend in strict mode (the JS test's strict fake): nothing a browser
//! would reject, every part sounds in every section that names it, every
//! drum voice has a sample, and the playlist moves on.

use mr_audio::music::{Music, compile_track, kit_default, note_to_midi};
use mr_audio::timers::Timers;
use mr_audio::tracks::{LEVEL_TRACK, PLAYLIST, TRACKS, Track};
use mr_audio::wa::null::{self, NullHandle, NullOptions};
use mr_audio::wa::{AudioContext, ContextOptions};
use std::cell::RefCell;
use std::rc::Rc;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

fn make_music(log: bool) -> (AudioContext, NullHandle, Music) {
    let (ctx, h) = null::context(
        NullOptions {
            log,
            ..Default::default()
        },
        ContextOptions::default(),
    );
    ctx.set_strict(true);
    let out = ctx.create_gain();
    // An offline context to the music (no timers): as the JS test's fake.
    let m = Music::build(
        &ctx,
        &out,
        false,
        Rc::new(RefCell::new(mr_math::Mulberry32::new(1))),
    );
    (ctx, h, m)
}

#[test]
fn note_to_midi_scientific_pitch() {
    for (t, m) in [
        ("C4", 60.0),
        ("A4", 69.0),
        ("C#5", 73.0),
        ("Db5", 73.0),
        ("Bb3", 58.0),
        ("G#5", 80.0),
        ("E2", 40.0),
        ("C-1", 0.0),
        // The octave number belongs to the letter.
        ("Cb4", 59.0),
        ("B#3", 60.0),
    ] {
        assert_eq!(note_to_midi(t), Some(m), "{t}");
    }
    for bad in ["H4", "C", "4", "c4", "C##4", "", "C44"] {
        assert_eq!(note_to_midi(bad), None, "{bad}");
    }
}

#[test]
fn songs_playlist_and_level_tracks_agree() {
    let mut ids: Vec<&str> = TRACKS.iter().map(|t| t.id).collect();
    let n = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), n, "unique ids");
    let mut pl = PLAYLIST.to_vec();
    pl.sort();
    assert_eq!(pl, ids, "the playlist holds every song once");
    // The levels (mr_levels: sierra, coast, streets, desert, seaside, cruise).
    for level in ["sierra", "coast", "streets", "desert", "seaside", "cruise"] {
        let t = LEVEL_TRACK
            .iter()
            .find(|(l, _)| *l == level)
            .map(|(_, t)| *t);
        let t = t.unwrap_or_else(|| panic!("{level} has its own track"));
        assert!(ids.contains(&t), "{level}'s track {t} exists");
        assert_eq!(Music::level_track(level), t);
    }
    assert_eq!(Music::level_track("nope"), PLAYLIST[0]);
    // README, controls: "T  Next music track. There are seven; …"
    let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../README.md"));
    if let Ok(readme) = readme {
        let i = readme
            .find("Next music track. There are ")
            .expect("README says how many tracks");
        let word = readme[i + 28..].split(|c: char| !c.is_alphabetic()).next();
        let words = [
            ("five", 5),
            ("six", 6),
            ("seven", 7),
            ("eight", 8),
            ("nine", 9),
        ];
        let count = words
            .iter()
            .find(|(w, _)| Some(*w) == word)
            .map(|(_, n)| *n);
        assert_eq!(count, Some(TRACKS.len()));
    }
    assert_eq!(Music::tracks().len(), TRACKS.len());
}

fn chord_name_ok(n: &str) -> bool {
    // /^[A-G][#b]?[a-z0-9]*(\/[A-G][#b]?)?$/
    let (head, slash) = match n.split_once('/') {
        Some((h, s)) => (h, Some(s)),
        None => (n, None),
    };
    let note = |s: &str| -> Option<usize> {
        let b = s.as_bytes();
        if b.is_empty() || !(b'A'..=b'G').contains(&b[0]) {
            return None;
        }
        Some(if b.len() > 1 && (b[1] == b'#' || b[1] == b'b') {
            2
        } else {
            1
        })
    };
    let Some(k) = note(head) else { return false };
    if !head[k..]
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        return false;
    }
    match slash {
        None => true,
        Some(s) => note(s).is_some_and(|k| k == s.len()),
    }
}

#[test]
fn every_song_compiles_and_names_real_parts() {
    for t in TRACKS {
        assert!(!t.title.is_empty() && !t.style.is_empty());
        assert!((60.0..=200.0).contains(&t.bpm), "{} bpm", t.id);
        let c = compile_track(t);
        for (k, bars) in &c.prog {
            for bar in bars {
                for ch in bar {
                    assert!(chord_name_ok(ch.name), "chord '{}' in prog {k}", ch.name);
                }
            }
        }
        for (k, lanes) in &c.drums {
            for l in lanes {
                assert_eq!(l.steps.len() % 16, 0, "drums {k}.{}", l.voice);
                assert!(l.steps.iter().all(|c| "Xxo.".contains(*c)));
            }
        }
        assert!(!t.sections.is_empty());
        for (i, sec) in t.sections.iter().enumerate() {
            assert!(sec.bars > 0, "{} section {i}", t.id);
            assert!(c.prog.iter().any(|(k, _)| *k == sec.prog.unwrap_or("a")));
            if let Some(d) = sec.drums {
                assert!(c.drums.iter().any(|(k, _)| *k == d), "drums {d}");
            }
            for (part, key) in sec.p {
                let p = c.parts.iter().find(|p| p.part.name == *part);
                let p = p.unwrap_or_else(|| panic!("{} section {i}: part {part}", t.id));
                assert!(p.pats.iter().any(|(k, _)| k == key), "{part}.{key}");
            }
            if let Some(r) = sec.riser {
                assert!(r <= sec.bars, "riser fits");
            }
            if let Some(g) = sec.gap {
                assert!(g > 0 && g < 16, "gap");
            }
        }
    }
}

/// The oscillator types a run of the log set, in order (`'custom'` for a
/// periodic wave).
fn osc_types(lines: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for l in lines {
        let v: serde_json::Value = serde_json::from_str(l).unwrap();
        match v[1].as_str() {
            Some("set") if v[3] == "type" => {
                let t = v[4].as_str().unwrap_or("");
                if ["sine", "square", "sawtooth", "triangle"].contains(&t) {
                    out.push(t.to_owned());
                }
            }
            Some("setPeriodicWave") => out.push("custom".into()),
            _ => {}
        }
    }
    out
}

#[test]
fn every_patch_plays_a_valid_oscillator_type() {
    let (ctx, h, music) = make_music(true);
    let mut seen = Vec::new();
    for t in TRACKS {
        for p in t.parts {
            if !seen.contains(&p.inst) {
                seen.push(p.inst);
            }
        }
    }
    let mut n = 0;
    for p in &seen {
        if p.kind == "fm" {
            continue; // FM: sine carriers and modulators, no type set
        }
        let want = match p.kind {
            "saw" => "sawtooth",
            "tri" => "triangle",
            "square" => "square",
            "sine" => "sine",
            "pulse" => "custom",
            k => panic!("unknown patch type '{k}'"),
        };
        h.take_log();
        let g = ctx.create_gain();
        music.note(&g, 1.0, &[60.0], 0.5, p, 1.0, None);
        let mut expect = vec![want.to_owned(); p.voices.unwrap_or(1.0) as usize];
        if p.sub.is_some_and(|s| s != 0.0) {
            expect.push(p.sub_type.unwrap_or("sine").to_owned());
        }
        assert_eq!(osc_types(&h.take_log()), expect, "a '{}' patch", p.kind);
        n += 1;
    }
    assert!(n > 10, "{n} patches checked");
    assert!(ctx.problems().is_empty());
}

fn play_through(t: &'static Track) {
    let (ctx, h, mut music) = make_music(false);
    let started: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let s2 = started.clone();
    music.on_track = Some(Rc::new(move |info| s2.borrow_mut().push(info.id)));
    music.trace_parts = Some(Vec::new());
    let mut timers = Timers::default();
    music.play(t.id, 0, true, &mut timers);
    music.start(&mut timers);
    assert_eq!(music.current().map(|i| i.id), Some(t.id));
    let bars: u32 = t.sections.iter().map(|s| s.bars).sum();
    let length = (bars as f64 * 16.0 * 60.0) / t.bpm / 4.0;
    let mut now = 0.5;
    while now < length + 3.0 {
        h.set_now(now);
        music.pump_until(now + 0.2, &mut timers);
        now += 0.5;
    }
    assert!(ctx.problems().is_empty(), "nothing a browser would reject");
    let played = music.trace_parts.take().unwrap();
    for (i, sec) in t.sections.iter().enumerate() {
        for (name, _) in sec.p {
            assert!(
                played
                    .iter()
                    .any(|(id, s, p)| *id == t.id && *s == i && p == name),
                "{} section {i}: '{name}' played",
                t.id
            );
        }
    }
    // At the end the next song in the playlist starts.
    let i = PLAYLIST.iter().position(|p| *p == t.id).unwrap();
    let next = PLAYLIST[(i + 1) % PLAYLIST.len()];
    assert_eq!(*started.borrow(), vec![t.id, next]);
    assert_eq!(music.current().map(|i| i.id), Some(next));
}

#[test]
fn every_song_plays_start_to_finish() {
    for t in TRACKS {
        play_through(t);
    }
}

#[test]
fn every_drum_voice_has_a_sample() {
    let (_ctx, _h, music) = make_music(false);
    for t in TRACKS {
        for (_, lanes) in t.drums {
            for (voice, _) in *lanes {
                let (s, _, _) = kit_default(voice).expect("a kit voice");
                let s = t
                    .kit
                    .iter()
                    .find(|(k, _)| k == voice)
                    .and_then(|(_, k)| k.s)
                    .unwrap_or(s);
                assert!(music.kit_buffer(s).is_some(), "{}: {voice} ({s})", t.id);
            }
        }
    }
}

#[test]
fn next_walks_the_playlist_and_play_keeps_a_song_that_is_on() {
    let (_ctx, _h, mut music) = make_music(false);
    let mut timers = Timers::default();
    music.play(PLAYLIST[0], 0, true, &mut timers);
    assert_eq!(music.info().map(|i| i.id), Some(PLAYLIST[0]), "queued");
    assert_eq!(music.next(&mut timers).map(|i| i.id), Some(PLAYLIST[1]));
    music.start(&mut timers);
    assert_eq!(music.current().map(|i| i.id), Some(PLAYLIST[1]));
    let serial = music.song_serial();
    music.play(PLAYLIST[1], 0, true, &mut timers);
    assert_eq!(music.song_serial(), serial, "the same song carries on");
    let mut id = PLAYLIST[1];
    for _ in 0..PLAYLIST.len() {
        id = music.next(&mut timers).unwrap().id;
    }
    assert_eq!(id, PLAYLIST[1], "wraps round");
    music.stop(&mut timers);
    assert!(!music.on);
}
