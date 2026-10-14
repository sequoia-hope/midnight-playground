//! Events at places (docs/vision/peninsula.md, stage 5): an open level's
//! races, time trials and deliveries, started out of a cruise by stopping
//! on a start's marker and holding gas and brake, as Burnout Paradise does.
//!
//! The level lists them ([`mp_track::EventSpot`]); the rules here run in
//! the tick, so a recording replays them and the client only shows them.
//! A race or a trial counts progress as a circuit does (`prog`, carried by
//! the distance moved, [`crate::race::RaceState::progged`]) to a finish
//! the event's length past its start, so the rivals finish, park and are
//! placed by the code that does it on a circuit. A delivery ends at its
//! drop, reached by any road, or when its time runs out. At the results
//! the cruise comes back as it was.

use mp_math::{js, kernel};
use mp_track::{EventKind, EventSpot, Level, Track};

use crate::input::Input;
use crate::park::Park;
use crate::race::{
    LevelRuntime, PlayerRules, RaceStateKind, ResultRow, SimEvent, SimState, grid, level_rivals,
    results,
};
use crate::vehicle::Vehicle;

/// Seconds of gas and brake together, stopped on a start, that begin it.
pub const HOLD: f64 = 1.0;
/// Below this speed (m/s) the car counts as stopped on a start.
pub const STOPPED: f64 = 2.0;
/// A start's marker: metres behind and ahead of its `s`.
pub const SPOT_BACK: f64 = 20.0;
pub const SPOT_AHEAD: f64 = 12.0;
/// A delivery is made within this many metres of its drop.
pub const DROP_RADIUS: f64 = 15.0;
/// Traffic within this many metres of a start goes when the grid forms.
const CLEAR: f64 = 220.0;

/// The event being run, and the cruise it came out of.
#[derive(Clone, Debug, PartialEq)]
pub struct EventRun {
    /// Into the level's events.
    pub index: usize,
    pub kind: EventKind,
    /// The cruise's rules (score, multiplier, distance) as they were.
    pub cruise: PlayerRules,
    /// The cruise's clock when the event began.
    pub cruise_time: f64,
    /// A delivery's end has been decided, and whether it was made.
    pub decided: bool,
    pub delivered: bool,
}

/// How an event went, for the results.
#[derive(Clone, Debug, PartialEq)]
pub struct EventResult {
    pub index: usize,
    pub kind: EventKind,
    /// The player's time (a delivery out of time: the time it had).
    pub time: f64,
    /// The player's place (1 in a trial or a delivery).
    pub place: usize,
    /// A race's results.
    pub rows: Vec<ResultRow>,
    /// A race won, a trial's time beaten, a delivery made in time.
    pub made_it: bool,
}

/// Metres from `a` to `b` the road's way (round a loop, never negative).
pub fn ahead(t: &Track, a: f64, b: f64) -> f64 {
    if t.is_loop {
        let n = t.n as f64;
        ((b - a) % n + n) % n
    } else {
        b - a
    }
}

/// A trial's time to beat: the road's speed profile (the pace the rivals
/// drive to) from its start to its finish, with this much in hand.
pub const PAR_SLACK: f64 = 1.3;

/// Seconds the speed profile takes from `a` to `b` the road's way.
pub fn profile_time(t: &Track, a: f64, b: f64) -> f64 {
    let len = ahead(t, a, b);
    let mut secs = 0.0;
    let mut s = 0.0;
    while s < len {
        let v = f64::from(t.speed_profile[t.idx(a + s)]);
        secs += 1.0 / js::max(v, 5.0);
        s += 1.0;
    }
    secs
}

/// An event's time: a delivery's is the level's; a trial's the road's
/// speed profile with [`PAR_SLACK`], rounded up to five seconds.
pub fn time_of(t: &Track, e: &EventSpot) -> f64 {
    match e.kind {
        EventKind::TimeTrial => (profile_time(t, e.s, e.finish) * PAR_SLACK / 5.0).ceil() * 5.0,
        _ => e.time,
    }
}

