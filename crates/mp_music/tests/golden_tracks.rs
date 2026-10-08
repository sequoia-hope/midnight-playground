//! The grammars against the lab: `parity/golden/music/tracks.json` holds
//! the SHA-256 of every genre's canonical track over a few seeds, and one
//! whole track per genre (seed 1) so a mismatch can be read, not just
//! detected.

use mp_music::genres::genre;
use sha2::{Digest, Sha256};

fn golden() -> serde_json::Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../parity/golden/music/tracks.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("tracks.json")).expect("json")
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
    let lo = i.saturating_sub(120);
    let cut = |s: &str| {
        let hi = (i + 120).min(s.len());
        let lo = (lo..=i).rev().find(|&k| s.is_char_boundary(k)).unwrap_or(0);
        let hi = (hi..=s.len())
            .find(|&k| s.is_char_boundary(k))
            .unwrap_or(s.len());
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
fn every_genre_and_seed_hashes_as_the_lab() {
    let g = golden();
    let seeds: Vec<u64> = g["seeds"]
        .as_array()
        .expect("seeds")
        .iter()
        .map(|s| s.as_u64().expect("a seed"))
        .collect();
    let mut failures = Vec::new();
    let mut checked = 0;
    for (key, hashes) in g["hashes"].as_object().expect("hashes") {
        let genre = genre(key).unwrap_or_else(|| panic!("no genre {key}"));
        for seed in &seeds {
            let t = (genre.make)(*seed as u32, None);
            let text = t.canon();
            let hash = format!("{:x}", Sha256::digest(text.as_bytes()));
            let want = hashes[&seed.to_string()].as_str().expect("a hash");
            checked += 1;
            if hash != want {
                let mut msg = format!("{key} seed {seed}: {hash} != {want}");
                if *seed == seeds[0] {
                    let sample = g["sample"][key].as_str().expect("a sample");
                    msg.push('\n');
                    msg.push_str(&diff(&text, sample));
                }
                failures.push(msg);
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(checked >= 8 * 6, "{checked} tracks checked");
}
