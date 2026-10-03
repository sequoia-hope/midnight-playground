//! The race rules (port of the simulation half of `src/game/Race.js`, SPEC
//! 4.3): the state machine (countdown, racing, finished), the player's rule
//! state, circuits' lap counting, parking after the finish, the cool-down
//! driver, bonuses, cruise scoring, standings and results. `step` is
//! `Race.update` in its order; the HUD, audio, camera, effects and rumble it
//! drives in the JS become [`SimEvent`]s.
//!
//! The JS has one player; the state keeps `players` as a vector (SPEC 4.3)
//! and the rules here run for `players[0]`. Multiplayer (M10) decides what
//! the race-wide rules (rubber-banding, traffic around "the player") mean
//! with several.

use std::cmp::Ordering;
use std::sync::Arc;

use mr_levels::world::{WorldData, world_data};
use mr_math::{clamp, js, kernel, wrap_angle};
use mr_track::{Level, Mode, Track};

use crate::ai::{AiCtx, AiDriver, AiOpts};
use crate::body::{AgentView, Body, PhysicsBody, player_view};
use crate::collisions::{Hit, resolve_collisions};
use crate::dims::dims;
use crate::input::{Input, InputFrame, RESET};
use crate::park::{PARK_GAP, PARK_ROW, Park};
use crate::physics::{CarPhysics, CarSpec, PhysEvent, car_spec};
use crate::rng::RngStreams;
use crate::traffic::{Agent, Traffic, TrafficCar};
use crate::vehicle::Vehicle;

/// The fixed tick (SPEC 4.1).
pub const DT: f64 = 1.0 / 120.0;

/// Built once per level, shared, immutable (SPEC 4.3).
pub struct LevelRuntime {
    pub level: Level,
    pub track: Arc<Track>,
    pub world: WorldData,
}

