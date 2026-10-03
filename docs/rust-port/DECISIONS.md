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
`-Oz` against `-O3`. The `wasm-bindgen` crate is pinned exactly
(`=0.2.129`), because its CLI must match.

The Rust toolchain is pinned too (`rust-toolchain.toml`, 1.99.0). It first
followed `stable`, on the grounds that the compared results are fixed by
`libm` and IEEE arithmetic, not by the compiler; but CI then picked up a
newer stable than the dev machine's and failed on a clippy lint the older
one did not have. Pinning keeps CI and every machine on the same lints.
Upgrade it deliberately, between milestones, and rerun
`cargo xtask kernel --check` when you do.

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

## D12. The trace record carries the input and spells out "missing"

2026-10-03, WP 0.4. `parity/trace-format.md` is the definition. Beyond SPEC
4.6's list it carries each tick's quantised input (so a Rust autopilot that
drifts from the JS one shows up as an input difference, not a physics one),
the counts of every pool up front, the rule state Race keeps in WeakMaps
(each traffic car's last gap to the player and its near-miss mark,
PursuitView's last damaging hit per body), the parking targets as data, and
the four streams' draw counts. A value JS can leave `undefined` or `null` is
written either as an explicit "missing" NaN payload (`0x7FF8_0000_0000_0D1E`)
where the difference matters, or normalised to the value it is read as
(e.g. a police car's `avoid` before its first swerve is 0); the format says
which for each field.

## D13. Small inert hooks for the trace

2026-10-03, WP 0.4. To read state that JS keeps in closures, without
changing behaviour: `Race.parkSpot` returns its parameters beside its two
closures (`kind`, `stopAt`, `laneLat`, `s0`, `lat0`); `Traffic` takes an
optional `rng` (default `mulberry32(seed)` as before), so the reference run
can pass the same seed-99 stream with a draw counter; the `?autodrive=1`
autopilot moved, unchanged, from `main.js` to `src/game/autopilot.js`, so the
Node oracle drives with the same code. `?fuzz=N` swaps the autopilot for
`fuzzer(N)` (`src/parity/sim.js`), a seeded random-controls generator the
Rust fuzz test replays.

## D14. Which races are recorded

2026-10-03, WP 0.4. `tools/parity/sim-race.mjs`: every level in race mode,
each with a different car so all five specs run (Sierra sports, Coast
muscle, Streets super, Desert rally, Seaside electric); the cruise loop for
three minutes; Hot Pursuit from heat 1 on the four levels with police; and
one extra Sierra pursuit from heat 5, which is where roadblocks, spikes and
boxing happen. Seed 1. Each runs until the tick Race reports its results.
The files (about 1.5 to 2 MB each) live in the cache; their summaries
(tick count, final hash, results) are committed in
`parity/golden/sim/races.json`.

## D15. Module scenarios run without Race's rules, gzipped and committed

2026-10-03, WP 0.4. `Race` needs a DOM, so the Node scenarios
(`parity/scenarios.md`) step the modules in Race's order but leave out
Race's own rules and PursuitView's; they keep the crash flag and the
pursuit's hit rules with the PIT yaw kick, which the traffic and police
scenarios need to mean anything. 35 scenarios, 3.3 MB gzipped, committed.
Their metadata holds only id, seed and tick count, so a documentation edit
or an unrelated JS change does not change the files.

## D16. Fuzz and speed baselines

2026-10-03, WP 0.4. The fuzz baseline runs in the real game (so Race's
rules are in it, as they will be in the Rust `step()`), three minutes per
level in race mode and Hot Pursuit (heat 3), `fuzzer(1)`; excursion is
`max(lat + halfW - wallR, -lat + halfW - wallL)` per body per tick, by class
(`parity/golden/sim/fuzz.json`). The speed baseline is the Node full Sierra
field without Race's rules or the trace, with V8's Math and with the kernel
(`parity/golden/sim/bench.json`); it is machine-dependent, so the Rust
benchmark is compared on the same machine.
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
## WP 0.7 decisions

## D40. Math.random in the audio is one seeded stream

2026-10-03, WP 0.7. The audio code draws from `Math.random` for the noise
beds, the tunnel's impulse response, loop start offsets, pops, misfires,
one-shot variations, radio takes and the burble. Every audio capture
replaces it with `mulberry32(1)` (the algorithm of `src/util/math.js`)
before `GameAudio` is built; nothing else draws from it there. So the
Rust `mr_audio` takes one random stream and draws from it exactly where
and in the order the JS calls `Math.random`; in the parity tests it is
`mulberry32(1)`. With that, the Node fake and Chrome build bit-identical
buffers (the renders stage checks it).

## D41. The call log is a facade log played back on a virtual clock

2026-10-03, WP 0.7. A Web Audio call log taken in the browser is not
reproducible: `currentTime` runs on the audio clock, and the music
scheduler, the gate tails and the radio's decode-or-burble race run on
`setTimeout`. So the capture has two halves. In Chrome, the real game
(`?parity=1`, autopilot) logs only the calls it makes on its audio facade,
tagged with the race tick; that log is identical run to run. In Node, the
log is played back into `GameAudio` on a recording Web Audio fake whose
clock is game time (tick k is at k/120 s), with `setTimeout` on that clock
and decoding standing in for Chrome's (`parity/golden/audio/README.md`
has the rules). The Web Audio call log is a pure function of the facade
log. M5 plays the same facade log into `mr_audio` with the null backend
under the same rules and compares the two call logs; this tests what the
web backend will send to the browser, which is what SPEC 7.5 asks of it.

## D42. AudioParam.value reads evaluate the automation timeline

2026-10-03, WP 0.7. The game reads `param.value` twice (`shift` and
`setPursuitMood`) and schedules from it. The fake, and so the null backend,
returns the param's automation timeline evaluated at `currentTime` (Web
Audio 1.0 rules: the value setter is `setValueAtTime(v, now)`, ramps run
from the previous event, a target curve from the value at its start),
rounded to float32 as Chrome stores params. A ramp straight after a target
curve starts from the target's start value (the spec's literal reading);
the game never reads a param in that state.

