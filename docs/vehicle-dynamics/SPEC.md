# Vehicle dynamics: sim handling, tyre models and soft-body tyres

Status: **design only.** Nothing here is built. Work starts after the Rust
port reaches cutover (ROADMAP M9), except the seams in section 9, which may
land earlier only if they leave every parity trace bit-identical.

This is the design document that ROADMAP "M12 and beyond" row e (realistic
physics) asks for, widened to cover off-road crawlers and the RL work in
SPEC section 10. It replaces SPEC 4.5's sketch of "a rigid-body car on
Rapier" (decision VD-1 below says why).

## 1. Goals

The game began as an arcade street racer and is becoming a racing game with
more than one way to drive. Three vehicle models, one interface:

| Tier | Feel | Used for | Cost |
|---|---|---|---|
| **0. Arcade** | Today's handling (`CarPhysics`) | Default on every platform; all rivals, police and traffic | Microseconds per tick |
| **1. Sim** | Assetto Corsa class: per-wheel tyre forces, suspension, weight transfer, drivetrain | The player's car when "Sim handling" is on; desktop first, phones if the budget holds | Tens of microseconds per tick |
| **2. Soft tyre** | BeamNG class tyres: node-and-beam tyres that wrap over rocks, on a rigid chassis | Rock crawlers and other off-road vehicles; RL environments | About a millisecond per tick |

What each tier must deliver:

- **Tier 1** gives trail-braking, lift-off and power oversteer, understeer,
  weight transfer, wheelspin and lock-ups, all from the physics, with nothing
  scripted. It reads a real wheel and pedals, gives force feedback through
  them, and comes with an assist stack that makes it drivable on a keyboard,
  a gamepad and a phone.
- **Tier 2** gives tyres that deform under load and over obstacles, with low
  pressures, articulating solid axles, lockers and low range. The terrain is
  more than the road ribbon.
- **All tiers** are deterministic, headless, steppable as a library, and
  usable as RL environments, including vehicles that are robots rather than
  cars (skid-steer, a motor per wheel).

Out of scope: a fully soft-body chassis (BeamNG crash deformation), tyre
thermals and wear (section 4.6 keeps room for them), and a Pacejka coefficient
library.

### 1.1 Principles

1. **Tier 0 is untouched.** Arcade handling stays bit-identical to the port's
   goldens. A sim car sits beside it and never changes it.
2. **Same rules as the simulation core** (port SPEC 4.2). The code is a
   library with no engine, clock, threads or global state. All values are
   `f64`, and every inexact function goes through `mr_math`'s kernel: no
   `f64::sin`, `powi` or `mul_add`. Results are bit-identical on native and
   wasm, so multiplayer rollback, replays and RL seeds keep working.
3. **One wheel interface.** The chassis, suspension and drivetrain are shared.
   The tyre is a component behind a single interface, so the rigid brush tyre
   (Tier 1) and the soft tyre (Tier 2) are swappable on the same car. This is
   the decision that stops Tier 2 from tearing up Tier 1.
4. **Vehicles are data.** Wheels, axles, drivetrain and tyres are described
   by a definition, not hard-coded to four wheels and one engine, so a
   six-wheeled robot or a skid-steer rover is a new definition, not new code.
5. **Measured, not assumed.** Every performance claim in this document is a
   target to measure on the reference devices. Tier 1 rivals and Tier 1 on
   phones are decided by those numbers.

## 2. Where the code lives

```
mr_math ← mr_vdyn ← mr_terrain
mr_math ← mr_track ← mr_levels ← mr_sim   (mr_sim also uses mr_vdyn, mr_terrain)
mr_worldgen also reads mr_terrain, to draw it
```

- **`mr_vdyn`** (new) holds the vehicle dynamics: the rigid body, the
  suspension, the drivetrain, the tyre models, the assists and the `Ground`
  trait. It depends only on `mr_math`. It knows nothing about tracks, races
  or levels. It has the same rules as the other simulation crates:
  `#![forbid(unsafe_code)]`, no clock, threads, `rand` or hash maps, and a
  rule in `xtask/src/deps.rs`.
