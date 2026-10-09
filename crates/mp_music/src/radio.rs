//! The radio's stations as streams that exist outside the player
//! (docs/vision/radio.md 7): a station's time is the wall clock since its
//! [`EPOCH`], and its programme is a pure function of that time, so every
//! player hears the same song at the same moment and a tune-in catches a
//! song wherever it is.
//!
//! Time is cut into blocks of [`BLOCK_SECS`]. A block's songs come from a
//! generator seeded by the station and the block's index: genres drawn by
//! the station's weights, a seed per song, until the block is full; the
//! last song is fitted to the block's end ([`fit`]): whole 8-bar blocks of
//! its groove are added or dropped, then its tempo nudged (under 4 %) so
//! it ends on the boundary. Songs follow each other with no gap.
//!
//! Nothing here reads a clock: the caller passes the wall time (the
//! client's `Date.now()` or `SystemTime`), in seconds since the Unix epoch.
//!
//! [`Tuner`] is the sound of turning the dial: one station fading into
//! static, and the static fading into the next.

use crate::dsp::{Rng, Svf, TAU};
use crate::genres::{GENRES, Genre};
use crate::js_round;
use crate::track::{Section, Track};

/// When the stations went on air: 2026-01-01T00:00:00Z. The schedule
/// counts from here.
pub const EPOCH: f64 = 1_767_225_600.0;

/// A block of the programme, in seconds (twenty minutes).
pub const BLOCK_SECS: f64 = 1200.0;

/// A song is fitted to the block's end when at least this much of the
/// block is left; with less, the previous song is stretched over the
/// remainder instead.
const MIN_FIT_SECS: f64 = 150.0;
/// Songs are added while the block has at least this much room beyond
/// them, so the fitted last song never has to shrink too far.
const MARGIN_SECS: f64 = 90.0;

/// A station: its name on the dial, the DJ whose clips it plays (the
/// `tools/dj-voice` id), the genres it draws from with their weights, and
/// the seed its programme is made from.
#[derive(Clone, Debug, PartialEq)]
pub struct Station {
    pub key: &'static str,
    pub name: &'static str,
    pub freq: &'static str,
    pub dj: Option<&'static str>,
    /// (genre key, weight).
    pub genres: &'static [(&'static str, f64)],
    pub seed: u32,
}

/// The stations, in dial order (radio.md 7). The index in this list is
/// the radio node's `station` param.
pub const STATIONS: &[Station] = &[
    Station {
        key: "tide",
        name: "The Tide",
        freq: "88.1",
        dj: Some("marisol"),
        genres: &[("house", 0.5), ("garage", 0.2), ("dnb", 0.3)],
        seed: 881,
    },
    Station {
        key: "ridgeline",
        name: "Ridgeline Radio",
        freq: "97.7",
        dj: Some("kit"),
        genres: &[
            ("techno", 0.3),
            ("psytrance", 0.15),
            ("trance", 0.2),
            ("eurobeat", 0.2),
            ("dnb", 0.15),
        ],
        seed: 977,
    },
    Station {
        key: "pacifico",
        name: "Radio Pacífico",
        freq: "104.3",
        // Teo brings in some house and garage of a morning (D1154).
        dj: Some("teo"),
        genres: &[("chicha", 0.7), ("house", 0.2), ("garage", 0.1)],
        seed: 1043,
    },
];

/// A station by key.
pub fn station(key: &str) -> Option<&'static Station> {
    STATIONS.iter().find(|s| s.key == key)
}

/// The station a level tunes to by default (radio.md 7: as `LEVEL_TRACK`
/// picks a song today).
pub fn level_station(level: &str) -> &'static str {
    match level {
        "coast" | "seaside" => "tide",
        "sierra" => "ridgeline",
        "streets" => "tide",
        "desert" => "pacifico",
        _ => "tide",
    }
}

/// A song's length in seconds: `bars * 16 * (60 / bpm / 4)`.
pub fn song_secs(t: &Track) -> f64 {
    t.bars() as f64 * 16.0 * (60.0 / t.bpm / 4.0)
}

