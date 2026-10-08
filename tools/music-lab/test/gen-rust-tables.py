"""Turns the Music Lab's evaluated tables (the JSON of
`node tools/music-lab/test/dump-tables.mjs`) into the Rust port's generated
tables: crates/mp_music/src/patches.rs (BPATCH) and kits.rs (KITS and
SAMPLE_TWEAK). Generated from the evaluated objects, not the source, so a key
written twice in a literal (the FM operators' `r`) carries the value the lab
plays with (DECISIONS D1150). Run from the repo root, then `cargo fmt`:

    node tools/music-lab/test/dump-tables.mjs | python3 tools/music-lab/test/gen-rust-tables.py
"""
import json
import sys


d = json.load(sys.stdin)
KITS, TWEAK, SHA = d["KITS"], d["SAMPLE_TWEAK"], d["sha"]["KITS"]

NUMS = {"a":"a","d":"d","s":"s","r":"r","fa":"fa","fd":"fd","fs":"fs","fr":"fr","gain":"gain","oct":"oct","cutoff":"cutoff","res":"res","fenv":"fenv","keytrack":"keytrack","drive":"drive","sub":"sub","noise":"noise","glide":"glide","bend":"bend","bendT":"bend_t","vib":"vib","vibRate":"vib_rate","vibDelay":"vib_delay","saw":"saw","pulse":"pulse","pw":"pw","pwm":"pwm","unison":"unison","detune":"detune","hpf":"hpf","chorus":"chorus","lfoRate":"lfo_rate","vowelMix":"vowel_mix","vowelQ":"vowel_q","env":"env","decay":"decay","accent":"accent","fbk":"fbk","damp":"damp","pick":"pick","body":"body","bodyQ":"body_q","bodyMix":"body_mix","tone":"tone","trem":"trem","tremRate":"trem_rate","wah":"wah","wahHz":"wah_hz","wahQ":"wah_q","wahRate":"wah_rate","strum":"strum"}


def gen_patches(d):
    f = lambda x: repr(float(x))
    out = ["""//! The lab's patches (`instruments.js` `BPATCH`), generated from the
//! evaluated JS objects by `tools/music-lab/test/gen-rust-tables.py` over
//! `node tools/music-lab/test/dump-tables.mjs`, not transcribed from the
//! source: an object literal that writes a key twice keeps the last value
//! (the FM operators' `r`: the ratio, then the release), and that is what
//! the lab plays with. The table's canonical JSON hashes to the lab's
//! (`tests/patches.rs`). Regenerate whenever `BPATCH` changes.

use crate::track::{Lab, Op, OscSpec};
use std::sync::OnceLock;

/// The lab's `BPATCH[name]`, a copy.
pub fn bpatch(name: &str) -> Option<Lab> {
    all().iter().find(|(n, _)| *n == name).map(|(_, l)| l.clone())
}

/// Every patch, in the lab's order.
pub fn all() -> &'static [(&'static str, Lab)] {
    static ALL: OnceLock<Vec<(&'static str, Lab)>> = OnceLock::new();
    ALL.get_or_init(build)
}

/// The SHA-256 of the lab's `canon(BPATCH)` (`dump-tables.mjs` prints it).
pub const LAB_SHA256: &str = "%s";

fn build() -> Vec<(&'static str, Lab)> {
    vec![""" % d["sha"]["BPATCH"]]
    for name, p in d["BPATCH"].items():
        fields = ['kind: "%s".into()' % p["kind"]]
        for k in ["wave", "algo", "vowel"]:
            if k in p:
                fields.append('%s: Some("%s".into())' % (k, p[k]))
        if "osc" in p:
            oscs = []
            for o in p["osc"]:
                parts = ['w: "%s".into()' % o["w"]] + ["%s: Some(%s)" % (k, f(o[k])) for k in ["det", "oct", "lvl", "pw"] if k in o]
                oscs.append("OscSpec { %s, ..Default::default() }" % ", ".join(parts))
            fields.append("osc: Some(vec![%s])" % ", ".join(oscs))
        if "ops" in p:
            ops = []
            for o in p["ops"]:
                if o is None:
                    ops.append("None")
                    continue
                parts = ["%s: Some(%s)" % (k, f(o[k])) for k in ["r", "l", "a", "d", "s", "v", "det", "fix", "rs"] if k in o]
                ops.append("Some(Op { %s, ..Default::default() })" % ", ".join(parts))
            fields.append("ops: Some(vec![%s])" % ", ".join(ops))
        for k, v in p.items():
            if k in ("kind", "wave", "algo", "vowel", "osc", "ops"):
                continue
            assert k in NUMS, (name, k)
            fields.append("%s: Some(%s)" % (NUMS[k], f(v)))
        out.append('        ("%s", Lab { %s, ..Default::default() }),' % (name, ", ".join(fields)))
    out.append("    ]\n}\n")
    return "\n".join(out)