- **`mr_terrain`** (new, at milestone V5) holds collision ground beyond the
  road ribbon: height fields, rock primitives and surface materials. It
  depends on `mr_math` and `mr_vdyn` (it implements `mr_vdyn::Ground`);
  `mr_vdyn` never depends on it. `mr_worldgen` reads `mr_terrain` to *draw*
  that ground. The rule matches levels today: **collision geometry is
  generated on the simulation side, and world generation dresses it. The
  simulation never depends on world generation.**
- **`mr_sim`** gains an adapter that implements `Ground` for `Track`, and a
  `VehicleModel` enum on `PlayerCar` (section 8).
- **`mr_py`** (already planned in port SPEC 10) wraps environments built on
  any tier.

## 3. Time

The game tick stays at **1/120 s**: the race rules, AI, networking and
rendering interpolation do not change. A sim vehicle **substeps** inside
the tick:

- Each vehicle definition sets its substep count `n` (an integer), so
  `h = (1/120) / n`. The stiffest component decides it.
- Starting values, to be tuned by measurement:
  - Tier 1: `n = 5` (600 Hz). Assetto Corsa runs at 333 Hz; wheel hop at
    12–15 Hz and the tyre relaxation dynamics are well resolved at 600 Hz.
  - Tier 2: `n = 20` to `32` (2.4–3.8 kHz), set by the stability limit of the
    stiffest tyre beam (section 5.4). BeamNG runs at 2 kHz.
- Inputs are held constant across substeps. Events are gathered and emitted
  once per tick, as now.
- Integration is **semi-implicit (symplectic) Euler**: update velocity from
  forces, then position from the new velocity. Orientation is a unit
  quaternion integrated with angular velocity and renormalised every substep.
  That needs only `+ − × ÷` and `sqrt`, all of which are exact, so the rigid
  body needs no kernel calls at all.

## 4. Tier 1: the sim car

### 4.1 Chassis

- One rigid body with six degrees of freedom: position, orientation
  quaternion, linear and angular velocity, mass, inertia tensor (diagonal in
  body axes to start), and centre of mass.
- Aerodynamics: drag `½ρCdA v²` at the centre of pressure, and downforce
  per axle as `C_L,front` and `C_L,rear`. Downforce matters to the
  feel of the super car.
- Walls: the road corridor walls stay. The chassis collides with them as a
  box: each corner is tested against the wall planes and pushed back with a
  penalty spring and damper and Coulomb friction. A sim car that hits a wall
  should spin as a real car would, not by the arcade's scripted rules in
  `collide_walls`.

### 4.2 Suspension

Each axle is one of:

- **Independent.** Each wheel's hub moves along a fixed axis in the body
  frame (strut-like), with a spring, a damper (separate bump and rebound
  rates), travel limits and bump stops. Camber and toe are static angles to
  start with.
- **Solid axle** (V5, crawlers). The axle is a body with heave and roll
  relative to the chassis, held by links that are reduced to a roll centre
  and a pivot. It carries both wheels. This gives articulation, which
  crawlers need.

Plus an **anti-roll bar** per axle (torsion stiffness between the two sides).

Hubs carry **unsprung mass**, so wheel hop and kerb strikes behave. A
`Geometry` enum keeps room for real linkages (double wishbone points, with
camber gain and bump steer) later, without changing the interface.

### 4.3 Wheels and the tyre interface

A wheel is a hub (pose and velocity from the suspension), a spin rate `ω`
about the axle, a spin inertia, a steer angle, a brake, and a **tyre**. The
tyre interface is the most important boundary in this document:

```rust
/// What the hub tells the tyre at the start of a substep.
pub struct HubState {
    pub pose: Iso3,        // hub position and orientation (axle along local y)
    pub vel: Vec3,         // hub linear velocity, world frame
    pub ang_vel: Vec3,     // hub angular velocity excluding spin, world frame
    pub omega: f64,        // spin rate about the axle (rad/s)
}

/// What the tyre gives back: the force and moment it applies to the hub
/// (world frame, about the hub centre), the torque about the spin axis
/// that the road returns, and readouts for audio, FX, telemetry and FFB.
pub struct TyreOutput {
    pub force: Vec3,
    pub moment: Vec3,
    pub spin_torque: f64,
    pub info: ContactInfo, // load, slip angle, slip ratio, sliding share,
                           // aligning moment, contact point, surface id
}

pub enum Tyre {
    Brush(BrushTyre),      // Tier 1
    Soft(SoftTyre),        // Tier 2
    // later: MagicFormula(MfTyre)
}

impl Tyre {
    pub fn step(&mut self, hub: &HubState, ground: &dyn Ground, h: f64) -> TyreOutput;
}
```

