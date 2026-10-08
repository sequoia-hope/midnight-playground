//! The car radio's stations (docs/vision/radio.md 7, DECISIONS D1151): the
//! radio node (`mp_music`'s station player in an AudioWorklet) playing into
//! the music bus, so the bus's compressor, gate, duck, mood filter and
//! volume apply to a station exactly as to the old music. While a station
//! plays, the old `Music` (the playlist) is stopped; tuning off brings it
//! back. Off (the default of [`GameAudio::new`], so every parity run and
//! call-log test is untouched), nothing here creates a node.
//!
//! One radio node lives as long as the graph: tuning moves its params. The
//! client reads the clock ([`GameAudio::set_station`] takes the wall time);
//! `GameAudio` never does.

use super::{GameAudio, clamp};
use crate::wa::{GainNode, Pending, RadioNode};
use mp_music::radio::{STATIONS, wall_parts};

pub(crate) struct StationNode {
    node: RadioNode,
    /// The station's output (the DJ ducks it while talking).
    pub out: GainNode,
}

/// Where the radio stands (as the exhaust's `ExState`).
#[derive(Default)]
pub(crate) enum StState {
    /// Not asked for yet.
    #[default]
    Idle,
    /// Loading (the web's worklet module and wasm).
    Preparing(Pending<bool>),
    /// Ready, with the node built the first time a station is tuned.
    Ready(Option<StationNode>),
    /// This platform cannot run it: the old music plays.
    Unavailable,
}

impl GameAudio {
    /// Tunes the radio to `STATIONS[station]` as of the wall time `wall`
    /// (Unix seconds, the client's clock), or off (`None`). Every tune
    /// re-syncs the player, even to the station already playing. While the
    /// node is loading the request waits and is applied once it settles
    /// (from [`GameAudio::poll`] or [`GameAudio::update`]). Where the
    /// platform cannot run the node the old music keeps playing.
    pub fn set_station(&mut self, station: Option<usize>, wall: f64) {
        let station = station.filter(|&i| i < STATIONS.len());
        let was = self.station_want;
        self.station_want = station;
        self.station_wall = wall;
        self.station_pending = true;
        if !self.ready() {
            return;
        }
        if station != was {
            self.dj_cut();
        }
        if station.is_some() {
            self.station_tune += 1.0;
            if was.is_none() && !matches!(self.station, StState::Unavailable) {
                // The playlist is quiet while a station plays.
                if let Some(m) = self.music.as_mut() {
                    m.stop(&mut self.timers);
                }
            }
        } else if was.is_some() {
            self.music_back();
        }
        self.station_poll();
    }

    /// The station's `energy` (0..1, the game's intensity; radio.md 7).
    pub fn set_energy(&mut self, e: f64) {
        self.station_energy = clamp(e, 0.0, 1.0);
        if let StState::Ready(Some(n)) = &self.station {
            let t = self.now();
            n.node
                .energy
                .set_target_at_time(self.station_energy, t, 0.5);
        }
    }

    /// The station tuned or requested (`None`: off, or the platform cannot
    /// run the radio and the old music plays).
    pub fn station(&self) -> Option<usize> {
        match self.station {
            StState::Unavailable => None,
            _ => self.station_want,
        }
    }

    /// How many radio nodes this GameAudio has made (tests: one at most).
    pub fn radio_nodes(&self) -> u32 {
        self.station_made
    }

    /// Whether this platform cannot run the radio (no AudioWorklet): the
    /// playlist plays whatever station is asked for.
    pub fn radio_unavailable(&self) -> bool {
        matches!(self.station, StState::Unavailable)
    }

    /// Whether the radio node is built and the last request applied.
    pub fn radio_ready(&self) -> bool {
        matches!(self.station, StState::Ready(Some(_))) && !self.station_pending
    }

    /// The radio's output gain, once built (the DJ ducks it).
    pub(super) fn station_out(&self) -> Option<&GainNode> {
        match &self.station {
            StState::Ready(Some(n)) => Some(&n.out),
            _ => None,
        }
    }

    /// Drives the radio: starts the loading on the first request, builds
    /// the node once loaded, applies the request waiting. Cheap when there
    /// is nothing to do; called from `poll`, `update` and `init`.
    pub(super) fn station_poll(&mut self) {
        if !self.station_pending || !self.ready() {
            return;
        }
        if matches!(self.station, StState::Idle) {
            if self.station_want.is_none() {
                // Off before anything was tuned: nothing to load.
                self.station_pending = false;
                return;
            }
            let ctx = self.ctx.clone().expect("a context");
            self.station = StState::Preparing(ctx.prepare_radio());
        }
        if let StState::Preparing(p) = &self.station {
            match p.result() {
                None => return,
                Some(Ok(true)) => self.station = StState::Ready(None),
                Some(_) => {
                    // No AudioWorklet here: the old music plays on.
                    self.station = StState::Unavailable;
                    self.station_pending = false;
                    self.music_back();
                    return;
                }
            }
        }
        if matches!(self.station, StState::Unavailable) {
            self.station_pending = false;
            return;
        }
        if let StState::Ready(None) = self.station {
            if self.station_want.is_none() {
                self.station_pending = false;
                return;
            }
            let n = self.build_station();
            self.station = StState::Ready(Some(n));
        }
        let t = self.now();
        let (day, sec) = wall_parts(self.station_wall);
        let (want, tune) = (self.station_want, self.station_tune);
        let StState::Ready(Some(n)) = &self.station else {
            unreachable!()
        };
        match want {
            Some(i) => {
                n.node.station.set_value_at_time(i as f64, t);
                n.node.wall_day.set_value_at_time(day, t);
                n.node.wall_sec.set_value_at_time(sec, t);
                n.node.tune.set_value_at_time(tune, t);
            }
            None => {
                n.node.station.set_value_at_time(-1.0, t);
            }
        }
        self.station_pending = false;
    }

    fn build_station(&mut self) -> StationNode {
        let ctx = self.ctx.clone().expect("a context");
        let g = self.graph();
        let node = ctx.create_radio();
        self.station_made += 1;
        // Its output, at unity: the DJ ducks it while talking.
        let out = ctx.create_gain();
        let _ = node.connect(&out);
        let _ = out.connect(&g.music_in);
        // The energy asked for before the node existed.
        if self.station_energy != 1.0 {
            node.energy
                .set_value_at_time(self.station_energy, ctx.current_time());
        }
        StationNode { node, out }
    }

    /// The old music comes back (tuning off, or the radio unavailable): as
    /// `set_music(true)` and `play_track` start it.
    fn music_back(&mut self) {
        if !self.music_on {
            return;
        }
        let track = self.track.clone();
        let Some(m) = self.music.as_mut() else {
            return;
        };
        m.start(&mut self.timers);
        if let Some(t) = track {
            m.play(&t, 0, true, &mut self.timers);
        }
    }
}
