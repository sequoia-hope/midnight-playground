//! Police radio chatter in Hot Pursuit (`src/game/audio/radioLines.js`):
//! what dispatch says for each event, as the text on the HUD and the
//! recorded clips that speak it.
//!
//! A line is `{ text, parts }`: parts are what's spoken, one clip each, in
//! order (usually just the text; a callsign can be its own clip so it
//! doesn't multiply every sentence it starts). A clip is named after its
//! words ([`clip_id`]), so the recordings in `audio/radio/` can't drift from
//! the text: change a line and its old clip simply stops matching, and the
//! radio falls back to the synthesised burble until `render.py` records it.
//!
//! The JS imports `CALLSIGNS` from `Pursuit.js` as `radioClips`' default
//! units; `mp_audio` may not depend on the simulation (SPEC 3.2), so the
//! callsigns are an argument here (`mp_sim::pursuit::CALLSIGNS` is the
//! default the callers pass).

use super::clip_id;

/// `DIRS`: the headings dispatch names, east first, counter-clockwise.
pub const DIRS: [&str; 8] = [
    "east",
    "southeast",
    "south",
    "southwest",
    "west",
    "northwest",
    "north",
    "northeast",
];

/// Lines with nothing filled in come up in every pursuit, so they get a
/// second take to keep them from sounding canned (`TAKES`).
pub const TAKES: u32 = 2;

/// A zone's name as dispatch says it: SIERRA PASS → Sierra Pass
/// (`name.toLowerCase().replace(/\b\w/g, (c) => c.toUpperCase())`).
pub fn place_name(name: &str) -> String {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut out = String::with_capacity(name.len());
    let mut prev_word = false;
    for c in name.to_lowercase().chars() {
        let w = word(c);
        if w && !prev_word {
            out.push(c.to_ascii_uppercase());
        } else {
            out.push(c);
        }
        prev_word = w;
    }
    out
}

/// A line: the text on the HUD and the parts the voice says, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    pub parts: Vec<String>,
}

/// `line(text)`: one part, the text itself.
fn line(text: String) -> Line {
    Line {
        parts: vec![text.clone()],
        text,
    }
}

/// `RADIO.pursuit(heading, zone)`.
pub fn pursuit(heading: &str, zone: &str) -> Line {
    line(format!(
        "All units, suspect heading {heading} on {zone}. Pursuit is on."
    ))
}

/// `RADIO.spotted()`.
pub fn spotted() -> Line {
    line("Visual on the suspect again, closing in.".into())
}

/// `RADIO.lost()`.
pub fn lost() -> Line {
    line("Lost visual. All units, search the area.".into())
}

/// `RADIO.escaped()`.
pub fn escaped() -> Line {
    line("We lost the suspect. All units, resume patrol.".into())
}

/// `RADIO.heat(heat)`.
pub fn heat(heat: i32) -> Line {
    line(if heat >= 4 {
        format!("Heat level {heat}. Bring in everything we've got.")
    } else {
        format!("Heat level {heat}. Requesting more units.")
    })
}

/// `RADIO.intercept(unit, zone)`: the callsign, then the message.
pub fn intercept(unit: i32, zone: &str) -> Line {
    Line {
        text: format!("Unit {unit}, speeder on {zone}, moving to intercept."),
        parts: vec![
            format!("Unit {unit}."),
            format!("Speeder on {zone}, moving to intercept."),
        ],
    }
}

/// `RADIO.joining(unit)`.
pub fn joining(unit: i32) -> Line {
    line(format!("Unit {unit} joining the pursuit."))
}

/// `RADIO.unitDown(unit)`.
pub fn unit_down(unit: i32) -> Line {
    line(format!("Unit {unit} is down! Unit down!"))
}

/// `RADIO.roadblock(heavy)`.
pub fn roadblock(heavy: bool) -> Line {
    line(if heavy {
        "Heavy roadblock in position. Nobody gets through.".into()
    } else {
        "Roadblock set up ahead. Suspect is heading right for it.".into()
    })
}

