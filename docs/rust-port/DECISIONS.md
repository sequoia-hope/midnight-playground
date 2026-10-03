# Decisions the spec did not cover

Choices made while executing the roadmap, where SPEC.md and ROADMAP.md were
silent. Each one took the option that best preserves parity (ROADMAP working
rule 4). The owner can overturn any of them; add a line saying so.

Format: number, date, work package, the decision, why.

## D1. Hash maps are banned outright in the simulation crates

2026-10-03, WP 0.1. SPEC 3.2 forbids hash-map *iteration* in `mr_math`,
`mr_track`, `mr_levels` and `mr_sim`. `check-deps` forbids the types
(`HashMap`, `HashSet`) instead: a source scan cannot tell lookup from
iteration, and SPEC 4.2 wants JS insertion order, which a vector of pairs or
an index-keyed array gives and a hash map never does.

## D2. `check-deps` also scans for platform float functions

2026-10-03, WP 0.1. SPEC 4.2 says the simulation and world generation never
call `f64::sin` and friends. `check-deps` enforces it by scanning the
sources of `mr_math`, `mr_track`, `mr_levels`, `mr_sim`, `mr_worldgen` (and
the kernel's wasm crate) for `x.sin(`, `f64::sin(` and the like: every
inexact function, plus `powi` (not the same rounding as `Math.pow`) and
`mul_add` (fuses a rounding away). Comments are skipped. `mr_canvas`,
`mr_audio` and `mr_game` are not scanned: their output is compared with
tolerances, not bit for bit.

## D3. Banned crates are checked on the default feature set for every target

2026-10-03, WP 0.1. `cargo tree --target all -e normal` with default
features. For world generation, `rayon` appearing there means it is not
behind a non-default feature, which is what SPEC 3.2 requires. The
simulation crates additionally ban `getrandom`, `rand*`, `rayon*`,
`crossbeam*`, `js-sys` and `wasm-bindgen*`, which carry ambient randomness,
threads or the browser.

## D4. The web build profile

2026-10-03, WP 0.1. `cargo xtask web --release` builds the `web-release`
profile (fat LTO, one codegen unit, `panic = "abort"`) and runs
`wasm-opt -Oz`. SPEC 11 allows size optimisation where a benchmark shows no
frame-time cost; there are no frames to time until M2, so WP 2.6 revisits
`-Oz` against `-O3`. The Rust toolchain follows stable (`rust-toolchain.toml`)
rather than a pinned version: the results the port compares are fixed by
`libm` and IEEE arithmetic, not by the compiler. The `wasm-bindgen` crate is
pinned exactly (`=0.2.129`), because its CLI must match.

## D5. The WP 0.1 page reports WebGPU and secure context

2026-10-03, WP 0.1. The bare wasm page prints whether `navigator.gpu` exists
and whether the page is a secure context, so opening it on a candidate
reference phone over the tailnet already answers the first question of G1.

## D6. The kernel includes `log10`

2026-10-03, WP 0.3. SPEC 4.2 lists thirteen functions as the full set the
game and three.js use. three.js's `BufferGeometryUtils.mergeVertices` also
calls `Math.log10`, and world generation calls `mergeVertices` (desert props
and parts, valley flora), so a kernel-on world build would have thrown.
`log10` joins the kernel. The functions that throw when the kernel is on are
the remaining inexact ones: `sinh`, `cosh`, `asinh`, `acosh`, `atanh`,
`cbrt`, `expm1`, `log1p`.

## D7. The kernel's `pow` and `hypot` follow ECMAScript at their edges

2026-10-03, WP 0.3. C's `pow(1, NaN)` and `pow(-1, ±inf)` are 1; JS's are
NaN. C's `hypot` is left to `libm`, but the kernel spells out JS's rule (any
infinity gives +Infinity, else any NaN gives NaN) rather than rely on it.
Otherwise the kernel-on reference could take a branch the live game does not.

## D8. The kernel's wasm is committed

2026-10-03, WP 0.3. `tools/parity/kernel/mr_kernel.wasm` (20 KB) is in git,
so the JS oracle runs, and `npm run test:unit:kernel` passes, without a Rust
build, and the `js-reference` tag carries the exact kernel its goldens were
taken with. `cargo xtask kernel` rebuilds it and checks a million inputs per
function against native Rust; CI runs `cargo xtask kernel --check`, which
checks both a fresh build and the committed file and writes nothing. The
check hashes outputs with every NaN taken as the canonical quiet NaN, since
wasm does not fix NaN payloads (a NaN in the simulation is a bug either way).

## D9. How the hooks are switched on

2026-10-03, WP 0.3. URL parameters, read by `src/parity/hooks.js`, the first
module `main.js` imports, so the kernel is in place before any other module
computes at load time (it fetches the wasm synchronously for that reason):
`kernel=1`, `fixeddt=1` (with `ticks=N` per frame, default 2), `quant=1`,
`seed=N`, or `parity=1` for all of them with seed 1. In fixed-dt mode the
input layer is also read once per tick and the frame's game time is the ticks
run, so everything (camera, effects, scenery) follows game time, not the
clock. The e2e harness adds `MR_QUERY` to every page, so the whole suite runs
with any hook on (`MR_QUERY=kernel=1 npm run test:e2e`).

## D10. Generator parameters default to `Math.random`, one per stream

2026-10-03, WP 0.3. `AIDriver` takes `opts.rng`, `PoliceDriver` a fourth
argument `rng`, `Pursuit` a `policeRng` beside its existing `rng`, and
`PursuitView` passes `rng` and `policeRng` through; `Race` takes `rngs`
(`{ ai, police, pursuit }`, from `simStreams(seed)` in `src/parity/sim.js`).
Every default is `Math.random`, including `policeRng` when only `rng` is
given (the unit tests pass a seeded `rng` to `Pursuit` and leave the police
weave on `Math.random`, as before). The streams count their draws, so a
trace can record where each one is.

## D11. Input quantisation

2026-10-03, WP 0.3. `quantiseInput` (`src/parity/sim.js`) turns the steer
into `Math.round(clamp(steer, -1, 1) * 32767) / 32767` and throttle and brake
into `Math.round(clamp(x, 0, 1) * 255) / 255`; `handbrake`, `nitro` and
`analog` become booleans. The Rust `InputFrame` stores the integers and
divides the same way, so both sides integrate identical values. It applies
to the input from the player's devices or the autopilot, after the
autopilot, not to the controls the race itself produces (the cool-down
driver after the finish, the penalty hold).

## WP 0.5 decisions

## D20. The scene export seeds `Math.random`

2026-10-03, WP 0.5. World generation draws from `Math.random` in two
places: the steam puffs' `aSeed` attribute (`world/streets/props.js:415`)
and the plank noise of the wooden signs (`world/Valley.js:53`); desert
`paint()` also draws (with zero jitter, so without effect) when no `rng` is
given. Left alone, those two would differ on every export. The exporter
(`tools/parity/scene-export.mjs`) replaces `Math.random` with mulberry32
seeded 0x5eed before the page's scripts run (the harness's `init` hook), so
the game itself is untouched and the exports are reproducible. The world
generator port meets these two sites as a known gap: matching them needs
the same stream in the same draw order, or a recorded deviation.

