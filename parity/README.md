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