# The JS keys of KitVoice's numeric fields, in the struct's order, with the
# Rust field names.
FIELDS = [
    ("tune", "tune"), ("pitch", "pitch"), ("pt", "pt"), ("decay", "decay"),
    ("click", "click"), ("clickHz", "click_hz"), ("drive", "drive"), ("lvl", "lvl"),
    ("tone", "tone"), ("snappy", "snappy"), ("nhp", "nhp"), ("nlp", "nlp"), ("bp", "bp"),
    ("tail", "tail"), ("bursts", "bursts"), ("spacing", "spacing"), ("q", "q"), ("hp", "hp"),
    ("noise", "noise"), ("bp2", "bp2"), ("decay2", "decay2"), ("f1", "f1"), ("f2", "f2"),
    ("attack", "attack"), ("pan", "pan"), ("gate", "gate"), ("len", "len"), ("rate0", "rate0"),
    ("rate1", "rate1"), ("slap", "slap"),
]
RUST = {js: rs for js, rs in FIELDS}
TYPES = {
    "kick808": "Kick808", "kick909": "Kick909", "tom": "Tom", "boom": "Boom",
    "snare808": "Snare808", "snare909": "Snare909", "clap": "Clap", "metal": "Metal",
    "cymbal": "Cymbal", "cowbell": "Cowbell", "rim": "Rim", "shaker": "Shaker",
    "snap": "Snap", "swell": "Swell", "conga": "Conga", "guiro": "Guiro",
}


def num(v):
    if isinstance(v, bool):
        raise SystemExit("a boolean in a kit table")
    if isinstance(v, int):
        return f"{v}.0"
    return repr(float(v))


out = []
out.append("//! The lab's kits (`instruments.js` `KITS`) and the game's sample tweaks")
out.append("//! (`SAMPLE_TWEAK`), generated from the evaluated JS objects by")
out.append("//! `tools/music-lab/test/gen-rust-tables.py` over `dump-tables.mjs`, not")
out.append("//! transcribed. The table's canonical JSON hashes to the lab's")
out.append("//! (`tests/kits.rs`). Regenerate whenever `KITS` or `SAMPLE_TWEAK` change.")
out.append("")
out.append("use crate::instruments::{HitType, KitVoice};")
out.append("")
out.append("/// The SHA-256 of the lab's `canon(KITS)`.")
out.append(f'pub const KITS_SHA256: &str = "{SHA}";')
out.append("")
out.append("/// `KITS[kit]`: the lanes of a kit, in the lab's order.")
out.append("pub fn kit(name: &str) -> Option<&'static [(&'static str, KitVoice)]> {")
out.append("    KITS.iter().find(|(n, _)| *n == name).map(|(_, k)| *k)")
out.append("}")
out.append("")
out.append("/// `SAMPLE_TWEAK[sample]`: the tweak's keys and factors, in the lab's order.")
out.append("pub fn sample_tweak(name: &str) -> Option<&'static [(&'static str, f64)]> {")
out.append("    SAMPLE_TWEAK.iter().find(|(n, _)| *n == name).map(|(_, t)| *t)")
out.append("}")
out.append("")
out.append("/// Every kit, in the lab's order: per lane name, a voice type and its")
out.append("/// parameters.")
out.append("pub static KITS: &[(&str, &[(&str, KitVoice)])] = &[")
for kit, lanes in KITS.items():
    out.append(f'    ("{kit}", &[')
    for lane, v in lanes.items():
        keys = list(v.keys())
        assert keys[0] == "t", (kit, lane)
        for k in keys[1:]:
            assert k in RUST, (kit, lane, k)
        fields = ", ".join(f"{RUST[k]}: Some({num(v[k])})" for k in keys[1:])
        out.append(f'        ("{lane}", KitVoice {{ t: HitType::{TYPES[v["t"]]}, {fields}, ..KitVoice::NONE }}),')
    out.append("    ]),")
out.append("];")
out.append("")
out.append("/// The game's kit sample names (samples.js) as tweaks of the lab's voices.")
out.append("pub static SAMPLE_TWEAK: &[(&str, &[(&str, f64)])] = &[")
for name, tw in TWEAK.items():
    pairs = ", ".join(f'("{k}", {num(v)})' for k, v in tw.items())
    out.append(f'    ("{name}", &[{pairs}]),')
out.append("];")
open("crates/mp_music/src/kits.rs", "w").write("\n".join(out) + "\n")
open("crates/mp_music/src/patches.rs", "w").write(gen_patches(d))
print("wrote crates/mp_music/src/kits.rs and patches.rs")
