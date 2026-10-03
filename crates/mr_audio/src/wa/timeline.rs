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

    /// Forget the past: the events before the last one at or before `now`
    /// become one `Set` at that event's time with the value they reach
    /// there. Every value from then on is unchanged (that event still starts
    /// from the same time and value), and the list stays short however long
    /// a param is steered.
    pub fn prune(&mut self, now: f64) {
        let Some(k) = self.events.iter().rposition(|e| e.t <= now) else {
            return;
        };
        if k == 0 {
            return;
        }
        let tk = self.events[k].t;
        let before = Timeline {
            intrinsic: self.intrinsic,
            events: self.events[..k].to_vec(),
        }
        .at(tk);
        self.events.splice(
            ..k,
            [Event {
                kind: EventKind::Set,
                v: before,
                t: tk,
            }],
        );
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

    #[test]
    fn pruning_keeps_every_value_from_now_on() {
        let mut p = Timeline::new(440.0);
        let ev = |kind, v, t| Event { kind, v, t };
        p.insert(ev(EventKind::Set, 12.0, 0.0));
        for k in 0..20 {
            let t = k as f64 * 0.008;
            p.insert(ev(EventKind::Target { tc: 0.03 }, 50.0 + k as f64, t));
        }
        p.insert(ev(EventKind::Lin, 80.0, 0.3));
        p.insert(ev(EventKind::Target { tc: 0.05 }, 20.0, 0.4));
        let before: Vec<f64> = (0..60).map(|i| p.at(0.1 + i as f64 * 0.01)).collect();
        p.prune(0.1);
        assert_eq!(p.events.len(), 11, "the 11 events from 0.096 on");
        let after: Vec<f64> = (0..60).map(|i| p.at(0.1 + i as f64 * 0.01)).collect();
        for (a, b) in before.iter().zip(&after) {
            assert!((a - b).abs() <= 1e-12 * a.abs().max(1.0), "{a} vs {b}");
        }
    }
}
