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

use mp_levels::world::{WorldData, level_world_data};
use mp_math::{clamp, js, kernel, wrap_angle};
use mp_track::{Level, Mode, Track};

use crate::ai::{AiCtx, AiDriver, AiOpts};
use crate::autopilot::autopilot;
use crate::body::{AgentView, BodyId};
use crate::collisions::Hit;
use crate::dims::dims;
use crate::field::{Field, RacerAccess};
use crate::input::{AUTOPILOT, Input, InputFrame, RESET};
use crate::park::{PARK_GAP, PARK_ROW, Park};
use crate::physics::{CarPhysics, CarSpec, PhysEvent, car_spec};
use crate::police::Mode as PoliceMode;
use crate::pursuit::{HoldReason, Pursuit, PursuitEvent, PursuitOpts, WRECK_PENALTY, top_speed};
use crate::rng::RngStreams;
use crate::trace::LastHit;
use crate::traffic::{Traffic, TrafficCar};
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
        let world = level_world_data(&level, &t);
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
    /// the players, then the rivals.
    pub prog_s: Vec<f64>,
    /// A multiplayer race's rules and end ([`SimState::new_multi`]).
    pub multi: Option<Multi>,
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
    /// Multiplayer: the perfect-start moment of players after the first
    /// (player 0's is `RaceState::throttle_at`, as in the JS).
    pub throttle_at: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayerCar {
    pub v: Vehicle,
    pub phys: CarPhysics,
    pub spec: CarSpec,
    pub rules: PlayerRules,
}

impl PlayerRules {
    /// A player's rules at the start (`n_cars`: the traffic pool's size).
    pub fn new(n_cars: usize) -> PlayerRules {
        PlayerRules {
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
            throttle_at: None,
        }
    }
}

impl PlayerCar {
    /// The player's car as Race builds it.
    pub fn new(car: &'static str) -> PlayerCar {
        let spec = car_spec(car).expect("a car in CAR_SPECS");
        let v = Vehicle::new(dims(car).expect("dims"), car, spec.mass, "You", spec.color);
        let phys = CarPhysics::new(&v, spec);
        PlayerCar {
            v,
            phys,
            spec,
            rules: PlayerRules::new(0),
        }
    }
}

/// Everything that changes. Plain data.
#[derive(Clone, Debug, PartialEq)]
pub struct SimState {
    pub tick: u32,
    pub race: RaceState,
    pub players: Vec<PlayerCar>,
    pub rivals: Vec<AiDriver>,
    pub traffic: Traffic,
    /// Hot Pursuit, when it is on.
    pub pv: Option<PursuitView>,
    pub rng: RngStreams,
}

/// What the simulation tells the client (SPEC 4.3, "events, not side
/// effects"): the HUD, audio, rumble, camera and effects calls Race makes.
#[derive(Clone, Debug, PartialEq)]
pub enum SimEvent {
    /// A countdown number: 3, 2, 1.
    Countdown(i32),
    Go,
    /// Player `player` got on the throttle at the right moment.
    PerfectStart {
        player: usize,
    },
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
    /// Something the pursuit did (heat, units, props, busts, ...).
    Pursuit(PursuitEvent),
    /// The player drove this tick on controls other than its input: the
    /// cool-down driver after the finish, or the hold of a bust. The audio
    /// hears this throttle (`ctrl.throttle` in Race.update's audio call).
    Controls {
        player: usize,
        throttle: f64,
    },
}

/// How a race is set up (Race's constructor arguments).
#[derive(Clone, Copy, Debug)]
pub struct RaceOpts {
    pub car: &'static str,
    pub seed: u32,
    pub pursuit: bool,
    /// Hot Pursuit's starting heat (`?heat=`, 1 by default).
    pub heat: f64,
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
        let pv = pursuit_on.then(|| {
            let opts = PursuitOpts {
                heat: o.heat,
                max_units: 6.0,
                player_top: top_speed(&spec),
                flash: true,
            };
            let RngStreams {
                pursuit, police, ..
            } = &mut rng;
            let mut pu = Pursuit::new(t, level, opts, pursuit, police);
            let mut list = vec![(BodyId::Player(0), true, false, "You")];
            list.extend(
                rivals
                    .iter()
                    .enumerate()
                    .map(|(i, a)| (BodyId::Rival(i), false, true, a.name)),
            );
            pu.set_racers(list);
            let last_hit = LastHit {
                rivals: vec![None; rivals.len()],
                traffic: vec![None; n_cars],
                police: vec![None; pu.units.len() + pu.block_cars.len()],
                sawhorses: vec![None; pu.sawhorses.len()],
            };
            PursuitView {
                pursuit: pu,
                damage: 0.0,
                wrecks: 0,
                penalty: 0.0,
                t: 0.0,
                last_hit,
            }
        });
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
                multi: None,
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
                    throttle_at: None,
                },
            }],
            rivals,
            traffic,
            pv,
            rng,
        }
    }
}