## D21. MaterialKind: the kinds beyond SPEC 6.2's table, and kind options

2026-10-03, WP 0.5. `Points` joins the built-in kinds: a `PointsMaterial`
without a patch (glow points, lantern points) is in eleven places and the
table had no plain kind for it. `CityAtlas` (`patchAtlasMaterial`) stays a
kind though nothing calls it at present. `TriplanarRock` covers both
`Mountain.js` and `coast/kit.js` (the same GLSL). Where a JS patch takes
values from its closure rather than from the material, the tag carries them
as `kindOpts` and the file as `kind_opts`: `Terrain` {packed, photo},
`Siding` {mode}, `FlickerPoints` {rate, depth, blink}, `AmbientProp` {rgb}.
The material's `customProgramCacheKey()` is exported too, as `program_key`.

## D22. How the JS carries the tags

2026-10-03, WP 0.5. A tag is `material.userData.kind = 'Kind'` (plus
`userData.kindOpts`), set beside each `onBeforeCompile` assignment and each
`ShaderMaterial`: a plain property three never reads, so the game renders
exactly as before. Built-in materials without a patch are classified by the
exporter from `material.type`. The exporter refuses an untagged material
that has its own `onBeforeCompile` or is a `ShaderMaterial`, an unknown kind
name, and a tag on a material with no patch (which would mean a clone that
lost its patch: three's `clone()` copies `userData` but not
`onBeforeCompile`).

