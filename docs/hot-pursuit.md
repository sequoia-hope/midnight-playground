# Hot Pursuit — design spec

Status: draft, 2026-09-29. Nothing here is implemented yet.

Police join the game as a second kind of opponent. The goal is the feel of the
classic Need for Speed pursuits: sirens getting louder in the mirrors, a
rising heat level, roadblocks ahead, and the relief of losing them in a
tunnel. It has to be built the way the rest of the game is built: generated
in code, with no asset files, and fast enough on a phone.

## 1. Modes

Two modes share one pursuit system (`Pursuit`, §4).

### 1.1 Hot Pursuit race (sprint levels: sierra, coast, streets, desert)

This is the normal sprint against five rivals, with police patrolling the
route.

- You win by finishing first, as now. Getting **busted** ends your race: you
  get a DNF and a "BUSTED" results screen.
- Police chase whichever racer they're closest to, not only you. Rivals can be
  busted too. They then drop out of the standings and pull over onto the
  shoulder, with the police car parked behind them and its lights flashing.
- Heat starts at 1 and rises with events (§3). Each zone of a level has a heat
  cap, so the calm first zone doesn't turn into a roadblock gauntlet.
- The finish line is a safe zone. Once you cross it, pursuers break off.

### 1.2 Most Wanted (cruise loop: `cruise`)

An endless evade mode on the freeway loop, built on Night City Cruise.

- The run starts at heat 1, with a patrol car that spots you on a speeding
  trigger.
- Each pursuit alternates between **Pursuit** (the cops can see you) and
  **Cooldown** (you've broken line of sight, the evade meter fills, and
  cops search). A full evade meter means **ESCAPED**, and you bank the bounty.
- If you're busted, the run ends and you keep 50% of the unbanked bounty.
- A new pursuit starts after 20–40 s of free cruising, with the starting heat
  one level higher each time (capped at 5).
- The score is banked bounty. The best score is stored per level, as cruise
  scores are today (`bestScore.cruise.pursuit`).

### 1.3 Out of scope for v1

- Playing as the police (takedowns against racers). This is a natural v2, since
  the police AI and lightbar already exist by then.
- Damage to the player's car, or wrecking it.
- Helicopters. They're at heat 5 in §3, but ship them in phase 3.

## 2. Menu and settings

- The level card gets a mode toggle next to the Race button: **Race / Hot
  Pursuit** on sprint levels, and **Cruise / Most Wanted** on the loop. The
  choice is saved per level (`store 'mode.<levelId>'`).
- In the options, **Police lights flash** can be turned off. Strobing
  red and blue is a photosensitivity concern, so when it's off the lightbars
  glow steadily and the scene-light pulse (§6.3) is disabled.
- Dev hooks, documented in the README next to the others:
  - `?pursuit=1` forces the mode on (and `?pursuit=0` off).
  - `?heat=1..5` sets the starting heat.
  - `?cops=N` caps the number of active units (`?cops=0` gives the mode
    without police, for testing the HUD).
  - `window.__pursuit` is the live Pursuit instance.

## 3. Heat

Heat is an integer from 1 to 5 plus a fractional **heat meter**. Filling the
meter raises the heat by one level. Heat never drops within a pursuit.

| Event | Heat meter |
| --- | --- |
| Every second in pursuit | +0.01 |
| Hitting a police car (strength > 0.2) | +0.15 |
| Disabling a police car (§5.4) | +0.35 |
| Dodging a roadblock or spike strip | +0.2 |
| Hitting a civilian car while police can see it | +0.05 |

| Heat | Max active units | Unit mix | Tactics unlocked | Top-speed factor |
| --- | --- | --- | --- | --- |
| 1 | 2 | Patrol | Follow, bump | 0.92 |
| 2 | 3 | Patrol, interceptor | PIT | 0.97 |
| 3 | 4 | + interceptor | Rolling block, roadblocks | 1.00 |
| 4 | 5 | + SUV | Boxing in, spike strips | 1.03 |
| 5 | 6 | + SUV | Helicopter (phase 3), heavy roadblocks | 1.06 |

The top-speed factor multiplies the unit's speed cap relative to the player
car's `vmax`/power curve (§5.2). At heat 5, cops are slightly faster than you
on a straight, so you escape through lines, traffic and nitro, not raw speed.