## D43. Audio arrays at 48 kHz, the big ones by hash

2026-10-03, WP 0.7. The arrays are generated at 48 kHz, the rate desktops
and iPhones run. All of them come to 12 MB and noise does not compress, so
`parity/golden/audio/arrays.json` commits each array's hash and stats and
`arrays-small.bin` the ones up to 16384 floats (every engine wave and
curve, the short kit voices); the whole set is in the cache, which the
`arrays` stage rebuilds in about a second. SPEC 7.3's 1e-5 tolerance for
`samples.js` is applied against the cached arrays.

## D44. Offline renders are compared by band level; Chrome's floor measured

2026-10-03, WP 0.7. Chrome's offline renders are not bit-reproducible:
connections summing into one input are mixed in an order that changes
between runs (differences near 1e-7), and where that reaches an
oscillator's frequency through audio-rate modulation it accumulates in the
phase (up to about 1e-3 on the engine). The reference therefore records
third-octave band levels (`tools/parity/lib/bands.mjs`, one analyser for
both sides), renders each scenario twice and records the larger band
difference as `jitter` (at most 0.21 dB; 0.38 dB between two separate
runs, all on the engines). `--check` accepts 1.0 dB per band and ignores
bands quieter than -90 dB; SPEC 7.5's 1.5 dB for the Rust comparison stays,
since it is about four times Chrome's own floor.

## D45. Render control frames are 8 ms, not a tick

2026-10-03, WP 0.7. An OfflineAudioContext can only suspend on a
128-sample render quantum, and 1/120 s is 400 samples at 48 kHz. The
renders steer the audio every 384 samples (8 ms, three quanta) with
`update(0.008, state)`, and pump the music every 64 frames a second ahead,
as `tools/audio-test.html` does.

## D46. The one `**` in the audio is `x * x`

2026-10-03, WP 0.7. `pulseWave` squares with `Math.cos(...) ** 2`. The
operator does not go through the kernel. V8's `x ** 2` equals `x * x` on a
million random inputs and on every input `pulseWave` gives it, so the port
writes `x * x`. No audio path calls a function the kernel leaves out
(`sinh`, `cbrt`, `expm1`, ...): every capture ran with the kernel on and
nothing threw.

## D47. Radio clips are decoded by Chrome once, then stood in for

2026-10-03, WP 0.7. The Node playback cannot decode MP3. Chrome decodes
every clip in `audio/radio/` at 48 kHz once (`radio-clips.json`: hash,
channels, length); in the playback `decodeAudioData` resolves at once with
a buffer of that length, so a radio line's clips always beat its 700 ms
burble fallback (the game prefetches them at race start, so they do in
practice too). The clip samples are not part of the reference: both ports
decode the same files.