/// A human in a multiplayer race.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Human {
    pub car: &'static str,
    /// The car's colour, if not its own.
    pub color: Option<u32>,
}

/// How a multiplayer race is set up (MULTIPLAYER.md, section 2).
#[derive(Clone, Debug, PartialEq)]
pub struct MultiOpts {
    pub seed: u32,
    /// The humans; player `i` of the race is `humans[i]`.
    pub humans: Vec<Human>,
    /// Indexes into `humans` in grid order (a permutation of them).
    pub grid: Vec<usize>,
    /// The field AI rivals fill to, humans included; `None` for every rival
    /// the level has. At most [`MAX_FIELD`] cars race.
    pub field: Option<usize>,
    pub rubber_band: bool,
    /// Humans pass through each other.
    pub ghost: bool,
}

/// A multiplayer race's own state.
#[derive(Clone, Debug, PartialEq)]
pub struct Multi {
    pub rubber_band: bool,
    pub ghost: bool,
    /// Seconds left for the others once the first human has finished.
    pub end_timer: Option<f64>,
    /// Seconds from the race's end to the results.
    pub end_delay: f64,
    pub reported: bool,
}

/// The most cars in a race (four rows of two).
pub const MAX_FIELD: usize = 8;
/// Seconds the others have to finish after the first human (MULTIPLAYER 2.8).
pub const FINISH_WINDOW: f64 = 45.0;
/// The players' names in the simulation; the client shows the names people
/// chose.
const PLAYER_NAMES: [&str; MAX_FIELD] = ["P1", "P2", "P3", "P4", "P5", "P6", "P7", "P8"];
/// Names for rivals beyond the level's own, when the field is filled.
const EXTRA_RIVALS: [&str; MAX_FIELD] = [
    "Nova", "Juno", "Rook", "Vega", "Echo", "Lynx", "Onyx", "Sable",
];

