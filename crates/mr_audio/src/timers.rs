//! `setTimeout` for the audio code: the gate tails, the music's 25 ms
//! scheduler, its track-change and retire callbacks, and the radio's 700 ms
//! wait (DECISIONS D250).
//!
//! The JS sets real timers; the audio reference runs them on a virtual clock
//! (`VirtualTimers` in `tools/parity/lib/webaudio-fake.mjs`): a timer set at
//! time `now` for `ms` is due at `now + max(0, ms) / 1000`, and due timers
//! run in order of due time, ties in the order they were set. This is that
//! queue. The callbacks are [`Task`]s that [`crate::game::GameAudio`] runs;
//! its driver says when time has passed (the call-log playback: game time;
//! the client: the audio clock).

use crate::music::{SongKill, TrackInfo};

pub type TimerId = u64;

/// What a timer does when it runs.
pub enum Task {
    /// `_gateLater`'s `_gateTick()`.
    GateTick,
    /// The music's `_tick()`.
    MusicTick,
    /// `_begin`'s `fire`: tell the game the song `serial` started.
    MusicOnTrack { serial: u64, info: TrackInfo },
    /// `_retire`'s `kill`.
    MusicKill(SongKill),
    /// `radioLine`'s wait for line `seq`'s clips.
    RadioWait(u32),
}

struct Timer {
    id: TimerId,
    due: f64,
    task: Task,
}

/// The pending timers.
#[derive(Default)]
pub struct Timers {
    queue: Vec<Timer>,
    seq: TimerId,
}

impl Timers {
    /// `setTimeout(task, ms)` at time `now`; returns the id (never 0).
    pub fn set_timeout(&mut self, now: f64, ms: f64, task: Task) -> TimerId {
        self.seq += 1;
        // `Math.max(0, Number(ms) || 0) / 1000`
        let ms = if ms.is_nan() { 0.0 } else { ms };
        let due = now + mr_math::js::max(0.0, ms) / 1000.0;
        self.queue.push(Timer {
            id: self.seq,
            due,
            task,
        });
        self.seq
    }

    /// `clearTimeout(id)`.
    pub fn clear(&mut self, id: Option<TimerId>) {
        if let Some(id) = id {
            self.queue.retain(|x| x.id != id);
        }
    }

    /// The due time of the next timer to run.
    pub fn next_due(&self) -> Option<f64> {
        self.best().map(|i| self.queue[i].due)
    }

    fn best(&self) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (i, x) in self.queue.iter().enumerate() {
            // Ties go to the earlier one set, which is earlier in the queue.
            if best.is_none_or(|b| x.due < self.queue[b].due) {
                best = Some(i);
            }
        }
        best
    }

    /// Take the next timer due at or before `t`: `(due, task)`.
    pub fn pop_due(&mut self, t: f64) -> Option<(f64, Task)> {
        let i = self.best()?;
        if self.queue[i].due > t {
            return None;
        }
        let x = self.queue.remove(i);
        Some((x.due, x.task))
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_order_then_set_order() {
        let mut t = Timers::default();
        t.set_timeout(0.0, 25.0, Task::MusicTick);
        t.set_timeout(0.0, 10.0, Task::RadioWait(1));
        t.set_timeout(0.0, 25.0, Task::RadioWait(2));
        let id = t.set_timeout(0.0, 5.0, Task::GateTick);
        t.clear(Some(id));
        assert!(matches!(t.pop_due(1.0), Some((_, Task::RadioWait(1)))));
        assert!(matches!(t.pop_due(1.0), Some((_, Task::MusicTick))));
        assert!(t.pop_due(0.02).is_none());
        assert!(matches!(t.pop_due(1.0), Some((_, Task::RadioWait(2)))));
        assert!(t.is_empty());
        // A negative or NaN delay is due at once.
        t.set_timeout(2.0, -5.0, Task::GateTick);
        t.set_timeout(2.0, f64::NAN, Task::GateTick);
        assert_eq!(t.next_due(), Some(2.0));
    }
}
