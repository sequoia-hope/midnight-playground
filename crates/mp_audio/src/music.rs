//! `src/game/audio/Music.js`: a small step sequencer playing arranged songs
//! ([`crate::tracks`]) on synthesized instruments, all in Web Audio.
//!
//! Per song:
//!   drum voices ─────────────────────────────┐
//!   instrument channels ─ [pump] ─ song filter ─ song bus ─┐
//!          └ sends ─ song reverb send ─ shared reverb ─────┼─ out (the game's music bus)
//!          └ sends ─ song ping-pong delay ─ song filter    │
//!   risers / sweeps ───────────────────────────────────────┘
//!
//! Every song gets its own channels, delay and bus, so a skip can fade the old
//! one out while the next starts, and the per-song mix (levels, sends, pump,
//! drive, chorus) never leaks into the next. Drums are pre-rendered buffers
//! (samples.js): one source + gain per hit. Synth notes build a few nodes per
//! note (a pad chord is one filter/amp around all its oscillators), which keeps
//! a busy 16th at a couple of dozen nodes, not hundreds.
//!
//! Timing: a lookahead scheduler (setTimeout every 25 ms, events placed
//! ~0.2 s ahead on the audio clock). If the main thread stalls long enough that
//! a step is already in the past, that step is dropped rather than played late,
//! so the groove stays on the grid. pumpUntil() is public so an
//! OfflineAudioContext can drive it to render songs faster than real time.
//!
//! Port: WP 5.2 made the arrays (`pulse_wave`, `hall_ir`); the sequencer and
//! instruments came with WP 5.3–5.5, which needed them for the call log
//! (DECISIONS D251). The timers are [`crate::timers`]; the `Math.random` of
//! the risers' noise is the audio's one stream.

use crate::engine::Fourier;
use crate::radio::Random;
use crate::timers::{Task, TimerId, Timers};
use crate::tracks::{self, Channel, KitVoice, Part, PartKind, Patch, Section, Track};
use crate::wa::{
    AudioBuffer, AudioContext, BiquadFilterNode, BiquadFilterType, Connectable, ConvolverNode,
    GainNode, Node, OscillatorNode, OverSampleType, PeriodicWave,
};
use mp_math::js;
use mp_math::kernel::{exp, pow, sin, tanh};
use std::f64::consts::PI;
use std::rc::Rc;

/// Pulse waves for the square-ish leads (25 % duty: hollow and nasal).
/// Cosine terms: a sine-only series of the same magnitudes is a spiky, quiet
/// wave. 64 coefficients (`this.pulse`).
pub fn pulse_wave() -> Fourier {
    let hh = 64;
    let mut re = vec![0f32; hh];
    let im = vec![0f32; hh];
    for (h, r) in re.iter_mut().enumerate().skip(1) {
        let h = h as f64;
        *r = ((2.0 / (h * PI)) * sin(h * PI * 0.25)) as f32;
    }
    Fourier { real: re, imag: im }
}

/// Shared hall reverb: stereo, 2.6 s, darkening as it decays, with a short
/// pre-delay and a few early reflections (`this.reverb.buffer`). Its noise
/// is a fixed xorshift per channel, not `Math.random`.
pub fn hall_ir(sample_rate: f64) -> Vec<Vec<f32>> {
    let sr = sample_rate;
    let len = (sr * 2.6).floor() as usize;
    let mut ir = Vec::with_capacity(2);
    for c in 0..2 {
        let mut d = vec![0f32; len];
        let mut lp = 0.0;
        // The JS keeps `seed` as an int32 after the first `^=`; every step is
        // a 32-bit operation, so u32 with wrapping shifts is the same bits.
        let mut seed: u32 = if c != 0 { 0x9e3779b9 } else { 0x7f4a7c15 };
        let mut rand = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed as f64 / 4294967296.0) * 2.0 - 1.0
        };
        let pre = (0.018 * sr).floor() as usize;
        for i in pre..len {
            let t = (i - pre) as f64 / sr;
            let u = i as f64 / len as f64;
            let k = 0.75 - 0.6 * u; // one-pole low-pass that closes over the tail
            lp += k * (rand() - lp);
            d[i] = (lp * exp((-6.9 * t) / 2.3) * (if t < 0.03 { t / 0.03 } else { 1.0 })) as f32;
        }
        for k in 0..10 {
            let kf = k as f64;
            let at = pre
                + (sr * (0.005 + 0.06 * ((kf * 0.37 + c as f64 * 0.19) % 1.0))).floor() as usize;
            let sign = if k % 2 != 0 { -1.0 } else { 1.0 };
            d[at] = (d[at] as f64 + sign * 0.35 * (1.0 - kf / 12.0)) as f32;
        }
        ir.push(d);
    }
    ir
}

// ── The sequencer ────────────────────────────────────────────────

fn mtof(m: f64) -> f64 {
    440.0 * pow(2.0, (m - 69.0) / 12.0)
}
const LOOKAHEAD: f64 = 0.2;
// The kit buffers are normalised near full scale; this sits them under the synths.
const DRUM_TRIM: f64 = 0.16;
const TICK_MS: f64 = 25.0;

/// Instrument shorthands in tracks.js → OscillatorType (`OSC_TYPE[P.type] ||
/// P.type || 'sawtooth'`).
fn osc_type(kind: &str) -> &str {
    match kind {
        "saw" => "sawtooth",
        "tri" => "triangle",
        "" => "sawtooth",
        k => k,
    }
}

/// `x ?? d` on a patch field.
fn or(x: Option<f64>, d: f64) -> f64 {
    x.unwrap_or(d)
}

/// A field used as a JS truthy test (`if (P.fenv)`).
fn truthy(x: Option<f64>) -> bool {
    x.is_some_and(|v| v != 0.0 && !v.is_nan())
}

// ── Notes, chords, voicings ──────────────────────────────────────
fn pc_letter(c: u8) -> Option<i64> {
    Some(match c {
        b'C' => 0,
        b'D' => 2,
        b'E' => 4,
        b'F' => 5,
        b'G' => 7,
        b'A' => 9,
        b'B' => 11,
        _ => return None,
    })
}

fn pc_of(s: &str) -> i64 {
    let b = s.as_bytes();
    let mut p = pc_letter(b[0]).expect("a note letter");
    match b.get(1) {
        Some(b'#') => p += 1,
        Some(b'b') => p -= 1,
        _ => {}
    }
    (p + 12) % 12
}

/// `noteToMidi(tok)`: `C#5` → 73; `None` for a malformed token. No wrap to a
/// pitch class here: Cb4 is B3 and B#3 is C4.
pub fn note_to_midi(tok: &str) -> Option<f64> {
    let b = tok.as_bytes();
    let letter = pc_letter(*b.first()?)?;
    let mut i = 1;
    let mut acc = 0;
    match b.get(1) {
        Some(b'#') => {
            acc = 1;
            i = 2;
        }
        Some(b'b') => {
            acc = -1;
            i = 2;
        }
        _ => {}
    }
    let rest = &tok[i..];
    let (neg, digits) = match rest.strip_prefix('-') {
        Some(d) => (true, d),
        None => (false, rest),
    };
    if digits.len() != 1 || !digits.as_bytes()[0].is_ascii_digit() {
        return None;
    }
    let mut oct = (digits.as_bytes()[0] - b'0') as i64;
    if neg {
        oct = -oct;
    }
    Some((12 * (oct + 1) + letter + acc) as f64)
}

fn qual(q: &str) -> Option<&'static [i64]> {
    Some(match q {
        "" => &[0, 4, 7],
        "m" => &[0, 3, 7],
        "7" => &[0, 4, 7, 10],
        "m7" => &[0, 3, 7, 10],
        "maj7" => &[0, 4, 7, 11],
        "sus2" => &[0, 2, 7],
        "sus4" => &[0, 5, 7],
        "5" => &[0, 7, 12],
        "add9" => &[0, 4, 7, 14],
        "madd9" => &[0, 3, 7, 14],
        "m9" => &[0, 3, 7, 10, 14],
        "maj9" => &[0, 4, 7, 11, 14],
        "dim" => &[0, 3, 6],
        "6" => &[0, 4, 7, 9],
        "m6" => &[0, 3, 7, 9],
        "7sus4" => &[0, 5, 7, 10],
        "aug" => &[0, 4, 8],
        "9" => &[0, 4, 7, 10, 14],
        _ => return None,
    })
}

