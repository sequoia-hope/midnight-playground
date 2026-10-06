//! `test/unit/radio.test.js` (roadmap WP 5.7, L1): Hot Pursuit's radio
//! voice. Every line dispatch can say, on every level, has its recordings in
//! `audio/radio/` (so none falls back to the burble), the clip list agrees
//! with what the lines say, callsigns stay inside the set that was
//! recorded, and `RadioVoice` picks, fetches and decodes the takes.
//!
//! The recordings check reads `audio/radio/` from disk, so it is native
//! only; the rest runs in wasm too.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use mp_audio::radio::lines::{self as radio, Clip, DIRS, LevelRadio, Line, TAKES};
use mp_audio::radio::{Bytes, Fetch, RadioVoice, Random, clip_id};
use mp_audio::wa::null::{self, ClipInfo, NullOptions};
use mp_audio::wa::{AudioBuffer, AudioContext, ContextOptions, ContextState, Pending};
use mp_sim::pursuit::{CALLSIGNS, callsign};
use mp_track::Level;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

fn levels() -> Vec<Level> {
    mp_levels::levels()
}

fn level_radio(l: &Level) -> LevelRadio<'_> {
    LevelRadio {
        police: l.police.is_some(),
        zones: l.zones.iter().map(|z| z.name).collect(),
        rivals: l.rivals.iter().map(|r| r.name).collect(),
    }
}

/// `levelsRadioClips(LEVELS)`.
fn all_clips() -> Vec<Clip> {
    let ls = levels();
    let lr: Vec<LevelRadio> = ls.iter().map(level_radio).collect();
    radio::levels_radio_clips(&lr, &CALLSIGNS)
}

/// Everything PursuitView can say, with every value filled in.
fn every_line() -> Vec<Line> {
    let ls = levels();
    let police: Vec<&Level> = ls.iter().filter(|l| l.police.is_some()).collect();
    let zones: Vec<String> = police
        .iter()
        .flat_map(|l| l.zones.iter().map(|z| radio::place_name(z.name)))
        .collect();
    let mut names: Vec<&str> = Vec::new();
    for l in &police {
        for r in &l.rivals {
            if !names.contains(&r.name) {
                names.push(r.name);
            }
        }
    }
    let mut out = Vec::new();
    for z in &zones {
        for d in DIRS {
            out.push(radio::pursuit(d, z));
        }
    }
    for z in &zones {
        for &u in &CALLSIGNS {
            out.push(radio::intercept(u, z));
        }
    }
    for &u in &CALLSIGNS {
        out.push(radio::joining(u));
        out.push(radio::unit_down(u));
    }
    for h in [2, 3, 4, 5] {
        out.push(radio::heat(h));
    }
    for n in &names {
        out.push(radio::rival_busted(n));
    }
    out.extend([
        radio::spotted(),
        radio::lost(),
        radio::escaped(),
        radio::roadblock(true),
        radio::roadblock(false),
        radio::spikes(),
        radio::spiked(),
        radio::busted(),
        radio::wrecked(),
    ]);
    out
}

#[test]
fn names_clip_ids_are_the_words_place_names_are_title_case() {
    assert_eq!(
        clip_id("Unit 23, speeder on Nob Hill!"),
        "unit-23-speeder-on-nob-hill"
    );
    assert_eq!(
        clip_id("Bring in everything we've got."),
        "bring-in-everything-we-ve-got"
    );
    assert_eq!(radio::place_name("INTERSTATE 9"), "Interstate 9");
    assert_eq!(radio::place_name("OLD MILL VALLEY"), "Old Mill Valley");
}

#[test]
fn the_clip_list_covers_every_part_of_every_line_once_and_nothing_else() {
    let all = all_clips();
    let ids: BTreeSet<String> = all.iter().map(|c| c.id.clone()).collect();
    let said: BTreeSet<String> = every_line()
        .iter()
        .flat_map(|l| l.parts.iter().map(|p| clip_id(p)))
        .collect();
    assert_eq!(said, ids);
    assert_eq!(ids.len(), all.len(), "no clip listed twice");
    for l in every_line() {
        assert!(
            !l.parts.is_empty() && !l.text.is_empty(),
            "a line has text and something to say"
        );
    }
}