/// One song of a block's programme.
#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub genre: &'static str,
    pub seed: u32,
    /// Seconds from the block's start.
    pub start: f64,
    pub secs: f64,
    /// The song as it plays: the grammar's track, fitted when it is the
    /// block's last.
    pub track: Track,
}

/// Where a station is at a moment: the block, the song and the 16th it is
/// on.
#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    pub block: u64,
    pub slot: usize,
    pub track: Track,
    /// The 16th of the song that is playing (from 0).
    pub step: u32,
    /// How far into that 16th, 0..1.
    pub frac: f64,
    /// Seconds of the song left.
    pub left: f64,
}

/// A seeded generator for a block: the station's seed mixed with the
/// block's index (a Weyl step and a multiply, so neighbouring blocks share
/// nothing).
fn block_rng(st: &Station, block: u64) -> Rng {
    let hi = (block >> 32) as u32;
    let lo = block as u32;
    let mut s = st.seed ^ lo.wrapping_mul(0x9E37_79B9) ^ hi.wrapping_mul(0x85EB_CA6B);
    s = s.wrapping_add(0x6D2B_79F5);
    Rng::new(if s == 0 { 1 } else { s })
}

fn genre_by_weight(st: &Station, u: f64) -> &'static Genre {
    let total: f64 = st.genres.iter().map(|(_, w)| w).sum();
    let mut acc = u * total;
    for (key, w) in st.genres {
        acc -= w;
        if acc < 0.0 {
            return GENRES
                .iter()
                .find(|g| g.key == *key)
                .expect("a station's genre exists");
        }
    }
    let key = st.genres[0].0;
    GENRES
        .iter()
        .find(|g| g.key == key)
        .expect("a station's genre exists")
}

/// The programme of one block: songs from its start, the last one fitted
/// to its end. Deterministic in (station, block).
pub fn block(st: &Station, block: u64) -> Vec<Slot> {
    let mut r = block_rng(st, block);
    let mut slots: Vec<Slot> = Vec::new();
    let mut total = 0.0;
    loop {
        let g = genre_by_weight(st, r.next());
        let seed = 1 + (r.next() * 99999.0).floor() as u32;
        let track = (g.make)(seed, None);
        let secs = song_secs(&track);
        if total + secs <= BLOCK_SECS - MARGIN_SECS {
            slots.push(Slot {
                genre: g.key,
                seed,
                start: total,
                secs,
                track,
            });
            total += secs;
            continue;
        }
        let left = BLOCK_SECS - total;
        if left >= MIN_FIT_SECS || slots.is_empty() {
            let track = fit(track, left);
            slots.push(Slot {
                genre: g.key,
                seed,
                start: total,
                secs: left,
                track,
            });
        } else {
            // Too little room for a song: the last one plays on to the end.
            let last = slots.last_mut().expect("a song");
            let target = last.secs + left;
            last.track = fit(last.track.clone(), target);
            last.secs = target;
        }
        break;
    }
    slots
}

/// The section whose 8-bar blocks are added or dropped to fit: the last
/// `drop` (a chorus or the main groove), else the middle section.
fn anchor(t: &Track) -> usize {
    t.sections
        .iter()
        .rposition(|s| s.drop == Some(true))
        .unwrap_or(t.sections.len() / 2)
}