## WP 0.6 decisions

## D17. Screenshot stations

2026-10-03, WP 0.6. `tools/parity/shots.mjs`: the fly camera at every 250 m
of each route, a chase-height view (`h 2.2, back 7, pitch -0.04`) and a high
view (`h 40, back 70, pitch -0.3`), plus the attract camera's first pose
(`s 120, h 7, back 22, lat 3, pitch -0.05`), 1280 × 800, high quality, the
kernel on, the scenery frozen, `Math.random` seeded and a fresh Chrome per
level (D20, D27). Stations are visited in increasing s on one page per level;
at 250 m spacing each one on a sprint level is more than 2.5 % of the route
from the last, so the environment map is refreshed at every station, as it
would be if the page had been loaded there. 398 stations in all.

## D18. The picture metric, read precisely

2026-10-03, WP 0.6. "Quarter resolution" is half the width and half the
height (2 × 2 box filter, averaged in linear light). ΔE is CIEDE2000 on
CIELAB with the D65 white. "95 % of 16-pixel blocks under 6" is read as: the
mean ΔE of each 16 × 16 block of the quarter-resolution image, and the 95th
percentile of those under 6. The JS against itself: 0 on all 398 stations
and all 45 material scenes (the JS renders bit-identically run to run on the
dev machine), so SPEC 12's limits (mean 3, block 6) stand, unraised. The
noise floor of a different GPU or driver was not measured.

## D19. Material test scenes

2026-10-03, WP 0.6. `tools/parity/materials.mjs`. The roadmap says "built
from the exposed JS patch functions"; instead of exporting each patch, a
kind scene takes the live material object the game built (the first mesh
with that kind, levels in order, captured as the scene export captures them,
or the models group the export builds), so the JS game needed no further
hooks. The test geometry is a sphere and a plane carrying the source mesh's
extra attributes set to its first vertex. Everything is data in
`parity/golden/materials/scenes.json`: the common setup (512 × 512, the
game's post chain, a fixed sun with shadows and a hemisphere light, the
level's sky dome as the environment), and per scene the source path, the
patch uniforms as captured, and any overrides. Overrides exist where a
kind's look comes from per-frame state rather than the material: the night
level, a clock, live particles or skid marks, the siren colours, and for the
glows that fade near the camera, a camera further back or the source's own
geometry (`OVERRIDES` in the tool says which and why). Six fixed scenes use
only plain materials: the bloom chart, the fog ramp, the shadow edge, and
standard and physical sphere grids. 37 kinds and 45 scenes.

## M1 decisions

## D50. The shape of `mr_math`'s port of `util/math.js`

2026-10-03, WP 1.1. JS closures that carry state become structs that can
live in a cloned, hashed `SimState`: `mulberry32(seed)` is `Mulberry32`
(its state `a` public, `next_f64()` the closure), `makeNoise2D(seed)` is
`Noise2D` (`noise(x, y)` the closure). Anything that takes an `rng`
function takes `&mut impl Rng`. Default arguments become a second function:
`fbm`/`ridged` take `octaves` with the JS defaults `lac = 2`, `gain = 0.5`,
and `fbm_with`/`ridged_with` take all three; the default `hash2` seed (0) and
noise seed (1) are written out at the call. `hash2` takes its arguments as
`f64` and applies ToInt32 itself, as `Math.imul` does, so a caller cannot
saturate with `as i32` where the JS wraps. Seeds are `u32`: a caller with a
double passes it through `js::to_uint32`, the JS `seed >>> 0`.

## D51. The math golden

2026-10-03, WP 1.1. The roadmap gate "bit-exact against the JS run with the
kernel" is `parity/golden/math/math.json` (`tools/parity/math-golden.mjs`,
checked in CI): every function of `util/math.js` and every helper in
`mr_math::js` over the same inputs on both sides (edge values, then
mulberry32 spreads; noise also at coordinates past 2^31, where `i & 255`
wraps through ToInt32), stored as FNV-1a 64 of the f64 bits plus the first
16 values in hex. NaN is written canonically on both sides, because V8 keeps
whatever NaN bits an operation left (`Math.max(NaN, 1)` stores
0xfff8000000000000). The golden is compiled into the test, so it runs in
wasm too.

## D52. Index loops stay index loops