The heat level is shown as five stars on the HUD (§7). At a phone's
effective resolution the limit is **6 active units**. The pool is built
once, like the traffic pool, and recycled.

## 4. The Pursuit system

This is a new file, `src/game/Pursuit.js`, owned by `Race`. `Race` creates it
when the mode is on and calls `pursuit.update(dt, ctx)` right after
`traffic.update` and before `resolveCollisions`, so police cars are part of
`agents` and collide like everything else.

```
Pursuit
  state: 'patrol' | 'pursuit' | 'cooldown' | 'escaped' | 'busted'
  heat, heatMeter, bustMeter, evadeMeter, bounty
  units: PoliceDriver[]        (fixed pool, inactive until spawned)
  props: RoadblockSet, SpikeStrips
  update(dt, { player, playerBody, ais, agents, time })
  events[]                     (consumed by Race: HUD text, audio, scoring)
```

### 4.1 States (player's point of view)

- **patrol**: no pursuit yet (Most Wanted between pursuits; the first seconds
  of a race). Patrol cars drive with traffic. A patrol car that sees the
  player going more than 1.25× the zone's traffic speed within 120 m, or
  being hit, triggers **pursuit**.
- **pursuit**: at least one unit has line of sight (§4.2) within 300 m.
  The evade meter drains at 0.5/s.
- **cooldown**: no unit has line of sight. The evade meter fills at
  1/`evadeTime(heat)` per second, where `evadeTime` = 8, 11, 14, 18, 22 s for
  heat 1–5. Units search (§5.3). Being spotted again returns to
  **pursuit** and keeps the current meter value.
- **escaped** (Most Wanted only): bank the bounty, despawn the units behind
  the player as they fall out of view, then return to **patrol**.
- **busted**: the end state, described in §4.3.

In Hot Pursuit races, cooldown and escape happen per pursuit too, but nothing
is banked. Escaping just returns the police to patrol until they spot you
again.

### 4.2 Line of sight

Line of sight is cheap and in track coordinates, with no raycasts:

- the along-track distance `|t.ds(unit.s, player.s)|` is < 300 m (< 180 m at
  night with the player's lights off, which isn't a feature yet, so ignore
  that for now); and
- neither car is inside a tunnel tag unless both are in the same one; and
- the road between them doesn't turn more than 70° (the sum of |kappa|·ds
  over the gap, precomputed on the track as a cumulative array so it's a
  subtraction). This models canyon walls and city blocks hiding you around
  corners.

When `level.police.losOpenGround` is true (the desert basin, the playa), the
turning rule is skipped. On open ground they can always see you within
range.

### 4.3 Busted

The bust meter (0–1) fills while all of these hold:

- the player's speed is below 6 m/s, and
- at least one unit is within 9 m (by car centres), and
- the state is **pursuit**.

It fills at 0.45/s, plus 0.25/s for each extra unit within 9 m, and drains at
0.8/s otherwise. Boxing in (§5.2) is how a skilled pursuit fills it.
Being stuck against a wall with a cop nose-in behind you also fills it, which
is intended.

When the meter is full: freeze input, fade the music, play the radio
"suspect in custody" chatter, show the slow-motion
**BUSTED** card, then the results screen (a DNF for races; the bounty for
Most Wanted).

The reset key (R) is disabled while the bust meter is above 0. Otherwise it
would be a free escape.

### 4.4 Spawning

Spawning reuses the traffic pool pattern. Only these slots are valid:

- **behind**: 180–260 m behind the player, on a road position hidden from the camera
  (always true in chase view at that range). This is the usual way units join a
  pursuit, and they arrive with siren on and at `catchUpSpeed`.
- **ahead, oncoming** (levels with oncoming lanes): 400–600 m ahead, in the
  oncoming lane. The unit U-turns in a puff of tyre smoke when it passes the
  player. The U-turn is faked: it swaps `dir` and plays a 180° `spin` over
  0.8 s.
- **ahead, parked** (patrol state): on the shoulder at a level's
  `police.spots` (see §8), lights off and waiting.

Units that are more than 600 m behind and out of sight are recycled.

## 5. Police AI

This is a new file, `src/vehicles/PoliceDriver.js`, with
`class PoliceDriver extends KinematicCar`. It works in track coordinates like
`AIDriver` and `TrafficCar`, so it gets `advance()`, walls, spin and
collision response for free, and it stays cheap.