`Tyre` is an enum, not a trait object, so the state stays `Clone +
PartialEq` and hashable for rollback and desync checks (port SPEC 4.3).

The wheel integrates `ω` from drive torque, brake torque and
`spin_torque`. **Brakes are friction, not a negative torque.** If the brake
can absorb everything else acting on the wheel and `ω` would cross zero, `ω`
is set to zero for that substep. Without this rule, locked wheels chatter
and a parked car creeps.

### 4.4 The brush tyre (Tier 1's tyre model)

The physical brush model (Pacejka, *Tyre and Vehicle Dynamics*, chapter 3):
a contact patch of bristles with a parabolic pressure distribution. It is
chosen over the Magic Formula for v1 because its parameters mean something
(stiffness, patch length, friction), so a believable car can be tuned
without measured tyre data, and it gives combined slip and an aligning
moment from first principles.

- **Slip.** Theoretical slip `σx = κ/(1+κ)` and `σy = tan α/(1+κ)`, with
  `κ` from the transient state below.
- **Force.** With cornering stiffness `C = 2·c_p·a²` (bristle stiffness per
  length `c_p`, patch half-length `a`), `θ = C / (3·μ·Fz)` and `s = |σ|`:
  `F = μ·Fz·(3θs − 3(θs)² + (θs)³)` while `θs < 1`, otherwise `μ·Fz`,
  acting against the slip direction. One isotropic stiffness to start;
  separate longitudinal and lateral stiffness later.
- **Load sensitivity.** `μ = μ0·(1 − k_μ·(Fz/Fz0 − 1))`, clamped to stay
  positive. This is what makes weight transfer cost grip, and what makes
  anti-roll bars change the balance.
- **Sliding friction.** A separate sliding friction (`μ_slide < μ0`, falling
  with slip speed) gives the peak and drop-off that drifting needs.
- **Vertical.** A spring and damper from the hub to the ground (tyre
  vertical stiffness), so the contact load `Fz` is dynamic.
- **Aligning moment.** The brush model's own `Mz` (its pneumatic trail),
  plus mechanical trail from the kingpin geometry. The steering force
  feedback comes from this (section 7.2).
- **Transient and low speed.** Slip is not computed by dividing by wheel
  speed. Each tyre holds a lateral and a longitudinal **carcass deflection**
  state that relaxes over a relaxation length `σ_α` and `σ_κ`:
  `dy/dt = v_sy − |V_x|·y/σ_α`, with `α′ = atan(y/σ_α)`, and the same for
  the longitudinal direction. At a standstill this becomes a spring with a
  low-speed damping term, so a parked car holds on a slope and does not
  jitter. That matters doubly for crawlers at walking pace.
- **Rolling resistance:** `C_rr·Fz`, against the direction of spin.
- **Contact.** By default one ray from the hub along the suspension axis.
  An option of several sample points across the patch (multi-point contact)
  smooths kerbs and steps, and is the cheap off-road tyre for phones
  (section 5.6).

Room is kept in the tyre state for temperature and wear (one surface and
one carcass temperature per tyre, and a wear value), but they are not
modelled in v1.

### 4.5 Drivetrain

A small graph from power sources to wheels, defined in data:

- **Engine:** a torque curve (table over rpm), idle and limiter, engine
  braking, flywheel inertia, turbo lag (the arcade's `boost` behaviour is a
  good start). **Electric motor:** a torque and power envelope, regeneration
  as negative torque, and per-wheel motors for robots.
- **Clutch:** friction-limited torque transfer, so the engine can stall and
  the car can launch. The automatic modes drive it.
