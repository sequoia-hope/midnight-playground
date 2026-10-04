# Parity data

What the Rust port is compared against (SPEC 4.6, 5.7, 7.5, 12). Everything
here is produced from the JS game by the tools in `tools/parity/`, with the
parity hooks on (`src/parity/`).

| Path | In git | What |
|---|---|---|
| `golden/` | yes | Small goldens: simulation traces of staged scenarios, the math functions, digests, audio arrays, the kernel. Regenerating one must give an identical file. |
| `cache/<key>/` | no | Large outputs regenerated on demand: whole-race recordings, scene exports, Track and terrain dumps, screenshots. `<key>` is a hash of the JS tree (`node tools/parity/lib/jstree.mjs`), so a changed game never reuses a stale capture. |
| `report/` | no | The comparison report (`cargo xtask parity ...`), viewed through the registered server at `/parity/report/`. |
| `trace-format.md` | yes | The trace record both sides produce (WP 0.4). |
| `scenarios.md` | yes | The catalogue of staged simulation scenarios (WP 0.4). |
| `golden/world/` | yes | Per level: SHA-256 of every Track array and of the terrain heights at 10,000 points, and the scene's counts, material kinds and digest hash; `models.json` likewise (WP 0.5). |
| `golden/terrain/` | yes | Per level: the scenery's terrain plan as hex bits and a digest of the JS terrain meshes (WP 3.4). |
| `golden/road/` | yes | Per level: the scenery's road plan (fence gaps, unpainted stretches, runout) as hex bits, a hash of the sky along the route, and digests of the JS road, sky and sea: mesh lines, material descriptions, lights, fog (WP 3.5). |
| `golden/flora/` | yes | `valley/flora.js` under Node: every rock, conifer, canopy, grass, flower and shrub template and the foliage material, as hashes and heads of the typed arrays (WP 3.6, D310). |
| `golden/mountain/` | yes | The Sierra Pass scenery of the Sierra export as a digest (`sierra.json`: per child of the group `mountain` its node, mesh line, instances and material; the canvas textures' block means), and its lettered textures drawn by Chrome with the bundled fonts (`textures.json`) (WP 3.6, D312, D313). |
| `golden/animators/` | yes | Level 1's animators time step by time step: Sierra's `world.update` over 16 uneven frames and 360 ticks of 1/120 s, every value it changes under `world.root` (the flag, the waterfall, the windpumps, the creek, the lamps, the aircraft lights, the traffic streams, the sky glow, the dew, the night parameters) per frame, as hex bits and hashes (WP 3.9, D470). |
| `golden/desert/` | yes | Desert Run of the Desert export as a digest (`desert.json`: per node of the groups `desert` and `road` its line and material, the textures' block means, the lights), and its lettered textures drawn by Chrome with the bundled fonts (`textures.json`) (WP 7.3, D551). `golden/animators/desert.json` holds Desert Run's animators the same way as Sierra's (D555). |
| `golden/streets/` | yes | Downtown Streets of the Streets export as a digest (`streets.json`: per node of the groups `streets` and `road` its line and material, the textures' block means), and every canvas of both groups drawn by Chrome with the bundled fonts (`textures.json`) (WP 7.2, D613). `golden/animators/streets.json` holds Downtown Streets' animators the same way as Sierra's (D612). |
| `golden/animators/cruise.json` | yes | The Night City Cruise's animators, as Sierra's: the loop's `world.update` with the camera hopping round the loop, so the chunk cut-off shows and hides City's chunks (WP 7.5, D631). |
| `cache/<key>/scenes/` | no | `<level>.mrscene` and `models.mrscene` scene exports (`crates/mr_scene/FORMAT.md`), each with the digest of the live scene; `<level>.base.mrscene` with `--base` (WP 0.5). |
| `cache/<key>/world/<level>/` | no | `track.{json,bin}` and `terrain.{json,bin}`: the Track arrays and terrain heights (WP 0.5). |

## The hooks

`src/parity/hooks.js` is the first module the game loads. Each hook changes
nothing unless its URL parameter is given:

| Parameter | Effect |
|---|---|
| `kernel=1` | Every inexact `Math` function comes from `mr_math`'s kernel, compiled to wasm (`tools/parity/kernel/mr_kernel.wasm`, rebuilt by `cargo xtask kernel`). Any other inexact one throws. |
| `fixeddt=1` | The race steps in fixed ticks of 1/120 s, `ticks=N` per rendered frame (default 2). Game time follows the ticks, not the clock. |
| `quant=1` | The player's input is quantised as the Rust `InputFrame` is. |
| `seed=N` | Rivals, police and the pursuit draw from seeded streams (`src/parity/sim.js`). |
| `parity=1` | All of the above, seed 1 unless `seed` is given. |
| `fuzz=N` | With `fixeddt`: random controls from `fuzzer(N)` (`src/parity/sim.js`) instead of the autopilot, for the fuzz baseline. |
| `freeze=1` | Scenery animation holds still (`world.update` gets dt 0), for scene exports and screenshots. Not part of `parity=1`. |

In Node, `NODE_OPTIONS=--import=./tools/parity/kernel/register.mjs` installs
the kernel before anything runs (`npm run test:unit:kernel`).

Every capture is taken with the kernel on.

## The tools

| Command | What it makes | Where |
|---|---|---|
| `cargo xtask kernel [--check]` | The math kernel's wasm, checked bit for bit against native Rust | `tools/parity/kernel/mr_kernel.wasm` |
| `NODE_OPTIONS=--import=./tools/parity/kernel/register.mjs node tools/parity/math-golden.mjs [--check]` | `util/math.js` and the JS semantics of SPEC 4.2 over fixed inputs, as f64 bits (WP 1.1) | `golden/math/math.json` |
| `NODE_OPTIONS=--import=./tools/parity/kernel/register.mjs node tools/parity/seaside-golden.mjs [--check]` | Seaside's survey data as `load.js` decodes it: line, grids, features, the samplers at 33,000 points (WP 1.2) | `golden/seaside/survey.json` |
| `NODE_OPTIONS=--import=./tools/parity/kernel/register.mjs node tools/parity/three-geom.mjs [--check]` | The three.js r180 geometry `mr_worldgen::three_geom` ports: every generator over a spread of parameters, Shape/Path, triangulateShape, CatmullRomCurve3, transforms and normals, mergeGeometries and mergeVertices, as hashes and heads of the typed arrays (WP 3.1) | `golden/three_geom/three_geom.json` |
| `NODE_OPTIONS=--import=./tools/parity/kernel/register.mjs node tools/parity/builders.mjs [--check]` | The world builders run under Node: `valley/Builder.js` (Builder, PaintBuilder), `beach/ColorBuilder.js`, `city/geom.js`, Road.js's `extrude` and `runs` over real tracks, `THREE.Color`, the built-in materials' parameters as the export writes them, and World.build's progress labels (WP 3.3) | `golden/builders/builders.json` |
| `NODE_OPTIONS=--import=./tools/parity/kernel/register.mjs node tools/parity/terrain-plan.mjs [--check]` | What the scenery's `plan()` registers with the terrain (flattens, carves, the railway bed) per level, run under Node and checked against the world golden's height hashes; with the cache, a digest of the export's terrain meshes (WP 3.4, DECISIONS D232, D233) | `golden/terrain/<level>.json` |
| `NODE_OPTIONS=--import=./tools/parity/kernel/register.mjs node tools/parity/road-plan.mjs [--check]` | What the scenery's `plan()` registers for the road (`fenceGaps`, `noMarks`, `runout`) per level, run under Node and checked against the world golden; the sky along the route from the JS `Sky` under Node; with the cache, digests of the export's road, sky and sea (WP 3.5, DECISIONS D273, D274) | `golden/road/<level>.json` |
| `NODE_OPTIONS=--import=./tools/parity/kernel/register.mjs node tools/parity/flora.mjs [--check]` | `valley/flora.js`'s templates under Node (WP 3.6, D310) | `golden/flora/flora.json` |
| `node tools/parity/mountain-scene.mjs [--check]` | The group `mountain` of the cached Sierra export as a digest (WP 3.6, D312) | `golden/mountain/sierra.json` |
| `node tools/parity/mountain-textures.mjs [--check]` | The Sierra Pass scenery's lettered textures, drawn by the game in headless Chrome with the bundled fonts (WP 3.6, D313) | `cache/<key>/mountain/`, summary in `golden/mountain/textures.json` |
| `node tools/parity/city-golden.mjs [--check]` | The group `city` of the cached Sierra and cruise exports as a digest: nodes, materials, texture block means (WP 3.8, D354) | `golden/city/<level>.json` |
| `NODE_OPTIONS=--import=./tools/parity/kernel/register.mjs node tools/parity/animators.mjs [--level coast\|streets\|cruise] [--check]` | Level 1's animators: the game's World.build of Sierra under Node (a canvas that draws nothing), then `world.update` over fixed frames and ticks, every value it changes; `--browser` takes the same capture in the game in headless Chrome and writes the same file (WP 3.9, D470); `--level` takes the Coast Highway's (D537), Downtown Streets' (D612) or the Night City Cruise's (D631) | `golden/animators/<level>.json` |
| `node tools/parity/city-textures.mjs [--check]` | City's lettered textures on Sierra and the cruise loop, drawn by the game in headless Chrome with the bundled fonts (WP 3.8, D354) | `cache/<key>/city/`, summary in `golden/city/textures.json` |
| `node tools/parity/desert-golden.mjs [--check]` | Desert Run of the cached export as a digest: the groups `desert` and `road` (nodes, materials, texture block means) and the lights (WP 7.3, D551) | `golden/desert/desert.json` |
| `node tools/parity/desert-textures.mjs [--check]` | Desert's lettered textures, drawn by the game in headless Chrome with the bundled fonts (WP 7.3, D551) | `cache/<key>/desert/`, summary in `golden/desert/textures.json` |
| `node tools/parity/streets-golden.mjs [--check]` | Downtown Streets of the cached export as a digest: the groups `streets` and `road` (nodes, materials, texture block means) (WP 7.2, D613) | `golden/streets/streets.json` |
| `node tools/parity/streets-textures.mjs [--check]` | Every canvas of the groups `streets` and `road`, drawn by the game in headless Chrome with the bundled fonts (WP 7.2, D613) | `cache/<key>/streets/`, summary in `golden/streets/textures.json` |
| `NODE_OPTIONS=--import=./tools/parity/kernel/register.mjs node tools/parity/desert-animators.mjs [--check]` | Desert Run's animators, as `animators.mjs` takes Sierra's, `Math.random` reseeded before the frames; `--browser` in the game (WP 7.3, D555) | `golden/animators/desert.json` |
| `node tools/parity/car-model-golden.mjs [--check]` | The car models of the cached models export (every kind at both detail levels, the far models, the police liveries) as a digest: node lines, materials, texture block means (WP 4.1, D413) | `golden/car_model/models.json` |
| `node tools/parity/textures.mjs [--check]` | The shared textures of `src/world/textures.js` and canvas probes, in headless Chrome with the bundled fonts (WP 3.2) | `cache/<key>/textures/`, summary in `golden/textures/` |
| `node tools/parity/fonts-gallery.mjs` | The font gallery: the game's sign strings in each bundled font and the alternatives, rendered by `mr_canvas` (WP 3.2) | `report/fonts/` |
| `node tools/parity/sim-world.mjs [--check]` | What the simulation needs from the world: runout, the opposite carriageway, every vehicle's dimensions | `golden/sim/world-data.json` |
| `NODE_OPTIONS=--import=./tools/parity/kernel/register.mjs node tools/parity/sim-module.mjs [--check] [--doc]` | Module traces of the staged scenarios (`scenarios.md`, which `--doc` regenerates) | `golden/sim/module/*.trace.gz` |
| `node tools/parity/sim-race.mjs [--check] [--only id,...]` | Whole-race recordings from the real game; `--check` records each twice and compares | `cache/<key>/sim-races/*.trace`, summaries in `golden/sim/races.json` |
| `node tools/parity/sim-fuzz.mjs` | The fuzz baseline: how far bodies get past the walls under random controls | `golden/sim/fuzz.json` |
| `node tools/parity/sim-bench.mjs` | The JS simulation's ticks per second, full Sierra field | `golden/sim/bench.json` |
| `node tools/parity/shots.mjs [--run a]` | Screenshots: every 250 m of every route, chase and high views, and the attract view; frozen, seeded | `cache/<key>/shots/<run>/` |
| `node tools/parity/materials.mjs [--run a]` | Material test scenes: every MaterialKind on a sphere and a plane, bloom chart, fog ramp, shadow edge, standard and physical grids | `cache/<key>/materials/<run>/`, definitions in `golden/materials/scenes.json` |
| `cargo xtask parity shots --a <dir> --b <dir> --label <name>` | Compares two sets of pictures (CIEDE2000, SPEC 12's limits), with a report | `report/shots-<name>/` |
| `cargo xtask parity materials [--only all\|every\|<names>]` | The material test scenes rendered by the JS (if not cached) and by the native Rust client (`--materials`), compared; fails over the limits (WP 2.3, D175, D176) | `cache/<key>/materials/rust/`, `report/shots-materials/` |
| `cargo xtask parity stations [--levels a,b]` | The screenshot stations flown by the native Rust client (`--stations`), compared with the JS shots (WP 2.5, D176) | `cache/<key>/shots/rust/`, `report/shots-stations/` |
| `node tools/parity/rust-web.mjs --level <id> [--query ...]` | The Rust web build (`cargo xtask web --release`) in headless Chrome on WebGPU: waits for `__mr.ready`, saves `__mr.screenshot`, fails on page errors (D176) | `report/rust-web/` |
| `node tools/parity/perf-baseline.mjs` | Desktop frame-rate baseline, fly camera along each route | printed, for `docs/rust-port/BASELINE.md` |
| `node tools/parity/trace-inspect.mjs <trace> [--tick N \| --diff <other>]` | Read a trace: summary, every field of a record by name, or the first differing tick and fields | |

The browser tools drive the game through `test/e2e/harness.js` (headless
Chrome on the GPU, files served from the working tree by request
interception: no server, no port).

## Scene export (WP 0.5)

```
node tools/parity/scene-export.mjs            # all six levels and the models, ~1 min
node tools/parity/scene-export.mjs --base     # terrain, road and sky only
node tools/parity/scene-export.mjs --check    # again, and compare with the goldens and the last run
cargo xtask parity scene-check                # read every scene back with mr_scene, check its digest
```

Each JS material is tagged with its `MaterialKind` (`material.userData.kind`);
the exporter refuses a custom material without one. The export seeds
`Math.random` and uses a fresh Chrome per level, so two runs give identical
files (DECISIONS D20 to D29).