#[test]
fn the_intercept_call_is_the_callsign_then_the_message() {
    let l = radio::intercept(17, "Nob Hill");
    assert_eq!(l.text, "Unit 17, speeder on Nob Hill, moving to intercept.");
    assert_eq!(
        l.parts,
        ["Unit 17.", "Speeder on Nob Hill, moving to intercept."]
    );
}

#[test]
fn lines_with_nothing_filled_in_get_extra_takes() {
    let all = all_clips();
    let find = |l: Line| {
        let id = clip_id(&l.text);
        all.iter().find(|c| c.id == id).unwrap().takes
    };
    assert_eq!(find(radio::spikes()), TAKES);
    assert_eq!(find(radio::joining(12)), 1);
}

struct Fixed(f64);

impl mp_math::Rng for Fixed {
    fn next_f64(&mut self) -> f64 {
        self.0
    }
}

#[test]
fn every_callsign_a_unit_can_get_was_recorded() {
    for i in 0..7 {
        for r in [0.0, 0.5, 0.9999] {
            assert!(CALLSIGNS.contains(&callsign(i, &mut Fixed(r))), "unit {i}");
        }
    }
    assert_eq!([CALLSIGNS[0], *CALLSIGNS.last().unwrap()], [10, 30]);
}

#[test]
fn a_levels_preload_list_is_a_subset_of_the_recordings() {
    let ls = levels();
    let police: Vec<&Level> = ls.iter().filter(|l| l.police.is_some()).collect();
    assert!(police.len() >= 4 && !police.iter().any(|l| l.id == "cruise"));
    let ids: BTreeSet<String> = all_clips().into_iter().map(|c| c.id).collect();
    for l in police {
        let zones: Vec<&str> = l.zones.iter().map(|z| z.name).collect();
        let names: Vec<&str> = l.rivals.iter().map(|r| r.name).collect();
        for c in radio::radio_clips(&zones, &[11, 16], &names) {
            assert!(ids.contains(&c.id), "{}", c.id);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn audio_radio_has_every_take_of_every_clip_and_index_json_lists_them() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../audio/radio");
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("index.json")).unwrap()).unwrap();
    let all = all_clips();
    let mut missing = Vec::new();
    for c in &all {
        let n = index["clips"][&c.id].as_u64().unwrap_or(0) as u32;
        if n != c.takes {
            missing.push(format!("{} ({n}/{} in index.json)", c.id, c.takes));
        }
        for t in 1..=c.takes {
            let f = if t > 1 {
                format!("{}.{t}.mp3", c.id)
            } else {
                format!("{}.mp3", c.id)
            };
            let ok = std::fs::metadata(dir.join(&f)).is_ok_and(|m| m.len() >= 1000);
            if !ok {
                missing.push(f);
            }
        }
    }
    assert!(
        missing.is_empty(),
        "record them with tools/radio-voice/render.py: {missing:?}"
    );
    let ids: BTreeSet<&str> = all.iter().map(|c| c.id.as_str()).collect();
    let extra: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|f| f.ends_with(".mp3"))
        .filter(|f| {
            // `f.replace(/(\.\d)?\.mp3$/, '')`
            let stem = f.strip_suffix(".mp3").unwrap();
            let b = stem.as_bytes();
            let stem = if b.len() >= 2 && b[b.len() - 2] == b'.' && b[b.len() - 1].is_ascii_digit()
            {
                &stem[..stem.len() - 2]
            } else {
                stem
            };
            !ids.contains(stem)
        })
        .collect();
    assert!(extra.is_empty(), "clips no line says any more: {extra:?}");
}

// ── A RadioVoice over a fake fetch and a fake decoder ──────────────

/// The fake `fetch`: `index.json` lists `clips`; any other file's bytes are
/// its name. Every fetched name is recorded. `ok: false` fails every fetch.
struct FakeFetch {
    clips: String,
    ok: bool,
    got: RefCell<Vec<String>>,
}

impl Fetch for FakeFetch {
    fn fetch(&self, file: &str) -> Pending<Bytes> {
        self.got.borrow_mut().push(file.to_owned());
        let p = Pending::new();
        let body = if !self.ok {
            None
        } else if file == "index.json" {
            Some(Rc::new(
                format!("{{\"voice\": \"test\", \"clips\": {}}}", self.clips).into_bytes(),
            ))
        } else {
            Some(Rc::new(file.as_bytes().to_vec()))
        };
        p.resolve(Ok(body));
        p
    }
}

