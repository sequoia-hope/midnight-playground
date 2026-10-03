//! An AudioParam's automation timeline, evaluated the way the recording fake
//! does (`FakeParam` in `tools/parity/lib/webaudio-fake.mjs`; DECISIONS D42):
//! Web Audio 1.0 rules, where setting `value` is `setValueAtTime(value,
//! currentTime)`, a ramp runs from the previous event's time and value, and
//! a target curve runs from the value at its start until the next event. A
//! ramp straight after a target curve starts from the target's start value
//! (the spec's literal reading; the game never reads a param in that state).

use mr_math::kernel::{exp, pow};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EventKind {
    Set,
    Lin,
    Exp,
    Target { tc: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Event {
    pub kind: EventKind,
    pub v: f64,
    pub t: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Timeline {
    pub intrinsic: f64,
    /// Sorted by time; ties in insertion order.
    pub events: Vec<Event>,
}

impl Timeline {
    pub fn new(value: f64) -> Self {
        Timeline {
            intrinsic: value,
            events: Vec::new(),
        }
    }

    pub fn insert(&mut self, e: Event) {
        let mut i = self.events.len();
        while i > 0 && self.events[i - 1].t > e.t {
            i -= 1;
        }
        self.events.insert(i, e);
    }

    /// `param.value = v` at time `now`.
    pub fn set_value(&mut self, v: f64, now: f64) {
        self.intrinsic = v;
        self.insert(Event {
            kind: EventKind::Set,
            v,
            t: now,
        });
    }

    /// `cancelScheduledValues(t)`: drops every event at or after `t`.
    pub fn cancel(&mut self, t: f64) {
        self.events.retain(|e| e.t < t);
    }

    /// The automation value at time `tt` (before the float32 rounding
    /// Chrome's params store).
    pub fn at(&self, tt: f64) -> f64 {
        let ev = &self.events;
        let mut v = self.intrinsic;
        let mut t0 = 0.0;
        let mut i = 0;
        while i < ev.len() {
            let e = ev[i];
            if e.t > tt {
                return match e.kind {
                    EventKind::Lin => v + (e.v - v) * (tt - t0) / (e.t - t0),
                    EventKind::Exp => {
                        if v * e.v > 0.0 {
                            v * pow(e.v / v, (tt - t0) / (e.t - t0))
                        } else {
                            v
                        }
                    }
                    _ => v,
                };
            }
            if let EventKind::Target { tc } = e.kind {
                let next = ev.get(i + 1);
                let end = match next {
                    Some(n) if n.t <= tt => n.t,
                    _ => tt,
                };
                let start = v;
                v = e.v + (start - e.v) * exp(-(end - e.t) / tc);
                if end == tt {
                    return v;
                }
                // Ramps after a target start from the target's start (see above).
                let next = next.expect("a next event before tt");
                if matches!(next.kind, EventKind::Lin | EventKind::Exp) {
                    v = start;
                }
                t0 = e.t;
                i += 1;
                continue;
            }
            v = e.v;
            t0 = e.t;
            i += 1;
        }
        v
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    use super::*;

    #[test]
    fn ramps_and_targets() {
        let mut p = Timeline::new(1.0);
        assert_eq!(p.at(5.0), 1.0);
        p.insert(Event {
            kind: EventKind::Set,
            v: 0.0,
            t: 1.0,
        });
        p.insert(Event {
            kind: EventKind::Lin,
            v: 1.0,
            t: 2.0,
        });
        assert_eq!(p.at(0.5), 1.0);
        assert_eq!(p.at(1.5), 0.5);
        assert_eq!(p.at(3.0), 1.0);
        p.insert(Event {
            kind: EventKind::Target { tc: 0.5 },
            v: 0.0,
            t: 3.0,
        });
        assert_eq!(p.at(3.5), exp(-1.0));
        p.cancel(3.0);
        assert_eq!(p.at(3.5), 1.0);
        p.insert(Event {
            kind: EventKind::Exp,
            v: 4.0,
            t: 4.0,
        });
        assert_eq!(p.at(3.0), 1.0 * pow(4.0, 0.5));
    }
}
