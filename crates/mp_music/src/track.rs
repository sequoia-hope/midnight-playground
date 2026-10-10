//! The song format as the lab's grammars write it (`seq.js`'s header, the
//! lab's extensions, `tracks.js`'s format), as Rust data. Every JS object
//! whose keys the lab iterates (`Object.entries`) is a `Vec` of pairs in
//! insertion order, and every optional field an `Option` (`None` is a
//! missing key), so a track's canonical JSON ([`crate::json`]) is the lab's
//! and the golden hashes match.
//!
//! The shared data model: the grammars (`genres`) make a [`Track`], the
//! sequencer (`seq`) reads it, the instruments (`instruments`) read each
//! part's [`Lab`] patch live (section automation writes into it by the
//! parameter's JS name, [`Lab::set`]).

use crate::json::Val;

/// Pairs in insertion order (a JS object the lab iterates).
pub type Pairs<T> = Vec<(String, T)>;

/// One of a Moog-style voice's oscillators (`p.osc[i]`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct OscSpec {
    /// 'saw' | 'pulse' | 'tri' | 'sine'.
    pub w: String,
    pub det: Option<f64>,
    pub oct: Option<f64>,
    pub lvl: Option<f64>,
    pub pw: Option<f64>,
}

impl OscSpec {
    pub fn to_val(&self) -> Val {
        Val::obj(vec![
            ("w", Val::str(&self.w)),
            ("det", Val::onum(self.det)),
            ("oct", Val::onum(self.oct)),
            ("lvl", Val::onum(self.lvl)),
            ("pw", Val::onum(self.pw)),
        ])
    }
}

/// An FM operator (`p.ops[i]`): `r` its ratio, `rel` its release (one key
/// each since D1153; the lab once wrote `r` for both and played the release
/// as the ratio).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Op {
    pub r: Option<f64>,
    pub rel: Option<f64>,
    pub l: Option<f64>,
    pub a: Option<f64>,
    pub d: Option<f64>,
    pub s: Option<f64>,
    pub v: Option<f64>,
    pub det: Option<f64>,
    pub fix: Option<f64>,
    pub rs: Option<f64>,
}

impl Op {
    pub fn to_val(&self) -> Val {
        Val::obj(vec![
            ("r", Val::onum(self.r)),
            ("rel", Val::onum(self.rel)),
            ("l", Val::onum(self.l)),
            ("a", Val::onum(self.a)),
            ("d", Val::onum(self.d)),
            ("s", Val::onum(self.s)),
            ("v", Val::onum(self.v)),
            ("det", Val::onum(self.det)),
            ("fix", Val::onum(self.fix)),
            ("rs", Val::onum(self.rs)),
        ])
    }
}

/// Declares `Lab`'s numeric fields with their JS names, and `get` / `set`
/// / `to_val` over them.
macro_rules! lab_nums {
    ($( $field:ident : $js:literal ),* $(,)?) => {
        /// A lab patch (`instruments.js` `BPATCH[name]` with overrides,
        /// `gen.js`'s `P()`): the instrument `kind` ('tb303', 'juno',
        /// 'mono', 'fm', 'string') and every parameter its voices read.
        /// `None` is a missing key: the instruments apply their defaults
        /// (`p.cutoff ?? 400`).
        #[derive(Clone, Debug, PartialEq, Default)]
        pub struct Lab {
            pub kind: String,
            pub name: Option<String>,
            /// The 303's: 'saw' | 'square'.
            pub wave: Option<String>,
            /// The FM voice's algorithm.
            pub algo: Option<String>,
            /// The choir's vowel.
            pub vowel: Option<String>,
            /// The Moog-style voice's oscillators.
            pub osc: Option<Vec<OscSpec>>,
            /// The FM voice's operators (`None` entries are the JS array's
            /// holes: `ops[i]` undefined).
            pub ops: Option<Vec<Option<Op>>>,
            $( pub $field: Option<f64>, )*
        }

        impl Lab {
            /// A numeric parameter by its JS name (section automation's
            /// targets: 'cutoff', 'decay', 'res', 'env', ...).
            pub fn get(&self, name: &str) -> Option<f64> {
                match name {
                    $( $js => self.$field, )*
                    _ => None,
                }
            }

            /// Sets a numeric parameter by its JS name (`lab[param] = v`);
            /// an unknown name is kept nowhere (the JS would add a key no
            /// instrument reads).
            pub fn set(&mut self, name: &str, v: f64) {
                match name {
                    $( $js => self.$field = Some(v), )*
                    _ => {}
                }
            }

            pub fn to_val(&self) -> Val {
                let mut pairs: Vec<(&str, Option<Val>)> = vec![
                    ("kind", Val::str(&self.kind)),
                    ("name", Val::ostr(&self.name)),
                    ("wave", Val::ostr(&self.wave)),
                    ("algo", Val::ostr(&self.algo)),
                    ("vowel", Val::ostr(&self.vowel)),
                    ("osc", self.osc.as_ref().map(|o| Val::Arr(o.iter().map(OscSpec::to_val).collect()))),
                    ("ops", self.ops.as_ref().map(|o| Val::Arr(o.iter().map(|op| op.as_ref().map_or(Val::Null, Op::to_val)).collect()))),
                ];
                $( pairs.push(($js, Val::onum(self.$field))); )*
                Val::obj(pairs)
            }
        }
    };
}