### 5.1 Unit types

| Kind | Model | Mass | Speed cap | Aggression | Notes |
| --- | --- | --- | --- | --- | --- |
| `patrol` | Police sedan (a new `police` spec in CarModel) | 1700 | 58 m/s | 0.4 | Black and white, with a lightbar |
| `interceptor` | The `muscle` or `sports` body in police livery | 1550 | 70 m/s | 0.7 | Slicktop: lights in the grille and rear window |
| `suv` | Police SUV (a new `policeSuv` spec) | 2400 | 62 m/s | 1.0 | Rams hardest, and is the roadblock vehicle |

The speed caps above are before the heat factor (§3).

### 5.2 Driving behaviours

Each unit picks one behaviour per tick, with hysteresis. The minimum time in
a behaviour is 1.5 s.

- **chase**: target `s` is the player's, with lat matched to the player.
  Target speed is `min(speedProfile·skill, cap·heatFactor)` plus a catch-up
  of +8 m/s when more than 60 m behind. Unlike `AIDriver`'s rubber band, this
  catch-up is only for the pursuers, so the chase stays tense.
- **bump**: close in to 0 m gap at player speed + 3 m/s, aiming at the
  player's rear bumper. The collision system provides the push.
- **PIT** (heat ≥ 2): approach the player's rear quarter (lat offset ±1.3 m,
  2.5 m behind), then add latVel toward the player's side. `resolveCollisions`
  turns this into a spin through the two-circle model. The cooldown is 6 s per
  unit.
- **rolling block** (heat ≥ 3, needs a unit ahead): the unit ahead matches
  the player's lat and brakes gently (−4 m/s²), trying to force a slow down.
- **box** (heat ≥ 4, needs 3 units within 40 m): assign the positions ahead,
  left and right (or ahead and behind on two-lane roads), and hold the
  player's speed minus 2 m/s. This is the main route to a bust at high heat.
- **search** (cooldown state): drive to the player's last known `s` at cruise
  speed and weave across the lanes. Each unit spreads out to cover the
  distance on either side of it (±150 m along the track).

Traffic avoidance reuses `AIDriver`'s look-ahead and side-picking code, so
factor that out into a shared helper instead of copying it. Police
avoid civilians, but hit one if the choice is between the civilian and
losing the player. This produces the chaos you want, and uses the same
`crashy` path traffic already uses.

### 5.3 Rival targeting (Hot Pursuit races)

Each unit's target is the nearest racer by along-track distance, weighted
×0.6 toward the player so the player gets most of the attention. When a unit
busts a rival, the rival's `AIDriver` goes to `park` on the shoulder, gets
`finished = true, dnf = true`, and `standings()` sorts DNFs last.

### 5.4 Disabling police

Units have `health` from 0 to 1, and each hit subtracts `strength × (1.4 −
unit.mass/3000)`. A unit is disabled at health ≤ 0, and also when a wall hit
at more than 25 m/s spins it out (`stunned > 1.2`). A disabled unit pulls to
a stop with smoke, and after 3 s its lights go to steady red. Disabling a
unit gives "TAKEDOWN" text, +3000 bounty and the heat meter event. It does
**not** give nitro, so ramming the police doesn't become the fastest way to
refill the tank.

## 6. Props and models

### 6.1 Police cars (`src/vehicles/CarModel.js`)

- `police`: a four-door sedan loft (between `sedan` and `sports` in shape),
  with black and white panels assigned through the loft's material callback
  (doors and roof white, the rest black) and a push bar.
- `policeSuv`: a boxy SUV with the same lightbar.
- **Lightbar**: a low box on the roof with 2×4 lens segments. It gets two new
  per-instance buckets, `lightRed` and `lightBlue`, both emissive, with
  `setSiren(phase)` driving their intensities. Everything else merges into
  the existing buckets, so a police car costs 2 draw calls more than a
  traffic car.
- The interceptor reuses the racer bodies. Add a `livery: 'police'` option to
  `buildVehicle` (black and white paint split with a door decal from a
  canvas), and grille and rear-deck strobes in the same two light buckets.
- API: `buildVehicle(kind, { livery, siren: true })` returns the usual
  handle plus `setSiren(on, t)`. The flash patterns (alternating
  red/blue quad-flash at about 2.5 Hz, and a steady mode) are computed inside
  `setSiren` from `t`.

