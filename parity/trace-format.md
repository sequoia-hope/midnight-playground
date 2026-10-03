# The trace record, version 1

What the Rust simulation and the JS reference runs are compared on, tick for
tick (SPEC 4.6). The JS writer is `tools/parity/lib/trace.mjs`
(`traceRecord`); the Rust writer will be `mr_sim::trace_record()`. Where the
two documents disagree, fix one of them: the bytes must be the same.

A record is a flat sequence of little-endian values, in exactly the order
below, with no padding:

| Type | Bytes | Meaning |
|---|---|---|
| `f64` | 8 | IEEE double, its bits as stored. Never NaN in a healthy state. |
| `opt` | 8 | A double that may be missing (JS `null` or `undefined`, Rust `None`): missing is the bits `0x7FF8_0000_0000_0D1E`, a NaN payload no arithmetic makes. |
| `i32` | 4 | Integer, enum code or count. |
| `bool` | 4 | `i32` 0 or 1. |

Enum codes are the index in these lists; `null` is -1:

| Enum | Codes |
|---|---|
| race state | countdown, racing, finished |
| park kind | circuit, lane |
| pursuit state | patrol, pursuit, cooldown |
| police mode | parked, chase, oncoming, search, standdown, hold, disabled, block |
| behaviour | chase, bump, pit, roll, box |
| slot | ahead, left, right, behind |
| siren | off, flash, disabled |
| hold reason | busted, wrecked |

A body reference (a police unit's target, a roadblock's cars, the spike
strip's car) is an `i32`: player `i` is `i`, rival `i` is `100 + i`, traffic
car `i` is `200 + i`, police car `i` is `300 + i` (units, then roadblock
cars, as one list), sawhorse `i` is `500 + i`; none is -1.

The record's hash is FNV-1a 64 over its bytes (offset basis
`0xcbf29ce484222325`, prime `0x100000001b3`).

Presentation state is left out: body pitch and roll and their rates, the
brake light, `accelLong` and `accelLat` (read only by the body springs),
anything the HUD, camera, effects or audio keep.

## Layout