/// A parsed chord. `id` stands for the JS object's identity: the same chord
/// of the same progression bar is the same object.
#[derive(Clone, Debug, PartialEq)]
pub struct Chord {
    pub id: usize,
    pub name: &'static str,
    pub root: i64,
    pub iv: &'static [i64],
    pub bass: i64,
}

fn parse_chord(name: &'static str, id: usize) -> Chord {
    let mut it = name.split('/');
    let head = it.next().unwrap_or("");
    let slash = it.next();
    let b = head.as_bytes();
    let n = if b.len() > 1 && (b[1] == b'#' || b[1] == b'b') {
        2
    } else {
        1
    };
    let root = pc_of(&head[..n]);
    let iv = qual(&head[n..]).unwrap_or(&[0, 4, 7]);
    Chord {
        id,
        name,
        root,
        iv,
        bass: slash.map_or(root, pc_of),
    }
}

fn md12(x: i64) -> i64 {
    x.rem_euclid(12)
}

/// Chord tones between lo and ~lo+16, choosing the inversion that moves the
/// least from the previous voicing (smooth voice leading).
pub fn voice(ch: &Chord, lo: f64, prev: Option<&[f64]>) -> Vec<f64> {
    let lo = lo as i64;
    let mut pcs: Vec<i64> = Vec::new();
    for i in ch.iv {
        let pc = (ch.root + i) % 12;
        if !pcs.contains(&pc) {
            pcs.push(pc);
        }
    }
    let mut best: Vec<f64> = Vec::new();
    let mut best_score = f64::INFINITY;
    for r in 0..pcs.len() {
        let mut notes: Vec<f64> = Vec::new();
        let mut m = lo + md12(pcs[r] - lo);
        for k in 0..pcs.len() {
            let pc = pcs[(r + k) % pcs.len()];
            if k > 0 {
                m += 1;
                while md12(m) != pc {
                    m += 1;
                }
            }
            notes.push(m as f64);
        }
        let score = match prev {
            Some(p) if !p.is_empty() => notes.iter().enumerate().fold(0.0, |a, (i, n)| {
                a + (n - p.get(i).copied().unwrap_or(p[p.len() - 1])).abs()
            }),
            _ => notes[0] - lo as f64,
        };
        if score < best_score {
            best_score = score;
            best = notes;
        }
    }
    best
}

// ── Pattern compilation ──────────────────────────────────────────
/// Drum lanes: one char per 16th. X accent, x hit, o ghost, . rest.
fn vel(c: char) -> f64 {
    match c {
        'X' => 1.0,
        'x' => 0.8,
        'o' => 0.42,
        _ => f64::NAN,
    }
}

/// One event of a part's pattern.
#[derive(Clone, Debug, PartialEq)]
pub enum Ev {
    /// Bass: a degree char (lowercase), accent, length in 16ths, slide.
    Bass {
        deg: char,
        accent: bool,
        len: usize,
        slide: bool,
    },
    /// Chord hit.
    Hit { vel: f64, len: usize },
    /// Arp (chord-tone index) or melody (MIDI note).
    Tok {
        idx: f64,
        midi: f64,
        len: usize,
        accent: bool,
    },
}

impl Ev {
    fn len(&self) -> usize {
        match *self {
            Ev::Bass { len, .. } | Ev::Hit { len, .. } | Ev::Tok { len, .. } => len,
        }
    }
}