/// Fits a song to `target` seconds: whole 8-bar blocks of its groove are
/// added (copies of the anchor section after it) or dropped (sections
/// after the anchor with the anchor's drums, then any middle section), so
/// the length is within half a block of the target; then the tempo is
/// nudged so the song ends exactly on it.
pub fn fit(mut t: Track, target: f64) -> Track {
    let step_dur = 60.0 / t.bpm / 4.0;
    let block_secs = 8.0 * 16.0 * step_dur;
    let secs = song_secs(&t);
    let k = js_round((target - secs) / block_secs) as i64;
    if k > 0 {
        let a = anchor(&t);
        let copy: Section = Section {
            // A copy plays the groove on: no crash or drop again, no fill
            // or riser until the real last block.
            crash: None,
            drop: None,
            down: None,
            riser: None,
            swell: None,
            fill: None,
            gap: None,
            ..t.sections[a].clone()
        };
        for _ in 0..k {
            t.sections.insert(a + 1, copy.clone());
        }
    } else if k < 0 {
        let mut n = (-k) as usize;
        let a = anchor(&t);
        let drums = t.sections[a].drums.clone();
        // After the anchor, sections with its drums, from the end.
        let mut i = t.sections.len().saturating_sub(2);
        while n > 0 && i > a {
            if t.sections[i].drums == drums && t.sections[i].bars == 8 {
                t.sections.remove(i);
                n -= 1;
            }
            i -= 1;
        }
        // Then any middle section, from the end, never the first or last.
        let mut i = t.sections.len().saturating_sub(2);
        while n > 0 && i > 0 && t.sections.len() > 3 {
            if t.sections[i].bars == 8 {
                t.sections.remove(i);
                n -= 1;
            }
            i -= 1;
        }
    }
    let bars = t.bars() as f64;
    t.bpm = bars * 16.0 * 60.0 / 4.0 / target;
    t
}

/// What the block index and the seconds into it are at `wall`.
pub fn block_at(wall: f64) -> (u64, f64) {
    let since = (wall - EPOCH).max(0.0);
    let b = (since / BLOCK_SECS).floor();
    (b as u64, since - b * BLOCK_SECS)
}

/// Where a station is at `wall` (Unix seconds).
pub fn cue(st: &Station, wall: f64) -> Cue {
    let (b, into) = block_at(wall);
    let slots = block(st, b);
    cue_in(b, &slots, into)
}

/// The cue within a block's slots at `into` seconds from its start.
pub fn cue_in(block: u64, slots: &[Slot], into: f64) -> Cue {
    let i = slots.iter().rposition(|s| s.start <= into).unwrap_or(0);
    let s = &slots[i];
    let step_dur = 60.0 / s.track.bpm / 4.0;
    let steps = s.track.bars() as f64 * 16.0;
    let pos = ((into - s.start) / step_dur).clamp(0.0, steps - 1e-9);
    let step = pos.floor();
    Cue {
        block,
        slot: i,
        track: s.track.clone(),
        step: step as u32,
        frac: pos - step,
        left: s.start + s.secs - into,
    }
}

/// The slot after `slot` of `block`: the next of the same block, or the
/// first of the next.
pub fn next_slot(st: &Station, block: u64, slot: usize, slots: &[Slot]) -> (u64, usize, Vec<Slot>) {
    if slot + 1 < slots.len() {
        (block, slot + 1, slots.to_vec())
    } else {
        let b = block + 1;
        let next = self::block(st, b);
        (b, 0, next)
    }
}

/// The wall time split for the node's two `f32` params: whole days since
/// the Unix epoch, and seconds into the day.
pub fn wall_parts(wall: f64) -> (f64, f64) {
    let day = (wall / 86400.0).floor();
    (day, wall - day * 86400.0)
}

/// The wall time from the node's two params.
pub fn wall_from(day: f64, sec: f64) -> f64 {
    day * 86400.0 + sec
}

// ── The tuner ────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TunerState {
    /// Nothing between the station and the speaker.
    Clear,
    /// A turn of the dial (or the radio switching off): see [`Tuner`].
    Turning,
    /// Off and quiet.
    Silent,
}

/// The vowels a far-off voice says: the first two formants.
const VOWELS: [(f64, f64); 5] = [
    (730.0, 1090.0),
    (530.0, 1840.0),
    (270.0, 2290.0),
    (570.0, 840.0),
    (440.0, 1020.0),
];

/// Where the muffling filter sits with the station all but gone, and
/// fully open.
const MUFFLED_HZ: f64 = 350.0;
const OPEN_HZ: f64 = 20000.0;

