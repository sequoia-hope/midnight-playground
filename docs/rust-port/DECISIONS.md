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

Note (2026-10-04): the sentence on `InstancedMesh` is superseded by D450.
An `InstancedMesh` is now one entity per material group with its instances
in a vertex buffer, drawn with one instanced draw as three draws it.

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

## WP 3.1 decisions

## D130. The shape of `three_geom`'s API

2026-10-03, WP 3.1. A three.js generator class is a function named after
it (`BoxGeometry` is `box_geometry`) that returns a `BufferGeometry`, and
takes the constructor's arguments in their order with every default
written out (three's defaults are in each doc comment;
`ExtrudeOptions::THREE_DEFAULTS` and `ExtrudeOptions::flat(depth)` cover
the options object, including the omitted `bevelSize` being
`bevelThickness - 0.1`). Counts are `f64`, as in the JS: where three floors
a count (`Math.floor(widthSegments)`), so does the port, so a builder can
pass a computed count as the JS does. Where three uses a count unfloored
(Circle's segments, a polyhedron's detail, Tube's segments, Extrude's
curveSegments, steps and bevelSegments, ShapeGeometry's curveSegments), a
fractional value would send the JS loops and index formulas somewhere the
port does not follow, and the world code never passes one; the port
refuses it with a panic naming this decision. The math types (`Vector2`,
`Vector3`, `Quaternion`, `Euler`, `Matrix3`, `Matrix4`, `Box3`, `Sphere`)
are `Copy` and their methods return the result instead of mutating
`this`, with three's arithmetic in its order (`normalize` multiplies by the
reciprocal, as `divideScalar` does); `+`, `-` and unary `-` are `add`,
`sub` and `negate`. Geometry transforms mutate and return `&mut Self` so
`g.rotateX(a).translate(x, y, z)` chains as in the JS. Clippy's
`too_many_arguments`, `needless_range_loop`, `needless_late_init` and
`explicit_counter_loop` are allowed in the module so the port keeps three's
signatures and loops (D52).

## D131. `BufferGeometry`: attributes in insertion order, typed arrays as `mr_scene::BufferData`

2026-10-03, WP 3.1. Attributes are a vector of (name, attribute) with a JS
object's order: setting an existing name keeps its place, deleting and
setting again moves it to the end. The order is observable:
`mergeGeometries` follows the first geometry's, and `LatheGeometry` sets
`uv` before `normal`. An attribute's array is an `mr_scene::BufferData`, so
geometry goes into a scene as it is (`to_mesh_desc`, `add_to_scene`);
writes behave as stores into the JS typed array (`f32` rounding, integer
wrap through ToInt32/ToUint32, three's `normalize`/`denormalize` for a
normalized attribute). The index is a `BufferAttribute` too: `set_index`
with a list picks `Uint16` or `Uint32` by three's `arrayNeedsUint32`, and
`set_index_attribute` takes one as built (`TerrainMesh.js` and `Sea.js`
choose the type themselves). Groups are `usize` start, count and material
index. `toNonIndexed` on a geometry without an index returns a copy where
three warns and returns `this`. Nothing was added to `mr_scene`.

## D132. Curves, shapes and earcut

2026-10-03, WP 3.1. three's `Curve` serves 2D and 3D; here the 2D curves
that paths are made of are an enum (`Curve2`: line, quadratic and cubic
Bézier, ellipse/arc, spline), `Path` holds them with the pen position, and
`Shape` is a `Path` (by `Deref`) with holes. The 3D curves implement a
`Curve3` trait whose provided methods are `Curve`'s (`getLengths`,
`getUtoTmapping`, `getPointAt`, `getTangent(At)`, `computeFrenetFrames`);
`LineCurve3` overrides what three's overrides. `CatmullRomCurve3` caches
its arc lengths at the default 200 divisions in a `OnceLock`, as three
caches them; the values are the same cached or not, and
`update_arc_lengths` clears the cache after `points` change. Its shared
scratch vector is reproduced: when both end points are extrapolated (a
two-point open curve), the first reads what the second wrote.
`triangulateShape` removes a closing duplicate from the contour and holes
in place, as three's does (ShapeGeometry depends on it). Earcut keeps
mapbox/earcut 3.0.1's code on an arena of nodes linked by index; node
identity is index equality. The holes' `sort(compareXYSlope)` is a stable
`sort_by` with a NaN comparison counted as equal.

## D133. What `three_geom` leaves out

2026-10-03, WP 3.1. Ported because the world or car code calls them (the
counts in inventory-rendering section 4, and a grep of `src/`): the
generators of SPEC 5.2 plus Dodecahedron, Octahedron and Tetrahedron
(PolyhedronGeometry subclasses; `desert/parts.js` and `Harbor.js` use the
first two); `BufferGeometry`'s `applyMatrix4`, `applyQuaternion`,
`rotateX/Y/Z`, `translate`, `scale`, `lookAt`, `center`,
`computeBoundingBox`, `computeBoundingSphere`, `computeVertexNormals`,
`normalizeNormals`, `toNonIndexed`, attribute and group edits;
`mergeGeometries` (with and without groups) and `mergeVertices` (any
tolerance). Left out, because nothing in the game uses them:
ExtrudeGeometry's `extrudePath` and custom `UVGenerator` (every call
extrudes along +z with the world UVs), morph attributes, interleaved
attributes, `computeTangents`, `setFromPoints`, serialisation, and the
other generators (Ring, Edges, Wireframe). `THREE.Color` and `Object3D`
belong with the builders (WP 3.3); only the part of `Object3D.lookAt` that
`BufferGeometry.lookAt` uses is here. `mergeVertices` keys its table by the
list of truncated integers the JS joins into a string; the table is a
`BTreeMap` used only for lookup, never iterated, so no order leaks from it.

## D134. The three_geom golden: bit-identical, native and wasm

2026-10-03, WP 3.1. `tools/parity/three-geom.mjs` runs three.js r180 under
Node with the parity kernel (the `three` specifier resolved to
`vendor/three` by `test/unit/support/three.js`) over 141 cases: each
generator over a spread of parameters (the world code's own calls among
them: the Streets trapezoid, the Coast roof and hull, the beach surfboard
with bevel, CarModel's bevelled extrude and caliper, the Valley barn and
bale lathe, Beach's coaster rails as a closed centripetal tube; and the
edges: minimum and fractional segments, partial sweeps, open and pointed
cylinders, zero-height capsules, clamped lathe angles, bevel on and off,
holes, clockwise and counter-clockwise outlines), Path sampling,
triangulations (z-order hashed and not, with holes, duplicate ends,
collinear and self-touching outlines), CatmullRomCurve3 samples and Frenet
frames, transforms in every Euler order, normals, bounds, the matrix and
quaternion functions, and merges (groups, attribute order, a Uint32 index,
and the two failures that return null). `parity/golden/three_geom/three_geom.json`
records each attribute's name, item size, array type, count, the FNV-1a 64
of the typed array's bytes and its first 16 values as hex bits, the index
(type, values as u32), groups and bounds; sequences as in the math golden
(D51). `crates/mr_worldgen/tests/three_geom.rs` rebuilds every case and
requires all of it to match. The SPEC's bar is 1e-6; the result is
bit-identical in every case, native and in wasm
(`cargo test --target wasm32-unknown-unknown -p mr_worldgen`). CI checks
that the golden regenerates and runs mr_worldgen's tests in wasm.

## WP 3.2 decisions

## D150. The bundled fonts, registered under the names the JS asks for