fn no_ws(s: &str) -> Vec<char> {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

// Bass lanes: one char per 16th. r root, o octave, f fifth, t third,
// s seventh, l root an octave down, u fifth an octave up; uppercase = accent;
// '-' holds, '~' holds and slides into the next note, '.' rest.
fn bass_deg(d: char) -> Option<f64> {
    Some(match d {
        'r' => 0.0,
        'o' => 12.0,
        'f' => 7.0,
        'l' => -12.0,
        'u' => 19.0,
        _ => return None,
    })
}

fn compile_bass(s: &str) -> Vec<Option<Ev>> {
    let s = no_ws(s);
    let n = s.len();
    let mut out = vec![None; n];
    for i in 0..n {
        let c = s[i];
        if "-~.".contains(c) {
            continue;
        }
        let (mut len, mut slide) = (1, false);
        while i + len < n && (s[i + len] == '-' || s[i + len] == '~') {
            if s[i + len] == '~' {
                slide = true;
            }
            len += 1;
        }
        let lower = c.to_ascii_lowercase();
        out[i] = Some(Ev::Bass {
            deg: lower,
            accent: c != lower,
            len,
            slide,
        });
    }
    out
}

// Chord lanes: x hit, X accent, '-' hold, '.' rest.
fn compile_hits(s: &str) -> Vec<Option<Ev>> {
    let s = no_ws(s);
    let n = s.len();
    let mut out = vec![None; n];
    for i in 0..n {
        if s[i] != 'x' && s[i] != 'X' {
            continue;
        }
        let mut len = 1;
        while i + len < n && s[i + len] == '-' {
            len += 1;
        }
        out[i] = Some(Ev::Hit {
            vel: if s[i] == 'X' { 1.0 } else { 0.8 },
            len,
        });
    }
    out
}

// Token lanes (arps, melodies): space separated, each token `res` 16ths long.
// A token is a note (C#5), a chord-tone index (arps), '_' hold or '.' rest.
// A trailing ! accents.
fn compile_tokens(s: &str, res: usize, arp: bool) -> Vec<Option<Ev>> {
    let toks: Vec<&str> = s.split_whitespace().collect();
    let n = toks.len() * res;
    let mut out = vec![None; n];
    for i in 0..toks.len() {
        let mut t = toks[i];
        if t == "_" || t == "." {
            continue;
        }
        let accent = t.ends_with('!');
        if accent {
            t = &t[..t.len() - 1];
        }
        let mut len = 1;
        while i + len < toks.len() && toks[i + len] == "_" {
            len += 1;
        }
        let (idx, midi) = if arp {
            (t.parse::<f64>().unwrap_or(f64::NAN), f64::NAN)
        } else {
            let m = note_to_midi(t).unwrap_or_else(|| panic!("bad note {t}"));
            (f64::NAN, m)
        };
        out[i * res] = Some(Ev::Tok {
            idx,
            midi,
            len: len * res,
            accent,
        });
    }
    out
}

/// A drum lane: a voice and its steps.
#[derive(Clone, Debug)]
pub struct Lane {
    pub voice: &'static str,
    pub steps: Vec<char>,
}

/// A compiled part: the track's part and its patterns by key.
#[derive(Clone, Debug)]
pub struct CPart {
    pub part: &'static Part,
    pub pats: Vec<(&'static str, Vec<Option<Ev>>)>,
}

/// `compileTrack(T)`.
#[derive(Clone, Debug)]
pub struct Compiled {
    pub prog: Vec<(&'static str, Vec<Vec<Chord>>)>,
    pub drums: Vec<(&'static str, Vec<Lane>)>,
    pub parts: Vec<CPart>,
}

impl Compiled {
    fn prog(&self, k: &str) -> Option<&Vec<Vec<Chord>>> {
        self.prog.iter().find(|(n, _)| *n == k).map(|(_, v)| v)
    }

    fn drums(&self, k: &str) -> Option<&Vec<Lane>> {
        self.drums.iter().find(|(n, _)| *n == k).map(|(_, v)| v)
    }

    fn part(&self, k: &str) -> Option<&CPart> {
        self.parts.iter().find(|p| p.part.name == k)
    }
}

pub fn compile_track(t: &'static Track) -> Compiled {
    let mut id = 0;
    let prog = t
        .prog
        .iter()
        .map(|&(k, v)| {
            let bars = v
                .split_whitespace()
                .map(|bar| {
                    bar.split(',')
                        .map(|c| {
                            id += 1;
                            parse_chord(c, id)
                        })
                        .collect()
                })
                .collect();
            (k, bars)
        })
        .collect();
    let drums = t
        .drums
        .iter()
        .map(|&(k, lanes)| {
            let l = lanes
                .iter()
                .map(|&(voice, s)| Lane {
                    voice,
                    steps: no_ws(s),
                })
                .collect();
            (k, l)
        })
        .collect();
    let parts = t
        .parts
        .iter()
        .map(|p| CPart {
            part: p,
            pats: p
                .pat
                .iter()
                .map(|&(k, s)| {
                    let c = match p.kind {
                        PartKind::Bass => compile_bass(s),
                        PartKind::Chord => compile_hits(s),
                        k => compile_tokens(s, p.res.unwrap_or(1), k == PartKind::Arp),
                    };
                    (k, c)
                })
                .collect(),
        })
        .collect();
    Compiled { prog, drums, parts }
}

// Drum fills for the last bar of a section, from step `from`. They replace
// the snare/tom/hat lanes; the kick keeps its own pattern unless the fill has one.
struct Fill {
    from: usize,
    ramp: bool,
    lanes: &'static [(&'static str, &'static str)],
}

fn fill(name: &str) -> Option<Fill> {
    let f = |from, ramp, lanes| Some(Fill { from, ramp, lanes });
    match name {
        "snare" => f(
            8,
            false,
            &[("snare", "........x.xxXxXX"), ("kick", "x.......x.......")],
        ),
        "tom" => f(
            8,
            false,
            &[
                ("tomH", "........x.x....."),
                ("tomM", "............x.x."),
                ("tomL", "..............xX"),
                ("kick", "x.......x......."),
            ],
        ),
        "roll" => f(
            0,
            true,
            &[("snare", "x.x.x.x.xxxxxxxx"), ("kick", "x...x...x...x...")],
        ),
        "dnb" => f(
            8,
            false,
            &[("snare", "........X.oXxoXX"), ("kick", "x.........x.....")],
        ),
        "west" => f(
            8,
            false,
            &[
                ("tomL", "........x..x..x."),
                ("tomM", "..........x..x.."),
                ("snare", "..............xX"),
                ("kick", "x.......x......."),
            ],
        ),
        "crash" => f(
            12,
            false,
            &[("snare", "............XXXX"), ("kick", "x.......x...x...")],
        ),
        _ => None,
    }
}

const FILL_REPLACES: &[&str] = &[
    "snare", "clap", "hat", "ohat", "ride", "shaker", "rim", "snap", "tomL", "tomM", "tomH",
];

/// The default kit: buffer, level, reverb send. Levels are set by measured
/// RMS in a full mix (kick ≈ -3 dB under the whole mix, the snare ~8 dB
/// under the kick, hats ~15 dB under). A track's kit entries override the
/// buffer / send / rate, and scale the level (g is a multiplier there).
pub fn kit_default(name: &str) -> Option<(&'static str, f64, f64)> {
    Some(match name {
        "kick" => ("kickPunch", 0.9, 0.0),
        "snare" => ("snareGated", 1.7, 0.25),
        "clap" => ("clap", 3.0, 0.3),
        "hat" => ("hat", 1.5, 0.0),
        "ohat" => ("ohat", 0.9, 0.05),
        "ride" => ("ride", 0.7, 0.05),
        "crash" => ("crash", 0.6, 0.2),
        "revCrash" => ("revCrash", 0.6, 0.2),
        "shaker" => ("shaker", 0.9, 0.05),
        "rim" => ("rim", 1.2, 0.2),
        "snap" => ("snap", 1.2, 0.35),
        "tomL" => ("tomL", 1.0, 0.25),
        "tomM" => ("tomM", 1.0, 0.25),
        "tomH" => ("tomH", 1.0, 0.25),
        "boom" => ("boom", 0.9, 0.1),
        _ => return None,
    })
}

/// `{ id, title, style, bpm }` of a track.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackInfo {
    pub id: &'static str,
    pub title: &'static str,
    pub style: &'static str,
    pub bpm: f64,
}

impl TrackInfo {
    fn of(t: &Track) -> TrackInfo {
        TrackInfo {
            id: t.id,
            title: t.title,
            style: t.style,
            bpm: t.bpm,
        }
    }
}

struct DrumVoice {
    input: GainNode,
    buf: Option<AudioBuffer>,
    rate: f64,
    _rev: Option<GainNode>,
}

#[derive(Clone, Copy, Debug)]
struct Last {
    midi: f64,
    slide: bool,
    end: f64,
}

/// A song playing (or ringing out): `S`.
pub struct Song {
    serial: u64,
    t: &'static Track,
    c: Rc<Compiled>,
    step_dur: f64,
    ch: Vec<(&'static str, GainNode)>,
    dv: Vec<(&'static str, DrumVoice)>,
    lfos: Vec<OscillatorNode>,
    voicing: Vec<(&'static str, usize, Vec<f64>)>,
    last: Vec<(&'static str, Last)>,
    bus: GainNode,
    filter: BiquadFilterNode,
    pump: GainNode,
    rev: GainNode,
    dly: GainNode,
    /// Nodes only the graph holds in the JS (the delay, the channels'
    /// insides): kept for the song's life.
    keep: Vec<Node>,
}

/// A retired song waiting for `_retire`'s `kill`.
pub struct SongKill(Box<Song>);

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pos {
    pub sec: usize,
    pub bar: u32,
    pub step: u32,
}

/// Called when a track starts playing.
pub type OnTrack = Rc<dyn Fn(&TrackInfo)>;

/// `Music`.
pub struct Music {
    ctx: AudioContext,
    out: Node,
    song: Option<Box<Song>>,
    pub on: bool,
    /// `onTrack`: (info) => void, called when a track starts playing.
    pub on_track: Option<OnTrack>,
    pub realtime: bool,
    pub playlist: Vec<&'static str>,
    timer: Option<TimerId>,
    /// Mixing aid: a part name, or `"drums"`, plays alone.
    pub solo: Option<String>,
    pub kit: Vec<(&'static str, AudioBuffer)>,
    pub pulse: PeriodicWave,
    pub reverb: ConvolverNode,
    pub rev_out: GainNode,
    pending: Option<(&'static Track, u32)>,
    wanted: Option<&'static str>,
    pub pos: Pos,
    pub next_t: f64,
    noise_buf: Option<AudioBuffer>,
    random: Random,
    compiled: Vec<(&'static str, Rc<Compiled>)>,
    serial: u64,
    /// Retired songs an offline context never kills.
    retired: Vec<Song>,
    /// Tests: when set, each part's note is listed as (track, section, part).
    pub trace_parts: Option<Vec<(&'static str, usize, &'static str)>>,
}

fn find_track(id: &str) -> &'static Track {
    tracks::track(id).unwrap_or(&tracks::TRACKS[0])
}

/// `(i + 1) % n` of the playlist, where `i` is `indexOf` (-1 when absent).
fn next_in(playlist: &[&'static str], cur: Option<&str>) -> &'static str {
    let i = cur.and_then(|c| playlist.iter().position(|p| *p == c));
    let n = playlist.len() as i64;
    playlist[((i.map_or(-1, |i| i as i64) + 1) % n) as usize]
}

impl Music {
    /// `new Music(ctx, out)` then `build()`: the drum kit, the pulse wave and
    /// the shared hall reverb. `realtime` is false on an offline context.
    pub fn build(ctx: &AudioContext, out: &Node, realtime: bool, random: Random) -> Music {
        let kit = crate::samples::render_kit(ctx);
        // Pulse waves for the square-ish leads (25 % duty: hollow and nasal).
        let pw = pulse_wave();
        let pulse = ctx
            .create_periodic_wave(&pw.real, &pw.imag, None)
            .expect("the pulse wave");
        // Shared hall reverb: stereo, 2.6 s, darkening as it decays, with a
        // short pre-delay and a few early reflections.
        let sr = ctx.sample_rate();
        let len = (sr * 2.6).floor() as u32;
        let ir = ctx.create_buffer(2, len, sr);
        for (c, d) in hall_ir(sr).iter().enumerate() {
            ir.copy_to_channel(d, c as u32);
        }
        let reverb = ctx.create_convolver();
        let _ = reverb.set_buffer(Some(&ir));
        let rev_out = ctx.create_gain();
        rev_out.gain.set_value(0.42);
        let _ = reverb.connect(&rev_out);
        let _ = rev_out.connect(out);
        Music {
            ctx: ctx.clone(),
            out: out.clone(),
            song: None,
            on: false,
            on_track: None,
            realtime,
            playlist: tracks::PLAYLIST.to_vec(),
            timer: None,
            solo: None,
            kit,
            pulse,
            reverb,
            rev_out,
            pending: None,
            wanted: None,
            pos: Pos::default(),
            next_t: 0.0,
            noise_buf: None,
            random,
            compiled: Vec::new(),
            serial: 0,
            retired: Vec::new(),
            trace_parts: None,
        }
    }

    /// `Music.tracks`: `[{ id, title, style, bpm }]`.
    pub fn tracks() -> Vec<TrackInfo> {
        tracks::TRACKS.iter().map(TrackInfo::of).collect()
    }

    /// `Music.levelTrack(level)`.
    pub fn level_track(level: &str) -> &'static str {
        tracks::LEVEL_TRACK
            .iter()
            .find(|(l, _)| *l == level)
            .map_or(tracks::PLAYLIST[0], |(_, t)| t)
    }

    /// A kit buffer by name (`this.kit[name]`).
    pub fn kit_buffer(&self, name: &str) -> Option<&AudioBuffer> {
        self.kit.iter().find(|(k, _)| *k == name).map(|(_, b)| b)
    }

    /// The serial of the song playing.
    pub fn song_serial(&self) -> Option<u64> {
        self.song.as_ref().map(|s| s.serial)
    }

    /// `current`: the playing track.
    pub fn current(&self) -> Option<TrackInfo> {
        self.song.as_ref().map(|s| TrackInfo::of(s.t))
    }

    /// `info`: the track queued to play when music starts, else the playing
    /// one. A track is queued only while the music is off, and it replaces
    /// the stopped song when the music comes back, so it comes first: the
    /// menu plays the level's track at once (D1010), so with the music
    /// turned down a picked track found the old song reported instead.
    pub fn info(&self) -> Option<TrackInfo> {
        if let Some((t, _)) = self.pending {
            return Some(TrackInfo::of(t));
        }
        if let Some(s) = &self.song {
            return Some(TrackInfo::of(s.t));
        }
        self.wanted.and_then(tracks::track).map(TrackInfo::of)
    }

    fn now(&self) -> f64 {
        self.ctx.current_time()
    }

    // ── Transport ───────────────────────────────────────────────────
    /// Start (or keep) a track. If it is already the one playing, nothing
    /// changes, so restarting a race doesn't restart the song.
    pub fn play(&mut self, id: &str, bar: u32, fade: bool, timers: &mut Timers) {
        let t = find_track(id);
        if let Some(s) = &self.song
            && std::ptr::eq(s.t, t)
            && bar == 0
        {
            self.pending = None;
            self.wanted = Some(t.id);
            return;
        }
        self.wanted = Some(t.id);
        if !self.on {
            self.pending = Some((t, bar));
            return;
        }
        let now = self.now();
        let start = if self.song.is_some() && fade {
            now + 0.55
        } else {
            now + 0.1
        };
        if let Some(s) = self.song.take() {
            self.retire(s, now, if fade { 0.12 } else { 0.02 }, timers);
        }
        self.begin(t, start, bar, timers);
    }

    /// Start a track at a bar right now, even the one already playing (the
    /// music player's restart and seek). A hard cut, no fade.
    pub fn seek(&mut self, id: &str, bar: u32, timers: &mut Timers) {
        let t = find_track(id);
        self.wanted = Some(t.id);
        if !self.on {
            self.pending = Some((t, bar));
            return;
        }
        let now = self.now();
        if let Some(s) = self.song.take() {
            self.retire(s, now, 0.02, timers);
        }
        self.begin(t, now + 0.05, bar, timers);
    }

    pub fn next(&mut self, timers: &mut Timers) -> Option<TrackInfo> {
        let cur = self
            .song
            .as_ref()
            .map(|s| s.t.id)
            .or(self.pending.map(|(t, _)| t.id))
            .or(self.wanted);
        let id = next_in(&self.playlist, cur);
        if !self.on {
            let t = tracks::track(id)?;
            self.pending = Some((t, 0));
            self.wanted = Some(id);
            return Some(TrackInfo::of(t));
        }
        self.play(id, 0, true, timers);
        self.current()
    }

    /// Scheduling on/off. Stopping keeps the position; starting again picks
    /// the song up from the start of the bar it stopped in.
    pub fn start(&mut self, timers: &mut Timers) {
        if self.on {
            return;
        }
        self.on = true;
        let now = self.now();
        if let Some((t, bar)) = self.pending.take() {
            if let Some(s) = self.song.take() {
                self.retire(s, now, 0.02, timers);
            }
            self.begin(t, now + 0.1, bar, timers);
        } else if let Some(s) = &self.song {
            self.pos.step = 0;
            self.next_t = now + 0.1;
            s.bus.gain.cancel_scheduled_values(now);
            s.bus
                .gain
                .set_target_at_time(s.t.gain.unwrap_or(1.0), now, 0.05);
        } else {
            let t = self.wanted.map_or(&tracks::TRACKS[0], find_track);
            self.begin(t, now + 0.1, 0, timers);
        }
        if self.realtime {
            self.tick(timers);
        }
    }

    pub fn stop(&mut self, timers: &mut Timers) {
        self.on = false;
        timers.clear(self.timer);
    }

    pub fn on_resume(&mut self) {
        if self.on && self.song.is_some() {
            self.next_t = js::max(self.next_t, self.now() + 0.05);
        }
    }

    /// `_tick()`: the scheduler's timer.
    pub fn tick(&mut self, timers: &mut Timers) {
        if !self.on {
            return;
        }
        let until = self.now() + LOOKAHEAD;
        self.pump_until(until, timers);
        let now = self.now();
        self.timer = Some(timers.set_timeout(now, TICK_MS, Task::MusicTick));
    }

    pub fn pump_until(&mut self, until: f64, timers: &mut Timers) {
        let now = self.now();
        let mut guard = 0;
        loop {
            if self.song.is_none() || self.next_t >= until {
                break;
            }
            let go = guard < 256;
            guard += 1;
            if !go {
                break;
            }
            let mut s = self.song.take().expect("a song");
            let late = self.realtime && self.next_t < now - 0.03;
            if !late {
                self.step(&mut s, self.next_t);
            }
            self.next_t += s.step_dur;
            let t = s.t;
            let p = &mut self.pos;
            p.step += 1;
            if p.step == 16 {
                p.step = 0;
                p.bar += 1;
                if p.bar >= t.sections[p.sec].bars {
                    p.bar = 0;
                    p.sec += 1;
                    if p.sec >= t.sections.len() {
                        // Song over: let it ring out, then the playlist moves on.
                        let next_t = find_track(next_in(&self.playlist, Some(t.id)));
                        let at = self.next_t;
                        self.retire(s, at + 2.5, 0.8, timers);
                        self.begin(next_t, at + 1.2, 0, timers);
                        continue;
                    }
                }
            }
            self.song = Some(s);
        }
    }

    // ── Song setup / teardown ──────────────────────────────────────
    fn compiled(&mut self, t: &'static Track) -> Rc<Compiled> {
        if let Some((_, c)) = self.compiled.iter().find(|(id, _)| *id == t.id) {
            return c.clone();
        }
        let c = Rc::new(compile_track(t));
        self.compiled.push((t.id, c.clone()));
        c
    }

    fn begin(&mut self, t: &'static Track, time: f64, bar: u32, timers: &mut Timers) {
        let ctx = self.ctx.clone();
        let c = self.compiled(t);
        let bus = ctx.create_gain();
        bus.gain.set_value(0.0);
        bus.gain.set_value_at_time(0.0, time - 0.01);
        bus.gain
            .linear_ramp_to_value_at_time(t.gain.unwrap_or(1.0), time + 0.03);
        let _ = bus.connect(&self.out);
        let filter = ctx.create_biquad_filter();
        filter.set_type(BiquadFilterType::Lowpass);
        filter.frequency.set_value(20000.0);
        filter.q.set_value(0.9);
        let _ = filter.connect(&bus);
        let pump = ctx.create_gain();
        let _ = pump.connect(&filter);
        let rev = ctx.create_gain();
        rev.gain.set_value(t.rev.unwrap_or(1.0));
        let _ = rev.connect(&self.reverb);
        // Ping-pong delay, tempo-synced (dotted eighth unless the track says otherwise).
        let dt = t.delay.unwrap_or(0.75) * (60.0 / t.bpm);
        let dly = ctx.create_gain();
        dly.gain.set_value(1.0);
        let d_l = ctx.create_delay(2.0);
        let d_r = ctx.create_delay(2.0);
        d_l.delay_time.set_value(dt);
        d_r.delay_time.set_value(dt);
        let fb = ctx.create_gain();
        fb.gain.set_value(t.delay_fb.unwrap_or(0.38));
        let dlp = ctx.create_biquad_filter();
        dlp.set_type(BiquadFilterType::Lowpass);
        dlp.frequency.set_value(3200.0);
        let dhp = ctx.create_biquad_filter();
        dhp.set_type(BiquadFilterType::Highpass);
        dhp.frequency.set_value(280.0);
        let merge = ctx.create_channel_merger(2);
        let d_out = ctx.create_gain();
        d_out.gain.set_value(0.5);
        let _ = dly.connect(&dhp);
        let _ = dhp.connect(&d_l);
        let _ = d_l.connect_with(&merge, Some(0), Some(0));
        let _ = d_l.connect(&dlp);
        let _ = dlp.connect(&fb);
        let _ = fb.connect(&d_r);
        let _ = d_r.connect_with(&merge, Some(0), Some(1));
        let _ = d_r.connect(&d_l);
        let _ = merge.connect(&d_out);
        let _ = d_out.connect(&filter);
        self.serial += 1;
        let mut s = Box::new(Song {
            serial: self.serial,
            t,
            c: c.clone(),
            step_dur: 60.0 / t.bpm / 4.0,
            ch: Vec::new(),
            dv: Vec::new(),
            lfos: Vec::new(),
            voicing: Vec::new(),
            last: Vec::new(),
            bus,
            filter,
            pump,
            rev,
            dly,
            keep: vec![
                (*d_l).clone(),
                (*d_r).clone(),
                (*fb).clone(),
                (*dlp).clone(),
                (*dhp).clone(),
                merge,
                (*d_out).clone(),
            ],
        });
        // Instrument channels.
        for p in &c.parts {
            let inp = self.channel(&mut s, &p.part.ch);
            s.ch.push((p.part.name, inp));
        }
        let serial = s.serial;
        self.song = Some(s);
        self.pos = Pos::default();
        // Starting mid-song (tests, WAV renders): jump to the section holding `bar`.
        let mut b = bar;
        while b > 0 && self.pos.sec < t.sections.len() - 1 && b >= t.sections[self.pos.sec].bars {
            b -= t.sections[self.pos.sec].bars;
            self.pos.sec += 1;
        }
        self.pos.bar = b.min(t.sections[self.pos.sec].bars - 1);
        self.next_t = time;
        let info = TrackInfo::of(t);
        if let Some(cb) = &self.on_track {
            if self.realtime {
                let now = self.now();
                timers.set_timeout(
                    now,
                    js::max(0.0, (time - now) * 1000.0),
                    Task::MusicOnTrack { serial, info },
                );
            } else {
                cb(&info);
            }
        }
    }

    /// `fire`: the track change the timer announces, if that song still
    /// plays.
    pub fn fire_on_track(&self, serial: u64, info: &TrackInfo) {
        if self.song_serial() == Some(serial)
            && let Some(cb) = &self.on_track
        {
            cb(info);
        }
    }

    fn retire(&mut self, s: Box<Song>, time: f64, tc: f64, timers: &mut Timers) {
        for g in [&s.bus.gain, &s.rev.gain, &s.dly.gain] {
            g.cancel_scheduled_values(time);
            g.set_target_at_time(0.0, time, tc);
        }
        let now = self.now();
        let wait = js::max(0.0, time - now) + tc * 8.0 + 3.0;
        if self.realtime {
            timers.set_timeout(now, wait * 1000.0, Task::MusicKill(SongKill(s)));
        } else {
            self.retired.push(*s);
        }
    }

    /// `kill`: the retired song's LFOs stop and its outputs come off.
    pub fn kill(k: SongKill) {
        let s = k.0;
        for o in &s.lfos {
            let _ = o.stop();
        }
        s.bus.disconnect();
        s.rev.disconnect();
        s.dly.disconnect();
    }

    // A mixer channel: level → [drive] → [high-pass] → [chorus] → pan → pump or
    // song filter, with reverb and delay sends.
    fn channel(&mut self, s: &mut Song, o: &Channel) -> GainNode {
        let ctx = self.ctx.clone();
        let inp = ctx.create_gain();
        let mut node: Node = (*inp).clone();
        if let Some(drive) = o.drive.filter(|d| *d != 0.0) {
            let sh = ctx.create_wave_shaper();
            let n = 1024;
            let mut curve = vec![0f32; n];
            for (i, c) in curve.iter_mut().enumerate() {
                let x = (i as f64 * 2.0) / (n as f64 - 1.0) - 1.0;
                *c = (tanh(drive * x) / tanh(drive)) as f32;
            }
            sh.set_curve(Some(&curve));
            sh.set_oversample(OverSampleType::X2);
            let post = ctx.create_biquad_filter();
            post.set_type(BiquadFilterType::Lowpass);
            post.frequency.set_value(o.drive_lp.unwrap_or(5000.0));
            let _ = node.connect(&sh);
            let _ = sh.connect(&post);
            s.keep.push((*sh).clone());
            s.keep.push(node);
            node = (*post).clone();
        }
        if let Some(hpf) = o.hp.filter(|h| *h != 0.0) {
            let hp = ctx.create_biquad_filter();
            hp.set_type(BiquadFilterType::Highpass);
            hp.frequency.set_value(hpf);
            let _ = node.connect(&hp);
            s.keep.push(node);
            node = (*hp).clone();
        }
        let pan = ctx.create_stereo_panner();
        pan.pan.set_value(o.pan.unwrap_or(0.0));
        if let Some(chorus) = o.chorus.filter(|c| *c != 0.0) {
            // Two modulated short delays panned apart, under the dry signal.
            for (base, rate, side) in [(0.011, 0.53, -0.8), (0.017, 0.71, 0.8)] {
                let d = ctx.create_delay(0.05);
                d.delay_time.set_value(base);
                let lfo = ctx.create_oscillator();
                lfo.frequency.set_value(rate);
                let lg = ctx.create_gain();
                lg.gain.set_value(0.0025 * chorus);
                let _ = lfo.connect(&lg);
                let _ = lg.connect(&d.delay_time);
                let _ = lfo.start();
                s.lfos.push(lfo);
                let p = ctx.create_stereo_panner();
                p.pan.set_value(side);
                let wg = ctx.create_gain();
                wg.gain.set_value(0.7);
                let _ = node.connect(&d);
                let _ = d.connect(&wg);
                let _ = wg.connect(&p);
                let _ = p.connect(&pan);
                s.keep
                    .extend([(*d).clone(), (*lg).clone(), (*p).clone(), (*wg).clone()]);
            }
        }
        let _ = node.connect(&pan);
        // The fader comes after the drive, so level never changes the distortion.
        let lvl = ctx.create_gain();
        lvl.gain.set_value(o.level.unwrap_or(1.0));
        let _ = pan.connect(&lvl);
        let _ = lvl.connect(if o.pump { &*s.pump } else { &*s.filter });
        if let Some(r) = o.rev.filter(|r| *r != 0.0) {
            let g = ctx.create_gain();
            g.gain.set_value(r);
            let _ = lvl.connect(&g);
            let _ = g.connect(&s.rev);
            s.keep.push((*g).clone());
        }
        if let Some(d) = o.dly.filter(|d| *d != 0.0) {
            let g = ctx.create_gain();
            g.gain.set_value(d);
            let _ = lvl.connect(&g);
            let _ = g.connect(&s.dly);
            s.keep.push((*g).clone());
        }
        s.keep.extend([node, (*pan).clone(), (*lvl).clone()]);
        inp
    }

    fn drum_voice(&mut self, s: &mut Song, name: &'static str) -> usize {
        if let Some(i) = s.dv.iter().position(|(n, _)| *n == name) {
            return i;
        }
        let ctx = self.ctx.clone();
        let (ks, kg, krev) = kit_default(name).expect("a kit voice");
        let o: KitVoice =
            s.t.kit
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, k)| *k)
                .unwrap_or(KitVoice {
                    s: None,
                    g: None,
                    rev: None,
                    rate: None,
                });
        let k_s = o.s.unwrap_or(ks);
        let k_rev = o.rev.unwrap_or(krev);
        let k_g = kg * o.g.unwrap_or(1.0);
        let g = ctx.create_gain();
        g.gain.set_value(k_g * DRUM_TRIM);
        let _ = g.connect(&s.filter);
        let mut rev = None;
        if k_rev != 0.0 && !k_rev.is_nan() {
            let r = ctx.create_gain();
            r.gain.set_value(k_rev);
            let _ = g.connect(&r);
            let _ = r.connect(&s.rev);
            rev = Some(r);
        }
        let buf = self.kit_buffer(k_s).cloned();
        s.dv.push((
            name,
            DrumVoice {
                input: g,
                buf,
                rate: o.rate.unwrap_or(1.0),
                _rev: rev,
            },
        ));
        s.dv.len() - 1
    }

    /// `hit(S, name, time, vel, rate, offset)`: one drum hit.
    fn hit(
        &mut self,
        s: &mut Song,
        name: &'static str,
        time: f64,
        vel: f64,
        rate: f64,
        offset: f64,
    ) {
        let i = self.drum_voice(s, name);
        let v = &s.dv[i].1;
        let Some(buf) = &v.buf else {
            return;
        };
        let src = self.ctx.create_buffer_source();
        let _ = src.set_buffer(Some(buf));
        src.playback_rate.set_value(v.rate * rate);
        let g = self.ctx.create_gain();
        g.gain.set_value(vel);
        let _ = src.connect(&g);
        let _ = g.connect(&v.input);
        let _ = src.start_with(time, offset, None);
        if name == "kick" && s.t.pump.is_some() {
            Self::duck(s, time);
        }
    }

    // Sidechain pump: every kick ducks the pumped channels and lets them swell back.
    fn duck(s: &Song, time: f64) {
        let pump = s.t.pump.expect("a pump");
        let depth = pump.depth.unwrap_or(0.6);
        let release = pump.release.unwrap_or(0.16);
        let g = &s.pump.gain;
        g.set_value_at_time(1.0, time);
        g.linear_ramp_to_value_at_time(1.0 - depth, time + 0.006);
        g.set_target_at_time(1.0, time + 0.03, release / 3.0);
    }

    // ── The sequencer step ──────────────────────────────────────────
    fn step(&mut self, s: &mut Song, time: f64) {
        let t = s.t;
        let c = s.c.clone();
        let p = self.pos;
        let sec: &Section = &t.sections[p.sec];
        let (st, bar) = (p.step, p.bar);
        let tt = time
            + if st % 2 != 0 {
                truthy_or(t.swing, 0.0) * s.step_dur
            } else {
                0.0
            };
        let bar_dur = s.step_dur * 16.0;
        let last_bar = bar == sec.bars - 1;
        if st == 0 {
            self.bar_start(s, sec, bar, tt, bar_dur);
        }
        // A gap: everything drops out for the last steps before a drop.
        if last_bar
            && let Some(gap) = sec.gap.filter(|g| *g != 0)
            && st >= 16 - gap
        {
            return;
        }

        // Harmony.
        let prog = c.prog(sec.prog.unwrap_or("a")).expect("a progression");
        let pbar = &prog[bar as usize % prog.len()];
        let ci = (st as usize * pbar.len()) / 16;
        let chord = &pbar[ci];
        let chord_start = (st as usize * pbar.len()).is_multiple_of(16);

        // Drums (with the fill over the end of the last bar).
        let idx = (bar * 16 + st) as usize;
        let fill = if last_bar {
            sec.fill.and_then(fill)
        } else {
            None
        };
        let in_fill = fill.as_ref().is_some_and(|f| st as usize >= f.from);
        let solo = self.solo.clone();
        let solo_s = solo.as_deref();
        let lanes = match sec.drums {
            Some(d) if solo_s.is_none_or(|x| x == "drums" || kit_default(x).is_some()) => {
                c.drums(d)
            }
            _ => None,
        };
        if let Some(lanes) = lanes {
            for l in lanes {
                if in_fill {
                    let f = fill.as_ref().expect("a fill");
                    if FILL_REPLACES.contains(&l.voice)
                        || (l.voice == "kick" && f.lanes.iter().any(|(v, _)| *v == "kick"))
                    {
                        continue;
                    }
                }
                if let Some(x) = solo_s
                    && x != "drums"
                    && x != l.voice
                {
                    continue;
                }
                let ch = l.steps[idx % l.steps.len()];
                if ch != '.' {
                    let h = self.human(idx, l.voice);
                    self.hit(s, l.voice, tt, vel(ch) * h, 1.0, 0.0);
                }
            }
        }
        if in_fill && solo_s.is_none_or(|x| x == "drums") {
            let f = fill.as_ref().expect("a fill");
            for &(voice, lane) in f.lanes {
                let ch = lane.as_bytes()[st as usize] as char;
                if ch == '.' {
                    continue;
                }
                let ramp = if f.ramp {
                    0.35 + 0.65 * (st as f64 / 15.0)
                } else {
                    1.0
                };
                self.hit(s, voice, tt, vel(ch) * ramp, 1.0, 0.0);
            }
        }

        // Instruments.
        for &(name, key) in sec.p {
            if let Some(x) = solo_s
                && x != name
            {
                continue;
            }
            let part = c.part(name).expect("a part");
            let Some((_, pat)) = part.pats.iter().find(|(k, _)| *k == key) else {
                continue;
            };
            let pidx = (bar as usize * 16 + st as usize) % pat.len();
            let Some(ev) = &pat[pidx] else {
                continue;
            };
            self.part(s, name, part.part, ev, chord, chord_start, tt);
        }
    }

    /// Deterministic per-hit humanising (±6 %), so repeats never sound pasted.
    fn human(&self, idx: usize, voice: &str) -> f64 {
        let x = idx as f64 * 2654435761.0
            + voice.encode_utf16().count() as f64 * 97.0
            + self.pos.sec as f64 * 131.0
            + self.pos.bar as f64 * 17.0;
        let h = js::to_uint32(x);
        // `h ^= h >>> 15` leaves an int32 (it can be negative).
        let h = (h ^ (h >> 15)) as i32;
        0.94 + (((h % 1000) as f64) / 1000.0) * 0.12
    }

    fn bar_start(&mut self, s: &mut Song, sec: &Section, bar: u32, t: f64, bar_dur: f64) {
        let sec_dur = bar_dur * sec.bars as f64;
        if bar == 0 {
            if sec.crash {
                self.hit(s, "crash", t, 1.0, 1.0, 0.0);
            }
            if sec.drop {
                self.hit(s, "boom", t, 1.0, 1.0, 0.0);
            }
            let f = &s.filter.frequency;
            f.cancel_scheduled_values(t);
            if let Some([a, b]) = sec.lp {
                f.set_value_at_time(a, t);
                f.exponential_ramp_to_value_at_time(b, t + sec_dur);
            } else {
                f.set_value_at_time(20000.0, t);
            }
        }
        let left = sec.bars - bar;
        if let Some(r) = sec.riser.filter(|r| *r != 0)
            && left == r
        {
            self.riser(s, t, bar_dur * r as f64);
        }
        if let Some(d) = sec.down.filter(|d| *d != 0)
            && bar == 0
        {
            self.downlifter(s, t, bar_dur * d as f64);
        }
        // A reverse crash sucking into the next section's downbeat.
        // It can be longer than a bar: then it starts part-way in, on this downbeat.
        if sec.swell && left == 1 {
            let dur = self
                .kit_buffer("revCrash")
                .map_or(f64::NAN, |b| b.duration());
            let start = t + bar_dur - dur;
            self.hit(
                s,
                "revCrash",
                js::max(t, start),
                0.9,
                1.0,
                js::max(0.0, t - start),
            );
        }
    }

    fn noise(&mut self) -> AudioBuffer {
        if let Some(b) = &self.noise_buf {
            return b.clone();
        }
        let b = self.make_noise();
        self.noise_buf = Some(b.clone());
        b
    }

    // White noise through a band-pass that sweeps up while it swells.
    fn riser(&mut self, s: &Song, t: f64, dur: f64) {
        let ctx = self.ctx.clone();
        let src = ctx.create_buffer_source();
        let nb = self.noise();
        let _ = src.set_buffer(Some(&nb));
        src.set_loop(true);
        let bp = ctx.create_biquad_filter();
        bp.set_type(BiquadFilterType::Bandpass);
        bp.q.set_value(1.4);
        bp.frequency.set_value_at_time(350.0, t);
        bp.frequency
            .exponential_ramp_to_value_at_time(7500.0, t + dur);
        let g = ctx.create_gain();
        g.gain.set_value_at_time(0.0001, t);
        g.gain.exponential_ramp_to_value_at_time(
            0.16 * s.t.riser_gain.unwrap_or(1.0),
            t + dur * 0.98,
        );
        g.gain.linear_ramp_to_value_at_time(0.0, t + dur + 0.02);
        let _ = src.connect(&bp);
        let _ = bp.connect(&g);
        let _ = g.connect(&s.bus);
        let rg = ctx.create_gain();
        rg.gain.set_value(0.5);
        let _ = g.connect(&rg);
        let _ = rg.connect(&s.rev);
        let _ = src.start_at(t);
        let _ = src.stop_at(t + dur + 0.05);
    }

    fn downlifter(&mut self, s: &Song, t: f64, dur: f64) {
        let ctx = self.ctx.clone();
        let src = ctx.create_buffer_source();
        let nb = self.noise();
        let _ = src.set_buffer(Some(&nb));
        src.set_loop(true);
        let bp = ctx.create_biquad_filter();
        bp.set_type(BiquadFilterType::Bandpass);
        bp.q.set_value(1.2);
        bp.frequency.set_value_at_time(6000.0, t);
        bp.frequency
            .exponential_ramp_to_value_at_time(250.0, t + dur);
        let g = ctx.create_gain();
        g.gain
            .set_value_at_time(0.12 * s.t.riser_gain.unwrap_or(1.0), t);
        g.gain.exponential_ramp_to_value_at_time(0.0005, t + dur);
        let _ = src.connect(&bp);
        let _ = bp.connect(&g);
        let _ = g.connect(&s.bus);
        let rg = ctx.create_gain();
        rg.gain.set_value(0.6);
        let _ = g.connect(&rg);
        let _ = rg.connect(&s.rev);
        let _ = src.start_at(t);
        let _ = src.stop_at(t + dur + 0.05);
    }

    fn make_noise(&mut self) -> AudioBuffer {
        let ctx = &self.ctx;
        let n = (ctx.sample_rate() * 2.0) as u32;
        let b = ctx.create_buffer(2, n, ctx.sample_rate());
        for c in 0..2 {
            let mut r = self.random.borrow_mut();
            b.with_channel_data_mut(c, |d| {
                for x in d.iter_mut() {
                    *x = (r.next_f64() * 2.0 - 1.0) as f32;
                }
            });
        }
        b
    }

    #[allow(clippy::too_many_arguments)]
    fn part(
        &mut self,
        s: &mut Song,
        name: &'static str,
        part: &Part,
        ev: &Ev,
        chord: &Chord,
        chord_start: bool,
        t: f64,
    ) {
        if let Some(tr) = self.trace_parts.as_mut() {
            tr.push((s.t.id, self.pos.sec, name));
        }
        let dur = ev.len() as f64 * s.step_dur;
        let dest =
            s.ch.iter()
                .find(|(n, _)| *n == name)
                .map(|(_, g)| (**g).clone())
                .expect("a channel");
        let inst = &part.inst;
        match (part.kind, ev) {
            (
                PartKind::Bass,
                Ev::Bass {
                    deg, accent, slide, ..
                },
            ) => {
                let lo = part.lo.unwrap_or(33.0) as i64;
                let root = lo + md12(chord.bass - lo);
                let iv = match deg {
                    't' => chord.iv[1] as f64,
                    's' => chord.iv.get(3).map_or(10.0, |x| *x as f64),
                    d => bass_deg(*d).unwrap_or(0.0),
                };
                let midi = root as f64 + iv;
                let prev = s.last.iter().find(|(n, _)| *n == name).map(|(_, l)| *l);
                let glide_from = prev.filter(|p| p.slide).map(|p| p.midi);
                let gate = if *slide {
                    1.05
                } else {
                    part.gate.unwrap_or(0.9)
                };
                self.note(
                    &dest,
                    t,
                    &[midi],
                    dur * gate,
                    inst,
                    if *accent { 1.0 } else { 0.82 },
                    glide_from,
                );
                set_last(
                    &mut s.last,
                    name,
                    Last {
                        midi,
                        slide: *slide,
                        end: f64::NAN,
                    },
                );
            }
            (PartKind::Chord, Ev::Hit { vel, .. }) => {
                let lo = part.lo.unwrap_or(55.0);
                let cur = s.voicing.iter().position(|(n, _, _)| *n == name);
                let needs = chord_start || cur.is_none_or(|i| s.voicing[i].1 != chord.id);
                if needs {
                    let prev = cur.map(|i| s.voicing[i].2.clone());
                    let notes = voice(chord, lo, prev.as_deref());
                    match cur {
                        Some(i) => s.voicing[i] = (name, chord.id, notes),
                        None => s.voicing.push((name, chord.id, notes)),
                    }
                }
                let i = s
                    .voicing
                    .iter()
                    .position(|(n, _, _)| *n == name)
                    .expect("a voicing");
                let notes = s.voicing[i].2.clone();
                self.note(
                    &dest,
                    t,
                    &notes,
                    dur * part.gate.unwrap_or(1.0),
                    inst,
                    *vel,
                    None,
                );
            }
            (PartKind::Arp, Ev::Tok { idx, accent, .. }) => {
                let v = voice(chord, part.lo.unwrap_or(60.0), None);
                let k = v.len() as f64;
                let i = *idx;
                let j = ((i % k) + k) % k;
                let midi = v[j as usize] + 12.0 * (i / k).floor();
                self.note(
                    &dest,
                    t,
                    &[midi],
                    dur * part.gate.unwrap_or(0.8),
                    inst,
                    if *accent { 1.0 } else { 0.8 },
                    None,
                );
            }
            (_, Ev::Tok { midi, accent, .. }) => {
                let prev = s.last.iter().find(|(n, _)| *n == name).map(|(_, l)| *l);
                let glide = if part.legato {
                    prev.filter(|p| p.end > t - 0.01).map(|p| p.midi)
                } else {
                    None
                };
                self.note(
                    &dest,
                    t,
                    &[*midi],
                    dur * part.gate.unwrap_or(0.95),
                    inst,
                    if *accent { 1.0 } else { 0.85 },
                    glide,
                );
                set_last(
                    &mut s.last,
                    name,
                    Last {
                        midi: *midi,
                        slide: false,
                        end: t + dur,
                    },
                );
            }
            _ => {}
        }
    }

    // ── Instruments ────────────────────────────────────────────────
    /// One note or chord on a synth patch (see tracks.rs for the fields).
    #[allow(clippy::too_many_arguments)]
    pub fn note(
        &self,
        dest: &impl Connectable,
        t: f64,
        midis: &[f64],
        dur: f64,
        p: &Patch,
        vel: f64,
        glide_from: Option<f64>,
    ) {
        let ctx = &self.ctx;
        let a = or(p.a, 0.004);
        let d = or(p.d, 0.25);
        let s = or(p.s, 0.7);
        let r = or(p.r, 0.12);
        let end = t + js::max(dur, a);
        let stop_at = end + r + 0.05;
        let amp = ctx.create_gain();
        let peak = or(p.gain, 0.1) * vel / (midis.len() as f64).sqrt();
        amp.gain.set_value_at_time(0.0, t);
        amp.gain.linear_ramp_to_value_at_time(peak, t + a);
        if s < 1.0 {
            amp.gain.set_target_at_time(peak * s, t + a, d / 3.0);
        }
        amp.gain.set_target_at_time(0.0, end, r / 5.0);
        let _ = amp.connect(dest);
        let oct = or(p.oct, 0.0);

        // FM: carrier + modulator(s) per note, no filter.
        if p.kind == "fm" {
            const DEFAULT_MODS: &[tracks::FmMod] = &[tracks::FmMod {
                ratio: 1.0,
                index: 2.0,
                dec: Some(0.4),
                sus: Some(0.2),
            }];
            for &m in midis {
                let f = mtof(m + 12.0 * oct);
                let dets: Vec<f64> = if truthy(p.fm_detune) {
                    let x = p.fm_detune.expect("fmDetune");
                    vec![-x, x]
                } else {
                    vec![0.0]
                };
                for det in dets {
                    let car = ctx.create_oscillator();
                    car.frequency.set_value(f);
                    car.detune.set_value(det);
                    self.bend(&car, t, f, p, glide_from);
                    for mm in p.mods.unwrap_or(DEFAULT_MODS) {
                        let md = ctx.create_oscillator();
                        md.frequency.set_value(f * mm.ratio);
                        let mg = ctx.create_gain();
                        let depth = mm.index * f * mm.ratio * (0.6 + 0.4 * vel);
                        mg.gain.set_value_at_time(depth, t);
                        mg.gain.set_target_at_time(
                            depth * mm.sus.unwrap_or(0.0),
                            t,
                            mm.dec.unwrap_or(0.3) / 3.0,
                        );
                        let _ = md.connect(&mg);
                        let _ = mg.connect(&car.frequency);
                        let _ = md.start_at(t);
                        let _ = md.stop_at(stop_at);
                    }
                    if truthy(p.fm_detune) {
                        let cg = ctx.create_gain();
                        cg.gain.set_value(0.6);
                        let _ = car.connect(&cg);
                        let _ = cg.connect(&amp);
                    } else {
                        let _ = car.connect(&amp);
                    }
                    let _ = car.start_at(t);
                    let _ = car.stop_at(stop_at);
                }
            }
            return;
        }

        // Subtractive: detuned oscillator stack (split L/R when wide) → low-pass.
        let mut into: Node = (*amp).clone();
        let base_cut = or(p.cutoff, 20000.0);
        if base_cut < 18000.0 || truthy(p.fenv) {
            let lp = ctx.create_biquad_filter();
            lp.set_type(BiquadFilterType::Lowpass);
            lp.q.set_value(or(p.q, 0.8));
            let kt = if truthy(p.keytrack) {
                pow(
                    2.0,
                    ((midis[0] - 60.0) / 12.0) * p.keytrack.expect("keytrack"),
                )
            } else {
                1.0
            };
            let cut = js::min(18000.0, base_cut * kt);
            if truthy(p.fenv) {
                let top = js::min(18000.0, cut * (1.0 + p.fenv.expect("fenv") * vel));
                lp.frequency.set_value_at_time(top, t);
                lp.frequency
                    .set_target_at_time(cut, t, or(p.fdec, 0.15) / 3.0);
            } else if truthy(p.fattack) {
                lp.frequency.set_value_at_time(cut * 0.2, t);
                lp.frequency
                    .set_target_at_time(cut, t, p.fattack.expect("fattack") / 3.0);
            } else {
                lp.frequency.set_value(cut);
            }
            let _ = lp.connect(&amp);
            into = (*lp).clone();
        }
        let nv = or(p.voices, 1.0);
        let spread = or(p.detune, 0.0);
        let mut sides: Vec<Node> = vec![into.clone(), into.clone()];
        let mut split = false;
        if truthy(p.width) && nv > 1.0 {
            let w = p.width.expect("width");
            sides = [-w, w]
                .iter()
                .map(|&pv| {
                    let g = ctx.create_gain();
                    let pn = ctx.create_stereo_panner();
                    pn.pan.set_value(pv);
                    let _ = g.connect(&pn);
                    let _ = pn.connect(&into);
                    (*g).clone()
                })
                .collect();
            split = true;
        }
        let mix = 1.0 / nv.sqrt();
        let mixers: Vec<GainNode> = if split {
            sides.iter().map(|_| ctx.create_gain()).collect()
        } else {
            vec![ctx.create_gain()]
        };
        for (i, g) in mixers.iter().enumerate() {
            g.gain.set_value(mix);
            let _ = g.connect(sides.get(i).unwrap_or(&into));
        }
        let mut vib = None;
        if truthy(p.vib) {
            let v = ctx.create_gain();
            v.gain.set_value_at_time(0.0, t);
            v.gain
                .linear_ramp_to_value_at_time(p.vib.expect("vib"), t + or(p.vib_delay, 0.3) + 0.2);
            let lfo = ctx.create_oscillator();
            lfo.frequency.set_value(or(p.vib_rate, 5.5));
            let _ = lfo.connect(&v);
            let _ = lfo.start_at(t);
            let _ = lfo.stop_at(stop_at);
            vib = Some(v);
        }
        for &m in midis {
            let f = mtof(m + 12.0 * oct);
            let mut v = 0.0;
            while v < nv {
                let o = ctx.create_oscillator();
                if p.kind == "pulse" {
                    o.set_periodic_wave(&self.pulse);
                } else {
                    o.set_type_js(osc_type(p.kind));
                }
                o.frequency.set_value(f);
                o.detune.set_value(if nv > 1.0 {
                    (v / (nv - 1.0) - 0.5) * spread
                } else {
                    0.0
                });
                self.bend(&o, t, f, p, glide_from);
                if let Some(vb) = &vib {
                    let _ = vb.connect(&o.detune);
                }
                let _ = o.connect(&mixers[(v as usize) % mixers.len()]);
                let _ = o.start_at(t);
                let _ = o.stop_at(stop_at);
                v += 1.0;
            }
            if truthy(p.sub) {
                // An octave down, unless that falls below ~40 Hz (felt, not heard, and
                // it eats headroom): then it doubles the fundamental instead.
                let down = f / 2.0 >= 40.0;
                let fs = if down { f / 2.0 } else { f };
                let o = ctx.create_oscillator();
                o.set_type_js(p.sub_type.unwrap_or("sine"));
                o.frequency.set_value(fs);
                self.bend(
                    &o,
                    t,
                    fs,
                    p,
                    glide_from.map(|g| g - if down { 12.0 } else { 0.0 }),
                );
                let g = ctx.create_gain();
                g.gain.set_value(p.sub.expect("sub"));
                let _ = o.connect(&g);
                let _ = g.connect(&into);
                let _ = o.start_at(t);
                let _ = o.stop_at(stop_at);
            }
        }
    }

    fn bend(&self, o: &OscillatorNode, t: f64, f: f64, p: &Patch, glide_from: Option<f64>) {
        if let Some(g) = glide_from {
            let f0 = mtof(g + 12.0 * or(p.oct, 0.0));
            o.frequency.set_value_at_time(f0, t);
            o.frequency
                .exponential_ramp_to_value_at_time(f, t + or(p.glide, 0.06));
        } else if truthy(p.bend) {
            o.frequency
                .set_value_at_time(f * pow(2.0, -p.bend.expect("bend") / 12.0), t);
            o.frequency
                .exponential_ramp_to_value_at_time(f, t + or(p.bend_t, 0.06));
        }
    }
}

/// `x || d` on a field.
fn truthy_or(x: Option<f64>, d: f64) -> f64 {
    if truthy(x) { x.expect("truthy") } else { d }
}

fn set_last(v: &mut Vec<(&'static str, Last)>, name: &'static str, l: Last) {
    match v.iter_mut().find(|(n, _)| *n == name) {
        Some(x) => x.1 = l,
        None => v.push((name, l)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_to_midi_values() {
        assert_eq!(note_to_midi("A4"), Some(69.0));
        assert_eq!(note_to_midi("C#5"), Some(73.0));
        assert_eq!(note_to_midi("Cb4"), Some(59.0));
        assert_eq!(note_to_midi("B#3"), Some(60.0));
        assert_eq!(note_to_midi("C-1"), Some(0.0));
        for bad in ["H4", "C", "C10", "c4", "C#", "Cx4", ""] {
            assert_eq!(note_to_midi(bad), None, "{bad}");
        }
    }

    #[test]
    fn every_track_compiles() {
        for t in tracks::TRACKS {
            let c = compile_track(t);
            for sec in t.sections {
                assert!(c.prog(sec.prog.unwrap_or("a")).is_some(), "{}", t.id);
                if let Some(d) = sec.drums {
                    assert!(c.drums(d).is_some(), "{} drums {d}", t.id);
                }
                for (part, key) in sec.p {
                    let p = c.part(part).expect("a part");
                    assert!(
                        p.pats.iter().any(|(k, _)| k == key),
                        "{} {part}.{key}",
                        t.id
                    );
                }
            }
            for (_, lanes) in &c.drums {
                for l in lanes {
                    assert_eq!(l.steps.len() % 16, 0, "{} {}", t.id, l.voice);
                    assert!(l.steps.iter().all(|c| "Xxo.".contains(*c)));
                    assert!(kit_default(l.voice).is_some());
                }
            }
        }
    }

    #[test]
    fn voicing_moves_least() {
        let c = parse_chord("Am", 1);
        assert_eq!(voice(&c, 57.0, None), vec![57.0, 60.0, 64.0]);
        let f = parse_chord("F", 2);
        let v = voice(&f, 57.0, Some(&[57.0, 60.0, 64.0]));
        assert_eq!(v, vec![57.0, 60.0, 65.0]);
        let s = parse_chord("D/F#", 3);
        assert_eq!(s.bass, 6);
    }
}
