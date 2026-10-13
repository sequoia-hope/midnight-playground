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

- **`tools/atlas/ground.py <corridor>`** writes `<corridor>.ground.bin`
  beside the corridor file: the Terrarium terrain tiles (15 m real, the
  same source as the planning grid, cached in `tools/atlas/cache/`)
  resampled onto a grid of 12 game metres over the corridor's box plus
  1.5 km, heights compressed as the road is. 341 kB zlib in git;
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

### Stage 3. Two roads meeting

- A road network: several corridors sharing junctions, from the atlas's
  graph. The projection picks the nearest road; traffic and rivals
  follow a route through the network.
- The junction surface: where 84 meets Highway 1 at San Gregorio first.
- **Gate:** turn off Highway 1 onto 84 and drive up to La Honda.

### Stage 4. Streaming

- World generation per tile from the atlas and the plan, built in a ring
  round the player on the build thread, scene data streamed into the
  client as the existing world builds are.
- The loop (1, 92, Skyline, 84) as one drivable world; Skyline's
  dripping fog.
- **Gate:** the owner drives the whole loop without a loading screen.

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