lab_nums! {
    a: "a", d: "d", s: "s", r: "r", fa: "fa", fd: "fd", fs: "fs", fr: "fr",
    gain: "gain", oct: "oct", cutoff: "cutoff", res: "res", fenv: "fenv",
    keytrack: "keytrack", drive: "drive", sub: "sub", noise: "noise",
    glide: "glide", bend: "bend", bend_t: "bendT", vib: "vib", vib_rate: "vibRate",
    vib_delay: "vibDelay", saw: "saw", pulse: "pulse", pw: "pw", pwm: "pwm",
    unison: "unison", detune: "detune", hpf: "hpf", chorus: "chorus",
    lfo_rate: "lfoRate", vowel_mix: "vowelMix", vowel_q: "vowelQ",
    env: "env", decay: "decay", accent: "accent", fbk: "fbk",
    damp: "damp", shape: "shape", pick: "pick", pick_pos: "pickPos", pickup: "pickup", body: "body", body_q: "bodyQ", body_mix: "bodyMix",
    tone: "tone", trem: "trem", trem_rate: "tremRate", wah: "wah", wah_hz: "wahHz",
    wah_q: "wahQ", wah_rate: "wahRate", strum: "strum",
}

/// A part's mixer channel (`part.ch`, `engine.js` `Channel`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Ch {
    pub pump: Option<bool>,
    pub level: Option<f64>,
    pub rev: Option<f64>,
    pub dly: Option<f64>,
    pub pan: Option<f64>,
    pub hp: Option<f64>,
    pub drive: Option<f64>,
    pub drive_lp: Option<f64>,
}

impl Ch {
    pub fn to_val(&self) -> Val {
        Val::obj(vec![
            ("pump", Val::obool(self.pump)),
            ("level", Val::onum(self.level)),
            ("rev", Val::onum(self.rev)),
            ("dly", Val::onum(self.dly)),
            ("pan", Val::onum(self.pan)),
            ("hp", Val::onum(self.hp)),
            ("drive", Val::onum(self.drive)),
            ("driveLp", Val::onum(self.drive_lp)),
        ])
    }
}

/// A part: its type ('bass', 'chord', 'arp', 'mel'), register, channel,
/// patterns by name and lab patch.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Part {
    /// The JS `type`.
    pub kind: String,
    pub lo: Option<f64>,
    /// 16ths per token of a `mel` / `arp` pattern.
    pub res: Option<f64>,
    pub legato: Option<bool>,
    pub gate: Option<f64>,
    pub ch: Ch,
    pub pat: Pairs<String>,
    pub lab: Lab,
}

impl Part {
    pub fn to_val(&self) -> Val {
        Val::obj(vec![
            ("type", Val::str(&self.kind)),
            ("lo", Val::onum(self.lo)),
            ("res", Val::onum(self.res)),
            ("legato", Val::obool(self.legato)),
            ("gate", Val::onum(self.gate)),
            ("ch", Some(self.ch.to_val())),
            ("pat", Some(Val::strs(&self.pat))),
            ("lab", Some(self.lab.to_val())),
        ])
    }
}

/// A per-lane kit tweak (`T.kit[lane]`): the game's sample name `s`, a
/// gain `g`, a reverb send `rev`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct KitTweak {
    pub s: Option<String>,
    pub g: Option<f64>,
    pub rev: Option<f64>,
}

impl KitTweak {
    pub fn to_val(&self) -> Val {
        Val::obj(vec![
            ("s", Val::ostr(&self.s)),
            ("g", Val::onum(self.g)),
            ("rev", Val::onum(self.rev)),
        ])
    }
}

/// The sidechain pump (`T.pump`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pump {
    pub depth: f64,
    pub release: f64,
}

impl Pump {
    pub fn to_val(&self) -> Val {
        Val::obj(vec![
            ("depth", Val::num(self.depth)),
            ("release", Val::num(self.release)),
        ])
    }
}

/// A section: which drum pattern and which part patterns play for how many
/// bars, and the seams. `v` / `vd` (variant counts) are read by
/// `compose::expand`, which does not copy them into the sections it makes.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Section {
    pub bars: u32,
    pub drums: Option<String>,
    pub p: Pairs<String>,
    pub prog: Option<String>,
    pub crash: Option<bool>,
    pub drop: Option<bool>,
    pub down: Option<f64>,
    pub riser: Option<f64>,
    pub swell: Option<bool>,
    pub fill: Option<String>,
    pub gap: Option<f64>,
    pub lp: Option<[f64; 2]>,
    pub auto: Option<Pairs<[f64; 2]>>,
    pub v: Option<Pairs<u32>>,
    pub vd: Option<u32>,
}

