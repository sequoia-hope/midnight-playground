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
//! [`Tuner`] is the sound of turning the dial: the static sweep between
//! stations and the lock onto the next one.

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
    On,
    /// The dial moving: static, whistles, no station.
    Sweep,
    /// The station coming up through the opening filter.
    Lock,
    /// Switched off: the click and the hiss falling away, then silence.
    Off,
    Silent,
}

/// Most other stations the dial passes on one turn.
const MAX_PASSED: usize = 4;

/// A station the dial passes on the way (one that isn't ours): where it
/// sits on the sweep, how wide its signal is, and what it is playing.
#[derive(Clone, Copy, Debug)]
struct Passed {
    /// Its place on the dial, 0 (where the turn starts) to 1 (ours).
    pos: f64,
    /// The half-width of its signal, on the same scale.
    width: f64,
    /// Hertz of beat per unit of dial: the heterodyne whistle falls to
    /// zero on the carrier and rises again past it.
    beat: f64,
    /// Talk (a voice's formants) or music (a chord).
    talk: bool,
    /// The voice's pitch, or the chord's root, in hertz.
    pitch: f64,
    level: f64,
    ph: [f64; 3],
    w_ph: f64,
}

/// The vowels the far-off voices say: the first two formants.
const VOWELS: [(f64, f64); 5] = [
    (730.0, 1090.0),
    (530.0, 1840.0),
    (270.0, 2290.0),
    (570.0, 840.0),
    (440.0, 1020.0),
];

/// The sound of the dial (radio.md 2.1), applied in place to the
/// station's output. A turn is modelled, not played back: the dial moves
/// from 0 to our station at 1 along a drawn path (its speed curve and the
/// hand's wobble), past a few other stations scattered on the way. Near
/// each one the hiss quietens (an FM receiver's capture) and a scrap of
/// it comes through, a voice or a bar of music, distorted at the edges;
/// on an AM-like turn each also whistles, its beat falling to nothing on
/// the carrier and rising past it, and the static crackles. Our station
/// can bleed in before the dial gets there. Then the lock, the station
/// opening up from a few hundred hertz to full width; off is a click and
/// the hiss falling away. Every part is drawn from the seed, so no two
/// turns sound alike.
pub struct Tuner {
    sr: f64,
    rng: Rng,
    state: TunerState,
    t: usize,
    len: usize,
    bp: Svf,
    lock: [Svf; 2],
    hiss: f64,
    // The turn.
    /// The dial's speed curve (an exponent on the turn's progress) and
    /// the hand's wobble: depth and cycles over the turn.
    curve: f64,
    wobble: (f64, f64),
    passed: [Passed; MAX_PASSED],
    n_passed: usize,
    /// How AM-like the turn is (whistles, crackle) against FM's clean hiss.
    am: f64,
    hiss_level: f64,
    /// The receiver's audio bandwidth: FM's is wider than AM's.
    band: Svf,
    /// Crackle: impulses per second, and the one sounding.
    crackle_rate: f64,
    crack: f64,
    /// The scraps' radio voice: a band-pass, and the formant filters.
    tone: Svf,
    formant: [Svf; 2],
    /// The syllable: its vowel's formants, and when the next starts.
    vowel: (f64, f64),
    syllable: usize,
    /// How much of our station leaks in before the lock, and from where.
    bleed: f64,
    bleed_from: f64,
    bleed_lp: [Svf; 2],
    // The lock.
    lock_secs: f64,
    lock_from: f64,
    click: f64,
}

impl Tuner {
    pub fn new(seed: u32, sr: f64) -> Tuner {
        let none = Passed {
            pos: 0.0,
            width: 0.0,
            beat: 0.0,
            talk: false,
            pitch: 0.0,
            level: 0.0,
            ph: [0.0; 3],
            w_ph: 0.0,
        };
        let mut t = Tuner {
            sr,
            rng: Rng::new(seed),
            state: TunerState::Silent,
            t: 0,
            len: 1,
            bp: Svf::new(),
            lock: [Svf::new(), Svf::new()],
            hiss: 0.0,
            curve: 1.0,
            wobble: (0.0, 1.0),
            passed: [none; MAX_PASSED],
            n_passed: 0,
            am: 0.0,
            hiss_level: 0.2,
            band: Svf::new(),
            crackle_rate: 0.0,
            crack: 0.0,
            tone: Svf::new(),
            formant: [Svf::new(), Svf::new()],
            vowel: VOWELS[0],
            syllable: 0,
            bleed: 0.0,
            bleed_from: 0.8,
            bleed_lp: [Svf::new(), Svf::new()],
            lock_secs: 0.3,
            lock_from: 400.0,
            click: 0.3,
        };
        t.bp.set(1200.0, 2.0, sr);
        for f in &mut t.lock {
            f.set(20000.0, 0.7, sr);
        }
        t
    }