/// The start whose marker the car is on, if any: on the level's own road
/// (road 0), within the marker's length and on the tarmac.
pub fn at_start(t: &Track, level: &Level, v: &Vehicle) -> Option<usize> {
    if v.road != 0 {
        return None;
    }
    level.events.iter().position(|e| {
        let d = t.ds(e.s, v.s);
        let f = t.frame(v.s);
        (-SPOT_BACK..=SPOT_AHEAD).contains(&d) && v.lat.abs() < f.hw + 1.0
    })
}

/// A start's point on the road (its marker's centre).
pub fn start_point(t: &Track, e: &EventSpot) -> (f64, f64) {
    let p = t.point_at(e.s, 0.0);
    (p.x, p.z)
}

/// The nearest start within `r` metres of (x, z), as the crow flies, and
/// how far it is.
pub fn nearest(t: &Track, level: &Level, x: f64, z: f64, r: f64) -> Option<(usize, f64)> {
    let mut best: Option<(usize, f64)> = None;
    for (i, e) in level.events.iter().enumerate() {
        let (sx, sz) = start_point(t, e);
        let d = kernel::hypot(sx - x, sz - z);
        if d < r && best.is_none_or(|(_, b)| d < b) {
            best = Some((i, d));
        }
    }
    best
}

/// The event being run's spot.
pub fn running<'a>(lr: &'a LevelRuntime, st: &SimState) -> Option<&'a EventSpot> {
    st.race
        .event
        .as_ref()
        .and_then(|e| lr.level.events.get(e.index))
}

/// Player `i` has reached a delivery's drop this tick.
pub fn delivered(lr: &LevelRuntime, st: &SimState, i: usize) -> bool {
    let Some(e) = running(lr, st) else {
        return false;
    };
    let Some([dx, dz]) = e.drop else {
        return false;
    };
    let v = &st.players[i].v;
    e.kind == EventKind::Delivery
        && st.race.state == RaceStateKind::Racing
        && kernel::hypot(v.x - dx, v.z - dz) < DROP_RADIUS
}

/// After a delivery's end: stand on the brakes where it ended (the
/// circuit's slow lap would steer for the level's road from wherever the
/// drop is).
pub fn halt() -> Input {
    Input {
        brake: 1.0,
        cruise: true,
        ..Input::default()
    }
}

/// The tick's part: a cruise's hold on a start (which starts its event),
/// and a delivery's end.
pub fn step(lr: &LevelRuntime, st: &mut SimState, inps: &[Input], events: &mut Vec<SimEvent>) {
    let t = &*lr.track;
    let level = &lr.level;
    if level.events.is_empty() || st.players.len() != 1 {
        return;
    }
    if st.race.cruise && st.race.event.is_none() {
        let p = &st.players[0];
        let inp = inps.first().copied().unwrap_or_default();
        let spot = at_start(t, level, &p.v).filter(|_| {
            st.race.state == RaceStateKind::Racing
                && kernel::hypot(p.v.vx, p.v.vz) < STOPPED
                && inp.throttle > 0.5
                && inp.brake > 0.5
        });
        st.race.hold = match (spot, st.race.hold) {
            (Some(i), Some((h, held))) if h == i => Some((i, held + crate::race::DT)),
            (Some(i), _) => Some((i, crate::race::DT)),
            (None, _) => None,
        };
        if let Some((i, held)) = st.race.hold
            && held >= HOLD
        {
            start(lr, st, i, events);
        }
        return;
    }
    // A delivery: decided when the car reaches the drop (the race's finish
    // check marks it finished) or when its time runs out.
    let Some(e) = running(lr, st) else {
        return;
    };
    if e.kind != EventKind::Delivery {
        return;
    }
    let limit = e.time;
    let time = st.race.time;
    let run = st.race.event.as_mut().expect("running");
    if run.decided {
        return;
    }
    let p = &mut st.players[0];
    if p.rules.finished {
        run.decided = true;
        run.delivered = true;
    } else if st.race.state == RaceStateKind::Racing && time > limit {
        run.decided = true;
        let r = &mut p.rules;
        r.finished = true;
        r.finish_time = Some(time);
        r.park = Some(Park::Circuit);
        r.finish_delay = 3.2;
        st.race.state = RaceStateKind::Finished;
        events.push(SimEvent::Finished {
            player: 0,
            place: 1,
        });
    }
}

