//! The sequencer against the lab: `parity/golden/music/events.json` holds,
//! for a few generated tracks, the events of `steps` 16ths from a given bar
//! as canonical JSON (one array per step).

use mp_music::genres::genre;
use mp_music::json::{Val, canon};
use mp_music::seq::Seq;

fn golden() -> serde_json::Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../parity/golden/music/events.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("events.json")).expect("json")
}

/// The first differing position of two texts, with context either side.
fn diff(ours: &str, theirs: &str) -> String {
    let a = ours.as_bytes();
    let b = theirs.as_bytes();
    let i = a
        .iter()
        .zip(b)
        .position(|(x, y)| x != y)
        .unwrap_or(a.len().min(b.len()));
    let cut = |s: &str| {
        let lo = i.saturating_sub(160).min(s.len());
        let hi = (i + 160).min(s.len());
        s[lo..hi].to_owned()
    };
    format!(
        "first difference at byte {i} (ours {} bytes, lab {} bytes)\n  ours: …{}…\n  lab:  …{}…",
        a.len(),
        b.len(),
        cut(ours),
        cut(theirs)
    )
}

#[test]
fn the_events_are_the_labs() {
    let runs = golden();
    let mut failures = Vec::new();
    let runs = runs.as_array().expect("runs");
    assert!(runs.len() >= 5, "{} runs", runs.len());
    for run in runs {
        let key = run["genre"].as_str().expect("genre");
        let seed = run["seed"].as_u64().expect("seed") as u32;
        let bar = run["bar"].as_u64().expect("bar") as u32;
        let steps = run["steps"].as_u64().expect("steps");
        let want = run["events"].as_str().expect("events");
        let t = (genre(key).unwrap_or_else(|| panic!("no genre {key}")).make)(seed, None);
        let mut s = Seq::new(t);
        s.seek_bar(bar);
        let mut out = Vec::new();
        for _ in 0..steps {
            out.push(Val::Arr(s.step().iter().map(|e| e.to_val()).collect()));
        }
        let text = canon(&Val::Arr(out));
        if text != want {
            failures.push(format!(
                "{key} seed {seed} bar {bar} x{steps}:\n{}",
                diff(&text, want)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