    /// The dial turned: a new turn is drawn (0.45 to 1 s), then the lock.
    pub fn tune(&mut self) {
        let sr = self.sr;
        self.state = TunerState::Sweep;
        self.t = 0;
        self.len = ((0.45 + self.rng.next() * 0.55) * sr) as usize;
        // A slow start or a slow finish, and a hand that isn't steady.
        self.curve = 0.6 + self.rng.next() * 1.0;
        self.wobble = (self.rng.next() * 0.04, 1.0 + self.rng.next() * 3.0);
        self.am = if self.rng.next() < 0.4 {
            self.rng.next() * 0.3
        } else {
            0.4 + self.rng.next() * 0.6
        };
        self.hiss_level = 0.13 + self.rng.next() * 0.1;
        self.crackle_rate = self.am * self.rng.next() * 40.0;
        self.band.set(9000.0 - self.am * 5500.0, 0.7, sr);
        // The hiss's colour: from dull to bright.
        self.bp.set(
            1200.0 + self.rng.next() * 3000.0,
            0.5 + self.rng.next() * 0.8,
            sr,
        );
        self.tone.set(
            900.0 + self.rng.next() * 1200.0,
            0.8 + self.rng.next() * 0.8,
            sr,
        );
        // The stations on the way: none at all now and then, up to four.
        let u = self.rng.next();
        self.n_passed = if u < 0.12 {
            0
        } else {
            1 + (self.rng.next() * MAX_PASSED as f64) as usize
        }
        .min(MAX_PASSED);
        for k in 0..self.n_passed {
            let slot = (k as f64 + 0.15 + self.rng.next() * 0.7) / self.n_passed as f64;
            let talk = self.rng.next() < 0.5;
            self.passed[k] = Passed {
                pos: 0.05 + slot * 0.75,
                width: 0.03 + self.rng.next() * 0.06,
                beat: 4000.0 + self.rng.next() * 9000.0,
                talk,
                pitch: if talk {
                    95.0 + self.rng.next() * 130.0
                } else {
                    110.0 * 2f64.powf(self.rng.next() * 2.0)
                },
                level: 0.3 + self.rng.next() * 0.7,
                ph: [0.0; 3],
                w_ph: 0.0,
            };
        }
        self.syllable = 0;
        // Our station: sometimes heard creeping in before the dial lands.
        self.bleed = if self.rng.next() < 0.6 {
            0.3 + self.rng.next() * 0.5
        } else {
            0.0
        };
        self.bleed_from = 0.7 + self.rng.next() * 0.2;
        for f in &mut self.bleed_lp {
            f.set(500.0 + self.rng.next() * 1500.0, 0.9, sr);
        }
        self.lock_secs = 0.2 + self.rng.next() * 0.25;
        self.lock_from = 250.0 + self.rng.next() * 500.0;
        self.hiss = 1.0;
    }