impl Section {
    pub fn to_val(&self) -> Val {
        Val::obj(vec![
            ("bars", Val::num(self.bars as f64)),
            ("drums", Val::ostr(&self.drums)),
            ("p", Some(Val::strs(&self.p))),
            ("prog", Val::ostr(&self.prog)),
            ("crash", Val::obool(self.crash)),
            ("drop", Val::obool(self.drop)),
            ("down", Val::onum(self.down)),
            ("riser", Val::onum(self.riser)),
            ("swell", Val::obool(self.swell)),
            ("fill", Val::ostr(&self.fill)),
            ("gap", Val::onum(self.gap)),
            ("lp", self.lp.as_ref().map(Val::pair)),
            (
                "auto",
                self.auto
                    .as_ref()
                    .map(|a| Val::Obj(a.iter().map(|(k, v)| (k.clone(), Val::pair(v))).collect())),
            ),
            (
                "v",
                self.v.as_ref().map(|v| {
                    Val::Obj(
                        v.iter()
                            .map(|(k, n)| (k.clone(), Val::Num(*n as f64)))
                            .collect(),
                    )
                }),
            ),
            ("vd", Val::onum(self.vd.map(|n| n as f64))),
        ])
    }
}

/// A song (`gen.js` `skeleton()` and what a grammar fills in).
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub id: String,
    pub title: Option<String>,
    pub style: Option<String>,
    pub bpm: f64,
    pub gain: f64,
    pub swing: Option<f64>,
    pub delay: f64,
    pub delay_fb: f64,
    pub pump: Pump,
    pub kit_name: Option<String>,
    pub kit: Option<Pairs<KitTweak>>,
    pub prog: Pairs<String>,
    /// Drum patterns by name: each a list of (lane, steps).
    pub drums: Pairs<Pairs<String>>,
    pub parts: Pairs<Part>,
    pub sections: Vec<Section>,
    /// Lane or part → the energy below which it is silent.
    pub lay: Pairs<f64>,
}

impl Track {
    /// `skeleton(id, {})`: the defaults every grammar starts from.
    pub fn skeleton(id: &str) -> Track {
        Track {
            id: id.to_owned(),
            title: None,
            style: None,
            bpm: 120.0,
            gain: 1.0,
            swing: None,
            delay: 0.75,
            delay_fb: 0.32,
            pump: Pump {
                depth: 0.45,
                release: 0.18,
            },
            kit_name: None,
            kit: None,
            prog: Vec::new(),
            drums: Vec::new(),
            parts: Vec::new(),
            sections: Vec::new(),
            lay: Vec::new(),
        }
    }

    /// Total bars of the sections.
    pub fn bars(&self) -> u32 {
        self.sections.iter().map(|s| s.bars).sum()
    }

    /// A part by name.
    pub fn part(&self, name: &str) -> Option<&Part> {
        self.parts.iter().find(|(n, _)| n == name).map(|(_, p)| p)
    }

    /// A drum pattern by name.
    pub fn drum(&self, name: &str) -> Option<&Pairs<String>> {
        self.drums.iter().find(|(n, _)| n == name).map(|(_, d)| d)
    }

    pub fn to_val(&self) -> Val {
        Val::obj(vec![
            ("id", Val::str(&self.id)),
            ("title", Val::ostr(&self.title)),
            ("style", Val::ostr(&self.style)),
            ("bpm", Val::num(self.bpm)),
            ("gain", Val::num(self.gain)),
            ("swing", Val::onum(self.swing)),
            ("delay", Val::num(self.delay)),
            ("delayFb", Val::num(self.delay_fb)),
            ("pump", Some(self.pump.to_val())),
            ("kitName", Val::ostr(&self.kit_name)),
            (
                "kit",
                self.kit
                    .as_ref()
                    .map(|k| Val::Obj(k.iter().map(|(n, t)| (n.clone(), t.to_val())).collect())),
            ),
            ("prog", Some(Val::strs(&self.prog))),
            (
                "drums",
                Some(Val::Obj(
                    self.drums
                        .iter()
                        .map(|(n, lanes)| (n.clone(), Val::strs(lanes)))
                        .collect(),
                )),
            ),
            (
                "parts",
                Some(Val::Obj(
                    self.parts
                        .iter()
                        .map(|(n, p)| (n.clone(), p.to_val()))
                        .collect(),
                )),
            ),
            (
                "sections",
                Some(Val::Arr(
                    self.sections.iter().map(Section::to_val).collect(),
                )),
            ),
            ("lay", Some(Val::nums(&self.lay))),
        ])
    }

    /// The canonical JSON text (`golden.mjs` `canon(T)`).
    pub fn canon(&self) -> String {
        crate::json::canon(&self.to_val())
    }
}

/// Looks a name up in pairs.
pub fn lookup<'a, T>(pairs: &'a [(String, T)], name: &str) -> Option<&'a T> {
    pairs.iter().find(|(n, _)| n == name).map(|(_, v)| v)
}

/// Sets a name in pairs, keeping its place if present (JS assignment to an
/// existing key keeps the key's insertion order).
pub fn put<T>(pairs: &mut Pairs<T>, name: &str, v: T) {
    if let Some(slot) = pairs.iter_mut().find(|(n, _)| n == name) {
        slot.1 = v;
    } else {
        pairs.push((name.to_owned(), v));
    }
}