### 6.2 Roadblocks and spikes (`src/game/PursuitProps.js`)

- **Roadblock**: 2–3 police cars parked at angles across the road with a gap
  of 1.2 car widths. At heat 5 it's SUVs with no gap, and you get through by
  shoulder barriers you can smash (§6.2.1). Place it on a straight run
  (|kappa| < 0.002 over 150 m) at least 450 m ahead, and announce it with a
  radio line plus a HUD marker. The parked cars are `KinematicCar`s with
  `speed = 0` and a high mass, so contact is a real impact (a crash, with
  heat).
- **Spike strip**: a thin, flat instanced mesh across a lateral span,
  with one police car on the shoulder next to it. A player wheel crossing it
  sets `phys.tyres = 'spiked'` for 10 s. Add support to `CarPhysics`: grip
  ×0.7, vmax ×0.75, rim sparks from `Effects.sparksAt` at the wheels, and a
  flapping-rubber loop sound. It's a new field that defaults to off, so
  normal races are unchanged.
- Sprint levels only place props in zones whose heat cap is 3 or more.

#### 6.2.1 Breakable barriers

Heavy roadblocks need a way through: 1–2 sawhorse barriers on the
shoulders that are `crashy` kinematic bodies with low mass. They fly apart
(reuse the traffic crash tumble) and cost 5 km/h.

### 6.3 Light on the world

The flashing lights at night are what sell a pursuit, but real lights are
expensive:

- **Emissive lightbars + bloom**: these are free and always on.
- **One shared `PointLight`** (HQ only), which follows the nearest active unit
  within 40 m and takes its red/blue flash colour and intensity. That puts
  moving colour on the road, the player's car and nearby walls for the cost of
  one light. On phones (HQ off), skip it.
- **An additive glow sprite** on each lightbar, from `glowTexture()`, which
  flashes in sync, so distant units read as police from far away.
- The whole pulse respects the **Police lights flash** option.

## 7. HUD (`src/game/HUD.js`, `hud.css`)

- **Heat stars**: five stars, top-centre. The next star fills with the heat
  meter.
- **Pursuit bar**: under the stars. It shows a red **BUST** meter while it's
  above 0, otherwise a blue **EVADE** meter during cooldown, and is hidden
  during patrol.
- **State text**: `PURSUIT`, `COOLDOWN`, `ESCAPED +12,400`, `BUSTED`, using
  the existing `center()`/`toast()`.
- **Minimap**: units are red/blue blinking dots, and roadblocks are a red bar
  across the route ahead.