/// The sound of turning the dial (radio.md 2.1). The owner (2026-10-09):
/// tuning off one station and onto another should be symmetric, "the
/// music fades to static, which then fades back to the other music", and
/// the static should have some character so no two turns sound alike.
///
/// A turn has three parts: the station we leave fades into static (its
/// sound muffling as it goes), the static holds a moment, and the static
/// fades into the new station (opening up as it comes). The fade in is
/// the fade out played backwards: equal-power curves, the same length,
/// the muffling filter closing on the way out and opening on the way in.
/// Nothing slides in pitch, so the turn doesn't sweep past in one
/// direction. Switching on fades in from silence through the static;
/// switching off fades into the static and out to silence.
///
/// The static's character is drawn afresh for each turn and holds still
/// through it: the hiss's colour and the receiver's bandwidth, a flutter
/// (the signal coming and going), crackle, now and then a steady
/// heterodyne whistle, and now and then a far-off station under the
/// noise, a voice or a chord, garbled.
pub struct Tuner {
    sr: f64,
    rng: Rng,
    state: TunerState,
    /// Samples into the turn, and its parts' lengths: the fade out (and
    /// the fade in, the same) and the static held between.
    t: usize,
    fade: usize,
    hold: usize,
    /// Whether a station is left (fades out) and one arrives (fades in).
    from: bool,
    to: bool,
    /// The muffling filters: on the station leaving, on the one arriving.
    muffle_out: [Svf; 2],
    muffle_in: [Svf; 2],
    // The static's character.
    level: f64,
    colour: Svf,
    white: f64,
    band: Svf,
    flutter: (f64, f64, f64),
    flutter_ph: f64,
    crackle_rate: f64,
    crack: f64,
    whistle: (f64, f64),
    whistle_ph: f64,
    /// The far-off station: its level (0: none), talk or music, its pitch,
    /// and its voice.
    far: f64,
    far_talk: bool,
    far_pitch: f64,
    far_ph: [f64; 3],
    tone: Svf,
    formant: [Svf; 2],
    syllable: usize,
}

impl Tuner {
    pub fn new(seed: u32, sr: f64) -> Tuner {
        let mut t = Tuner {
            sr,
            rng: Rng::new(seed),
            state: TunerState::Silent,
            t: 0,
            fade: 1,
            hold: 0,
            from: false,
            to: false,
            muffle_out: [Svf::new(), Svf::new()],
            muffle_in: [Svf::new(), Svf::new()],
            level: 0.18,
            colour: Svf::new(),
            white: 0.3,
            band: Svf::new(),
            flutter: (0.0, 4.0, 0.0),
            flutter_ph: 0.0,
            crackle_rate: 0.0,
            crack: 0.0,
            whistle: (0.0, 0.0),
            whistle_ph: 0.0,
            far: 0.0,
            far_talk: false,
            far_pitch: 110.0,
            far_ph: [0.0; 3],
            tone: Svf::new(),
            formant: [Svf::new(), Svf::new()],
            syllable: 0,
        };
        t.colour.set(2000.0, 0.8, sr);
        t.band.set(8000.0, 0.7, sr);
        t
    }

    /// A turn of the dial onto another station, from the one playing (if
    /// any). Tuned again while the new station is fading in, the turn
    /// runs back from the same point, so the sound never jumps: the caller
    /// then hands over the station that was arriving as the one leaving.
    pub fn tune(&mut self) {
        self.start(true);
    }

    /// Switched off: the station fades into static and the static away.
    pub fn off(&mut self) {
        self.start(false);
    }

    fn start(&mut self, to: bool) {
        match self.state {
            TunerState::Turning if self.t < self.fade + self.hold => {
                // Still leaving, or in the static: only the arrival changes.
                self.to = to;
                return;
            }
            TunerState::Turning => {
                // Arriving: the arrival becomes the station leaving, at the
                // mirrored point, where the gains are the same.
                let total = 2 * self.fade + self.hold;
                self.t = total.saturating_sub(self.t);
                self.from = self.to;
                self.muffle_out = self.muffle_in;
                self.to = to;
                return;
            }
            TunerState::Clear => self.from = true,
            TunerState::Silent => self.from = false,
        }
        if !to && !self.from {
            return; // off, and already quiet
        }
        self.to = to;
        self.state = TunerState::Turning;
        self.t = 0;
        self.draw();
    }

