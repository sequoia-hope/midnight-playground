# Midnight Racer in Rust: port specification

Status: proposed, 2026-10-03. Companions: [ROADMAP.md](ROADMAP.md)
(milestones, work packages, gates) and three inventories of the JS game with
file and line references, to port from:
[rendering and world generation](inventory-rendering.md),
[audio](inventory-audio.md),
[UI, input, platform and tests](inventory-ui-input-tests.md).

This document says what the Rust version is, how it is structured, and how we
know each part is finished. It is written to be executed by an agent that has
not seen the conversation that produced it. Where it describes the JavaScript
game it cites the file to read; the JS source is the authority on behaviour,
this document is the authority on structure.

## 1. Goals

The game is being rewritten in Rust so it can grow into a racing suite: more
modes (pursuit, combat, kart-style), open-world areas beside the current
routes, LAN multiplayer between browser tabs, more realistic physics, and
reinforcement-learning agents trained on the simulation.

The port comes first. Its goal is the game as it is today, with nothing
missing and nothing redesigned.

| Requirement | Decision (owner, 2026-10-03) |
|---|---|
| Platforms | Desktop browsers, iPhone Safari, Android browsers, and a native desktop app. All four must run well. |
| Web renderer | **WebGPU is required** (owner, revising an earlier WebGL2-baseline answer). That means iPhone 11 or newer on iOS 26+, Android 12+ with current Chrome, and desktop Chrome, Edge and Safari. Firefox works only where it ships WebGPU (Windows, Apple Silicon Macs). A browser without WebGPU gets a page that says so. |
| Look | Match the three.js game as closely as possible, judged side by side. |
| Content | Generated in code for the port. The architecture includes a normal asset pipeline so later content can be authored. |
| Multiplayer | A native host program first. The netcode is designed so a browser tab can be the host later. |
| First multiplayer release | Two to eight humans in the existing race levels, AI filling the grid, traffic on. Hot Pursuit and the cruise loop stay single-player until later. |
| Repository | This repo. The JS game is frozen except for bug fixes. GitHub Pages keeps serving it, unchanged, until the port is done; the Rust build is not published there before cutover. |

Not goals of the port: new gameplay, retuned handling, authored art, or the
open-world design. Those come after cutover and each
gets its own design document.

### 1.1 Principles

These are the rules that resolve most questions an implementer will have.

1. **The JS game is the oracle.** Parity is measured against it by tests,
   numeric traces, geometry digests and side-by-side pictures. A port that
   "looks about right" is not done.
2. **Port, don't improve.** Keep the same structure, constants, order of
   operations and order of random draws as the JS. Keep the comments that
   explain why. A change in behaviour is a *deviation*: it needs a reason and
   an entry in `docs/rust-port/DEVIATIONS.md` (section 13 seeds the list).
3. **The simulation is a library.** It has no engine, no renderer, no clock
   and no global state. It steps a plain data structure by one fixed tick.
   Multiplayer, replays, tests and RL all depend on this.
4. **World generation is a library.** It turns a level into engine-neutral
   scene data. The renderer can be replaced without touching it.
5. **One implementation for every platform.** Platform differences live
   behind small traits (storage, audio backend, gestures, rumble).
6. **Deterministic by construction.** Fixed tick, seeded random streams held
   in the state, software math functions. The same inputs give the same
   result on native and in wasm.

## 2. Engine decision

**Use Bevy as the client framework, with our own shading model inside it, and
keep the simulation, world generation and audio graph independent of it.**

Three options were weighed.

| Option | For | Against |
|---|---|---|
| **Bevy** (chosen) | Windowing, input, ECS, UI, asset pipeline, native and web from one codebase. On WebGPU the web build has the same renderer features as native. Custom materials and post-processing are supported. Room to grow: glTF, physics crates, tooling. | Wasm is large (25 to 35 MB raw, 6.5 to 8.5 MB compressed in recent third-party builds). No published evidence of a Bevy 3D game running well on iPhone Safari. About a hundred migration items per release, every three to five months. No wasm threads. |
| Custom engine on wgpu and winit | Smallest wasm, full control of pipeline warm-up, a renderer that is a direct port of the three.js subset we use. | Everything else is ours to build and keep: scene management, culling, UI, text, input, gamepads, asset loading. That is the wrong place to spend effort on a project meant to keep expanding. |
| Keep three.js for rendering, Rust for the simulation only | Lowest risk for the phone. | No native renderer, two languages forever, and not the rewrite that was asked for. |

Why Bevy is workable here:

- With WebGPU required, the web and native builds share one render path and
  later work (open world, more lights, cascaded shadows, GPU-driven
  culling) is not held back by WebGL2.
- The port's own content is modest: one shadow-casting directional light, a
  hemisphere light and at most three small local lights. It fits inside
  Bevy's WebGL2 limits too (one directional light, one shadow cascade, no
  compute), which is what makes a WebGL2 build a usable contingency.
- Matching the three.js look by tuning Bevy's `StandardMaterial` would be
  guesswork, because the lighting models differ. Instead the port brings its
  own shading: custom Bevy materials whose WGSL reproduces the parts of
  three.js r180's physical material that the game uses, fed by our own
  light, fog and exposure uniforms (section 6). Bevy supplies meshes, views,
  culling, batching and the shadow map.
- The simulation (about 3,300 lines of JS), world generation (about 26,000)
  and audio (about 3,500) sit in crates that do not depend on Bevy. If Bevy
  fails on phones, the client crate is replaced and those crates are kept.

**Gate G1** (roadmap M2) tests the risk early: a real level, exported from
the JS game as a scene file, rendered by the Bevy client through WebGPU on
the owner's iPhone and an Android phone. Safari is where the open WebGPU
bugs are (Appendix A), so this gate matters most there. The fallbacks, in
order:

1. A WebGL2 build of the same client for the phones where WebGPU fails.
   Bevy selects the backend at compile time, so this is a second wasm file
   chosen by the page at load. CI keeps it compiling until cutover, and the
   port's shaders avoid WebGPU-only features until G1 has passed on Safari.
2. If Bevy itself misses its frame-time, memory, size or load-time budgets
   and cannot be fixed: a small forward renderer on wgpu that consumes the
   same scene data.

**Version policy.** Bevy 0.19.1 is current; 0.20 is at its second release
candidate and replaces Bevy's shader import dialect with WESL. Start the
client on 0.20 if it is final when M2 begins, otherwise on 0.19.1 and migrate
straight after G1, before many shaders exist. After that, pin the version and
upgrade only between milestones. Use no third-party Bevy plugins unless a
decision record justifies one: each is a reason an upgrade stalls.

## 3. Architecture

### 3.1 Workspace

```
Cargo.toml                 workspace root (repo root)
crates/
  mr_math/       scalar math on libm, mulberry32, hash2, simplex, fbm, ridged
  mr_track/      level definition types, Track (1 m samples), road types
  mr_levels/     the six levels as data; Seaside's survey data loader
  mr_sim/        vehicles, physics, AI, traffic, collisions, race rules,
                 pursuit; SimState and step()
  mr_scene/      scene data types and the .mrscene file format; data only
  mr_canvas/     the Canvas 2D subset the texture generators use (tiny-skia)
  mr_worldgen/   terrain, road, sky, sea, scenery, car models, textures;
                 produces a mr_scene::Scene plus animators
  mr_audio/      Web Audio shaped facade and backends; engine, SFX, music,
                 radio
  mr_net/        protocol, transports, session (server), prediction and
                 rollback (client)
  mr_game/       the Bevy client: states, rendering adapter and shaders,
                 camera, effects, HUD and menus, input, platform glue
  mr_host/       native host binary: static files, WebSocket, authority
  mr_py/         (later) PyO3 bindings: a Gymnasium environment
xtask/           build, parity and size tooling (cargo xtask ...)
assets/          fonts, Seaside survey data, radio clips
tools/parity/    reference capture from the JS game (Node, headless Chrome)
legacy/          the JS game, moved here at cutover (until then it stays at
                 the repo root, untouched)
docs/rust-port/  this spec, the roadmap, DEVIATIONS.md, DECISIONS.md
```

### 3.2 Dependency rules

```
mr_math ← mr_track ← mr_levels ← mr_sim ← mr_net ← mr_host

mr_scene ← mr_worldgen   (also uses mr_canvas, mr_math, mr_track, mr_levels)

mr_game uses mr_sim, mr_net, mr_scene, mr_worldgen, mr_audio
```

- `mr_math`, `mr_track`, `mr_levels`, `mr_sim`: no Bevy, wgpu, web-sys,
  `std::time`, threads, `rand`, or hash-map iteration. `#![forbid(unsafe_code)]`.
  They build for `wasm32-unknown-unknown` and native.
- `mr_scene`: plain data and its reader and writer. No Bevy, no wgpu. Both
  `mr_worldgen` (which produces scenes) and `mr_game` (which draws them)
  depend on it, so the client can load an exported scene before any world
  generation exists.
- `mr_canvas`, `mr_worldgen`: no Bevy, no wgpu. `rayon` is allowed behind a
  native-only feature.
- `mr_audio`: no Bevy. Backends are features (`web`, `native`, `null`).
- `mr_net`: no Bevy. Transports are features.
- Only `mr_game` depends on Bevy.

`cargo xtask check-deps` enforces this from `cargo tree` and runs in CI.

### 3.3 One frame