## D23. Patch uniforms are read from the compiled program

2026-10-03, WP 0.5. Most patches create their uniforms inside the
`onBeforeCompile` closure (`sh.uniforms.tRock = { value: tex }`), where no
one can reach them. The exporter calls `renderer.compile()` on the scene
(which builds any program not built yet and changes nothing in the scene)
and reads the uniforms three keeps per material (`renderer.properties`),
minus the ones three's own shader for that type has. That is three
internals, read only, in a tool.

## D24. Where and when a level is captured

2026-10-03, WP 0.5. `?level=<id>&kernel=1&freeze=1&s=0`: the fly camera at
the route's start (h 5 m), scenery frozen, after three frames. Camera
dependent state is recorded as it is there: visibility (the cruise loop's
City chunks beyond 2000 m are exported with `visible: false`), the sky and
fog at that point of the route, the night-scaled material values (with
`night_params` giving day and night values), the sky dome's position.
Night parameters of materials no mesh uses (the road chevron on levels
without chevrons) are left out. Desert tumbleweeds not yet launched are
zero-scale instances, as in the game.

## D25. Scene file choices

2026-10-03, WP 0.5. Buffers are an accessor table (glTF style), shared by
index wherever three shares the array, so a texture cloned from another
(same image) and attributes shared between geometries are stored once.
Texture pixels are stored decoded (RGBA8 or R8, as `getImageData` or the
`DataTexture` holds them), including Seaside's photo (decoded by Chrome;
`url` names the JPEG for a loader that prefers the file). Nodes carry both
the local and the world matrix. An `InstancedMesh` is exported as type
`InstancedMesh` (three leaves its `type` as `Mesh`), with per-instance
custom attributes in its mesh marked `instanced`. Material parameters are
every own property of the material, as JSON with tagged colours, vectors,
matrices and texture references. The format is
`crates/mr_scene/FORMAT.md`.

## D26. The digest, and what is committed

2026-10-03, WP 0.5. The digest that proves a file matches the live scene is
computed on both sides with the same IEEE operations in the same order
(counts, f32 bounds, SHA-256 of every attribute, index, pixel array and
instance array, triangle area and centroid, world bounds over every vertex
of every instance), and must match exactly. A full digest is 0.3–0.4 MB per
scene, so it lives in the cache beside the scene; `parity/golden/world/`
commits per level the Track array and terrain height checksums, the scene's
counts and kinds, and the SHA-256 of the full digest
(`cargo xtask parity scene-check` checks the cached digest against it).

## D27. A fresh Chrome for every level

2026-10-03, WP 0.5. Exporting the levels one after another in one browser
made Desert's sign atlases (`world/beach/atlas.js`) differ by one or two
units in a few hundred pixels from a run where Desert came first: canvas
text and blur drawn on the GPU depend on what earlier pages drew. Each
level, and the models, get their own browser; three runs in two orders then
gave byte-identical files.

## D28. Track and terrain dumps

2026-10-03, WP 0.5. The Track dump is every own property of `world.track`
after the world is built, in insertion order: typed arrays as their bytes,
plain values as JSON; `level`, functions (`looseAt`) and the spatial hash
(a `Map`) are skipped. Terrain heights (`terrain.heightAt`) are taken at
10,000 points per level: 6,000 on and beside the road (s evenly spaced,
lateral offset from a golden-ratio sequence over ±60 m) and 4,000 from a
Halton (2, 3) sequence over the terrain's bounds. The points are stored
with the heights (f64), so the Rust test does not need the Track port to
regenerate them.

## D29. The models scene

2026-10-03, WP 0.5. `models.mrscene` holds every car kind at high detail
and at low detail with its far model (as traffic and police build them),
the police liveries of the muscle and sports cars at both, all with seed 0
and default colours; the effects (smoke, sparks, skid marks, a headlight
pool and nitro flames on a sports car); and the pursuit props: a sawhorse
and a spike strip laid on Sierra's road. `sawhorseModel` and `spikeStrip`
in `game/PursuitView.js` are exported for this, which changes nothing.