    /// One sample of the sweep, with `inp` the station's own output (for
    /// the bleed): the static, the stations passed, the crackle.
    fn sweep_sample(&mut self, u: f64, inp: (f64, f64)) -> (f64, f64) {
        let sr = self.sr;
        let (wd, wc) = self.wobble;
        let dial = u.powf(self.curve) + wd * (TAU * wc * u).sin() * u * (1.0 - u);
        // A new syllable every 70 to 180 ms.
        if self.syllable == 0 {
            self.vowel = VOWELS[(self.rng.next() * VOWELS.len() as f64) as usize % VOWELS.len()];
            self.syllable = ((0.07 + self.rng.next() * 0.11) * sr) as usize;
            self.formant[0].set(self.vowel.0, 6.0, sr);
            self.formant[1].set(self.vowel.1, 8.0, sr);
        }
        self.syllable -= 1;
        let mut capture: f64 = 0.0;
        let mut scrap = 0.0;
        let mut whistle = 0.0;
        for k in 0..self.n_passed {
            let p = &mut self.passed[k];
            let d = (dial - p.pos) / p.width;
            let s = (-d * d).exp() * p.level;
            capture = capture.max(s);
            // The programme: a buzzing voice, or a chord.
            if p.talk {
                p.ph[0] = (p.ph[0] + p.pitch / sr).fract();
                scrap += (p.ph[0] * 2.0 - 1.0) * s;
            } else {
                for (j, r) in [1.0, 1.25, 1.5].iter().enumerate() {
                    p.ph[j] = (p.ph[j] + p.pitch * r / sr).fract();
                    scrap += if p.ph[j] < 0.5 { 0.33 } else { -0.33 } * s;
                }
            }
            // The whistle: heard wider than the programme.
            let ws = (-d * d * 0.15).exp() * p.level;
            let f = (p.beat * (dial - p.pos).abs()).min(7000.0);
            p.w_ph = (p.w_ph + f / sr).fract();
            whistle += (TAU * p.w_ph).sin() * ws;
        }
        let voice = {
            self.formant[0].run(scrap);
            self.formant[1].run(scrap);
            self.tone.run(scrap);
            (self.formant[0].bp + self.formant[1].bp * 0.7) * 0.5 + self.tone.bp * 0.5
        };
        // Off the carrier's centre it breaks up.
        let garble = 1.0 + (self.rng.next() * 2.0 - 1.0) * (1.0 - capture) * 0.8;
        let scrap = (voice * 4.0 * garble).tanh() * 0.16;
        // FM quietens on a carrier; AM keeps its hiss.
        let quiet = 1.0 - capture * (0.9 - 0.5 * self.am);
        let noise = self.rng.next() * 2.0 - 1.0;
        self.bp.run(noise);
        let hiss = (self.bp.bp * 0.7 + noise * 0.3 * (1.0 - self.am)) * self.hiss_level * quiet;
        // Crackle: sparse impulses, a few milliseconds each.
        if self.rng.next() < self.crackle_rate / sr {
            self.crack = 0.1 + self.rng.next() * 0.2;
        }
        self.crack *= 1.0 - 1.0 / (0.002 * sr);
        let crackle = self.crack * (self.rng.next() * 2.0 - 1.0);
        let wh = whistle * 0.05 * self.am;
        let mono = self.band.run(hiss + scrap + wh + crackle);
        // Our station, thin and coming up as the dial nears it.
        let b = ((dial - self.bleed_from) / (1.0 - self.bleed_from)).clamp(0.0, 1.0);
        let b = b * b * self.bleed;
        let bl = self.bleed_lp[0].run(inp.0);
        let br = self.bleed_lp[1].run(inp.1);
        (mono + bl * b, mono + br * b)
    }

    /// Switched off: the click and the hiss falling away.
    pub fn off(&mut self) {
        self.state = TunerState::Off;
        self.t = 0;
        self.len = ((0.15 + self.rng.next() * 0.15) * self.sr) as usize;
        self.click = 0.15 + self.rng.next() * 0.3;
        self.hiss = 1.0;
    }

    /// Straight through (a node built already tuned).
    pub fn on(&mut self) {
        self.state = TunerState::On;
    }

    /// Whether the station is heard at all (off and silent are not).
    pub fn station_audible(&self) -> bool {
        matches!(self.state, TunerState::On | TunerState::Lock)
    }

