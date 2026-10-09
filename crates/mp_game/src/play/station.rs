//! The radio in the client (radio.md 7, DECISIONS D1151): which station the
//! setting means, what is playing on it (the same schedule the node runs,
//! computed from the wall clock for the pause screen and the HUD), the
//! race's energy, and the DJ's director: when a DJ says something, and what
//! (only in a race: the menus are music only).
//!
//! Presentation, not simulation: its random choices use their own
//! generator, never the simulation's streams, and each player hears their
//! own radio.

use mp_audio::game::GameAudio;
use mp_music::radio::{STATIONS, Station, cue, level_station, station};

/// Seconds between two DJ breaks at the least.
const BREAK_GAP: f64 = 150.0;
/// A break at a song change, this often.
const BREAK_CHANCE: f64 = 0.35;
/// An ident shortly after tuning in, this often.
const IDENT_CHANCE: f64 = 0.5;
/// Lines not said again until this many others have been.
const HISTORY: usize = 8;

/// The station a setting means for a level: `None` is the playlist.
pub fn resolve(setting: &str, level: &str) -> Option<usize> {
    let key = match setting {
        "playlist" => return None,
        "auto" => level_station(level),
        k => k,
    };
    station(key).map(|st| {
        STATIONS
            .iter()
            .position(|s| s.key == st.key)
            .expect("in STATIONS")
    })
}

/// The station `dir` steps along the dial from `setting` (+1 the next, -1
/// the one before), round the stations; the radio stays on (D1160). From
/// the playlist (the radio off) it is the first station either way.
pub fn step_setting(setting: &str, level: &str, dir: i32) -> String {
    let n = STATIONS.len() as i32;
    match resolve(setting, level) {
        None => STATIONS[0].key.to_owned(),
        Some(i) => STATIONS[(i as i32 + dir).rem_euclid(n) as usize]
            .key
            .to_owned(),
    }
}

/// What the radio plays with it switched on: `remembered` (the station last
/// chosen, or `auto`), never the playlist.
pub fn radio_on_setting(remembered: &str) -> String {
    if remembered == "playlist" || !crate::ui::store::station_ok(remembered) {
        "auto".to_owned()
    } else {
        remembered.to_owned()
    }
}

/// `"The Tide 88.1"`.
pub fn label(st: &Station) -> String {
    format!("{} {}", st.name, st.freq)
}

/// What the race is doing, for the energy and the DJ's silences.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Scene {
    /// A race is on (not the menus).
    pub racing: bool,
    pub countdown: bool,
    pub results: bool,
    pub pursuit: bool,
    /// The player's speed, m/s.
    pub speed: f64,
    /// The police radio spoke this frame.
    pub police: bool,
}

/// The client's side of the radio.
#[derive(Debug)]
pub struct StationState {
    /// The station tuned, if any.
    pub station: Option<usize>,
    /// The second the cue was last computed at, and its slot.
    cue_sec: i64,
    slot: Option<(usize, u64, usize)>,
    /// The now-playing line (`♪ station · song · style`).
    pub text: String,
    /// A line for the HUD to flash (the station or a new song).
    pub flash: Option<String>,
    pub energy: f64,
    last_frame: Option<f64>,
    // The DJ.
    history: Vec<String>,
    last_break: f64,
    due: Option<f64>,
    police_at: f64,
    prefetched: Option<&'static str>,
    rng: u64,
}

impl Default for StationState {
    fn default() -> Self {
        StationState::new()
    }
}

impl StationState {
    pub fn new() -> StationState {
        StationState {
            station: None,
            cue_sec: i64::MIN,
            slot: None,
            text: String::new(),
            flash: None,
            energy: 0.7,
            last_frame: None,
            history: Vec::new(),
            last_break: f64::MIN,
            due: None,
            police_at: f64::MIN,
            prefetched: None,
            rng: 0x9E37_79B9_7F4A_7C15,
        }
    }