    /// The turn's lengths and its static's character.
    fn draw(&mut self) {
        let sr = self.sr;
        let r = &mut self.rng;
        self.fade = ((0.22 + r.next() * 0.2) * sr) as usize;
        self.hold = ((0.08 + r.next() * 0.3) * sr) as usize;
        // AM-like (narrow, crackly, whistling) or FM-like (wide, smooth).
        let am = r.next();
        self.level = 0.17 + r.next() * 0.11;
        self.colour
            .set(700.0 + r.next() * 3800.0, 0.5 + r.next() * 1.2, sr);
        self.white = (1.0 - am) * (0.15 + r.next() * 0.35);
        self.band.set(9500.0 - am * 6000.0, 0.7, sr);
        // The signal coming and going: depth, rate (Hz) and a second rate
        // against it, so the flutter isn't a plain tremolo.
        self.flutter = (r.next() * 0.55, 2.0 + r.next() * 8.0, 0.3 + r.next() * 1.5);
        self.crackle_rate = am * am * r.next() * 45.0;
        self.whistle = if am > 0.5 && r.next() < 0.5 {
            (0.01 + r.next() * 0.02, 900.0 + r.next() * 5000.0)
        } else {
            (0.0, 0.0)
        };
        self.far = if r.next() < 0.55 {
            0.3 + r.next() * 0.7
        } else {
            0.0
        };
        self.far_talk = r.next() < 0.5;
        self.far_pitch = if self.far_talk {
            95.0 + r.next() * 130.0
        } else {
            110.0 * 2f64.powf(r.next() * 2.0)
        };
        self.tone
            .set(800.0 + r.next() * 1400.0, 0.8 + r.next() * 0.8, sr);
        self.syllable = 0;
        self.flutter_ph = 0.0;
        for f in self.muffle_in.iter_mut() {
            *f = Svf::new();
            f.set(MUFFLED_HZ, 0.7, sr);
        }
        if self.from {
            for f in self.muffle_out.iter_mut() {
                *f = Svf::new();
                f.set(OPEN_HZ.min(sr * 0.45), 0.7, sr);
            }
        }
    }

    /// Straight through (a node built already tuned).
    pub fn on(&mut self) {
        self.state = TunerState::Clear;
    }

    /// Whether the station arriving is heard at all.
    pub fn station_audible(&self) -> bool {
        match self.state {
            TunerState::Clear => true,
            TunerState::Silent => false,
            TunerState::Turning => self.to && self.t > self.fade + self.hold,
        }
    }

    /// Whether the station being left is still heard: the caller keeps
    /// it playing and passes it to [`Tuner::process`] until it isn't.
    pub fn leaving(&self) -> bool {
        self.state == TunerState::Turning && self.from && self.t < self.fade
    }

    /// Whether a turn (or a switch off) is under way.
    pub fn turning(&self) -> bool {
        self.state == TunerState::Turning
    }