- **Gearbox:** ratios and final drive, with a shift time. Modes: automatic
  (the arcade's shift logic, as an assist), sequential paddles, and an
  H-pattern with a clutch later.
- **Differentials:** open, locked, clutch-pack LSD (preload plus ramp
  angles), viscous. A centre differential and a transfer case with a low
  range for 4WD. Selectable lockers for crawlers.
- The graph is solved each substep. The rigid shafts are collapsed into
  equivalent inertias; slipping clutches and differentials pass torque.

### 4.6 Steering

Steering wheel angle → steering ratio → road-wheel angle, with per-car
lock, Ackermann (a percentage), speed-sensitive assistance (power steering
affects force feedback, not the angle), and steering rate limits for
digital input. With a real wheel, the input is the **wheel angle in
degrees**, not a −1..1 command, and the car's lock range maps 1:1 onto the
physical wheel (section 7.1).

### 4.7 What a sim car hands to the race

Race rules, collisions, AI awareness, the HUD and audio read the arcade car
through `Vehicle` and a few `CarPhysics` fields. A sim car **writes the same
body view** every tick, so nothing downstream changes:

- `Vehicle`: `x y z yaw vx vz vy yaw_rate s lat speed steer_angle
  accel_long accel_lat on_ground brake_light vis_y`. `yaw` is the heading
  of the chassis's forward axis projected onto the ground plane.
- From `CarPhysics`, the fields the race and client read today: `gear`,
  `rpm`, `nitro`, `nitro_active`, `drifting`, `drift_time`, `slip`, `skid`,
  `scrape`, `scrape_side`, `events`, `locked`, `damage`, `spiked`, `boost`,
  `power_out`, `electric`. `drifting` and `slip` come from the rear axle's
  slip angle with the same thresholds as the arcade (0.18 rad on, 0.06 rad
  off), so drift scoring carries over. Nitro is a temporary torque boost;
  `spiked` scales `μ0`; `locked` holds the car.
- **Collisions** (`mr_sim::collisions`) work on planar velocity and spin.
  For a sim car, the change in planar velocity and yaw rate that the
  collision pass computes is applied back to the rigid body as an impulse
  at the centre of mass plus a yaw impulse. That is approximate but
  symmetric, and good enough for contact with arcade rivals.
- New render-side data (section 8.3) is added beside the body view, not in
  place of it.

### 4.8 Assists ("arcade steering" on a sim car)

Sim handling on a keyboard, a pad or a phone needs help. The assists are a
layer between `InputFrame` and the vehicle. Each one is a setting, and each
one can be off:

- **Steering:** a grip-scaled lock (the arcade's `ANALOG_LOCK` idea: full
  stick asks for slightly more than the front tyres can give at this
  speed), a speed-sensitive rate limit for digital input, and a
  counter-steer assist that steers toward the velocity vector in a slide.
- **Stability control:** brakes individual wheels to stop yaw rate running
  past the target yaw rate.
- **Traction control and ABS:** slip-ratio targets per axle.
- **Gears and clutch:** automatic shifting and an automatic clutch.
- **Presets:** *Casual* (all on, close to arcade feel), *Assisted* (ABS, TC
  and auto shift), *Sim* (none, as in Assetto Corsa).

The assists see only what a car's electronics could see (wheel speeds, yaw
rate, steering angle, lateral acceleration), so they read as real systems.

## 5. Tier 2: soft-body tyres on a rigid chassis

### 5.1 Scope

The chassis, suspension (usually solid axles), drivetrain and wheels are
Tier 1's. Only the `Tyre` changes: `Tyre::Soft`. The first target is a
plain **balloon tyre**: large, low pressure, no tread blocks. After that
comes the owner's own tyre, designed in Onshape (section 13).

### 5.2 Structure

Built from parameters (outer radius, rim radius, width, pressure, sidewall
stiffness, tread stiffness), not authored by hand:

- **Rings of nodes** around the circumference: two bead rings fixed to the
  rim, and two or three tread rings. Start with 24 nodes per ring, as a
  parameter.