/// Begins event `index`: its rivals on a grid behind the start (a race's),
/// the traffic round the grid gone, the countdown.
pub fn start(lr: &LevelRuntime, st: &mut SimState, index: usize, events: &mut Vec<SimEvent>) {
    let t = &*lr.track;
    let level = &lr.level;
    let e = &level.events[index];
    let run = EventRun {
        index,
        kind: e.kind,
        cruise: st.players[0].rules.clone(),
        cruise_time: st.race.time,
        decided: false,
        delivered: false,
    };
    for i in 0..st.traffic.cars.len() {
        let c = &st.traffic.cars[i];
        if c.active && t.ds(e.s, c.k.s).abs() < CLEAR {
            st.traffic.despawn(i);
        }
    }
    let mut rivals = if e.kind == EventKind::Race {
        level_rivals(level, &mut st.rng)
    } else {
        Vec::new()
    };
    let n_cars = st.traffic.cars.len();
    let p = &mut st.players[0];
    grid(t, &mut p.phys, &mut p.v, &mut rivals, e.s);
    p.phys.locked = true;
    p.rules = PlayerRules::new(n_cars);
    let race = &mut st.race;
    race.prog_s = std::iter::once(p.v.s)
        .chain(rivals.iter().map(|a| a.k.s))
        .collect();
    st.rivals = rivals;
    race.finish_prog = match e.kind {
        EventKind::Delivery => f64::INFINITY,
        _ => e.s + ahead(t, e.s, e.finish),
    };
    race.cruise = false;
    race.state = RaceStateKind::Countdown;
    race.countdown = 3.999;
    race.last_beep = 4.0;
    race.time = 0.0;
    race.throttle_at = None;
    race.park_rows = None;
    race.hold = None;
    race.event = Some(run);
    events.push(SimEvent::EventStart { index });
}

/// Ends the event being run: how it went, then the cruise as it was (its
/// score and multiplier; the distance and top speed carry the event's),
/// the rivals gone.
pub fn end(lr: &LevelRuntime, st: &mut SimState, events: &mut Vec<SimEvent>) {
    let t = &*lr.track;
    let Some(e) = running(lr, st) else {
        return;
    };
    let rows = results(st);
    let place = rows.iter().position(|r| r.human == Some(0)).unwrap_or(0) + 1;
    let p = &st.players[0];
    let time = p.rules.finish_time.unwrap_or(st.race.time);
    let run = st.race.event.take().expect("running");
    let made_it = match e.kind {
        EventKind::Race => place == 1,
        EventKind::TimeTrial => time <= time_of(t, e),
        EventKind::Delivery => run.delivered && time <= e.time,
    };
    let result = EventResult {
        index: run.index,
        kind: e.kind,
        time,
        place,
        rows: if e.kind == EventKind::Race {
            rows
        } else {
            Vec::new()
        },
        made_it,
    };
    let p = &mut st.players[0];
    let ev = std::mem::replace(&mut p.rules, run.cruise);
    let r = &mut p.rules;
    r.dist += ev.dist;
    r.top_speed = js::max(r.top_speed, ev.top_speed);
    r.near_misses += ev.near_misses;
    r.last_s = ev.last_s;
    r.odo = ev.odo;
    r.passed = ev.passed;
    r.near_miss_hit = ev.near_miss_hit;
    r.reset_cooldown = ev.reset_cooldown;
    r.stuck = None;
    r.wrong_way = 0.0;
    p.v.prog = None;
    p.phys.locked = false;
    st.rivals.clear();
    let race = &mut st.race;
    race.prog_s = vec![p.v.s];
    race.cruise = true;
    race.state = RaceStateKind::Racing;
    race.time += run.cruise_time;
    race.finish_prog = t.finish_s;
    race.park_rows = None;
    race.throttle_at = None;
    race.hold = None;
    events.push(SimEvent::EventEnd(result));
}