```
tick                    i32   1 for the first tick of the run
hasRace                 bool  a Race (game oracle); 0 for most module scenarios
hasPursuit              bool
if hasRace:
  state                 i32   race state
  time                  f64   race clock
  countdown             f64
  throttleAt            opt   perfect-start bookkeeping
nPlayers nRivals nTraffic nPolice nSawhorses   5 × i32 (nPolice = units + roadblock cars)
for each player:        the input that drove this tick (InputFrame)
  steer                 i32   round(clamp(steer,-1,1) * 32767); I32_MIN if the player was not driven
  throttle              i32   round(clamp(throttle,0,1) * 255)
  brake                 i32   round(clamp(brake,0,1) * 255)
  flags                 i32   handbrake 1 | nitro 2 | analog 4

for each player:
  x y z yaw vx vy vz yawRate s lat speed steerAngle     f64 × 12   (Vehicle)
  onGround              bool
  visualYaw             f64
  gear                  i32
  rpm shiftTimer nitro  f64 × 3
  nitroActive drifting  bool × 2
  driftTime slip skid scrape                            f64 × 4
  scrapeSide            i32   (undefined before the first scrape: 0)
  airTime               f64
  locked                bool
  boost powerOut regen damage spiked                    f64 × 5
  offTrack              f64   (undefined before the first tick on the ground: 0)
  hasRules              bool  Race rule state follows
  if hasRules:
    prog lastS odo      opt × 3
    dist                f64
    lap                 i32
    lapStart            f64
    lapCount            i32   lap times so far
    lastLap             opt   the last lap time
    score mult multTimer topSpeed                       f64 × 4
    nearMisses          i32
    bonusCooldown resetCooldown wrongWay                f64 × 3
    stuck               opt
    lastDrift finishDelay                               f64 × 2
    finished            bool
    finishTime          opt
    reported            bool  results handed over
    passTimer passLat   opt × 2   (the cool-down driver's overtake)
    park                (see below)
    nParkRows           i32, then one i32 per row (cars parked in each lane)
  hasPursuitView        bool
  if hasPursuitView:
    damage              f64
    wrecks              i32
    penalty t           f64 × 2

park:                   kind i32 (-1 none); if lane: stopAt laneLat s0 lat0 f64 × 4

kinematic:              (every track-coordinate car)
  s lat speed latVel    f64 × 4
  dir                   i32
  spin spinRate stunned f64 × 3
  x y z yaw visualYaw vx vz                             f64 × 7  (Vehicle, as writePos left it)

for each rival:
  kinematic
  avoid avoidTimer nitro nitroTimer                     f64 × 4
  finished              bool
  finishTime            opt
  throttle              f64
  nitroActive           bool
  hold                  f64
  holdLat               opt
  spiked                f64
  prog                  opt
  park
  if hasPursuit: lastHit opt   (PursuitView's last damaging hit by this car, its clock)

hasTraffic              bool
if hasTraffic:
  nextSpawnS nextOppS   f64 × 2
  maxActive             i32
for each traffic car (pool order: sedan, hatch, van, pickup, boxtruck, tractor, then the far carriageway's; inactive ones included):
  active                bool
  kinematic
  crashed cruise laneLat                                f64 × 3
  lane                  i32   (-1 before the first spawn)
  passed                opt   Race's last along-track gap to the player
  nearMissHit           bool  Race marked it 'hit'
  if hasPursuit: lastHit opt

if hasPursuit:
  state                 i32
  heat maxHeat          i32 × 2
  heatMeter bust evade time spawnT propT                f64 × 6
  patrolT               opt
  takedowns busts       i32 × 2
  hasRoadblock          bool
  if hasRoadblock: s gapLat f64 × 2; heavy passed touched bool × 3; nCars i32; car refs i32 × nCars
  hasSpikes             bool
  if hasSpikes: s lat0 lat1 f64 × 3; car ref i32; passed bool; hit i32 (bit i: racer i)
  nSpots                i32, then used bool per spot
  nRacers               i32, then per racer (player first, then the rivals):
    bust hold holdTotal f64 × 3
    holdReason          i32
    grace               f64
    finished            bool
    prevS               opt
  for each police car (units in pool order, then roadblock cars):
    active              bool
    kinematic
    mode behaviour      i32 × 2
    behT modeT          f64 × 2
    slot                i32
    pitSide             i32
    pitCooldown pitPush f64 × 2   (pitPush undefined before the first chase: 0)
    target              i32   body reference
    lastSeenS health disabledT parkLat blockYaw          f64 × 5
    gapLat              opt
    laneLat cap weave   f64 × 3
    retarget            opt
    uturned             bool
    avoid avoidTimer    f64 × 2   (undefined until first set, on cars never activated or never swerved: 0)
    siren               i32
    lastHit             opt
  for each sawhorse:
    active broken       bool × 2
    kinematic
    h vy age            f64 × 3
    gapLat lastHit      opt × 2

streams                 i32 × 4: draws so far from ai, police, pursuit, traffic (-1: not a counted stream)
```

## Trace files

A trace file holds one run (`TraceWriter` and `readTrace` in
`tools/parity/lib/trace.mjs`), little-endian:

```
magic                   8 bytes  "MRTRACE\0"
version                 u32      this document's version
metaLength              u32
meta                    JSON (UTF-8): id, level, car, seed, query, jsTree, source
N                       u32      ticks
K                       u32      full-record interval (120)
hashes                  N × u64  FNV-1a 64 of each tick's record, as lo u32 then hi u32
M                       u32      full records kept
M × { tick u32, length u32, bytes }   every tick that is a multiple of K, and the last
```

To localise a mismatch: find the first tick whose hash differs, take the
nearest full record at or before it, run Rust forward from there, and diff
the first differing record field by field (the layout above names them).
