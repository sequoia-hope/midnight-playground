# The Peninsula slice: building the open world

Status: **started 2026-10-13** (vision ROADMAP M18, brought forward: the
owner asked to start building out the open world). The existing levels
stay as they are and need not connect to the world; abstract levels (the
owner's holodeck idea) are welcome later as destinations of their own.
The atlas (`docs/vision/atlas.md`) is the world's plan; this document is
how the plan becomes a drivable world, in stages that are each playable
on their own.

## 1. What the open world needs that a level does not

A level today is one road: a ribbon the simulation projects the car onto
(`Track::project`, `surface_y`, the walls either side), which the world
generator shapes land around and dresses with a scenery kit. The arcade
physics keeps the car between the walls. Everything else (traffic,
rivals, police, the HUD's route bar) is measured along that ribbon.

An open world needs four things on top:

1. **Roads from real lines.** A corridor's centreline comes from the
   atlas (a real road, compressed), not from a turtle walk of
   `[length, turn, rise]` segments.
2. **Ground everywhere.** A height field under the whole box, with the
   roads blended into it, so the car can leave the road. The port kept
   this seam (port SPEC 4.5): the sim car already reads the world through
   `mp_vdyn::Ground`; the arcade physics still reads the ribbon directly.
3. **A road network.** Several ribbons meeting at junctions; the
   projection picks the nearest road; the junction surface is shared.
4. **Streaming.** The world generated per tile, deterministically from
   the atlas, the plan, the seed and the tile's coordinates, built in a
   ring around the player.

Multiplayer relies on determinism (CLAUDE.md principles 3 to 6), so every
one of these stays a plain function of its inputs: no clock, no hash
maps, exact maths through `mp_math`'s kernel.

## 2. The stages

Each stage is playable, each has a gate the owner can try, and the
design of a stage is written here before its work starts.

### Stage 1. A corridor from the atlas (started 2026-10-13)

The first race, **Midnight on the Coast** (the crest above San Gregorio
to Half Moon Bay's big intersection), as a level the menu lists, built
from the real road through the existing coast generator.

- **`mp-atlas corridor <event>`** writes `assets/atlas/peninsula/corridors/<event>.txt`:
  the event's route resolved over the real roads and profiled over the
  real ground, then compressed to the plan's scale about the world's
  origin, resampled every 5 game metres, with the land cover beside each
  sample. The file is committed, small (about 60 kB) and diffable, and a
  test fails when it is stale, as `resolved.json`'s does. The game never
  reads the atlas itself: `mp_levels` includes the corridor file at
  compile time, as Seaside's survey is read, so the simulation crates'
  rules (no dependencies beyond `mp_math` and `mp_track`) hold.
- **`Route::Path`** in `mp_track`: an open road given as a path (x, z,
  y per point), beside `Segments` and `Loop`. It is `Loop`'s builder
  without the wrap: the same resample to a metre, tags and road types by
  metres, zones by metres, curvature from the geometry. A point-to-point
  race on it runs as one on segments does (start, finish, runout, the
  route bar).
- **The level** (`mp_levels::south_coast`, id `south-coast`): the
  corridor's path splined as Seaside's is, the coast kit's cliff road
  (`coastal`) for the bluffs and the beach kit for the town's last
  stretch, zones switched where the real cover turns urban, a night sky
  with the moon over the sea, Coast Highway's traffic, police and rivals.
  The ground round the road is still the kit's own landform: the real
  ground arrives with stage 2.
- **Scale 0.5** (the owner, 2026-10-13), horizontal and vertical alike,
  so grades stay true: the climb from the junction to the crest is
  62 game metres over 1.05 km, as steep as the real one. The crest stays
  a crest.
- **Gate:** the owner drives the first race from the menu and recognises
  the road: the climb to the crest, the farms, the drop to the creeks,
  the town's lights at the end.

### Stage 2. Ground under the whole box (built 2026-10-13)

- **`tools/atlas/ground.py <corridor>...`** writes `<corridor>.ground.bin`
  beside the corridor files: the Terrarium terrain tiles (15 m real, the
  same source as the planning grid, cached in `tools/atlas/cache/`)
  resampled onto a grid of game metres over the corridors' box plus a
  margin (16 m and 1.2 km since stage 3; 12 m and 1.5 km before), heights
  compressed as the road is. 541 kB zlib in git;
  `mp_levels::ground::HeightGrid` reads it (bilinear, clamped) and the
  level includes it at compile time, inflated once when the level is
  first made. Packing it into the wasm rather than fetching it as
  Seaside's survey is fetched is a delivery choice for the owner: this
  way needed no page, loader or host plumbing.
- **An open level** (`Level::open`): its `ground` is the land under the
  whole box. The Track blends the road into it (`Track::ground_y`): the
  road's own surface out to the drawn shoulder, a metre's step down to
  the land with the road blended in by the coast landform's law
  (`OPEN_R0`, `OPEN_BAND`, `OPEN_POW`, `OPEN_DROP` in `mp_track`), and
  the land itself past the band. World generation's terrain uses the
  same constants in its near field and the level's ground as every
  zone's landform, so the car sits on what is drawn: a test holds the
  two within 0.75 m along the corridor, and equal past the band.
- **The arcade car off the road:** on an open level the walls are gone
  (no `collide_walls`), the height comes from `ground_y`, and past the
  shoulder the car is on loose ground (the circuit run-off's drag). The
  sim car's `TrackGround` reads the same ground and offers no wall
  colliders. Rivals, traffic and police are unchanged: they live on the
  ribbon.
- Not yet: the land's cover (farms, scrub, forest) is still the kit's
  colouring and planting over the real shape; the coast kit's sea stacks
  and cliffs are placed by its own rules, so some stand on dry land.
  Progress along the road is still the nearest point of the ribbon, so
  a field cuts a corner; a road network (stage 3) is where that gets
  its proper answer.
- **Gate:** pull off Highway 1 at the crest and drive across a field.

### Stage 3. Two roads meeting (built 2026-10-13)

- **Two corridors.** The level's road is now the whole of `coast-south`
  (the junction at San Gregorio to Half Moon Bay, 8.7 km); the race
  still starts at the crest, which the level finds as the highest point
  of the road's first 2 km. `hwy-84-lower` (a route added to the plan:
  the junction to La Honda, 7.1 km) is the first branch. The corridor
  writer now smooths the map's line (a triangular window 30 m either
  side, real) and writes the samples to the centimetre, so the road is a
  curve and not the map's chain of segments.
- **A branch** (`Level::branches`, `mp_track::Branch`): a corridor of its
  own with a road type and a zone of the level's, starting where it meets
  the parent. `Track::new` builds each as an open path
  (`Track::branches`, `BranchTrack`): the path is trimmed to where it
  leaves the parent's pavement and started on the parent's centreline;
  over the parent its heights are the parent's surface (lifted 2 cm over
  the shoulder so the meshes do not fight), and its own again 120 m on;
  the parent's fences open at the junction. The branch is one zone, its
  own road type the whole way.
- **The road under the car** (`Track::project_road`): the road the car
  was on keeps it while its shoulder holds it; otherwise the road whose
  shoulder does, or the one it is least far off. `Vehicle` carries
  `road`, `road_s` and `road_lat` beside `s` and `lat`: on a branch `s`
  and `lat` are still the car's place on the level's road, so the race,
  the rivals, the police and the HUD measure progress as before (a car
  up 84 is stopped at the junction, far off the road, as far as the race
  is concerned). The arcade physics, the sim car's ground adapter and
  the steering assists drive the road under the car.
- **World generation**: the terrain's box covers every road; the far
  field's distance and road height take every road (the mesh's detail
  and scenery's road clearance follow 84 too), its s, side and zone stay
  the level's road's; the near field blends every road into the land, so
  the junction is one surface (a test holds 84's first metres to Highway
  1's surface within 8 cm and the drawn ground beside the mouth within
  0.75 m). On an open level the zones follow the roads, not x
  (`Terrain::zone_weights_at`: the nearest road sample's zone, blended
  between the far field's cells), which also puts the crest in the coast
  zone's colours. The road builder makes a `Road` per branch, begun at
  the parent's paved edge, with no start or finish line and nothing
  across its mouth.
- Not yet: traffic, rivals and police stay on the level's road; the
  branch has no scenery (the redwoods of 84 are planting by land cover,
  which the whole open world still wants); the end of a road is open
  (the car drives off its end onto the land); the branch's own s is not
  shown anywhere.
- **Gate:** turn off Highway 1 onto 84 and drive up to La Honda.
- 2026-10-14 (D1211): the camera's floor follows the road under the car
  (it had been the level's road's plane, which on 84 is far off and
  banked); and the level card offers **Cruise** beside Race and Hot
  Pursuit: the corridor with no race on it, the Night City Cruise's
  score, no rivals, the level's traffic. The owner's "light night city
  cruise but the new map".

### Stage 4. The loop as one world (started 2026-10-14)

The loop (1, 92, Skyline, 84) as one drivable world, in parts that are
each playable. The whole-level build the game has (plan, terrain fields,
tiles, road, scenery, then the scene handed to the client) stays the
shape of a build; streaming (4b) makes it incremental rather than
replacing it.

**4a. The loop as one road, and planting by land cover** (built
2026-10-14, D1212).

- **The data.** Corridors `hwy-92`, `skyline-south` and `hwy-84` (the
  whole of 84, Skylonda to the sea; `hwy-84-lower` is its last 7 km) join
  `coast-south`: the four meet within 4 m and close the loop, 34.8 km of
  game road. `peninsula.ground.bin` is the real ground for the loop's box
  (16 m cells, 1.2 km margin, 660 kB) and the south coast level drives on
  it too (`coast-south.ground.bin` is gone; `ground.py --name`). Beside
  it `peninsula.cover.bin`: the land cover of every ground cell from the
  atlas's planning grid (ESA WorldCover and Overture's land cover, 50 m
  real), one byte a cell, read by `mp_levels::ground::CoverGrid`, carried
  by `Level::cover`. Both still packed into the wasm (D1208's choice).
- **The level** `peninsula`, "The Loop": `Route::Loop` through the four
  corridors splined closed; a race is one lap, from the crest above San
  Gregorio round to the crest; the card offers Cruise (D1211). Zones by
  metres along the lap: the coast (Highway 1, the Coast kit), Half Moon
  Bay (the Beach kit), over the hill (92, the Land kit), Skyline (35 and
  84, the Land kit); road types coastal, boulevard, valley, mountain.
- **The Land kit** (`mp_worldgen::land`): the open world's planting by
  land cover, which stages 2 and 3 wanted. Within reach of the roads
  (the far field's distance) it walks the land in 512 m cells and plants
  what the cover says: firs and redwood-dark spruce where it is forest
  (verge, mid and far detail by distance from the road, as the Sierra's
  forest is built), oaks and scrub where it is shrub, scattered oaks on
  grass, nothing on crop, urban, barren, sand or water, nothing on the
  road's clearance, below the sea or on the steepest ground. Every cell
  is seeded from the level's seed and its coordinates, so a cell's trees
  are a function of the cell alone: what 4b streams.
- **Gate:** the owner drives the loop from the menu, built whole behind
  the loading screen.

**4b. Streaming.** The plan once (the track, the terrain's fields, every
module's `plan()`), then the tiles, the road's chunks and the kits'
chunks built in sections round the player (`mp_worldgen::section`,
cached by section), spawned as they arrive and dropped behind; the level
starts with the section round the start, so there is no loading screen
to speak of. Natively on the build thread; on the web stepped on the
main thread between frames with a small budget (a hitch now and then,
until 4c). Landed 2026-10-14: the world's half (D1213:
`mp_worldgen::stream::Planned`, partitioned sections, the base layer)
and the client's (D1214: `mp_game::stream`, the ring round the camera,
several scenes' animators at once, shared images, `?stream=0` for the
whole build).

**4c. The web build off the main thread** (built 2026-10-14, D1215): a
worker running the builder as its own wasm (`mp_builder`, as `mp_exhaust`
and `mp_music` are), each section handed back packed with its animators
as data (`mp_worldgen::anim`), read and spawned by the client; `?worker=0`
keeps the stepped build.

**4d.** Skyline's dripping fog; fields and fence lines on crop; the
branches (Tunitas Creek Road, Stage Road); the far ends of roads closed.

- **Gate (4b):** the owner drives the whole loop without a loading screen.

### Stage 5. Events at places

- The plan's places and events become starts in the world: hold gas and
  brake at a marked intersection, as Burnout Paradise does. Races, time
  trials, a delivery from a garage in Half Moon Bay.
- **Gate (M18's):** from pressing Drive, the owner finds and plays a
  race, a time trial and a delivery without opening a menu.

## 3. Decisions

- **The levels stay.** Nothing in the menu changes; the corridor is a
  seventh entry. The remix of the existing levels (atlas.md 3.5) waits.
- **Road trips, not remixes, for the Sierra, the desert and the
  raceway:** they remain levels; the open world is home. Abstract
  levels (a holodeck grid, a figure of eight in the void) fit the same
  slot when they come.
- **The corridor file is text.** One line a sample: `x z y cover`, in
  game metres. It is read with `split_whitespace`, so `mp_levels` gains
  no dependency, and a change to the road shows in a diff.
- **Heights scale with the ground plan** (0.5 both ways). A vertical
  exaggeration can be added per route in `plan.json` if a road feels
  flat; none is applied now.
- **The real profile is smoothed** over 60 real metres before it is
  compressed: the planning grid is 50 m cells, and the road's own
  smoothing in `Track` (`elevation_smooth`) handles the rest. Stage 2's
  15 m tiles replace this.