impl LevelRuntime {
    /// The level's Track with the world data the scenery gives it (runout).
    pub fn new(level: Level) -> Result<LevelRuntime, String> {
        let mut t = Track::new(&level)?;
        let world = world_data(level.id);
        t.runout = world.runout;
        Ok(LevelRuntime {
            level,
            track: Arc::new(t),
            world,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RaceStateKind {
    Countdown,
    Racing,
    Finished,
}

/// The race as a whole (Race's own fields that are not about the player).
#[derive(Clone, Debug, PartialEq)]
pub struct RaceState {
    pub state: RaceStateKind,
    /// Race clock (runs from GO).
    pub time: f64,
    pub countdown: f64,
    /// The last countdown number announced (for the events).
    pub last_beep: f64,
    /// Perfect-start bookkeeping: the countdown when the throttle went down.
    pub throttle_at: Option<f64>,
    pub cruise: bool,
    pub pursuit_on: bool,
    /// Laps of a circuit, 0 elsewhere.
    pub laps: u32,
    /// The progress that finishes the race.
    pub finish_prog: f64,
    /// Cars parked in each lane past the finish (`parkRows`).
    pub park_rows: Option<Vec<i32>>,
    /// Circuits: each racer's s at the last progress update (`progS`):
    /// the player, then the rivals.
    pub prog_s: Vec<f64>,
}

/// Race's state about a player (SPEC 4.3: per player, in `PlayerCar`).
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerRules {
    pub lap: i32,
    pub lap_start: f64,
    pub lap_times: Vec<f64>,
    pub score: f64,
    pub mult: f64,
    pub mult_timer: f64,
    pub top_speed: f64,
    pub dist: f64,
    pub near_misses: i32,
    pub bonus_cooldown: f64,
    pub reset_cooldown: f64,
    pub wrong_way: f64,
    pub finish_delay: f64,
    pub stuck: Option<f64>,
    pub last_drift: f64,
    pub finished: bool,
    pub finish_time: Option<f64>,
    /// Results handed over.
    pub reported: bool,
    /// The cool-down driver's overtake.
    pub pass_timer: Option<f64>,
    pub pass_lat: Option<f64>,
    pub park: Option<Park>,
    pub last_s: Option<f64>,
    /// Unwrapped position (loops).
    pub odo: Option<f64>,
    /// Per traffic car: the last along-track gap to the player (`passed`).
    pub passed: Vec<Option<f64>>,
    /// Per traffic car: hit by the player, which cancels its near miss.
    pub near_miss_hit: Vec<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayerCar {
    pub v: Vehicle,
    pub phys: CarPhysics,
    pub spec: CarSpec,
    pub rules: PlayerRules,
}

/// Everything that changes. Plain data.
#[derive(Clone, Debug, PartialEq)]
pub struct SimState {
    pub tick: u32,
    pub race: RaceState,
    pub players: Vec<PlayerCar>,
    pub rivals: Vec<AiDriver>,
    pub traffic: Traffic,
    pub rng: RngStreams,
}

/// What the simulation tells the client (SPEC 4.3, "events, not side
/// effects"): the HUD, audio, rumble, camera and effects calls Race makes.
#[derive(Clone, Debug, PartialEq)]
pub enum SimEvent {
    /// A countdown number: 3, 2, 1.
    Countdown(i32),
    Go,
    PerfectStart,
    /// A physics event of player `player` (shift, land, wall impact,
    /// touchdown).
    Phys {
        player: usize,
        e: PhysEvent,
    },
    /// A car-to-car contact; `player` when a player's car is in it.
    CarHit {
        hit: Hit,
        player: Option<usize>,
    },
    /// A crash that costs the cruise multiplier.
    Crash {
        player: usize,
    },
    NearMiss {
        player: usize,
        traffic: usize,
        lat: f64,
        rel: f64,
    },
    /// A near pass that is not a near miss (the audio's whoosh).
    Whoosh {
        player: usize,
        traffic: usize,
        lat: f64,
        rel: f64,
    },
    /// A bonus toast: its text, the nitro it gave and the points scored.
    Bonus {
        player: usize,
        text: String,
        nitro: f64,
        points: f64,
    },
    WrongWay {
        player: usize,
    },
    Lap {
        player: usize,
        lap: i32,
        time: f64,
        best: bool,
    },
    Finished {
        player: usize,
        place: usize,
    },
    /// Race.onFinish: the results are ready.
    Results,
    Reset {
        player: usize,
    },
}

/// How a race is set up (Race's constructor arguments).
#[derive(Clone, Copy, Debug)]
pub struct RaceOpts {
    pub car: &'static str,
    pub seed: u32,
    pub pursuit: bool,
}

impl SimState {
    /// `new Race(...)`: the field on the grid, traffic, the countdown.
    pub fn new(lr: &LevelRuntime, o: RaceOpts) -> SimState {
        let level = &lr.level;
        let t = &*lr.track;
        let cruise = level.mode == Mode::Cruise;
        let pursuit_on = o.pursuit && level.police.is_some() && !cruise;
        let laps = t.laps;
        let finish_prog = if laps > 0 {
            t.start_s + laps as f64 * t.n as f64
        } else {
            t.finish_s
        };
        let mut rng = RngStreams::new(o.seed);
        let spec = car_spec(o.car).expect("a car in CAR_SPECS");
        let mut v = Vehicle::new(
            dims(o.car).expect("dims"),
            o.car,
            spec.mass,
            "You",
            spec.color,
        );
        let mut phys = CarPhysics::new(&v, spec);

        // Rivals.
        let mut rivals: Vec<AiDriver> = level
            .rivals
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let rv = Vehicle::new(dims(r.kind).expect("dims"), r.kind, 1400.0, r.name, r.color);
                let opts = AiOpts {
                    skill: Some(r.skill),
                    name: r.name,
                    power: Some(r.power),
                    bias: Some((if i % 2 == 1 { 1.0 } else { -1.0 }) * 0.6),
                    line_factor: Some(0.8 + (i % 3) as f64 * 0.08),
                };
                let mut ai = AiDriver::new(rv, opts, &mut rng.ai);
                ai.color = r.color;
                ai
            })
            .collect();

        // Grid: rows of two; the player starts fourth (alone for a cruise).
        let order: Vec<Option<usize>> = if rivals.len() >= 5 {
            vec![Some(0), Some(1), Some(2), None, Some(3), Some(4)]
        } else {
            std::iter::once(None)
                .chain((0..rivals.len()).map(Some))
                .collect()
        };
        for (k, c) in order.into_iter().enumerate() {
            let (row, col) = ((k / 2) as f64, k % 2);
            let s = t.start_s - 5.0 - row * 10.0 - col as f64 * 3.0;
            let lat = if col == 1 { 2.4 } else { -2.4 };
            match c {
                None => {
                    phys.reset(&mut v, t, t.wrap(s), lat);
                    v.prog = Some(s);
                }
                Some(i) => {
                    let a = &mut rivals[i];
                    a.k.s = t.wrap(s);
                    a.k.lat = lat;
                    a.k.speed = 0.0;
                    a.prog = Some(s);
                    a.write_pos(t);
                }
            }
        }
        let prog_s = std::iter::once(v.s)
            .chain(rivals.iter().map(|a| a.k.s))
            .collect();

        let count = if cruise {
            30
        } else if pursuit_on {
            18
        } else {
            22
        };
        let traffic = Traffic::new(
            level,
            lr.world.opposite_carriageway.clone(),
            count,
            &mut rng.traffic,
        );
        phys.locked = true;
        let n_cars = traffic.cars.len();
        SimState {
            tick: 0,
            race: RaceState {
                state: RaceStateKind::Countdown,
                time: 0.0,
                countdown: 3.999,
                last_beep: 4.0,
                throttle_at: None,
                cruise,
                pursuit_on,
                laps,
                finish_prog,
                park_rows: None,
                prog_s,
            },
            players: vec![PlayerCar {
                v,
                phys,
                spec,
                rules: PlayerRules {
                    lap: 1,
                    lap_start: 0.0,
                    lap_times: Vec::new(),
                    score: 0.0,
                    mult: 1.0,
                    mult_timer: 0.0,
                    top_speed: 0.0,
                    dist: 0.0,
                    near_misses: 0,
                    bonus_cooldown: 0.0,
                    reset_cooldown: 0.0,
                    wrong_way: 0.0,
                    finish_delay: 0.0,
                    stuck: None,
                    last_drift: 0.0,
                    finished: false,
                    finish_time: None,
                    reported: false,
                    pass_timer: None,
                    pass_lat: None,
                    park: None,
                    last_s: None,
                    odo: None,
                    passed: vec![None; n_cars],
                    near_miss_hit: vec![false; n_cars],
                },
            }],
            rivals,
            traffic,
            rng,
        }
    }
}

/// Who an entry of the agent list is (the JS list holds the objects).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentRef {
    Player(usize),
    Rival(usize),
    Traffic(usize),
}

fn view(st: &SimState, r: AgentRef) -> AgentView {
    match r {
        AgentRef::Player(i) => player_view(&st.players[i].v),
        AgentRef::Rival(i) => st.rivals[i].view(),
        AgentRef::Traffic(i) => st.traffic.cars[i].view(),
    }
}

/// A comparator result as JS's sort reads it: negative, positive, or
/// (zero or NaN) equal.
fn js_order(x: f64) -> Ordering {
    if x < 0.0 {
        Ordering::Less
    } else if x > 0.0 {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

/// One line of the standings.
#[derive(Clone, Debug, PartialEq)]
pub struct Standing {
    pub player: bool,
    pub name: &'static str,
    pub color: u32,
    pub s: f64,
    pub prog: Option<f64>,
    pub finished: bool,
    pub time: Option<f64>,
}

/// `standings()`: finishers by time, then the rest by progress (on a
/// circuit `prog`, elsewhere s). A stable sort, as JS's.
pub fn standings(st: &SimState) -> Vec<Standing> {
    let laps = st.race.laps > 0;
    let p = &st.players[0];
    let mut list = vec![Standing {
        player: true,
        name: "You",
        color: p.spec.color,
        s: p.v.s,
        prog: if laps { p.v.prog } else { Some(p.v.s) },
        finished: p.rules.finished,
        time: p.rules.finish_time,
    }];
    for a in &st.rivals {
        list.push(Standing {
            player: false,
            name: a.name,
            color: a.color,
            s: a.k.s,
            prog: if laps { a.prog } else { Some(a.k.s) },
            finished: a.finished,
            time: a.finish_time,
        });
    }
    let num = |x: Option<f64>| x.unwrap_or(f64::NAN);
    list.sort_by(|a, b| {
        if a.finished && b.finished {
            return js_order(num(a.time) - num(b.time));
        }
        if a.finished {
            return Ordering::Less;
        }
        if b.finished {
            return Ordering::Greater;
        }
        js_order(num(b.prog) - num(a.prog))
    });
    list
}

/// One line of the results.
#[derive(Clone, Debug, PartialEq)]
pub struct ResultRow {
    pub place: usize,
    pub name: &'static str,
    pub player: bool,
    pub color: u32,
    pub time: f64,
    pub estimated: bool,
}

/// `results()`: the standings, with times estimated for anyone still
/// driving.
pub fn results(st: &SimState) -> Vec<ResultRow> {
    standings(st)
        .into_iter()
        .enumerate()
        .map(|(i, r)| {
            let time = if r.finished {
                r.time.unwrap_or(f64::NAN)
            } else {
                let rem = st.race.finish_prog - r.prog.unwrap_or(f64::NAN);
                st.race.time + rem / 45.0
            };
            ResultRow {
                place: i + 1,
                name: r.name,
                player: r.player,
                color: r.color,
                time,
                estimated: !r.finished,
            }
        })
        .collect()
}

/// `cruiseResults()`.
#[derive(Clone, Debug, PartialEq)]
pub struct CruiseResults {
    pub score: f64,
    pub dist: f64,
    pub top: f64,
    pub time: f64,
    pub near_misses: i32,
}

pub fn cruise_results(st: &SimState) -> CruiseResults {
    let r = &st.players[0].rules;
    CruiseResults {
        score: js::round(r.score),
        dist: r.dist,
        top: r.top_speed,
        time: st.race.time,
        near_misses: r.near_misses,
    }
}

/// A finisher's parking spot (`parkSpot`): the lane nearest where it crossed
/// the line, unless that lane is already filling up, in the row behind
/// anyone parked there.
pub fn park_spot(t: &Track, race: &mut RaceState, s0: f64, lat0: f64) -> Park {
    // A circuit has no end to park at: a slow lap on the right-hand side,
    // easing off for the corners, and the racers still going pass on the left.
    if race.laps > 0 {
        return Park::Circuit;
    }
    let front = js::max(t.finish_s + 60.0, t.road_end() - PARK_GAP);
    let f = t.frame(front);
    let n = js::max(1.0, (f.hw / 2.1).floor()) as usize; // four lanes on a freeway
    let w = (2.0 * f.hw) / n as f64;
    let lane_lat = |i: usize| -f.hw + w * (i as f64 + 0.5);
    let rows = race.park_rows.get_or_insert_with(|| vec![0; n]);
    let (mut lane, mut best) = (0, f64::INFINITY);
    for i in 0..n {
        let cost = rows[i] as f64 * 6.0 + (lane_lat(i) - lat0).abs();
        if cost < best {
            best = cost;
            lane = i;
        }
    }
    let stop_at = front - rows[lane] as f64 * PARK_ROW;
    rows[lane] += 1;
    Park::Lane {
        stop_at,
        lane_lat: lane_lat(lane),
        s0,
        lat0,
    }
}

/// Drives the player's car after the finish, toward its parking spot
/// (`coolDown`).
fn cool_down(t: &Track, p: &mut PlayerCar, me: usize, agents: &[AgentView], dt: f64) -> Input {
    let v = &p.v;
    let park = p.rules.park.expect("a finisher has a parking spot");
    let sp = kernel::hypot(v.vx, v.vz);
    let look = 8.0 + sp * 0.6;
    let mut target = park.speed(t, v.s);
    let mut lat = park.lat(t, v.s + look);
    // Anything ahead in our path: go round it while we're closing fast
    // (like the rivals do), otherwise queue behind it.
    let (mut block, mut gap) = (None, f64::INFINITY);
    for (i, o) in agents.iter().enumerate() {
        if i == me || o.dir != 1 {
            continue;
        }
        let ds = t.ds(v.s, o.s);
        let in_path = (o.lat - v.lat).abs() < v.half_w + o.half_w + 0.7
            || (o.lat - lat).abs() < v.half_w + o.half_w + 0.7;
        if ds > 2.0 && ds < 60.0 && ds < gap && in_path {
            block = Some(o);
            gap = ds;
        }
    }
    let r = &mut p.rules;
    r.pass_timer = Some(js::max(0.0, js::or_opt(r.pass_timer, 0.0) - dt));
    match block {
        Some(b) if sp - b.speed_along > 5.0 => {
            let f = t.frame(v.s);
            let lim = f.hw - v.half_w - 0.4;
            let need = v.half_w + b.half_w + 1.2;
            let sides: Vec<f64> = [b.lat + need, b.lat - need]
                .into_iter()
                .filter(|l| l.abs() < lim)
                .collect();
            if !sides.is_empty() {
                // sides.sort((a, b) => |a - lat| - |b - lat|)[0], stable.
                let pick =
                    if sides.len() == 2 && (sides[0] - lat).abs() - (sides[1] - lat).abs() > 0.0 {
                        sides[1]
                    } else {
                        sides[0]
                    };
                r.pass_lat = Some(pick);
                r.pass_timer = Some(0.9);
            } else {
                target = js::min(target, b.speed_along);
            }
        }
        Some(b) => target = js::min(target, js::max(0.0, b.speed_along + (gap - 10.0) * 0.4)),
        None => {}
    }
    if r.pass_timer.unwrap_or(0.0) > 0.0 {
        lat = r.pass_lat.unwrap_or(f64::NAN);
    }
    let pt = t.point_at(v.s + look, lat);
    let err = target - sp;
    Input {
        throttle: if target > 0.5 && err > 0.0 {
            clamp(0.08 + err * 0.15, 0.0, 0.6)
        } else {
            0.0
        },
        brake: if err < -0.3 && sp > 1.0 {
            clamp(-err * 0.12, 0.0, 0.4)
        } else {
            0.0
        },
        steer: clamp(
            wrap_angle(kernel::atan2(pt.z - v.z, pt.x - v.x) - v.yaw) * 2.0,
            -1.0,
            1.0,
        ),
        handbrake: false,
        nitro: false,
        analog: false,
        cruise: true,
    }
}

/// `crash()`: a crash costs the cruise multiplier.
fn crash(race: &RaceState, r: &mut PlayerRules, events: &mut Vec<SimEvent>) {
    if !race.cruise || race.state == RaceStateKind::Countdown {
        return;
    }
    events.push(SimEvent::Crash { player: 0 });
    r.mult = 1.0;
    r.mult_timer = 0.0;
}

/// `bonus()`: nitro, and in a cruise the points (a chain raises the
/// multiplier).
fn bonus(
    race: &RaceState,
    p: &mut PlayerCar,
    text: String,
    nitro: f64,
    points: f64,
    chain: bool,
    events: &mut Vec<SimEvent>,
) {
    p.phys.nitro = js::min(1.0, p.phys.nitro + nitro);
    let r = &mut p.rules;
    let mut scored = 0.0;
    if race.cruise && points != 0.0 {
        let gained = js::round(points * r.mult);
        r.score += gained;
        if chain {
            r.mult = js::min(10.0, r.mult + 1.0);
            r.mult_timer = 6.0;
        }
        scored = gained;
    }
    events.push(SimEvent::Bonus {
        player: 0,
        text,
        nitro,
        points: scored,
    });
}

/// `resetPlayer()`: back on the road five metres back, on the tarmac.
fn reset_player(t: &Track, race: &RaceState, p: &mut PlayerCar, events: &mut Vec<SimEvent>) {
    let s = if t.is_loop {
        t.wrap(p.v.s - 5.0)
    } else {
        js::max(t.start_s, p.v.s - 5.0)
    };
    crash(race, &mut p.rules, events);
    let f = t.frame(s);
    let lat = clamp(p.v.lat, -f.hw * 0.5, f.hw * 0.5);
    p.phys.reset(&mut p.v, t, s, lat);
    p.rules.reset_cooldown = 2.0;
    events.push(SimEvent::Reset { player: 0 });
}

/// One tick (`Race.update`, SPEC 4.3's tick order). `inputs[0]` is the
/// player's.
pub fn step(
    lr: &LevelRuntime,
    st: &mut SimState,
    inputs: &[InputFrame],
    events: &mut Vec<SimEvent>,
) {
    let t = &*lr.track;
    let dt = DT;
    st.tick += 1;
    let frame = inputs.first().copied().unwrap_or_default();
    let inp = frame.input();
    let race = &mut st.race;

    // ── State machine ──────────────────────────────────────────
    if race.state == RaceStateKind::Countdown {
        race.countdown -= dt;
        // Perfect start = get on the throttle in the last moment before GO.
        if inp.throttle > 0.5 {
            if race.throttle_at.is_none() {
                race.throttle_at = Some(race.countdown);
            }
        } else {
            race.throttle_at = None;
        }
        let n = race.countdown.ceil();
        if n < race.last_beep && n >= 1.0 {
            events.push(SimEvent::Countdown(n as i32));
            race.last_beep = n;
        }
        if race.countdown <= 0.0 {
            race.state = RaceStateKind::Racing;
            let p = &mut st.players[0];
            p.phys.locked = false;
            events.push(SimEvent::Go);
            if race.throttle_at.is_some_and(|ta| ta < 0.75) {
                let fx = kernel::cos(p.v.yaw);
                let fz = kernel::sin(p.v.yaw);
                p.v.vx += fx * 6.0;
                p.v.vz += fz * 6.0;
                events.push(SimEvent::PerfectStart);
            }
        }
    }
    let started = race.state != RaceStateKind::Countdown;
    if started {
        race.time += dt;
    }

    // ── Player ─────────────────────────────────────────────────
    {
        let p = &mut st.players[0];
        p.rules.reset_cooldown = js::max(0.0, p.rules.reset_cooldown - dt);
        if frame.flags & RESET != 0 && started && p.rules.reset_cooldown == 0.0 {
            reset_player(t, &st.race, p, events);
        }
    }
    // ── Agents list for AI/traffic awareness ───────────────────
    let mut agents: Vec<AgentRef> = vec![AgentRef::Player(0)];
    agents.extend((0..st.rivals.len()).map(AgentRef::Rival));
    agents.extend(
        st.traffic
            .cars
            .iter()
            .enumerate()
            .filter(|(_, c)| c.active)
            .map(|(i, _)| AgentRef::Traffic(i)),
    );
    let ctrl = if st.players[0].rules.finished {
        let views: Vec<AgentView> = agents.iter().map(|&r| view(st, r)).collect();
        cool_down(t, &mut st.players[0], 0, &views, dt)
    } else {
        inp
    };
    {
        let p = &mut st.players[0];
        p.phys.update(&mut p.v, t, dt, &ctrl);
    }

    let player_s = st.players[0].v.s;
    let player_prog = if st.race.laps > 0 {
        st.players[0].v.prog
    } else {
        None
    };
    for i in 0..st.rivals.len() {
        let views: Vec<AgentView> = agents.iter().map(|&r| view(st, r)).collect();
        let ctx = AiCtx {
            cars: &views,
            me: Some(1 + i),
            player_s,
            player_prog,
            started,
            time: st.race.time,
        };
        st.rivals[i].update(t, dt, &ctx, &mut st.rng.ai);
        let a = &st.rivals[i];
        if a.finished && a.park.is_none() {
            let (s, lat) = (a.k.s, a.k.lat);
            let park = park_spot(t, &mut st.race, s, lat);
            st.rivals[i].park = Some(park);
        }
    }
    if st.race.laps > 0 {
        track_progress(t, st);
    }
    let odo = {
        let s = st.players[0].v.s;
        let r = &mut st.players[0].rules;
        let d_s = t.ds(r.last_s.unwrap_or(s), s);
        r.last_s = Some(s);
        r.dist += d_s.abs();
        let odo = r.odo.unwrap_or(s) + d_s; // unwrapped position (loops)
        r.odo = Some(odo);
        odo
    };
    {
        let list: Vec<Agent> = agents
            .iter()
            .map(|&r| match r {
                AgentRef::Traffic(i) => Agent::Traffic(i),
                r => Agent::Other(view(st, r)),
            })
            .collect();
        let dist = if t.is_loop { odo } else { player_s };
        st.traffic
            .update(t, dt, player_s, &list, 0.0, dist, &mut st.rng.traffic);
    }

    // ── Collisions ─────────────────────────────────────────────
    let hits = collide(st, t, &agents);
    for a in &mut st.rivals {
        a.write_pos(t);
    }
    for c in st.traffic.cars.iter_mut().filter(|c| c.active) {
        c.k.write_pos(t);
    }
    for h in hits {
        let involves_player =
            agents[h.a] == AgentRef::Player(0) || agents[h.b] == AgentRef::Player(0);
        let other = if agents[h.a] == AgentRef::Player(0) {
            h.b
        } else {
            h.a
        };
        let other_traffic = match agents[other] {
            AgentRef::Traffic(i) => Some(i),
            _ => None,
        };
        if let Some(i) = other_traffic
            && h.strength > 0.15
        {
            let c = &mut st.traffic.cars[i];
            c.crashed = js::max(c.crashed, 0.01);
        }
        events.push(SimEvent::CarHit {
            hit: h,
            player: involves_player.then_some(0),
        });
        if involves_player {
            if let Some(i) = other_traffic {
                st.players[0].rules.near_miss_hit[i] = true;
            }
            if h.strength > 0.2 {
                crash(&st.race, &mut st.players[0].rules, events);
            }
        }
    }
    let phys_events = std::mem::take(&mut st.players[0].phys.events);
    for e in phys_events {
        match e {
            PhysEvent::Impact { strength, .. } if strength > 0.35 => {
                crash(&st.race, &mut st.players[0].rules, events)
            }
            PhysEvent::Land { air, .. } if air > 0.55 => {
                events.push(SimEvent::Phys { player: 0, e });
                bonus(
                    &st.race,
                    &mut st.players[0],
                    format!("AIR {}s", to_fixed1(air)),
                    0.12,
                    0.0,
                    false,
                    events,
                );
                continue;
            }
            _ => {}
        }
        events.push(SimEvent::Phys { player: 0, e });
    }

    // ── Bonuses: drift, near miss, overtakes ───────────────────
    {
        let p = &mut st.players[0];
        p.rules.bonus_cooldown = js::max(0.0, p.rules.bonus_cooldown - dt);
        if p.phys.drifting {
            p.rules.last_drift = p.phys.drift_time;
        } else if p.rules.last_drift > 1.2 {
            let ld = p.rules.last_drift;
            bonus(
                &st.race,
                p,
                format!("DRIFT {}s", to_fixed1(ld)),
                0.0,
                js::round(ld * 150.0),
                false,
                events,
            );
            p.rules.last_drift = 0.0;
        } else {
            p.rules.last_drift = 0.0;
        }
    }
    let psp = {
        let v = &st.players[0].v;
        kernel::hypot(v.vx, v.vz)
    };
    for ci in 0..st.traffic.cars.len() {
        let c: &TrafficCar = &st.traffic.cars[ci];
        if !c.active {
            continue;
        }
        let p = &st.players[0];
        let ds = t.ds(p.v.s, c.k.s);
        let prev = p.rules.passed[ci];
        let (clat, chw, cdir, cspeed) = (c.k.lat, c.k.v.half_w, c.k.dir, c.k.speed);
        st.players[0].rules.passed[ci] = Some(ds);
        if let Some(prev) = prev
            && prev > 0.0
            && ds <= 0.0
        {
            let p = &st.players[0];
            let gap = (clat - p.v.lat).abs() - chw - p.v.half_w;
            let rel = if cdir == 1 {
                psp - cspeed
            } else {
                psp + cspeed
            };
            let lat = clat - p.v.lat;
            if !p.rules.near_miss_hit[ci] && gap < 1.4 && rel > 12.0 {
                st.players[0].rules.near_misses += 1;
                events.push(SimEvent::NearMiss {
                    player: 0,
                    traffic: ci,
                    lat,
                    rel,
                });
                bonus(
                    &st.race,
                    &mut st.players[0],
                    "NEAR MISS".to_string(),
                    0.08,
                    250.0,
                    true,
                    events,
                );
            } else if rel > 20.0 && gap < 4.0 {
                events.push(SimEvent::Whoosh {
                    player: 0,
                    traffic: ci,
                    lat,
                    rel,
                });
            }
        }
    }

    // Wrong way / finish.
    {
        let p = &mut st.players[0];
        let f = t.frame(p.v.s);
        let along = kernel::cos(p.v.yaw) * f.fx + kernel::sin(p.v.yaw) * f.fz;
        let spd = p.v.speed;
        if started && !p.rules.finished && along < -0.3 && spd > 4.0 {
            p.rules.wrong_way += dt;
        } else {
            p.rules.wrong_way = 0.0;
        }
        if p.rules.wrong_way > 1.5 {
            events.push(SimEvent::WrongWay { player: 0 });
        }
        // Stuck? Offer the reset key.
        p.rules.stuck = Some(if started && !p.rules.finished && spd < 1.5 {
            js::or_opt(p.rules.stuck, 0.0) + dt
        } else {
            0.0
        });
    }

    if st.race.cruise && started {
        cruise_score(&mut st.players[0].rules, dt, psp);
    }
    if st.race.laps > 0 && started && !st.players[0].rules.finished {
        lap_check(t, st, events);
    }
    let prog_now = if st.race.laps > 0 {
        st.players[0].v.prog.unwrap_or(f64::NAN)
    } else {
        st.players[0].v.s
    };
    if !st.race.cruise
        && !st.players[0].rules.finished
        && prog_now >= st.race.finish_prog
        && started
    {
        let time = st.race.time;
        let laps = st.race.laps > 0;
        {
            let r = &mut st.players[0].rules;
            r.finished = true;
            r.finish_time = Some(time);
            if laps {
                let lt = time - r.lap_start;
                r.lap_times.push(lt);
            }
        }
        let (s, lat) = (st.players[0].v.s, st.players[0].v.lat);
        let park = park_spot(t, &mut st.race, s, lat);
        st.players[0].rules.park = Some(park);
        let place = standings(st).iter().position(|r| r.player).unwrap() + 1;
        events.push(SimEvent::Finished { player: 0, place });
        st.race.state = RaceStateKind::Finished;
        st.players[0].rules.finish_delay = 3.2;
    }
    if st.race.state == RaceStateKind::Finished {
        let r = &mut st.players[0].rules;
        r.finish_delay -= dt;
        if r.finish_delay <= 0.0 && !r.reported {
            r.reported = true;
            events.push(SimEvent::Results);
        }
    }
}

/// `x.toFixed(1)` for the bonus texts.
fn to_fixed1(x: f64) -> String {
    format!("{:.1}", js::round(x * 10.0) / 10.0)
}

/// Circuits: carry each racer's unwrapped progress on by how far it moved
/// along the loop this tick (a reset back down the road counts too).
fn track_progress(t: &Track, st: &mut SimState) {
    {
        let p = &mut st.players[0];
        let last = st.race.prog_s[0];
        p.v.prog = Some(p.v.prog.unwrap_or(f64::NAN) + t.ds(last, p.v.s));
        st.race.prog_s[0] = p.v.s;
    }
    for (i, a) in st.rivals.iter_mut().enumerate() {
        let last = st.race.prog_s[1 + i];
        a.prog = Some(a.prog.unwrap_or(f64::NAN) + t.ds(last, a.k.s));
        st.race.prog_s[1 + i] = a.k.s;
    }
    for a in &mut st.rivals {
        if !a.finished && a.prog.unwrap_or(f64::NAN) >= st.race.finish_prog {
            a.finished = true;
            a.finish_time = Some(st.race.time);
        }
    }
}

/// The player's lap: announce each new one and keep the lap times.
fn lap_check(t: &Track, st: &mut SimState, events: &mut Vec<SimEvent>) {
    let p = &mut st.players[0];
    let done = ((p.v.prog.unwrap_or(f64::NAN) - t.start_s) / t.n as f64).floor(); // laps completed
    let r = &mut p.rules;
    if done < r.lap as f64 || done >= st.race.laps as f64 {
        return;
    }
    let lt = st.race.time - r.lap_start;
    r.lap_times.push(lt);
    r.lap_start = st.race.time;
    r.lap = done as i32 + 1;
    let best = js::min_n(&r.lap_times) == lt && r.lap_times.len() > 1;
    events.push(SimEvent::Lap {
        player: 0,
        lap: r.lap,
        time: lt,
        best,
    });
}

/// Cruise scoring: points for sustained speed, a multiplier from chained
/// near misses that decays if you stop taking risks, lost on a crash.
fn cruise_score(r: &mut PlayerRules, dt: f64, speed: f64) {
    r.top_speed = js::max(r.top_speed, speed);
    let kmh = speed * 3.6;
    if kmh > 120.0 {
        r.score += (kmh - 120.0) * 0.9 * r.mult * dt;
    }
    r.mult_timer -= dt;
    if r.mult_timer <= 0.0 && r.mult > 1.0 {
        r.mult -= 1.0;
        r.mult_timer = 2.5;
    }
}

/// `resolveCollisions(agents, hits)` over the state's pools, in agent order.
fn collide(st: &mut SimState, t: &Track, agents: &[AgentRef]) -> Vec<Hit> {
    let mut hits = Vec::new();
    let mut players: Vec<Option<PhysicsBody>> = st
        .players
        .iter_mut()
        .map(|p| Some(PhysicsBody { v: &mut p.v }))
        .collect();
    let mut rivals: Vec<Option<&mut AiDriver>> = st.rivals.iter_mut().map(Some).collect();
    let mut cars: Vec<Option<&mut TrafficCar>> = st.traffic.cars.iter_mut().map(Some).collect();
    let mut taken_players: Vec<PhysicsBody> = Vec::new();
    let mut order: Vec<(u8, usize)> = Vec::with_capacity(agents.len());
    for &r in agents {
        match r {
            AgentRef::Player(i) => {
                taken_players.push(players[i].take().unwrap());
                order.push((0, taken_players.len() - 1));
            }
            AgentRef::Rival(i) => order.push((1, i)),
            AgentRef::Traffic(i) => order.push((2, i)),
        }
    }
    let mut tp: Vec<Option<&mut PhysicsBody>> = taken_players.iter_mut().map(Some).collect();
    let mut bodies: Vec<&mut dyn Body> = Vec::with_capacity(agents.len());
    for (kind, i) in order {
        match kind {
            0 => bodies.push(tp[i].take().unwrap()),
            1 => bodies.push(rivals[i].take().unwrap()),
            _ => bodies.push(cars[i].take().unwrap()),
        }
    }
    resolve_collisions(&mut bodies[..], t, &mut hits);
    hits
}

/// A 64-bit hash of the state, for determinism and desync checks (SPEC 4.3).
/// It is the FNV-1a 64 of the trace record without inputs, which covers
/// every field that affects later ticks.
pub fn hash(st: &SimState) -> u64 {
    crate::trace::fnv1a64(&crate::trace::race_record(st, &[]))
}
