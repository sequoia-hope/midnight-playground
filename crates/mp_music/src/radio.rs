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

/// The sound of the dial (radio.md 2.1), applied in place to the
/// station's output: a sweep of band-passed static whose centre glides,
/// two heterodyne whistles crossing, then the lock, the station opening
/// up from a few hundred hertz to full width; off is a click and the hiss
/// falling away. Seeded, so never quite the same twice.
pub struct Tuner {
    sr: f64,
    rng: Rng,
    state: TunerState,
    t: usize,
    len: usize,
    f0: f64,
    f1: f64,
    bp: Svf,
    lock: [Svf; 2],
    w: [f64; 2],
    w_ph: [f64; 2],
    hiss: f64,
}

impl Tuner {
    pub fn new(seed: u32, sr: f64) -> Tuner {
        let mut t = Tuner {
            sr,
            rng: Rng::new(seed),
            state: TunerState::Silent,
            t: 0,
            len: 1,
            f0: 800.0,
            f1: 2500.0,
            bp: Svf::new(),
            lock: [Svf::new(), Svf::new()],
            w: [0.0; 2],
            w_ph: [0.0; 2],
            hiss: 0.0,
        };
        t.bp.set(1200.0, 2.0, sr);
        for f in &mut t.lock {
            f.set(20000.0, 0.7, sr);
        }
        t
    }

    /// The dial turned: the sweep starts (0.55 to 0.85 s), then the lock.
    pub fn tune(&mut self) {
        self.state = TunerState::Sweep;
        self.t = 0;
        self.len = ((0.55 + self.rng.next() * 0.3) * self.sr) as usize;
        let up = self.rng.next() < 0.5;
        let (a, b) = (
            500.0 + self.rng.next() * 700.0,
            2200.0 + self.rng.next() * 2000.0,
        );
        (self.f0, self.f1) = if up { (a, b) } else { (b, a) };
        self.w = [
            300.0 + self.rng.next() * 400.0,
            2800.0 + self.rng.next() * 1500.0,
        ];
        self.hiss = 1.0;
    }

    /// Switched off: the click and the hiss falling away.
    pub fn off(&mut self) {
        self.state = TunerState::Off;
        self.t = 0;
        self.len = (0.22 * self.sr) as usize;
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
                    if self.t.is_multiple_of(32) {
                        let f = self.f0 * (self.f1 / self.f0).powf(u);
                        self.bp.set(f, 1.6 + u * 2.0, self.sr);
                    }
                    let noise = self.rng.next() * 2.0 - 1.0;
                    self.bp.run(noise);
                    // The whistles: one rising, one falling, crossing in the middle.
                    let w0 = self.w[0] * (self.w[1] / self.w[0]).powf(u);
                    let w1 = self.w[1] * (self.w[0] / self.w[1]).powf(u);
                    self.w_ph[0] += w0 / self.sr;
                    self.w_ph[1] += w1 / self.sr;
                    let wh = ((TAU * self.w_ph[0]).sin() + (TAU * self.w_ph[1]).sin() * 0.6)
                        * 0.035
                        * (1.0 - (u * 2.0 - 1.0).abs()).max(0.0);
                    let env = (self.t as f64 / (0.01 * self.sr)).min(1.0);
                    let y = (self.bp.bp * 0.22 + wh) * env;
                    l[i] = y as f32;
                    r[i] = y as f32;
                    self.t += 1;
                    if self.t >= self.len {
                        // The lock starts with the next block; the rest of
                        // this one is the last of the static.
                        self.state = TunerState::Lock;
                        self.t = 0;
                        self.len = (0.3 * self.sr) as usize;
                        for j in i + 1..l.len() {
                            let noise = self.rng.next() * 2.0 - 1.0;
                            self.bp.run(noise);
                            l[j] = (self.bp.bp * 0.22) as f32;
                            r[j] = l[j];
                        }
                        break;
                    }
                }
            }
            TunerState::Lock => {
                for i in 0..l.len() {
                    let u = (self.t as f64 / self.len as f64).min(1.0);
                    if self.t.is_multiple_of(32) {
                        let f = 400.0 * (20000.0 / 400.0_f64).powf(u);
                        for fl in &mut self.lock {
                            fl.set(f.min(self.sr * 0.45), 0.7, self.sr);
                        }
                    }
                    self.hiss *= 1.0 - 1.0 / (0.08 * self.sr);
                    let noise = self.rng.next() * 2.0 - 1.0;
                    self.bp.run(noise);
                    let st = self.bp.bp * 0.22 * self.hiss;
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
                        0.3
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