- **Beams** (spring plus damper, rest length, optional compression and
  tension limits): sidewall beams from bead to tread, circumferential tread
  beams, diagonal shear beams between rings, and a bending term along the
  tread so it does not fold.
- **Bead nodes are kinematic.** They follow the rim's pose and spin. Every
  beam force on a bead node is summed into the force, moment and spin
  torque returned through `TyreOutput`. This is how the soft tyre plugs into
  the same wheel as the brush tyre.

### 5.3 Pressure

The tyre's surface is a closed triangle mesh (tread rings, sidewalls and the
rim as the inner wall). Each substep:

- compute the enclosed volume `V` with the divergence theorem;
- set `p = p0·V0/V` (isothermal; a polytropic exponent is a later option);
- apply `p·A·n` on each triangle, split over its three nodes.

Pressure is a gameplay setting: airing down for rocks is the point of a
crawler.

### 5.4 Integration and stability

Explicit semi-implicit Euler, like the rest, at the vehicle's substep. The
stability limit is roughly `h < 2/ω_max` with `ω_max = sqrt(k/m)` for the
stiffest beam on the lightest node (with damping, keep a margin of 2). For
example, a 0.2 kg node on a 10⁵ N/m beam gives `ω ≈ 700 rad/s`, so `h` must
stay well under 2.8 ms. 2–4 kHz is comfortable. If that proves too costly,
the fallback is **XPBD** (position-based dynamics with compliance), which is
stable at larger steps but makes friction less physical. Decide that by
measurement at V6, not now.

### 5.5 Contact and friction

- Each tread node is tested against the ground's **signed distance**
  (section 6). Penetration gives a normal penalty force (spring plus
  damper) along the ground normal.
- **Friction is a per-node bristle.** When a node touches down it stores an
  anchor point on the surface. A tangential spring pulls it toward the
  anchor, capped at `μ·N`; past the cap the anchor slides along with the
  node. This is the brush model done node by node: static friction holds a
  crawler on a steep face, and sliding emerges when the cap is reached.
  The anchors are part of the tyre state.
- Surface materials (section 6) give `μ`, and later deformable ground (sand,
  mud) changes the normal response.

### 5.6 Cost and the in-between option

Rough count: 4 tyres × about 100 nodes × 3 kHz is about 1.2 M node updates
a second, plus the beams. That is a fraction of a millisecond per tick
natively. Target: under 1 ms per tick for a crawler on the desktop
reference, then measure wasm and phones.

For phones, the in-between option is a **multi-point rigid tyre**: a brush
tyre whose contact samples a grid of points over a rigid torus against the
ground's SDF. It is cheap and climbs rocks plausibly, but does not deform.
It is the same `Tyre` interface, so a crawler can ship with soft tyres on
the desktop and multi-point tyres on a phone.

### 5.7 State size

A soft tyre holds about 100 nodes × (position and velocity, 6 × f64), plus
the anchors: about 6 KB per tyre and 25 KB per crawler. That fits the port's
32 KB state budget for a single player, but makes multiplayer rollback with
several crawlers expensive. Tier 2 is single-player and RL first.

## 6. Ground

Port SPEC 4.5 asked for the track to sit behind an interface. It is defined
here:

```rust
pub trait Ground {
    /// Height and normal under (x, z), and the surface there.
    /// Enough for a single-ray brush tyre.
    fn height(&self, x: f64, z: f64) -> GroundSample;
    /// Signed distance from p to the surface (negative inside),
    /// with the outward normal and the surface. For soft tyres and
    /// multi-point contact.
    fn sdf(&self, p: Vec3) -> SdfSample;
    /// Static obstacles near a box (walls, rocks), for chassis contact.
    fn colliders(&self, aabb: Aabb, out: &mut Vec<Collider>);
}

pub struct Surface { pub mu: f64, pub rolling: f64, pub loose: f64, pub id: u16 }
```

- **`TrackGround`** (in `mr_sim`) wraps `Track`: `project`, `surface_y`,
  the frame's bank and grade for the normal, `loose_at` for `loose`, and the
  corridor walls as colliders. The SDF of a ribbon is its height difference
  along the normal, which is close enough within the corridor.