    /// A tune (or the playlist: `None`) at wall time `wall`.
    pub fn tuned(&mut self, station: Option<usize>, wall: f64) {
        if station != self.station {
            self.station = station;
            self.slot = None;
            self.cue_sec = i64::MIN;
            self.text.clear();
            self.rng ^= (wall * 1000.0) as u64;
            match station {
                Some(i) => {
                    self.flash = Some(label(&STATIONS[i]));
                    // An ident a couple of seconds in, sometimes.
                    self.due = (self.chance(IDENT_CHANCE)).then_some(wall + 2.0);
                    self.refresh(wall);
                    self.flash = Some(self.text.clone());
                }
                None => {
                    self.flash = Some("Playlist".into());
                    self.due = None;
                }
            }
        }
    }

    /// A draw in 0..1 (an xorshift of its own; never the simulation's).
    fn next(&mut self) -> f64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        (x >> 11) as f64 / (1u64 << 53) as f64
    }

    fn chance(&mut self, p: f64) -> bool {
        self.next() < p
    }

    /// Recomputes the cue at most once a second; true when the song changed.
    fn refresh(&mut self, wall: f64) -> bool {
        let Some(i) = self.station else { return false };
        let sec = wall.floor() as i64;
        if sec == self.cue_sec {
            return false;
        }
        self.cue_sec = sec;
        let st = &STATIONS[i];
        let c = cue(st, wall);
        let slot = Some((i, c.block, c.slot));
        let changed = self.slot.is_some() && slot != self.slot;
        self.slot = slot;
        let title = c.track.title.clone().unwrap_or_else(|| c.track.id.clone());
        let style = c.track.style.clone().unwrap_or_default();
        self.text = format!("{} · {title} · {style}", label(st));
        changed
    }

    /// Every frame: the energy, the now-playing line, the DJ.
    pub fn frame(&mut self, audio: &mut GameAudio, wall: f64, scene: Scene) {
        let dt = self
            .last_frame
            .map_or(0.016, |t| (wall - t).clamp(0.0, 0.5));
        self.last_frame = Some(wall);
        if scene.police {
            self.police_at = wall;
        }
        // Energy: the mix follows the race (sound.md 3.4).
        let target = if !scene.racing {
            0.7
        } else if scene.results {
            0.35
        } else if scene.countdown {
            0.5
        } else if scene.pursuit {
            0.95
        } else {
            0.6 + 0.3 * (scene.speed / 45.0).clamp(0.0, 1.0)
        };
        let e = self.energy + (target - self.energy) * dt.min(1.0);
        if (e - self.energy).abs() > 0.02 || (target - self.energy).abs() < 0.02 && e != self.energy
        {
            self.energy = e;
            audio.set_energy(e);
        }
        let Some(i) = self.station else { return };
        if audio.station() != Some(i) {
            // Not tuned (yet, or no AudioWorklet): nothing to say.
            return;
        }
        if self.refresh(wall) {
            self.flash = Some(self.text.clone());
            if self.chance(BREAK_CHANCE) {
                self.due = Some(wall);
            }
        }
        let st = &STATIONS[i];
        let Some(dj) = st.dj else { return };
        self.prefetch(audio, dj);
        if !scene.racing {
            // The menus are music only: a break due now is dropped, not
            // kept for the race (the owner, 2026-10-09).
            self.due = None;
            return;
        }
        if let Some(due) = self.due
            && wall >= due
        {
            let quiet = !scene.countdown && wall - self.police_at > 4.0;
            if !quiet {
                return; // keep it pending
            }
            self.due = None;
            if wall - self.last_break >= BREAK_GAP {
                self.say(audio, dj, wall);
            }
        }
    }

    /// The DJ's clips, fetched once the index is known.
    fn prefetch(&mut self, audio: &mut GameAudio, dj: &'static str) {
        if self.prefetched == Some(dj) {
            return;
        }
        let Some(voice) = audio.dj() else { return };
        let Some(Ok(index)) = voice.index().result() else {
            return;
        };
        let ids: Vec<String> = index
            .keys()
            .filter(|id| topic_of(id, dj).is_some())
            .cloned()
            .collect();
        let _ = voice.prefetch(&ids);
        self.prefetched = Some(dj);
    }

    /// A break: a song line (70 %) or an ident, not one said lately.
    fn say(&mut self, audio: &mut GameAudio, dj: &'static str, wall: f64) {
        let Some(voice) = audio.dj() else { return };
        let Some(Ok(index)) = voice.index().result() else {
            return;
        };
        let topic = if self.chance(0.7) { "music" } else { "ident" };
        let mut ids: Vec<(&String, u32)> = index
            .iter()
            .filter(|(id, _)| topic_of(id, dj) == Some(topic))
            .filter(|(id, _)| !self.history.contains(id))
            .map(|(id, n)| (id, *n))
            .collect();
        ids.sort();
        if ids.is_empty() {
            return;
        }
        let k = (self.next() * ids.len() as f64) as usize;
        let (id, takes) = ids[k.min(ids.len() - 1)];
        let take = 1 + (self.next() * takes.max(1) as f64) as u32;
        let id = id.clone();
        self.history.push(id.clone());
        if self.history.len() > HISTORY {
            self.history.remove(0);
        }
        self.last_break = wall;
        audio.dj_say(&id, take.min(takes.max(1)));
    }
}