2026-10-03, WP 1.2. Clippy's `needless_range_loop` is allowed in
`mr_track`, `mr_levels` and `mr_sim`. A JS `for (let k = 0; k < n; k++)`
that reads several arrays at `k` is ported as the same loop, so the port can
be read beside the JS line for line; rewriting it as iterator chains hides
the correspondence and invites reordering.

## D53. Seaside's survey file

2026-10-03, WP 1.2. `tools/seaside/build.py` now also writes
`assets/seaside/survey.bin` (format in its `Survey` docstring): every value
`circuit.js` and `ground.js` hold, from the same Python values, with each
grid's zlib stream as it is (the JS file holds it in base64). Rerunning the
script from its cache reproduces both JS modules byte for byte, so the binary
was written by a real run, not converted from the JS. `mr_levels::survey`
decodes it the way `load.js` does, including where `load.js` stores into a
`Float32Array` (grid values, the blend's scratch buffers) and where into a
plain array (the samplers' results). `tools/parity/seaside-golden.mjs`
writes `parity/golden/seaside/survey.json` (the decoded line, grids and
features, and the four samplers at 33,000 points); the Rust matches it bit
for bit. The photo stays in `src/levels/seaside/` until world generation
needs it (M7).

## D54. Level functions are shared closures; Seaside is prepared

2026-10-03, WP 1.2. A JS level carries functions (`elevation`, `ground`,
`looseGround`, `loop.path`). `mr_track::Level` holds them as
`Arc<dyn Fn ... + Send + Sync>`, so a level is plain to clone and share. As
in the JS, `seaside::level()` is the menu's level without survey data, and
building its Track fails until `seaside::prepare(level, data)` has filled in
the path and the ground. `mr_levels` does not embed the survey: the caller
parses `survey.bin` (`SeasideData::parse`) and passes it in, so the web
client can fetch it as an asset.

## D55. JSON goldens are parsed with `float_roundtrip`

2026-10-03, WP 1.2. serde_json's default float parser is not correctly
rounded: `oppY` at Sierra's first sample came out one unit in the last place
away from the JS value it was parsed from. Every test that reads decimal
numbers from a JSON golden enables serde_json's `float_roundtrip` feature.
Goldens written for M1 store bits in hex instead, which avoids the question.

## D56. Unit tests that need code from later packages move with it

2026-10-03, WP 1.2. From `track.test.js`, "the terrain never covers the
road" needs `Terrain.js` (M3). From `seaside.test.js`, "inside the barriers
the terrain is the run-off" needs Terrain (M3), "loose run-off slows the car"
needs `CarPhysics` (WP 1.3) and "rivals rubber-band on race progress" needs
`AIDriver` (WP 1.4). From `levels.test.js`, the landform and scenery checks
use the JS names until world generation exists, and a rival's kind is
checked against `CAR_SPECS` in `mr_sim`'s tests (WP 1.3). Each is ported in
the package that brings what it needs.

## D57. The shape of `mr_sim::physics` and `vehicle`

2026-10-03, WP 1.3. `CarPhysics` holds only its own state (and its spec,
which is a small copy); the vehicle and the track are passed to `update`,
`step` and `reset`, where the JS object holds references to them. The
`Vehicle` has no pitch and roll springs: physics' landing kick
(`v.pitchV -= impact * 0.02`, on every landing) is emitted as
`PhysEvent::Touchdown { impact }` beside the JS's own `land` event (only
above 2.5 m/s), so the client's springs get the same kick. `accelLong`,
`accelLat`, `brakeLight` and `visY` stay on the Vehicle: physics writes them
and the client reads them. JS fields that start `undefined` (`offTrack`,
`scrapeSide`) are `Option`s and the trace writes them as the JS writer does.
The `cruise` input (the gearbox hint) is carried on `Input` but not in
`InputFrame`, as `quantiseInput` passes it through.

## D58. Module traces are replayed by `mr_sim::staged`

2026-10-03, WP 1.3. The module oracle's staging (`tools/parity/lib/node-sim.mjs`)
is ported as `mr_sim::staged`, which grows with the work packages; the test
`crates/mr_sim/tests/module_phys.rs` transcribes each scenario's input
function from the catalogue in `sim-module.mjs`. The trace record is written
from views of the state (`mr_sim::trace`), so it can follow the JS layout
while the Rust state takes its own shape. The goldens are compiled into the
test, so the replay runs in wasm under Node too: the WP 1.3 traces are
bit-identical native and in wasm. A failing replay keeps its own trace in
`target/parity/<id>.trace` for `trace-inspect.mjs --diff`.

## D59. Agent lists are views; collisions go through a `Body` trait

2026-10-03, WP 1.4. The JS hands drivers lists of car objects and reads
them live, so a rival updated earlier in the tick is seen where it now is;
identity is `o === this`. In Rust a driver gets a slice of `AgentView`s
(s, lat, dir, half sizes, speed along, gap) built from the current state
just before it reads them, and its own index in that slice. Traffic, whose
update moves its own cars while it reads the agent list, takes entries that
are either one of its cars (read live) or a view of another body; the list
is the one built at the start of the tick, so a car despawned during the
update is still read, as in the JS. Collisions take a `BodySet` of
`&mut dyn Body` (velocity, setVelocity, translate, addSpin), which the
staged sim builds over its pools in agent order. Kinematic cars keep the
JS's cached frame (`this.F`), because `velocity()`, `setVelocity()` and
`translate()` use whatever frame was last computed. `yFn` is a `Surface`
enum (road or opposite carriageway). Parking spots are a `Park` enum with
`speed(s)` and `lat(s)`. Random draws come from counted `Stream`s
(`mr_sim::rng`), seeded as `simStreams` seeds them.

## D60. Race rules: one player for now, events, hash

2026-10-03, WP 1.5. `mr_sim::race` ports Race.update's simulation in its
order (SPEC 4.3). The state keeps `players` as a vector and each player's
rule state on its `PlayerCar` (SPEC 4.3), but the rules run for
`players[0]`: the race-wide things the JS does with "the player"
(rubber-banding, traffic kept around it, standings) need the multiplayer
design of M10 before they can mean anything with several. `parkRows` stays
race-wide, as in the JS, since finishers share the parking rows. The
WeakMaps keyed by traffic car (`passed`, `nearMiss`) are arrays by pool
index on the player's rules; like the JS's, they are never cleared when a
car is recycled. Race's HUD, audio, rumble, camera and effect calls are
`SimEvent`s (countdown, GO, perfect start, physics events, car hits,
crashes, near misses, whooshes, bonuses with their text and points, wrong
way, laps, the finish and its place, results, resets). `hash(state)` is the
FNV-1a 64 of the trace record without inputs. The whole-race test checks
every tick against the cached recordings when they are present and, from
the committed `races.json`, always the tick the results came, that tick's
hash and the results, so it also runs in wasm and in CI without the cache.

## D61. Hot Pursuit: racers by index, bodies by id, PursuitView's rules in the race

2026-10-03, WP 1.6. `mr_sim::pursuit` ports Pursuit.js, `mr_sim::police`
PoliceDriver.js. Every body has a `BodyId` (pool and index; police are units
then roadblock cars, as the trace counts them), carried in its `AgentView`
with the `police` and `kinematicOnly` flags the drivers test. A unit's
target is a racer's index in `racers` (player first, then the rivals); the
pursuit reaches the racers' bodies through a small `Racers` trait (their
pose, a rival's finish, holding a rival, spiking a tyre), so the race, the
module staging and the tests each supply their own. `mr_sim::field` holds
what the race and the staging share: the agent list in collision order,
live views, and the collision pass over all five pools. The simulation
half of PursuitView (damage from hits and walls with the half-second
`lastHit` per body, wrecks, holds and `holdControls`, the PIT yaw kick, and
the effects of its events: release onto the road, the barrier's slowdown,
a bust's crash) is `race::PursuitView`, applied at SPEC 4.3's step 17. The
pursuit's events reach the client as `SimEvent::Pursuit`. Pursuit.js's
`hud()` and `propMark()` are presentation and stay with the client (M8).

## D62. The `mr-sim` runner lives outside `src/`

2026-10-03, WP 1.8. `mr-sim` (`crates/mr_sim/bin/mr-sim.rs`) reads the
Seaside survey from a file, writes traces and times its benchmark, which
the simulation library may not (no clock, no I/O; `check-deps` scans
`src/`). A binary in the crate whose source sits in `bin/` keeps the
library's rule intact without a second crate. Its seed is 1 and its car is
the sports car unless told otherwise; `race` stops at the results, as the
recordings do (or after three minutes on the cruise loop).

## D63. Fuzz and determinism are checked against the JS, not only bounds

2026-10-03, WP 1.7. The fuzz test replays `tools/parity/sim-fuzz.mjs`'s ten
runs with the same `fuzzer(1)` and requires what SPEC 4.6 asks (no NaN, no
growth, no body further outside the walls than the JS goes), and, since the
simulation is bit-identical, also the same maximum excursion at the same
tick for every class of body. Native/wasm agreement is shown by the whole
races, the module traces and the fuzz runs passing in wasm against the same
JS-derived numbers. The determinism test steps a clone of a mid-race Hot
Pursuit state beside the original and compares hashes every tick and the
whole state at the end.

## M2 decisions

## D100. Bevy 0.19.1, pinned, with default features off

2026-10-03, WP 2.1. SPEC 2's policy: 0.20 if it is final when M2 begins,
else 0.19.1. On crates.io on 2026-10-03 0.20 is at `0.20.0-rc.2`
(2026-09-28), so the client is on 0.19.1, pinned exactly (`=0.19.1`), and
moves to 0.20 straight after G1, before many shaders exist. Default features
are off; `mr_game` turns on the 3D renderer (`bevy_render`,
`bevy_core_pipeline`, `bevy_pbr`, `bevy_post_process` and the asset, mesh,
image, light, camera, material and shader crates), windowing, states,
logging, keyboard, mouse and touch input, and `png` (screenshots); natively
also `multi_threaded`, `x11` and `wayland`; on the web `web` and `webgpu`.
No UI, text, audio, glTF, gizmos, picking or gamepads yet: the packages that
need them add them. No third-party Bevy plugins. The `wayland` feature links
`libwayland-client`, so CI installs `libwayland-dev`.

## D101. The client's states and how a scene is built

2026-10-03, WP 2.1 and 2.2. States: `Waiting` (for the scene file),
`Building`, `Running`, `Failed`. The build turns a `.mrscene` into Bevy
assets a slice at a time (60 ms of work per frame), so the page's loading
bar keeps moving: textures first, then nodes, then lights and the camera's
environment. Every mesh and image is created `RENDER_WORLD` only and the
`Scene` is dropped at the end, so only GPU copies remain. Transforms are the
export's world matrices (the tree is flattened; nothing moves until
animators exist), with three's hierarchical visibility applied (a node under
an invisible one is skipped). An `InstancedMesh` becomes one entity per
instance, which Bevy batches back into instanced draws; zero-scale instances
are skipped. A multi-material mesh becomes one Bevy mesh per group, cut to
three's draw range. The client is "ready" once the scene is up and the
render world's pipeline cache has nothing waiting (natively pipelines
compile in the background and a mesh draws only once its pipeline is
ready); `--screenshot` waits for that.

