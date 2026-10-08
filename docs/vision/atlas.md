# The atlas: planning the open world on the real peninsula

Status: **started 2026-10-08** (vision ROADMAP M13, the world plan file).
The owner asked for the system to plan the open world with, before any
level changes: real elevation for the whole peninsula, regions mapped out
on it, and a proposal for them. Nothing here changes a level yet.

## 1. The owner's direction (2026-10-08)

- **Half Moon Bay becomes more like the real Half Moon Bay,** and
  **over the hill the city is the peninsula:** Redwood City all the way up
  to San Francisco becomes the city. Pacifica and Half Moon Bay get more
  realistic too. This replaces "the sleepy town grown into a real city by
  the beach" of WORLD 3.
- **Close the loop:** Highway 84, Highway 1, Highway 92 and Highway 35
  (Skyline). The first piece of world is that loop.
- **The first race starts in the dark at the crest above San Gregorio.**
  Where 84 meets Highway 1 the road is almost at sea level; heading north
  toward Half Moon Bay it climbs a little to a crest with a beautiful
  view. The race starts at that crest (duplicating the view), races
  through the farmland, and **finishes at the big intersection in Half
  Moon Bay** (Highway 1 at 92). The coastal section can be extended.
- **The farms carry the colour:** seasonal flowers, wildflowers, the
  yellow mustard cover crops, pumpkins in their season. The owner drove
  this road daily (working in San Gregorio, living in Half Moon Bay).
- **Remix the levels to fit the peninsula:** the desert and its train
  don't fit, and we are not in the Sierra. But the variety of Cruis'n
  World and Cruis'n USA, jumping round the world, is fun too.
- **Don't change the levels yet.** Build the planning system first.

## 2. What is built

| Piece | Where | What it does |
|---|---|---|
| The data build | `tools/atlas/build.py` | Downloads real terrain and map data for the box, caches it, writes the atlas files |
| The atlas files | `assets/atlas/peninsula/` | `terrain.bin`, `geo.json` (generated), `plan.json` (by hand), `resolved.json` (measured) |
| `mp_atlas` | `crates/mp_atlas/` | Reads them; resolves routes over the real roads; profiles them over the real ground; reports |
| `mp-atlas` | `cargo run -p mp_atlas -- report` | The plan measured, as text (`resolve` rewrites `resolved.json`; `profile <id>` prints CSV) |
| The planner | `tools/atlas.html` | The map: terrain, land cover, roads, rail, parks, towns; the plan's regions, routes and places; profiles; editing |

### 2.1 The data

The box runs from Pigeon Point and Big Basin's edge (37.18° N) to the
Golden Gate (37.84° N), and from the ocean (122.56° W) to the bay shore
(122.08° W): about 42 by 73 km.

- **Terrain:** Mapzen/Tilezen terrain tiles on AWS Open Data, zoom 13
  (about 15 m a pixel here), a blend of USGS 3DEP 10 m, SRTM and
  bathymetry. Public domain and open licences. Resampled to a **50 m
  grid** for planning (849 × 1465 cells); the cache keeps the 15 m tiles
  for corridor generation. The sea floor is clamped at −120 m.
- **Land cover:** ESA WorldCover through Overture Maps (CC BY 4.0): sea,
  lakes, towns, farms, grass, scrub, forest, bare, marsh, sand. Farmland,
  orchards and nurseries from OpenStreetMap paint over it, then lakes and
  reservoirs, then the sea and the bay.
- **Roads:** Overture Maps' transportation segments (OpenStreetMap, ODbL),
  tertiary and up, split at junctions into a graph, joined back where a
  road only met a residential street, simplified to 4 m. Each road keeps
  its class, name, route numbers ("1", "92", "35", "84", "280", "101"),
  and whether it is a bridge or a tunnel.
- **Railways, parks, towns:** Overture Maps (OpenStreetMap). Caltrain is
  there for the remix of the desert's train.
- **Network:** this needs only AWS S3 (the tile bucket and Overture's
  bucket). OpenStreetMap's own servers and USGS's are blocked from the
  cloud sessions, which is why Overture is the source.

Sizes in git: `terrain.bin` 3.7 MB, `geo.json` 2.1 MB, `resolved.json`
85 kB.

### 2.2 The local frame

As the levels: **x east, z south, metres from the origin**, which is Half
Moon Bay's big intersection (Highway 1 at 92, 37.46802° N 122.43350° W).
The projection is equirectangular about the origin's latitude, with the
metres per degree the build script computes there, so `mp_atlas` needs no
trigonometry; east-west distances are off by at most 0.6 % at the box's
top and bottom edges.