- **`mr_terrain`** (V5) adds height fields with bilinear height and an
  analytic normal, rock primitives (spheres, capsules, convex hulls) unioned
  into the SDF, and materials per region. The crawler test area is
  generated there from a seed, and `mr_worldgen` draws it.
- **Rapier** is not used for vehicles (decision VD-1). It stays available
  later as a broad-phase and collider backend behind `Ground::colliders` for
  open-world geometry, if writing our own stops being cheaper.

## 7. Input and force feedback

### 7.1 Input

`InputFrame` grows when Tier 1 lands (the network protocol is versioned, so
this is a version bump):

- `steer: i16` keeps its range. In sim mode with a wheel it means the
  physical wheel angle as a fraction of the device's calibrated range,
  which is 0.03° resolution on a 900° wheel.
- New `clutch: u8`.
- New flag bits: shift up (16) and shift down (32), both edges. Later, a
  byte for an H-pattern gear.
- The steering ramp for digital input stays in the input layer, as now. The
  sim assists (section 4.8) sit after it.

### 7.2 Force feedback

The sim computes the **rack force** each substep: the sum of the steered
tyres' aligning moments and the mechanical trail, through the steering
geometry. It is reduced to a **steering-column torque**.

A 120 Hz stream of raw torque across USB is too slow and too jittery for a
stiff, direct-drive wheel to feel good. The split is:

- **The game sends, each tick,** a target column torque plus slowly
  changing effect parameters: the column's spring centre and stiffness (the
  soft lock at the car's lock), damping, friction, and the estimated rack
  stiffness for the device to interpolate with.
- **The device runs its own 1 kHz+ loop** with those parameters, adding the
  game's torque as a feed-forward term. That keeps it stable and
  low-latency whatever the frame rate.

`mr_game`'s `Rumble` trait becomes a `Feedback` trait with a rumble channel
(as now) and a force channel. Backends:

- **Native:** the owner's controller through hidapi (7.3), and standard
  wheels through SDL or gilrs force feedback where they support it.
- **Web:** WebHID on desktop Chrome. It is secure-context only, which the
  tailnet front already provides.

### 7.3 The owner's wheel

The owner's wheel is their own design: a motor on their own motor
controller, driven by an **RP2350**. Its firmware lives in **a separate
repository**; this repo only owns the game's side of the protocol. The
firmware presents one composite USB HID device with two faces:

- **Standard HID PID force feedback** (the USB "Physical Interface Device"
  usage page): a steering axis, pedal axes and buttons, and the standard
  effects (constant force, spring, damper, friction, periodic). Windows
  DirectInput and Linux's `hid-pidff` driver then treat it as an ordinary
  force-feedback wheel, so other games (Assetto Corsa under Proton, for
  example) work with it. The PID report descriptor is large and fussy, and
  getting it to compile and enumerate is what has made this hard before.
  OpenFFBoard's descriptor is the reference to start from. Check
  `hid-pidff`'s behaviour on the owner's kernel early: its support for
  direct-drive wheels improved recently but has a history of quirks.
- **A vendor-defined report** for this game: per tick, the target column
  torque and the effect parameters of 7.2, versioned. The game uses it
  through WebHID in the browser and hidapi natively, with no driver. It
  also reports the wheel angle at full sensor resolution.

The firmware's own loop closes the torque and effect loop at 1 kHz or
more. The game never depends on the PID face, and the PID face never
depends on the game.

## 8. How it fits the existing simulation

### 8.1 The vehicle model on `PlayerCar`

```rust
pub enum VehicleModel {
    Arcade(CarPhysics),
    Sim(Box<SimCar>),   // mr_vdyn's vehicle plus the body-view writer
}
```

`PlayerCar.phys` becomes this enum. `Arcade` runs exactly today's code
path, so the goldens do not move. Rivals keep `AiDriver` and kinematic or
arcade physics. Running rivals on Tier 1 is an experiment behind a flag
(open question 1); it needs an AI that drives through the assists.

### 8.2 Vehicle definitions