## D102. How the client finds a scene in development

2026-10-03, WP 2.2. `?level=<id>` (`--level` natively) names a level, or
`models`; `?scene=<url>` (`--scene <file>`) names a file outright. The
default is the level's export in the parity cache of the current JS tree:
`mr_scene::cache` computes `tools/parity/lib/jstree.mjs`'s key in Rust, the
native client reads `parity/cache/<key>/scenes/<id>.mrscene` under the repo
root, and `cargo xtask web` writes the same directory into
`dist/next/build.json` as the relative URL `../../parity/cache/<key>/scenes/`,
which the registered server (repo root) serves. Every URL is relative. The
page downloads the file itself (for the progress bar) and hands the bytes to
the wasm (`load_scene`), which parses them at once so the page's copy can
go. Seaside's Track needs its survey: the page fetches
`../../assets/seaside/survey.bin`, natively it is read from the repo.

## D103. Stand-in materials

2026-10-03, WP 2.2. Until WP 2.3 and 2.4 port the shading, every kind draws
with Bevy's `StandardMaterial`, lit for three's standard, physical and
Lambert types (and every patch on them) and unlit for basic, line and
points, with the material's colour, map (and its offset, repeat, rotation),
emissive and emissive map, roughness, metalness, clearcoat, side, blending,
alpha test, fog flag and polygon offset (as a depth bias of
-(factor + units) × 32, judged by eye on the road markings). Vertex colours
apply where the material has `vertexColors`. Exposure: three's ACES applies
`exposure / 0.6` before the same fitted curve Bevy's `AcesFitted` uses, so
the camera's `ev100` is `-log2(1.2 × exposure / 0.6)`; unlit colours, the
fog colour and the sky are multiplied by the same factor themselves, and
emission takes the view exposure (`emissive_exposure_weight` 1) as three's
does. The hemisphere light becomes the camera's ambient light (sky 0.75,
ground 0.25, divided by π as three's diffuse is), plus the fog colour at
half the environment intensity for the missing environment map. Fog is
`ExponentialSquared` with three's density; the sun-tinted fog is Bevy's
directional scattering at a low weight. The sky dome draws as its own sphere
with vertex colours from the dome shader's gradient, sun glow and horizon
haze (no clouds, sun disc, moon or stars), and follows the focus as
`Sky.update` moves it. The effect `ShaderMaterial`s (TrafficStreams,
SkyGlow, Surf, LighthouseBeam, Steam, Particles, SkidMarks, PoliceGlow) and
sprites are not drawn: as plain quads they would be white sheets. Points
draw one pixel each. Instance colours ride in Bevy's `MeshTag`, 10 bits a
channel over 0..2, and a small extension of the standard fragment shader
(`crates/mr_game/src/tint.wgsl`) multiplies the base colour by them, so
instances keep one material per JS material (Sierra: 119 materials, not the
1,838 a material per distinct colour gave). Canvas textures are flipped on
upload where three flips them; mip chains are made on the CPU (2 × 2 box
filter, sRGB averaged in linear light) where three generates them, and
anisotropy is set only where WebGPU allows it (linear filtering throughout).