```
devices ─► input layer ─► InputFrame (quantised)
                               │
              session.advance(frame_dt, input)      mr_net (loopback in
                               │                    single-player)
                 0..n fixed ticks: mr_sim::step
                               │
              prev state, curr state, alpha, events
                               │
        ┌──────────────┬───────┴───────┬──────────────┐
   transforms      camera, FX      audio.update     HUD
   (interpolated)                  + event calls
```

The client never reads simulation state mid-tick and never writes it. It
draws an interpolation between the last two ticks and reacts to the events
those ticks produced.

## 4. Simulation core (`mr_math`, `mr_track`, `mr_levels`, `mr_sim`)

### 4.1 Time

The JS game steps player physics in equal substeps of about 1/120 s that sum
to the frame time (`CarPhysics.update`), and steps AI, traffic, pursuit,
collisions and the input ramp once per frame. The Rust simulation uses a
**fixed tick of 1/120 s** for everything:

- It is close to what the JS does at 60 fps (two physics substeps of half
  the frame time), and it is exactly what the JS does when run at a fixed
  frame time of 1/120 s: one substep, everything else once. That fixed-dt
  JS run is the reference the Rust is compared with (section 4.6), not the
  JS at 60 fps.
- The renderer interpolates between the previous and current tick, so motion
  is smooth at any display rate and input latency stays under 9 ms. (The JS
  project learned that a fixed step without interpolation lurches at speed.
  Interpolation is not optional.)
- A frame runs at most six ticks (1/20 s, the JS clamp). Beyond that
  single-player slows down; a multiplayer client resynchronises from a
  snapshot.
- The whole simulation costs microseconds per tick, so 120 Hz is free, and a
  rollback of sixteen ticks stays well under a millisecond.

Only `step()` fixes the tick. The module functions underneath keep their
`dt` parameters as in the JS (`CarPhysics.update(frameDt)` with its substep
rule included), because the JS unit tests call them at 1/60 s and at varying
frame times, and those tests are ported as they are.

### 4.2 Numeric rules

The aim is **bit-identical results** between the Rust simulation and the JS
reference run, tick for tick. With thirty bodies and many thresholds (gear
changes, the drifting flag, overtaking decisions, spawn checks, contact), a
trace that is merely close stops being close within seconds, so a tolerance
does not work. Bit-identical is achievable because `+ - * /` and `sqrt` on
doubles are exact in both languages; the rest is discipline:

- **One math kernel for both sides.** `mr_math` implements every `Math`
  function that is not exact by definition, on the `libm` crate (software
  implementations, the same bits on every platform): `sin`, `cos`, `tan`,
  `asin`, `acos`, `atan`, `atan2`, `exp`, `log`, `log2`, `pow`, `tanh` and
  `hypot`. That is the full set the game and three.js use. `hypot` is
  defined by the kernel for any argument count, since the JS calls it with
  one, two and three: `abs` for one, `libm`'s for two, and the square root
  of the left-to-right sum of squares for more. The simulation and world
  generation never call `f64::sin` and friends, which use the platform's
  library on native. For the reference run, `mr_math` is compiled to wasm
  and the JS oracle replaces those `Math` functions with it; any other
  inexact `Math` function is replaced by one that throws, so nothing slips
  through unnoticed (roadmap WP 0.3). The functions that are exact (`sqrt`,
  `floor`, `ceil`, `round`, `trunc`, `abs`, `min`, `max`, `sign`, `imul`,
  `fround`) are left alone. The reference therefore differs from the live
  JS game by at most the last bit of the kernel functions, which is the
  difference the port has anyway, and the Rust can be required to match it
  exactly. No attempt is made to reproduce V8's own math library. Every
  dump and golden that Rust is compared with (traces, Track arrays, terrain
  heights, world data, scene exports) is taken with the kernel on.
- **JS semantics, in `mr_math::js`.** Port each use through a helper that
  behaves as JavaScript does:
  - `Math.sign(0)` is 0 and `Math.sign(-0)` is -0, where `f64::signum`
    gives ±1. This decides the handbrake and coasting terms at a standstill
    (`CarPhysics.js:208,217`).
  - `Math.round` is "floor of x + 0.5" as the language defines it, with its
    edge cases: 0.49999999999999994 rounds to 0, and values in (-0.5, 0)
    round to -0. Neither Rust's `round` nor a naive `floor(x + 0.5)` is the
    same.
  - `Math.max` and `Math.min` return NaN if either argument is NaN, and
    order the zeros: `Math.max(-0, 0)` is +0. Rust's `f64::max` does neither
    reliably, and the sign of a zero reaches `atan2` (`AIDriver.js:118`,
    `Kinematic.js:77-81`).
  - `x || d` treats 0 as missing; `x ?? d` does not.
  - `Array.prototype.sort` is stable. Use `sort_by`, never
    `sort_unstable_by` (`Race.js:140,515`, `Pursuit.js:551,553,669`).
- **The same float widths at the same points.** Scalars are `f64`. Wherever
  the JS stores into a `Float32Array`, round to `f32` at that point and read
  back as `f64`: all of Track's per-sample arrays and its smoothing weights,
  the collision circles (`Collisions.js:21`), Pursuit's cumulative turn
  (`Pursuit.js:104`). Search each ported file for typed arrays.
- **The same order.** Keep the JS order of arithmetic within an expression
  (no reassociation, no `mul_add`). Where the JS iterates an object or a
  Map, iterate in the JS insertion order, with an array or a vector of
  pairs: `Object.entries(per)` at `Traffic.js:62` fixes both the colour
  draws and the order of cars in collisions, and `POOL` at `Pursuit.js:110`
  does the same for police units. A sorted map would silently change both.
- **Integer hashing.** Port `mulberry32` and `hash2` with wrapping 32-bit
  arithmetic so they are bit-exact.
- **Random draws.** See section 4.3.

### 4.3 State and API

```rust
/// Built once per level, shared, immutable. Track arrays, level data,
/// survey grids (Seaside), racing line, speed profile.
pub struct LevelRuntime { /* ... */ }

/// Everything that changes. Plain data: Clone, PartialEq, serde.
/// Target size under 32 KB so a ring of 64 past states is cheap.
pub struct SimState {
    pub tick: u32,
    pub race: RaceState,            // countdown, clock, laps, standings, score
    pub players: Vec<PlayerCar>,    // one in single-player
    pub rivals: Vec<Rival>,
    pub traffic: TrafficState,
    pub pursuit: Option<PursuitState>,
    pub rng: RngStreams,
}

/// One player's controls for one tick, already quantised, so the local
/// simulation, the network and a replay all see identical values.
pub struct InputFrame {
    pub steer: i16,      // -32767..32767
    pub throttle: u8,
    pub brake: u8,
    pub flags: u8,       // handbrake, nitro, analog, reset (edge)
}

pub fn step(level: &LevelRuntime, state: &mut SimState,
            inputs: &[InputFrame], events: &mut Vec<SimEvent>);

pub fn hash(state: &SimState) -> u64;   // for determinism and desync checks
```

`players` is a vector from the first commit. Single-player is the one-player
case of the same code, so multiplayer does not need a second pass through the
simulation. State that JS keeps on `Race` for "the player" (lap, lap times,
score, multiplier, wrong-way and stuck timers, parking target, pass timer,
reset cooldown, damage, penalties) is per player, in `PlayerCar`.

**Plain data, no closures, no identity.** The JS keeps some state in forms
that cannot be cloned or hashed. Each gets a data form:

| JS | Rust |
|---|---|
| Parking targets as closures (`Race.js:476,490`) | An enum: circuit cool-down lap, or a lane and stopping point |
| Height overrides as closures (`car.yFn`, `Traffic.js:151`; sawhorses) | An enum naming the surface: road, opposite carriageway, fixed |
| `WeakMap`s and `Map`s keyed by car object (`Race.js:99,115-116`, `PursuitView.js:55`) | Fields on the car, or arrays indexed by the car's pool index |
| A car's identity | Its index in its pool. The agent list is rebuilt each tick in the JS order (`Race.js:216`, then pursuit bodies appended at `:234`), because that order is the collision order |

**Random streams.** In the live game the simulation and the presentation
share `Math.random`: rival nitro timers (`AIDriver.js:21,110`), the police
weave and callsigns (`PoliceDriver.js:60`, `Pursuit.js:110-116`, where
`PursuitView` builds `Pursuit` with no generator of its own), and also
sparks, smoke, audio and scenery animators. That sharing cannot be
reproduced and is not kept. The Rust state holds named `Mulberry32` streams:
`ai`, `police` (weave), `pursuit`, and `traffic` (which is already its own
seeded stream in JS, seed 99, and stays so). The other three are seeded from
the race seed: stream k (ai 0, police 1, pursuit 2) starts at
`(seed + 0x9E3779B9 * (k + 1))` as an unsigned 32-bit value. Presentation
draws never touch them. The JS gains optional generator parameters at those
call sites, defaulting to `Math.random`, so the reference run can be given
the same streams (roadmap WP 0.3). `PoliceDriver` is constructed inside
`Pursuit` (`Pursuit.js:111,119`), so the police generator is passed through
`PursuitView` and `Pursuit` to reach it.

**Tick order.** `step` reproduces `Race.update` (`src/game/Race.js:182-416`).
The order below is the contract; every item changes results if moved.

1. Countdown and perfect start; race clock.
2. Reset request (blocked while being busted or held).
3. Build the agent list: players, rivals, active traffic, pursuit bodies.
4. Player control: the cool-down driver after the finish, the penalty hold
   (`PursuitView.holdControls`, which also sets `locked`), or the input.