### 2.3 The plan file

`assets/atlas/peninsula/plan.json`, written by hand. Coordinates are
`[lat, lon]`, the order Google Maps copies them in, so a place can be
pasted from a map.

- `scale`: game metres per real metre (the compression), 0.5 to start.
- `places`: `id`, `name`, `kind` (junction, start, town, landmark,
  viewpoint, trailhead, offshore), `at`, `note`.
- `regions`: `id`, `name`, `stage` (first, planned, later), `character`
  (what it should feel like), `real` (what it compresses), `kits` (the
  existing generators it would draw on), `ring` (its outline).
- `routes`: `id`, `name`, `roads` (route numbers) or `names` (road names,
  for roads without numbers like Tunitas Creek Road) or `classes`;
  `from`, `via`, `to` (place ids); `scale` to override the plan's.
- `circuits`: routes end to end; the report checks each ends where the
  next begins.
- `events`: `kind` (race, time trial), `route` (whose roads it follows),
  `from`, `to`, `light`, `note`.
- `remix`: what each existing level becomes.

Ids are unique across the file, and every id a route, event or circuit
names must exist: the loader refuses the plan otherwise.

### 2.4 How a route is measured

- **Resolved over the real roads:** each place snaps to the nearest point
  on a road the route may use (not the nearest junction: on the coast a
  road runs kilometres between junctions), then the shortest way between
  them over those roads. A place more than 150 m from its roads is
  flagged.
- **Profiled over the real ground** every 20 m: length, climb and
  descent, lowest and highest, the steepest grade over 200 m, and the
  crests (a high point the road drops at least 6 m from on both sides
  before it climbs higher). Through tunnels and over bridges the road
  keeps its own level instead of following the hill or the creek, so
  Devil's Slide's tunnels read 16 %, not 49 %.
- **What it passes through:** the land cover within 150 m either side.
- **Regions:** area, land, heights, cover and the towns inside.

### 2.5 The planner

`tools/atlas.html` on the registered server (`/tools/atlas.html`). A
shaded survey map of the box (land cover, hillshade from the north-west,
100 m contours), the roads by class, Caltrain, towns, and the plan on top:
regions by stage, routes in red, events in mustard when picked, places as
diamonds. Tap anything for its details: a route or event shows its
measured facts and its elevation profile with the crests; a region its
land and cover. The readout gives any point's coordinates, height and
ground.

**Edit** turns on dragging places and the picked region's corners, and
**Add place** drops a new one. Edits stay in that browser; **Copy
plan.json** copies the edited plan to paste into the file or into a chat.
Then `cargo run -p mp_atlas -- resolve` measures it again.

## 3. The proposal

### 3.1 The first race: Midnight on the Coast

Measured on the real road (`cargo run -p mp_atlas -- report`):

- **The junction of 84 and 1 is at 19 m**; Highway 1 north climbs
  **125 m in 2.1 km** to the crest at about 145 m (37.33868, −122.39447).
  That is the crest with the view; the start sits on it.
- **From the crest to the big intersection: 15.3 km of real road,** with
  farms on **39 %** of the roadside, scrub on 34 %, then the town. Five
  crests along the way, steepest grade 14 %.
- **At the plan's scale of 0.5 it is 7.7 km in the game,** the length of
  today's Coast Highway (7.7 km; Sierra is 9.4 km, Desert 7.8 km). The
  scale is the owner's call (section 4).
- **Light:** night, the moon on the sea, the farms dark, the town's lights
  ahead.

### 3.2 The loop

**1, 92, Skyline and 84: 69.8 km of real road; it closes.**

| Route | Real | Game at 0.5 | Heights | Climb | Beside the road |
|---|---|---|---|---|---|
| Highway 1: San Gregorio to Half Moon Bay | 17.4 km | 8.7 km | 17–145 m | 284 m | scrub, farms 34 % |
| Highway 92 over the hill | 8.3 km | 4.1 km | 18–268 m | 291 m | forest, farms 17 % |
| Skyline: 92 to Skylonda | 20.0 km | 10.0 km | 262–714 m | 687 m | forest 92 % |
| Highway 84: Skylonda to the sea | 24.1 km | 12.0 km | 15–456 m | 320 m | forest, scrub |