## D104. The models scene is laid out in a grid

2026-10-03, WP 2.2. `models.mrscene` holds every model at the origin
(D29). For viewing, the client places each child of its root in a grid
(eight to a row, 7 m by 12 m), centred on the child's drawn nodes, adds a
sun (it has no lights of its own), and frames the grid.

## D105. The fly camera and the time of day

2026-10-03, WP 2.2. `?s=` starts the JS debug fly camera with its
parameters and defaults (`s`, `h` 5, `back` 14, `lat` 0, `v` 0, `yaw` 0,
`pitch` -0.08), on the Rust `Track` from `mr_track` and `mr_levels`;
`flyCamera` is ported line for line, with `dt` clamped to 1/20 s as the JS
frame loop does. Without `s` the client runs the menu's attract camera
(16 m/s along the first zone from s = 120, h 7, back 22, lat 3, pitch
-0.05). Up and Down change the fly camera's speed by 10 m/s, a convenience
the JS does not have. The sky, fog and lights are the export's, captured at
the route's start (D24), and do not follow the route's time of day yet: on
the sprint levels the light at the far end differs from the JS until the
sky is ported (WP 2.3 and M3).

## D106. The web page, the gesture bridge and the test hooks

2026-10-03, WP 2.1. `crates/mr_game/web/index.html` checks
`navigator.gpu.requestAdapter()` before downloading anything (asking up to
four times: headless Chrome answers null while its GPU process starts) and
shows a plain "no WebGPU" page naming the browsers to use. It downloads the
wasm and the scene with progress, starts the app before the scene arrives
(so the first frame is early), and forwards pointer-up, touch-end, click and
key-down to the wasm's `gesture()` inside the handler; the wasm side only
counts them for now (audio, fullscreen, the landscape lock and the motion
permission arrive in M5 and M6). The wasm publishes its state on
`window.__mr` (`state`, `progress`, `ready`, `frames`, `firstFrameMs`,
`readyMs`, `counts`, ...), and `?stats=1` shows a panel with the frame rate
and worst frame measured as the JS game's panel does.
High quality (shadows on) defaults as in the JS: on, except on touch
devices; `?hq=0` or `?hq=1` overrides it. The shadow map is one cascade
to 140 m (about the JS's ±70 m box) at 2048², with Bevy's own biases.
`__mr.screenshot(name)` saves the next frame through Bevy's screenshot as
a download: headless Chrome does not composite a WebGPU canvas into its own
screenshots. Headless Chrome on the dev machine gets the hardware adapter
with `--enable-unsafe-webgpu --enable-features=Vulkan --use-angle=vulkan
--ignore-gpu-blocklist`. Scenes over about 100 MB cannot be answered
through the harness's request interception (the tab dies), so the larger
levels are checked through the registered server.

## D107. The dev server sends precompressed files under `dist/`

2026-10-03, WP 2.1. `cargo xtask web --release` writes `mr_game_bg.wasm.gz`
and `mr_game.js.gz`; `tools/serve.py` sends the `.gz` with
`Content-Encoding: gzip` (and the original type) for a file under `/dist/`
when the browser accepts gzip and the `.gz` exists, and sends `.wasm` as
`application/wasm`. Everything else is served as before.