`mr_vdyn::VehicleDef`: chassis mass, inertia, centre of mass and aero;
axles (independent or solid, wheels, steering, anti-roll bar); the
drivetrain graph; a `TyreDef` per axle; the substep count; and the default
assist preset. Definitions start as Rust data in a `mr_vdyn::cars` module
(like `CAR_SPECS`), one per arcade car so "Sim handling" works on every car
in the garage. A text format (RON) for user and owner-supplied vehicles
comes at V6.

### 8.3 What the client draws

Today the body's pitch and roll are client springs (port SPEC 4.3). For a
sim car they are real state. The render view gains, beside the body view,
an optional:

- chassis orientation (quaternion);
- per wheel: hub position, steer angle, spin angle, suspension travel;
- per soft tyre: node positions, so the tyre mesh is skinned to them and
  interpolated between ticks like everything else.

The client uses them when present and its own springs otherwise.

### 8.4 Telemetry

A sim car exposes per-tick telemetry (per-tyre load, slip angle, slip
ratio, sliding share, rack force, suspension travel, wheel speeds) through
the headless CLI (`mr-sim race --trace`) and an in-game overlay. Tuning a
tyre model without it is guesswork.

## 9. Breadcrumbs: what to keep in mind before this starts

These are cheap now and expensive later. None of them may change a parity
trace.

1. **Do not reach further into `Track` from physics.** New code in
   `mr_sim::physics` should not read more track fields than the JS does,
   so the `TrackGround` adapter stays small.
2. **`PlayerCar.phys` will become an enum.** Code outside `mr_sim` should
   read player car data through `Vehicle` and the `CarPhysics` fields
   listed in 4.7, not new ones, where it can.
3. **Version the input and the network protocol** with room for a clutch
   byte and two more flag bits (7.1).
4. **Keep body pitch and roll an input to the renderer**, not hard-wired to
   the client springs, so a sim orientation can replace them (8.3).
5. **Make rumble a trait with room for a force channel** (7.2) when the
   input port touches it.
6. **Collision ground belongs to the simulation side.** Any free-roam or
   off-road ground added for other reasons (expansion track b) goes in a
   simulation-side crate that worldgen reads, never the reverse (2).
7. **The RL `Env`** (port SPEC 10) should take a vehicle model choice from
   the start, and its observation builder should read the body view, not
   `CarPhysics` internals.

## 10. Testing

There is no JS oracle for any of this, so other oracles replace it:

- **Unit tests against formulas:** the brush force curve and its slope at
  zero (`C`), the sliding limit, combined slip on the friction ellipse,
  load sensitivity, the relaxation response to a slip step (time constant
  `σ/V`), the soft tyre's volume and pressure.
- **Vehicle tests against textbook behaviour:** a steady-state skidpad
  (lateral acceleration against the tyres' `μ`, the understeer gradient
  against the linear bicycle model in the linear range), straight-line
  braking distance against `v²/(2μg)` with ABS, a step steer with the
  expected yaw response, acceleration-limited launches.
- **Stillness tests:** a car parked on a 20 % grade with the brake on
  holds still for 60 s (no creep, no jitter); a crawler resting on a rock
  stays put; energy does not grow in a frictionless coast.
- **Determinism:** the state hash after N ticks matches between native and
  wasm, in CI, for each tier.
- **External reference (offline):** Project Chrono's vehicle module
  (TMeasy and Pacejka tyres, deformable terrain) as a sanity check for the
  crawler and soft tyre: same vehicle, same obstacle, compare loads and
  trajectories by eye and within broad tolerances. It is not a parity
  target.
- **Owner's drives:** a sim build is not done until the owner has driven it
  on the wheel and signed off the feel, as with the port's visual gates.

## 11. Milestones

Each milestone ends with tests passing, a headless run, and (from V2) a
drivable build.