- **Radio chatter**: short toast lines at the bottom (dispatch-style, for
  example "Unit 12 in pursuit, northbound on the 9", "Roadblock set at mile
  4", "Suspect's gone dark, all units search"). These are driven by
  `Pursuit.events` and rate-limited to one every 4 s.
- **Most Wanted**: the bounty counter replaces the cruise score block, with
  an unbanked value in amber and a banked value in white.
- Touch UI: nothing new to press. Keep the stars and bars clear of the top-left
  buttons.

## 8. Level data

Each level gets an optional `police` block. A level without one is not
offered in pursuit mode.

```js
police: {
  heatCap: [2, 3, 5],          // per zone
  spots: [{ s: 1200, lat: 5.5 }, …],  // parked patrol cars (patrol state)
  losOpenGround: [false, true, true], // per zone (§4.2)
  roadblockTags: ['summit', 'bridge'], // preferred straights for roadblocks
  spikeTags: [],
},
```

- **sierra**: the pass is heat-capped at 2 (narrow and twisty, so
  roadblocks there would be unfair), the valley at 3, and Interstate 9 at 5.
- **coast**: the cliffs at 2, the boulevard at 3 (with traffic both ways, so
  oncoming units spawn here), and the harbour bridge at 5 (with a roadblock on
  the bridge deck).
- **streets**: 3 / 4 / 5. Units spawn from the barricaded side streets, and
  the barriers there open for them, which is a scenery hook for Streets.js.
- **desert**: 2 / 4 / 5, with open-ground line of sight on Route 66 and the
  playa. That makes the desert hard to escape, which suits a sprint.
- **cruise**: Most Wanted. Tunnels are the escape routes (line of sight is
  broken), with spawns in both directions.

## 9. Audio (`src/game/Audio.js`)

Audio is synthesized like everything else:

- **Siren voices**: a pool of 3, for the nearest units, as with
  `setRivalEngines`. Each voice is two detuned oscillators (square and
  saw) through a bandpass filter, which gives the horn-speaker quality. The
  pitch follows the pattern, with doppler as a pitch offset from the relative
  along-track speed, plus distance gain, pan and a lowpass that rises with
  distance. The patterns are **wail** (a 650→1450 Hz sweep over 1.7 s),
  **yelp** (the same over 0.3 s, used under 60 m), **hi-lo** (for cooldown and
  the search) and a **horn** blip at pursuit start.
  API: `setSirens([{ dist, pan, relSpeed, mode }])`.
- **Radio**: a band-limited (300–3000 Hz) gibberish "voice" from formant-filtered
  noise with a random syllable rhythm, squelch clicks at the start and end,
  and a bus with light distortion. It plays alongside each chatter toast.
- **Music intensity**: the new music system exposes
  `setIntensity(0..1)`. Pursuit sets it from heat and the state: cooldown
  drops to a filtered, sparse arrangement, and escape plays a resolving
  sting. If the track system can't do intensity, fall back to a lowpass on the
  music bus during cooldown.
- **One-shots**: spike strip pop and hiss, busted sting, escaped sting, and
  the takedown crunch (reusing `impact` with a metal-heavy layer).

## 10. Performance budget

This is measured on the cruise loop at heat 5 with full traffic. That's the
worst case.

- Draw calls: at most +20 over the non-pursuit mode. That's 6 units × ~5
  calls, less the traffic the police replace (pursuit mode lowers the
  traffic pool from 30 to 24).
- Triangles: police cars use low LOD, except a unit within 30 m of the
  camera, which swaps to high LOD the same way the racers do. That's at most
  2 high-LOD units at a time.
- CPU: Pursuit + 6 PoliceDrivers under 0.3 ms per frame at 60 fps. The
  line-of-sight check is O(units) using the cumulative turn array, with no
  raycasts.
- Audio: 3 siren voices, and radio is one voice at a time.
- Phone: the shared PointLight is off, glow sprites are on, and the
  lightbars are emissive only.

## 11. Implementation plan

Each phase can be played on its own and verified headlessly (puppeteer
screenshots, `__pursuit` state checks, Node tests on the three-free parts).

1. **Core loop**: `Pursuit.js` (the states, the bust and evade meters, heat
   without props), `PoliceDriver` with chase, bump and search, a placeholder
   police model (a sedan with lightbar buckets), HUD stars and bars, and
   `?pursuit=1&heat=N`. Test in Node: a scripted player that stops is busted
   within 5 s with 2 units adjacent, and a player who holds 300 m of
   separation around a 90° corner escapes in `evadeTime(heat)` ± 1 s.
2. **Tactics and models**: PIT, rolling block and box; the real `police`,
   `policeSuv` and interceptor livery; siren audio; rival targeting and busts;
   the menu toggle; the results screens.
3. **Props and polish**: roadblocks, spike strips (with the `CarPhysics`
   field), breakable barriers, radio chatter with voice, music intensity, the
   shared flash light, the helicopter (a spotlight cone plus rotor sound,
   hovering over the player's s and never colliding), and the photosensitivity
   option.

Acceptance for each phase: every existing mode is unchanged with pursuit off
(the same `__stats` and no console errors on all five levels), and there are
no console errors in pursuit mode on all five levels. Performance must stay
within §10, measured headlessly.

## 12. Open questions

1. **Damage**: should the player's car take damage in pursuit (a
   health bar, with wrecking as a second loss condition)? It's out of v1,
   but it changes how hard ramming should be.
2. **Busted in a race**: DNF, or respawn 200 m back with a time penalty? A
   DNF is truer to the fantasy but punishing on a 9 km sprint.
3. **Rivals as targets**: is it fun, or does it make races random? One
   option is a toggle, "Police chase: everyone / only you".
4. **Most Wanted progression**: is a per-session escalating heat enough, or
   should there be a persistent "wanted level" and a bounty leaderboard
   between sessions?
5. **Play as cop (v2)**: the same systems, inverted. Which level would suit
   it first? The desert has open ground to chase across and room to set
   roadblocks.
