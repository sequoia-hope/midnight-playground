//! Every police radio clip the game can play, from every level of the Rust
//! game, as JSON on stdout for `tools/radio-voice/render.py` (what
//! `tools/radio-voice/lines.mjs` listed for the JS game, whose levels are
//! frozen; the Rust game's are the script now).
//!
//!   cargo run -p mp_audio --example radio_lines

use mp_audio::radio::lines::{LevelRadio, levels_radio_clips};
use mp_sim::pursuit::CALLSIGNS;

fn main() {
    let ls = mp_levels::levels();
    let lr: Vec<LevelRadio> = ls
        .iter()
        .map(|l| LevelRadio {
            police: l.police.is_some(),
            zones: l.zones.iter().map(|z| z.name).collect(),
            rivals: l.rivals.iter().map(|r| r.name).collect(),
        })
        .collect();
    let clips: Vec<serde_json::Value> = levels_radio_clips(&lr, &CALLSIGNS)
        .into_iter()
        .map(|c| serde_json::json!({ "id": c.id, "text": c.text, "takes": c.takes }))
        .collect();
    println!("{}", serde_json::to_string_pretty(&clips).unwrap());
}
