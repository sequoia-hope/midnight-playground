//! STUB from WP 8.3 (the pursuit HUD): `PursuitView.say`'s rate limit and
//! the lines said each frame, only as much as the HUD and the test bridge
//! need. WP 8.4 (pursuit audio) owns this file and replaces it with the
//! real one (the event-to-line table, heading and zone names, the voice);
//! the names here are the agreed interface: `Radio::said`, `frame`, `say`.

use mr_audio::radio::lines::Line;

/// Seconds between chatter lines (`RADIO_GAP` in PursuitView.js).
pub const SAY_GAP: f64 = 4.0;

/// Dispatch: what was said this frame, and the gap until the next line.
#[derive(Clone, Debug, Default)]
pub struct Radio {
    /// The lines said this frame (past the rate limit), in order.
    pub said: Vec<Line>,
    /// `radioT`.
    pub t: f64,
}

impl Radio {
    /// A new frame: last frame's lines are gone, the gap runs down.
    pub fn frame(&mut self, dt: f64) {
        self.said.clear();
        self.t -= dt;
    }

    /// `say(line, force)`: a line, unless one was said in the last
    /// [`SAY_GAP`] seconds and this one is not forced.
    pub fn say(&mut self, line: Line, force: bool) {
        if self.t > 0.0 && !force {
            return;
        }
        self.t = SAY_GAP;
        self.said.push(line);
    }
}