/// The topic of a clip id `<dj>-<topic>-<n>` when it is `dj`'s.
fn topic_of<'a>(id: &'a str, dj: &str) -> Option<&'a str> {
    let rest = id.strip_prefix(dj)?.strip_prefix('-')?;
    let (topic, n) = rest.rsplit_once('-')?;
    n.parse::<u32>().ok()?;
    Some(topic)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_resolve_and_step_round_the_dial() {
        assert_eq!(resolve("playlist", "coast"), None);
        assert_eq!(resolve("auto", "coast"), Some(0));
        assert_eq!(resolve("auto", "sierra"), Some(1));
        assert_eq!(resolve("pacifico", "coast"), Some(2));
        assert_eq!(resolve("nope", "coast"), None);
        assert_eq!(step_setting("playlist", "coast", 1), "tide");
        assert_eq!(step_setting("tide", "coast", 1), "ridgeline");
        assert_eq!(step_setting("auto", "sierra", 1), "pacifico");
        // Round the stations, never off.
        assert_eq!(step_setting("pacifico", "coast", 1), "tide");
        assert_eq!(step_setting("tide", "coast", -1), "pacifico");
        assert_eq!(radio_on_setting("playlist"), "auto");
        assert_eq!(radio_on_setting("ridgeline"), "ridgeline");
        assert_eq!(radio_on_setting("kzzz"), "auto");
    }

    #[test]
    fn clip_ids_parse() {
        assert_eq!(topic_of("marisol-ident-3", "marisol"), Some("ident"));
        assert_eq!(topic_of("kit-music-10", "kit"), Some("music"));
        assert_eq!(topic_of("kit-music-10", "marisol"), None);
        assert_eq!(topic_of("marisol-weather", "marisol"), None);
    }

    #[test]
    fn a_tune_flashes_the_station_and_refreshes_once_a_second() {
        let mut s = StationState::new();
        let wall = mp_music::radio::EPOCH + 1000.0;
        s.tuned(Some(0), wall);
        assert!(s.flash.take().unwrap().starts_with("The Tide 88.1 · "));
        assert!(s.text.contains(" · "));
        assert!(!s.refresh(wall + 0.5), "same second, no recompute");
        s.tuned(None, wall);
        assert_eq!(s.flash.take().as_deref(), Some("Playlist"));
        assert_eq!(s.station, None);
    }
}