    /// Applies the tuner to one block of the station's output, in place.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        match self.state {
            TunerState::On => {}
            TunerState::Silent => {
                l.fill(0.0);
                r.fill(0.0);
            }
            TunerState::Sweep => {
                for i in 0..l.len() {
                    let u = (self.t as f64 / self.len as f64).min(1.0);
                    let env = (self.t as f64 / (0.01 * self.sr)).min(1.0);
                    let (yl, yr) = self.sweep_sample(u, (l[i] as f64, r[i] as f64));
                    l[i] = (yl * env) as f32;
                    r[i] = (yr * env) as f32;
                    self.t += 1;
                    if self.t >= self.len {
                        // The lock starts with the next block; the rest of
                        // this one is the last of the static.
                        self.state = TunerState::Lock;
                        self.t = 0;
                        self.len = (self.lock_secs * self.sr) as usize;
                        for j in i + 1..l.len() {
                            let (yl, yr) = self.sweep_sample(1.0, (l[j] as f64, r[j] as f64));
                            l[j] = yl as f32;
                            r[j] = yr as f32;
                        }
                        break;
                    }
                }
            }
            TunerState::Lock => {
                for i in 0..l.len() {
                    let u = (self.t as f64 / self.len as f64).min(1.0);
                    if self.t.is_multiple_of(32) {
                        let f = self.lock_from * (20000.0 / self.lock_from).powf(u);
                        for fl in &mut self.lock {
                            fl.set(f.min(self.sr * 0.45), 0.7, self.sr);
                        }
                    }
                    self.hiss *= 1.0 - 1.0 / (0.08 * self.sr);
                    let noise = self.rng.next() * 2.0 - 1.0;
                    self.bp.run(noise);
                    let st = self.bp.bp * self.hiss_level * self.hiss;
                    let gl = self.lock[0].run(l[i] as f64) * (0.5 + 0.5 * u) + st;
                    let gr = self.lock[1].run(r[i] as f64) * (0.5 + 0.5 * u) + st;
                    l[i] = gl as f32;
                    r[i] = gr as f32;
                    self.t += 1;
                    if self.t >= self.len {
                        self.state = TunerState::On;
                        break;
                    }
                }
            }
            TunerState::Off => {
                for i in 0..l.len() {
                    // A click (the first two milliseconds), then the hiss falling.
                    let click = if self.t < (0.002 * self.sr) as usize {
                        self.click
                    } else {
                        0.0
                    };
                    self.hiss *= 1.0 - 1.0 / (0.05 * self.sr);
                    let noise = self.rng.next() * 2.0 - 1.0;
                    self.bp.run(noise);
                    let y = self.bp.bp * 0.15 * self.hiss + click * noise;
                    l[i] = y as f32;
                    r[i] = y as f32;
                    self.t += 1;
                    if self.t >= self.len {
                        self.state = TunerState::Silent;
                        for j in i + 1..l.len() {
                            l[j] = 0.0;
                            r[j] = 0.0;
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

    #[test]
    fn no_two_turns_of_the_dial_sound_alike() {
        let sr = 48000.0;
        let mut t = Tuner::new(11, sr);
        let mut turns = Vec::new();
        for _ in 0..4 {
            t.tune();
            let mut out = Vec::new();
            let (mut l, mut r) = (vec![0.0f32; 128], vec![0.0f32; 128]);
            while !t.station_audible() {
                l.fill(0.0);
                r.fill(0.0);
                t.process(&mut l, &mut r);
                out.extend_from_slice(&l);
            }
            assert!(out.iter().all(|x| x.is_finite() && x.abs() < 1.0));
            turns.push(out);
        }
        for i in 0..turns.len() {
            for j in i + 1..turns.len() {
                assert_ne!(turns[i], turns[j], "turns {i} and {j} are the same");
            }
        }
        let lens: Vec<usize> = turns.iter().map(Vec::len).collect();
        assert!(lens.iter().any(|n| *n != lens[0]), "lengths vary: {lens:?}");
    }

    #[test]
    fn the_tuner_sweeps_locks_and_switches_off() {
        let sr = 48000.0;
        let mut t = Tuner::new(3, sr);
        let mut l = vec![0.0f32; 128];
        let mut r = vec![0.0f32; 128];
        t.process(&mut l, &mut r);
        assert!(l.iter().all(|x| *x == 0.0), "silent before a tune");
        t.tune();
        let mut static_rms = 0.0;
        let mut blocks = 0;
        while !t.station_audible() {
            l.fill(0.5);
            r.fill(0.5);
            t.process(&mut l, &mut r);
            static_rms += l.iter().map(|x| (*x as f64).powi(2)).sum::<f64>();
            blocks += 1;
            assert!(blocks < 1000, "the sweep ends");
        }
        assert!(static_rms > 0.0, "static was heard");
        assert!(blocks as f64 * 128.0 / sr > 0.5, "half a second at least");
        for _ in 0..200 {
            l.fill(0.5);
            r.fill(0.5);
            t.process(&mut l, &mut r);
        }
        assert!((l[0] - 0.5).abs() < 1e-6, "straight through once locked");
        t.off();
        for _ in 0..200 {
            l.fill(0.5);
            r.fill(0.5);
            t.process(&mut l, &mut r);
        }
        assert!(l.iter().all(|x| *x == 0.0), "silent after off");
    }
}