impl SimState {
    /// A race for several humans (MULTIPLAYER.md). With one human, every
    /// rival and the default rules it is exactly [`SimState::new`].
    pub fn new_multi(lr: &LevelRuntime, o: &MultiOpts) -> SimState {
        let nh = o.humans.len();
        assert!((1..=MAX_FIELD).contains(&nh), "1 to 8 humans");
        let mut sorted = o.grid.clone();
        sorted.sort_unstable();
        assert!(
            sorted.iter().copied().eq(0..nh),
            "the grid is an order of the humans"
        );
        let t = &*lr.track;
        let mut st = SimState::new(
            lr,
            RaceOpts {
                car: o.humans[0].car,
                seed: o.seed,
                pursuit: false,
                heat: 1.0,
            },
        );

        // The AI field.
        let want_ai = o
            .field
            .map_or(st.rivals.len(), |f| f.saturating_sub(nh))
            .min(MAX_FIELD - nh);
        st.rivals.truncate(want_ai);
        let defs = &lr.level.rivals;
        while st.rivals.len() < want_ai && !defs.is_empty() {
            let i = st.rivals.len();
            let r = &defs[i % defs.len()];
            let name = EXTRA_RIVALS[(i - defs.len()) % EXTRA_RIVALS.len()];
            let rv = Vehicle::new(dims(r.kind).expect("dims"), r.kind, 1400.0, name, r.color);
            let opts = AiOpts {
                skill: Some(r.skill),
                name,
                power: Some(r.power),
                bias: Some((if i % 2 == 1 { 1.0 } else { -1.0 }) * 0.6),
                line_factor: Some(0.8 + (i % 3) as f64 * 0.08),
            };
            let mut ai = AiDriver::new(rv, opts, &mut st.rng.ai);
            ai.color = r.color;
            st.rivals.push(ai);
        }

        // The humans.
        let n_cars = st.traffic.cars.len();
        st.players = o
            .humans
            .iter()
            .enumerate()
            .map(|(i, h)| {
                let mut spec = car_spec(h.car).expect("a car in CAR_SPECS");
                spec.color = h.color.unwrap_or(spec.color);
                let name = if nh == 1 { "You" } else { PLAYER_NAMES[i] };
                let v = Vehicle::new(
                    dims(h.car).expect("dims"),
                    h.car,
                    spec.mass,
                    name,
                    spec.color,
                );
                let mut phys = CarPhysics::new(&v, spec);
                phys.locked = true;
                PlayerCar {
                    v,
                    phys,
                    spec,
                    rules: PlayerRules::new(n_cars),
                }
            })
            .collect();

        // The grid: rows of two; humans from fourth place back among five
        // or more rivals, from the front otherwise (as the one player is).
        let nai = st.rivals.len();
        let humans = o.grid.iter().map(|&h| Err(h));
        let order: Vec<Result<usize, usize>> = if nai >= 5 {
            (0..3)
                .map(Ok)
                .chain(humans)
                .chain((3..nai).map(Ok))
                .collect()
        } else {
            humans.chain((0..nai).map(Ok)).collect()
        };
        for (k, c) in order.into_iter().enumerate() {
            let (row, col) = ((k / 2) as f64, k % 2);
            let s = t.start_s - 5.0 - row * 10.0 - col as f64 * 3.0;
            let lat = if col == 1 { 2.4 } else { -2.4 };
            match c {
                Err(h) => {
                    let p = &mut st.players[h];
                    p.phys.reset(&mut p.v, t, t.wrap(s), lat);
                    p.v.prog = Some(s);
                }
                Ok(i) => {
                    let a = &mut st.rivals[i];
                    a.k.s = t.wrap(s);
                    a.k.lat = lat;
                    a.k.speed = 0.0;
                    a.prog = Some(s);
                    a.write_pos(t);
                }
            }
        }
        st.race.prog_s = st
            .players
            .iter()
            .map(|p| p.v.s)
            .chain(st.rivals.iter().map(|a| a.k.s))
            .collect();
        st.race.multi = Some(Multi {
            rubber_band: o.rubber_band,
            ghost: o.ghost,
            end_timer: None,
            end_delay: 0.0,
            reported: false,
        });
        st
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
    /// A human's car (any player's).
    pub player: bool,
    /// Which player, for a human's car.
    pub human: Option<usize>,
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
    let mut list: Vec<Standing> = st
        .players
        .iter()
        .enumerate()
        .map(|(i, p)| Standing {
            player: true,
            human: Some(i),
            name: p.v.name,
            color: p.spec.color,
            s: p.v.s,
            prog: if laps { p.v.prog } else { Some(p.v.s) },
            finished: p.rules.finished,
            time: p.rules.finish_time,
        })
        .collect();
    for a in &st.rivals {
        list.push(Standing {
            player: false,
            human: None,
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
    pub human: Option<usize>,
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
                human: r.human,
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
fn crash(race: &RaceState, r: &mut PlayerRules, player: usize, events: &mut Vec<SimEvent>) {
    if !race.cruise || race.state == RaceStateKind::Countdown {
        return;
    }
    events.push(SimEvent::Crash { player });
    r.mult = 1.0;
    r.mult_timer = 0.0;
}

/// `bonus()`: nitro, and in a cruise the points (a chain raises the
/// multiplier).
#[allow(clippy::too_many_arguments)]
fn bonus(
    race: &RaceState,
    p: &mut PlayerCar,
    player: usize,
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
        player,
        text,
        nitro,
        points: scored,
    });
}

/// `resetPlayer()`: back on the road five metres back, on the tarmac.
fn reset_player(
    t: &Track,
    race: &RaceState,
    p: &mut PlayerCar,
    player: usize,
    events: &mut Vec<SimEvent>,
) {
    let s = if t.is_loop {
        t.wrap(p.v.s - 5.0)
    } else {
        js::max(t.start_s, p.v.s - 5.0)
    };
    crash(race, &mut p.rules, player, events);
    let f = t.frame(s);
    let lat = clamp(p.v.lat, -f.hw * 0.5, f.hw * 0.5);
    p.phys.reset(&mut p.v, t, s, lat);
    p.rules.reset_cooldown = 2.0;
    events.push(SimEvent::Reset { player });
}

/// One tick (`Race.update`, SPEC 4.3's tick order). `inputs[i]` is player
/// `i`'s (a missing one is no input). With one player this is the JS's
/// update exactly; with several, every per-player part of the tick runs for
/// each player in turn, in player order (MULTIPLAYER.md).
pub fn step(
    lr: &LevelRuntime,
    st: &mut SimState,
    inputs: &[InputFrame],
    events: &mut Vec<SimEvent>,
) {
    let t = &*lr.track;
    let dt = DT;
    st.tick += 1;
    let np = st.players.len();
    let frames: Vec<InputFrame> = (0..np)
        .map(|i| inputs.get(i).copied().unwrap_or_default())
        .collect();
    // A player who dropped out is driven by the autopilot (MULTIPLAYER 2.7).
    let inps: Vec<Input> = (0..np)
        .map(|i| {
            if frames[i].flags & AUTOPILOT != 0 {
                let mut inp = Input::default();
                autopilot(&mut inp, &st.players[i].v, t);
                InputFrame::quantise(&inp).input()
            } else {
                frames[i].input()
            }
        })
        .collect();
    let race = &mut st.race;

    // ── State machine ──────────────────────────────────────────
    if race.state == RaceStateKind::Countdown {
        race.countdown -= dt;
        // Perfect start = get on the throttle in the last moment before GO.
        // (Player 0's moment is the race's `throttle_at`, as in the JS.)
        for i in 0..np {
            let ta = if i == 0 {
                &mut race.throttle_at
            } else {
                &mut st.players[i].rules.throttle_at
            };
            if inps[i].throttle > 0.5 {
                if ta.is_none() {
                    *ta = Some(race.countdown);
                }
            } else {
                *ta = None;
            }
        }
        let n = race.countdown.ceil();
        if n < race.last_beep && n >= 1.0 {
            events.push(SimEvent::Countdown(n as i32));
            race.last_beep = n;
        }
        if race.countdown <= 0.0 {
            race.state = RaceStateKind::Racing;
            for p in st.players.iter_mut() {
                p.phys.locked = false;
            }
            events.push(SimEvent::Go);
            for i in 0..np {
                let ta = if i == 0 {
                    race.throttle_at
                } else {
                    st.players[i].rules.throttle_at
                };
                if ta.is_some_and(|ta| ta < 0.75) {
                    let p = &mut st.players[i];
                    let fx = kernel::cos(p.v.yaw);
                    let fz = kernel::sin(p.v.yaw);
                    p.v.vx += fx * 6.0;
                    p.v.vz += fz * 6.0;
                    events.push(SimEvent::PerfectStart { player: i });
                }
            }
        }
    }
    let started = race.state != RaceStateKind::Countdown;
    if started {
        race.time += dt;
    }

    // ── Players ────────────────────────────────────────────────
    for i in 0..np {
        let blocks_reset = st
            .pv
            .as_ref()
            .is_some_and(|pv| pv.held() || pv.pursuit.bust > 0.0);
        let p = &mut st.players[i];
        p.rules.reset_cooldown = js::max(0.0, p.rules.reset_cooldown - dt);
        if frames[i].flags & RESET != 0 && started && p.rules.reset_cooldown == 0.0 && !blocks_reset
        {
            reset_player(t, &st.race, p, i, events);
        }
    }
    // ── Agents list for AI/traffic awareness ───────────────────
    let mut agents = field(st).agents();
    for i in 0..np {
        // The pursuit holds player 0 (Hot Pursuit is single-player).
        let held = i == 0 && st.pv.as_ref().is_some_and(|pv| pv.held());
        let ctrl = if st.players[i].rules.finished {
            let views = field(st).views(&agents);
            cool_down(t, &mut st.players[i], i, &views, dt)
        } else if held {
            hold_controls(&mut st.players[i])
        } else {
            inps[i]
        };
        if st.players[i].rules.finished || held {
            events.push(SimEvent::Controls {
                player: i,
                throttle: ctrl.throttle,
            });
        }
        let p = &mut st.players[i];
        p.phys.update(&mut p.v, t, dt, &ctrl);
    }

    let laps = st.race.laps > 0;
    let player_s = st.players[0].v.s;
    let player_prog = if laps { st.players[0].v.prog } else { None };
    for i in 0..st.rivals.len() {
        let views = field(st).views(&agents);
        // Whom this rival rubber-bands against (MULTIPLAYER 2.1).
        let (ps, pp) = match &st.race.multi {
            None => (player_s, player_prog),
            Some(m) if !m.rubber_band => {
                // Its own position: a gap of zero, no banding.
                let a = &st.rivals[i];
                (a.k.s, if laps { a.prog } else { None })
            }
            Some(_) => {
                let k = nearest_human(st, i);
                let v = &st.players[k].v;
                (v.s, if laps { v.prog } else { None })
            }
        };
        let ctx = AiCtx {
            cars: &views,
            me: agents.iter().position(|&r| r == BodyId::Rival(i)),
            player_s: ps,
            player_prog: pp,
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
    if laps {
        track_progress(t, st);
    }
    let odos: Vec<f64> = (0..np)
        .map(|i| {
            let s = st.players[i].v.s;
            let r = &mut st.players[i].rules;
            let d_s = t.ds(r.last_s.unwrap_or(s), s);
            r.last_s = Some(s);
            r.dist += d_s.abs();
            let odo = r.odo.unwrap_or(s) + d_s; // unwrapped position (loops)
            r.odo = Some(odo);
            odo
        })
        .collect();
    {
        // Traffic lives ahead of the leading player and stays until the
        // last has passed (one player: both are player 0).
        let (lead, trail) = lead_and_trail(st);
        let lead_s = st.players[lead].v.s;
        let trail_s = st.players[trail].v.s;
        let list = field(st).traffic_agents(&agents);
        let dist = if t.is_loop { odos[lead] } else { lead_s };
        st.traffic.update(
            t,
            dt,
            lead_s,
            &list,
            0.0,
            dist,
            trail_s,
            &mut st.rng.traffic,
        );
    }
    if st.pv.is_some() {
        let list = field(st).pursuit_agents(&agents);
        let time = st.race.time;
        let pv = st.pv.as_mut().unwrap();
        pv.t += dt;
        let mut racers = RacerAccess {
            players: &mut st.players,
            rivals: &mut st.rivals,
        };
        pv.pursuit.update(
            t,
            dt,
            &list,
            &mut racers,
            time,
            started,
            &mut st.rng.pursuit,
        );
        // Units that joined this tick collide from now on.
        for b in field(st).agents() {
            if !agents.contains(&b) {
                agents.push(b);
            }
        }
    }

    // ── Collisions ─────────────────────────────────────────────
    let ghost = st.race.multi.as_ref().is_some_and(|m| m.ghost);
    let hits = field(st).collide_except(t, &agents, ghost);
    for a in &mut st.rivals {
        a.write_pos(t);
    }
    for c in st.traffic.cars.iter_mut().filter(|c| c.active) {
        c.k.write_pos(t);
    }
    if let Some(pv) = &mut st.pv {
        pv.pursuit.write_pos(t);
    }
    let human = |id: BodyId| match id {
        BodyId::Player(k) => Some(k),
        _ => None,
    };
    for h in hits {
        let (a, b) = (agents[h.a], agents[h.b]);
        if st.pv.is_some() {
            pursuit_on_hit(t, st, a, b, h.strength, events);
        }
        let (ha, hb) = (human(a), human(b));
        // `other`: the body that isn't the player, or `a` when no player is
        // in the hit (so only the earlier of two others can be flagged, as
        // in the JS).
        let other = if ha.is_some() { b } else { a };
        if let BodyId::Traffic(i) = other
            && h.strength > 0.15
        {
            let c = &mut st.traffic.cars[i];
            c.crashed = js::max(c.crashed, 0.01);
        }
        events.push(SimEvent::CarHit {
            hit: h,
            player: ha.or(hb),
        });
        for k in [ha, hb].into_iter().flatten() {
            let other_k = if Some(k) == ha { b } else { a };
            if let BodyId::Traffic(i) = other_k {
                st.players[k].rules.near_miss_hit[i] = true;
            }
            if h.strength > 0.2 {
                crash(&st.race, &mut st.players[k].rules, k, events);
            }
        }
    }
    for i in 0..np {
        let phys_events = std::mem::take(&mut st.players[i].phys.events);
        for e in phys_events {
            match e {
                PhysEvent::Impact { strength, .. } => {
                    if strength > 0.35 {
                        crash(&st.race, &mut st.players[i].rules, i, events);
                    }
                    // PursuitView.onWallImpact
                    if i == 0 && st.pv.is_some() && strength > 0.2 {
                        hurt(st, strength * DAMAGE_WALL, t);
                    }
                }
                PhysEvent::Land { air, .. } if air > 0.55 => {
                    events.push(SimEvent::Phys { player: i, e });
                    bonus(
                        &st.race,
                        &mut st.players[i],
                        i,
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
            events.push(SimEvent::Phys { player: i, e });
        }
    }

    // ── Bonuses: drift, near miss, overtakes ───────────────────
    for i in 0..np {
        {
            let p = &mut st.players[i];
            p.rules.bonus_cooldown = js::max(0.0, p.rules.bonus_cooldown - dt);
            if p.phys.drifting {
                p.rules.last_drift = p.phys.drift_time;
            } else if p.rules.last_drift > 1.2 {
                let ld = p.rules.last_drift;
                bonus(
                    &st.race,
                    p,
                    i,
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
            let v = &st.players[i].v;
            kernel::hypot(v.vx, v.vz)
        };
        for ci in 0..st.traffic.cars.len() {
            let c: &TrafficCar = &st.traffic.cars[ci];
            if !c.active {
                continue;
            }
            let p = &st.players[i];
            let ds = t.ds(p.v.s, c.k.s);
            let prev = p.rules.passed[ci];
            let (clat, chw, cdir, cspeed) = (c.k.lat, c.k.v.half_w, c.k.dir, c.k.speed);
            st.players[i].rules.passed[ci] = Some(ds);
            if let Some(prev) = prev
                && prev > 0.0
                && ds <= 0.0
            {
                let p = &st.players[i];
                let gap = (clat - p.v.lat).abs() - chw - p.v.half_w;
                let rel = if cdir == 1 {
                    psp - cspeed
                } else {
                    psp + cspeed
                };
                let lat = clat - p.v.lat;
                if !p.rules.near_miss_hit[ci] && gap < 1.4 && rel > 12.0 {
                    st.players[i].rules.near_misses += 1;
                    events.push(SimEvent::NearMiss {
                        player: i,
                        traffic: ci,
                        lat,
                        rel,
                    });
                    bonus(
                        &st.race,
                        &mut st.players[i],
                        i,
                        "NEAR MISS".to_string(),
                        0.08,
                        250.0,
                        true,
                        events,
                    );
                } else if rel > 20.0 && gap < 4.0 {
                    events.push(SimEvent::Whoosh {
                        player: i,
                        traffic: ci,
                        lat,
                        rel,
                    });
                }
            }
        }
    }

    // Wrong way / finish.
    for i in 0..np {
        let p = &mut st.players[i];
        let f = t.frame(p.v.s);
        let along = kernel::cos(p.v.yaw) * f.fx + kernel::sin(p.v.yaw) * f.fz;
        let spd = p.v.speed;
        if started && !p.rules.finished && along < -0.3 && spd > 4.0 {
            p.rules.wrong_way += dt;
        } else {
            p.rules.wrong_way = 0.0;
        }
        if p.rules.wrong_way > 1.5 {
            events.push(SimEvent::WrongWay { player: i });
        }
        // Stuck? Offer the reset key.
        p.rules.stuck = Some(if started && !p.rules.finished && spd < 1.5 {
            js::or_opt(p.rules.stuck, 0.0) + dt
        } else {
            0.0
        });
    }

    for i in 0..np {
        if st.race.cruise && started {
            let v = &st.players[i].v;
            let psp = kernel::hypot(v.vx, v.vz);
            cruise_score(&mut st.players[i].rules, dt, psp);
        }
        if laps && started && !st.players[i].rules.finished {
            lap_check(t, st, i, events);
        }
        let prog_now = if laps {
            st.players[i].v.prog.unwrap_or(f64::NAN)
        } else {
            st.players[i].v.s
        };
        if !st.race.cruise
            && !st.players[i].rules.finished
            && prog_now >= st.race.finish_prog
            && started
        {
            let time = st.race.time;
            {
                let r = &mut st.players[i].rules;
                r.finished = true;
                r.finish_time = Some(time);
                if laps {
                    let lt = time - r.lap_start;
                    r.lap_times.push(lt);
                }
            }
            let (s, lat) = (st.players[i].v.s, st.players[i].v.lat);
            let park = park_spot(t, &mut st.race, s, lat);
            st.players[i].rules.park = Some(park);
            let place = standings(st)
                .iter()
                .position(|r| r.human == Some(i))
                .unwrap()
                + 1;
            events.push(SimEvent::Finished { player: i, place });
            match &mut st.race.multi {
                None => {
                    st.race.state = RaceStateKind::Finished;
                    st.players[i].rules.finish_delay = 3.2;
                }
                // The first human home starts the others' countdown.
                Some(m) => {
                    if m.end_timer.is_none() {
                        m.end_timer = Some(FINISH_WINDOW);
                    }
                }
            }
        }
    }
    let racing = st.race.state == RaceStateKind::Racing;
    let any_home = st.players.iter().any(|p| p.rules.finished);
    let all_home = st
        .players
        .iter()
        .zip(&frames)
        .all(|(p, f)| p.rules.finished || f.flags & AUTOPILOT != 0);
    if let Some(m) = &mut st.race.multi
        && racing
    {
        // The race ends when every player still driving has finished, or
        // when the countdown runs out (MULTIPLAYER 2.8).
        if let Some(e) = &mut m.end_timer {
            *e -= dt;
        }
        if (any_home && all_home) || m.end_timer.is_some_and(|e| e <= 0.0) {
            st.race.state = RaceStateKind::Finished;
            m.end_delay = 3.2;
        }
    }
    if st.race.state == RaceStateKind::Finished {
        match &mut st.race.multi {
            None => {
                let r = &mut st.players[0].rules;
                r.finish_delay -= dt;
                if r.finish_delay <= 0.0 && !r.reported {
                    r.reported = true;
                    events.push(SimEvent::Results);
                }
            }
            Some(m) => {
                m.end_delay -= dt;
                if m.end_delay <= 0.0 && !m.reported {
                    m.reported = true;
                    events.push(SimEvent::Results);
                }
            }
        }
    }

    // PursuitView.sync → events: the rule effects of the pursuit's events.
    if st.pv.is_some() {
        pursuit_events(t, st, events);
    }
}

/// Multiplayer: the human nearest rival `i` in race progress (on a circuit
/// `prog`, elsewhere s), the first of equals (MULTIPLAYER 2.1).
fn nearest_human(st: &SimState, i: usize) -> usize {
    let a = &st.rivals[i];
    let laps = st.race.laps > 0;
    let mut best = (0, f64::INFINITY);
    for (k, p) in st.players.iter().enumerate() {
        let gap = if laps {
            a.prog.unwrap_or(f64::NAN) - p.v.prog.unwrap_or(f64::NAN)
        } else {
            a.k.s - p.v.s
        };
        if gap.abs() < best.1 {
            best = (k, gap.abs());
        }
    }
    best.0
}

/// The players furthest ahead and furthest behind in race progress (the
/// first of equals); player 0 twice with one player.
fn lead_and_trail(st: &SimState) -> (usize, usize) {
    let laps = st.race.laps > 0;
    let prog = |p: &PlayerCar| {
        if laps {
            p.v.prog.unwrap_or(f64::NAN)
        } else {
            p.v.s
        }
    };
    let (mut lead, mut trail) = (0, 0);
    for k in 1..st.players.len() {
        let x = prog(&st.players[k]);
        if x > prog(&st.players[lead]) {
            lead = k;
        }
        if x < prog(&st.players[trail]) {
            trail = k;
        }
    }
    (lead, trail)
}

/// The pools of a state, borrowed apart.
fn field(st: &mut SimState) -> Field<'_> {
    Field {
        players: &mut st.players,
        rivals: &mut st.rivals,
        traffic: Some(&mut st.traffic),
        pursuit: st.pv.as_mut().map(|pv| &mut pv.pursuit),
    }
}

/// While held: brake to a stop, then sit there (`holdControls`).
fn hold_controls(p: &mut PlayerCar) -> Input {
    let sp = kernel::hypot(p.v.vx, p.v.vz);
    p.phys.locked = sp < 0.8;
    Input {
        brake: if sp > 0.8 { 1.0 } else { 0.0 },
        ..Input::default()
    }
}

/// Player damage (0..1; a wreck at 1): per unit of hit strength, scaled by
/// the other car's mass, and per wall impact.
const DAMAGE_CAR: f64 = 0.11;
const DAMAGE_WALL: f64 = 0.1;

/// PursuitView.hurt.
fn hurt(st: &mut SimState, d: f64, t: &Track) {
    let pv = st.pv.as_mut().unwrap();
    if pv.held() || st.players[0].rules.finished || st.race.state == RaceStateKind::Countdown {
        return;
    }
    pv.damage = js::min(1.0, pv.damage + d);
    st.players[0].phys.damage = pv.damage;
    if pv.damage >= 1.0 {
        pv.wrecks += 1;
        let p = pv.pursuit.player.expect("the player races");
        let mut racers = RacerAccess {
            players: &mut st.players,
            rivals: &mut st.rivals,
        };
        pv.pursuit
            .arrest(t, &mut racers, p, HoldReason::Wrecked, WRECK_PENALTY);
    }
}

/// `PursuitView.hurt(d)` called from outside a tick (the client's test
/// bridge, `race.pv.hurt(x)` in the e2e suites); nothing without a pursuit.
pub fn hurt_player(lr: &LevelRuntime, st: &mut SimState, d: f64) {
    if st.pv.is_some() {
        hurt(st, d, &lr.track);
    }
}

/// PursuitView.onHit: the pursuit's hit rules (unit health, PIT push), then
/// damage to the player's car.
fn pursuit_on_hit(
    t: &Track,
    st: &mut SimState,
    a: BodyId,
    b: BodyId,
    strength: f64,
    _events: &mut Vec<SimEvent>,
) {
    let pb = BodyId::Player(0);
    let (va, vb) = {
        let f = field(st);
        (f.velocity(a), f.velocity(b))
    };
    let plat = st.players[0].v.lat;
    let pit = {
        let pv = st.pv.as_mut().unwrap();
        let mut racers = RacerAccess {
            players: &mut st.players,
            rivals: &mut st.rivals,
        };
        pv.pursuit.on_hit(
            t,
            &mut racers,
            a,
            b,
            strength,
            va,
            vb,
            plat,
            &mut st.rng.pursuit,
        )
    };
    if pit != 0.0 {
        st.players[0].v.yaw_rate += pit * 2.2;
    }
    if a != pb && b != pb {
        return;
    }
    let other = if a == pb { b } else { a };
    if matches!(other, BodyId::Sawhorse(_)) {
        return; // barriers: no damage
    }
    // Police rams are braced, glancing shoves, and rubbing with rivals is
    // racing: both wear the car down more slowly than a crash into traffic.
    // One shunt is one hit: a car can hurt you at most twice a second, not
    // every frame the two stay in contact.
    let pv = st.pv.as_mut().unwrap();
    let slot = pv.last_hit.slot(other);
    let last = slot.unwrap_or(-1.0);
    if strength > 0.1 && pv.t - last > 0.5 {
        pv.last_hit.set(other, pv.t);
    } else {
        return;
    }
    let mass = field(st).mass(other);
    let pv = st.pv.as_ref().unwrap();
    let k = match other {
        BodyId::Police(i) => {
            if pv.pursuit.police(i).mode != PoliceMode::Block {
                0.7
            } else {
                1.0
            }
        }
        BodyId::Traffic(_) => 1.0,
        _ => 0.5,
    };
    hurt(
        st,
        strength * DAMAGE_CAR * clamp(mass / 1500.0, 0.5, 2.0) * k,
        t,
    );
}

/// PursuitView.events: what the pursuit's events do to the race (a release
/// back onto the road, the barrier's slowdown, a bust's crash), and the
/// events the client shows.
fn pursuit_events(t: &Track, st: &mut SimState, events: &mut Vec<SimEvent>) {
    let pv_events = std::mem::take(&mut st.pv.as_mut().unwrap().pursuit.events);
    for e in pv_events {
        match &e {
            PursuitEvent::Barrier { player: true, .. } => {
                let v = &mut st.players[0].v;
                v.vx *= 0.97;
                v.vz *= 0.97;
            }
            PursuitEvent::Busted { player: true, .. } => {
                crash(&st.race, &mut st.players[0].rules, 0, events)
            }
            PursuitEvent::Release {
                racer,
                player: true,
                spot: Some((s, lat)),
            } => {
                // The penalty is served: back on the road ahead of the police,
                // repaired if it was wrecked.
                let pv = st.pv.as_mut().unwrap();
                let r = &pv.pursuit.racers[*racer];
                pv.penalty += r.hold_total;
                let wrecked = r.hold_reason == Some(HoldReason::Wrecked);
                let p = &mut st.players[0];
                p.phys.locked = false;
                p.phys.reset(&mut p.v, t, *s, *lat);
                if wrecked {
                    pv.damage = 0.0;
                    p.phys.damage = 0.0;
                }
                p.phys.spiked = 0.0;
            }
            _ => {}
        }
        events.push(SimEvent::Pursuit(e));
    }
}

/// Hot Pursuit inside a race (the simulation half of `PursuitView`): the
/// pursuit, and the player's damage, wrecks and penalties.
#[derive(Clone, Debug, PartialEq)]
pub struct PursuitView {
    pub pursuit: Pursuit,
    pub damage: f64,
    pub wrecks: i32,
    /// Seconds served.
    pub penalty: f64,
    pub t: f64,
    pub last_hit: LastHit,
}

impl PursuitView {
    pub fn held(&self) -> bool {
        self.pursuit
            .player
            .is_some_and(|p| self.pursuit.racers[p].hold > 0.0)
    }
}

impl LastHit {
    fn slot(&self, id: BodyId) -> Option<f64> {
        match id {
            BodyId::Rival(i) => self.rivals[i],
            BodyId::Traffic(i) => self.traffic[i],
            BodyId::Police(i) => self.police[i],
            BodyId::Sawhorse(i) => self.sawhorses[i],
            _ => None,
        }
    }

    fn set(&mut self, id: BodyId, v: f64) {
        let slot = match id {
            BodyId::Rival(i) => &mut self.rivals[i],
            BodyId::Traffic(i) => &mut self.traffic[i],
            BodyId::Police(i) => &mut self.police[i],
            BodyId::Sawhorse(i) => &mut self.sawhorses[i],
            _ => return,
        };
        *slot = Some(v);
    }
}

/// The pursuit's summary for the results (`pv.stats()`).
#[derive(Clone, Debug, PartialEq)]
pub struct PursuitStats {
    pub busts: i32,
    pub wrecks: i32,
    pub takedowns: i32,
    pub penalty: f64,
    pub heat: i32,
}

pub fn pursuit_stats(st: &SimState) -> Option<PursuitStats> {
    st.pv.as_ref().map(|pv| PursuitStats {
        busts: pv.pursuit.busts,
        wrecks: pv.wrecks,
        takedowns: pv.pursuit.takedowns,
        penalty: pv.penalty,
        heat: pv.pursuit.max_heat,
    })
}

/// `x.toFixed(1)` for the bonus texts.
fn to_fixed1(x: f64) -> String {
    format!("{:.1}", js::round(x * 10.0) / 10.0)
}

/// Circuits: carry each racer's unwrapped progress on by how far it moved
/// along the loop this tick (a reset back down the road counts too).
fn track_progress(t: &Track, st: &mut SimState) {
    let np = st.players.len();
    for (i, p) in st.players.iter_mut().enumerate() {
        let last = st.race.prog_s[i];
        p.v.prog = Some(p.v.prog.unwrap_or(f64::NAN) + t.ds(last, p.v.s));
        st.race.prog_s[i] = p.v.s;
    }
    for (i, a) in st.rivals.iter_mut().enumerate() {
        let last = st.race.prog_s[np + i];
        a.prog = Some(a.prog.unwrap_or(f64::NAN) + t.ds(last, a.k.s));
        st.race.prog_s[np + i] = a.k.s;
    }
    for a in &mut st.rivals {
        if !a.finished && a.prog.unwrap_or(f64::NAN) >= st.race.finish_prog {
            a.finished = true;
            a.finish_time = Some(st.race.time);
        }
    }
}

/// A player's lap: announce each new one and keep the lap times.
fn lap_check(t: &Track, st: &mut SimState, player: usize, events: &mut Vec<SimEvent>) {
    let p = &mut st.players[player];
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
        player,
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

/// A 64-bit hash of the state, for determinism and desync checks (SPEC 4.3).
/// It is the FNV-1a 64 of the trace record without inputs, which covers
/// every field that affects later ticks.
pub fn hash(st: &SimState) -> u64 {
    crate::trace::fnv1a64(&crate::trace::race_record(st, &[]))
}