/// `RADIO.spikes()`.
pub fn spikes() -> Line {
    line("Spike strip deployed.".into())
}

/// `RADIO.spiked()`.
pub fn spiked() -> Line {
    line("Suspect hit the spikes!".into())
}

/// `RADIO.busted()`.
pub fn busted() -> Line {
    line("Suspect in custody.".into())
}

/// `RADIO.rivalBusted(name)`.
pub fn rival_busted(name: &str) -> Line {
    line(format!("{name} is in custody."))
}

/// `RADIO.wrecked()`.
pub fn wrecked() -> Line {
    line("Suspect vehicle is totalled. Tow it back onto the road.".into())
}

/// A clip the radio can play: `{ id, text, takes }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clip {
    pub id: String,
    pub text: String,
    pub takes: u32,
}

/// `radioClips({ zones, units, names })`: every clip the radio can play for
/// the given zones (display names), callsigns and rival names, in the JS's
/// order: all of them for the recording tool, one level's worth to preload
/// in a race.
pub fn radio_clips(zones: &[&str], units: &[i32], names: &[&str]) -> Vec<Clip> {
    let places: Vec<String> = zones.iter().map(|z| place_name(z)).collect();
    let mut lines: Vec<Line> = Vec::new();
    for z in &places {
        for d in DIRS {
            lines.push(pursuit(d, z));
        }
    }
    for z in &places {
        for &u in units {
            lines.push(intercept(u, z));
        }
    }
    for &u in units {
        lines.push(joining(u));
        lines.push(unit_down(u));
    }
    for h in [2, 3, 4, 5] {
        lines.push(heat(h));
    }
    for n in names {
        lines.push(rival_busted(n));
    }
    let fixed = [
        spotted(),
        lost(),
        escaped(),
        roadblock(true),
        roadblock(false),
        spikes(),
        spiked(),
        busted(),
        wrecked(),
    ];
    // A Map by id, in insertion order; the first entry for an id wins.
    let mut clips: Vec<Clip> = Vec::new();
    let mut add = |words: &str, takes: u32| {
        let id = clip_id(words);
        if !clips.iter().any(|c| c.id == id) {
            clips.push(Clip {
                id,
                text: words.to_owned(),
                takes,
            });
        }
    };
    for l in &lines {
        for p in &l.parts {
            add(p, 1);
        }
    }
    for l in &fixed {
        for p in &l.parts {
            add(p, TAKES);
        }
    }
    clips
}

/// What `levelsRadioClips` reads of a level: whether it has police, its
/// zones' names and its rivals' names.
#[derive(Clone, Debug, Default)]
pub struct LevelRadio<'a> {
    pub police: bool,
    pub zones: Vec<&'a str>,
    pub rivals: Vec<&'a str>,
}

/// `levelsRadioClips(levels)`: every clip for the levels that have police
/// (zones in level order, rival names once each in first-seen order), with
/// `units` as the callsigns (`CALLSIGNS`).
pub fn levels_radio_clips(levels: &[LevelRadio<'_>], units: &[i32]) -> Vec<Clip> {
    let police: Vec<&LevelRadio> = levels.iter().filter(|l| l.police).collect();
    let zones: Vec<&str> = police
        .iter()
        .flat_map(|l| l.zones.iter().copied())
        .collect();
    let mut names: Vec<&str> = Vec::new();
    for n in police.iter().flat_map(|l| l.rivals.iter().copied()) {
        if !names.contains(&n) {
            names.push(n);
        }
    }
    radio_clips(&zones, units, &names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn place_names_as_title_case() {
        assert_eq!(place_name("INTERSTATE 9"), "Interstate 9");
        assert_eq!(place_name("OLD MILL VALLEY"), "Old Mill Valley");
        assert_eq!(place_name("O'HARE-WAY"), "O'Hare-Way");
    }
}