| | What | Gate |
|---|---|---|
| **V0. Seams** | `Ground` trait and `TrackGround`; `VehicleModel` enum with only `Arcade`; render-view fields; input and protocol version with the new fields unused | Every parity golden bit-identical; `check-deps` rules for `mr_vdyn` |
| **V1. Core** | `mr_vdyn`: rigid body, independent suspension, wheel spin with brake friction, brush tyre with relaxation, flat-ground test rig, telemetry | Section 10 unit and vehicle tests on flat ground; native and wasm hashes equal |
| **V2. Sim car on track** | One car (Vento GT) as a `VehicleDef`, "Sim handling" setting for the player, the automatic gearbox and *Casual* assists, wall contact, collisions | Drivable on keyboard and pad on desktop; owner drive |
| **V3. Wheel and FFB** | Wheel-angle input, clutch, paddles, rack force, the `Feedback` force channel, the owner's controller backend | Owner drive on the wheel with assists off |
| **V4. Every car, every device** | `VehicleDef`s for all garage cars, drivetrain variety (LSD, AWD, electric), assist presets, phone measurements, Tier 1 rivals behind a flag | Budgets measured and written down; decision on Tier 1 for phones and rivals |
| **V5. Off-road ground** | `mr_terrain` (height field, rocks, materials), a crawler test area, solid axles, lockers and low range, multi-point rigid tyre | A crawler climbs the test rocks on rigid tyres; still on slopes |
| **V6. Soft tyre** | `SoftTyre`: rings, beams, pressure, per-node bristle friction, skinned tyre mesh; then the owner's model | Balloon tyre wraps over a rock and holds the crawler still; per-tick cost measured |
| **V7. Crawler RL** | `Env` for the crawler (observations: IMU, wheel speeds, suspension travel, height-map patch; actions: throttle, steer, lockers, or per-wheel torque for robots), seeded domain randomisation of tyre and terrain parameters, `mr_py` vector env | A baseline policy learns to climb a course |

Later: a Magic Formula tyre behind the same enum, tyre temperatures and
wear, real suspension geometry, deformable ground (sand, mud), and a soft
chassis.

## 12. Decisions taken here

- **VD-1. Own vehicle solver, not Rapier.** Port SPEC 4.5 suggested Rapier's
  ray-cast vehicle controller. A vehicle is a handful of bodies with
  special-purpose constraints (suspension axes, spin, drivetrain), and the
  feel lives in exactly the parts a general engine abstracts away. Our own
  `f64` code on `mr_math`'s kernel is deterministic by the same rules as the
  rest of the simulation, with no extra engine to audit. Rapier has no soft
  bodies, so Tier 2 would need our own code anyway. Rapier may come back as
  a collider backend for open-world ground (section 6).
- **VD-2. Brush tyre first, Magic Formula later.** Covered in 4.4.
- **VD-3. Substeps inside the 1/120 s tick**, per vehicle definition, rather
  than a faster game tick (section 3).
- **VD-4. One `Tyre` enum for rigid and soft tyres,** behind the hub
  interface (section 4.3).
- **VD-5. The game sends torque plus effect parameters to the wheel, and
  the device closes the fast loop** (section 7.2).
- **VD-6. Tier 1 is player-only to start.** Rivals stay arcade until V4's
  measurements say otherwise.

## 13. Owner's answers and open questions

Answered (2026-10-06):

- **Tier 1 is player-only first**, rivals on arcade. Tier 1 rivals are to
  be tried on the desktop to measure the cost, behind a flag (V4).
- **The force-feedback wheel** is the owner's own RP2350 controller, made
  into a proper HID device in a separate firmware repository (7.3).
- **The owner's tyre model is in Onshape (CAD).** For V6 the soft tyre
  builder takes a cross-section profile (tread, shoulder and sidewall
  outline, exported from Onshape as a sketch or a section), plus width, rim
  size and tread layout, and sweeps the profile round the rim to place its
  node rings.
- **Sim handling is a toggle on every car**, with each car getting a
  physical `VehicleDef` and a "racing tune" of assists by default (4.8).
- **Crawlers** use soft tyres on a rigid chassis (Tier 2 as written).
- **Timing:** nothing here starts before the port's cutover.

Still open:

1. **AI and assists:** if rivals ever run on Tier 1, do they drive through
   the same assists?
2. **The wheel's loop:** which effect parameters can the firmware take,
   and at what rate does its loop run? (Settles the vendor report in 7.3.)
3. **Fairness in races:** in a mixed field (player on Tier 1, rivals on
   arcade), should the rivals' pace be set from the sim car's measured lap
   times on each track?