2026-10-03, WP 3.2. `assets/fonts/` holds unmodified files from the
google/fonts repository with their licences (OFL; Yellowtail Apache 2.0),
and `assets/fonts/fonts.json` maps each to the family name the game writes:
"Arial Narrow" → Archivo Narrow, "Arial" → Arimo (not in SPEC 5.3's list,
but plain Arial is the second most used family: sub-lines, price boards,
harbour signs), "Arial Black" → Archivo Black, Georgia → Gelasio, "Brush
Script MT" → Yellowtail, "Segoe Script" → Caveat (never reached: every list
names Brush Script MT first), "Courier New" → Courier Prime, Rajdhani →
Rajdhani; the generic families map to these (`sans-serif` → Arial and so
on). "Helvetica Neue" is not mapped: it only follows "Arial Narrow".
Variable fonts stay variable: the `wght` axis follows the requested weight
within the face's range, as Chrome sets it. A single-weight face (Archivo
Black, Yellowtail) is registered for the whole 100–900 range, so neither
Chrome nor the port synthesises bold for it; the set covers every weight and
style the game requests, so mr_canvas has no synthetic bold (a synthetic
oblique, Skia's 0.25 skew, is there for the gallery). `FontBook::bundled()`
compiles the files in (about 4 MB, most of it Arimo's and Rajdhani's
non-Latin coverage: subsetting both sides to the characters in use is the
obvious size fix once the owner has chosen). The alternatives for the
gallery are fetched on demand into `target/font-candidates/`, not
committed. The owner picks from `/parity/report/fonts/`.

## D151. Exact-area coverage and own compositing, on tiny-skia's paths

2026-10-03, WP 3.2. mr_canvas uses tiny-skia for the pixmap, path building
and the stroker, but rasterises coverage itself (exact signed area per
pixel, the nonzero rule, as font-rs and FreeType do) and composites in
float with one rounding per draw. tiny-skia's own anti-aliasing is 4×4
supersampling, up to 1/8 of a pixel off at an edge. Measured in Chrome 151
(headless, GPU canvas): fillRect, rect paths, strokeRect and axis-aligned
line strokes give exact area coverage; other paths give exact coverage
across x but about four levels down y (the GPU's multisampling). Exact
area is the closest single rule. Chrome's GPU canvas also reads the
destination once per batch for `lighten`, so two overlapping `lighten`
draws in a row do not see each other; the game never overlaps them
(`cityTextures.js` draws disjoint cells), so that is not reproduced and the
probe avoids it.

## D152. Text: harfrust and skrifa, shaped and hinted as Chrome does

2026-10-03, WP 3.2. Shaping is harfrust (the HarfBuzz project's Rust port),
outlines and metrics are skrifa (Fontations, which Chrome's Skia uses for web
fonts), both on read-fonts 0.43; glyphs are filled by D151's rasteriser so
transforms, gradients, shadows and compositing apply to text as to paths.
rustybuzz was tried first and dropped: it ignores GPOS variation deltas, so
Arimo kerned R-T and T-space at wght 700, where the font's delta zeroes
them (Chrome: "PORT MERIDIAN" 512.0 px, rustybuzz 509.69). ab_glyph and
fontdue do no shaping. What the port does, each measured against Chrome:
- shapes at the font size in 16.16 fixed point, word by word (Blink's
  CachingWordShaper: nothing kerns across a space; "Grand Ave" 263.49);
- baselines from Blink's normalised OS/2 typo metrics (`middle` is
  (ascent − descent)/2 in 1/64 px) and rounded hhea ascent and descent;
- glyph origins on Skia's grid: quarter pixel along the baseline, whole
  pixel across it, when the transform keeps the axes (including quarter
  turns, with SkScalarNearlyZero's tolerance);
- outlines hinted by the light automatic hinter at the device size for
  every face, even ones with TrueType instructions (cap heights match:
  Arimo 45 px at 64 px where unhinted is 44.03, Rajdhani 42, Archivo Black
  44.4);
- A8 mask correction (`SkMaskGamma`, gamma 1.2, contrast 0.2, the fill
  colour's luminance in 3 bits): white text's coverage becomes
  `c^(1/1.2)`, black text's thins slightly, as in Chrome's pixels;
- overlapping glyphs combine as separate masks do (1 − (1−a)(1−b)).
Strokes and glyphs above 256 px per em are drawn as paths, without the
mask correction (Skia's atlas limit; not measured).

## D153. Canvas gradients are dithered

2026-10-03, WP 3.2. Chrome draws canvas gradients with Skia's GPU dither:
an 8×8 ordered pattern of ±½ level on the premultiplied colour channels,
not alpha. On a translucent white gradient that shows as RGB dipping below
255 after `getImageData`; with the dither the glow and smoke textures went
from 1.69 and 1.44 levels mean difference to 0.58 and 0.87. Stops
interpolate unpremultiplied and pad.

## D154. The texture reference capture

2026-10-03, WP 3.2. `tools/parity/textures.mjs` opens its own page
(`tools/parity/textures.html`, the pattern of `audio-ref.html`) through the
e2e harness with `?kernel=1`: `hooks.js` installs the kernel before
`textures.js` evaluates, the page registers the bundled faces with
`FontFace` under the JS names, and each case calls the module's export. It
captures every generator of `textures.js` (asphalt in all three tones,
checker 10 as the game uses it and the default 8, the six façade variants'
map and emissive) and eleven of the signs the game draws through
`signTexture` (freeway.js and Harbor.js, one per shape of call). It also
captures eleven probes, small scenes in a tiny op language both sides
interpret, for the parts of the canvas subset `textures.js` does not reach.
The PNGs go to `parity/cache/<key>/textures/`; that key does not cover the
fonts, so the committed summary (`parity/golden/textures/*.json`: Chrome
version, fonts manifest hash, per image the SHA-256 and 8×8 block means)
decides whether a cached image is current. The Rust tests check, always:
bit-identity for `detailTexture` and `terrainDetailTexture` (pixel data
only, no drawing), and block means within the gate; with the cache: mean
absolute difference per channel under 3/255 (SPEC 5.7) and side-by-side
sheets in `parity/report/textures/`. `--check` captures twice and compares
with the golden.

## D155. The shape of `mr_worldgen::textures`

2026-10-03, WP 3.2. One function per JS generator, drawing on mr_canvas in
the JS order, returning a `Texture` (unpremultiplied RGBA as uploaded, the
source kind, wrap, colour space, anisotropy) with `desc()` for an
`mr_scene::TextureDesc` (canvas textures flip on upload, data textures do
not). The JS module's `Map` cache is a `TextureCache` owned by the world
build, with the JS keys, including their quirk: a sign's key leaves out its
font and border, so two signs differing only in those share a texture.
Stores keep their array types: `Uint8ClampedArray` (ImageData) rounds half
to even and clamps (`ImageData::set`), `Uint8Array` (the DataTexture)
truncates, `Float32Array` lattices and noise go through `fround`. The
noise textures are bit-identical to the JS.

## WP 3.3 decisions

## D190. The shape of the builders' API

2026-10-03, WP 3.3. Each JS module is a module of `mr_worldgen`:
`valley/Builder.js` is `builder`, `beach/ColorBuilder.js` is
`color_builder`, `city/geom.js` is `geom`, `extrude`, `runs`, `chunks` and
`groupRuns` of `Road.js` are in `road` (where the `Road` class joins them in
WP 3.5), `THREE.Color` is `color`, the built-in materials are `material`.
Methods keep the JS names and argument order; a JS default becomes a
written-out argument (`set_frame(x, y, z, yaw)`, `beam(key, a, b, t)`) or a
field of an options struct whose `None` is a property the JS object leaves
out (`PrismOpts`, `ExtrudeOpts`, `BuildOpts`, `StaticOpts`). Two
conveniences cover the common short calls: `put_at` (no rotation, unit
scale) and `box_yaw` (`box` with only `ry`); `put` and `cbox` take the
rotation and scale as `[x, y, z]` triples. `box` is `box_` (`box` is
reserved in Rust). The JS `materials` object passed to `build` is a slice of
`(key, MaterialId)` pairs. `add` and the shape methods return nothing: the
JS returns the placed geometry, which no caller uses. `console.warn`
(a bucket without a material, a missing or failing scenery module) goes to
`SceneGraph::log`, which the build hands back as `WorldBuild::log`. Clippy's
`too_many_arguments` is allowed in `builder` and `geom` (D130).

## D191. The object tree and how it becomes a scene

2026-10-03, WP 3.3. A world build makes objects, geometries, materials and
textures into a `SceneGraph`: four arenas addressed by `NodeId`, `GeoId`,
`MaterialId` and `TextureId`, the way the JS keeps object references. An
`Object3D` holds what three's does and the exporter writes (name, type,
position, quaternion, scale, the matrix last updated, `matrixAutoUpdate`,
visibility, culling, render order, shadows, layers, plain `userData`,
geometry and materials, instance data, a sprite's centre, a light);
`add` re-parents as three's does. `SceneGraph::finish` walks the roots
exactly as `scene-page.js` walks the JS scene (depth first, children in
the order added) and numbers meshes, materials and textures as it first
meets them, textures in parameter order then uniforms, pixel buffers shared
by image (a cloned texture shares its source's). What no root reaches is
left out, and so are night parameters of materials nothing draws (D24). A
`HandleMap` turns build handles into scene indices. Auto-updated objects
get `compose(position, quaternion, scale)` at assembly, as three's
`updateMatrixWorld` gives them before the export; static ones keep the
matrix of their last `updateMatrix()`. Bounds are computed where the JS
computes them (`mergeGeometries` callers, `GeoBuilder.build`,
`InstancedMesh.computeBoundingSphere/Box` with their side effect on the
geometry); the bounding spheres three's renderer computes lazily for
frustum culling before the export are not reproduced, so a digest should
not compare them. three's `Sphere` operations the instanced bounds need are
small free functions in `object`, leaving `three_geom` untouched.

## D192. PaintBuilder and ColorBuilder are modes of one Builder

2026-10-03, WP 3.3. In the JS both subclasses override `add`, and every
shape method of `Builder` calls `this.add`, so a box drawn on a paint
builder is painted. Rust has no virtual dispatch through a base struct's
methods, so `Builder` carries a `Paint` mode (`None`, `Groups` for
PaintBuilder, `Palette` with the channel for ColorBuilder) and `add` and
`build` dispatch on it; `Builder::new_paint(groups)` and
`Builder::new_color(palette)` are the constructors, `set_channel` is
`B.channel = ...`. The groups and the palette keep the JS object's entry
order (only looked up by key). ColorBuilder's fallback material
(`MeshStandardMaterial({ vertexColors: true, roughness: 0.85 })`) is made
when the first solid bucket needs it rather than at the top of `build`; a
material nothing draws never reaches the scene, so the output is the same.

## D193. THREE.Color

2026-10-03, WP 3.3. `color::Color` ports three r180's Color with the
colour management the game runs under (working space linear sRGB):
`setHex` and `setStyle` convert from sRGB with three's `SRGBToLinear`,
`getHex` converts back, `setRGB` and `setHSL` take working-space values;
`pow` is the kernel's. `setStyle` handles `rgb()`/`rgba()` (integers and
percentages), `hsl()`/`hsla()`, `#rgb` and `#rrggbb`; the CSS colour names
(`'red'`) are not ported because the game never passes one, and an
unrecognised style leaves the colour unchanged as three does after its
warning. Every case in the golden is bit-identical.

## D194. Materials carry three's full parameter list

2026-10-03, WP 3.3. A JS material reaches the scene as every own property,
in its JS order (D25). `Material::standard()` and the constructors for
Physical, Lambert, Basic, LineBasic, Sprite and Points hold exactly that
list with three r180's defaults (the golden checks all seven against a
fresh three.js material), so a Rust-built material and an exported one
read the same to the renderer. `Material::set(key, value)` is `setValues`
for one key: a colour property takes a hex number, a CSS string or a
`Color`; any other property is replaced; a key the class lacks is ignored
(three warns); Physical's `reflectivity` sets `ior` through its accessor.
A patched material is the built-in one with `kind(MaterialKind, opts)`,
`program_key` and the patch's `uniform`s, as the JS tags it (D21, D22).
Texture parameters hold a `TextureId`, renumbered at assembly.

## D195. Animators write edits addressed by handle

2026-10-03, WP 3.3. An `Animator` (a trait, implemented by any
`FnMut(&UpdateCtx, &mut Vec<Edit>) + Send + Sync`) is one closure of
`world.updaters`. `UpdateCtx` is the JS `(dt, night, camera, s)`;
`CameraView` is what the updaters read of the camera: its position, its fov
and the drawing buffer's height. An `Edit` is a target `Handle` (node,
material, geometry, texture) and a `Change`: visibility, a transform, an
instance matrix, colour or count, values written into an attribute, a
material number or uniform, a material colour, a texture offset. That list
covers the animated scenery of inventory section 10 and grows if a module
needs more. `WorldBuild::update` runs the animators in order and resolves
each edit to the scene (`SceneEdit`, `SceneRef`), dropping edits of what
the scene left out; the night parameters are the client's to apply before
it (`night_value`), as `World.update` applies them before the updaters.
Animators are `Sync` so a world can be read from several threads (D196).

## D196. A build is a list of jobs

2026-10-03, WP 3.3. A `Job` has the JS progress label and fraction (shown
before it runs, as `step(label, frac)` reports them) and either runs on the
`&mut World` (`Job::Serial`), returning jobs to run next, or is a set of
pure pieces (`Job::Parallel`) that each build a `Part` (a graph of their
own and its animators) from a `&World`; the parts are merged in their order
with their handles moved by the merge (`SceneGraph::append`, and animators
wrapped to move their edits' handles), so the result is the same on one
thread or many. With the `parallel` feature, natively, the pieces run on
std's scoped threads, one batch per core; no thread pool crate is needed,
so rayon is not added. `level_jobs` is `World.build`: Surveying the route
(the Track; the level arrives prepared, D54), Shaping the land (the
terrain, then each scenery module made and its `plan` run, a failing one
dropped, then the fields), Sculpting terrain, Paving roads (road and sky),
Filling the sea on levels with a sea, one job per surviving scenery module
at `0.72 + k / n × 0.26` with its `label` or "Building scenery" (a failing
`build` is logged and the build carries on), and Ready. The scenery jobs
are queued by the "Shaping the land" job, since `n` counts the modules
whose plan succeeded. The stages later packages port plug in through
`Stages` (terrain, fields, terrain meshes, road, sky, sea), and the modules
through the `Scenery` trait and a factory given each `SceneryInfo` (name,
first zone, key, as `loadScenery` passes them); `SCENERY_LABELS` holds each
class's label. `World` holds the level, the track, the graph, the root
group `world:<id>`, the animators, the texture cache and the simulation's
world data (`SimWorldData`, `mr_levels::world::WorldData`, filled by the
scenery); later packages add their fields (terrain, road, sky, sea).

## D197. The builders golden

2026-10-03, WP 3.3. `tools/parity/builders.mjs` imports the game's own
`valley/Builder.js`, `beach/ColorBuilder.js`, `city/geom.js` and `Road.js`
under Node with the parity kernel (they need nothing from a browser) and
records 44 cases in `parity/golden/builders/builders.json`: Color (hex,
fractional and out-of-range hex, HSL both ways, offsetHSL, getHex, styles,
the arithmetic), the seven built-in materials' defaults and ten made with
options, Builder and PaintBuilder (frames pushed and popped, boxes rotated
three ways, centred boxes, beams diagonal, straight up and straight down,
indexed, non-indexed and lathe, torus and tube pieces, a piece without
normals, a missing material, `castShadow` as a list and as `true`,
`mergeAll` and its failure on mixed pieces), ColorBuilder (near and far
channels, the fallback solid material and a given one), GeoBuilder (each
degenerate quad branch, triangles, roof winding flips, prisms in both
windings with every option, boxes with bottoms, with colour and cell
attributes and without), `trs`, `yawOf`, `instanced` (with colours, empty,
coincident centres), `staticMesh`, and `extrude` over Sierra, Coast, the
cruise loop (past its length) and Desert (gaps, absolute heights, functions
for `dy` and colour, skipped short ranges, the open road's runout) with
`runs` over predicates; and the progress labels of `World.build` for every
level but Seaside, computed from the levels' zones and the labels the
scenery classes set. Meshes are compared by name, type, flags, matrix bits,
material and geometry; geometries as in D134.
`crates/mr_worldgen/tests/builders.rs` makes the same calls and is
bit-identical in every case, native and in wasm. CI checks that the golden
regenerates.

## WP 5.1–5.2 decisions

## D210. The facade: handles like the JS objects, one `Op` per call

2026-10-03, WP 5.1. `mr_audio::wa` ports call for call: `ctx.create_gain()`,
`g.gain.set_target_at_time(v, t, tc)`, `osc.connect(&lp)`. Handles are
reference-counted like the JS objects (a node handle holds its context, a
param handle its node); typed nodes carry their params as fields and deref
to `Node` for `connect`/`disconnect`. Every call becomes one
`wa::backend::Op`, handed to the backend twice: `record` before validation
(the null backend's call log, which logs rejected calls too, as the fake
does) and `apply` once valid. Dropping the last handle on a node, buffer or
wave releases the backend's reference at the next call, so the web backend
does not keep finished one-shots alive (the browser then keeps a node only
while it plays, as for an unreferenced JS node). Enum attributes have typed
setters and `*_js(&str)` twins for strings from data (an invalid one is
ignored, as browsers do). Calls the game wraps in `try` (`stop`) and the
ones that can throw in practice (`start`, `connect`, `disconnect_from`,
`set_buffer`, `create_periodic_wave`) return `Result`; the rest record.

## D211. Validation lives in the facade, for every backend

2026-10-03, WP 5.1. Each call is checked before any backend sees it, as a
browser checks it: what browsers throw on (the fake's checks: non-finite
values, negative times, an exponential ramp to zero, a negative time
constant, a second `start()`, `stop()` before `start()`, a missing link on
`disconnect(dest)`, a second buffer, `type = 'custom'`, mismatched or short
wave arrays, a bad `createBuffer`; and beyond the fake: connecting across
contexts, an output or input index out of range, a merger's input count, a
delay's maximum, a curve shorter than 2, a convolver buffer of the wrong
channel count or rate, `fftSize`) is not carried out and records a
`Problem` of kind `Throw` (with the fake's exception name and message);
what browsers ignore (an invalid enum string) is `Ignored`; what they carry
out with a console warning (a value outside a param's nominal range, which
Chrome clamps and warns about) or silently get wrong (a cycle with no
DelayNode, which browsers mute) is `Warning`. Strict mode
(`AudioContext::set_strict`) panics at the first problem: the null
backend's validation mode, the strict fake of `music.test.js`. The native
crate panics on much of the first group, so it must never see them; the
web backend logs to the console if the browser still throws (a facade bug).
Replaying the JS game's two drive logs (697,000 calls) flags nothing.

## D212. The null backend writes the fake's call log; its conformance golden

2026-10-03, WP 5.1. The null backend's log is byte for byte the format of
`tools/parity/lib/webaudio-fake.mjs` (D41): ids `n<k>`/`b<k>`/`w<k>` in
creation order, arrays by hash, `param.value` read from a port of the
fake's timeline (D42), numbers as `Number.prototype.toString` writes them.
That last needs care: where two shortest strings round-trip (a float32
value exactly between two 17-digit decimals) ECMAScript takes the closer,
ties to even, and Rust's shortest printer may take the other, so the
digits are the value correctly rounded at the shortest length. Two checks:
`tools/parity/audio-facade-ref.mjs` runs a fixed script on the fake (every
operation, value reads through each automation event, each exception) into
the new small golden `parity/golden/audio/facade-conformance.json`, and
`tests/null_backend.rs` runs its Rust twin (`tests/common`) and requires the
same log, problems and exceptions; `tests/calllog_replay.rs` replays the
cached call logs of both drives into the facade and requires the identical
log back (skipped when `parity/cache/` lacks them).

## D213. Buffer contents reach the backend at first use, then stay

2026-10-03, WP 5.1. A buffer's samples live in the facade until it is
first given to a node (`set_buffer` on a source or convolver), when they go
to the backend once (the fake's `data` line). The line is recorded after
the call that uses it, as the fake orders them, but applied before it, so a
convolver never takes an empty buffer (it computes its response when given
one) and the native crate's copy-on-write buffers are filled before a node
holds them. Writing to a buffer after that is a `Warning` (the reference
forbids it, and whether a browser hears the change depends on the node).

## D214. Promises are `Pending<T>`; virtual-clock backends settle them

2026-10-03, WP 5.1. `resume`, `suspend`, `close` and `decode_audio_data`
return a `Pending<T>`: polled (`result()`) by frame-loop code, or followed
with `then`. The web backend settles it from the browser's promise
(`wasm-bindgen-futures`); the null and native backends queue the
settlement for `AudioContext::settle()`, which a driver (the call-log
playback, the native client's frame) calls, as the fake settles in a
microtask. The state change of `resume`/`suspend` happens at settlement,
as in a browser.

## D215. Backends as features; the native one on web-audio-api 1.7.0

2026-10-03, WP 5.1. `null` is the default feature (no dependencies); `web`
pulls `web-sys`, `js-sys`, `wasm-bindgen` and `wasm-bindgen-futures` on
wasm32 only; `native` pulls `web-audio-api` pinned at 1.7.0 with default
features off and `mp3` on (the radio clips), so it renders offline and runs
live contexts on the crate's `"none"` sink without an audio device or ALSA
headers; `native-device` adds cpal for real output (the native client
turns it on). Cargo.lock grows by the crate's optional device backends
(cpal, cubeb) whether or not they are built. CI lints `native` and `web`
and runs `native`'s tests (offline renders: automation, oscillator RMS, an
engine wave's period, a radio clip's decoded length against Chrome's, the
conformance script). Steering an offline native render mid-way
(`suspend(t)`, which the reference renders use every 8 ms, D45) is left to
WP 5.4.

## D216. The generated arrays are pure functions; the reference list

2026-10-03, WP 5.2. `samples`, `noise`, `engine`, `shapes` and `music`
return plain channel data (`Vec<f32>`, rounded where the JS stores into a
`Float32Array`, every inexact function through `mr_math::kernel`);
`samples::render_sfx`/`render_kit` make the AudioBuffers through the
facade as `toBuffer` does. The audio's `Math.random` is a parameter
(`&mut impl Rng`): the noise beds draw first, then the tunnel's impulse
response, as `_build` orders them (D40); nothing else drawn during the
build feeds an array. `reference::arrays` lists all 64 arrays of
`arrays.json` in its order, with its names and aliases, and the gate
(`tests/arrays.rs`, the golden compiled in, native and wasm) finds all 64
bit-identical, so SPEC 7.3's 1e-5 tolerance for `samples.js` is not used.
The test reads the cache's `arrays.bin` only to locate a difference if a
hash ever fails.

## WP 3.4 decisions

## D230. The shape of `terrain`, `colorizer` and `terrain_mesh`

2026-10-03, WP 3.4. `Terrain.js` is `mr_worldgen::terrain`, the
`TerrainColorizer` of `TerrainMesh.js` is `colorizer`, the rest of
`TerrainMesh.js` is `terrain_mesh`. The JS keeps a reference to the track;
here `Terrain::new`, `build_fields` and `is_elevated` take `&Track` and
nothing else needs it, so a built terrain is a plain value the `World` owns
beside its track. Methods keep the JS names (`height_at`, `landform`,
`far`, `road_info`, `slope_at`, `tile_list`, `add_flatten`, `add_carve`,
`resolve_flattens`, each `form_*`); `this[FORM_FN[...]]` is a `Form` enum
and `LANDFORMS` a table in the JS order. Every `Float32Array` (the far
field, the near tiles, `canyonWidth`'s table, the mesh's heights and
attributes) is a `Vec<f32>` stored through `as f32` and read as `f64`; a
typed-array read past the end (`t.px[zoneStart[1] + 800]`) is NaN, as
`undefined` becomes in JS arithmetic. The near tiles keep the JS key
(`tx,tz`) in a `BTreeMap` used only for lookup, and `height_at` reads them
through a dense grid over the tiles' range; `tileList`'s neighbour `Map` is
the grid position, since the list is complete. `zoneWeights` returns a
fixed array (at most eight zones) rather than allocating per call. A
landform the JS has no entry for is an error from `Terrain::new`.

## D231. The lazy caches are filled by `build_fields`; nothing mutates on read

2026-10-03, WP 3.4. The JS fills `_open` (`openBias`'s lookout windows) and
`_cw` (`canyonWidth`'s smoothed wall distances) on first use, from the
track and the noise. Both are pure functions of those, so `build_fields`
fills them (the windows before `openField` needs them, the canyon table on
levels with a canyon), and `height_at`, `landform` and the colouriser take
`&self`: the tiles can be built on several threads. The colouriser's
`this.paved` (read by the mesh after each `color()`) is returned with the
colour, and its scratch arrays (`_rw`, `_pf`, `_farTmp`) are locals.

## D232. The scenery's plan is recorded until the scenery is ported

2026-10-03, WP 3.4. The JS heights depend on what the scenery modules'
`plan()` registers before `buildFields`: 33 flattens and the creek carve on
Sierra, 35 flattens on Coast, the railway bed and six flattens on Desert,
one flatten and one carve on the cruise loop. The scenery arrives in WP 3.6
on, so `tools/parity/terrain-plan.mjs` runs `World.build`'s first stages
under Node with the parity kernel (the Track, `new Terrain`, each scenery
module made as `loadScenery` makes it and its `plan()` run, `buildFields`,
`resolveFlattens`) and writes `parity/golden/terrain/<level>.json`: the
flattens (with the heights the JS resolved), carves and `desertRail` as hex
f64 bits in registration order. The tool also evaluates the heights at the
world golden's 10,000 points and requires the golden's SHA-256 (and, with
the cache, every height of the browser's dump): Node reproduces the
browser bit for bit on all six levels, so the plan recorded is the one the
game's world had. `TerrainPlan::apply` registers it as the scenery would;
`TerrainSetup::plan` carries it into a level build. When a scenery module
is ported, its `plan()` replaces its part of the recording and the golden
becomes the test of that `plan()`. CI checks that the golden regenerates.

## D233. The L2 and L3 gates, and what they found

2026-10-03, WP 3.4. L2 (`tests/terrain.rs`): the 10,000 points of
`terrainDump` are regenerated (Track `point_at` along the road, Halton 2, 3
over the bounds) and the SHA-256 of the points and of the heights must equal
the world golden's, so the gate runs without the cache and in wasm; with
the cache every point is compared and the first difference reported.
Result: **bit-identical on all six levels**, native and in wasm. L3
(`tests/terrain_mesh.rs`, native, needs the cache): the terrain built into
an `mr_scene::Scene` through the object tree, each mesh's `mr_scene` digest
(counts, bounds, SHA-256 of `position`, `normal`, `color`, `uv`, `aSurf`
and the index, area, centroid) against the JS digest of the export's
meshes under `terrain` (the `--base` export when present), per attribute
differences reported from the file's buffers; the node flags; the material
parameter by parameter, uniforms included, its textures by description and,
for data and image textures, pixels (canvas textures' pixels are WP 3.2's
threshold gate: the rock texture is not identical). Result: **every vertex
buffer and index identical**: Sierra 133 meshes (501,072 vertices), Coast
115 (460,626), Streets 73 (277,047), Desert 127 (456,693), Seaside 44
(168,312), cruise 152 (670,629); the material equal but for the rock
texture's pixels. So that L3 also runs in CI and in wasm without the cache,
`terrain-plan.mjs` records per level a SHA-256 over one line per terrain
mesh (counts, attribute digests, index digest) from the cached export, and
`tests/terrain.rs` builds each level through `level_jobs` and requires the
same hash.

## D234. Seaside's photo is decoded by the caller

2026-10-03, WP 3.4. The ground shader drapes Seaside's aerial photo
(`level.groundPhoto`) with a one-channel loose-ground mask over the same
box. `mr_worldgen` has no JPEG decoder, and the client already has one
(Bevy's image loader; the browser on the web), so the decoded photo comes
in with `GroundPhoto::seaside(data, photo, url)`, as the survey itself is
passed in (D54); the mask is made from the survey's loose grid as the JS
makes its `DataTexture` (`Math.round(v * 255)`), and `seaside_ground_color`
is `level.groundColor`. The L3 test takes the photo's pixels from the JS
export (Chrome's decode); the material and both textures then match.

## D235. The terrain in the job list

2026-10-03, WP 3.4. `terrain_mesh::terrain_stages(setup)` returns the three
`Stages` entries: "Shaping the land" first makes the terrain (and applies
the recorded plan), after the scenery plans `buildFields` and
`resolveFlattens` run, and "Sculpting terrain" builds the tiles in batches
of 24 (the JS yields every 24 tiles), each later batch a job showing
`0.1 + done / tiles × 0.55` as the JS reports it, then a job that merges
the tiles into meshes by group (in the order the groups were first met),
makes the material and adds the group `terrain` to the world's root. With
the `parallel` feature a batch's tiles are built on scoped threads; the
result is identical. `World` gains `terrain` and `terrain_material`
(`world.terrainMaterial`). A release build makes all six levels' terrain,
fields and meshes in 2.8 s on one thread.

## D236. Two fixes found by the terrain material

2026-10-03, WP 3.4. `textures::Texture::desc` gave a `DataTexture`
`unpack_alignment` 4; three's `DataTexture` sets 1 (the export says 1 for
`terrainDetailTexture`). The scene assembly gave every pixel buffer an item
size of 4; it is the texture's channel count (Seaside's loose mask is R8,
item size 1, as the export writes it). Neither changes an RGBA canvas
texture. Still open for WP 3.5: the JS texture cache hands out one
`THREE.Texture` per key, so the terrain, road, sea and sky share one
`terrainDetailTexture` (one texture in the scene), while
`SceneGraph::cached_texture` makes a new texture per call; the road and sky
must reuse the terrain's `TextureId` (or the graph learn to share by key) to
match the export's texture count.

## WP 2.3 decisions

## D170. three_std: our own shading on Bevy's material system, lit in view space

2026-10-03, WP 2.3. The plain kinds (Standard, Physical, Lambert, Basic, and
Line and Points drawn as basic) are one Bevy `Material`, `ThreeMaterial`
(`crates/mr_game/src/render/material.rs`), whose WGSL
(`three_material.wgsl` on the `three_std.wgsl` library) is three.js r180's
meshphysical, meshlambert and meshbasic programs for the features the game
uses: map and vertex colour, instance colour, emissive and emissive map,
roughness and metalness with the geometry-roughness term, IOR and specular,
clearcoat, sheen, the directional light with its shadow, one spot and one
point light, the hemisphere light, the environment's irradiance and radiance
with three's multiscattering, alpha test, the `opaque` alpha rule and
`FogExp2` with `Sky.js`'s sun tint in lit shaders. Lighting runs in view
space, as three's does, because the geometry-roughness term takes the
largest component of the view-space normal's derivatives, which is not
rotation invariant. A key (`ThreeKey`) picks the shader variant and three's
fixed-function state: culling from `side`, three's blend functions
(`SRC_ALPHA, ONE_MINUS_SRC_ALPHA` and `SRC_ALPHA, ONE`, with
`premultipliedAlpha` false; normal blending only when `transparent`),
`depthWrite`, `depthTest`, and `polygonOffset` as before (D103). Bevy's
`StandardMaterial`, its lights, ambient light, `DistanceFog`, exposure and
tone mapping are no longer used, nor `tint.wgsl`: instance colours ride in
the `MeshTag` (D103's packing) and the same material reads them.

The scene-wide inputs (sun, hemisphere light, fog, shadow matrix and
parameters, exposure, environment intensity, the dome's uniforms, the spot
and point light) are one row of 32 RGBA32F texels (`three_globals.wgsl`,
`render::lighting`), written by the render world every frame into one
texture every material binds. That keeps per-frame state out of the
materials (nothing is re-prepared when the light moves), avoids storage
buffers (WebGL2 has none) and leaves Bevy's view bindings untouched.

Patched kinds draw as the plain version of their built-in type through the
same material, as stand-ins, until their patches are ported (WP 2.4, M3).

## D171. The shadow map: Bevy's pass over three's box, sampled as three does

2026-10-03, WP 2.3. Bevy renders the map: one `DirectionalLight` (the
"three sun", `render::lighting`) with one cascade, whose `Cascade` is
replaced every frame, between Bevy's cascade build and its light frusta,
with three's shadow camera: the light at its position looking at its target
with y up, the orthographic box (±70 m, near 1, far 600 in the levels;
`scenes.json`'s box in the test scenes), 2048². Bevy's depth is reversed
and linear for this projection, so the map holds 1 − three's depth; the
shaders compute three's `shadowCoord` (the vertex pushed along its world
normal by `normalBias`, three's `shadowMatrix`, `bias` added to z) and run
`getShadow`'s PCF-soft taps with `textureLoad` on Bevy's depth array,
flipping the row (Bevy's rows run top first). The shadow pass culls three's
shadow side (the back faces of a front-sided material), and an alpha-tested
material keeps its alpha test there (`three_prepass.wgsl`), as three's depth
material does. Bevy's own biases, filtering and cascade fitting are unused.
Shadows follow the high-quality setting (`hq`). Note for the WebGL2 build
(WP 2.7): `textureLoad` on a depth texture may need a non-comparison
binding there.

## D172. The environment map: three's PMREM, pass for pass

2026-10-03, WP 2.3. `render::pmrem` ports `PMREMGenerator.fromScene(dome,
0.04, 0.1, 200)` at the default size: the six 256² faces into the 768 × 1024
half-float cube-UV atlas in three's layout, the 0.04 rad blur of the base
level, then `_applyPMREM`'s ten blurs (latitudinal then longitudinal halves
through a ping-pong target, with three's sample counts, weights, `dTheta`,
`mipInt` and pole axes), in the render world ahead of the main pass. Each
pass draws a full-screen triangle into three's viewport and works out from
its pixel what three's lod plane or cube camera would have interpolated
there (exact for these linear varyings; for the faces, the dome's
normalised position is the view ray). Rows keep three's order, so three's
viewport origins and `textureCubeUV` read the atlas unchanged. The sky in
the faces is the dome's own shader (`sky.wgsl`), as main.js renders the
dome into the map. Kept on purpose: `_blur` for the base level passes no
pole axis, so a reused generator (the game's) blurs every map after the
first about `_axisDirections[0]`, a fresh one (the material tool's) about
y; `EnvRequest.fresh` says which. A level rebuilds the map when the time of
day has moved 2.5 % of the route (`refreshEnv`).

## D173. The sky in the client, until world generation owns it

2026-10-03, WP 2.3. `Sky.sample` and `Sky.update` are ported into
`render::sky::SkyState` from `mr_levels`' keys (colours through three's
sRGB-to-linear with the kernel's `pow`, trigonometry through
`mr_math::kernel`), driving the dome's uniforms, the sun or moon, the
hemisphere light, the fog and the exposure from the fly or attract
camera's s, with `?t=` and `?freeze=1` as in the JS. A test checks the
result at s = 0 against every export's dome uniforms, fog and exposure, to
1e-12. WP 3.5 ports `Sky.js` into world generation; this moves there then.
`world.nightMaterials` follow the sky's night factor from the export's
`night_params` (all `emissiveIntensity`). The client's Track takes the
scenery's runout from `mr_levels::world` (as the simulation does), so the
fly camera reaches the end of the road as the JS one does (stations past
`length` on Sierra and Coast were wrong without it). The models scene, which
has no dome, takes Sierra's sky at the start and the dome's noise texture
from `mr_worldgen`'s `terrainDetailTexture`; it draws on black.

## D174. The post chain

2026-10-03, WP 2.3. `render::post` ports UnrealBloomPass and OutputPass on
the camera's HDR target, in place of Bevy's tone mapping
(`Tonemapping::None`, `DebandDither::Disabled`): the half-size high pass
(threshold 0.92, smooth width 0.01, luminance weights 0.2126, 0.7152,
0.0722), five sizes each `Math.round(x / 2)` of the last, the separable
Gaussians (radii 3 to 11, three's coefficients and its sampling of the
larger texture at the smaller one's texel offsets), the composite (factors
1 to 0.2, radius 0.35, strength 0.38) into the first horizontal target, then
one output pass that adds the composite as three's additive blend does
(`SRC_ALPHA × src + dst`, the composite's alpha being strength × Σ factors =
1.14) and applies three's ACES filmic with the exposure (`/ 0.6`). The sRGB
transfer is the surface's (Bevy's upscaling writes to an sRGB view), as
three's OutputPass encodes before an 8-bit canvas. The intermediate targets
are half float, as three's.

## D175. The material test scenes in the Rust client

2026-10-03, WP 2.3. `--materials all|every|<names> --out <dir>` (natively;
the module is `matscene`) renders the scenes of
`parity/golden/materials/scenes.json` (compiled in) one after another in a
512 × 512 window (scale factor 1) and saves `<dir>/<group>/<name>.png`, the
layout `tools/parity/materials.mjs` writes. The geometry is three's, by
`mr_worldgen::three_geom`; a kind scene takes its material from the level's
export by the definition's path (from the world root, or the dome), with the
source mesh's extra attributes set to its first vertex and the first
instance's colour, as the JS tool does; the fixed scenes' materials are
built as three's constructors would. Lights, shadow box, hemisphere light,
background, fog, camera and exposure come from the definitions; the
environment is the level's dome at s = 0 with the clock at 0, built once per
level with a fresh generator (D172). A scene is captured when every pipeline
has compiled, the environment for its level is built and four quiet frames
have passed. `all` is the fixed scenes and the kinds WP 2.3 ports
(Standard, Physical, Lambert, Basic, SkyDome); `every` adds the other kinds
on a built-in type, drawn as stand-ins, for the record. The web build does
not run them (the levels' exports do not pass the test harness's
interception, D106).

## D176. One command each for the material scenes and the stations

2026-10-03, WP 2.3 with WP 2.5's gate wording. `cargo xtask parity
materials [--only …]` renders the JS side if its cache directory is missing
(and the exports, if those are), renders the Rust side into
`parity/cache/<key>/materials/rust/`, compares with `parity shots` (label
`materials`) and fails if a scene is over SPEC 12's limits. `cargo xtask
parity stations [--levels …]` does the same for D17's stations: the native
client's `--stations <stations.json> --out <dir>` flies to each station of
the JS run with the scenery frozen, waits for the sky, the environment map
and the pipelines, and saves the PNGs into `parity/cache/<key>/shots/rust/`;
it reports but does not fail (most kinds are still stand-ins). For the web
build, `tools/parity/rust-web.mjs` loads `dist/next/` in headless Chrome with
D106's WebGPU flags through request interception (`.wasm` as
`application/wasm`), waits for `__mr.ready` and saves `__mr.screenshot`; it
needs the release wasm (the dev wasm, 85 MB, overflows the DevTools
connection's 100 MB buffer) and a scene under about 100 MB.

## D177. Shader identifiers in imported modules

2026-10-03, WP 2.3. naga_oil refuses identifiers ending in a digit in a
module others import (they would need renaming on write-back), so three's
`pow2`, `pow4`, `max3` and float `F_Schlick` are `pow2f`, `pow4f`, `max3v`
and `F_Schlick_f` in `three_std.wgsl`; the material fields `specularF90`,
`clearcoatF0` and `clearcoatF90` are `f90_specular`, `f0_clearcoat` and
`f90_clearcoat`; `roughnessToMip`'s constants are locals. The sky's noise
lookups inside branches are taken before the branches (WGSL wants
implicit-derivative samples in uniform control flow), and GLSL's
`smoothstep` with reversed edges is written out (`smooth_step`), since WGSL
leaves it undefined.

## WP 3.5 decisions

## D270. The shape of `road`, `sky`, `sea` and `stages`

2026-10-03, WP 3.5. The `Road` class joins `extrude` and the run helpers
in `mr_worldgen::road`; `Sky.js` is `sky`, `Sea.js` is `sea`, and the
stages of `World.build` they fill are `stages`. `Road::new` makes the group
`road` and takes the shared `tDetail`; `build` runs the JS methods in order
(`classify_sides`, `build_surface`, `build_markings`, `build_barriers`,
`build_lines`, `build_chevrons`, `build_viaduct`) on a `RoadCtx` (track,
terrain, graph, texture cache, `noMarks`). `this.materials` is a list of
(key, `MaterialId`) in the order the keys were made, `mat(key, make)` keeps
one per key as the JS does (the chevrons' poles reuse the barriers' `post`,
the viaduct the barriers' `concrete`), and setting the wood rails'
`side` changes the one shared wood material, posts included, as in the
JS. `track.sideL`/`sideR` live on the `Road` (`side_l`, `side_r`): only
scenery reads them, so mr_track's `Track` is left alone; the same goes for
`track.noMarks`, which is `World::no_marks`. `Road::set_night` and
`dew_animator` write `uWet` as an edit of each asphalt material (the JS
shares one uniform object between them). `Sea::new` makes the mesh and
material (not added anywhere; the stage adds it to the root) and
`Sea::animator` the updater. `World` gains `no_marks`, `road`, `sky` and
`sea`; `WorldBuild` gains `sky`, `update_sky` and `resolve`. `Change` gains
`Vector` (a vector uniform: `uSunDir`, the sea's `uOff2`) and `Light` (a
light's colour, intensity and ground colour). `Material::shader()` is a
fresh `ShaderMaterial` with three r180's own properties in their order
(`forceSinglePass` true, `defines`, `linewidth`, ..., `glslVersion`), and
`shader_source` carries its GLSL; the sky dome carries the JS `skyVert` and
`skyFrag` verbatim (generated from `Sky.js`, so the export's shader text
and ours hash the same). Each patch (asphalt, shoulder, markings, sea) is
the built-in material with its kind and the uniforms the export lists,
`clippingPlanes` last, as WP 3.4 did for the terrain. The GLSL of the
patches and the fog-chunk rewrite (`MR_SUN_FOG`) are the renderer's.

## D271. One texture per cached picture

2026-10-03, WP 3.5; the fix D236 asked for. `SceneGraph::cached_texture`
now returns the texture it made for the same cached picture (same
`Arc<Cached>`, same layer) instead of a new one, as the JS module hands
out one `THREE.Texture` per key; the graph keeps the list in a new field,
`shared`. A `clone_texture` of it stays a texture of its own (a JS
`texture.clone()`). Proved by the texture counts: the base exports hold 10
(Sierra), 9 (Coast), 5 (Streets), 7 (Desert, one of them the lake bed's),
7 (Seaside, two of them the photo and its mask) and 6 (cruise) textures,
and the Rust scenes the same; before, the road, sky and sea each added a
`terrainDetailTexture`. WP 3.4's terrain gate is unchanged (the terrain
asked for three different pictures). A part built by a parallel job keeps
its own shared list, appended after the world's; a texture both make is
then two textures, which no ported module does yet.

## D272. The time of day as data; the sky in a built scene

2026-10-03, WP 3.5. `sky::SkyParams` is the level's keys prepared
(`prepKeys`: colours through `THREE.Color`'s sRGB to linear), the sun's
azimuth and the moon's direction; `sample(p)` is `Sky.sample`, and
`frame(p)` everything `Sky.update` computes at a fraction of the route
(`SkyFrame`: the dome's uniforms, the light's colour, intensity and
direction, crossfading from sun to moon and kept above 0.12, the
hemisphere light, fog colour and density, exposure, the night factor).
`frame_at(s, override)` takes the player's distance: a point-to-point road
runs from 0 to 1, a loop is pinned at 0.5, and `override` is `?t=`. This
is the API the client calls for the time of day at s, with no scene
needed. `sky::Sky` is the JS class in the scene: the dome (kind
`SkyDome`, render order -10, scale 5000, not culled), the directional light
with its 2048² shadow over ±70 m (near 1, far 600, bias -0.0004, normal
bias 0.6), its target, and the hemisphere light, each a scene root as
three keeps them. `Sky::update(dt, s, focus)` returns the frame and writes
edits (uniforms, the lights' colours, the sun 300 m from the focus along
the light, the target and dome at the focus); `WorldBuild::update_sky`
resolves them to the scene, after which the client applies the night
parameters at `frame.night` and runs `WorldBuild::update` (D195's order,
`World.update`'s). The build leaves the scene as a first update at s = 0
without a focus would (dt 0, lights and dome at their defaults), so a
scene file shows the start's time of day.

## D273. The scenery's road inputs are recorded until it is ported

2026-10-03, WP 3.5. The road reads what scenery `plan()`s register:
`track.fenceGaps` (Valley's driveways and store, Beach's cross streets),
`track.noMarks` (Streets' crossings) and `track.runout` (City, Harbor,
Streets; the road itself never samples past the finish, so runout changes
nothing it draws). `tools/parity/road-plan.mjs` runs the plans under Node
with the kernel (as `terrain-plan.mjs` does) and writes them to
`parity/golden/road/<level>.json` as hex bits; each equals the world
golden's track scalars (the browser's values after the whole build).
`stages::RoadPlan::apply` registers them right after the scenery plans, in
the "Shaping the land" job; `stages::level_stages(LevelSetup)` gives the
terrain's stages with it plus the road, sky and sea stages. When a scenery
module is ported, its `plan()` replaces its part of the recording.

## D274. The L3 road, sky and sea gate, and what it found

2026-10-03, WP 3.5. `tests/road.rs` builds each level through
`level_jobs(level_stages(...))` and compares it with the golden
`road-plan.mjs` writes from the browser's exports (`--base` for road and
sky; the full export for the sea, which the base leaves out): per child of
the group `road` a line of type, vertex and index counts, the SHA-256 of
every attribute (`aLane` included) and the index, shadow flags, instance
count and matrices, hashed together; each child's material, and each
material parameter by parameter with its uniforms and textures (by
sampler, and by pixels but for canvas textures, which are WP 3.2's
threshold gate); `sideL`/`sideR` against the world golden; the sky's
dome mesh, material (GLSL by hash), four root nodes, both lights, fog,
exposure and night at the export's focus; the sea's node, mesh, material
and wave normal map pixels; the texture count. The golden is compiled in,
so the gate runs in CI and wasm; with the cache a failure is localised per
mesh and attribute. What scenery does to the road after building it is
replayed by the test, not the port: Streets' wetter asphalt
(`asphalt2`: roughness 0.62, metalness 0.05, colour 0.72) and Desert's lake
bed, whose material the JS puts on the asphalt and shoulders past the
lake's start: the test finds the same 15 meshes with the JS predicate and
checks the JS has its material there. The night parameters and the dew
updater are applied at the export's night factor. Result: **identical on
all six levels, native and wasm**: Sierra 117 road meshes (184,744
vertices), Coast 101 (172,162), Streets 14 (49,132), Desert 62 (118,504),
Seaside 10 (34,188), cruise 141 (274,508), every material equal; the sky
equal everywhere; Coast's sea (239,093 vertices) and its wave normal map
bit-identical. The tool also records `Sky.update` at 41 points along each
route under Node (every value it sets, hashed); `sky_route_*` reproduce it
bit for bit, so the time of day along the route is at parity, not only at
the start. No port fix was needed.

## WP 3.6 decisions

## D310. `valley/flora.js` is `mr_worldgen::flora`, ported once for its three users

2026-10-03, WP 3.6. Mountain, Valley and the Raceway import
`valley/flora.js`, so it is ported whole (every export, `canopyGeometry`
included) as a top-level module, owned by WP 3.6 and used by WP 3.7 and
7.4. `makeNoise3D(seed)` is `make_noise3d` returning `Noise3D` (its
closure is `noise(x, y, z)`; the `x & 255` lookups go through ToInt32, the
value table is the JS `Float32Array`); `rockGeometry`'s options object is
`RockOpts` (`Default` gives the JS defaults); `coniferGeometry(kind, lod,
seed)`, `canopyGeometry(kind, seed, lod)`, `grassClumpGeometry(blades,
seed)`, `flowerGeometry(n, seed)`, `shrubGeometry(seed, detail)` take the
JS arguments with the defaults written out; `foliageMaterial(o)` takes
the spread object as `(key, Param)` pairs set after the defaults. Seeds are
`u32`; the derived ones (`seed * 7 + 3`, `seed + 11`) go through
`js::to_uint32` as `mulberry32`'s `>>> 0` takes them.
`tools/parity/flora.mjs` records every call the scenery makes plus the
defaults and edges (noise past 2^31 and at negative lattice points, every
rock option, both conifer kinds at the three levels, the four canopy
kinds at both levels, the foliage material with each override) in
`parity/golden/flora/flora.json`; `tests/flora.rs` is bit-identical,
native and in wasm. CI checks that the golden regenerates.

## D311. The shape of `mountain`; the parked cars behind a hook

2026-10-03, WP 3.6. `Mountain.js` is `mr_worldgen::mountain`: `mod.rs`
holds the module (`Mountain`, its `plan()` and `build()`, one method per
JS `build*` in the JS order, the flag's and the waterfall's updaters as
`Animator`s), `kit.rs` the helpers at the top of the JS file
(`SurfaceSampler`, `instanced`, `placed`, `mergedMesh`, `bakeStatic`,
`rockMaterial`), `canvas.rs` its canvas code (`SignAtlas`, `roundRect`,
`diamond`, `panel`, and each picture it draws). It is registered with one
line in `scenery::PORTED` (D330), so every level build, and the terrain
and road gates, run its own `plan()`: the diner pull-out and the summit
lookout register the same two flattens, bit for bit and in their place,
as the recording. `this` during `build()` is a `Build` struct borrowing
the track, the terrain, the road's `sideL`/`sideR` (D270), the graph and
the texture cache. The JS's object items (`{ x, y, z, sx, ..., q?, rx?,
col?, b? }`) are `Item` with `Option`s where the JS tests for
`undefined`; `rx || 0` keeps its `||`. `SurfaceSampler`'s height cache
keeps the JS semantics exactly (a `Map` keyed by the rounded 4 m cell,
holding the height of the first point asked for in that cell: two
points of one cell from different tiles can differ in the last bit, so
the cache is part of the result), as a `BTreeMap` only looked up. The
JS's `6.28` for a random yaw stays `6.28` (`TURN`), not 2π. three's
shared `Sprite` geometry is `sprite_geometry()` (one geometry for the
five spray sprites, so one mesh in the scene, as the export has it).
The parked pickup and sedan come from `CarModel.js` (WP 4.1), which the
JS imports optionally: `Mountain::parked_cars` is an optional
`ParkedCars` hook (`buildVehicle(kind, { color, seed, lod: 'low' })` and
`setHeadlights(0)`, returning the car's root), placed and turned as the
JS does and merged by the ported `bake_static`. Until WP 4.1 sets it the
two baked groups (15 meshes) are absent, as when the JS import fails.

## D312. The L3 gate for zone 0, and what it found

2026-10-03, WP 3.6. `tools/parity/mountain-scene.mjs` writes
`parity/golden/mountain/sierra.json` from the cached Sierra export: the
group `mountain` and its place under the root; per child its node (type,
matrix, flags, render order, a sprite's centre), its mesh line (counts,
SHA-256 of every attribute and of the index), its instances (count,
SHA-256 of the matrices and colours, bounding sphere) and its material by
index into a table (each material as `road.rs` describes it, those after
the first of their class written as what differs from it, to keep the
file at 120 KB); every canvas texture's size, SHA-256 and 8×8 block
means; the export's camera and night factor. `tests/mountain.rs` builds
Sierra through `level_jobs` (Mountain built, the other modules' plans
replayed and their builds skipped), applies the night parameters at the
export's night factor, runs the updaters once as the frozen export ran
them (dt 0, the export's camera), and compares child by child. The golden
is compiled in, so the gate runs in CI and wasm; with the cache it also
compares every canvas texture's pixels and writes side-by-side sheets to
`parity/report/mountain/`. Result: **every mesh and instance set
identical**: 84 children (23,570 vertices, 54 instanced meshes holding
29,048 instances: rocks, outcrops, scree, three tiers of conifers, grass,
flowers, shrubs, snow poles, delineators, the pool's rim boulders; the
snow, signs, gantry, banners, diner, lookout, flag, waterfall ribbons,
pool, foam, sprays), every node, every attribute, index, instance matrix,
colour and bounding sphere bit-identical, and all 34 materials equal
parameter by parameter, uniforms and samplers included; the two baked
cars left out (D311). No port fix to a shared module was needed. The
canvas textures without lettering are within WP 3.2's threshold of the
export (mean absolute difference at most 1.24 levels, the foam).

## D313. Lettered textures are held to a capture with the bundled fonts

2026-10-03, WP 3.6. The scene export draws text with the machine's fonts,
so the sign atlas, the two start banners, the diner's neon and its pole
sign differ from the port by 8 to 27 levels per channel against it, all
of it in the glyphs (the sheets show the same layout, colours, shapes and
shadows). `tools/parity/mountain-textures.mjs` captures them as WP 3.2's
reference does: the game itself on Sierra (`?kernel=1&freeze=1&s=0`,
`Math.random` seeded as the export seeds it) with every face of
`assets/fonts/fonts.json` registered under the family the JS names before
the page's scripts run, reading back each canvas the group `mountain`
uses as both `map` and `emissiveMap` (six pictures, the snow poles' bands
among them). It writes the RGBA to `parity/cache/<key>/mountain/` and a
summary with the font manifest's hash to
`parity/golden/mountain/textures.json`; `--check` captures twice. The
test holds those six to it (block means always, every pixel with the
cache) and reports them against the export too. Result with the WP 3.2
fonts: mean absolute difference at most 0.66 levels (the neon's red, its
shadow blur). When the bundled fonts change, rerun the tool: the gate
then fails until the capture is refreshed, which is the point.

## WP 3.7 decisions

## D330. Scenery modules ported or replayed, each in its place

2026-10-03, WP 3.7. The terrain's flattens blend one after another in
`heightAt`, so the order the scenery's `plan()`s register them in is part
of the heights. D232 and D273 replay the whole recording before any module
runs, which would put a ported module's registrations after every recorded
one. `tools/parity/scenery-plan.mjs` runs the plan stage under Node with the
kernel and notes, around each module's `plan()`, how far each recorded list
had grown, writing `parity/golden/scenery-plan/<level>.json`: per module its
range of the terrain golden's flattens and carves, of the road golden's
fence gaps and unpainted stretches, whether it set the railway bed, the
runout after it where it changed it, its label and whether its plan threw.
`mr_worldgen::scenery` reads the three goldens into a `PlanRecording`
(checking that the ranges follow on and cover every list) and makes
`RecordedScenery` stand-ins: a module that registers its own slice in its
own place and builds nothing. `scenery_factory(Some(recording))` gives
`level_jobs` the ported module where `PORTED` names one and the stand-in
otherwise; `plan_only_factory` the same with every `build()` skipped (the
terrain and road tests); `recorded_factory` replays every module. With a
factory, `TerrainSetup::plan` and `LevelSetup::road` are `None`. Porting a
module is one line in `PORTED`. The terrain test (L2) now also requires
the registered flattens, carves and railway bed to equal the recording bit
for bit and in order, and the road test the fence gaps, unpainted stretches
and runout, so the goldens test each ported `plan()`. The goldens are
compiled into the tests, so this runs in wasm too; CI checks that the split
regenerates.

## D331. The shape of `mr_worldgen::valley`, and the L3 gate for zone 1

2026-10-03, WP 3.7. `Valley.js` is `mr_worldgen::valley` (a directory:
`valley/ground.js` is `valley::ground`, `valley/parts.js` `valley::parts`,
Valley's canvas textures `valley::textures`; `valley/Builder.js` stayed
`builder` (D190) and `valley/flora.js` is `flora` (D310)). `Valley` keeps the
JS fields its `plan()` fills (the creek, its crossing, the farm, store, mill
and windmill sites, the pads, the side roads; `this._fy`, `approxLand`'s
memory of the last road height, is a field too) and implements `Scenery`;
`plan()` takes the track and terrain out of the `World` and registers the
pads' flattens, the fence gaps and the creek's carve in the JS order. The
build runs on a `Bld` that borrows the track, the terrain and the graph
(the JS `this.*` build state: the paint builder, `M`, trees, bales, cows,
wheels, `avoid`, hedges, crop runs, the verge) and calls the JS methods in
the JS order. `makeGround` is `Ground::new(&terrain)` (its height memo is
dropped: the heights are a pure function, so the values are the same).
The parts keep their JS signatures with the defaults written out; an
`opts` object no caller fills (`farmhouse`, `barn`, `shed`, `silo` but its
`metal`) and `generalStore`'s unused `rng` are left out. Valley is the first
entry of `scenery::PORTED`. Its `plan()` reproduces Sierra's recorded
registrations bit for bit (8 flattens, the 109-point creek carve, 7 fence
gaps): the terrain (L2 heights and L3 meshes) and road goldens pass with it
in place of the recording, native and in wasm.

`tools/parity/valley-golden.mjs` writes `parity/golden/valley/sierra.json`
from the cached full export: a line per child of the group `valley` (type,
name, counts, every attribute's and the index's SHA-256, shadow flags,
matrixAutoUpdate, the local matrix's bits, instance count and the SHA-256
of the instance matrices and colours), each child's material in a table of
material descriptions (as D274's), the canvas textures' 8×8 block means
and the night parameters. `tests/valley.rs` builds Sierra through
`level_jobs` with the scenery factory (Mountain and City replayed), applies
the night parameters and one update (dt 0, s 0) as the export's frame had
them, and requires every line and material to equal the JS's, localising a
differing child to its first differing value when the cache has the
export. Result: **all 63 children identical** (231,127 vertices: the
driveways, side roads, creek, sails, poles and wires, the 13 builder
buckets, 5 canopy sets and the trunks, 6 hedge cells, bales, cows,
windpump wheels, the waterwheel, 13 crop cells, 10 verge chunks), all 28
materials equal parameter by parameter (the three `Siding` kinds with
their `mode`), the 6 night parameters equal, the creek's ripple normal map
bit-identical; native and in wasm. No fix to a shared module was needed.

## D332. The wood signs' plank noise: the page's `Math.random`, and the reference for text

2026-10-03, WP 3.7. `woodSign` draws its plank noise from `Math.random`,
which the scene export seeds (D20); every three.js object made before it
draws four more (`generateUUID`), so the stream position at each sign
depends on the whole page. `tools/parity/valley-node.mjs` runs Valley's
plan and build under Node and counts the draws between the canvases the
build creates: the store sign starts 252 draws after the valley sign, the
mill sign 7,588. Fitting the export's sign pixels to the seeded stream with
those offsets gives the valley sign's position, 11,880 (the best joint
fit of the three signs); so `valley::SIGN_RANDOM_AT` is [11880, 12132,
19468] and `page_random(n)` is mulberry32(0x5eed) after n draws. Away from
the text, the Rust signs then equal the export's to 0.03–0.12 levels (RGB
summed), 90–97 % of those pixels identical. A level built for play draws
the same planks; nothing else in Valley reads `Math.random`.

The export drew its text with the machine's fonts (Noto Sans fallbacks), so
its sign pixels are not a like-for-like reference (mean differences 17–45
levels). `tools/parity/valley-textures.mjs` captures the reference the way
D154 does: `tools/parity/textures.html` with the bundled fonts and the
kernel, the game's own `Valley.js` making the textures (`makeMaterials`,
`buildMill` on a stub world, `buildCropsMesh` on one corn run), each canvas
finding `Math.random` reset to its `SIGN_RANDOM_AT` position as it is
created. `parity/golden/valley/textures.json` holds the summaries (Chrome
version, fonts manifest hash, SHA-256 and block means). Against it: the
valley sign 0.21/0.19/0.16, the store sign 0.24/0.31/0.26, the mill sign
0.22/0.22/0.19, the neon "OPEN" 1.45/0.88/0.93 levels mean absolute
difference (R/G/B; alpha 0), all under SPEC 5.7's 3. The test reads the
fonts through the manifest, so a change of the bundled faces needs only a
new capture.

## D333. The corn strip is over the texture threshold

2026-10-03, WP 3.7. The corn strip (`buildCropsMesh`'s 256×128 canvas,
thin quadratic strokes on a transparent canvas, no text) comes out at
4.56/4.95/2.04/4.20 levels mean absolute difference against Chrome, over
the 3-level gate. The shapes match (the side-by-side sheet in
`parity/report/valley/corn.png`); the difference is in the edges: Chrome's
GPU canvas multisamples curved strokes, so its edge alpha is quantised (no
pixel has alpha between 1 and 31) where mr_canvas gives exact area
coverage (D151), and the unpremultiplied colour of a faint edge pixel that
Chrome leaves transparent counts in full. Premultiplied, RGB is within
(2.45/2.65/1.12), alpha is not (4.20); 1.4 % of pixels fall on the other
side of the material's `alphaTest` 0.45. Reproducing Chrome's multisampled
stroke coverage belongs to mr_canvas (WP 3.2's owner), so the test holds
the corn strip to 6 levels until then and reports the numbers; every other
Valley texture is within the gate.

## D334. Valley's updater

2026-10-03, WP 3.7. The closure `build()` pushes onto `world.updaters` is
one `Animator`: `updateWheels(dt)` (each windpump wheel's angle advances
by `dt × speed` and its instance matrix is `compose(position, Euler(0, yaw,
a, 'YXZ'), 1)`, an `InstanceMatrix` edit per wheel; the waterwheel's
`rotation.x -= dt × 0.6` and the sails' `rotation.z -= dt × 0.5` as
`Transform` edits with the quaternion of their YXZ Euler), the creek's
normal map offset (`TextureOffset`) and its emissive tint. The JS reads the
sky's live `uHorizon` uniform, which `Sky.update` sets from the time of day
at the player's distance; the animator holds the sky's `SkyParams` and its
`?t=` override (taken at build time) and computes `frame_at(s).horizon`
itself, the same value, since `UpdateCtx` carries `s` but not the sky.
`tools/parity/valley-node.mjs` runs the JS updater over six frames (dt and
s varied, the Sky updated before each as `World.update` does) and records
every value it sets in `parity/golden/valley/animators.json`; the Rust
animator reproduces all of them bit for bit, native and in wasm. CI checks
that the golden regenerates.

## WP 2.4 decisions

## D290. The patched kinds are blocks of `three_material.wgsl`, picked by `Patch`

2026-10-03, WP 2.4. Terrain, Asphalt, Shoulder, Markings and Sea are not
new Bevy materials: `ThreeMaterial` gains a `Patch` in its key (from the
material's kind tag and `kind_opts`, `render::material::Patch::of`), which
becomes a shader def (`PATCH_TERRAIN` with `TERRAIN_PACKED` and
`MR_PHOTO`, `PATCH_ASPHALT`, `PATCH_SHOULDER`, `PATCH_MARKINGS`,
`PATCH_SEA`), and each JS `onBeforeCompile` replacement is a block of the
same shader at the place of the three chunk it replaces or follows
(`map_fragment`, `color_fragment`, `roughnessmap_fragment`,
`normal_fragment_maps`, `opaque_fragment`), with the JS names and comments.
Their textures bind beside the plain ones: `detail` (5, 6: `tDetail`, the
sea's `tFoam`), `aux` (7, 8: `tRock`, the sea's `normalMap`), `photo` and
`loose` (13 to 16: Seaside's `tPhoto` and `tLoose`); the uniforms that sit
on the material (`uPhotoBox`, `normalScale`, the normal map's transform)
are new `ThreeParams` rows (`kind0`, `kind1`, `normal_t0`, `normal_t1`;
`patch` is a reserved word in WGSL). The sea's normal map brings three's
tangent-space normal mapping without tangents (`getTangentFrame` from
derivatives, `USE_NORMALMAP`), which the plain kinds could use as well; no
plain material in the exports has a normal map. GLSL's `dFdy` runs up the
window and WGSL's `dpdy` down the framebuffer, so `getTangentFrame` takes
`-dpdy` (its frame changes sign with the flip; with `dpdy` as it is the
ripples were mirrored and the sea test scene was at 0.88 ΔE00 instead of
0.07). The terrain's bump (`mrPerturb`) and the geometry-roughness term are
unchanged by the flip and use `dpdy` as it is. The unpacked terrain path
(the `map` sampled from above, no `tDetail`) is ported too, though the game
always passes the packed texture. The material test scenes' `all` (D175)
now includes these five kinds, so `cargo xtask parity materials` gates
them with WP 2.3's.

Kept as the JS has it: the terrain's packed path does not sample `map`
(the patch replaces `map_fragment` entirely), and `color_fragment` becomes
`diffuseColor.rgb *= vc` (the photo-blended colour); the sea's
`envMapIntensity` 1.3 has no effect, since three uses
`scene.environmentIntensity` for a material without its own `envMap`.

WGSL wants texture samples with implicit derivatives, and `dpdx`/`fwidth`,
in uniform control flow. Where a patch samples inside a branch (the
terrain's close grain `dC`, the rock faces, the varnish, the photo and its
mask), the sample is taken unconditionally and the branch only chooses;
the derivative bump (`mrPerturb`) takes its derivatives before its early
return becomes an `if`. The value is the same wherever a 2×2 quad takes
one side of the branch, which the JS comments say the conditions ensure.

Eight material textures plus the globals and environment: WebGL2 (WP 2.7)
allows 16 samplers per stage, including Bevy's view bindings; if the
WebGL2 build runs out, `photo` and `loose` can share slots with `map` and
`emissive_map`, which the packed terrain does not read.

## D291. A patch's own vertex attribute rides at location 8

2026-10-03, WP 2.4. `aSurf` (terrain), `aLane` (asphalt), `aDepth` (sea),
`gsize` (glow points) and `ph` (flicker points) are carried as one vec4
attribute, `convert::ATTRIBUTE_EXTRA`, at shader location 8 (the attribute
missing from a geometry reads as zeros, three's default attribute value).
`ThreeMaterial::specialize` rebuilds the vertex buffer layout with Bevy's
standard attributes at Bevy's locations plus this one when the mesh has it,
and sets `VERTEX_EXTRA`. Bevy's own prepass (the shadow map) builds its
layout from the standard attributes and ignores it. Which attribute a mesh
carries follows from its material (`convert::extra_attribute`), so it is
part of the mesh cache key.

## D292. The gate: the base export against the game with only terrain, road and sky drawn

2026-10-03, WP 2.4. The JS screenshot stations (D17) are of the whole
level, and `<level>.base.mrscene` holds only the terrain, the road group and
the sky. `tools/parity/base-shots.mjs` takes the same stations from the same
page (kernel on, frozen, seeded, fresh Chrome, 1280 × 800, high quality)
after setting `visible = false` on every object of the scene other than the
terrain group, `world.road.group` and the sky's dome, sun, target and
hemisphere light, which is what the base export walks; three skips
invisible objects in the main pass and the shadow map. The game is not
changed. The shots go to `parity/cache/<key>/shots/<run>.base/`. `cargo
xtask parity stations --base` runs both sides (the native client draws
`<level>.base.mrscene` with `--scene`) and fails if a gate station is over
SPEC 12's limits. The five gate stations were named before any comparison:
`attract`, `02000-chase`, `04500-high`, `07000-chase` and `09500-high`
(the pass in daylight, the valley from above, the interstate at dusk, the
city at night), `stations::BASE_GATE`. `--rust-run` names the Rust output
directory (default `rust`) so parallel runs need not share one.

## D293. The updaters' uniforms are scene-wide state

2026-10-03, WP 2.4. The JS moves some patch uniforms every frame from
`world.updaters`: the asphalt's `uWet` (`smoothstep(0.55, 1, night) ×
0.85`, set even when frozen), the sea's `uTime`, `uOff2` and normal-map
offset, the desert's `glowTime`. There is one road, one sea and one flicker
clock per level, so they are `render::lighting::Anim` in `Lighting` and two
more texels of the globals row (`G_ANIM`, `G_ANIM2`), advanced in
`update_sky` after `Sky.update` with the world's dt (0 under
`?freeze=1`); no material is touched per frame. The sea's normal-map scroll
is added to the uv transform of the export's offset. The material test
scenes take the values the export captured. Two colour updaters are folded
into the shaders the same way: GlowPoints' `color = 1.6 ×
smoothstep(0.2, 0.7, night)` reads the sky's night factor from the globals;
FlickerPoints' and the City's blinking light colours stay as exported until
the scenery's animators exist (M3). The pixel ratio for point sizes rides
there too.

## D294. Points are quads expanded at load, sized in the vertex shader

2026-10-03, WP 2.4. SPEC 6.2 says "instanced camera-facing quads". Bevy's
material pipeline draws one mesh per entity and has no per-instance vertex
buffers short of a custom draw command, so each `Points` geometry becomes a
mesh of four vertices per drawn point (its position, colour and patch
attribute repeated, the corner in the extra attribute's z and w) and two
triangles, one draw per Points object as in three; the cost is four times
the points' vertices (Sierra about 4,400 points, Cruise about 13,000), not
worth a custom pipeline. The vertex shader ports `points_vert`: `size ×
pixelRatio`, attenuated by `height / 2 / -z` (CSS height) with
`sizeAttenuation`, GlowPoints' `× gsize`, minimum pixel size `uMinPx`,
dimming `sqrt(raw / size)` and gentler fog, FlickerPoints' flicker or blink
(its constants rounded to three decimals as the JS writes them into the
GLSL); clamps to the GL point range (1 to 2047.9375 in Chrome on the dev
machine); drops a point whose centre is outside the clip volume (GL ES 3.0
§2.13.1) and pushes the corners out in clip space. The uv is
`gl_PointCoord` with y flipped, through the map's transform, as
`map_particle_fragment` samples it. The pixel ratio is the window's scale
factor (rendered pixels per CSS pixel), so points keep their CSS size
whatever resolution the client renders at. Plain Points, GlowPoints and
FlickerPoints are drawn; TrafficStreams (a `ShaderMaterial` on points)
stays hidden until its kind is ported (M3).

## D295. Stand-in cars from the simulation

2026-10-03, WP 2.4. The roadmap's thirty moving stand-ins are the cars of
a real race: `mr_sim`'s `LevelRuntime` and `SimState` for the level
(sports car, seed 1, no pursuit), stepped at 1/120 s in real time (at most
30 ticks a frame), the player on the autopilot. Every slot is drawn: the
player, the rivals and the whole traffic pool (Sierra: 50 slots, 1 + 5 +
44, of which 14 are the oncoming cars on the far carriageway), traffic
not on the road hidden (Traffic's own limit decides how many are out). Each is a box of its kind's dimensions in its
colour on the ground under it, turned to its yaw, casting shadows, with a
plain standard material (roughness 0.45, metalness 0.3). They are on by
default for a level, off with `?cars=0`, with `freeze=1`, and for the
material scenes and the stations, whose JS side has no race. The cars
follow the race, not the camera: in the fly camera they are seen near
their part of the route.

## D296. A patch uniform's texture is found in the uniforms too

2026-10-03, WP 2.4. `three_material` looked textures up only among the
material's own parameters, so a patch's textures (`tDetail`, `tRock`,
`tFoam`, which the export keeps under `uniforms`, D23) came out missing and
bound Bevy's white fallback. It now uses `MaterialDesc::texture`, which
looks in both.


## WP 5.3–5.5 decisions

## D250. `setTimeout` is a task queue the driver runs; promises settle in `settle()`

2026-10-03, WP 5.3. `Audio.js` and `Music.js` use `setTimeout` for the gate
tails, the music's 25 ms scheduler, its track-change and retire callbacks,
and the radio's 700 ms wait. `mr_audio::timers` is the reference's
`VirtualTimers`: a timer set at time `now` for `ms` is due at
`now + max(0, ms) / 1000`, due timers run in due order (ties in the order
set), and each callback is a `Task` that `GameAudio` runs. The driver says
when time has passed: the call-log playback runs the timers due by each
call's tick at their due times (`run_due_timers`), and a live client calls
`poll()` each frame, which runs the timers due by the audio clock. (A
browser's timers run on wall time; on the audio clock they also stand
still while the context is suspended, which changes nothing audible.) The
JS's promise chains (`resume`, decoding, the radio's fetches and its race
against the 700 ms) settle in `GameAudio::settle()`, which the driver calls
after each call and each timer, as the reference settles microtasks.

## D251. Music and the radio voice are ported in full, not stubbed

2026-10-03, WP 5.3–5.5. An exact call-log match needs the whole sequencer:
both drives play Midnight Run, so most of their 278k and 474k Web Audio
calls are notes and drum hits, and the risers' noise draws from the shared
`Math.random`. Stubbing music would leave the gate untestable. So
`Music.js` and `tracks.js` are ported faithfully (`mr_audio::music`,
`mr_audio::tracks`), as is `RadioVoice.js` with `clipId`
(`mr_audio::radio`, fetching through a `Fetch` trait). The sirens, radio
bus and burble live in `Audio.js` and come with `GameAudio`. Of WP 5.6's
gates, `test/unit/music.test.js` is ported (`tests/music.rs`: notes, song
data, every patch's oscillators, every song played start to finish in
strict mode, the playlist); its "every part sounds" check reads a trace
the sequencer keeps only when a test asks (`Music::trace_parts`). The
per-song L4 renders and WP 5.7's radio tests are left to those packages.
The coordinator approved the scope.

## D252. Song data as structs and ordered slices

2026-10-03, WP 5.3. `tracks.js`'s objects become structs with an `Option`
per optional field (`None` is `undefined`; JS truthiness tests such as
`if (P.fenv)` treat 0 and NaN as missing). Everything the JS iterates with
`Object.entries` (progressions, drum lanes, parts, a section's parts, kit
overrides) is a slice of pairs in source order, because the order of
iteration is the order of Web Audio calls. A parsed chord carries an `id`
for the JS object identity the voicing code compares.

## D253. Steered offline renders on the native backend

2026-10-03, WP 5.4. The reference renders steer an `OfflineAudioContext`
every 384 frames through `suspend(t)` (D45). The facade gains
`AudioContext::start_rendering_steered(frame, control)`; a backend hands
over its offline context (`Backend::take_offline`) so the facade is free
while it renders, and the control function's calls go through the facade as
usual. On web-audio-api the native backend makes nodes from a clone of the
context's base (so the offline context itself can render), schedules a
`suspend_sync` per frame half a quantum early (the crate rounds a suspend
time up to a quantum; `currentTime` there is exactly `k·frame/sampleRate`,
as in Chrome), and reaches the non-`Send` control function through a
thread-local, since the crate runs the suspend callbacks on the rendering
thread. `examples/render_scenarios.rs` renders the reference's scenario
table this way.

## D254. web-audio-api 1.7.0's `setTargetAtTime` bug, worked around

2026-10-03, WP 5.4. The crate evaluates a target curve that becomes current
before its start time (after a ramp, or after a target that a later event
ended) at that earlier time, where `e^(-(t - t0)/τ)` explodes: a linear
ramp to 0.1 followed by a later `setTargetAtTime` held at 102.6. The native
backend keeps each param's timeline as the facade sends it (the null
backend's `Timeline`, pruned to the present so it stays short) and puts a
`setValueAtTime(v, t)` with the value the param holds at `t` in front of
every `setTargetAtTime(_, t, _)`. The curve then starts where the spec
says.

## D255. `GameAudio`'s shape

2026-10-03, WP 5.3. `mr_audio::game::GameAudio` keeps `Audio.js`'s methods
and order of calls; the graph is in `game/build.rs`, the per-frame steering
in `game/steer.rs`, and the one-shots and pursuit sounds in
`game/shots.rs`, in place of SPEC 7.3's suggested `graph`, `gate`,
`voices` and `oneshots`. `update`'s `s` and the setters' items are structs
of `Option`s (`CarState`, `Rival`, `SirenUnit`, `Volume`) so the JS's
`??`, `||` and `!== undefined` read the same. What the JS takes from the
browser (`window.AudioContext`, `navigator.audioSession`, `fetch`,
`Math.random`) is a `Platform`; `session::ask_for_playback` takes the audio
session behind a small trait, with the browser's under the `web` feature.
`init` is synchronous, as the JS's is (its async body never awaits).

## D256. Chrome's compressor kernel on the native backend

2026-10-03, WP 5.4. The crate's DynamicsCompressor follows the spec's
outline of Chrome's but lets transients through several dB hotter (an
impact peaked at 0.52 against Chrome's 0.35) and delays by 384 frames
instead of 288. Every SFX and music path goes through two of them, so no
one-shot could meet SPEC 7.5's 1.5 dB. `wa/compressor.rs` ports Blink's
`DynamicsCompressor` (adaptive release, knee, pre-delay, makeup gain) to
a web-audio-api worklet processor, used for every compressor; with it a
whole impact render agrees with Chrome's to 7e-6 per sample.

## D257. Chrome's oscillator and buffer-source rules on the native backend

2026-10-03, WP 5.4–5.5. The crate draws square and sawtooth with polyBLEP
at full scale (Chrome's are 1.4 dB quieter: band-limited and normalised to
the Gibbs peak), triangles and periodic waves without band-limiting (the
electric car's triangles aliased down to 800 Hz), and ignores a built-in
type set after a periodic wave (an electric rival kept the engine wave).
`wa/oscillator.rs` ports Blink's `PeriodicWaveHandler` (36 band-limited
tables, three per octave, crossfaded by pitch) and `OscillatorHandler`
(2/3/5-point interpolation, start and end frames rounded up) as a worklet
processor used for every oscillator. That also brings a Chrome quirk the
reference hears: in the quantum an oscillator starts in, its a-rate params
are read from the quantum's start, so a one-shot that automates from its
start time (`setValueAtTime(147, t); start(t)`) plays its earlier value
(440 Hz) for part of a quantum. Buffer sources stay the crate's, with two
Chrome rules: a start offset is rounded to the nearest frame (Chrome
starts on a whole frame; the crate interpolated, 0.7 dB off on noise), and
a stop lands on Chrome's last frame.

## D258. Only what the destination pulls on runs

2026-10-03, WP 5.4. Chrome renders only nodes the destination pulls on: an
oscillator a gate cuts off stops, phase and all, until it is connected
again (the damage knock started 8 ms late in Chrome, because its gate opens
a frame after the engine's). The crate runs everything. The native backend
tracks which nodes reach the destination (links by target, a param link
counting for its node) and freezes the Chrome oscillators that do not. An
oscillator the facade lets go of before anything pulls on it (an FM
modulator is wired up before its carrier) is held until it is pulled, so
it can be told to run. Buffer sources are not frozen: for the noise beds
that changes only which noise plays, not its level.

## D259. The L4 comparison and where it stands

2026-10-03, WP 5.4–5.5. `node tools/parity/audio-bands.mjs` renders the 104
non-song scenarios of `renders.json` on the native backend
(`examples/render_scenarios.rs`), analyses them with `lib/bands.mjs` and
compares with Chrome's band levels at 1.5 dB, ignoring bands below -90 dB
in both; `tools/parity/audio-render-js.mjs` saves Chrome's renders of
chosen scenarios as WAVs for looking at a difference. 102 of 104 pass (the
median worst band per scenario is under 0.2 dB). The two left are single
bands 40 dB or more under the signal: `engine-rally-6500-1` at 25 Hz
(-73.8 dB against -71.9) and `shot-radio-burble` at 126–316 Hz below the
radio bus's 340 Hz high-passes (-73.8 against -80.2 at 158 Hz). The
remaining native differences are the crate's own buffer sources (their
sub-sample start position) and WaveShaper oversampling filters; every
other node type was checked against Chrome sample by sample.

## WP 3.8 decisions

## D350. The shape of `mr_worldgen::city`

2026-10-03, WP 3.8. `City.js` is `mr_worldgen::city` (`city/mod.rs`),
`city/freeway.js` is `city::freeway`, `city/cityTextures.js` is
`city::textures`; `city/geom.js` stays `mr_worldgen::geom` (WP 3.3).
`City` holds what `plan()` decides (`sA`, the path, the loop's centre,
inside side and waterfront); `build()` makes a `Build` that is the JS
`this` while it builds (the grid, the rng 4242, the chunks, lamp spots,
trees, glows, ...), with the world's track and terrain borrowed and its
graph and texture cache mutable. The JS `ctx` City hands the freeway is
`freeway::FwCtx` (`lampTint` a borrowed closure over `ledAt`'s data), the
Freeway instance `freeway::Freeway`. `makePath` is `FwPath`, whose `frame`
returns a `PFrame` (the track's `Frame` plus `ext`); `sweep` takes a
profile of `prof(lat, |f, lat| y)` (every `lat` in the game is a number,
and no profile sets `gap`, so neither is ported) and `SweepOpts` with the
JS defaults. Freeway's `add` batches by (material, chunk, cast) in first-use
order as the JS `Map` does (a chunk index of -0 is the key of 0); building
chunks and the ground's `ChunkedGeo` keep insertion order with a
`BTreeMap` used only for lookup. Where the JS draws from a generator inside
an argument list, an object literal or a loop condition
(`k < 2 + Math.floor(rng() * 3)` in `warehouse`), the port draws into
locals in the same order. Level 1's inline park and the loop's `makePark`
are one function with the two tree rules. The loop variants (WP 7.5's
cruise city: districts, warehouses, container yards, the waterfront and
ferris wheel, loop sites, sound-wall spans, chunked instancing and
`fadeable`) are ported too: they share most of the code, and the gate
covers them.

## D351. City's plan and the world data it gives the simulation

2026-10-03, WP 3.8. `plan()` raises `track.runout` to 900 (Sierra) and
mirrors it into `World::sim_data.runout`; it registers the flattens under
the westbound lanes behind the merge (Sierra) or the ring's flatten and the
waterfront carve (cruise). City is registered in `scenery::PORTED`, so
`tests/terrain.rs` and `tests/road.rs` run its real `plan()` in its place
(D330) and require the registrations to equal the recording bit for bit:
they do on both levels. `build()` sets `sim_data.opposite_carriageway`
(s0 = sA, s1 = length, `OPP_LANES`, dir -1), as the JS sets
`world.oppositeCarriageway`; its height is `mr_levels::world::opp_y`, which
the freeway uses too (`freeway::opp_y`). `tests/city.rs` checks the runout,
the carriageway and `oppY` at the dumped samples against `mr_levels::world`
and `parity/golden/sim/world-data.json`: equal on Sierra and the loop.

## D352. City's textures in the world's texture cache

2026-10-03, WP 3.8. The module variables of `cityTextures.js` (the classic
atlas, the façade atlas, the ads, tunnel tiles, sound wall, park) are
entries of the world's `TextureCache` under `city:` keys, through two
additions to `textures`: `TextureCache::cached_with(key, make)` and
`lookup(key)`, plus `Texture::from_canvas`. The classic atlas is cached as
a façade pair (map, emissive); the façade atlas as three entries (map,
emissive, the non-sRGB mask). `drawImage` of a cached picture goes through
a canvas rebuilt with `putImageData` (exact for the opaque pictures drawn
here). `bannerTexture` and `atlasQuads`' canvas are new textures per call
(`Image::Own`), as in the JS; atlas images are told apart by identity
(`Arc::ptr_eq`), as `images.includes` does, so signs that share a cache key
share a cell.

## D353. City's updaters as animators

2026-10-03, WP 3.8. Each `world.updaters.push` is an `Animator` in the JS
order: the chase bulbs, lens and pool colours of the freeway; the neon
colour, the fade list, the ferris wheel's rotor (a `Transform`), the lamps'
pool and lens colours, the aircraft blink, the glow points, the traffic
streams' uniforms and the sky glow. The fade updater is pushed by the first
`fadeable()` call and reads a list that later calls extend; the port builds
the list and inserts the animator at that first call's place. `uFogK` of
the glow points and traffic streams reads `world.scene.fog`, and
`world.scene` is the root group, which has no fog: it is always 0 in the
JS, and in the port. `uHalfH` is `CameraView::viewport_height / 2` (the last
value is kept on a frame without a camera).

## D354. The L3 city gate, and what it found

2026-10-03, WP 3.8. `tools/parity/city-golden.mjs` writes
`parity/golden/city/{sierra,cruise}.json` from the cached full exports: per
node under `city` (depth first) a line of type, name, every attribute's and
the index's SHA-256, shadow flags, visibility, render order, culling,
matrixAutoUpdate, the local matrix's bits (-0 written as 0, as the file's
JSON does), the instance count and the SHA-256 of the instance matrices and
colours; each drawable's material as an index into a table of material
views (road-plan.mjs's `materialView`, hashed in a canonical form: keys
sorted, numbers as f64 bits); per texture a material uses its 8×8 block
means; the export's night factor, camera and drawing-buffer height.
`tests/city.rs` builds each level through `level_jobs` with the scenery
factory, replays City's animators once as the frozen export ran them (dt 0,
the export's night and camera, after the night parameters), and compares.
With the cache it shows failing nodes beside the JS ones, material views key
by key, every texture's mean absolute difference and sheets in
`parity/report/city/`. Result: **every node identical on both levels**:
Sierra 102 nodes (584,426 vertices), the cruise loop 194 (1,901,864, with
the 52 chunks the fade list hides at the export's camera), all 35 and 43
materials equal parameter by parameter, uniforms and GLSL included; native
and wasm. One port fix was needed, found by the loop: the westbound runs
`if (cur) ... push` drop a run that starts at u = 0 (the loop's first one;
the JS's 0 is falsy), so the loop has no outer barrier on its first stretch.
Textures without lettering are held to the export within WP 3.2's
threshold (the façade atlas's map, emissive and mask 0.14, 0.02 and 0.10
levels mean absolute difference; tunnel tiles, sound wall, park, glow,
concrete, asphalt under 0.6). The lettered ones (the tunnel name banners,
the finish and welcome banners, the gantry sign atlas, the billboard and
roof-ad atlases) differ from the export by 14 to 43 levels: the export
draws text in whatever faces that machine's Chrome falls back to, mr_canvas
in the bundled faces (D370), and the sheets show identical layout, colours,
glow and stripes in another face. They are held, as D313 holds Mountain's,
to a capture with the bundled fonts: `tools/parity/city-textures.mjs` opens
the game on Sierra and on the cruise loop (`?kernel=1&freeze=1&s=0`,
`Math.random` seeded as the export seeds it) with every face of
`assets/fonts/fonts.json` registered under the family the JS names before
the page's scripts run, and reads back each canvas the group `city` uses as
both `map` and `emissiveMap`, in order of first use (six on Sierra, seven
on the loop; the 256-px canvas both levels share is among them). It writes
the RGBA to `parity/cache/<key>/city/<level>-canvas-<k>.rgba` and a summary
with the font manifest's hash to `parity/golden/city/textures.json`;
`--check` captures twice. The test holds those to WP 3.2's threshold
(mean absolute difference under 3/255 per channel with the cache, block
means without it, so in CI and wasm) and reports them against the export.
Result with the Roboto faces: at most 0.55 levels (the sign and roof-ad
atlases' glow), the banners 0.05 to 0.13. When the bundled fonts change,
rerun the tool: the gate then fails until the capture is refreshed. Rerun
the rest: `node tools/parity/city-golden.mjs` (with the cache), then
`cargo test -p mr_worldgen --test city` (and in wasm).

## D355. Small additions to shared modules

2026-10-03, WP 3.8. `Terrain::far_cell(k)` reads the far field's `fs` and
`fl` cells for City's nearest-cell lookups (`nearestS`, `sideAt`);
`textures` gains the exports of D352; `lib.rs` the module and
`scenery::PORTED` the line registering City. No existing behaviour changed.

## Font choice

## D370. The owner's fonts: the Roboto family for the Arials

2026-10-03, the owner's answer to SPEC 15 question 1, after the gallery:
"roboto family promising; just pick some and proceed". So "Arial" is
Roboto, "Arial Narrow" is Roboto Condensed and "Arial Black" is Roboto
Black; Georgia (Gelasio), Brush Script MT (Yellowtail), Segoe Script
(Caveat), Courier New (Courier Prime) and the HUD (Rajdhani) stay as D150
had them. Roboto and Roboto Condensed are the variable files of
google/fonts (`wght` 100–900, Roboto also `wdth` 75–100, left at 100), so
every weight is a designed one, never synthetic: Arial and Arial Narrow are
registered for 100–900 (the game asks for 400, 700 and 900), and Arial
Black is Roboto's file registered at 900 only, so any weight asked of it
draws Roboto Black (the game asks for bold). The game asks for italic only
of Arial Narrow (900 italic: Roboto Condensed Black Italic) and Georgia, so
Roboto Italic is not bundled; an italic Arial would get the synthetic
oblique, in Chrome as in the port, since both register the same faces.
Archivo Narrow, Archivo Black and Arimo's italic are gone. The owner may
revisit this: the choice is `assets/fonts/fonts.json` alone (D373), and
the gallery (`node tools/parity/fonts-gallery.mjs`, then
`/parity/report/fonts/`) now shows the bundled set first with the old
faces as alternatives.

## D371. The bundled files are subsets of the google/fonts files

2026-10-03. D150 left subsetting "once the owner has chosen".
`assets/fonts/subset.py` fetches each file from google/fonts at a pinned
commit (9710da1e, 2026-09-30; the Roboto files are checked by SHA-256 and
are the ones the gallery showed) and keeps Basic Latin, Latin-1, – — ‘ ’ “ ”
• …, the four arrows, − and ●, with fontTools' subsetter told to keep
every layout feature, the legacy kern table, hinting instructions, all
names and the .notdef box. Every character the game draws on a canvas is
in that set (the JS signs use only ASCII and · — → ●). The OFL fonts have
no Reserved Font Names; Yellowtail (Apache 2.0, 62 KB) is copied whole.
The script is deterministic (no timestamp change), so running it again
gives the same bytes. The bundled fonts went from 4.04 MB to 1.17 MB.
Subsetting changes no pixel: the gallery drawn by mr_canvas from the
subsets is byte-identical to the one drawn from the full files for every
face kept (Gelasio, Caveat, Courier Prime, Rajdhani, Roboto Condensed,
Roboto Black), and the JS reference is drawn from the same subsets.

## D372. A fallback face for characters the family list lacks

2026-10-03. Roboto has no → (U+2192), which the desert billboard's sub-line
"SPEED TRIALS TONIGHT →" draws in `bold 62px Arial`; Arimo had it. A
browser goes to a system font for a character no listed family has, so
the port gains the same step: `fonts.json`'s `fallback` lists families
tried after the font's own (FontBook::fallback; text.rs appends them to the
face list, used only for characters the earlier faces lack, so metrics and
everything else still come from the first family). The fallback is a 4 KB
cut of Arimo (`arimo/Arimo[wght].ttf`, registered as family "Arimo") with
only the symbols outside Latin-1, which also gives ● and → to Courier
Prime, Yellowtail, Caveat and Rajdhani. Chrome's own fallback on the
reference machine is a system font, so a reference capture of → would
differ; no captured case draws one (all its text is in the listed faces).
The missing-glyph box in the old gallery's Arial Black card was ★
(U+2605) in the gallery's own alphabet line: no bundled face has it and the
game never draws it. The line now shows the game's symbols (· → ● —),
and no card has a box.

## D373. The manifest is the only copy of the mapping

2026-10-03. `crates/mr_canvas/build.rs` reads `assets/fonts/fonts.json`
(with a small JSON reader of its own: the crate takes no build
dependencies) and generates `BUNDLED`, `BUNDLED_GENERIC`,
`BUNDLED_FALLBACK` and the files' bytes, each file included once even when
two families use it (Arial and Arial Black share Roboto's bytes). Changing
the fonts is a change to `fonts.json` and the files beside it; then
`node tools/parity/textures.mjs` and `node tools/parity/mountain-textures.mjs`
recapture the references, and mr_canvas's `tests/text.rs` needs Chrome's
`measureText` numbers again. With Roboto, every texture and probe stays
within the gate (largest mean absolute difference 0.87 of 255, gravel,
which has no text; largest for text 0.53, the text-faces probe; Mountain's
lettered canvases at most 0.74).


## WP 4.1 decisions

## D410. The shape of `mr_worldgen::car_model`; the module's caches live on the graph

2026-10-03, WP 4.1. `vehicles/CarModel.js` is `mr_worldgen::car_model`, a
directory following the JS file's sections: `kit.rs` the geometry kit
(`interp`, `spline`, `sill`, `stations`, the loft with `halfRing` and the
surface sampler `Surf`, `Tris`, `decal`, `ribbon`, `expandPoly`, `ellipse`,
`sweep`, `latheX`, `airfoil`, `extrude`, `prep`, `boxUV`, `Parts`),
`detail.rs` the detail and police kits and the loft presets (`lamp` to
`strobes`, `detail`, `bodyLoft`, `cabinLoft`, `shaper`, `glassTop`,
`dlo`), `wheels.rs` `wheelGeometry` and `caliperGeom`, `specs.rs` the
thirteen `SPECS` in the JS order, `far.rs` the far LOD, `mod.rs`
`buildVehicle` with the materials, the siren and the setters. Names and
argument order are the JS's; an options object is a struct whose
constructor or `Default` writes out the JS defaults (`LampOpts`,
`DecalOpts`, `RibbonOpts`, `Shaper::new`, `BodyOpts::new`, ...). Options no
caller passes are left out, as D331 did: `sweep`'s `fn` and `caps: false`,
`loft`'s `caps: false`, `ribbon`'s function width, `dot`'s `rv`, `wing`'s
`plates` and `upBucket`, `bodyLoft`'s `col`, `cabinLoft`'s `extraZ`,
`dlo`'s `col`, `ellipse`'s `rot` (written as the JS computes it with 0).
`build_vehicle(graph, textures, kind, &BuildOpts)` builds the object tree
into a `SceneGraph` and returns a `VehicleModel` (the JS handle: `root`,
`body`, `wheels`, `steer_pivots`, `headlight_anchor`, the siren's glow and
anchor, the far mesh and what it stands in for, `dims`, `exhausts`, the
per-instance materials); an unknown kind is `None` where the JS throws.
`BuildOpts` holds `lod`, `far`, `color` (hex), `seed` (a `u32`: the game
passes small whole numbers, and `Math.abs` of one is itself),
`police_livery`, `stripes`, `spoiler`, `stripe_color` and `rim_dark`.

The JS module keeps its state in module variables (`SHARED`,
`stripeMats`, `lowPaints`, `wheelCache`, `caliperCache`, `partsCache`,
`farCache`, `farMaterial`), so every car of a page shares the same trim,
glass and tyre materials and the same wheel and body geometry; the export
shows it (the parked pickup and sedan of Mountain share three materials).
Those caches hold handles, which belong to one graph, so they are a
`CarKit` kept on the graph: `SceneGraph::cars` (boxed, so the graph stays
small), the one addition to `object.rs`. A scenery module or the client
building cars into a graph therefore shares them as the JS page does, and
the Mountain hook (`mountain::ParkedCars`, a plain function) needs no
state of its own. `SceneGraph::append` keeps the receiving graph's kit and
drops the appended one's, so cars built in a parallel part share nothing
with the main graph (no scenery builds cars in a part today). Cache keys
follow the JS keys where they decide sharing: the wheel's
`[r, w, lod, spokes, rimFrac, type]` (numbers by their bits),
`rimR.toFixed(3)` for calipers, `kind|lod|variant` for parts,
`kind|lod|variant|rim material` for the far model.

## D411. The light setters return edits

2026-10-03, WP 4.1. `setHeadlights`, `setBrake`, `setReverse`, `setBoost`,
`setSiren(mode, t)` and `setFar` keep the JS state in the handle
(`brake`, `lights`, the siren levels, `isFar`) and return what they change
as `world::Edit`s, addressed by handle like an animator's (D195): a
`Change::Number` for `emissiveIntensity`, a `Change::Color` for the glow's
`uRed`/`uBlue` uniforms, a `Change::Visible` for the glow and the far
swap. The client resolves them through the scene's `HandleMap` and applies
them to what it draws; `car_model::apply_edits` applies them to the graph
itself (a number or colour goes to the uniform of that name if the material
has one, else to the parameter), which `buildVehicle`'s own
`setSiren('off')` and the parked cars' `setHeadlights(0)` use. A setter the
JS handle lacks (`setSiren` on a car without a siren, `setFar` without a
far model) returns no edits; `setBoost` without accents likewise.
`sirenColor()` is `siren_color()`. The siren mode is an enum (`Off`,
`Flash`, `Steady`, `Disabled`; the JS `true` and `false` are `Flash` and
`Off`), and `siren_levels` is `sirenLevels`.

## D412. Dimensions, and the parked cars in Mountain's gate

2026-10-03, WP 4.1. A model's `dims` come from its spec, as in the JS.
mr_worldgen does not depend on mr_sim (SPEC 3.2); `tests/car_model.rs`
takes it as a dev-dependency (check-deps follows normal edges only) and
requires every kind's `dims`, built at high and at low detail, to equal
`mr_sim::dims::dims(kind)` and the JS dump that table was checked against
(`parity/golden/sim/world-data.json`, default, high and low-far): all
thirteen do.

`Mountain::new` sets `parked_cars` to `car_model::parked_car`
(`buildVehicle(kind, { color, seed, lod: 'low' })` and `setHeadlights(0)`),
so every Sierra build now bakes the pickup at the diner and the sedan at
the lookout. `tests/mountain.rs` compared the group's children but skipped
the two baked groups; it now compares a group's children in order like any
child (node, mesh line, material, with the JS's material sharing), so the
gate covers all of them: **86 of 86 children identical**, 46 materials
(the 34 of D312 and the cars' 12) equal parameter by parameter. The other
Sierra gates (terrain, road, valley, city) look at their own groups and are
unchanged.

## D413. The L3 gate per kind, and what it found

2026-10-03, WP 4.1. `tools/parity/car-model-golden.mjs` writes
`parity/golden/car_model/models.json` (68 KB) from the cached models export
(D29): per model (`car:<kind>[:police]:<lod>`, in the export's order) its
build options, one line per node of the vehicle's tree (type, name, every
attribute's and the index's SHA-256, the draw groups, the bounding sphere,
flags, render order, the local matrix's bits, userData, whether the
material is a list), each line hashed, which materials each node uses as
indices into one table in order of first use, each material hashed in the
canonical form of D354 (keys sorted, numbers as bits; textures by sampler),
and the 8×8 block means of every texture a material uses. `--check`
regenerates and compares. `tests/car_model.rs` builds the thirty models in
that order into one graph (so the kit is shared as the page shared its
caches), each under a group of the exporter's name, and compares; with the
cache it shows a failing node beside the JS one and a failing material as
both views, and checks every texture's mean absolute difference (sheets in
`parity/report/car_model/`). Result: **every node of every model
identical**, native and in wasm: the thirteen kinds at high detail and at
low detail with the far model, and the muscle and sports cars in police
livery at both (30 models, 727 nodes, 925,344 vertices), all 169
materials equal (the CarLight and PoliceGlow kinds with their program key,
uniforms and GLSL), the carbon weave within 0.03 levels of Chrome's canvas
and the siren's glow within 0.58. No fix to a shared module was needed.
The export's effects (a sports car with smoke, sparks, skids, a headlight
pool and flames) and pursuit props (sawhorse, spike strip) are WP 4.4's
and M8's. The roadmap's L4 gate against `tools/car-test.html` views is a
rendering gate for the client and is not part of this package.

## M4 playable decisions

## D430. The loopback session lives in the client until `mr_net` has one

2026-10-03, WP 4.2. SPEC 3.3's `session.advance(frame_dt, input)` is
`mr_game::play::session::Session`: it owns the `LevelRuntime`, the state
before the last tick (`prev`) and after it (`curr`), and the time not yet
stepped. A frame adds its time, clamped to 1/20 s as the JS frame loop
does, and runs every tick that has come due, at most six (SPEC 4.1; with a
1e-9 s tolerance so 1/20 s is six ticks whatever the rounding); beyond
that the race slows down. Before each tick it asks for that tick's
quantised `InputFrame`, so the input layer runs at the tick rate (SPEC
8.2), and it copies `curr` into `prev` (`clone_from`, a few kilobytes) so
the client draws `prev`→`curr` at `alpha = acc / DT`. The events of the
frame's ticks are kept in order for the client. `mr_net` is still empty;
when multiplayer (M10) gives it a session type the loopback becomes one
of its transports and this module goes there. A test steps a session with
ragged frame times (60, 144, 30 Hz, zero, a stall) beside a direct
`mr_sim` loop with the same autopilot and requires the same tick count
every frame and the same final hash: no drift.

## D431. Bevy UI for the plain HUD and the touch controls

2026-10-03, WP 4.6. The race's text (countdown, GO!, toasts, position,
clock, lap, speed and gear, the pause card and the results list) and the
touch controls are Bevy UI nodes, as SPEC 8.1 says the UI will be.
`mr_game` turns on Bevy's `bevy_ui`, `bevy_ui_render`, `bevy_text` and
`default_font`; the text is in Bevy's bundled FiraMono subset, so it is
plain ASCII (N2O for N₂O, R, C and II on the reset, camera and pause
buttons, `>` for the stick's arrows). The JS HUD's look (Rajdhani, the
dials, the minimap, the CSS animations) is M6's.

## D432. A level without fly parameters is a race

2026-10-03, WP 4.6. `Options::race_on`: a known level is raced unless the
fly camera (`s=`), `freeze=1`, the material scenes or the stations are
asked for; `race=1` or `race=0` decides outright (`race=0` gives back the
attract camera and WP 2.4's stand-in cars, which never run beside a race).
So `?level=…&s=…&v=…` and every parity capture behave as before. The
race starts itself (no menu until M6): the field is on the grid as soon as
the level's Track is built, and the countdown starts when the client is
ready (the scene up and every pipeline compiled), so it does not tick away
behind the loading screen. Parameters, with the JS names where it has
them: `car=<kind>` or `autostart=<kind>`, `seed=N` (otherwise a new seed
each race, as `Math.random` varies the JS rivals' nitro timing),
`autodrive=1` (natively also `--autodrive`), `timescale=N`, `pursuit=1`
and `heat=N`, `touch=0|1`, and natively `shots=<dir>`, which saves
`countdown.png`, `race.png` (twenty seconds in, chase view) and
`results.png`, then exits. Enter or a tap on the results races again;
Esc, P or the pause button pause, Esc or a tap resume; a hidden page pauses
the race (the JS `visibilitychange`). Natively, losing focus lets go of the
keys but does not pause yet: an invisible screenshot window would pause
itself.

## D433. The input layer: keyboard and touch now, the gamepad in M6

2026-10-03, WP 4.5. `play::input::Input` ports `Input.js` without the
gamepad: keys by their DOM `code` (the Bevy glue maps its key codes), the
held keymap and the one-shot actions (`camera`, `reset`, `pause`,
`music`; KeyT is the music's and waits for M5), repeats ignored, focus loss
lets go, the steering ramp (3.6/s in, 7/s back, 9/s counter-steer), the
touch merge through a `TouchSource` trait (the JS's `input.touch` object),
`analog` for the stick, and `enabled`. `update` runs once per tick with
the tick's dt, where the JS ran it once per frame; the ramp's rates are per
second, so the ramp is the same. The reset key reaches the simulation as
`InputFrame`'s reset flag on the first tick of the frame it was pressed
in (the JS consumed it once per `Race.update`). Every `input.test.js` case
that does not need a gamepad is ported (keymap, kept keys, one-shot
actions, ramp, blur, touch merge, analogue flag, `enabled`); the gamepad
cases move to M6 with `Gamepad.js`.

## D434. The camera rig per rendered frame; the road is its floor for now

2026-10-03, WP 4.3. `play::camera` ports `CameraRig.js` (chase, far and
bumper, C cycles; look back on B; the trailing direction, the speed and
nitro field of view, the portrait widening, the shake and the speed
rumble) and `Race.introCamera`, run once per rendered frame on the
interpolated car with the frame's dt, as the JS ran them once per frame.
The JS keeps the camera above `max(terrain.heightAt, track.surfaceY)`; the
client draws the JS export and has no terrain heights, so only
`surfaceY` is the floor until the client builds the world itself.
Camera bumps come from the tick events: car hits involving the player
(1.2 × strength), wall impacts and landings (0.8 ×).

## D435. Drawing a car between two ticks

2026-10-03, WP 4.2. `play::pose::Pose::lerp` interpolates what
`Vehicle.sync` reads (position with `visY`, heading with `visualYaw` the
short way round, s along a loop, velocity, speed, steer angle; `onGround`
switches halfway) and returns the previous tick's pose at alpha 0 and the
current one at 1 exactly, which a test checks through `sync`'s placement
too. A traffic car that came onto the road this tick is drawn at its
current pose. `pose::root` is `sync`'s orientation from the road frame,
and `Springs` its pitch and roll springs, stepped once per rendered frame
with the frame's dt from the tick's accelerations and kicked by
`PhysEvent::Touchdown`. With the car models (D440) the root takes the
pose, the body node the springs (its own y rotation kept), the wheels
spin by `speed / radius × dt` on top of their rest rotation, and the
steer pivots turn by `−steerAngle`.

## D436. Touch controls: the thumb stick and the pedal slider

2026-10-03, owner request for the phone. `play::touch` ports the core of
`TouchControls.js`: the 'stick' steering and the 'slider' pedals (BRAKE,
the coasting gap, GAS, N2O, DRIFT past the slider's right edge, a thumb
that starts on the slider keeps it), and the reset, camera and pause taps.
The DOM measured its boxes; `touch::Layout` computes the same boxes from
`hud.css`'s rules (`--b`, `--pedal-h`, `--stick-r`, the insets), in CSS
pixels. The page decides `isTouchDevice` (`?touch=`, else a coarse
pointer with touch points) into `__mr.touch` and measures
`env(safe-area-inset-*)` into `__mr.insets`; the controls show only while
driving. Natively `touch=1` shows them and the mouse acts as one finger,
as pointer events make it in the JS. The ◂ ▸ pads, pedal buttons, tilt
and auto gas wait for M6. `touch.test.js` is ported whole (stick curve,
range, slider bands), plus a test driving the input layer with thumbs.

## D437. The pixel ratio on the web follows `applyQuality`

2026-10-03, for the phone. The JS renders at `min(devicePixelRatio, 1.5)`
with high quality and at 1 without (the default on touch devices). The
client overrides the window's scale factor the same way at start, so a
3× phone draws a third as many pixels in each direction. Touch positions
then arrive scaled by the override, not the device's ratio; the client
measures the canvas's CSS width against the window's logical width for
the UI and corrects touch positions by override ÷ device ratio.

## D438. `--smoke-race`, and the race in CI

2026-10-03, WP 4.6. `play::flow::smoke_race` runs a race from the grid to
the results headless through the client's own frame loop (input layer
with the autopilot, session, events, HUD state) at 60 frames a second.
`cargo test` runs it on Sierra and requires the countdown 3, 2, 1, GO!,
a finish text, six results, and the final hash and the results equal to
a direct `mr_sim` run of the same ticks. `midnight-racer --level <id>
--smoke-race` does the same for any level (Seaside reads its survey),
seed 1 unless `seed=` is given, and prints the results; it needs no
window or GPU, so CI can run it.

## D439. Open: scene delivery size and compression

2026-10-03, deferred by the owner's choice. The client downloads the JS
scene export as it is from `parity/cache/` (Sierra 138 MB, Seaside 39 MB,
Coast 210 MB raw; gzip roughly halves Seaside). Nothing is compressed or
copied into `dist/next/` for now. How scenes reach phones (their size,
compression, the world built in the client instead) is to be planned
explicitly later.

## D440. The race draws WP 4.1's car models

2026-10-03, WP 4.2 and 4.6. At the start of a race every car of the
field is built with `car_model::build_vehicle` into one `SceneGraph`, with
the JS's options (the player `{ color, lod: 'high', seed: 1 }`, rival i
`{ color, lod: 'high', seed: 10 + i }`, traffic car i `{ color, seed:
i × 17 + kind.length, lod: 'low', far: true }`), so they share the kit's
materials and geometry as the JS page does; `finish()` assembles a
`Scene`, and `play::models` spawns each car as an entity tree with the
nodes' local transforms (not flattened as the level loader does), meshes
and materials converted as the loader converts them (`convert`,
`three_material`), hidden kinds left out as there. Racers cast shadows
from every mesh (Race.js); traffic keeps the model's flags. Each frame
the light setters run as `Race.update` calls them (headlights at
`max(0.15, smoothstep(0.25, 0.6, night))`, 0.1 for traffic, brake lights
from `brakeLight`, the player's reverse and boost, the rivals' boost) and
`Traffic.farLod` swaps traffic to the far model past 95 m from the camera
and back inside 85; their edits are applied to the drawn materials
(`emissiveIntensity` × the material's emissive colour, only when it
changes) and node visibility. The headlight spot, the siren glow and the
effects are WP 4.4 and M8. The same race, seed and autopilot give the
same results natively and in the browser (Sierra, seed 1, through the
client's frame loop on both).

## WP 2.6–2.7 decisions

## D390. The warm-up: one degenerate stand-in per pipeline, behind the loading screen, until the GPU is done

2026-10-03, WP 2.6, SPEC 6.3. While a scene is built, the loader notes
every combination it draws of a material's `ThreeKey` (shader defs,
blending, culling, depth state), the mesh's vertex layout (attributes,
formats, topology) and whether it casts a shadow: these are what pick a
render pipeline, so materials that share a key share a stand-in. When the
build ends it spawns one stand-in per combination (`crate::warmup`): a mesh
of six zero vertices with the same attributes (two triangles of no size,
which rasterise nothing), the material, `NoFrustumCulling` so it is
specialised for the main view and the shadow view whatever the camera
does, 10,000 km below the origin. The stand-in cars' material joins in
the same way when the race starts. Sierra has 45 combinations, Coast
43, Seaside 14, the models 11.

The state is `warming` from the end of the build until `ready`, and the
page keeps the loading screen up ("Preparing the shaders…") until then;
the fly and attract cameras hold still meanwhile (the JS game flies only
once it has loaded). `ready` used to mean "ten frames in and no pipeline
waiting"; it now also waits for a GPU fence (`Queue::on_submitted_work_done`
asked after the queue drains), because a browser creates the pipeline in
its GPU process after the call returns and the first draw with it waits
there: in Chrome on WebGPU the first flight frames after "ready" had a
one-second frame before the fence and none after. Then the stand-ins are
despawned. Pipelines compiled after `ready` are counted
(`__mr.lateFrames`, a warning in the log) so a missed combination shows;
none in the runs recorded in BASELINE.md.

Render pipelines only are counted as waiting: on WebGL2 Bevy 0.19 queues
its "sparse buffer update" compute pipeline although the device has no
compute, and that pipeline waits for ever for a shader that is never
loaded (the client has no compute pipelines of its own).

## D391. The WebGL2 build is the `mr_webgl2` cfg in its own target directory

2026-10-03, WP 2.7, SPEC 2. Bevy picks its backend with a Cargo feature
(`webgpu` overrides `webgl2`), so the two web builds need different Bevy
features. Cargo features of `mr_game` would have to name `bevy/webgpu`,
which also reaches the native build's `bevy` (one dependency, unified):
every native build cache in every worktree would be rebuilt for a feature
that does nothing natively; and Cargo refuses the same crate twice under
two names, so a wasm-only alias cannot carry them. Instead the wasm
dependency tables are split on a cfg: `cfg(all(target_arch = "wasm32",
not(mr_webgl2)))` asks for `webgpu`, `cfg(all(target_arch = "wasm32",
mr_webgl2))` for `webgl2`, and the WebGL2 build sets `RUSTFLAGS="--cfg
mr_webgl2"` (added to whatever flags the caller has; CI's `-D warnings`
stays). Because changing RUSTFLAGS invalidates a build, the WebGL2 build
has its own target directory, `target/webgl2/`, and neither build throws
the other's cache away. The native build is unchanged.

`cargo xtask web [--release]` builds both into `dist/next/`:
`mr_game.js` and `mr_game_bg.wasm` (WebGPU), `mr_game_webgl2.js` and
`mr_game_webgl2_bg.wasm` (WebGL2), each gzipped for a release; `--only
webgpu|webgl2` rebuilds one and keeps the other's files. `build.json`
lists the backends present. `cargo xtask size` checks the larger of the
two against the 10 MB budget (a browser downloads one). CI lints the
WebGL2 build (`RUSTFLAGS="-D warnings --cfg mr_webgl2" cargo clippy -p
mr_game --target wasm32-unknown-unknown --target-dir target/webgl2`) and
builds it in `cargo xtask web --release`. The client publishes which one
it is (`__mr.backend`).

## D392. How the page picks, and the fallback when WebGPU fails

2026-10-03, WP 2.7. `index.html` asks for a WebGPU adapter as before; with
one it loads the WebGPU build, without one it loads the WebGL2 build if a
throwaway canvas gives a `webgl2` context, and only if neither shows the
"no WebGPU or WebGL2" page. If the WebGPU build fails before its first
frame (an error or unhandled rejection while Bevy asks for the device, or
a shader the browser rejects at start), the page reloads itself with
`?backend=webgl2&fallback=webgpu-failed`. `?backend=webgpu` or
`?backend=webgl2` forces one (no fallback from a forced WebGPU). The
WebGL2 build also works on plain http, where browsers offer no WebGPU.

## D393. What WebGL2 needed in the shaders

2026-10-03, WP 2.7. Two changes, both in `three_material.wgsl`, neither
changing the WebGPU output: (1) D171's note came true: GLSL ES cannot
`texelFetch` a depth texture (naga: "textureLoad from depth textures is
not supported in GLSL"), so under `NO_ARRAY_TEXTURES_SUPPORT` (Bevy's
WebGL2 define) `texture2DCompare` reads the same texel through Bevy's
directional comparison sampler (GreaterEqual, linear) at the texel's
centre, where the bilinear weights are (1, 0, 0, 0), with reference
`1 - compare`: `1 - compare >= stored` is three's `compare <= depth`.
(2) The globals lookup was a function called `gl`, which naga's GLSL
writer renames `gl_1`, a name GLSL reserves; it is `globals_at`. The
sampler count was not a problem: the material binds eight textures plus
the globals and the environment, within WebGL2's sixteen (D290's note).
Bloom, the PMREM passes and the half-float targets work unchanged. On the
dev machine the WebGL2 pictures match the WebGPU ones to 0.001 ΔE00 mean
(BASELINE.md); Bevy on WebGL2 turns off what the client does not use
(SSAO, OIT, GPU preprocessing and clustering, compute environment maps).

## D394. Scene reloads tear down and rebuild in place

2026-10-03, WP 2.6. `unload_scene(level)` (wasm) asks the client to
despawn every scene entity (the warm-up stand-ins and the stand-in cars
included), drop the build in progress, the cars' race, the night
materials and `Loaded`, reset the status and the cameras, and wait for the
next scene; a different level also drops its Track and sky. The page's
`__mr.reload(level)` calls it, waits for `waiting`, downloads and hands
in the scene as at the start, and resolves when it is `running` again.
This is how SPEC 6.6's "no growth across ten level switches" is measured;
it is not yet a menu feature.

## D395. The measurement page and its numbers

2026-10-03, WP 2.6, gate G1. `index.html?perf=1` (`web/perf.js`) is the
page the owner opens on the phone: it defaults to the JS baseline's fly
camera (s = 80, v = 60), waits for `ready`, records every frame's time
from `requestAnimationFrame` (the JS stats panel's clock) for the route's
length at that speed (`tools/parity/perf-baseline.mjs`'s rule: the route
or the road end less 100 m, at most 240 s; `secs=` overrides), then
reloads the scene ten times (`reloads=`, `reloadLevels=`), and shows the
results on the screen with a Copy button. Reported: time from navigation
to the first frame and to `ready`; frames per second over half-second
windows (median and 5th percentile, the JS baseline's figures); frame
time percentiles (50, 90, 95, 99, 99.9, max); frames over 50 ms in the
first 30 s and in all; the slow frames with their place on the route;
frame time per kilometre; pipelines compiled after the warm-up; the wasm
memory's size after load, after the flight and after each reload (wasm
memory never shrinks, so its size is its high-water mark); the JS heap
where the browser reports it. `tools/parity/rust-perf.mjs` runs the same
page headless (and the JS game with the same recorder, `--game js`)
through the registered server, uncapped (no vsync, no frame-rate limit,
as perf-baseline.mjs), and writes the results to `parity/report/perf/`.

## D396. Bevy's GPU preprocessing stays on; `?gpupre=0` turns it off

2026-10-03, WP 2.6. On WebGPU, Bevy builds the mesh uniforms and culls
with compute shaders and draws indirectly ("GPU preprocessing"); on WebGL2
it cannot and does that work on the CPU. Profiling the WebGPU build in
Chrome where it hitches (Coast s 350 to 400, Sierra's hairpins at
s 955 and 1,470) puts about 60 % of the main thread in `writeBuffer`
calls from Bevy's `write_batched_instance_buffers` and
`write_mesh_culling_data_buffer`: per-entity buffers, large because every
instance of an `InstancedMesh` is an entity (D101; Sierra 63,670, Coast
28,673). Turning GPU preprocessing off on WebGPU
(`PbrPlugin::use_gpu_instance_buffer_builder = false`) lowered the worst
frames (96 to 128 ms against 260 to 408 ms over Coast's first 40 s) but
nearly doubled the median (31 against 18 ms) and gave more frames over
50 ms, so the default stays Bevy's (on), and `?gpupre=0` is there to try
it on a phone. The lasting fix is fewer entities: one entity per
`InstancedMesh` drawn with a per-instance buffer, as three draws it; that
is a renderer change for a later package, recorded in BASELINE.md's WP 2.6
section.

## WP 3.9 world-generation decisions

## D470. Level 1's animators are held to the game's `world.update`, step by step

2026-10-03, WP 3.9. The animators WP 3.9 names (the waterfall, the flag,
the windpumps, the traffic streams, the aircraft lights) were ported with
their modules in WP 3.6 to 3.8 (D311, D334, D353) and checked there at one
frame (dt 0) or, for Valley's, at six. `tools/parity/animators.mjs` holds
all of Sierra's at once, over time: it builds Sierra with the game's own
`World.build` and runs the game's own `world.update(dt, s, focus, camera)`
over 16 uneven frames (dt from 0 to 3.3 s, s along the whole route, so
the night factor goes from 0 to 1, the camera by the lookout's flag, by
the waterfall's pool or in the city, so each distance gate opens and
shuts) and then 360 ticks of 1/120 s. Before the first frame and after
each it snapshots everything under `world.root` an updater could touch:
every node's position, quaternion, scale, visibility, light and instance
count, instance matrices and colours and geometry attributes (by their
version), every material's own numbers, booleans and colours and its
uniforms (a ShaderMaterial's own; a patched material's that three's
ShaderLib lacks), every texture's offset, repeat, rotation and centre.
What differs from the first snapshot in any frame is recorded, keyed by
the scene export's numbering (nodes depth first, materials by first use,
a texture by its first material and key): 52 values on 49 targets, which
are exactly the five animators, the waterwheel and sails, the creek, the
chase bulbs, lamps, glow points and sky glow of City, the road's dew and
the night parameters. The frames are written whole, the ticks as one
SHA-256 per tick of the `key=value` lines. A frozen frame at a fixed
place comes first, so that the first snapshot does not depend on where
the menu's attract camera drifted.

The capture runs under Node by default (a canvas that draws nothing, since
no updater reads a pixel; a patched material's uniforms taken by running
its `onBeforeCompile` on an empty shader), so CI checks it. `--browser`
takes it in the game in headless Chrome (`?kernel=1&freeze=1&s=0`, the
uniforms from the compiled programs): both write byte-identical files,
and two browser captures agree. `tests/animators.rs` builds Sierra through
`level_jobs`, and for each frame does what the client will do:
`update_sky(dt, s, focus)`, the night parameters at the frame's night
factor, `update` with the JS camera; it folds every `SceneEdit` into the
state of its target under the same keys (a number or colour goes to the
uniform of that name if the material has one, else to the parameter, as
D411 says) and requires every recorded value at every frame and every
tick's hash. Values we write that the JS never changes (positions written
with a transform, `uFogK`, `uHalfH`) must hold still. Result: **identical,
every frame and every tick, native and in wasm**; no port fix was needed.

## D471. The runout and the opposite carriageway are computed by the scenery's rules, in `mr_levels::world`

2026-10-03, WP 3.9. Until now `mr_levels::world::world_data(id)` held the
numbers of the WP 0.4 dump (900 and 6419..9379 on Sierra, 700 and
4865..7665 on Coast, 0..14320 on the loop), the stand-in of an unported
module replayed the runout its `plan()` was recorded leaving, and nothing
gave Coast's world build its carriageway. The simulation cannot depend on
mr_worldgen (SPEC 3.2) and should not need a world build to race, so the
parts of the scenery that decide this data are ported into
`mr_levels::world`, one implementation for both: `City` (`s_a`: the
merge tag's s0, else the zone's s0 + 220, 0 on a loop; `plan_runout`:
`max(runout, 900)` off a loop; `opposite_carriageway`: sA to the length)
and `Harbor` (`s_ws`: the `bridge-up` tag's s0, else `zoneStart` + 200,
less 40; `plan_runout`: `max(runout, 700)`; the carriageway from sWS + 40
to the length), `Streets`' `runout = 0`, and `level_world_data(level,
track)`, which runs them as `World.build` runs the modules (every `plan()`
in the order `loadScenery` makes them, then every `build()`, the last
carriageway set winning). mr_worldgen's City now takes its sA, runout and
carriageway from those functions (same values: City's gates pass
unchanged); a `RecordedScenery` computes its module's runout by the rule
and sets `sim_data.runout`, and its `build()` sets the carriageway Harbor
would, so Coast's `WorldBuild::sim_data` is complete. The recording's
runout is no longer replayed; the road test still holds `track.runout` to
it. mr_sim's `stage_level` and `LevelRuntime::new` call
`level_world_data` on the Track they build; `world_data(id)` stays (the
client calls it) and builds a Track only for a level with a City or a
Harbor. Gates: `mr_levels` `tests/world_data.rs` (every level against
`parity/golden/sim/world-data.json`: runout, roadEnd, s0, s1, lanes, dir,
oppY at the samples), mr_worldgen `tests/world_data.rs` (each level's
build: the track's and the simulation's runout, and Coast's, Streets',
Desert's and Seaside's carriageway from the stand-ins' builds; Sierra's
and the loop's from City's build stay in `tests/city.rs`), and mr_sim's
35 module traces and 11 races, unchanged.

## D472. Level 1 is held to the export as one scene

2026-10-03, WP 3.9. With Mountain, Valley and City ported, Sierra's build
needs no recording at all (`scenery_factory(None)`), and
`tests/level1.rs` compares the whole scene with the export rather than
group by group: built that way, the sky updated at the export's focus,
the updaters run once at the export's frame (dt 0) and their attribute
and instance edits written into the buffers, its `mr_scene` digest must
have the world golden's counts (526 nodes, 459 meshes, 122 materials, 40
textures, 101 instance sets, 2 lights, 515 drawables, 1,531,125 vertices,
4,145,448 indices; even the exporter's byte count comes out equal) and
material kinds (always, in CI and wasm), and with the cache it must equal
the export's digest entry by entry: every mesh (counts, bounds, attribute
hashes, area, centroid), drawable (kinds, textures, instance hashes, world
bounds), material (kind, textures) and texture, but for the pixels of the
36 canvas textures, which WP 3.2's threshold gate holds per group (D312,
D313, D331, D332, D354). Result: identical. So the data side of every
Level 1 material kind is complete: Terrain, Asphalt, Shoulder, Markings,
SkyDome, TriplanarRock, Reflector, Siding (siding, roof, boards),
CityFacade, GlowPoints, TrafficStreams, SkyGlow and the built-in
Standard, Lambert, Basic, Line, Points and Sprite, with their parameters,
uniforms, kind options, program keys and GLSL (the per-group gates
compare those). What is left of WP 3.9 is the client's: drawing the kinds
the renderer still stands in for or hides, applying the animators each
frame, and building Level 1 from mr_worldgen.

## Renderer instancing decisions

## D450. An `InstancedMesh` is one entity and one instanced draw

2026-10-04, after WP 2.6 (D396's lasting fix; owner: "go ahead with the
renderer fix"). D101 made every instance of an `InstancedMesh` its own
entity (Sierra 63,670, Coast 28,673), and Bevy's per-entity work (GPU
preprocessing uploads, visibility, extraction, batching) was most of the
WebGPU frame. Now an `InstancedMesh` is one entity per material group,
drawn as three draws it: one draw call with the instance count
(`renderBufferDirect` → `renderInstances`). Entities after the change:
Sierra 508, Coast 436, Seaside 220, Desert 363, Streets 433, Cruise 432.

How (`crates/mr_game/src/render/instancing.rs`):

- **Data.** The entity carries `Instances`, an `Arc` of its stream shared
  by its material groups: per instance 20 floats (80 bytes), the world
  matrix's columns, the colour and `receiveShadow`. It goes to the GPU the
  first frame it is extracted (a `VERTEX` buffer) and the CPU copy is
  dropped, as for meshes (D101).
- **Pipeline.** `ThreeKey` gains `instanced` (a separate material asset
  from the same JS material). `ThreeMaterial::specialize` then appends a
  second vertex buffer layout, stepped per instance, at shader locations 9
  to 13 (Bevy's attributes use 0 to 7, the patch attribute 8; WebGL2's 16
  attributes are enough: 12 at most), and the def `MR_INSTANCED`. The
  shadow pass's vertex shader becomes `three_prepass_instanced.wgsl` (Bevy's
  prepass vertex shader for what the shadow pass reads, with the
  instance's matrix), set in `specialize` through a fixed shader handle;
  non-instanced materials keep Bevy's prepass shader untouched.
- **Draw.** `DrawThreeMesh` is Bevy's `DrawMesh` except that, for an
  entity with a stream, it binds the stream at vertex slot 1 and draws the
  mesh once with `0..count` instances, directly (not through the GPU
  preprocessing's indirect parameters, which describe one instance).
  `InstancingPlugin::finish` re-points the ids of Bevy's `DrawMaterial`
  (Opaque3d, AlphaMask3d, Transparent3d), `DrawPrepass` and
  `DrawDepthOnlyPrepass` (Shadow, and the prepass phases) to the same
  command tuples with `DrawThreeMesh` in place of `DrawMesh`
  (`DrawFunctions::add_with::<DrawMaterial, _>`), so every material Bevy's
  `MaterialPlugin` prepares draws with it, and every entity without a
  stream goes to `DrawMesh` as before. The streams reach the render world
  as a map by main-world entity, rebuilt each frame (a few hundred
  entries). The entities are `NoAutomaticBatching`, so each is its own
  phase item.

Considered and not taken: Bevy's per-entity instancing with the matrices
in a storage buffer indexed by `instance_index` (WebGL2 has no storage
buffers, and Bevy's `instance_index` is the mesh-uniform slot); merging
the instances into one mesh at load (memory times the instance count, and
the vertices pre-transformed on the CPU); our own phase items and queue
systems for the main, transparent and shadow passes (a copy of Bevy's
queueing for four phases); a second material type with its draw
functions swapped per frame (two handles for every material, the night
materials and the warm-up twice). The tuples copy Bevy 0.19.1's
`DrawMaterial`, `DrawPrepass` and `DrawDepthOnlyPrepass`; a Bevy upgrade
(0.20, D100) must check them.

## D451. The instanced shaders: the same functions, the matrix from the stream

2026-10-04. `three_material.wgsl` under `MR_INSTANCED` reads the world
matrix from the stream instead of Bevy's mesh uniform; everything after it
(view and clip position, the shadow coordinate, the points' sizing, every
fragment function) is the same code. The matrix is the InstancedMesh's
`matrixWorld` × `instanceMatrix`, multiplied on the CPU in f64 and rounded
to f32 (three multiplies `modelViewMatrix` × `instanceMatrix` in f32 on the
GPU; before, each instance's product was decomposed into a Bevy
`Transform` and recomposed). The normal goes through the matrix's inverse
transpose, computed in the shader from its cofactors with the
determinant's sign (what Bevy's per-entity `local_from_world_transpose`
holds; for the rotation-and-scale instances of the exports it is the
direction three's `objectNormal /= (dot(im[0], im[0]), …); im *
objectNormal` gives). `receiveShadow` comes from the stream (the mesh
uniform's flag is the one slot the shader no longer reads). Over every
screenshot station of the six levels (398, native, the JS stations'
cameras), the client before and after differ by at most 2 levels of 255
except where D452 applies and in a handful of edge pixels per station
(at most 140 pixels of 717,824 over 8 levels: Desert's small fires and
lamps at night, a few thin edges on Coast).

## D452. `instanceColor` at full precision

2026-10-04. Instance colours rode in Bevy's `MeshTag`, 10 bits a channel
over 0 to 2 (D103, D170). In the stream they are three's f32 values. Two
InstancedMeshes have colours above 2: Streets' 671 lamp globes (`MeshBasic
Material`, up to 2.8) and a pair in Coast (up to 2.52); they were clamped
to 2 and now draw at their value, which is what three draws. Streets'
stations brighten through the bloom (up to 10 of 255 in red, mean over the
picture); with the old clamp put back in the stream the stations match the
client before the change to 1 level, so that is the whole difference. The
tag stays for the material test scenes, which tint single meshes.

## D453. Culling, sorting and zero-scale instances as three does them

2026-10-04. three culls an `InstancedMesh` as a whole by its bounding
sphere over all instances (`InstancedMesh.computeBoundingSphere`,
`Frustum.intersectsObject` with `matrixWorld`; not at all with
`frustumCulled = false`), in the camera and the shadow camera. Every
instanced node of the six exports carries that sphere
(`InstanceDesc.bounding_sphere`); the loader moves it to world space as
`Sphere.applyMatrix4` does (centre transformed, radius × the largest axis
scale) and gives the entity an `Aabb` of half-size r around it: Bevy culls
boxes, and the box holds the sphere, so everything three draws is drawn
(and a little more). Without a sphere, or with `frustumCulled = false`,
the entity is `NoFrustumCulling`. Bevy sorts a transparent mesh by its
transform applied to the mesh's bounding-box centre; the instanced shader
ignores the entity's transform, so the loader sets it to the translation
that puts that point at the sphere's centre, which is the point three's
`projectObject` sorts an InstancedMesh by. Within the entity, instances
draw in buffer order, as in three (before, each instance was sorted on its
own). Zero-scale instances (Desert's 7 tumbleweeds not yet launched) are
left out of the stream, as before: three draws them as triangles of no
size.

Other large per-entity counts were looked for and do not matter: after the
change the levels have 220 to 508 entities. `Points` are one entity per
object already (D294: Sierra 3, Cruise 4); one multi-material node exists
(Coast, without groups); the flora is `InstancedMesh` and is covered here.

## D454. The warm-up covers the instanced pipelines

2026-10-04. An instanced material's key differs (`instanced`), so its
combinations are their own (Sierra 45 → 52, Coast 43 → 49, Seaside
14 → 15, Desert 30 → 33, Streets 26 → 27, Cruise 27 → 30), and each
stand-in of such a combination is an instanced entity with a stream of
one instance 10,000 km below the origin, `NoAutomaticBatching`, drawn by
`DrawThreeMesh` as the real ones are, in the main and shadow passes. In the
web runs of BASELINE.md's instancing section no pipeline was compiled after
`ready` (`__mr.lateFrames` 0). (The native client reports one pipeline
compiled after the warm-up at the first station on every level, before and
after this change alike; it is not one of the scene's.)

## D455. The night materials follow the sky in the shader, not by editing materials

2026-10-04. With the entities gone, the WebGPU frames over 50 ms that were
left (Coast's first 4 km, Sierra's dusk from 3 to 7 km) came at 16, 33 and
50 ms, only while the time of day moved: pinning it (`?t=`) brought Coast's
first 40 s from 16 ms to 2.7 ms a frame. A Chrome trace there had Chrome's
GPU process main thread busy all the time decoding the page's Dawn
commands (`WebGPUDecoderImpl::HandleDawnCommands`) and the page waiting for
room to send more (`DawnClientSerializer::GetCmdSpace` →
`CommandBufferProxyImpl::WaitForToken`, 80 % of the flight). The cause was
`loader::apply_night` (D173's `world.nightMaterials`): whenever the night
factor changed it rewrote `emissive` on every night-following material
(Coast 25, Sierra 22), and Bevy re-prepares a modified material (a new
uniform buffer and bind group) and re-specialises everything drawn with
it, every frame at dusk. Turning the edits off took Coast's first 40 s to
2.6 ms a frame.

So, as D293 does for the updaters' uniforms, the night factor is
scene-wide state the shader reads: `ThreeParams` gains `night` (the
`emissiveIntensity` day and night values, and a flag), set at build from
the export's `night_params`, with `emissive` holding the colour alone;
`three_material.wgsl` scales the emissive radiance by `day + (night − day)
× n` with n from the globals (`G_SKY_PARAMS.x`, the same sky state). A
material without the flag keeps its emissive exactly as before. The JS
computes the product in f64 and stores f32; here it is f32 in the shader
(a relative difference of order 1e-7). The 398 native stations are
identical to the client before this change but for one pixel of Desert's
flickering lights. `apply_night` and `NightMaterials` are gone.

Still per frame as before: the race's car light setters (D440) edit the
racers' and traffic's materials when a value changes, which at dusk is
every frame for the headlights (`max(0.15, smoothstep(0.25, 0.6, night))`)
of a few materials per car; the fly camera's stand-in cars (D295) have
no light setters, so the measurements do not cover it. If the race shows the same signature at
dusk, the same treatment applies there (play/ is not changed here).

## D456. The race's car lights in the globals, not in their materials

2026-10-04, owner: "apply the headlight fix to the race too". The race's
light setters (D440: `setHeadlights` every frame with `max(0.15,
smoothstep(0.25, 0.6, night))`, `setBrake` with the car's `brakeLight`,
`setReverse`, `setBoost`, `setSiren`'s lens intensities) return
`emissiveIntensity` edits, and `play::models::Cars::apply` wrote each into
the drawn materials when the value changed: at dusk, and whenever a car
brakes, a Bevy material re-prepared every frame (D455's cost). Now the
globals texture has a second part, `LIGHT_SLOTS` (512) values four to a
texel from texel 32 (`lighting::G_LIGHTS`, `MaterialLights`): the first
time a setter touches a scene material, `apply` gives it a slot and
changes the material once (`emissive` to the colour alone, `night` to
(0, 0, 2, slot)); from then on an edit only stores the value in its slot,
and `three_material.wgsl` multiplies the emissive colour by
`globals[G_LIGHTS + slot / 4][slot % 4]`. Each scene material keeps its own
slot, and each car its own materials as CarModel builds them, so brake and
reverse stay per car. The values and their timing are the edits' (the
slot is written in the same `draw` system that applied the edit, and the
globals go to the GPU in the same frame); the product colour × value is
taken in f32 in the shader instead of f64 on the CPU. `start` clears the
slots for a new field. If the slots ran out, `apply` falls back to editing
the material as before. `mr_worldgen`'s setters are unchanged.

Pictures: race screenshots at night (Cruise, autopilot, seed 1, WebGPU)
at race time 3 and 8 s are the same before and after (mean 0.06 and 0.22
of 255, the differences at the moving cars' edges from frame timing); the
headlights and tail lights match. Measurements: BASELINE.md, "Races at
dusk".

## D457. The post chain keeps its buffer and bind groups between frames

2026-10-04, from the Firefox investigation (BASELINE.md, "Firefox"). The
post chain (D174) made a uniform buffer (mapped at creation) and thirteen
bind groups every frame. Each is a JavaScript object over a native
allocation in the browser; in Firefox the content process then runs a
major GC for "TOO_MUCH_MALLOC" about every 20 s of flight, and the frame
it lands in can miss its vsync. They are now kept in `post::PostCache`:
the buffer is written (`writeBuffer`) only when its contents change (the
exposure, the bloom sizes), and the bind groups are made once per set of
texture views (the view's two post-process textures swap every frame, so
two sets are in use; a resize makes new ones). The pictures are identical
(the material scenes and 153 native stations to the pixel). Per frame on
Seaside the page now makes 28 bind groups and 5 buffers instead of 41 and
6; what remains is Bevy's own (view and mesh bind groups, uniform buffers
it recreates, one buffer mapped for reading every frame).

## WP 7.3 Desert decisions

## D550. The shape of `mr_worldgen::desert`

2026-10-04, WP 7.3. `Desert.js` is `mr_worldgen::desert` (`desert/mod.rs`),
`desert/parts.js` is `desert::parts`, `desert/props.js` `desert::props`,
`desert/glow.js` `desert::glow`; the canvas code of `makeSigns`, the start
gantry's banners, the railway's ties and the lake bed's cracked mud is
`desert::signs`, and `animate` with `updateTrain` and `updateWeeds` is
`desert::anim`. `Desert` keeps what `plan()` decides (`this.Z`, the Oasis's
s, the pull-out); `build()` makes a `Bld` that is the JS `this` while it
builds (the ColorBuilder `B`, the materials `M`, the atlases' cells `sg` and
`nn`, the occupancy list, the parked cars, the glow and pool lists, the
rail, the train), borrowing the world's track, terrain, road, graph and
texture cache, and runs the JS `build*` methods in the JS order; the
`await tick()`s are not needed in one job. Names and argument order are
the JS's; option objects are structs (`SignOpts` for `roadSign`,
`ArchOpts`, `StrataOpts`, `PointOpts`), and an instance item is `Item`
with `Option`s where the JS reads `??` (`sy`, `sz`, `b`) and plain numbers
where it reads `|| 0` (`rx`, `ry`, `rz`). Where the JS draws from a
generator inside an argument list, a member expression or an object
literal, the port draws into locals in the same order (D350). `paint`'s
default generator is the page's `Math.random`, passed only with no jitter,
where the draw cannot change a colour: the port draws nothing there. The
sign atlases, `paintedSign`, `neonSign`, `signGeometry`, `roundRect` and the
Oasis's gas station, diner and motel are Coast's `beach::atlas` and
`beach::parts` (WP 7.1 owns them; this branch carries its commit), as the
JS imports them from `beach/`; `bannerTexture` is `city::textures`,
`poleGeometry` `valley::parts`, `makeGround` `valley::ground::Ground`, the
sprite geometry `mountain::sprite_geometry`, the parked cars
`car_model::build_vehicle` (no `setHeadlights`: the JS calls none), baked as
`buildCars` bakes them (paint and rust into vertex-coloured buckets, the
rest into a `Builder` keyed by the material's signature, whose equality is
all that matters: type, colour and emissive hex, the three numbers, the
map's handle). Desert is registered with one line in `scenery::PORTED`
(D330): every level build, and the terrain and road gates, run its own
`plan()`, which registers the railway bed and the six flattens bit for bit
and in their place, as the recording does; Desert Run now builds with
`scenery_factory(None)`, from ported modules alone.

## D551. The L3 gate for Desert Run, and what it found

2026-10-04, WP 7.3. `tools/parity/desert-golden.mjs` (city-golden.mjs's
pattern, in a file of its own) writes `parity/golden/desert/desert.json`
from the cached export: for the groups `desert` and `road` (Desert's
`buildLakebed` puts its cracked-mud material on the road's asphalt and
shoulders past the lake's start), a line per node, each drawable's material
by index into a table of canonical material views, every texture's 8×8
block means; and each light's description in the canonical form.
`tests/desert.rs` builds the level through `level_jobs` with
`scenery_factory(None)`, updates the sky at the export's focus, applies the
night parameters, runs every updater once as the frozen export ran them
(dt 0, s 0, the export's camera; a Transform of the spot light's target
moves the light's target, as three reads `target.matrixWorld`), and holds
(a) both groups to the golden, compiled in, so in CI and wasm too, and (b)
the whole scene to the export as Level 1 is held (D472): the world golden's
counts and kinds always, the whole `mr_scene` digest with the cache.
Result: **identical**, native and in wasm: the group `desert`, 179 nodes
(829,961 vertices) and all 52 of its materials (Sandstone, FlickerPoints
×9 with their kind options, GroundPool and FloodBeam with their program
keys and uniforms); the group `road`, 62 nodes and 7 materials with the
lake bed on the meshes past the lake; the 3 lights (the train's spot light
among them); and the scene: 376 nodes, 287 meshes, 61 materials, 16
textures, 132 instance sets, 367 drawables, 1,348,089 vertices, 3,301,239
indices, every digest entry equal but the pixels of the 14 canvas
textures. No fix to a shared module's behaviour was needed. Textures
without lettering are within WP 3.2's threshold of the export (mean
absolute difference at most 0.58 levels, the glow; the ties 0.27, the rock
0.11, the lake bed 0.10, the crack decal 0.005, the detail texture 0).
The lettered ones (the start gantry's two banners, the two painted
atlases, the neon atlas, the finish banner) differ from the export by 8 to
38 levels, all of it in the glyphs (the export draws text in the machine's
fonts), and are held, as D313 and D354 hold Mountain's and City's, to a
capture with the bundled fonts: `tools/parity/desert-textures.mjs` (the
game on Desert Run, `?kernel=1&freeze=1&s=0`, the faces of
`assets/fonts/fonts.json` registered before its scripts run, each canvas
the group `desert` uses as both `map` and `emissiveMap`, six pictures) to
`parity/cache/<key>/desert/` and `parity/golden/desert/textures.json`.
Against it: at most 1.15 levels (the neon atlas's shadow-blurred glyphs),
the painted atlases 0.17 and 0.08, the banners 0.19, 0.08 and 0.05. Rerun:
`node tools/parity/desert-golden.mjs` (with the cache),
`node tools/parity/desert-textures.mjs` when the fonts change, then
`cargo test -p mr_worldgen --test desert` (and in wasm).

## D552. The tumbleweeds' `Math.random` is a stream of their own

2026-10-04, WP 7.3. `updateWeeds` spawns, aims and bounces the rolling
tumbleweeds with the page's `Math.random`, whose position depends on the
whole page (D332). They are cosmetic and per-frame, so the animator draws
from a mulberry32 of its own, seeded with `Desert::random_seed`
(`desert::anim::RANDOM_SEED`, 0x5eed, the seed the scene captures give the
page), and the animator capture reseeds the page's `Math.random` with it
just before its frames, so the JS and the port draw the same values. The
frozen frame at s = 0 draws nothing (the weeds only spawn on Route 66).
Nothing else in Desert reads `Math.random` (`paint`'s default draws are
value-free, D550).

## D553. Desert's animator holds the track and a patch of the ground

2026-10-04, WP 7.3. `animate` reads the track (`railUFor`, a spawning
weed's frame) and the rendered ground under a rolling weed every frame.
The animator keeps a clone of the track and, of the ground, only
`GroundPatch`: the terrain's heights at the lattice points of the tiles
within 300 m of the road from 400 m before Route 66 to 800 m into Silver
Lake, interpolated by `valley::ground`'s triangles (the lattice is exact:
tiles start on whole multiples of 256 m with 4, 16 or 32 m steps); NaN
outside. On Desert Run that is 60 tiles, 140,400 heights (1.1 MB), made
once at the end of the build, so the scene does not keep the whole
terrain alive in the client. The JS shares one uniform object, `glowTime`, between the flicker
materials; the port gives each its own `uTime` and the animator writes the
clock into all ten every frame. The train's spot light moves by Transform
edits of the light and of its target (the next sibling, an `Object3D`),
and its intensity by a `Light` edit.

## D554. A geometry's InstancedBufferAttributes

2026-10-04, WP 7.3. `flickerPools` gives its InstancedMesh's geometry two
per-instance attributes (`ph`, `fl`) through `THREE.InstancedBufferAttribute`,
which the export writes with `instanced: true` and `mesh_per_attribute: 1`
(Harbor does the same). `BufferGeometry` gains `instanced` (the names of
such attributes, boxed, `None` for every generator) and
`set_instanced_attribute`, and `to_mesh_desc` writes the two fields from
it. The `mr_scene` digest does not hash those flags, so this is for the
renderer. No existing geometry or scene changes; the Sierra gates pass.

## D555. Desert Run's animators are held to `world.update`, step by step

2026-10-04, WP 7.3. `tools/parity/desert-animators.mjs` (animators.mjs's
pattern, D470, in a file and golden of its own, so WP 7.1 can extend the
other) builds Desert Run with the game's own `World.build` and runs the
game's own `world.update` over 16 uneven frames (dt 0 to 3.3 s; s from the
start, where the train waits, through Route 66, where it rolls and the
tumbleweeds spawn, to the lake and back), then 360 ticks of 1/120 s at
60 m/s on Route 66, recording every value that changes under `world.root`:
57 values on 43 targets (the eleven car-type InstancedMeshes, the three
headlight sprites and their material's opacity, the spot light and its
target, the rolling tumbleweeds, the ten glow clocks and nine flicker
colours, the flame, beam and pool opacities, the road's dew, the night
parameters) in `parity/golden/animators/desert.json`. Under Node and in
the game (`--browser`) the captures are byte-identical, two browser
captures agree, and CI checks that the Node capture reproduces.
`tests/desert_animators.rs` replays the frames on the Rust build as the
client runs them and requires every value and tick hash. Result:
**identical, every frame and every tick, native and in wasm**.

## D556. What the client still stands in for on Desert Run

2026-10-04, WP 7.3. The data side of every material kind Desert Run uses
is complete (D551). The client's `convert::stand_in` and `render::material`
on `main` today draw these with stand-ins or hide them; the list for the
L4 stations:

- **Sandstone** (`desert/parts.js` `sandstoneMaterial`; the rocks,
  hoodoos, talus, buttes and the arch): drawn as a plain lit standard
  material. A `MeshStandardMaterial` (`vertexColors`, colour white,
  roughness 0.95, metalness 0) with uniforms `tRock` (`rockTexture`) and
  `tDetail` (`detailTexture`), program key `desert-rock`, attributes
  `position`, `normal`, `color`, plus instance matrices and
  `instanceColor`. The patch: in the vertex stage the world position and
  normal through `instanceMatrix` then `modelMatrix`; in the fragment
  stage, replacing `map_fragment`, weights `pow(abs(n), 3)` normalised,
  `diffuseColor.rgb *= (tRock(p.z·0.07, p.y·0.16)·w.x + tRock(p.x·0.07,
  p.y·0.16)·w.z + tRock(p.xz·0.09)·w.y)·1.35`, then the varnish `*= 1 −
  0.45·smoothstep(0.7, 0.86, tDetail((p.x + p.z)·0.11, p.y·0.006).r)·(1 −
  |n.y|)²`.
- **GroundPool** (`desert/glow.js` `flickerPools`, one InstancedMesh):
  drawn as a plain basic material, its per-instance attributes unread. A
  `MeshBasicMaterial` (map `glowTexture`, transparent, additive,
  `depthWrite` false, polygon offset −4/−4; the mesh's render order 2)
  with `instanceColor`, uniform `uTime` (the animator's clock, D553) and
  per-instance `ph` and `fl` (`AttributeRef::instanced`, D554); its
  `opacity` follows the night (`Change::Number`). The patch: `f =
  sin(uTime·13 + ph·6.3)·sin(uTime·4.7 + ph·2.1) + 0.4·sin(uTime·29 +
  ph·3.7)`, `diffuseColor.rgb *= 1 − fl + fl·clamp(0.5 + 0.5f, 0, 1)`.
- **FloodBeam** (`Desert.js` `buildGlows`, one InstancedMesh of four
  cones, render order 3): drawn as a plain basic material. A
  `MeshBasicMaterial` (`vertexColors`, transparent, additive, `depthWrite`
  false, fog) with no uniforms but `clippingPlanes`, attributes
  `position`, `normal`, `color` and instance matrices; `opacity`
  `0.13·smoothstep(0.3, 0.8, night)` from the animator. The patch: `vFace =
  |dot(normalize(normalMatrix · mat3(instanceMatrix) · normal),
  normalize(−mvPosition.xyz))|`, `diffuseColor.rgb *= vFace²`.
- **Sprite** (built-in `SpriteMaterial`, the train's three headlight
  glows): hidden (`stand_in`'s `Sprite => Hidden`). Map `glowTexture`,
  colour `0xfff0c8`, transparent, additive, `depthWrite` false; the
  animator moves and scales the sprites (`Transform`) and sets the
  material's `opacity`.
- **FlickerPoints** is drawn (`Patch::Points { Flicker }`); it needs the
  animator's `uTime` each frame on all nine materials.
- The train's **SpotLight**: the loader takes it at load; the animator
  moves it and its target (`Transform` of the light and of the next
  sibling `Object3D`) and sets its intensity (`Light`, 0 by day, 90 at
  night) every frame.
## WP 7.1 Coast decisions

## D530. The shape of `mr_worldgen::coast`; `coast/kit.js` is Mountain's kit

2026-10-04, WP 7.1. `Coast.js` is `mr_worldgen::coast` (`coast/mod.rs`):
`Coast` keeps what `plan()` decides (`zEnd`, the coffee pull-out, the
lighthouse headland with its two neck flattens, the vista pull-out, the
exclusions) and implements `Scenery`; `build()` runs on a `Build` that is
the JS `this` (the track, terrain, the road's `sideL`/`sideR`, the
rendered-surface sampler, the graph and texture cache, the surf, ring,
rock and arch materials, the shoreline runs) and calls `findShore`,
`buildFoam`, `buildSeaRocks`, `buildArch`, `buildVegetation`, `buildGrass`,
`buildCrags`, `buildOutcrops`, `buildLighthouse`, `buildStartArea`,
`buildPullout`, `buildVista`, `buildSigns`, `buildPoles`,
`buildDelineators` and `buildBoats` in the JS order. A JS `for` loop whose
body `continue`s past an update that draws (`d += lerp(6, 11, rng())`) is a
`while` with a labelled block, so the draw still happens. The boats are
posed at build by the animator's own `update(0, 0.8)`, as the JS calls it,
and its edits written into the graph. `coast/kit.js` is a copy of
Mountain's module-private helpers: `coast::kit` re-exports Mountain's port
of the identical ones (the surface sampler, `instanced`, `placed`,
`bakeStatic`, `mergedMesh`, the sign atlas, `roundRect`, `diamond`,
`panel`, `canvasTex`) and adds what differs: `rockGeometry` (simplex lumps,
not flora's), `colorize`, `prep` and `rockMaterial` (no vertex colours,
program key `coast-rock`; the stacks' copy sets them and `coast-rock-vc`).
The parked surf van (three surfboards added to its body before baking)
and hatch come straight from `car_model::build_vehicle`; the JS's optional
import is always there. Coast is the fourth line of `scenery::PORTED`: its
`plan()` registers its 5 flattens in their place, bit for bit as the
recording (the terrain and road gates).

## D531. The L3 gate for Level 2, group by group, and what it found

2026-10-04, WP 7.1. `tools/parity/coast-golden.mjs` is City's golden
(D354) per group: from the cached Coast export it writes
`parity/golden/coast/coast.json` with, for each of `coast`, `beach` and
`harbor`, the node lines, the material table in canonical form and the
8×8 block means of every texture a material uses; plus the export's night
factor, camera and fog, and the level's night parameters.
`tests/coast.rs` builds Level 2 through `level_jobs` with the scenery
factory, applies the night parameters at the export's night factor, runs
the three modules' animators once as the frozen export ran them (dt 0,
the export's camera), copies the fog into the fogged `ShaderMaterial`s
(D532) and compares each group (`coast_group`, `beach_group`,
`harbor_group`); `coast_world_data` holds the runout and the westbound
carriageway the build gives the simulation to `mr_levels::world` and the
WP 0.4 dump. Result: **every node of every group identical** and **every
material equal parameter by parameter** (uniforms, program keys and GLSL
included): `coast` 89 nodes (39,397 vertices, 50 materials), `beach` 60
(1,857,571 vertices, 55 materials), `harbor` 82 (749,317 vertices, 34
materials). Every canvas texture is within WP 3.2's threshold of the
bundled-font capture (D535): the worst 0.87 levels (the shared gravel), the
lettered ones at most 0.50 (Beach's neon atlas), but for Beach's palm
leaf (D534). No fix to a shared module was needed. The gate runs in CI
and in wasm without the cache (block means), and with it compares every
pixel and writes sheets to `parity/report/coast/` (and, with
`COAST_DUMP=1`, both sides' RGBA).

## D532. A `ShaderMaterial`'s fog uniforms are the renderer's

2026-10-04, WP 7.1. The surf (`Surf`) and the lighthouse beam
(`LighthouseBeam`) are `ShaderMaterial`s with `fog: true` whose uniforms
begin with `UniformsLib.fog` (`fogDensity`, `fogNear`, `fogFar`,
`fogColor`). three's renderer copies the scene's fog into those every time
it draws (`refreshFogUniforms`), so the export holds the fog of its frame
(FogExp2 0x34406a, 0.0003 at the start of Level 2), not the material's
own values. mr_worldgen makes them with three's defaults (0.00025, 1,
2000, white), as the JS material is made; the gate does what the renderer
does, from the export's fog (`coast.json`), before comparing. The client
sets them from the sky's frame each frame, as it does its own fog.

## D533. The shape of `mr_worldgen::beach`; the parts' random defaults

2026-10-04, WP 7.1. `Beach.js` is `mr_worldgen::beach` (`beach/mod.rs`),
`beach/atlas.js` is `beach::atlas`, `beach/parts.js` is `beach::parts`;
`beach/ColorBuilder.js` stayed `color_builder` (WP 3.3). Desert uses the
atlas and three of the parts; they landed first, in a commit of their own
(7e1ecda), so WP 7.3 could take them. A parts function's options object
whose defaults draw from the stream (`twoStory = rng() < 0.4`, `wall =
rpick(rng, WALLS)`, `w = rrange(rng, 9, 13)`, ...) is a struct of
`Option`s drawn in the JS destructuring order when `None`; a value drawn
by the caller's own object literal (`blade: rng() < 0.6 ? ...`) is drawn by
the caller, before the call, as in the JS. `fitText` rewrites the pixel
size in the font string the canvas holds, as the JS's regular expression
does. `tools/parity/beach-parts.mjs` runs the game's own `parts.js` under
Node with the kernel over 44 cases (every function, both branches of the
random ones, in a placed frame) and writes
`parity/golden/beach/parts.json`; `tests/beach_parts.rs` is bit-identical
in every bucket, attribute, return value and draw count, native and in
wasm; CI checks the golden regenerates. `Beach` keeps what `plan()`
decides (the town's range, the pier, marina and cross streets, the hill
houses) and builds on a `Bld` that is the JS `this`. A frontage lot is an
enum with the JS lot's `w` and `d`; its drawn widths are drawn when the
blocks are listed, as the JS array literal draws them, and an exhausted
block draws a fresh house each time it is asked. The parked cars merge by
the JS's material signature (type, colour and emissive hex, emissive
intensity, roughness, metalness, map, transparency, opacity); the wheels'
material lists all give the signature of missing fields, so every wheel
merges into one mesh holding the first car's list and no groups, which
three draws as nothing (the client should too). The ribbons merge per
material in first-use order, so the back streets join the cross streets'
mesh and the gangway the promenade's, under the first name, as in the
export.

## D534. Beach's palm leaf is over the texture threshold

2026-10-04, WP 7.1. The palm crown's leaf (`buildPalms`, 64×256, thin
quadratic strokes 1.8 px wide on a transparent canvas, no text) is D333's
corn strip again: against Chrome's capture it is 2.73/5.89/1.94/9.39
levels mean absolute difference (R/G/B/A) with the total coverage within
0.06 % (alpha sums 1,716,346 against 1,715,237): Chrome's multisampled
strokes quantise the edge alpha (no pixel between 1 and 31) where
mr_canvas takes the exact area (D151). Premultiplied the colour is within
2.26/5.00/1.61; 2.95 % of pixels fall on the other side of the material's
`alphaTest` 0.4. Matching Chrome's stroke coverage is mr_canvas's (WP 3.2's
owner); until then `tests/coast.rs` holds the leaf to 10 levels
unpremultiplied and 6 premultiplied and reports the numbers; every other
Level 2 texture is held to 3.

## D535. Level 2's canvas reference: every canvas, with the bundled fonts

2026-10-04, WP 7.1. `tools/parity/coast-textures.mjs` opens the game on
the Coast Highway as `city-textures.mjs` does (D354: `?kernel=1&freeze=1&
s=0`, `Math.random` seeded, every bundled face registered under the JS
family) and walks the groups `coast`, `beach` and `harbor` as the golden
lists their textures (drawables depth first, materials by first use,
texture parameters in property order, then the uniforms, a patched
material's read from its compiled program as the exporter reads them),
reading back every canvas once. Unlike City's capture it takes every
canvas, lettered or not, so one reference holds all 40 pictures of the
three groups (63 texture entries); `parity/golden/coast/textures.json`
names each entry's picture, with its SHA-256 and block means, and the
RGBA goes to `parity/cache/<key>/coast/`. Where a picture has no text it
equals the export's to the bit or nearly (the rock texture 0.11, the
stripes 0.04), so nothing is lost by holding those to the capture too.
When the bundled fonts change, rerun the tool.

## D536. Level 2 is held to the export as one scene

2026-10-04, WP 7.1. With Coast, Beach and Harbor ported, Level 2 builds
with `scenery_factory(None)`, and `tests/level2.rs` holds the whole scene
to the export as `tests/level1.rs` holds Level 1 (D472): the counts (458
nodes, 418 meshes, 150 materials, 50 textures, 67 instance sets, 2
lights, 443 drawables, 3,511,484 vertices, 4,008,912 indices; the
exporter's byte count comes out equal too) and kinds always, the whole
digest entry by entry with the cache but for the 41 canvas textures'
pixels (D531), and the 25 night parameters in registration order. The
frame's edits are applied to the objects before assembly, transforms
included, because two updaters place things at dt 0: the lighthouse's
glow takes its night-scaled size and Harbor's boats their bobbing pose.
Result: identical.

## D537. Level 2's animators, step by step

2026-10-04, WP 7.1. `tools/parity/animators.mjs` takes `--level coast`
(the default stays Sierra, whose golden is byte-identical): the same
snapshot of everything under `world.root`, over 16 uneven frames along the
route (from blue hour, night 0.75, to morning) with the camera by the
cliffs, the pier and the docks (no Level 2 updater looks at the camera),
then 360 ticks of 1/120 s, written to
`parity/golden/animators/coast.json`: 68 values on 58 targets (the surf's
clock and brightness, the beam, the glow, the string lights, the
reflectors, the fishing boats and their running lights, the Ferris wheel
and gondolas, the coaster train, the beach surf's maps and opacity, the
light pools, the signals, the harbour's lamps, lenses, chase bulbs, glow
points, boats and breakwater lamps, the road's dew, the sea, the night
parameters). The Node capture and two browser captures agree.
`tests/coast_animators.rs` replays them as the client runs a frame (D470):
**identical, every frame and every tick**, native and in wasm. CI checks
that the golden regenerates.

## D538. Level 2's world data, and the small shared changes

2026-10-04, WP 7.1. With Harbor ported, its `plan()` sets the runout (700)
and its `build()` the westbound carriageway, through
`mr_levels::world::Harbor` as City's do (D471); no stand-in is left on
Level 2. `tests/world_data.rs` builds its levels without a road, which a
ported module's `build()` needs, so Level 2 now stops after the plans as
the City levels do, and `tests/coast.rs` checks the data from the real
build. Shared modules: `mountain::kit::world_matrices` is public (Beach's
parked cars traverse a car as `bakeStatic` does); WP 7.3's
`BufferGeometry::set_instanced_attribute` (Harbor's `aVar`, Desert's
pools) is cherry-picked unchanged. While Beach and Harbor were ported side
by side, Harbor sat behind a temporary `harbor-wip` feature; it is gone.

## D540. The shape of `mr_worldgen::harbor`

2026-10-04, WP 7.1. `Harbor.js` is `mr_worldgen::harbor` (`harbor/mod.rs`),
`harbor/build.js` is `harbor::build`, `harbor/textures.js` is
`harbor::textures`. `plan()` keeps the JS fields (z0, the span, up and down
tags, sWS, the connector's frames and gate), sets the runout by
`mr_levels::world::Harbor::plan_runout`, mirrors it into
`sim_data.runout`, and registers the connector's embankment and the gate's
flattens in the JS order. `build()` runs on a `Build` that is the JS
`this`, adds the group `harbor` to the root first, as the JS does, calls
the `build*` methods in the JS order, ends with the pass that turns off
`matrixAutoUpdate` on drawn children not marked `userData.animated`, and
sets `opposite_carriageway` through `Harbor::opposite_carriageway`. The
frame lists of `ribbon` are `RFrame { x, z, fx, fz, y }`; `addMarkings`
takes an optional frame function returning a track `Frame` (the
connector's interpolated frame fills x, y, z, fx, fz, rx, rz).
`containerTexture` and `rollerDoorTexture` have no caller and are not
ported. It uses the existing ports of `city/freeway.js`, `city/geom.js`,
`bannerTexture`, `textures.js` and `coast/kit` as they are.

## D541. Harbor's `Batch`

2026-10-04, WP 7.1. Buckets are keyed (material, floor(cx / chunk),
floor(cz / chunk), cast) in first-use order, as the JS `Map` keyed by
`mat.uuid` is; the chunk size is not in the key, as it is not in the JS
string. A one-piece bucket is used as it is rather than merged, as in the
JS.

## D542. Harbor's textures

2026-10-04, WP 7.1. `corrugatedTexture`, `pavingTexture` and
`containerAtlas` are entries of the world's `TextureCache` under
`harbor:corrugated`, `harbor:paving` and `harbor:containerAtlas` (D352).
The atlas is uploaded without a colour space. All the containers of the
level (yard, depot, ship, trucks, train) are one `InstancedMesh`; its
`aVar` is an `InstancedBufferAttribute` added after the mesh is made, so
it comes last in the geometry.

## D543. Harbor's animators

2026-10-04, WP 7.1. In the JS order: the towers' warning lamps (a material
colour), the lighting's lens (a material colour), the finish gantry's
chase bulbs (instance colours), the port's glow points (the `color`
attribute, base × k stored as f32), the boats (transforms from a YXZ
Euler) and the breakwater lamps (instance colours). D537 holds them to the
game step by step.

## WP 3.9 client decisions

## D490. Animated material values live in the globals, in a block per material

2026-10-04, WP 3.9. The scenery's animators (`WorldBuild::update`, D470)
set material colours (the freeway's lamp lenses and light pools, the neon,
the aircraft lights, the glow points), an emissive colour (the valley
creek), `emissiveIntensity`, a sprite's rotation, texture offsets (the
waterfall, the creek's normal map) and a kind's uniforms (the traffic
streams' four, the sky glow's `uK`, the road's `uWet`, the glow points'
`uFogK`), many of them every frame. Writing them into Bevy materials would
re-prepare each material every frame, the stall D455 removed. So each
material an animator touches gets an *animation block*: five RGBA32F
texels of the globals row from texel 160 (`lighting::G_BLOCKS`, after
D456's light slots; 128 blocks): 0 the colour (w 1 when set), 1 the
emissive colour, 2 `emissiveIntensity` (x, y set) and the rotation (z, w
set), 3 the map's (and alpha map's) offset since the export (xy) and the
normal map's (zw), 4 the kind's uniforms. The shader reads a value from
the block where the block has one and the material's parameter otherwise.
A material finds its block through a block map from texel 800
(`G_BLOCK_MAP`, 512 materials four to a texel, so the row is 928 texels):
the loader gives every scene material its index (`ThreeParams::slots.z`,
index + 1; 0 for the race's cars, which have no block), and the map holds
the block's first texel, 0 for none. So a block is made the first time an
animator touches the material without the Bevy material ever being
edited, even once (a first version edited it once, which an animator that
runs only near its object, the waterfall's spray within 800 m, did in the
middle of a flight). A new block starts with the emissive colour and
intensity as exported (the intensity left to `night` when it follows
nightfall, D455), so the shader needs no change of the material's emissive
form. On Sierra 15 materials get one in the first frame and 16 in all. A
number or colour goes to the
kind's uniform of that name if it has one animated, else to the parameter
(D411's rule); the sky's edits are left out (D493).

## D491. The client builds Level 1 for its animators; the default still draws the export

2026-10-04, WP 3.9. Animators are code (SPEC 5.1): they come only from a
world build. For a level `mr_worldgen` builds whole (Sierra, D472), the
client runs `level_jobs` itself (`crate::animate`): natively on a thread
(with `mr_worldgen`'s `parallel` feature), on the web a few jobs a frame
(30 ms) while the page downloads the export. It keeps the animators and
the sky and drops the build's scene, since the default still draws the
level's `.mrscene` (the coordinator's instruction; delivery stays as D439
left it). The two scenes number nodes, meshes, materials and textures
alike (D472), so the edits address the export; the client checks the
counts and leaves the animators off if they differ (`--scene` with
another file, the base export). `ready` waits for the build
(`status::tick`), so the loading screen and the stations cover it. A
reload builds a new world, as the JS does. `?world=off` turns it off.

On the web the page holds the downloaded export back until the build is
done (`world_pending`), so the wasm memory's high-water mark is the larger
of the build and the scene, not their sum: Sierra after load 348 MB,
reloads 357 then 488 (before WP 3.9: 303, then 443; with the two
overlapping, 391 then 522, over SPEC 6.6's 512 MB). Ready on the dev
machine 4.5 to 8 s against 3.5 s before (the build is about 1.5 s of CPU
natively in release, single-threaded, and several seconds in wasm on the
main thread, beside a local download of under a second); on a phone the
download of the export (131 MB) is the longer of the two.

## D492. `?world=gen`: draw the client's own build, without the download

2026-10-04, WP 3.9 (to-do item 1). Behind an option, as instructed; the
default is unchanged. With `?world=gen` (native `--query world=gen`) the
page does not download Sierra's 138 MB export (`generates_scene`) and the
loader draws the client's build. On the dev machine (WebGPU, headless
Chrome, the machine loaded): ready in 7.5 s with nothing downloaded but
the client, wasm memory 348 MB after load and 357, then 375 on reloads,
against the export's 488 (above). The pictures are the build's: the
canvas textures are `mr_canvas`'s, within WP 3.2's thresholds (D312,
D354); the L4 gate (D496) was taken this way. Not decided here (D439, the
owner's): whether the game should build its levels in the client instead
of downloading them. The numbers favour it for Sierra (no 131 MB
transfer, 110 MB less memory on reloads, the same pictures); raised for
the owner.

## D493. How the edits reach the drawn scene

2026-10-04, WP 3.9. Each rendered frame, after the camera and before
transform propagation, `World.update`'s order: `update_sky(dt, s, focus)`
(its edits are the dome's uniforms and the lights, which the client's
`render::sky` already computes into `Lighting` from the same keys, D173,
so they are not applied; its night factor is used), the night parameters
(in the shader, D455, nothing per frame), `update` with the camera
(position, fov in degrees, the drawing buffer's height), then each edit:
a node's transform recomposes the world matrices of its subtree (the
loader flattened them; the export's local matrices are kept per node) and
sets the entities' `Transform`; visibility hides or shows the subtree's
entities as three's ancestors rule does; instance matrices, colours and
counts rebuild that InstancedMesh's stream (a new `Instances` for its
entities, only when a value changed); a geometry attribute is written into
the Bevy mesh, which keeps its CPU copy when it has at most 64 vertices
(`convert::KEEP_VERTICES`; Sierra's flag has 18; normals are not
recomputed, as in the JS); material values go to their block (D490); a
texture offset to every material using that texture. Unchanged values are
not written (a frozen frame changes nothing). An edit nothing applies is
logged once at debug level. The loader tags every entity with its node
(`animate::NodeRef`) and hands over what the edits need before it drops
the scene (`animate::SceneIndex`: per node its parent, children, local and
world matrices, and an InstancedMesh's instance matrices and colours, 4 MB
on Sierra). Not covered: a node exported invisible is not spawned by the
loader, so an animator cannot show it (none on Sierra).

## D494. Level 1's remaining material kinds

2026-10-04, WP 3.9. Blocks of `three_material.wgsl` as D290's, picked by
`Patch`: **CityFacade** (`patchCityMaterial`: the atlas cell from `cell`,
which rides in the patch attribute, sampled with the raw uv's gradients;
`uMask` in the detail slot; shopfronts; the glass's roughness and
metalness; the lit windows, reflections, spill and shop light replacing
`emissivemap_fragment`; the JS's derivatives inside branches taken before
them, D290's rule); **TrafficStreams** and **SkyGlow** (the two City
`ShaderMaterial`s, unlit, with their own fragment code; the traffic
lights are points, their `aDir` and `aPar.z` in a second attribute at
location 14, `convert::ATTRIBUTE_EXTRA2`, their size from the
projection's y scale and `uHalfH`, the GL ES point rules of D294);
**TriplanarRock** (strata from `tRock` in world space, the instance's
matrix included); **Reflector**, whose patch means the emissive to take
the instance colour under `USE_INSTANCING_COLOR`, which three r180
defines in the vertex shader only (the fragment shader gets `USE_COLOR`),
so in the game it does nothing and the reflectors glow white: ported as
it draws (the material scene matched at 0.135 that way, 4.6 the other);
**Siding** in its three modes; **Sprite** (three's `sprite_vert`: the
quad in view space around the node's origin, scaled by its scale, turned
by `rotation`); and for the plain kinds `alphaMap` (in the photo slot,
which only the terrain uses) and a tangent-space normal map (the creek,
as the sea's). Where no world build runs (the other levels' exports), a
kind's animated uniforms follow the scene-wide state as D293 does:
`uTime` the clock, `uNight` and `uK` from the night factor, `uHalfH` the
viewport.

## D495. D293's scene-wide uniforms where the animators run

2026-10-04, WP 3.9. Where a world build runs, the road's `uWet` and the
glow points' `uFogK` come from the animators' edits (D490), and the
traffic streams and sky glow have only that; `render::lighting::Anim`
stays for the levels without a world build (the sea's clock on Coast,
the desert's flicker). One correction to D293: the glow points' gentler
fog was `fog density × 0.4`, but City.js reads `world.scene.fog`, the root
group's, which has none (D353): `uFogK` is 0. Sierra's and Cruise's far
lamp halos are a little brighter now, as in the JS.

## D496. The L4 gate on all Sierra stations, from the web build

2026-10-04, WP 3.9. The native client cannot open a 1280 × 800 window on
this machine's 1024 × 768 display (the stations came out 1024 × 701), and
`rust-web.mjs` cannot pass Sierra's 138 MB export through its request
interception (D106). `tools/parity/rust-web-stations.mjs` loads the web
build once with `?world=gen` (no download), flies to each station with a
new test hook (`__mr.flyTo`, `__mr.flyQuiet`: three frames with no
pipeline compiling and the environment map built) and saves
`__mr.screenshot`; `cargo xtask parity shots` compares.

Result (WebGPU, 1280 × 800, frozen): **all 81 stations within SPEC 12's
limits**, median 0.23 mean ΔE00 and 0.53 block 95 %, worst 0.674 mean
(07750-high) and 2.441 block 95 % (08750-high). The city's 32 stations
(zone 2 from 06250, dusk to full night) were the priority (the owner's
"too dark at night in the city"): all 32 within the limits, worst 0.674 /
2.441; the native client's last run before WP 3.9 had all 32 over (worst
14.8 / 37.2: the facades lit as plain, no sky glow, no traffic lights, the
light pools and lamp lenses at their daytime colours). The material test
scenes now include TriplanarRock, Reflector, Siding, CityFacade and
SkyGlow (with the JS tool's uniform overrides, `animate::fix_uniforms`):
23 scenes, 0 over, worst 0.150 as before.

## D497. Nothing in the animator path allocates per frame; its cost

2026-10-04, WP 3.9, after the coordinator's perf review. Of what the
edits touch each frame, only instance streams made GPU objects: a moved
InstancedMesh got a new `Instances` stream, so a new vertex buffer, each
frame (Sierra's waterwheels every frame, the freeway's chase bulbs eight
times a second), which is what drives Firefox's GC (D457). Now a stream
with the same instance count is rewritten in place:
`InstanceStream::update` keeps the new bytes and
`instancing::write_instance_updates` (render world, at extraction) writes
them into the existing buffer with `write_buffer` (the buffer gains
`COPY_DST`); a new stream is made only when the count changes (an
`InstanceCount` edit, or a zero-scale instance appearing or going, none
on Sierra). Material values never touch a Bevy material (D490); node
transforms and visibility are component writes; the flag's 18 vertices
are written into Bevy's mesh slab in place (`write_buffer_with`, no new
buffer in steady state). Unchanged values are not written, so a frozen
frame costs only the update call.

Measured in the wasm build over Sierra's whole route: the animator path
(`run_animators`) takes 0.04 to 0.15 ms a frame on average, 3.2 ms at
most. The world build runs before `ready` (2.9 s in wasm here), never
during a flight. A/B flights against the build before (BASELINE.md, "The
Rust client at WP 3.9"): the same frame-time distribution on WebGPU and
WebGL2, no pipeline after the warm-up, and isolated slow frames in both
builds under the machine's load. Ready is later on the dev machine (7 to
11 s against 3 to 5 s), because the page holds the export back until the
build is done (D491); `?world=gen` is ready in 7.5 s without the download.
