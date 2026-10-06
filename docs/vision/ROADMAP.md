# After the port: roadmap

The order of work once the Rust port is done. It continues the port's
numbering (`docs/rust-port/ROADMAP.md` ends at M11) and replaces that
document's "M12 and beyond" table. What each milestone is *for* is in
`docs/vision/WORLD.md`; the vehicle work is specified in
`docs/vehicle-dynamics/SPEC.md`.

**The order is a proposal.** The owner reorders it freely. Two rules hold
whatever the order:

- **Each milestone gets a design document before work starts.** The
  document is named in its row, written from WORLD.md and the owner's
  notes, and approved by the owner.
- **Nothing here starts before cutover** (port M9). Multiplayer (M10–M11)
  comes first after it, and its gate is a stable game: no audio bugs, no
  frame stutters, tried on a friend's Android phone.

Accessibility is not a milestone: every milestone keeps one-handed play,
a colour-blind-safe HUD and the auto-steer option working.

## Already planned (port roadmap)

| | Goal |
|---|---|
| **M9. Cutover** | The Rust build replaces the JS game |
| **M10. Multiplayer: race together** | Two to eight people on a LAN racing the existing levels |
| **M11. Multiplayer: tab hosting** | WebRTC, a tab as host, Hot Pursuit and cruise with several players |

## M12. Sound

**Goal:** engines with a deep rumble, and music with real depth and range.
First after multiplayer, ahead of any world building: the game is already
fun, and sound is where it falls shortest.

- Engines as a physical exhaust model (pulses, headers, pipes, mufflers)
  with cycle-to-cycle variation, and bass that survives phone speakers.
- Instruments with character: 808, 909, 303, Juno-style voices, and
  Mutable Instruments' open modules (Plaits first).
- Arrangements that evolve: genre grammars for house, techno and more,
  seeded variation, music that follows the race.
- A small Rust DSP module in the AudioWorklet (revisiting port SPEC 7.1).
- **Gate:** sound.md S2 to S4: a V8 that rumbles on a phone, and a house
  and a techno track the owner would play outside the game.
- Design doc: `docs/vision/sound.md` (draft written).

## M13. Level tools

**Goal:** the owner and Claude can build levels together, and the world
plan exists as data.

- A **world plan file**: regions, the road network between them, and
  which corridor each level is.
- **Corridor authoring:** a level's road drawn and edited in the level
  viewer (god mode), with annotations both sides can leave.
- Scenery kits a corridor can draw on (the existing generators, made
  reusable).
- **Gate:** the redwood corridor (Skyline and the descent) is drafted
  with the tools, by the owner and Claude together.
- Design doc: `docs/vision/level-tools.md`.

## M14. A living world

**Goal:** the existing levels feel alive. Moved ahead of cars and
classes at the owner's request; Seaside Raceway first.

- Crowds, waving fans, camera operators and drones at Seaside Raceway.
- Birds on every level; pelicans flying alongside on the coast.
- Traffic that reacts; pedestrians and wildlife who always get out of
  the way.
- **Day and night, and weather** as simulation state: rain that changes
  grip, wet windscreens, headlights and high beams, fog.
- **Gate:** the owner's drive down the Coast Highway at dusk, with
  pelicans.
- Design doc: `docs/vision/living-world.md`.

## M15. Cars and classes

**Goal:** fair races at every level of performance, and more cars to love.

- **Classes and a performance index** (WORLD 8.1); races run within a
  class; the Ion Arc stops making races easy.
- **New cars** from the stable draft, starting with the owner's own:
  the 3000GT VR-4 type, the Saturn Sky Redline type, the S4 type and the
  Firebird type; then the Diablo and Porsche GT types.
- **Garage:** paint, liveries, interior colour; tuning that stays within
  the class.
- **Cameras, replays, photo mode and ghosts** (WORLD 8.3).
- **Gate:** a race in each class is close with good driving on every car
  in it, measured by the AI's lap times and the owner's.
- Design doc: `docs/vision/cars.md`.

## M16. Radio

**Goal:** generated radio stations worth leaving on.

- Stations by genre on M12's music system (sound.md S5).
- DJ chatter and station identities voiced like the police radio (Qwen
  voice design): sparse, and about what this player did (WORLD 1.1).
- **Radio v0 comes early:** two stations over the existing songs, DJ
  breaks at song changes (radio.md R1). It needs only cutover, so it can
  land right after it, alongside multiplayer.
- **Gate:** the owner leaves it on for a whole session.
- Design doc: `docs/vision/radio.md` (draft written).

## M17. Sim handling

**Goal:** the Assetto Corsa class option on every car.

- Vehicle-dynamics milestones **V0–V4**: the seams, the core, the sim car on
  track, the owner's wheel with force feedback, every car and device.
- Each car's `VehicleDef` and a racing tune of assists by default.
- Telemetry viewable in the game and exportable.
- **Gate:** vehicle-dynamics V4.
- Design doc: `docs/vehicle-dynamics/SPEC.md` (written).
- In parallel, in its own repository: the RP2350 wheel firmware
  (vehicle-dynamics SPEC 7.3).

## M18. The Peninsula slice

**Goal:** the first piece of open world.

- The loop coast → 92 → Skyline → the redwoods → the coast, the coastal
  city, and Highway 1 north, joined into one drivable world with
  streaming.
- **Skyline's dripping fog.**
- Start-anywhere events at intersections; time trials; delivery jobs
  from garages; gas stations and car washes.
- One breakable shortcut through a fence.
- **Gate:** from pressing Drive, the owner finds and plays a race, a
  time trial and a delivery without opening a menu.
- Design doc: `docs/vision/peninsula.md`.

## M19. Off the tarmac

**Goal:** dirt and rocks.

- **A rallycross loop** in the state park (arcade handling on loose
  surfaces is enough to start).
- **Crawler hills:** vehicle-dynamics **V5–V6**, from rigid multi-point
  tyres to the soft balloon tyre and then the owner's Onshape tyre.
- **Gate:** a rallycross race, and a crawler climbing the test rocks on
  soft tyres.
- Design docs: `docs/vision/off-road.md` and the vehicle-dynamics spec.

## M20. Damage, destruction and gadgets

**Goal:** crashing is part of the fun.

- Breakable props as simulation state; more shortcuts through them.
- Crash junctions scored in dollars; takedowns.
- Visible car damage, and with sim handling, damage that changes the car.
- Gadgets, not guns (WORLD 7.1): winch, EMP, drones, world hacking,
  possession games, paint tags.
- **Gate:** a crash junction the owner wants to replay.
- Design doc: `docs/vision/damage.md`.

## M21. Playing with others, beyond the race

**Goal:** records, friends and watching.

- Leaderboards per road and results shared with friends, preferring a
  serverless store (the player's AT Protocol repository) to a server the
  owner must run.
- Spectator mode; streaming to the owner's projector.
- Car meets in the open world.
- **Gate:** two friends compare times on Skyline without the owner
  running a server.
- Design doc: `docs/vision/social.md`.

## M22. RL and robotics

**Goal:** the simulation as a research environment.

- `mr_sim::Env` and `mr_py` (port SPEC 10); rivals trained with RL on the
  existing tracks.
- The crawler environment (vehicle-dynamics V7); dash camera and lidar
  sensor models.
- Can start any time after cutover, in parallel with the rest, since it
  touches no game feature.
- Design doc: `docs/vision/rl.md`.

## Later

The world grows (the San Francisco-like city, the bridge, the East Bay and
its trams, then the Pearl River Delta), and the parked ideas of WORLD.md
section 11 wait their turn.