/// The files a decoded buffer can be, told apart by their length.
const FILES: [&str; 4] = [
    "unit-17.mp3",
    "suspect-in-custody.mp3",
    "suspect-in-custody.2.mp3",
    "spike-strip-deployed.mp3",
];

fn ctx() -> AudioContext {
    let (ctx, h) = null::context(
        NullOptions {
            sample_rate: 48000.0,
            log: false,
            state: ContextState::Running,
        },
        ContextOptions::default(),
    );
    h.set_decoder(|bytes| {
        let name = String::from_utf8(bytes.to_vec()).ok()?;
        let i = FILES.iter().position(|f| *f == name)?;
        Some(ClipInfo {
            name,
            channels: 1,
            length: 1000 + i as u32,
        })
    });
    ctx
}

fn fake(clips: &str, ok: bool) -> Rc<FakeFetch> {
    Rc::new(FakeFetch {
        clips: clips.into(),
        ok,
        got: RefCell::new(Vec::new()),
    })
}

fn rng(r: f64) -> Random {
    Rc::new(RefCell::new(Fixed(r)))
}

/// Settles the context's promises until `p` has settled.
fn wait<T: Clone + 'static>(ctx: &AudioContext, p: &Pending<T>) -> T {
    for _ in 0..100 {
        if let Some(r) = p.result() {
            return r.expect("resolved");
        }
        ctx.settle();
    }
    panic!("never settled");
}

fn names(bufs: &Option<Vec<AudioBuffer>>) -> Option<Vec<&'static str>> {
    bufs.as_ref().map(|v| {
        v.iter()
            .map(|b| FILES[(b.length() - 1000) as usize])
            .collect()
    })
}

fn parts(p: &[&str]) -> Vec<String> {
    p.iter().map(|s| s.to_string()).collect()
}

#[test]
fn radio_voice_a_line_decodes_one_take_of_each_part_in_order() {
    let ctx = ctx();
    let f = fake(r#"{"unit-17": 1, "suspect-in-custody": 2}"#, true);
    let v = RadioVoice::new(f.clone());
    let p = v.buffers(
        &ctx,
        &parts(&["Unit 17.", "Suspect in custody."]),
        rng(0.99),
    );
    assert_eq!(
        names(&wait(&ctx, &p)),
        Some(vec!["unit-17.mp3", "suspect-in-custody.2.mp3"])
    );
    let p = v.buffers(&ctx, &parts(&["Suspect in custody."]), rng(0.0));
    assert_eq!(names(&wait(&ctx, &p)), Some(vec!["suspect-in-custody.mp3"]));
    let p = v.buffers(&ctx, &parts(&["Unit 17.", "Never recorded."]), rng(0.5));
    assert!(wait(&ctx, &p).is_none(), "a missing part: no voice");
}

#[test]
fn radio_voice_prefetch_fetches_every_take_once_and_a_line_then_needs_no_new_fetches() {
    let ctx = ctx();
    let f = fake(r#"{"spike-strip-deployed": 2, "unit-12": 1}"#, true);
    let v = RadioVoice::new(f.clone());
    let ids = parts(&["spike-strip-deployed", "unit-12", "not-recorded"]);
    let p = v.prefetch(&ids);
    wait(&ctx, &p);
    let mut got = f.got.borrow().clone();
    got.sort();
    assert_eq!(
        got,
        [
            "index.json",
            "spike-strip-deployed.2.mp3",
            "spike-strip-deployed.mp3",
            "unit-12.mp3"
        ]
    );
    let n = f.got.borrow().len();
    for _ in 0..2 {
        // The fake decoder knows take 1 only; any take needs no new fetch.
        let p = v.buffers(&ctx, &parts(&["Spike strip deployed."]), rng(0.0));
        wait(&ctx, &p);
    }
    assert_eq!(f.got.borrow().len(), n);
}

#[test]
fn radio_voice_without_index_json_there_is_no_voice_and_nothing_throws() {
    let ctx = ctx();
    let f = fake("{}", false);
    let v = RadioVoice::new(f);
    let p = v.buffers(&ctx, &parts(&["Suspect in custody."]), rng(0.5));
    assert!(wait(&ctx, &p).is_none());
    let p = v.prefetch(&parts(&["suspect-in-custody"]));
    wait(&ctx, &p);
}