    /// One sample of static, at `s` of its full level.
    fn static_sample(&mut self, s: f64) -> f64 {
        let sr = self.sr;
        let noise = self.rng.next() * 2.0 - 1.0;
        self.colour.run(noise);
        let mut hiss = self.colour.bp * (1.0 - self.white) + noise * self.white;
        // Flutter: two slow rates beating, so it wanders.
        let (depth, f1, f2) = self.flutter;
        self.flutter_ph += 1.0 / sr;
        let ph = self.flutter_ph;
        let wob = 0.5 + 0.5 * ((TAU * f1 * ph).sin() * (TAU * f2 * ph).cos());
        hiss *= 1.0 - depth * wob;
        // Crackle: sparse impulses, a few milliseconds each.
        if self.rng.next() < self.crackle_rate / sr {
            self.crack = 0.1 + self.rng.next() * 0.25;
        }
        self.crack *= 1.0 - 1.0 / (0.002 * sr);
        let crackle = self.crack * (self.rng.next() * 2.0 - 1.0);
        // A steady whistle: another carrier close by, its beat held.
        let (wl, wf) = self.whistle;
        let mut whistle = 0.0;
        if wl > 0.0 {
            self.whistle_ph = (self.whistle_ph + wf / sr).fract();
            whistle = (TAU * self.whistle_ph).sin() * wl;
        }
        // A far-off station under the noise, coming and going with it.
        let mut far = 0.0;
        if self.far > 0.0 {
            if self.syllable == 0 {
                let v = VOWELS[(self.rng.next() * VOWELS.len() as f64) as usize % VOWELS.len()];
                self.formant[0].set(v.0, 6.0, sr);
                self.formant[1].set(v.1, 8.0, sr);
                self.syllable = ((0.07 + self.rng.next() * 0.11) * sr) as usize;
            }
            self.syllable -= 1;
            let mut src = 0.0;
            if self.far_talk {
                self.far_ph[0] = (self.far_ph[0] + self.far_pitch / sr).fract();
                src = self.far_ph[0] * 2.0 - 1.0;
            } else {
                for (j, k) in [1.0, 1.25, 1.5].iter().enumerate() {
                    self.far_ph[j] = (self.far_ph[j] + self.far_pitch * k / sr).fract();
                    src += if self.far_ph[j] < 0.5 { 0.33 } else { -0.33 };
                }
            }
            self.formant[0].run(src);
            self.formant[1].run(src);
            self.tone.run(src);
            let voice = if self.far_talk {
                self.formant[0].bp + self.formant[1].bp * 0.7
            } else {
                self.tone.bp
            };
            let garble = 1.0 + (self.rng.next() * 2.0 - 1.0) * 0.6;
            far = (voice * 3.0 * garble).tanh() * 0.05 * self.far * wob.max(0.3);
        }
        self.band.run(hiss * self.level + crackle + whistle + far);
        self.band.lp * s
    }