Beside it: Highway 1 north to Pacifica through Devil's Slide's tunnels
(18.9 km), **Tunitas Creek Road** from Highway 1 up to Skyline at Kings
Mountain (15.1 km, 866 m of climb, redwoods all the way: the Highway 9
character the owner wants, inside the loop), and Stage Road from San
Gregorio to Pescadero (11.4 km).

### 3.3 The regions

| Region | Stage | Real | Kits |
|---|---|---|---|
| The South Coast | first | Highway 1 from San Gregorio to Half Moon Bay | coast |
| Half Moon Bay | planned | Half Moon Bay, El Granada, Princeton, Pillar Point, Mavericks | coast, harbor |
| Over the hill: Highway 92 | planned | The pumpkin patches, nurseries, the climb to Skyline | sierra, valley |
| Skyline | planned | Highway 35 from 92 to Skylonda, Kings Mountain, Alice's | mountain |
| The redwoods: 84 and La Honda | planned | Highway 84 | mountain |
| The Midcoast and Devil's Slide | planned | Moss Beach, Montara, the tunnels, Pacifica | coast |
| The state parks | later | Purisima Creek, El Corte de Madera, Tunitas Creek | mountain, rally |
| Montara Mountain | later | Crawler hills above Montara and Pacifica | desert (rocks) |
| Crystal Springs and 280 | later | The reservoir's valley, Cañada Road, 280 | valley, city |
| The Bayside | later | Redwood City to South San Francisco | city, cruise, desert |
| The City | later | San Francisco and Daly City | streets, city |

The outlines are first drafts on the real map, meant to be dragged into
shape in the planner.

### 3.4 Seasons on the coast

The farms change through the year, and the game can follow the calendar
or pick a season per event:

- **Late winter:** yellow mustard in the cover crops and the orchards.
- **Spring:** wildflowers on the bluffs and the hills; green hills.
- **Summer:** fog in the mornings, gold hills; Brussels sprouts and
  artichokes in the fields.
- **October:** pumpkins on 92 and the festival; the clearest days.
- All year: the nurseries' greenhouses, flower farms.

### 3.5 Remixing the levels

The levels stay as they are until a remix is chosen. The proposal:

- **Coast Highway** becomes the South Coast and Half Moon Bay: its cliff
  road is the bluffs above San Gregorio, Seabright's boulevard is Half
  Moon Bay's beaches and Main Street, the harbour is Pillar Point.
- **Sierra to the City** lends its switchbacks to 92's climb and the
  Kings Mountain grades, Mill Valley's farms to the pumpkin patches, and
  its Interstate to 280 beside the reservoir.
- **Desert Run** splits: the freight-train race becomes a race beside the
  railway on the bay side, the dry lake bed becomes the salt ponds by the
  bay under the moon, and the canyon's rocks become Montara Mountain's
  crawler hills.
- **Downtown Streets** is San Francisco already, nearly (Nob Hill's
  jumps).
- **Night City Cruise** becomes 101 and 280 at night, joined into a loop
  up and down the peninsula.
- **Seaside Raceway** stays itself, a real circuit to the south.
- **Road trips, as Cruis'n World jumps:** the real Sierra, the real desert
  and the raceway stay as places outside the peninsula, reached from the
  world's edges (a sign on 280 south, the bridge east) and loaded as
  their own levels. The open world is home; the road trips are the
  variety.

## 4. For the owner to decide

1. **The scale.** At 0.5 the loop is 35 km of game road and the first race
   7.7 km. Burnout Paradise's whole map is about 8 by 8 km; a smaller
   scale (0.3) makes a tighter world, a larger one a truer drive.
   Compression can also differ by region (the coast truer, the city
   tighter).
2. **The regions' outlines and names,** dragged in the planner.
3. **Road trips** for the Sierra, the desert and the raceway, or remix
   them into the peninsula entirely.
4. **Which season** the first race is in.

## 5. Next

1. **Look at the planner with the owner** and settle the scale and the
   regions.
2. **A corridor from the atlas:** generate the first race's road from the
   real route (its line compressed to the scale, its heights from the
   real profile, the farms and scrub from the real cover) through the
   existing coast generator. This is where `mp_atlas` meets
   `mp_worldgen`.
3. **The level viewer** (god mode) shows a corridor's place on the atlas.
4. Finer data where a corridor needs it: the cache's 15 m terrain, and
   the full road detail (residential streets, tracks) for Half Moon Bay
   and the state parks' dirt roads.