5. Player physics.
6. Rivals; parking targets for those that finished (before the player's
   own, since they share the parking rows).
7. Circuit progress (`trackProgress`).
8. Distance and odometer (`Race.js:226-229`); the odometer feeds traffic on
   loops.
9. Traffic.
10. Pursuit update (`Pursuit.update`); bodies that joined this tick are
    appended to the agent list.
11. Collisions; then `writePos` for rivals, traffic and pursuit bodies.
12. Per hit, in order (`Race.js:243-256`):
    - pursuit hit rules (`PursuitView.onHit`: unit health, PIT push, damage,
      wreck arrest);
    - the crash flag: with `other` being the body that is not the player,
      or `h.a` when the player is not in the hit, a body that has a
      `crashed` field is flagged if the hit is stronger than 0.15. So in a
      hit between two non-player bodies only the one earlier in agent order
      can be flagged. Reproduce this as it is;
    - if the player is in the hit: mark that car as hit, which cancels its
      near-miss bonus in step 14 (`nearMiss.set(other, 'hit')`), then the
      cruise crash if the hit is stronger than 0.2.
13. Physics events: wall impact (cruise crash, pursuit wall damage), shift,
    landing (air bonus).
14. Bonuses: drift, near miss.
15. Wrong-way and stuck timers.
16. Cruise score, then lap check, then finish detection, then the finish
    delay and results.
17. Pursuit rule effects that the JS applies from `PursuitView.events`,
    which runs inside `pv.sync` (`PursuitView.js:258`), after everything
    above: release from a hold (`phys.reset`, unlock, repair, clear spiked
    tyres), the barrier slowdown, the busted crash. They belong to the
    simulation and stay at this point in the tick.

**Events, not side effects.** JS `Race` calls the HUD, audio, rumble, camera
and effects directly. The Rust simulation emits `SimEvent`s (`Countdown`,
`Go`, `PerfectStart`, `Shift`, `Land`, `WallImpact`, `CarHit`, `NearMiss`,
`Bonus`, `Lap`, `Finished`, pursuit events, ...) and the client turns them
into text, sound, rumble and sparks.

**What is simulation and what is not.** Everything that affects the race is
simulation: all of the list above. Camera, effects, audio mapping, rumble
and HUD text stay in the client. Two fields sit on the line:

- `visualYaw` (a hit car's spin) looks visual but is part of the collision
  shape (`Collisions.js:11`). It is simulation state.
- Body pitch and roll are visual, but physics kicks the pitch spring on
  landing (`CarPhysics.js:297`). In Rust the springs live in the client and
  the kick travels in the `Land` event.

**Data the world gives the simulation.** Two things the simulation needs are
computed by scenery code in JS, not by the level files:

- `Track.runout`, the drivable road past the last sample: 900 m on Sierra
  (`City.js:48`), 700 m on Coast (`Harbor.js:57`), 0 on Streets. It moves
  the road-end wall, parking, traffic stopping and the kinematic clamp.
- `world.oppositeCarriageway` (`City.js:222`, `Harbor.js:179`): the range
  (`s0`, `s1`), the lane offsets (`OPP_LANES`, `city/freeway.js:26`) and the
  height of the far carriageway. Its cars draw from the traffic stream and
  take part in AI awareness.

`LevelRuntime` carries both. Until world generation is ported, the numbers
(`runout`, `s0`, `s1`, the lanes) are dumped from the JS world (roadmap
WP 0.4) into `mr_levels`; when the scenery that computes them is ported
(M3, M7), a test checks it reproduces the same values. The carriageway's
height is not dumped: it is a formula, `oppY(f) = f.y + max(0.04, HALF *
f.bank - 0.2)` (`city/freeway.js:64`), and is ported as that formula so the
cars' heights come out identical.

**Vehicle dimensions.** Length, width, wheel radius and wheelbase per kind
come from `CarModel.js` (thirteen kinds, `:1254-2010`) and the sawhorse from
`PursuitView.js`. Physics and collisions read them. `mr_sim::dims` holds
the table from M1, checked against a dump from JS; the car model port in M4
must agree with it.

**Inputs.** The reference run quantises inputs exactly as `InputFrame` does,
so both sides integrate identical values.

**Autodrive.** The `?autodrive=1` autopilot (`src/main.js:526-540`) is ported
into `mr_sim` as an input generator. Tests, the headless CLI and RL baselines
use it.

### 4.4 Port map

| JS | Lines | Rust |
|---|---|---|
| `src/util/math.js` | 106 | `mr_math` |
| `src/track/Track.js`, `roadTypes.js` | 513 | `mr_track` |
| `src/levels/*.js` | 690 | `mr_levels` (data and the Streets grid, cruise loop path) |
| `src/levels/seaside/load.js`, `circuit.js`, `ground.js` | n/a | `mr_levels::seaside`; `tools/seaside/build.py` gains a binary output (`assets/seaside/`) |
| `src/vehicles/CarPhysics.js` | 361 | `mr_sim::physics` (`CAR_SPECS`, `step`, `collide_walls`) |
| `src/vehicles/Vehicle.js` (state only) | 95 | `mr_sim::vehicle`; `sync()` and the body springs go to `mr_game` |
| Dimensions in `CarModel.js`; `runout` and the opposite carriageway from `City.js`, `Harbor.js`, `Streets.js` | n/a | `mr_sim::dims`, `mr_levels` world data (section 4.3) |
| `src/vehicles/Kinematic.js`, `AIDriver.js`, `Traffic.js` | 494 | `mr_sim::{kinematic, ai, traffic}` |
| `src/vehicles/Collisions.js` | 93 | `mr_sim::collisions` (the `Body` trait replaces duck typing) |
| `src/game/Race.js` (rules) | ~300 of 570 | `mr_sim::race` |
| `src/game/Pursuit.js`, `PoliceDriver.js`, parts of `PursuitView.js` | ~1,200 | `mr_sim::pursuit` |

### 4.5 Seams for later

The port does not build these, but the API must not block them:

- **Ground and walls.** `CarPhysics` reads the track through `project`,
  `frame`, `surfaceY`, the wall distances and `looseAt`. Keep those calls
  behind one small interface so a free-roaming ground (height field plus
  static colliders) can be a second implementation.
- **Vehicle model.** The handling code is one implementation of a
  `VehicleModel`. Later ones: the same arcade equations off the corridor,
  and a rigid-body car on Rapier (which has a ray-cast vehicle controller, a
  cross-platform determinism mode, and no Bevy dependency).
  `docs/vehicle-dynamics/SPEC.md` now designs the later models (and
  chooses its own solver over Rapier, decision VD-1).
- **More than one track.** `LevelRuntime` holds its Track by value; nothing
  assumes there is only one road in the world.

### 4.6 Tests

There are two reference runs of the JS, both with the shared math kernel,
named random streams, quantised inputs and dt = 1/120:

- **Module oracle (Node).** The pieces the JS unit tests already run under
  Node (physics, kinematic cars, rivals, traffic, collisions, pursuit),
  driven through staged scenarios in the order `Race` would call them.
- **Game oracle (headless Chrome).** The real game, real `Race.update`, with
  a fixed-dt hook and a recorder that captures simulation state after each
  update. `Race` cannot run under Node (it needs the HUD's DOM, car models,
  effects and a world), and a rewritten loop would be an oracle that is not
  the game. Presentation still runs but draws from `Math.random`, which the
  simulation no longer shares, so the recording is reproducible.

Tests:

1. **Ported unit tests.** `test/unit/{math,track,levels,seaside,physics,ai,traffic,pursuit}.test.js`
   become Rust tests with the same assertions, at the same dt (about 210
   tests). The few that assert on README text are dropped and listed in
   `DEVIATIONS.md`.
2. **Module traces.** Each scenario in the catalogue (`parity/scenarios.md`,
   written in WP 0.4: what it stages, on which level, with which bodies,
   for how long) is replayed in Rust and must match the JS **exactly**: the
   state hash at every tick, and the full state where the golden stores it.
3. **Whole races.** For every level, in race mode and (where the level has
   police) Hot Pursuit, the autopilot drives a full race in the game oracle.
   Rust must match exactly, tick for tick, to the results. For the cruise
   loop, which has no finish, the run is three minutes and the comparison
   includes score, multiplier and distance.
4. **Determinism.** The same run gives the same hashes on native and in wasm
   (under Node). Cloning the state, stepping both copies and comparing gives
   equality.
5. **Fuzz.** Random inputs on every level: nothing becomes NaN, the state
   does not grow, and no body is further outside the walls than the JS
   reaches under the same fuzz (measured in M0; collisions can push a body
   past a wall for a tick in the JS too, so "never past a wall" is not a
   property of a faithful port).
6. **Speed.** A benchmark records ticks per second for a full Sierra field,
   beside the same measurement of the JS from M0. The Rust must not be
   slower; the number is recorded for later RL targets.

**The trace record.** The goldens are written in M0, before the Rust state
exists, and the Rust keeps some state in other shapes than the JS. So the
thing compared is not either side's state but a **trace record** both can
produce, defined once in `parity/trace-format.md` (WP 0.4) and versioned:

- a header: tick, race state, clock, countdown;
- per player: position, yaw, velocities, yaw rate, `s`, `lat`, steer angle,
  on-ground flag, gear, rpm, shift timer, nitro, boost, drifting, slip,
  skid, scrape, damage, spiked, and the rule state (progress, lap, score,
  multiplier, timers);
- per rival, per traffic car, per pursuit unit, block car and sawhorse, in
  pool order with inactive ones included: active flag, `s`, `lat`, speed,
  lateral velocity, direction, spin and spin rate, stunned, world position
  and yaw, and the driver state that affects later ticks (avoid target and
  timer, nitro timers, crashed, mode, target);
- pursuit: heat, heat meter, state, bust and evade meters, holds, props;
- the position of each named random stream.

Numbers are written as the bits of an `f64` (or an `i32` for integers and
enum codes), little-endian, in a fixed field order. Presentation-only fields
are left out: body pitch and roll and their rates (`Vehicle.js:82-85`),
brake light, anything the HUD keeps. The per-tick hash is 64-bit FNV-1a over
the record's bytes. On the Rust side this is `mr_sim::trace_record()`, a
separate function from `hash()`, which is free to cover the real state.

A golden stores the hash for every tick and the full record every 120 ticks,
so a mismatch can be localised without committing tens of megabytes. Staged
scenarios are committed; whole races are regenerated on demand and cached.

The recorder stops and fails the capture on any page error. The JS main
loop catches exceptions and carries on (`main.js:577-580`), which would
otherwise leave a half-run tick in the recording.

When a trace diverges, the cause is a port bug until shown otherwise:
operation order, a JS semantic from section 4.2, a float width, an iteration
order, or a draw from the wrong stream. Find the first differing field at
the first differing tick.

## 5. World generation (`mr_canvas`, `mr_worldgen`)

About 26,000 lines of JS build the worlds: terrain and road (3,500),
scenery (20,500) and car models (2,400). They are ported one to one into a
crate that outputs data, not engine objects.

### 5.1 Scene description

The scene types live in `mr_scene`, so the client can use them without the
world generator.

```rust
// mr_scene: plain data
pub struct Scene {
    pub meshes: Vec<MeshDesc>,        // positions, normals, uvs, colours,
                                      // named custom attributes, indices
    pub instances: Vec<InstanceDesc>, // mesh + transforms (+ colours, attrs)
    pub materials: Vec<MaterialDesc>, // MaterialKind + parameters + textures
    pub textures: Vec<TextureDesc>,   // RGBA8 / R8 pixels + sampler + flags
    pub nodes: Vec<NodeDesc>,         // groups, chunk bounds, cull distances
    pub lights: Vec<LightDesc>,
    pub night_params: Vec<NightParam>,// material property, day and night value
}

// mr_worldgen: what a level build returns
pub struct WorldBuild {
    pub scene: Scene,
    pub animators: Vec<Box<dyn Animator>>,
    pub sim_data: SimWorldData,       // runout, opposite carriageway (4.3)
}
```

- `MaterialKind` is a closed enum: one variant per distinct shader in the JS
  game (section 6.2). Parameters are plain numbers and texture handles.
- Custom vertex attributes keep their JS names: `aLane`, `aSurf`, `cell`,
  `aVar`, `ndata`, the flicker phase, and so on.
- An `Animator` replaces a closure in `world.updaters`. Each frame it gets
  `(dt, night, camera, player s)` and writes edits (a transform, instance
  matrices or colours, a material parameter, a UV offset) addressed by
  handle. The freight train, tumbleweeds, Ferris wheel, lighthouse beam,
  boats, chaser bulbs, signals and the rest are all of this form. Animators
  are code, so they are not part of a scene file.
- **The scene file (`.mrscene`)** is a `Scene`: a JSON header (version,
  nodes, materials with their kind and parameters, accessors) followed by
  little-endian binary buffers, in the manner of glTF, so that JavaScript
  can write it and Rust can read it. The JS game exports one per level
  (roadmap WP 0.5), with animated objects at their rest pose. That gives the
  renderer real scenes before any scenery is ported and gives world
  generation its golden reference. For the export, each JS material carries
  a tag naming its `MaterialKind`; adding those tags is part of WP 0.5.

### 5.2 The three.js geometry subset

The builders rely on three.js generators and, in places, on their exact
vertex order and UV layout. `mr_worldgen::three_geom` is a line-by-line port
(three.js is MIT) of: Box, Cylinder, Cone, Plane, Circle, Sphere,
Icosahedron, Torus, Capsule, Lathe, Tube, Extrude (with bevel), Shape and
`ShapeUtils.triangulateShape` (earcut), `CatmullRomCurve3`, and
`BufferGeometryUtils.mergeGeometries`. Each is tested against a dump from
three.js r180: same vertex count, same order, values within 1e-6.

Conventions that carry over unchanged: Y up, right-handed, the camera looks
down -Z, yaw θ has forward (cos θ, sin θ) in XZ. Bevy uses the same axes.

Two that need care:

- **Colours.** three.js converts hex colours from sRGB to linear. Vertex
  colours and material colours in a `Scene` are linear.
- **Texture orientation.** A three.js `CanvasTexture` is uploaded flipped
  (`flipY`), and the JS UVs and shader atlas math assume that. The Rust
  uploader flips rows for textures marked `flip_y` (canvas textures) and not
  for the others (data textures, Seaside's photo). UVs and shader math stay
  exactly as in JS.

### 5.3 Canvas 2D (`mr_canvas`)

Almost every texture is drawn with the browser's Canvas 2D API. `mr_canvas`
implements the subset in use on tiny-skia, with the same method names, so
texture code ports nearly line for line: rectangles, paths, arcs and
ellipses, `roundRect`, linear and radial gradients, `globalAlpha`, the
composite modes in use (`source-over`, `destination-out`, `lighter`, and any
others found), `shadowBlur` and `shadowColor`, `fillText`, `strokeText`,
`measureText`, `getImageData`, `putImageData`, `createImageData`,
transforms and save/restore.

**Fonts.** The JS names system fonts (Arial Black, Arial Narrow, Georgia,
Brush Script MT, Segoe Script, Courier New), which differ per device: on the
Linux reference machine most of them fall back to Noto Sans. The port
bundles open-licence substitutes so every platform draws the same signs.
First candidates: Archivo Black, Archivo Narrow, Gelasio, Yellowtail, Caveat,
Courier Prime, and Rajdhani for the HUD. The reference capture injects the
same files into the JS page with `@font-face`, so text matches in
comparisons. The owner picks the final set from a gallery of sign textures.

### 5.4 Exactness

Scenery is placed on the terrain as drawn, so terrain heights must match the
JS to well under a millimetre or props float and sink. Port `Terrain.js`,
`TerrainMesh.js` and the noise functions with the same arithmetic in `f64`
and the rules of section 4.2, and test heights at 10,000 sample points per
level against the JS run with the shared math kernel. They should be
identical; the gate is 1e-9 m. The JS `**` operator cannot be redirected to
the kernel, so WP 0.3 rewrites it as `Math.pow` in simulation and
world-generation code (about forty places), which changes nothing in the
live game. Scenery modules must draw from their seeded generators in the same
order as the JS.

### 5.5 Building in steps

Wasm has one thread, so a level build must yield to keep the loading bar
moving. `mr_worldgen` exposes a build as a list of jobs with the JS progress
labels ("Surveying the route", "Shaping the land", "Sculpting terrain",
"Paving roads", "Filling the sea", then each scenery's label). On the web the
client runs jobs for a time slice per frame. On native, independent jobs run
on a thread pool. Output is identical either way: jobs are pure and their
results are assembled in a fixed order.

Static meshes are uploaded to the GPU and the CPU copy is dropped. Wasm
memory never shrinks, so the high-water mark is a budget (section 6.6).

### 5.6 Port volume

| Area | JS files | Lines |
|---|---|---|
| Infrastructure | `World`, `Terrain`, `TerrainMesh`, `Road`, `Sky`, `Sea`, `textures` | 2,900 |
| Builders | `valley/Builder`, `beach/ColorBuilder`, `beach/atlas`, `city/geom`, `harbor/build`, `coast/kit`, `valley/ground` | ~1,000 |
| Level 1 | `Mountain`, `Valley` (+ `valley/*`), `City` (+ `city/*`) | ~7,400 |
| Level 2 | `Coast`, `Beach` (+ `beach/*`), `Harbor` (+ `harbor/*`) | ~5,400 |
| Level 3 | `Streets` (+ `streets/*`) | ~3,200 |
| Level 4 | `Desert` (+ `desert/*`) | ~3,100 |
| Level 5 | `Raceway` (+ `raceway/*`), Seaside loader | ~1,100 |
| Cars | `CarModel` | 2,400 |

### 5.7 Parity tests

- **Terrain and track.** Heights and all Track arrays against JS dumps.
- **Geometry digest.** For each level, each mesh bucket in the Rust scene is
  matched to one in the JS scene export (by material kind and bounds) and
  compared: equal vertex and triangle counts, bounds within 1 cm, surface
  area within 0.1 %, centroid within 1 cm. Instances: equal counts,
  transforms within 1 mm. The report lists unmatched buckets on either side;
  the gate is none.
- **Textures.** Each generated texture against the JS canvas: mean absolute
  difference under 3/255 per channel, with a side-by-side sheet for review.
  Text regions are compared with the same fonts loaded on both sides.

## 6. Rendering (`mr_game::render`)

### 6.1 What to match

Read from `src/main.js:55-111` and `src/world/Sky.js`; the full list with
line references is in [inventory-rendering.md](inventory-rendering.md):

- Camera: 62° vertical field of view, near 0.3. Wider with speed (+16°) and
  nitro (+7°); a wider vertical field on portrait screens.
- Target: half-float HDR with 4× MSAA. Bloom: three.js `UnrealBloomPass`,
  strength 0.38, radius 0.35, threshold 0.92, always on. Then ACES filmic
  tone mapping with the sky's exposure, then sRGB.
- One directional light (sun, crossfading to moon) with one 2048² shadow map
  over a ±70 m box that follows the focus, PCF-soft filtering, bias -0.0004,
  normal bias 0.6. A hemisphere light. `FogExp2`, with the fog colour
  shifted toward the sun's colour when looking at the sun.
- Environment map: the sky dome prefiltered, intensity 0.7, refreshed when
  time of day moves by 2.5 % of the route.
- Local lights: the player's headlight spot, one shared police point light
  (high quality only), the desert train's spot. Everything else that glows
  is emissive colour above 1.0, additive points or additive ground quads.
- "High quality": pixel ratio min(device, 1.5) instead of 1, shadows on, the
  police light exists. Off by default on touch devices.

The port reproduces this pipeline as it is, including the single shadow
map. WebGPU makes better options available (cascades, more lights), but
those are for after cutover: using them now would break the comparison.

### 6.2 Shading model

A WGSL library, `three_std`, reproduces the lighting of three.js r180's
standard and physical materials for the features the game uses: base colour
map and vertex colour, emissive, roughness and metalness, clearcoat and
sheen (high-detail cars), environment reflection, transparency, the
directional light with its shadow, the hemisphere light, one spot and one
point light, and fog. It uses the same variable names as three.js where the
JS patches refer to them (`diffuseColor`, `totalEmissiveRadiance`,
`roughnessFactor`), so each patch ports as a block of shader code at the
same point in the pipeline.

Each JS `onBeforeCompile` patch and each `ShaderMaterial` becomes a
`MaterialKind` built on that library:

| Kind | JS source | What it adds |
|---|---|---|
| Terrain | `TerrainMesh.js:413` | Triplanar rock and ground, four detail scales from one packed texture, ground type from vertex colour and `aSurf`, derivative bump, optional draped photo and loose-ground mask |
| Asphalt, Shoulder, Markings | `Road.js:111,277,147` | Wheel paths and oil from `aLane`, patches, dusty edges, night dampness; gravel fade; paint wear |
| Sea | `Sea.js:115` | Depth foam, two scrolling normal scales, Fresnel alpha, glitter |
| SkyDome | `Sky.js:42` | Gradient, sun and moon, clouds, stars |
| CarLight | `CarModel.js:124` | Emission scaled by vertex colour |
| PoliceGlow | `CarModel.js:2065` | Camera-facing quads with a minimum screen size |
| TriplanarRock, Sandstone, Stucco, Siding | `Mountain.js:63`, `coast/kit.js:89`, `desert/parts.js:52`, `Beach.js:66`, `Valley.js:89` | World-space surface patterns |
| CityAtlas, CityFacade, StreetAtlas, StreetFacade, ContainerAtlas | `city/cityTextures.js:160,493`, `streets/textures.js:349`, `streets/facades.js:395`, `harbor/textures.js:221` | Atlas cell per vertex, lit windows, street-light spill |
| GlowPoints, FlickerPoints, GroundPool | `City.js:1518`, `desert/glow.js:14` | Sized, flickering, fog-softened glows |
| Neon, AmbientProp | `streets/props.js:27,15` | Flicker modes; ambient emissive |
| TrafficStreams, SkyGlow, Surf, LighthouseBeam, Steam, FloodBeam, Reflector | `City.js:1334,1389`, `Coast.js:26,72`, `streets/props.js:419`, `Desert.js:1786`, `Mountain.js:886` | Animated in the shader |
| Particles, SkidMarks | `Effects.js:7,107` | Soft points; alpha quads |
| Standard, Physical, Lambert, Basic, Line, Sprite | built-in | Plain versions |

**Points.** WebGPU and wgpu have no point size. Every three.js `Points`
object becomes instanced camera-facing quads with the size, minimum pixel
size and soft falloff computed in the vertex shader.

**Post-processing.** One module holds the chain: threshold bloom ported from
`UnrealBloomPass`, ACES filmic with exposure as three.js applies it, sRGB.
Bevy's tone mapping is turned off on the camera. Bevy's own bloom may stand
in until the port passes the bloom comparison scene.

**Sorting and offsets.** JS uses `renderOrder`, transparent sorting and
`polygonOffset` decals. Map `renderOrder` to explicit sort keys and
`polygonOffset` to depth bias, and check each use against a screenshot.

**Verification.** A set of material test scenes (a sphere and a plane per
material kind under fixed light, a bloom chart, a fog ramp, a shadow edge) is
rendered by three.js and by the Rust client with the same parameters and
compared per pixel. This isolates shading differences from geometry
differences.

### 6.3 Warm-up

On the web, Bevy compiles pipelines synchronously the first time a material
is drawn, on WebGPU as on WebGL2. During the loading screen the client draws one triangle with every
material and mesh-layout combination the level uses, off screen. The gate is
no frame over 50 ms in the first thirty seconds of a race.

### 6.4 Culling and LOD

Keep what the JS does: frustum culling per chunk (chunk sizes as in JS, 420
to 720 m), the city loop's 2,000 m chunk cut-off, traffic and police far
models at 95 m out and 85 m back, the three forest tiers, far blocks in
Streets. Nothing streams; a level is built whole.

### 6.5 Effects and camera

Port `Effects.js` (smoke and sparks as CPU-simulated ring buffers, skid
quads, nitro flame cones, headlight pools), `CameraRig.js` (chase, far and
bumper views, field-of-view changes, sine shake, the intro swing) and
`Vehicle.sync` (orientation from the road frame, pitch and roll springs,
wheel spin, steer pivots, light setters). They run per rendered frame on the
interpolated state. Per-frame random effects in JS (`Math.random() < 0.6`)
become rates defined at 60 Hz, so they do not double on 120 Hz displays.

### 6.6 Budgets

Measured on the JS game, desktop Chrome on an RTX 3060, high quality, driving
with the autopilot (peak values over the route, shadow and bloom passes
included):

| Level | Draw calls | Triangles | Textures | Geometries | Load | JS heap |
|---|---|---|---|---|---|---|
| Sierra to the City | 833 | 3.48 M | 67 | 563 | 8.9 s | 193 MB |
| Coast Highway | 705 | 3.50 M | 89 | 517 | 7.0 s | 237 MB |
| Downtown Streets | 686 | 1.98 M | 66 | 542 | 3.2 s | 461 MB |
| Desert Run | 644 | 3.13 M | 60 | 394 | 4.9 s | 127 MB |
| Seaside Raceway | 404 | 1.18 M | 31 | 199 | 2.5 s | 64 MB |
| Night City Cruise | 721 | 2.06 M | 35 | 404 | 3.3 s | 319 MB |

Rust budgets, per level and per reference device (the desktop above, the
owner's iPhone, one Android phone from about 2021; the phone baselines are
measured in M0):

- Draw calls and triangles: no more than the JS figure plus 10 %.
- Frame time: no worse than the JS game on the same device and settings.
- Level load: no slower than JS on the web; at least twice as fast native.
- Wasm memory high-water mark: under 512 MB on phones, and no growth across
  ten level switches.
- Download: the wasm file under 16 MB after gzip (raised from 10 MB,
  D675), measured by `cargo xtask size` and tracked in CI. The dev server
  sends precompressed files from `dist/` with `Content-Encoding`, so load
  times on phones are realistic.
- Time from navigation to the menu on a phone over Wi-Fi: within 5 s of the
  JS game's. (At G1 there is no menu yet; the gate there uses time to the
  client's first rendered frame, before any scene loads.)

## 7. Audio (`mr_audio`)

### 7.1 Decision

The JS audio is a Web Audio graph of about 250 persistent nodes, with no
AudioWorklet: built-in oscillators, filters, wave shapers, compressors and
convolvers steered by parameter automation. The port keeps that graph and
drives it from Rust through a facade with two backends:

- **Web:** the browser's own Web Audio nodes through `web-sys`.
- **Native:** the `web-audio-api` crate, a Rust implementation of the same
  API (version 1.7.0 at the time of writing).
- **Null:** records calls; used headless and by strict tests.

Why not one Rust DSP engine everywhere: on the web that means wasm inside an
AudioWorklet. The Rust audio engines that do this today need a nightly
toolchain, shared memory and cross-origin isolation headers that GitHub
Pages cannot set. It would also replace the browser's native DSP with wasm
DSP on the phones where audio cost has already been a problem. Using the
browser's nodes keeps the exact sound and the exact cost on the web.

### 7.2 Facade

`mr_audio::wa` exposes handles and methods shaped like Web Audio, limited to
what the game uses: Oscillator (built-in types and periodic waves),
BufferSource (loop, playback rate), Gain, BiquadFilter (lowpass, highpass,
bandpass, peaking, both shelves), WaveShaper (curve, oversampling),
DynamicsCompressor, Convolver, Delay, ChannelMerger, StereoPanner, Analyser;
connect and disconnect, including node-to-parameter connections; parameter
automation (`setValueAtTime`, linear and exponential ramps,
`setTargetAtTime`, `cancelScheduledValues`); buffers; `decodeAudioData`;
`currentTime`, suspend, resume and state; an offline context.

### 7.3 Port map

| JS | Rust | Notes |
|---|---|---|
| `Audio.js` graph, buses, `Gate` | `mr_audio::{graph, gate}` | The `Gate` (disconnect idle voices, tails, no steering before the context runs) is kept as is |
| `Audio.js` engine cycles, profiles, steering | `mr_audio::engine` | `engineCycle` and `rumbleCycle` are pure math: port and test against JS output arrays |
| `Audio.js` turbo, electric, damage, environment, rivals, sirens, tyres | `mr_audio::voices` | |
| `Audio.js` one-shots | `mr_audio::oneshots` | |
| `audio/samples.js` | `mr_audio::samples` | Pure sample math; outputs compared to JS buffers within 1e-5 |
| `audio/Music.js`, `tracks.js` | `mr_audio::{music, tracks}` | Lookahead scheduler pumped from the frame loop and from a timer; song data as Rust constants |
| `audio/radioLines.js`, `RadioVoice.js` | `mr_audio::radio` | Clips stay as MP3 files under `assets/radio/`; fetched on the web, read from disk native |
| `music.html` | an in-game Music player screen | Sections, solo, seek, repeat, level meter and spectrum |

The client calls the same interface the JS game does: `update(state)` each
frame with the fields listed in [inventory-audio.md](inventory-audio.md)
section 8 (rpm, throttle, speed,
gear, boost, skid, slip, off-road, scrape, nitro, on-ground, and the electric
car's motor, power and regen), per-frame setters (rival engines, sirens,
mood, damage, spiked tyres), and event calls (shift, impact, landing, beep,
whoosh, nitro burst, fanfare, pursuit stingers, radio lines, UI clicks).

### 7.4 Browser rules

Kept from the JS: create the context lazily; set
`navigator.audioSession.type = "playback"` before creating it so iPhones
play with the Silent switch on; resume on every pointer-up, touch-end, click
and key-down, inside the event handler (section 8.4); suspend on pause and
when the page is hidden; never await a resume.

### 7.5 Tests

- The strict fake backend from `test/unit/music.test.js` (invalid types,
  non-finite times, exponential ramps to zero, mismatched wave arrays) is the
  null backend's validation mode. The music, radio, pursuit-audio and
  audio-session unit tests are ported with it.
- Engine waveform arrays, drum kit and SFX buffers, noise beds and impulse
  responses match the JS arrays numerically.
- Offline renders (each car at a table of rpm and throttle, each song's
  first thirty seconds, each one-shot) from Chrome and from the native
  backend agree within 1.5 dB RMS per third-octave band. The web backend is
  the same browser nodes, so it is checked by a recorded call log matching
  the JS call log for the same scenario.

## 8. UI, HUD, input and platform (`mr_game`)

### 8.1 UI

Menus, HUD and touch controls are drawn in-engine with Bevy UI, so native
and web share one implementation. The DOM version is the visual reference;
match layout, type, colour and motion. Effects with no direct equivalent
(backdrop blur, gradient text) may be approximated and listed as deviations.

- **Screens:** loading, menu (level tabs, level card with Race / Hot Pursuit
  switch, car picks, options), pause, controller setup, results (race,
  pursuit, circuit and cruise forms), music player. States and transitions
  as in `src/main.js`.
- **HUD:** every element in `src/game/HUD.js` and `hud.css`: position,
  clock, lap panel, penalty, zone name and banner card, route bar with racer
  dots, minimap, tachometer or the electric power meter, speed and gear,
  nitro bar, damage bar, speed lines, centre pop text, toasts, heat stars,
  bust and evade bar, radio line, hold card, cruise panel, now-playing pill,
  stats overlay.
- **Dials and minimap** are drawn each frame as generated 2D meshes (arcs,
  ticks, polylines), not a CPU canvas. Speed lines are a full-screen UI
  shader.
- **Layout** follows the CSS breakpoints: width under 720, height under 780
  and 500, portrait, and the compact touch layout. Safe-area insets come
  from the page (section 8.4).
- **Widgets** live in one module so a Bevy UI API change touches one place.
- **Test ids.** Every interactive node carries the id its DOM element had
  (`btn-start`, `opt-hq`, ...). The test bridge finds controls by id.

The Bevy window is the whole page. Before wasm is ready, a small static HTML
loading screen shows the logo and a download progress bar.

### 8.2 Input

Port `Input.js`, `Gamepad.js`, `MenuNav.js`, `PadSetup.js`,
`TouchControls.js` and `TiltSteer.js` with their constants:

- Keyboard map and one-shot actions; digital steering ramp (3.6/s in, 7/s
  back, 9/s counter-steer); merge order of keys, touch, tilt and pad.
- Gamepad bindings (`{button}` or `{axis, dir, rest}`), default map, dead
  zone 0.12 with exponent 1.4, capture with an 8 s timeout, per-pad saved
  maps, the mute set for held buttons across screens, menu navigation with
  its repeat timing and spatial scoring.
- Touch: floating stick with its range, dead and end zones and exponent; the
  pedal slider bands and the drift strip; button modes; 14 px slop;
  multi-touch; auto gas.
- Tilt: `screenRoll`, `rollToSteer`, sensitivity, smoothing, and the sensor
  states (off, waiting, live, none, insecure, ask, denied).

The input layer runs at the tick rate and produces the quantised
`InputFrame`. The steering ramp is part of the input layer, not the
simulation.

**Rumble.** Bevy's gamepad backend has no rumble on the web. A `Rumble`
trait has a `web-sys` backend (`vibrationActuator.playEffect("dual-rumble")`
with the JS resend rules) and a native backend (force feedback through
gilrs). `kick` and `feel` behave as in `Gamepad.js`.

### 8.3 Settings and records

A `Store` trait: `localStorage` on the web, a JSON file in the user's config
directory native. Keys and formats are exactly the JS ones (`mr.musicVol`,
`mr.car`, `mr.best.<level>`, `mr.padMaps`, ...; the full table is in
[inventory-ui-input-tests.md](inventory-ui-input-tests.md) section 3). The Rust build is served from the same
origin, so players keep their settings, best times and controller maps.

### 8.4 Platform glue

| Concern | Web | Native |
|---|---|---|
| Gestures | A small script listens on the page for pointer-up, touch-end, click and key-down and, inside the handler, calls into wasm. The wasm side resumes audio and, if the pointer is on a control flagged for it, requests fullscreen, the landscape lock and motion permission. These must happen in the handler, not a frame later. | Not needed |
| Fullscreen | Best effort; iPhones do not have it | F11, saved window state |
| Tilt | `deviceorientation` through `web-sys` | None |
| Visibility | `visibilitychange` pauses a race | Focus loss pauses |
| Parameters | Query string, same names as today | `--query "level=sierra&autostart=super"` |
| Haptic tick | `navigator.vibrate(8)` on touch buttons | None |
| Shell | `index.html` with the manifest, og tags, icons and loading screen. It checks `navigator.gpu` first and shows a plain "this browser has no WebGPU" page, with what to use instead, when it is missing. | Window title and icon |
| Desktop app | n/a | Replaces the Electron shell: `play.sh` runs the native binary; the desktop entry script stays |

The same debug hooks exist: `level`, `pursuit`, `heat`, `cops`, `t`, the fly
camera (`s`, `h`, `back`, `lat`, `yaw`, `pitch`), `timescale`, `stats`,
`autodrive`, `autostart`, `touch`.

### 8.5 Test bridge

The e2e harness (`test/e2e/harness.js`) drives the JS game through
`window.__race`, `__game`, `__audio` and friends, and through DOM selectors.
The Rust web build exposes `window.__mr`:

- `ready`, `mode`, `screen`
- `ui(id)` returns `{x, y, w, h, visible, enabled, value}` for a test id
- `race()`, `pursuit()`, `audio()`, `stats()` return JSON snapshots with the
  field names the tests read today
- `stage(cmd)` runs the staging commands the tests use (place the car, set
  speeds, start a pursuit, place a roadblock)

The harness gains a `target` option. With `target: "rust"`, `tap("#btn-start")`
looks the id up through `ui()` and taps its centre. The e2e suites are then
run against both builds until cutover.

Headless Chrome needs WebGPU switched on for these runs (on the Linux dev
machine: Vulkan and the unsafe-WebGPU flag alongside the harness's existing
GPU flags). The harness's intercepted `https://` origin is a secure context
already. CI machines have no GPU, so CI runs unit, parity-data and build
jobs; the browser suites run on the dev machine, as they do today.

Native has `--smoke-test` (run to ready, report errors, exit code) and
`--screenshot <png> --after <frames>`.

### 8.6 Level viewer ("god mode")

A Rust-only tool, not in the JS game, for reviewing a level from any point
of view (the owner, 2026-10-04, DECISIONS D679). It changes nothing in the
game, the simulation or the pictures the parity gates take; it is the
built level (section 6.4: built whole) seen through a free camera.

- **Entry.** `?view=god` with `level=` on the web, `--query "view=god&..."`
  natively, and a "Level viewer" button on the menu's level card, so it is
  reachable on a phone without typing a URL. The level is the one the
  client builds by default (D678); `?world=export` works as elsewhere.
- **Cameras.**
  - *Free fly*: move, rise and sink, look; a speed that scales from walking
    pace to several hundred m/s.
  - *Orbit*: circle, tilt and zoom about a point (the route at the chosen
    position, or a picked point).
  - *Overview*: a top-down view of the whole level, the route drawn over
    it with its zone boundaries; tap or click a point to fly there.
  - *Ride*: the existing fly camera along the route (`s`, `h`, `back`,
    `lat`, `yaw`, `pitch`), with speed and height adjustable live.
- **Controls.** Keyboard and mouse (WASD and Q/E or Space/Ctrl, Shift
  faster, drag or pointer-lock to look, wheel for speed or zoom); gamepad
  (left stick moves, right stick looks, triggers sink and rise, bumpers
  change speed, with the WP 6.4 pad layer); touch (one finger looks or
  orbits, a thumb stick moves, two-finger pinch zooms and pans).
- **Panel.** A small collapsible panel over the view:
  - route position slider (jumps the camera) and the zone name;
  - time of day: follow the camera's route position as the game does, or
    pin it to any point of the route;
  - toggles: fog, far plane extended (to see the whole level), animators
    running or frozen, the level's top-level scene groups shown or hidden;
  - readout: camera position, nearest route position, frame time, draw
    calls and triangles, wasm memory;
  - screenshot (a PNG download) and a link that carries the camera pose
    (`cam=x,y,z,yaw,pitch`, plus the panel's state), so a view can be sent
    and opened again exactly.
- **The world follows the camera.** The world's focus (chunk cut-offs,
  LOD, the sky dome, distance-shown nodes) is the camera position, as the
  game's is the player's car.
- **Budgets.** Free fly and orbit hold the race's frame-time and memory
  budgets (section 6.6). Overview and the extended far plane draw more than
  any race view; they are measured and may cost more, but must not crash a
  phone.
- **Test bridge.** `__mr.viewer` reports the mode and pose and takes a pose,
  so the e2e harness and the parity tools can place the camera.

## 9. Multiplayer (`mr_net`, `mr_host`)

### 9.1 Model

**Server-authoritative, with every client simulating the whole world and
rolling back when it guessed wrong.**

- Each client runs the full simulation locally: its own car from its own
  inputs, AI, traffic and police from the shared seed, and remote players
  from their last known inputs.
- Clients send their inputs, stamped with the tick they apply to. The host
  runs the authoritative simulation and broadcasts each tick's inputs for
  all players.
- When a client learns that a remote input differed from its guess, it
  restores its saved state for that tick and re-simulates to the present.
  The state is small and a tick is cheap, so this is affordable every frame.
- The host also sends a full snapshot with a state hash a few times a
  second. A client whose hash differs adopts the snapshot. So determinism
  across devices makes corrections rare, but nothing breaks if a browser's
  arithmetic differs.

This fits a racing game: car-to-car contact is resolved by the same code on
both sides of it, with no interpolation delay between cars that are trading
paint. On a LAN, guessing "same input as last tick" is almost always right.

Single-player uses the same session with a loopback transport, so there is
one code path.

### 9.2 Parameters

- Input delay: one tick on a LAN, adjustable.
- Rollback window: 32 ticks (267 ms). Beyond it, resynchronise from a
  snapshot.
- Client clock: runs ahead of the host by half the round trip plus a margin,
  nudged by small tick-rate changes rather than jumps.
- Inputs are sent with the previous few ticks repeated, so one lost packet
  costs nothing on an unreliable channel.
- Snapshots: every 30 ticks, compressed.

### 9.3 Protocol and transports

Messages (binary, versioned): `Hello`, `Welcome` (slot, seed, level, laps,
tick), lobby messages (car, ready, start), `Input`, `Inputs` (authoritative,
per tick), `Snapshot`, `Ping`/`Pong`, `Leave`.

```rust
pub trait Transport {
    fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]);
    fn poll(&mut self, out: &mut Vec<NetEvent>);
}
pub enum Channel { Reliable, Unreliable }   // WebSocket maps both to itself
```

- **WebSocket** (first). The host serves the page over https and accepts
  `wss://` on the same port. TCP ordering is acceptable on a LAN.
- **WebRTC data channels** (second, via `matchbox_socket`, which is not tied
  to Bevy). Needed for a tab to be host, and gives a true unreliable channel.
  It needs a signalling service to introduce the peers. It is also the way a
  page loaded from GitHub Pages can reach players on a LAN without anyone
  holding a certificate (section 9.5).
- **Loopback** (single-player, tests). A test transport adds latency, jitter,
  loss and reordering.

### 9.4 The host program

`mr-host` is one native binary:

- Serves the built web client as static files, with the cache rules of
  `tools/serve.py`, and precompressed wasm.
- Accepts WebSocket connections at `/ws` on the same port.
- Runs the lobby and the authoritative simulation, headless.
- **Port:** `--port`, then `$PORT`, then `proj port`, then exit with an
  error. There is no default. When it is ready it replaces `tools/serve.py`
  inside the registered `serve.sh`, so the project still uses one registered
  port.
- `--tls-cert` and `--tls-key` serve https and wss. Other devices need
  this: WebGPU exists only in a secure context, and `http://192.168.x.x` is
  not one. `http://localhost` is, so the machine running the host can play
  without a certificate.

### 9.5 Secure context: the consequence of requiring WebGPU

Browsers expose WebGPU only to secure contexts: https pages and
`http://localhost`. A page served by a LAN machine over plain http gets no
WebGPU, so it cannot run the game at all. Every way of joining a LAN game
therefore has to start from an https page.

| How players load the game | What it needs | Notes |
|---|---|---|
| From `mr-host` over https | A certificate browsers trust for the host's name. For devices on the owner's tailnet, `tailscale serve` in front of the host does this with no certificate handling in the game. For guests not on the tailnet, a public DNS name that resolves to the LAN address with a certificate from DNS validation. | First multiplayer release. Works with no internet once set up. Everything works, tilt included. |
| From GitHub Pages, peers connect by WebRTC | A signalling service reachable over wss (a small public service) to introduce the peers. Game traffic then stays on the LAN. | Second release; the same transport as tab-hosting. The native host can join as a WebRTC peer. Needs internet for the introduction. |
| From GitHub Pages, WebTransport to `mr-host` with a certificate hash in the join link | Host generates a short-lived self-signed certificate; Chrome 100+, Firefox 125+, Safari 26.4+ | Experimental. Safari's WebTransport is new and Rust server interop with it is still being fixed. Not planned; revisit after the second release. |

An https page cannot open `ws://` to a LAN address (mixed content), which is
why "Pages plus a plain WebSocket host" is not on the list.

The same rule affects development. The registered dev server speaks plain
http, so by itself the Rust build opens from `localhost` on the dev machine
but not from a phone or another computer. `serve.sh` therefore also mounts
the server on the dev machine's tailnet https address with `tailscale serve`,
under the path `/midnight-racer/` (the registry records the address as the
project's `web_url`; the trailing slash matters). Any device on the tailnet
gets a secure context there, so WebGPU and tilt work, with the working tree
served live: the JS game at `/midnight-racer/` and the Rust build at
`/midnight-racer/dist/next/`. This is how the Rust build is tested on phones
throughout the port. GitHub Pages plays no part until cutover.

Because the game is always served under some sub-path (this mount, the
`dist/next/` directory, the Pages project path), every URL the client uses
must be relative: assets, the wasm, and later the WebSocket endpoint.

The same front covers tailnet multiplayer: `mr-host` can stay plain http
behind `tailscale serve`, and its own `--tls-cert` option is only needed for
guests outside the tailnet.

### 9.6 First release: race together

Two to eight humans on any race level. The host picks level, laps and
whether AI fills the grid. Traffic is on. Results list everyone. A player
who drops out is driven by the AI to the finish. Open design points, to be
settled in the M10 design note: what rival rubber-banding follows when there
are several humans, grid order, and whether nitro bonuses change.

### 9.7 Tab as host (later)

The session code compiles to wasm, so a tab can run the authority. Clients
connect by WebRTC through a signalling service. The host tab must stay in
the foreground, because background tabs are throttled.

## 10. Headless use and RL

Not part of the port, but the simulation is built for it and it can start
any time after M1:

- `mr_sim::Env`: `reset(level, car, seed)`, `step(action)`, an observation
  vector (speed, lateral offset, heading error, curvature and width ahead at
  fixed distances, wall distances, nearby cars), reward terms, and state
  save and restore.
- Batched stepping across cores in Rust; `mr_py` exposes it to Python as a
  Gymnasium vector environment through PyO3.
- Observations are state vectors. Rendering pixels headless is a separate
  later decision.

## 11. Build, tooling, deployment

- **Toolchain:** stable Rust, `wasm32-unknown-unknown`, `wasm-bindgen-cli`
  pinned to the crate version, `wasm-opt`.
- **`cargo xtask web [--release]`** builds the client and writes
  `dist/next/` (already git-ignored): `index.html`, the JS glue, the wasm,
  `assets/`. The registered server (`./serve.sh`, `proj up midnight-racer`)
  serves the repo root, so the build is at `/dist/next/` on the project's
  registered port. Do not start any other server, and never write a port
  number into a script, config or default. That server speaks plain http;
  phones and other machines reach it through the tailnet's https front
  (section 9.5).
- **Profiles:** dev builds dependencies optimised. Release web builds use
  fat LTO, one codegen unit, `panic = "abort"`, size-optimised where a
  benchmark shows no frame-time cost, then `wasm-opt`.
- **`cargo xtask parity ...`** runs the comparisons in section 12 and writes
  a static report to `parity/report/` (git-ignored), viewed through the same
  registered server at `/parity/report/`.
- **CI (GitHub Actions):** format, clippy with warnings denied, native
  tests, simulation tests in wasm, dependency rules, web build, size report,
  the JS unit tests. CI does not deploy. GitHub Pages keeps serving `main`
  as it is, which is the JS game, until cutover. At cutover (roadmap M9) the
  Pages source is switched from "branch" to "GitHub Actions" by the owner,
  and a deploy job publishes the Rust build at the root with the JS game
  under `/legacy/`.
- **Commits:** as today, finished and tested work goes straight to `main`.
  The JS game at the root is not touched by Rust work, and nothing the
  Rust build needs is served from Pages, so the live site keeps working
  throughout. Build output stays out of git.
- **`CLAUDE.md`** (created in M0) carries the working rules for agents:
  the principles in section 1.1, how to run tests and parity, how to view
  output locally, the port rule, and the file-ownership rule for parallel
  work.

## 12. Parity and acceptance

Five layers. A work package names the layers that gate it.

| Layer | What | Tool | Passes when |
|---|---|---|---|
| L1 | Ported unit tests | `cargo test` | Same assertions as the JS tests pass |
| L2 | Numeric goldens | `cargo xtask parity sim`, `audio-data` | Simulation traces identical (section 4.6); generated audio arrays within tolerance (section 7.5) |
| L3 | Structural goldens | `cargo xtask parity world`, `textures` | Geometry digests and textures within tolerance (section 5.7) |
| L4 | Behaviour and pictures | e2e suites on the Rust build; `cargo xtask parity shots`, `materials`, `audio-render` | e2e green; picture and audio metrics within thresholds |
| L5 | Owner sign-off | Side-by-side report; play test on the phone | Owner approves |

**Reference capture.** `tools/parity/` drives the JS game in headless Chrome
(the existing harness) and in Node and writes goldens: simulation traces,
Track and terrain dumps, scene exports, texture images, material test
renders, audio buffers and offline renders, and screenshots. Small goldens
are committed. Large ones are regenerated on demand and cached by the hash
of the JS tree.

To make that possible the JS game gains **parity hooks** during M0. A hook
must change nothing when it is not switched on, and the JS suites must still
pass with it in place. The hooks: a fixed-dt loop with several ticks per
frame; replacement of the `Math` functions by the shared kernel; optional
random-generator parameters where the simulation draws; a state recorder;
quantised inputs; frozen traffic and particles for screenshots; a
`MaterialKind` tag on every material and access to the shader patch
functions for the material test scenes; the scene exporter; an audio call
log. When M0's last capture tool is working, the JS is tagged `js-reference`
and frozen. Until then the hooks may touch any JS file that needs one.

**Screenshots.** For each level, camera stations every 250 m (chase view and
a high view) at the route's own time of day, plus the menu's attract view,
using the fly-camera parameters both builds accept. Moving things (traffic,
particles) are frozen or hidden for the comparison. The metric is colour
difference (CIEDE2000) on quarter-resolution images: mean under 3, and 95 %
of 16-pixel blocks under 6. Before using these numbers, M0 measures the
JS-against-JS noise floor and raises a threshold to twice the floor if
needed. The report shows each pair with a difference map, sorted worst
first.

**Performance.** `cargo xtask perf` runs the autopilot through each level
uncapped with CPU throttling, as the JS project already does, and compares
frame time, draw calls and triangles with the JS baseline. Phone numbers are
read from the stats overlay by the owner.

## 13. Deviations register (initial)

| Deviation | Reason |
|---|---|
| Fixed 120 Hz tick with interpolation, for all subsystems | Determinism, multiplayer, RL. JS steps AI and traffic once per frame. |
| Seeded random streams instead of `Math.random` | Determinism |
| Per-frame random effects become rates at 60 Hz | Same look at any display rate |
| Points drawn as instanced quads | No point size in WebGPU |
| Bundled fonts for generated textures and HUD | Same signs on every device; no web font request |
| UI drawn in-engine; some CSS effects approximated | One UI for native and web |
| Native audio through a Rust Web Audio implementation | One graph definition; small differences in compressor and oscillator behaviour |
| Music player is a screen in the game, not a second page | One wasm app |
| Seaside survey data in a binary file | No base64-in-JS loader |

Known JS quirks to reproduce, not fix, unless the owner says otherwise: all
headlight pools share one material, so one car's opacity wins
(`Effects.js:249,273`); the shadow box is said to snap to texels but does
not (`Sky.js:271`); `input.enabled` is never set false.

## 14. Risks

| Risk | Likelihood | Effect | Mitigation |
|---|---|---|---|
| Safari's WebGPU has bugs that break the game on iPhone (open reports of strobing output, rejected frames, tab kills) | Medium | High | Gate G1 on the owner's iPhone with a real exported level; WebGL2 build of the same client as the fallback, kept compiling until cutover |
| Bevy on phones is too slow or too big | Medium | High | Same gate; fallback renderer on the same scene data |
| Requiring WebGPU shuts out some players: iPhones not updated to iOS 26, Firefox on Linux and Android, older Android | Certain | Low to medium | Accepted by the owner. A clear message on unsupported browsers. The WebGL2 fallback build could be shipped for them if it matters later. |
| A LAN game needs https, so the host needs a certificate or the internet | Certain | Medium | Section 9.5: certificate for the first release, WebRTC through a signalling service for the second |
| Wasm memory growth across level switches gets tabs killed | Medium | High | CPU mesh copies dropped; memory high-water mark in the budgets; ten-switch test |
| Shader compile hitches in the first seconds of a race | High | Medium | Warm-up pass; hitch gate |
| Bevy upgrade churn stalls work | High | Medium | Pinned version; no plugins; shaders, UI widgets and post chain each in one module |
| Scenery port drifts from JS in ways nobody notices | Medium | Medium | Geometry digests and textures compared automatically, not by eye |
| Traces cannot be made bit-identical | Low | Medium | Both sides use one math kernel (section 4.2); WP 1.3 proves it on the physics alone before the rest of the simulation is ported. If it fails, fall back to tolerances on short staged traces and distributions over seeds for whole races, and record why |
| Native audio differs audibly | Medium | Low | Offline band comparisons; tune trims per backend |
| Bevy UI cannot reach the DOM version's polish | Medium | Medium | Owner reviews the menu and HUD early in M6; deviations listed |

## 15. Open questions for the owner

1. Font substitutes for the generated signs (section 5.3): pick from the
   gallery in M3.
2. Should the JS quirks listed in section 13 be fixed in the port?
3. Rival rubber-banding with several humans (section 9.6).
4. Which phones are the reference devices, and who reads their numbers at
   each gate. They must be on iOS 26+ and Android 12+.
5. For LAN games with guests who are not on the tailnet: is a public DNS
   name with a certificate acceptable, or should guests wait for the WebRTC
   release (section 9.5)?
6. Is "Midnight Racer" still the name once it is a suite? Crate names use
   `mr_` either way.

## Appendix A. Ecosystem facts (checked 2026-10-03)

| Item | Fact |
|---|---|
| Bevy | 0.19.1 (2026-08-13). 0.20 at rc.2 (2026-09-28); replaces the shader import dialect with WESL; deprecates the old UI `Button`/`Interaction`. |
| Bevy on the web | WebGPU and WebGL2 are separate compile-time features, so supporting both means two wasm builds. No wasm threads. Pipeline compilation is synchronous on wasm. On WebGL2 only: one directional light, one shadow cascade, no compute-based features, clustered lights capped at 204. |
| Bevy wasm size | Third-party builds on 0.19: 28 to 38 MB raw, 6.5 to 8.3 MB brotli after trimming features. |
| WebGPU | Chrome and Edge 113+ on Windows, macOS and ChromeOS; Android 12+ from Chrome 121; Linux on recent Intel and NVIDIA from Chrome 144 to 148. Safari 26 on macOS, iOS and iPadOS (September 2025; iPhone 11 and newer can run it). Firefox on Windows and Apple Silicon Macs only. Apple reported iOS 26 on 79 % of all iPhones and 86 % of those from the last four years in June 2026. |
| Safari WebGPU bugs open | Bevy #23753 (strobing output, no fix), Bevy #23956 (deferred rendering fails; the port uses forward), wgpu #9907 (long float literals rejected by WebKit's shader lexer), wgpu #10460 (staging-belt frames rejected, September 2026), wgpu #3735 (tab kills). |
| Secure-context-only APIs | WebGPU, WebTransport, DeviceOrientation, AudioWorklet, Gamepad (per MDN). `http://192.168.x.x` is not a secure context. Plain `AudioContext`, WebSocket and WebRTC are not restricted. |
| iPhone | No Fullscreen API. No gamepad haptics in Safari. |
| Gamepad rumble | Not available through Bevy's gamepad backend on wasm. |
| `web-audio-api` crate | 1.7.0. Native implementation of the Web Audio API. |
| Rapier | 0.36.0 (2026-09-24). Ray-cast vehicle controller; cross-platform determinism feature; usable without Bevy. |
| Avian | 0.7.0. No vehicle controller; tied to Bevy's ECS. |
| `matchbox_socket` | 0.14.0. WebRTC data channels, native and wasm, needs signalling; not tied to Bevy. |
| WebTransport | Safari 26.4 (March 2026), including certificate hashes; interop with Rust servers still being fixed. Not used in this plan. |
| GitHub Pages | Cannot set COOP/COEP headers, so no wasm threads there without a service-worker workaround. Not needed by this plan. |

Not verified: any first-hand report of a Bevy 3D game on iPhone Safari, on
either backend; whether browsers enforce the secure-context rule for
gamepads. G1 answers the first.