    /// Applies the tuner to one block, in place: `l`/`r` hold the station
    /// arriving (or anything, when switching off), `leaving` the station
    /// being left while [`Tuner::leaving`] says so.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], leaving: Option<(&[f32], &[f32])>) {
        match self.state {
            TunerState::Clear => {}
            TunerState::Silent => {
                l.fill(0.0);
                r.fill(0.0);
            }
            TunerState::Turning => {
                let sr = self.sr;
                let (fade, hold) = (self.fade as f64, self.hold);
                for i in 0..l.len() {
                    let t = self.t;
                    // The position in the turn: rising to 1 over the fade
                    // out, 1 in the hold, falling over the fade in.
                    let x = if t < self.fade {
                        t as f64 / fade
                    } else if t < self.fade + hold {
                        1.0
                    } else {
                        1.0 - ((t - self.fade - hold) as f64 / fade).min(1.0)
                    };
                    let q = std::f64::consts::FRAC_PI_2 * x;
                    let (music, stat) = (q.cos(), q.sin());
                    // The muffling: open at x = 0, closed at x = 1.
                    if t.is_multiple_of(32) {
                        let hz = (OPEN_HZ * (MUFFLED_HZ / OPEN_HZ).powf(x)).min(sr * 0.45);
                        for f in self.muffle_out.iter_mut().chain(self.muffle_in.iter_mut()) {
                            f.set(hz, 0.7, sr);
                        }
                    }
                    let st = self.static_sample(stat);
                    let (mut yl, mut yr) = (st, st);
                    if t < self.fade {
                        if self.from
                            && let Some((ll, lr)) = leaving
                        {
                            self.muffle_out[0].run(ll[i] as f64);
                            self.muffle_out[1].run(lr[i] as f64);
                            yl += self.muffle_out[0].lp * music;
                            yr += self.muffle_out[1].lp * music;
                        }
                    } else if t >= self.fade + hold && self.to {
                        self.muffle_in[0].run(l[i] as f64);
                        self.muffle_in[1].run(r[i] as f64);
                        yl += self.muffle_in[0].lp * music;
                        yr += self.muffle_in[1].lp * music;
                    }
                    l[i] = yl as f32;
                    r[i] = yr as f32;
                    self.t += 1;
                    if self.t >= 2 * self.fade + hold {
                        // The turn is over: the rest of the block is the
                        // station, or silence.
                        self.state = if self.to {
                            TunerState::Clear
                        } else {
                            TunerState::Silent
                        };
                        if !self.to {
                            for j in i + 1..l.len() {
                                l[j] = 0.0;
                                r[j] = 0.0;
                            }
                        }
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_fill_exactly_and_repeat() {
        let st = station("tide").unwrap();
        for b in [0u64, 1, 7, 1000] {
            let slots = block(st, b);
            assert!(slots.len() >= 3, "block {b}: {} songs", slots.len());
            let mut t = 0.0;
            for s in &slots {
                assert!((s.start - t).abs() < 1e-9, "no gap");
                assert!(
                    (song_secs(&s.track) - s.secs).abs() < 1e-6,
                    "the track lasts its slot"
                );
                t += s.secs;
            }
            assert!(
                (t - BLOCK_SECS).abs() < 1e-6,
                "block {b} ends on the boundary: {t}"
            );
            assert_eq!(slots, block(st, b), "deterministic");
        }
        assert_ne!(block(st, 0)[0].seed, block(st, 1)[0].seed);
    }

    #[test]
    fn the_fit_nudges_the_tempo_a_little() {
        let st = station("ridgeline").unwrap();
        for b in 0..6u64 {
            let slots = block(st, b);
            let last = slots.last().unwrap();
            let fresh =
                (GENRES.iter().find(|g| g.key == last.genre).unwrap().make)(last.seed, None);
            let ratio = last.track.bpm / fresh.bpm;
            assert!(
                (ratio - 1.0).abs() < 0.05,
                "block {b}: tempo nudged by {ratio}"
            );
        }
    }

    #[test]
    fn a_cue_lands_inside_its_song_and_moves_with_time() {
        let st = station("pacifico").unwrap();
        let wall = EPOCH + 3.0 * 86400.0 + 1234.5;
        let c = cue(st, wall);
        assert!(c.frac >= 0.0 && c.frac < 1.0);
        assert!(c.left > 0.0 && c.left <= BLOCK_SECS);
        let step_dur = 60.0 / c.track.bpm / 4.0;
        let later = cue(st, wall + 2.0 * step_dur);
        if later.slot == c.slot {
            let moved = (later.step as f64 + later.frac) - (c.step as f64 + c.frac);
            assert!((moved - 2.0).abs() < 1e-6, "moved {moved} steps");
        }
        let (day, sec) = wall_parts(wall);
        assert!((wall_from(day, sec) - wall).abs() < 1e-6);
        assert!((0.0..86400.0).contains(&sec));
    }

    /// One turn from a steady "station" (a tone) at `old` to another at
    /// `new`, returning the output with the leaving and arriving levels
    /// measured per 10 ms window.
    fn turn(t: &mut Tuner, sr: f64, from: bool) -> Vec<f32> {
        let (mut l, mut r) = (vec![0.0f32; 128], vec![0.0f32; 128]);
        let (mut ol, mut or) = (vec![0.0f32; 128], vec![0.0f32; 128]);
        let mut out = Vec::new();
        let mut n = 0usize;
        t.tune();
        while t.turning() {
            for i in 0..128 {
                let k = (n + i) as f64 / sr;
                l[i] = (0.3 * (TAU * 440.0 * k).sin()) as f32;
                r[i] = l[i];
                ol[i] = (0.3 * (TAU * 220.0 * k).sin()) as f32;
                or[i] = ol[i];
            }
            let leaving = (from && t.leaving()).then_some((&ol[..], &or[..]));
            t.process(&mut l, &mut r, leaving);
            out.extend_from_slice(&l);
            n += 128;
            assert!(n < (5.0 * sr) as usize, "the turn ends");
        }
        out
    }

    #[test]
    fn a_turn_fades_out_into_static_and_back_in_symmetrically() {
        let sr = 48000.0;
        let mut t = Tuner::new(5, sr);
        t.on();
        let out = turn(&mut t, sr, true);
        assert!(out.iter().all(|x| x.is_finite() && x.abs() < 1.0));
        // The level per 20 ms window: from the old station down into the
        // static and back up to the new one, about the same on both sides.
        let w = (0.02 * sr) as usize;
        let rms: Vec<f64> = out
            .chunks(w)
            .map(|c| (c.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / c.len() as f64).sqrt())
            .collect();
        let n = rms.len();
        let (head, tail) = (rms[0], rms[n - 2]);
        assert!(
            (head / tail).ln().abs() < 0.4,
            "it starts and ends at the stations' level: {head:.3} vs {tail:.3}"
        );
        // The static's middle is not the stations: the old tone (220 Hz)
        // and the new (440 Hz) are both all but gone there.
        let mid = &out[out.len() / 2 - w..out.len() / 2 + w];
        let tone = |f: f64| {
            let (mut c, mut s) = (0.0, 0.0);
            for (i, x) in mid.iter().enumerate() {
                let p = TAU * f * i as f64 / sr;
                c += *x as f64 * p.cos();
                s += *x as f64 * p.sin();
            }
            (c * c + s * s).sqrt() / mid.len() as f64
        };
        assert!(
            tone(220.0) < 0.02 && tone(440.0) < 0.02,
            "{} {}",
            tone(220.0),
            tone(440.0)
        );
        // The fade out and the fade in last the same: the level envelope
        // is symmetric about the turn's middle.
        let half = n / 2;
        let mut asym = 0.0;
        for k in 0..half.min(8) {
            asym += (rms[k] - rms[n - 1 - k]).abs();
        }
        assert!(asym / 8.0 < 0.06, "symmetric: {rms:?}");
        assert!(t.station_audible() && !t.turning());
    }

    #[test]
    fn no_two_turns_sound_alike() {
        let sr = 48000.0;
        let mut t = Tuner::new(11, sr);
        let turns: Vec<Vec<f32>> = (0..4).map(|_| turn(&mut t, sr, true)).collect();
        for i in 0..turns.len() {
            for j in i + 1..turns.len() {
                assert_ne!(turns[i], turns[j], "turns {i} and {j} are the same");
            }
        }
        let lens: Vec<usize> = turns.iter().map(Vec::len).collect();
        assert!(lens.iter().any(|n| *n != lens[0]), "lengths vary: {lens:?}");
    }

    #[test]
    fn switching_on_and_off_fades_through_the_static() {
        let sr = 48000.0;
        let mut t = Tuner::new(3, sr);
        let (mut l, mut r) = (vec![0.5f32; 128], vec![0.5f32; 128]);
        t.process(&mut l, &mut r, None);
        assert!(l.iter().all(|x| *x == 0.0), "silent before a tune");
        // On: from silence, through the static, to the station.
        let out = turn(&mut t, sr, false);
        assert!(out[..64].iter().all(|x| x.abs() < 0.02), "it starts quiet");
        for _ in 0..20 {
            l.fill(0.5);
            r.fill(0.5);
            t.process(&mut l, &mut r, None);
        }
        assert!((l[0] - 0.5).abs() < 1e-6, "straight through once tuned");
        // Off: the station fades out into the static, then silence.
        t.off();
        assert!(t.leaving());
        let mut blocks = 0;
        while t.turning() {
            l.fill(0.0);
            r.fill(0.0);
            let s = [0.5f32; 128];
            let lv = t.leaving().then_some((&s[..], &s[..]));
            t.process(&mut l, &mut r, lv);
            blocks += 1;
            assert!(blocks < 1000);
        }
        l.fill(0.5);
        t.process(&mut l, &mut r, None);
        assert!(l.iter().all(|x| *x == 0.0), "silent after off");
    }

    #[test]
    fn tuning_again_mid_turn_never_jumps() {
        let sr = 48000.0;
        let mut t = Tuner::new(9, sr);
        t.on();
        t.tune();
        let (mut l, mut r) = (vec![0.0f32; 128], vec![0.0f32; 128]);
        let s = [0.0f32; 128];
        // Into the fade in, then turn again: it runs back from that point.
        while !t.station_audible() {
            let lv = t.leaving().then_some((&s[..], &s[..]));
            t.process(&mut l, &mut r, lv);
        }
        for _ in 0..10 {
            t.process(&mut l, &mut r, None);
        }
        let before = t.t;
        t.tune();
        let total = 2 * t.fade + t.hold;
        assert_eq!(t.t, total - before, "mirrored");
        assert!(t.leaving(), "the arriving station is now the one leaving");
    }
}
